use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    sync::mpsc::{self, Receiver},
    thread,
};

use anyhow::{Result, anyhow};

use crate::{config::SavedConnection, remote::RemoteSession};

#[derive(Clone, Debug)]
pub enum TransferBackend {
    Local,
    Remote {
        connection: SavedConnection,
        password: String,
    },
}

#[derive(Clone, Debug)]
pub struct CopyJob {
    pub source_backend: TransferBackend,
    pub destination_backend: TransferBackend,
    pub sources: Vec<PathBuf>,
    pub destination_dir: PathBuf,
    pub cancel_flag: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
pub enum TransferEvent {
    Progress {
        copied_bytes: u64,
        total_bytes: u64,
        current_item: String,
    },
    Finished {
        copied_bytes: u64,
        total_bytes: u64,
        processed: usize,
        failed: usize,
        skipped: usize,
        error: Option<String>,
    },
    Cancelled {
        copied_bytes: u64,
        total_bytes: u64,
        processed: usize,
        failed: usize,
        skipped: usize,
    },
}

pub fn spawn_copy_worker(job: CopyJob) -> Receiver<TransferEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Err(error) = run_copy_worker(job, &tx) {
            let _ = tx.send(TransferEvent::Finished {
                copied_bytes: 0,
                total_bytes: 0,
                processed: 0,
                failed: 1,
                skipped: 0,
                error: Some(error.to_string()),
            });
        }
    });
    rx
}

fn run_copy_worker(job: CopyJob, tx: &mpsc::Sender<TransferEvent>) -> Result<()> {
    let mut source_remote: Option<RemoteSession> = None;
    let mut destination_remote: Option<RemoteSession> = None;

    if let TransferBackend::Remote {
        connection,
        password,
    } = &job.source_backend
    {
        source_remote = Some(RemoteSession::connect(connection, password)?);
    }

    if let TransferBackend::Remote {
        connection,
        password,
    } = &job.destination_backend
    {
        let reuse_source = match &job.source_backend {
            TransferBackend::Remote {
                connection: source_conn,
                ..
            } => source_conn.host == connection.host
                && source_conn.port == connection.port
                && source_conn.username == connection.username,
            TransferBackend::Local => false,
        };

        if reuse_source {
            destination_remote = None;
        } else {
            destination_remote = Some(RemoteSession::connect(connection, password)?);
        }
    }

    let mut total_bytes = 0u64;
    for source in &job.sources {
        total_bytes = total_bytes.saturating_add(estimate_source_bytes(
            &job.source_backend,
            source,
            source_remote.as_ref(),
        )?);
    }

    let mut copied_bytes = 0u64;
    let mut processed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    let is_cancelled = || job.cancel_flag.load(Ordering::Relaxed);

    for source in &job.sources {
        if is_cancelled() {
            let _ = tx.send(TransferEvent::Cancelled {
                copied_bytes,
                total_bytes,
                processed,
                failed,
                skipped,
            });
            return Ok(());
        }

        let Some(name) = source.file_name() else {
            skipped += 1;
            continue;
        };

        let destination = job.destination_dir.join(name);
        if path_exists(
            &job.destination_backend,
            &destination,
            source_remote.as_ref(),
            destination_remote.as_ref(),
        )? {
            skipped += 1;
            continue;
        }

        let mut emit = |delta: u64, current: &Path| {
            copied_bytes = copied_bytes.saturating_add(delta);
            let _ = tx.send(TransferEvent::Progress {
                copied_bytes,
                total_bytes,
                current_item: current.display().to_string(),
            });
        };

        let result = match (&job.source_backend, &job.destination_backend) {
            (TransferBackend::Local, TransferBackend::Local) => {
                copy_local_to_local(source, &destination, &mut emit, &is_cancelled)
            }
            (TransferBackend::Local, TransferBackend::Remote { .. }) => {
                let session = destination_remote
                    .as_ref()
                    .or(source_remote.as_ref())
                    .ok_or_else(|| anyhow!("Sesion remota de destino no disponible"))?;
                session.copy_local_to_remote_with_progress(source, &destination, &mut emit, &is_cancelled)
            }
            (TransferBackend::Remote { .. }, TransferBackend::Local) => {
                let session = source_remote
                    .as_ref()
                    .ok_or_else(|| anyhow!("Sesion remota de origen no disponible"))?;
                session.copy_remote_to_local_with_progress(source, &destination, &mut emit, &is_cancelled)
            }
            (TransferBackend::Remote { .. }, TransferBackend::Remote { .. }) => {
                let session = source_remote
                    .as_ref()
                    .ok_or_else(|| anyhow!("Sesion remota no disponible"))?;
                session.copy_remote_to_remote_with_progress(source, &destination, &mut emit, &is_cancelled)
            }
        };

        match result {
            Ok(()) => processed += 1,
            Err(error) if error.to_string().contains(crate::remote::TRANSFER_CANCELLED_MARKER) => {
                let _ = tx.send(TransferEvent::Cancelled {
                    copied_bytes,
                    total_bytes,
                    processed,
                    failed,
                    skipped,
                });
                return Ok(());
            }
            Err(_) => failed += 1,
        }
    }

    let _ = tx.send(TransferEvent::Finished {
        copied_bytes,
        total_bytes,
        processed,
        failed,
        skipped,
        error: None,
    });

    Ok(())
}

