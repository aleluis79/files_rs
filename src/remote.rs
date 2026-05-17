use std::{
    cmp::Ordering,
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};

use crate::{
    app::{FileEntry, SortMode, SortOrder},
    config::SavedConnection,
};

const MODE_TYPE_MASK: u32 = 0o170000;
const MODE_DIR: u32 = 0o040000;
pub const TRANSFER_CANCELLED_MARKER: &str = "__TRANSFER_CANCELLED__";

pub struct RemoteSession {
    pub connection: SavedConnection,
    password: String,
    _session: ssh2::Session,
    sftp: ssh2::Sftp,
    pub home_dir: PathBuf,
}

impl RemoteSession {
    pub fn connect(connection: &SavedConnection, password: &str) -> Result<Self> {
        let addr = format!("{}:{}", connection.host, connection.port);
        let tcp = TcpStream::connect(&addr).with_context(|| format!("No se pudo conectar a {addr}"))?;
        tcp.set_read_timeout(Some(Duration::from_secs(20))).ok();
        tcp.set_write_timeout(Some(Duration::from_secs(20))).ok();

        let mut session = ssh2::Session::new().context("No se pudo crear sesion SSH")?;
        session.set_tcp_stream(tcp);
        session.handshake().context("Fallo handshake SSH")?;
        session
            .userauth_password(&connection.username, password)
            .with_context(|| format!("Autenticacion fallida para {}", connection.username))?;

        if !session.authenticated() {
            return Err(anyhow!("No autenticado"));
        }

        let sftp = session.sftp().context("No se pudo abrir canal SFTP")?;
        let home_dir = sftp.realpath(Path::new(".")).unwrap_or_else(|_| PathBuf::from("/"));

        Ok(Self {
            connection: connection.clone(),
            password: password.to_string(),
            _session: session,
            sftp,
            home_dir,
        })
    }

    pub fn snapshot_credentials(&self) -> (SavedConnection, String) {
        (self.connection.clone(), self.password.clone())
    }