fn estimate_source_bytes(
    backend: &TransferBackend,
    source: &Path,
    source_remote: Option<&RemoteSession>,
) -> Result<u64> {
    match backend {
        TransferBackend::Local => estimate_local_bytes(source),
        TransferBackend::Remote { .. } => source_remote
            .ok_or_else(|| anyhow!("Sesion remota de origen no disponible"))?
            .estimate_remote_bytes(source),
    }
}

fn path_exists(
    backend: &TransferBackend,
    path: &Path,
    source_remote: Option<&RemoteSession>,
    destination_remote: Option<&RemoteSession>,
) -> Result<bool> {
    match backend {
        TransferBackend::Local => Ok(path.exists()),
        TransferBackend::Remote { .. } => {
            if let Some(session) = destination_remote {
                return Ok(session.exists(path));
            }
            if let Some(session) = source_remote {
                return Ok(session.exists(path));
            }
            Err(anyhow!("Sesion remota no disponible"))
        }
    }
}

fn estimate_local_bytes(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }

    if metadata.is_dir() {
        let mut total = 0u64;
        for child in fs::read_dir(path)? {
            total = total.saturating_add(estimate_local_bytes(&child?.path())?);
        }
        return Ok(total);
    }

    Ok(0)
}

fn copy_local_to_local<F, C>(
    source: &Path,
    destination: &Path,
    on_progress: &mut F,
    should_cancel: &C,
) -> Result<()>
where
    F: FnMut(u64, &Path),
    C: Fn() -> bool,
{
    if should_cancel() {
        return Err(anyhow!(crate::remote::TRANSFER_CANCELLED_MARKER));
    }

    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for child in fs::read_dir(source)? {
            let child = child?;
            let child_source = child.path();
            let child_destination = destination.join(child.file_name());
            copy_local_to_local(&child_source, &child_destination, on_progress, should_cancel)?;
        }
        return Ok(());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut src = fs::File::open(source)?;
    let mut dst = fs::File::create(destination)?;
    let mut buffer = vec![0u8; 4 * 1024 * 1024];

    loop {
        if should_cancel() {
            return Err(anyhow!(crate::remote::TRANSFER_CANCELLED_MARKER));
        }
        let read = std::io::Read::read(&mut src, &mut buffer)?;
        if read == 0 {
            break;
        }
        std::io::Write::write_all(&mut dst, &buffer[..read])?;
        on_progress(read as u64, source);
    }
    std::io::Write::flush(&mut dst).ok();
    Ok(())
}