    pub fn list_dir(
        &self,
        path: &Path,
        sort_mode: SortMode,
        sort_order: SortOrder,
        show_hidden: bool,
    ) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();
        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf());
        entries.push(FileEntry {
            name: "..".to_string(),
            path: parent,
            is_dir: true,
            is_executable: false,
            size_bytes: None,
            modified: None,
        });

        let mut remote = self
            .sftp
            .readdir(path)
            .with_context(|| format!("No se pudo listar remoto {}", path.display()))?
            .into_iter()
            .filter_map(|(full_path, stat)| {
                let name = full_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name == "." || name == ".." {
                    return None;
                }
                if !show_hidden && name.starts_with('.') {
                    return None;
                }

                let perm = stat.perm.unwrap_or(0);
                let is_dir = (perm & MODE_TYPE_MASK) == MODE_DIR;
                let is_executable = !is_dir && (perm & 0o111 != 0);
                let size = if is_dir { None } else { stat.size };
                let modified = stat
                    .mtime
                    .map(|secs| UNIX_EPOCH + Duration::from_secs(secs));

                Some(FileEntry {
                    name,
                    path: full_path,
                    is_dir,
                    is_executable,
                    size_bytes: size,
                    modified,
                })
            })
            .collect::<Vec<_>>();

        remote.sort_by(|a, b| {
            let dir_order = a.is_dir.cmp(&b.is_dir).reverse();
            if dir_order != Ordering::Equal {
                return dir_order;
            }

            let order = match sort_mode {
                SortMode::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortMode::Size => a.size_bytes.unwrap_or(0).cmp(&b.size_bytes.unwrap_or(0)),
                SortMode::Modified => a.modified.cmp(&b.modified),
                SortMode::Type => {
                    let a_ext = Path::new(&a.name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let b_ext = Path::new(&b.name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    a_ext.cmp(&b_ext)
                }
            };

            if sort_order == SortOrder::Descending {
                order.reverse()
            } else {
                order
            }
        });

        entries.extend(remote);
        Ok(entries)
    }

    pub fn read_file_bytes(&self, path: &Path) -> Result<Vec<u8>> {
        let mut file = self
            .sftp
            .open(path)
            .with_context(|| format!("No se pudo abrir remoto {}", path.display()))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .with_context(|| format!("No se pudo leer remoto {}", path.display()))?;
        Ok(buf)
    }

    pub fn remove_recursive(&self, path: &Path) -> Result<()> {
        let stat = self
            .sftp
            .stat(path)
            .with_context(|| format!("No se pudo obtener metadata remota de {}", path.display()))?;
        let perm = stat.perm.unwrap_or(0);
        if (perm & MODE_TYPE_MASK) == MODE_DIR {
            let children = self
                .sftp
                .readdir(path)
                .with_context(|| format!("No se pudo listar remoto {}", path.display()))?;
            for (child, _) in children {
                let Some(name) = child.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                    continue;
                };
                if name == "." || name == ".." {
                    continue;
                }
                self.remove_recursive(&child)?;
            }
            self.sftp
                .rmdir(path)
                .with_context(|| format!("No se pudo borrar directorio remoto {}", path.display()))?;
            return Ok(());
        }

        self.sftp
            .unlink(path)
            .with_context(|| format!("No se pudo borrar archivo remoto {}", path.display()))?;
        Ok(())
    }

    pub fn create_dir_all(&self, path: &Path) -> Result<()> {
        if path.as_os_str().is_empty() || path == Path::new("/") {
            return Ok(());
        }
        if self.sftp.stat(path).is_ok() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            self.create_dir_all(parent)?;
        }
        self.sftp.mkdir(path, 0o755).ok();
        Ok(())
    }

    pub fn rename(&self, source: &Path, destination: &Path) -> Result<()> {
        self.sftp
            .rename(source, destination, None)
            .with_context(|| {
                format!(
                    "No se pudo renombrar remoto {} a {}",
                    source.display(),
                    destination.display()
                )
            })
    }

    pub fn exists(&self, path: &Path) -> bool {
        self.sftp.stat(path).is_ok()
    }

    pub fn copy_local_to_remote(&self, source: &Path, destination: &Path) -> Result<()> {
        self.copy_local_to_remote_with_progress(source, destination, &mut |_, _| {}, &|| false)
    }

    pub fn copy_local_to_remote_with_progress<F, C>(
        &self,
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
            return Err(anyhow!(TRANSFER_CANCELLED_MARKER));
        }

        let metadata = fs::symlink_metadata(source)
            .with_context(|| format!("No se pudo leer metadata de {}", source.display()))?;

        if metadata.is_dir() {
            self.create_dir_all(destination)?;
            for child in fs::read_dir(source)
                .with_context(|| format!("No se pudo listar {}", source.display()))?
            {
                let child = child?;
                let child_src = child.path();
                let child_dst = destination.join(child.file_name());
                self.copy_local_to_remote_with_progress(
                    &child_src,
                    &child_dst,
                    on_progress,
                    should_cancel,
                )?;
            }
            return Ok(());
        }

        if let Some(parent) = destination.parent() {
            self.create_dir_all(parent)?;
        }

        let mut local_file = fs::File::open(source)
            .with_context(|| format!("No se pudo abrir {}", source.display()))?;
        let mut remote_file = self
            .sftp
            .create(destination)
            .with_context(|| format!("No se pudo crear remoto {}", destination.display()))?;
        copy_stream_with_progress(
            &mut local_file,
            &mut remote_file,
            source,
            on_progress,
            should_cancel,
        )
            .with_context(|| format!("No se pudo copiar {} a remoto", source.display()))?;
        remote_file.flush().ok();
        Ok(())
    }

    pub fn copy_remote_to_local(&self, source: &Path, destination: &Path) -> Result<()> {
        self.copy_remote_to_local_with_progress(source, destination, &mut |_, _| {}, &|| false)
    }

    pub fn copy_remote_to_local_with_progress<F, C>(
        &self,
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
            return Err(anyhow!(TRANSFER_CANCELLED_MARKER));
        }

        let stat = self
            .sftp
            .stat(source)
            .with_context(|| format!("No se pudo leer metadata remota de {}", source.display()))?;
        let perm = stat.perm.unwrap_or(0);
        let is_dir = (perm & MODE_TYPE_MASK) == MODE_DIR;

        if is_dir {
            fs::create_dir_all(destination)
                .with_context(|| format!("No se pudo crear {}", destination.display()))?;
            let children = self
                .sftp
                .readdir(source)
                .with_context(|| format!("No se pudo listar remoto {}", source.display()))?;
            for (child_src, _) in children {
                let Some(name) = child_src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                    continue;
                };
                if name == "." || name == ".." {
                    continue;
                }
                let child_dst = destination.join(name);
                self.copy_remote_to_local_with_progress(
                    &child_src,
                    &child_dst,
                    on_progress,
                    should_cancel,
                )?;
            }
            return Ok(());
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("No se pudo crear {}", parent.display()))?;
        }

        let mut remote_file = self
            .sftp
            .open(source)
            .with_context(|| format!("No se pudo abrir remoto {}", source.display()))?;
        let mut local_file = fs::File::create(destination)
            .with_context(|| format!("No se pudo crear {}", destination.display()))?;
        copy_stream_with_progress(
            &mut remote_file,
            &mut local_file,
            source,
            on_progress,
            should_cancel,
        )
            .with_context(|| format!("No se pudo copiar remoto {}", source.display()))?;
        local_file.flush().ok();
        Ok(())
    }

    pub fn copy_remote_to_remote(&self, source: &Path, destination: &Path) -> Result<()> {
        self.copy_remote_to_remote_with_progress(source, destination, &mut |_, _| {}, &|| false)
    }

    pub fn copy_remote_to_remote_with_progress<F, C>(
        &self,
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
            return Err(anyhow!(TRANSFER_CANCELLED_MARKER));
        }

        let stat = self
            .sftp
            .stat(source)
            .with_context(|| format!("No se pudo leer metadata remota de {}", source.display()))?;
        let perm = stat.perm.unwrap_or(0);
        let is_dir = (perm & MODE_TYPE_MASK) == MODE_DIR;

        if is_dir {
            self.create_dir_all(destination)?;
            let children = self
                .sftp
                .readdir(source)
                .with_context(|| format!("No se pudo listar remoto {}", source.display()))?;
            for (child_src, _) in children {
                let Some(name) = child_src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                    continue;
                };
                if name == "." || name == ".." {
                    continue;
                }
                let child_dst = destination.join(name);
                self.copy_remote_to_remote_with_progress(
                    &child_src,
                    &child_dst,
                    on_progress,
                    should_cancel,
                )?;
            }
            return Ok(());
        }

        if let Some(parent) = destination.parent() {
            self.create_dir_all(parent)?;
        }

        let mut remote_src = self
            .sftp
            .open(source)
            .with_context(|| format!("No se pudo abrir remoto {}", source.display()))?;
        let mut remote_dst = self
            .sftp
            .create(destination)
            .with_context(|| format!("No se pudo crear remoto {}", destination.display()))?;
        copy_stream_with_progress(
            &mut remote_src,
            &mut remote_dst,
            source,
            on_progress,
            should_cancel,
        )
            .with_context(|| format!("No se pudo copiar remoto {}", source.display()))?;
        remote_dst.flush().ok();
        Ok(())
    }

    pub fn estimate_remote_bytes(&self, path: &Path) -> Result<u64> {
        let stat = self
            .sftp
            .stat(path)
            .with_context(|| format!("No se pudo leer metadata remota de {}", path.display()))?;
        let perm = stat.perm.unwrap_or(0);
        if (perm & MODE_TYPE_MASK) != MODE_DIR {
            return Ok(stat.size.unwrap_or(0));
        }

        let mut total = 0u64;
        let children = self
            .sftp
            .readdir(path)
            .with_context(|| format!("No se pudo listar remoto {}", path.display()))?;
        for (child, _) in children {
            let Some(name) = child.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if name == "." || name == ".." {
                continue;
            }
            total = total.saturating_add(self.estimate_remote_bytes(&child)?);
        }

        Ok(total)
    }
}

fn copy_stream_with_progress<R, W, F, C>(
    reader: &mut R,
    writer: &mut W,
    current_path: &Path,
    on_progress: &mut F,
    should_cancel: &C,
) -> Result<()>
where
    R: Read,
    W: Write,
    F: FnMut(u64, &Path),
    C: Fn() -> bool,
{
    let mut buffer = vec![0u8; 4 * 1024 * 1024];
    loop {
        if should_cancel() {
            return Err(anyhow!(TRANSFER_CANCELLED_MARKER));
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        on_progress(read as u64, current_path);
    }
    Ok(())
}
