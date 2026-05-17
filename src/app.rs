use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc::{Receiver, TryRecvError},
    },
    time::Instant,
    time::SystemTime,
};

use anyhow::{Context, Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use glob::Pattern;

use crate::{
    config::{ConfigStore, SavedConnection},
    ops::{OverwriteBatchState, OverwriteOperation, apply_batch_operation, remove_path_recursive},
    remote::RemoteSession,
    transfer::{CopyJob, TransferBackend, TransferEvent, spawn_copy_worker},
    viewer::ViewerState,
};

pub enum AppMode {
    Panels,
    Viewer(ViewerState),
    Search(SearchState),
}

#[derive(Clone, Debug)]
pub struct RenameInputDialog {
    pub sources: Vec<PathBuf>,
    pub source_label: String,
    pub default_move_dir: PathBuf,
    pub input: String,
}

#[derive(Clone, Debug)]
pub struct MkdirInputDialog {
    pub base_dir: PathBuf,
    pub input: String,
}

#[derive(Clone, Debug)]
pub struct SearchInputDialog {
    pub root_dir: PathBuf,
    pub input: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteConnectionField {
    Host,
    Port,
    Username,
    Password,
    Save,
}

impl RemoteConnectionField {
    fn next(self) -> Self {
        match self {
            Self::Host => Self::Port,
            Self::Port => Self::Username,
            Self::Username => Self::Password,
            Self::Password => Self::Save,
            Self::Save => Self::Host,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Host => Self::Save,
            Self::Port => Self::Host,
            Self::Username => Self::Port,
            Self::Password => Self::Username,
            Self::Save => Self::Password,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RemoteConnectionDialog {
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub save_connection: bool,
    pub selected_saved: Option<usize>,
    pub selected_field: RemoteConnectionField,
}

#[derive(Clone, Debug)]
pub struct SearchState {
    pub root_dir: PathBuf,
    pub query: String,
    pub pattern: String,
    pub file_type: Option<String>,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub pending_dirs: Vec<PathBuf>,
    pub processed_dirs: usize,
    pub finished: bool,
}

impl SearchState {
    pub fn progress_fraction(&self) -> f64 {
        let remaining = self.pending_dirs.len();
        let total = self.processed_dirs + remaining;
        if total == 0 {
            return if self.finished { 1.0 } else { 0.0 };
        }
        self.processed_dirs as f64 / total as f64
    }
}

#[derive(Clone, Debug)]
pub struct RenamePlan {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub destination_backend: PanelBackend,
}

#[derive(Clone, Debug)]
pub enum PendingRenamePlan {
    Single(RenamePlan),
    Multiple {
        sources: Vec<PathBuf>,
        destination_dir: PathBuf,
        destination_backend: PanelBackend,
    },
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    Copy,
    Rename,
    Mkdir,
    OverwriteConflict,
    Delete,
    DeleteSavedConnection,
    Quit,
}

#[derive(Clone, Debug)]
pub struct ConfirmationDialog {
    pub action: PendingAction,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortMode {
    Name,
    Size,
    Modified,
    Type,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Name => SortMode::Size,
            SortMode::Size => SortMode::Modified,
            SortMode::Modified => SortMode::Type,
            SortMode::Type => SortMode::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Name => "Nombre",
            SortMode::Size => "Tamaño",
            SortMode::Modified => "Fecha",
            SortMode::Type => "Tipo",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn toggle(self) -> Self {
        match self {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            SortOrder::Ascending => "↑",
            SortOrder::Descending => "↓",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivePanel {
    Left,
    Right,
}

impl ActivePanel {
    pub fn toggle(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_executable: bool,
    pub size_bytes: Option<u64>,
    pub modified: Option<SystemTime>,
}

impl FileEntry {
    fn parent_dir(path: &Path) -> Result<Self> {
        Ok(Self {
            name: "..".to_string(),
            path: path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.to_path_buf()),
            is_dir: true,
            is_executable: false,
            size_bytes: None,
            modified: None,
        })
    }

    fn from_path(path: PathBuf) -> Result<Self> {
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("No se pudo leer metadata de {}", path.display()))?;
        #[cfg(unix)]
        let is_executable = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let is_executable = false;

        Ok(Self {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            path,
            is_dir: metadata.is_dir(),
            is_executable,
            size_bytes: if metadata.is_file() {
                Some(metadata.len())
            } else {
                None
            },
            modified: metadata.modified().ok(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct PanelState {
    pub backend: PanelBackend,
    pub cwd: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub marked: BTreeSet<PathBuf>,
    pub sort_mode: SortMode,
    pub sort_order: SortOrder,
    pub show_hidden: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PanelBackend {
    Local,
    Remote { connection_name: String },
}

impl PanelState {
    pub fn new(cwd: PathBuf) -> Result<Self> {
        let mut panel = Self {
            backend: PanelBackend::Local,
            cwd,
            entries: Vec::new(),
            selected: 0,
            marked: BTreeSet::new(),
            sort_mode: SortMode::Name,
            sort_order: SortOrder::Ascending,
            show_hidden: false,
        };
        panel.reload()?;
        Ok(panel)
    }

    pub fn reload(&mut self) -> Result<()> {
        if self.backend != PanelBackend::Local {
            return Err(anyhow!("reload local invocado en panel remoto"));
        }
        self.entries = read_dir_entries(&self.cwd, self.sort_mode, self.sort_order, self.show_hidden)?;
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            self.selected = 0;
            return;
        }

        let next = self.selected as isize + delta;
        let upper = self.entries.len().saturating_sub(1) as isize;
        self.selected = next.clamp(0, upper) as usize;
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }

    pub fn toggle_mark(&mut self) {
        if let Some(entry) = self.selected_entry() {
            let path = entry.path.clone();
            if !self.marked.insert(path.clone()) {
                self.marked.remove(&path);
            }
        }
    }

    pub fn marked_count(&self) -> usize {
        self.marked.len()
    }

    pub fn operation_targets(&self) -> Vec<String> {
        if self.marked.is_empty() {
            self.selected_entry()
                .map(|entry| vec![entry.name.clone()])
                .unwrap_or_default()
        } else {
            self.entries
                .iter()
                .filter(|entry| self.marked.contains(&entry.path))
                .map(|entry| entry.name.clone())
                .collect()
        }
    }

    pub fn operation_source_paths(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            return self
                .selected_entry()
                .filter(|entry| entry.name != "..")
                .map(|entry| vec![entry.path.clone()])
                .unwrap_or_default();
        }

        self.entries
            .iter()
            .filter(|entry| entry.name != ".." && self.marked.contains(&entry.path))
            .map(|entry| entry.path.clone())
            .collect()
    }

    pub fn operation_directory_count(&self) -> usize {
        if self.marked.is_empty() {
            return self
                .selected_entry()
                .filter(|entry| entry.name != ".." && entry.is_dir)
                .map(|_| 1usize)
                .unwrap_or(0);
        }

        self.entries
            .iter()
            .filter(|entry| entry.name != ".." && entry.is_dir && self.marked.contains(&entry.path))
            .count()
    }

    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }
}

pub struct App {
    pub left: PanelState,
    pub right: PanelState,
    pub active_panel: ActivePanel,
    pub mode: AppMode,
    pub panel_page_size: usize,
    pub marquee_tick: u64,
    pub show_help: bool,
    pub show_capibara: bool,
    pub rename_input: Option<RenameInputDialog>,
    pub mkdir_input: Option<MkdirInputDialog>,
    pub search_input: Option<SearchInputDialog>,
    pub remote_connection_input: Option<RemoteConnectionDialog>,
    pub saved_connections: Vec<SavedConnection>,
    pub pending_rename: Option<PendingRenamePlan>,
    pub pending_mkdir: Option<PathBuf>,
    pub pending_overwrite: Option<OverwriteBatchState>,
    pub pending_saved_connection_delete: Option<usize>,
    pub confirmation: Option<ConfirmationDialog>,
    pub exit_dir: Option<PathBuf>,
    pub should_quit: bool,
    pub status_message: String,
    pub config_store: ConfigStore,
    pub remote_sessions: HashMap<String, RemoteSession>,
    pub active_transfer: Option<TransferState>,
}

pub struct TransferState {
    pub receiver: Receiver<TransferEvent>,
    pub cancel_flag: Arc<AtomicBool>,
    pub started_at: Instant,
    pub source_panel: ActivePanel,
    pub copied_bytes: u64,
    pub total_bytes: u64,
}

impl App {
    pub fn new() -> Result<Self> {
        let cwd = std::env::current_dir().context("No se pudo obtener el directorio actual")?;
        let config_store = ConfigStore::new()?;
        let saved_connections = config_store.load_connections().unwrap_or_default();
        Ok(Self {
            left: PanelState::new(cwd.clone())?,
            right: PanelState::new(cwd)?,
            active_panel: ActivePanel::Left,
            mode: AppMode::Panels,
            panel_page_size: 10,
            marquee_tick: 0,
            show_help: false,
            show_capibara: false,
            rename_input: None,
            mkdir_input: None,
            search_input: None,
            remote_connection_input: None,
            saved_connections,
            pending_rename: None,
            pending_mkdir: None,
            pending_overwrite: None,
            pending_saved_connection_delete: None,
            confirmation: None,
            exit_dir: None,
            should_quit: false,
            status_message: "Listo".to_string(),
            config_store,
            remote_sessions: HashMap::new(),
            active_transfer: None,
        })
    }

    pub fn advance_transfer(&mut self) -> Result<()> {
        let mut completed_event = None;

        if let Some(state) = &mut self.active_transfer {
            loop {
                match state.receiver.try_recv() {
                    Ok(TransferEvent::Progress {
                        copied_bytes,
                        total_bytes,
                        current_item,
                    }) => {
                        state.copied_bytes = copied_bytes;
                        state.total_bytes = total_bytes;
                        let elapsed = state.started_at.elapsed().as_secs_f64().max(0.001);
                        let speed_mib = copied_bytes as f64 / 1024.0 / 1024.0 / elapsed;
                        let percent = if total_bytes == 0 {
                            0.0
                        } else {
                            (copied_bytes as f64 * 100.0) / total_bytes as f64
                        };
                        self.status_message = format!(
                            "Copiando {:.1}% ({:.1}/{:.1} MiB) {:.1} MiB/s | {}",
                            percent,
                            copied_bytes as f64 / 1024.0 / 1024.0,
                            total_bytes as f64 / 1024.0 / 1024.0,
                            speed_mib,
                            current_item
                        );
                    }
                    Ok(TransferEvent::Finished {
                        copied_bytes,
                        total_bytes,
                        processed,
                        failed,
                        skipped,
                        error,
                    }) => {
                        completed_event = Some((
                            state.source_panel,
                            copied_bytes,
                            total_bytes,
                            processed,
                            failed,
                            skipped,
                            error,
                        ));
                        break;
                    }
                    Ok(TransferEvent::Cancelled {
                        copied_bytes,
                        total_bytes,
                        processed,
                        failed,
                        skipped,
                    }) => {
                        completed_event = Some((
                            state.source_panel,
                            copied_bytes,
                            total_bytes,
                            processed,
                            failed,
                            skipped,
                            Some("__CANCELLED_BY_USER__".to_string()),
                        ));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        completed_event = Some((
                            state.source_panel,
                            state.copied_bytes,
                            state.total_bytes,
                            0,
                            1,
                            0,
                            Some("Canal de transferencia desconectado".to_string()),
                        ));
                        break;
                    }
                }
            }
        }

        if let Some((source_panel, copied, total, processed, failed, skipped, error)) = completed_event {
            self.active_transfer = None;
            self.panel_mut(source_panel).clear_marks();
            let _ = self.reload_panels();

            if let Some(error) = error {
                if error == "__CANCELLED_BY_USER__" {
                    self.status_message = format!(
                        "Transferencia cancelada ({:.1}/{:.1} MiB)",
                        copied as f64 / 1024.0 / 1024.0,
                        total as f64 / 1024.0 / 1024.0
                    );
                } else {
                    self.status_message = format!("Transferencia fallida: {}", error);
                }
            } else {
                let percent = if total == 0 {
                    100.0
                } else {
                    (copied as f64 * 100.0) / total as f64
                };
                self.status_message = format!(
                    "Transferencia completada {:.1}% | procesados {} | omitidos {} | fallidos {}",
                    percent, processed, skipped, failed
                );
            }
        }

        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.mkdir_input.is_some() {
            return self.handle_mkdir_input_key(key);
        }

        if self.rename_input.is_some() {
            return self.handle_rename_input_key(key);
        }

        if self.show_help {
            return self.handle_help_key(key);
        }

        if self.confirmation.is_some() {
            return self.handle_confirmation_key(key);
        }

        if key.code == KeyCode::F(1) && key.modifiers.contains(KeyModifiers::SHIFT) {
            self.show_capibara = !self.show_capibara;
            return Ok(());
        }

        if self.show_capibara
            && (key.code == KeyCode::Esc
                || (key.code == KeyCode::F(1) && key.modifiers.contains(KeyModifiers::SHIFT)))
        {
            self.show_capibara = false;
            return Ok(());
        }

        if self.search_input.is_some() {
            return self.handle_search_input_key(key);
        }

        if self.remote_connection_input.is_some() {
            return self.handle_remote_connection_input_key(key);
        }

        if self.active_transfer.is_some() && key.code == KeyCode::Esc {
            self.request_cancel_transfer();
            return Ok(());
        }

        if matches!(self.mode, AppMode::Viewer(_)) {
            return self.handle_viewer_key(key);
        }

        if matches!(self.mode, AppMode::Search(_)) {
            return self.handle_search_key(key);
        }

        match key.code {
            KeyCode::F(2) => {
                if self.active_panel().backend == PanelBackend::Local {
                    self.open_search_input();
                } else {
                    self.status_message = "Busqueda en panel remoto se habilitara en una fase posterior".to_string();
                }
            }
            KeyCode::F(1) => {
                self.show_help = true;
            }
            KeyCode::Tab => self.active_panel = self.active_panel.toggle(),
            KeyCode::Up => self.active_panel_mut().move_selection(-1),
            KeyCode::Down => self.active_panel_mut().move_selection(1),
            KeyCode::PageUp => {
                let step = self.panel_step() as isize;
                self.active_panel_mut().move_selection(-step);
            }
            KeyCode::PageDown => {
                let step = self.panel_step() as isize;
                self.active_panel_mut().move_selection(step);
            }
            KeyCode::Enter => self.open_selected()?,
            KeyCode::Backspace => self.go_parent()?,
            KeyCode::Char(' ') => self.active_panel_mut().toggle_mark(),
            KeyCode::F(3) => self.preview_selected(),
            KeyCode::F(5) => {
                if self.active_transfer.is_some() {
                    self.status_message = "Espera a que termine la transferencia actual".to_string();
                } else {
                    self.open_confirmation(PendingAction::Copy)
                }
            }
            KeyCode::F(6) => {
                if self.active_transfer.is_some() {
                    self.status_message = "Espera a que termine la transferencia actual".to_string();
                } else {
                    self.open_rename_input()
                }
            }
            KeyCode::F(7) => {
                if self.active_transfer.is_some() {
                    self.status_message = "Espera a que termine la transferencia actual".to_string();
                } else {
                    self.open_mkdir_input()
                }
            }
            KeyCode::F(8) => {
                if self.active_transfer.is_some() {
                    self.status_message = "Espera a que termine la transferencia actual".to_string();
                } else {
                    self.open_confirmation(PendingAction::Delete)
                }
            }
            KeyCode::F(12) => self.open_remote_connection_input(),
            KeyCode::F(9) if key.modifiers.contains(KeyModifiers::SHIFT) => {
                {
                    let panel = self.active_panel_mut();
                    panel.sort_order = panel.sort_order.toggle();
                }
                self.reload_active_panel()?;
                let panel = self.active_panel();
                self.status_message = format!("Orden: {} {}", panel.sort_mode.label(), panel.sort_order.symbol());
            }
            KeyCode::F(9) => {
                {
                    let panel = self.active_panel_mut();
                    panel.sort_mode = panel.sort_mode.next();
                }
                self.reload_active_panel()?;
                let panel = self.active_panel();
                self.status_message = format!("Orden: {} {}", panel.sort_mode.label(), panel.sort_order.symbol());
            }
            KeyCode::F(4) => {
                let show_hidden = {
                    let panel = self.active_panel_mut();
                    panel.show_hidden = !panel.show_hidden;
                    panel.show_hidden
                };
                self.reload_active_panel()?;
                self.status_message = if show_hidden {
                    "Ocultos visibles".to_string()
                } else {
                    "Ocultos ocultos".to_string()
                };
            }
            KeyCode::F(10) | KeyCode::Char('q') => self.open_confirmation(PendingAction::Quit),
            _ => {}
        }
        Ok(())
    }

    fn request_cancel_transfer(&mut self) {
        if let Some(transfer) = &self.active_transfer {
            transfer.cancel_flag.store(true, AtomicOrdering::Relaxed);
            self.status_message = "Cancelando transferencia...".to_string();
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent, left_panel_width: u16) {
        if let AppMode::Viewer(viewer) = &mut self.mode {
            match mouse.kind {
                MouseEventKind::ScrollDown => viewer.scroll_down(),
                MouseEventKind::ScrollUp => viewer.scroll_up(),
                _ => {}
            }
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let target = if mouse.column < left_panel_width {
                    ActivePanel::Left
                } else {
                    ActivePanel::Right
                };
                self.active_panel = target;
            }
            MouseEventKind::ScrollDown => self.active_panel_mut().move_selection(1),
            MouseEventKind::ScrollUp => self.active_panel_mut().move_selection(-1),
            _ => {}
        }
    }

    pub fn active_panel(&self) -> &PanelState {
        match self.active_panel {
            ActivePanel::Left => &self.left,
            ActivePanel::Right => &self.right,
        }
    }

    pub fn inactive_panel(&self) -> &PanelState {
        match self.active_panel {
            ActivePanel::Left => &self.right,
            ActivePanel::Right => &self.left,
        }
    }

    pub fn active_panel_mut(&mut self) -> &mut PanelState {
        match self.active_panel {
            ActivePanel::Left => &mut self.left,
            ActivePanel::Right => &mut self.right,
        }
    }

    fn panel(&self, panel: ActivePanel) -> &PanelState {
        match panel {
            ActivePanel::Left => &self.left,
            ActivePanel::Right => &self.right,
        }
    }

    fn panel_mut(&mut self, panel: ActivePanel) -> &mut PanelState {
        match panel {
            ActivePanel::Left => &mut self.left,
            ActivePanel::Right => &mut self.right,
        }
    }

    fn reload_panel(&mut self, panel: ActivePanel) -> Result<()> {
        let (backend, cwd, sort_mode, sort_order, show_hidden, selected) = {
            let state = self.panel(panel);
            (
                state.backend.clone(),
                state.cwd.clone(),
                state.sort_mode,
                state.sort_order,
                state.show_hidden,
                state.selected,
            )
        };

        let entries = match backend {
            PanelBackend::Local => read_dir_entries(&cwd, sort_mode, sort_order, show_hidden)?,
            PanelBackend::Remote { connection_name } => {
                let session = self
                    .remote_sessions
                    .get(&connection_name)
                    .ok_or_else(|| anyhow!("Sesion remota no disponible: {connection_name}"))?;
                session.list_dir(&cwd, sort_mode, sort_order, show_hidden)?
            }
        };

        let state = self.panel_mut(panel);
        state.entries = entries;
        state.selected = if state.entries.is_empty() {
            0
        } else {
            selected.min(state.entries.len().saturating_sub(1))
        };
        Ok(())
    }

    fn reload_active_panel(&mut self) -> Result<()> {
        self.reload_panel(self.active_panel)
    }

    fn remote_session_for(&self, connection_name: &str) -> Result<&RemoteSession> {
        self.remote_sessions
            .get(connection_name)
            .ok_or_else(|| anyhow!("Sesion remota no disponible: {connection_name}"))
    }

    fn transfer_backend_from_panel_backend(&self, backend: &PanelBackend) -> Result<TransferBackend> {
        match backend {
            PanelBackend::Local => Ok(TransferBackend::Local),
            PanelBackend::Remote { connection_name } => {
                let session = self.remote_session_for(connection_name)?;
                let (connection, password) = session.snapshot_credentials();
                Ok(TransferBackend::Remote {
                    connection,
                    password,
                })
            }
        }
    }

    fn path_exists_on_backend(&self, backend: &PanelBackend, path: &Path) -> Result<bool> {
        match backend {
            PanelBackend::Local => Ok(path.exists()),
            PanelBackend::Remote { connection_name } => {
                let session = self.remote_session_for(connection_name)?;
                Ok(session.exists(path))
            }
        }
    }

    fn copy_between_backends(
        &self,
        source_backend: &PanelBackend,
        destination_backend: &PanelBackend,
        source: &Path,
        destination: &Path,
    ) -> Result<()> {
        match (source_backend, destination_backend) {
            (PanelBackend::Local, PanelBackend::Local) => {
                apply_batch_operation(OverwriteOperation::Copy, source, destination)
            }
            (PanelBackend::Local, PanelBackend::Remote { connection_name }) => self
                .remote_session_for(connection_name)?
                .copy_local_to_remote(source, destination),
            (PanelBackend::Remote { connection_name }, PanelBackend::Local) => self
                .remote_session_for(connection_name)?
                .copy_remote_to_local(source, destination),
            (
                PanelBackend::Remote {
                    connection_name: source_conn,
                },
                PanelBackend::Remote {
                    connection_name: destination_conn,
                },
            ) if source_conn == destination_conn => self
                .remote_session_for(source_conn)?
                .copy_remote_to_remote(source, destination),
            (PanelBackend::Remote { .. }, PanelBackend::Remote { .. }) => {
                Err(anyhow!("Copia entre dos conexiones remotas distintas no soportada aun"))
            }
        }
    }

    fn remove_on_backend(&self, backend: &PanelBackend, path: &Path) -> Result<()> {
        match backend {
            PanelBackend::Local => remove_path_recursive(path),
            PanelBackend::Remote { connection_name } => {
                self.remote_session_for(connection_name)?.remove_recursive(path)
            }
        }
    }

    fn mkdir_on_backend(&self, backend: &PanelBackend, path: &Path) -> Result<()> {
        match backend {
            PanelBackend::Local => fs::create_dir_all(path)
                .with_context(|| format!("No se pudo crear directorio {}", path.display())),
            PanelBackend::Remote { connection_name } => {
                self.remote_session_for(connection_name)?.create_dir_all(path)
            }
        }
    }

    fn move_between_backends(
        &self,
        source_backend: &PanelBackend,
        destination_backend: &PanelBackend,
        source: &Path,
        destination: &Path,
    ) -> Result<()> {
        match (source_backend, destination_backend) {
            (PanelBackend::Local, PanelBackend::Local) => {
                apply_batch_operation(OverwriteOperation::Move, source, destination)
            }
            (
                PanelBackend::Remote {
                    connection_name: source_conn,
                },
                PanelBackend::Remote {
                    connection_name: destination_conn,
                },
            ) if source_conn == destination_conn => self
                .remote_session_for(source_conn)?
                .rename(source, destination),
            _ => {
                self.copy_between_backends(source_backend, destination_backend, source, destination)?;
                self.remove_on_backend(source_backend, source)
            }
        }
    }

    pub fn confirmation_message(&self) -> Option<&str> {
        self.confirmation
            .as_ref()
            .map(|dialog| dialog.message.as_str())
    }

    pub fn set_panel_page_size(&mut self, page_size: usize) {
        self.panel_page_size = page_size.max(1);
    }

    pub fn advance_marquee(&mut self) {
        self.marquee_tick = self.marquee_tick.wrapping_add(1);
    }

    pub fn exit_directory(&self) -> PathBuf {
        self.exit_dir
            .clone()
            .unwrap_or_else(|| self.active_panel().cwd.clone())
    }

    fn open_selected(&mut self) -> Result<()> {
        let selected = self.active_panel().selected_entry().cloned();
        if let Some(entry) = selected {
            if entry.is_dir {
                {
                    let panel = self.active_panel_mut();
                    panel.cwd = entry.path;
                    panel.selected = 0;
                }
                self.reload_active_panel()?;
            } else {
                self.status_message = format!("Seleccionado: {}", entry.name);
            }
        }
        Ok(())
    }

    fn go_parent(&mut self) -> Result<()> {
        let parent = self.active_panel().cwd.parent().map(Path::to_path_buf);
        if let Some(parent) = parent {
            {
                let panel = self.active_panel_mut();
                panel.cwd = parent;
                panel.selected = 0;
            }
            self.reload_active_panel()?;
        }
        Ok(())
    }

    fn open_search_input(&mut self) {
        self.search_input = Some(SearchInputDialog {
            root_dir: self.active_panel().cwd.clone(),
            input: String::new(),
        });
        self.status_message = "F2: ingrese texto o patron (ej. *.rs type:md)".to_string();
    }

    fn open_remote_connection_input(&mut self) {
        let mut dialog = RemoteConnectionDialog {
            host: String::new(),
            port: "22".to_string(),
            username: String::new(),
            password: String::new(),
            save_connection: true,
            selected_saved: None,
            selected_field: RemoteConnectionField::Host,
        };

        if !self.saved_connections.is_empty() {
            dialog.selected_saved = Some(0);
            Self::apply_selected_saved_connection(&mut dialog, &self.saved_connections, 0);
        }

        self.remote_connection_input = Some(dialog);
        self.status_message = "F12: complete datos SCP y Enter para guardar/conectar".to_string();
    }

    fn handle_remote_connection_input_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.remote_connection_input = None;
                self.status_message = "Conexion remota cancelada".to_string();
            }
            KeyCode::Tab => {
                if let Some(dialog) = &mut self.remote_connection_input {
                    dialog.selected_field = dialog.selected_field.next();
                }
            }
            KeyCode::BackTab => {
                if let Some(dialog) = &mut self.remote_connection_input {
                    dialog.selected_field = dialog.selected_field.prev();
                }
            }
            KeyCode::Up => {
                if let Some(dialog) = &mut self.remote_connection_input {
                    if self.saved_connections.is_empty() {
                        return Ok(());
                    }
                    let current = dialog.selected_saved.unwrap_or(0);
                    let next = current.saturating_sub(1);
                    dialog.selected_saved = Some(next);
                    Self::apply_selected_saved_connection(dialog, &self.saved_connections, next);
                }
            }
            KeyCode::Down => {
                if let Some(dialog) = &mut self.remote_connection_input {
                    if self.saved_connections.is_empty() {
                        return Ok(());
                    }
                    let current = dialog.selected_saved.unwrap_or(0);
                    let next = (current + 1).min(self.saved_connections.len().saturating_sub(1));
                    dialog.selected_saved = Some(next);
                    Self::apply_selected_saved_connection(dialog, &self.saved_connections, next);
                }
            }
            KeyCode::Delete => {
                let selected = self
                    .remote_connection_input
                    .as_ref()
                    .and_then(|dialog| dialog.selected_saved);

                if let Some(index) = selected {
                    if let Some(connection) = self.saved_connections.get(index) {
                        self.pending_saved_connection_delete = Some(index);
                        self.confirmation = Some(ConfirmationDialog {
                            action: PendingAction::DeleteSavedConnection,
                            message: format!(
                                "Eliminar conexion guardada '{}' ?",
                                connection.name
                            ),
                        });
                    } else {
                        self.status_message = "No hay conexion valida para eliminar".to_string();
                    }
                } else {
                    self.status_message = "No hay conexion seleccionada para eliminar".to_string();
                }
            }
            KeyCode::Enter => {
                let Some(dialog) = self.remote_connection_input.take() else {
                    return Ok(());
                };

                let host = dialog.host.trim().to_string();
                let username = dialog.username.trim().to_string();
                if host.is_empty() || username.is_empty() {
                    self.remote_connection_input = Some(dialog);
                    self.status_message = "Host y usuario son obligatorios".to_string();
                    return Ok(());
                }

                let port: u16 = match dialog.port.trim().parse() {
                    Ok(port) => port,
                    Err(_) => {
                        self.remote_connection_input = Some(dialog);
                        self.status_message = "Puerto invalido (use 1..65535)".to_string();
                        return Ok(());
                    }
                };

                if dialog.password.is_empty() {
                    self.remote_connection_input = Some(dialog);
                    self.status_message = "Ingrese contrasena para conectar por SCP".to_string();
                    return Ok(());
                }

                let connection_name = Self::build_connection_name(&host, &username, port);
                let connection = SavedConnection {
                    name: connection_name.clone(),
                    host: host.clone(),
                    port,
                    username: username.clone(),
                };

                let session = RemoteSession::connect(&connection, &dialog.password)?;
                let remote_home = session.home_dir.clone();
                self.remote_sessions.insert(connection_name.clone(), session);

                if dialog.save_connection {
                    self.upsert_saved_connection(&host, port, &username)?;
                }

                {
                    let panel = self.active_panel_mut();
                    panel.backend = PanelBackend::Remote {
                        connection_name: connection_name.clone(),
                    };
                    panel.cwd = remote_home;
                    panel.selected = 0;
                    panel.clear_marks();
                }
                self.reload_active_panel()?;

                self.status_message = format!("Conectado a {}", connection_name);
            }
            KeyCode::Backspace => {
                if let Some(dialog) = &mut self.remote_connection_input {
                    match dialog.selected_field {
                        RemoteConnectionField::Host => {
                            dialog.host.pop();
                        }
                        RemoteConnectionField::Port => {
                            dialog.port.pop();
                        }
                        RemoteConnectionField::Username => {
                            dialog.username.pop();
                        }
                        RemoteConnectionField::Password => {
                            dialog.password.pop();
                        }
                        RemoteConnectionField::Save => {}
                    }
                }
            }
            KeyCode::Char(' ') => {
                if let Some(dialog) = &mut self.remote_connection_input {
                    if matches!(dialog.selected_field, RemoteConnectionField::Save) {
                        dialog.save_connection = !dialog.save_connection;
                    } else {
                        match dialog.selected_field {
                            RemoteConnectionField::Host => dialog.host.push(' '),
                            RemoteConnectionField::Port => {}
                            RemoteConnectionField::Username => dialog.username.push(' '),
                            RemoteConnectionField::Password => dialog.password.push(' '),
                            RemoteConnectionField::Save => {}
                        }
                    }
                }
            }
            KeyCode::Char(c) => {
                if c.is_control() {
                    return Ok(());
                }
                if let Some(dialog) = &mut self.remote_connection_input {
                    match dialog.selected_field {
                        RemoteConnectionField::Host => dialog.host.push(c),
                        RemoteConnectionField::Port => {
                            if c.is_ascii_digit() {
                                dialog.port.push(c);
                            }
                        }
                        RemoteConnectionField::Username => dialog.username.push(c),
                        RemoteConnectionField::Password => dialog.password.push(c),
                        RemoteConnectionField::Save => {
                            if c == 's' || c == 'S' {
                                dialog.save_connection = !dialog.save_connection;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn apply_selected_saved_connection(
        dialog: &mut RemoteConnectionDialog,
        connections: &[SavedConnection],
        index: usize,
    ) {
        if let Some(saved) = connections.get(index) {
            dialog.host = saved.host.clone();
            dialog.port = saved.port.to_string();
            dialog.username = saved.username.clone();
            dialog.password.clear();
            dialog.save_connection = true;
        }
    }

    fn build_connection_name(host: &str, username: &str, port: u16) -> String {
        format!("{}@{}:{}", username, host, port)
    }

    fn upsert_saved_connection(&mut self, host: &str, port: u16, username: &str) -> Result<()> {
        let name = Self::build_connection_name(host, username, port);
        let new_item = SavedConnection {
            name: name.clone(),
            host: host.to_string(),
            port,
            username: username.to_string(),
        };

        if let Some(existing) = self
            .saved_connections
            .iter_mut()
            .find(|item| item.host == host && item.port == port && item.username == username)
        {
            *existing = new_item;
        } else {
            self.saved_connections.push(new_item);
            self.saved_connections
                .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }

        self.config_store
            .save_connections(&self.saved_connections)
            .with_context(|| {
                format!(
                    "No se pudo guardar conexiones en {}",
                    self.config_store.config_path().display()
                )
            })?;

        self.status_message = format!("Conexion guardada: {}", name);
        Ok(())
    }

    fn search_state_mut(&mut self) -> Option<&mut SearchState> {
        if let AppMode::Search(state) = &mut self.mode {
            Some(state)
        } else {
            None
        }
    }

    pub fn advance_search(&mut self) -> Result<()> {
        if let Some(state) = self.search_state_mut() {
            if state.finished {
                return Ok(());
            }

            let mut work = 0;
            while work < 4 {
                if let Some(dir) = state.pending_dirs.pop() {
                    let read_dir = fs::read_dir(&dir);
                    let entries = match read_dir {
                        Ok(entries) => entries,
                        Err(_) => {
                            state.processed_dirs += 1;
                            work += 1;
                            continue;
                        }
                    };

                    for entry in entries.flatten() {
                        let path = entry.path();
                        let metadata = match fs::symlink_metadata(&path) {
                            Ok(metadata) => metadata,
                            Err(_) => continue,
                        };

                        let file_type = metadata.file_type();
                        let is_dir = file_type.is_dir();
                        let is_symlink = file_type.is_symlink();
                        let is_executable = {
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                metadata.permissions().mode() & 0o111 != 0
                            }
                            #[cfg(not(unix))]
                            {
                                false
                            }
                        };

                        if let Some(name_os) = path.file_name() {
                            let name = name_os.to_string_lossy().to_string();

                            if is_dir && is_ignored_search_dir(&name) {
                                continue;
                            }

                            if matches_search(
                                &name,
                                &path,
                                is_dir,
                                is_executable,
                                &state.pattern,
                                &state.file_type,
                            ) {
                                if let Ok(found) = FileEntry::from_path(path.clone()) {
                                    state.entries.push(found);
                                }
                            }

                            if is_dir && !is_symlink {
                                state.pending_dirs.push(path);
                            }
                        }
                    }

                    state.processed_dirs += 1;
                    work += 1;

                    if state.entries.len() >= 500 {
                        break;
                    }
                } else {
                    state.finished = true;
                    break;
                }
            }

            if state.pending_dirs.is_empty() {
                state.finished = true;
            }
        }
        Ok(())
    }

    fn handle_search_input_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.search_input = None;
                self.status_message = "Busqueda cancelada".to_string();
            }
            KeyCode::Enter => {
                let Some(dialog) = self.search_input.take() else {
                    return Ok(());
                };
                let raw_input = dialog.input.trim();
                if raw_input.is_empty() {
                    self.search_input = Some(dialog);
                    self.status_message = "Ingrese un texto para buscar".to_string();
                    return Ok(());
                }
                let query = raw_input.to_string();
                let (pattern, file_type) = parse_search_query(&query);
                self.mode = AppMode::Search(SearchState {
                    root_dir: dialog.root_dir.clone(),
                    query,
                    pattern,
                    file_type,
                    entries: Vec::new(),
                    selected: 0,
                    pending_dirs: vec![dialog.root_dir],
                    processed_dirs: 0,
                    finished: false,
                });
                self.status_message = "Busqueda iniciada".to_string();
            }
            KeyCode::Backspace => {
                if let Some(dialog) = &mut self.search_input {
                    dialog.input.pop();
                }
            }
            KeyCode::Char(c) => {
                if !c.is_control() {
                    if let Some(dialog) = &mut self.search_input {
                        dialog.input.push(c);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::F(1) => {
                self.show_help = true;
            }
            KeyCode::Esc | KeyCode::Backspace => {
                self.mode = AppMode::Panels;
                self.status_message = "Busqueda cerrada".to_string();
            }
            KeyCode::Up => self.move_search_selection(-1),
            KeyCode::Down => self.move_search_selection(1),
            KeyCode::PageUp => self.move_search_selection(-(self.panel_step() as isize)),
            KeyCode::PageDown => self.move_search_selection(self.panel_step() as isize),
            KeyCode::Enter => self.open_search_selected()?,
            KeyCode::F(3) => self.open_search_selected_preview(),
            KeyCode::F(10) | KeyCode::Char('q') => self.open_confirmation(PendingAction::Quit),
            _ => {}
        }
        Ok(())
    }

    fn move_search_selection(&mut self, delta: isize) {
        if let Some(state) = self.search_state_mut() {
            if state.entries.is_empty() {
                state.selected = 0;
                return;
            }
            let next = state.selected as isize + delta;
            let upper = state.entries.len().saturating_sub(1) as isize;
            state.selected = next.clamp(0, upper) as usize;
        }
    }

    fn open_search_selected(&mut self) -> Result<()> {
        let state = match std::mem::replace(&mut self.mode, AppMode::Panels) {
            AppMode::Search(state) => state,
            other => {
                self.mode = other;
                return Ok(());
            }
        };

        let Some(entry) = state.entries.get(state.selected).cloned() else {
            self.mode = AppMode::Search(state);
            self.status_message = "No hay resultado seleccionado".to_string();
            return Ok(());
        };

        if entry.is_dir {
            {
                let panel = self.active_panel_mut();
                panel.cwd = entry.path;
                panel.selected = 0;
            }
            self.reload_active_panel()?;
            let cwd = self.active_panel().cwd.clone();
            self.status_message = format!("Directorio abierto: {}", cwd.display());
            return Ok(());
        }

        let parent = entry
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| state.root_dir.clone());
        {
            let panel = self.active_panel_mut();
            panel.cwd = parent;
            panel.selected = 0;
        }
        self.reload_active_panel()?;

        if let Some(index) = self
            .active_panel()
            .entries
            .iter()
            .position(|item| item.path == entry.path)
        {
            self.active_panel_mut().selected = index;
        }

        let cwd = self.active_panel().cwd.clone();
        self.status_message = format!(
            "Directorio abierto: {} (archivo seleccionado: {})",
            cwd.display(),
            entry.name
        );
        Ok(())
    }

    fn open_search_selected_preview(&mut self) {
        let state = match std::mem::replace(&mut self.mode, AppMode::Panels) {
            AppMode::Search(state) => state,
            other => {
                self.mode = other;
                return;
            }
        };

        let selected_entry = state.entries.get(state.selected).cloned();
        if let Some(entry) = selected_entry {
            if entry.is_dir {
                self.mode = AppMode::Search(state);
                self.status_message = format!("{} es un directorio; F3 solo abre archivos de texto", entry.name);
                return;
            }
            match ViewerState::open(&entry.path) {
                Ok(viewer) => {
                    self.mode = AppMode::Viewer(viewer);
                    self.status_message = format!("Visualizando: {}", entry.name);
                }
                Err(error) => {
                    self.mode = AppMode::Search(state);
                    self.status_message = format!("No se puede visualizar {}: {error}", entry.name);
                }
            }
        } else {
            self.mode = AppMode::Search(state);
        }
    }

    fn preview_selected(&mut self) {
        let selected = self.active_panel().selected_entry().cloned();
        if let Some(entry) = selected {
            if entry.is_dir {
                self.status_message = format!(
                    "{} es un directorio; F3 solo abre archivos de texto",
                    entry.name
                );
                return;
            }

            let backend = self.active_panel().backend.clone();
            let result = match backend {
                PanelBackend::Local => ViewerState::open(&entry.path),
                PanelBackend::Remote { connection_name } => {
                    let session = match self.remote_session_for(&connection_name) {
                        Ok(session) => session,
                        Err(error) => {
                            self.status_message = format!("No hay sesion remota: {error}");
                            return;
                        }
                    };

                    match session.read_file_bytes(&entry.path) {
                        Ok(bytes) => ViewerState::from_bytes(entry.path.clone(), bytes),
                        Err(error) => Err(error),
                    }
                }
            };

            match result {
                Ok(viewer) => {
                    self.mode = AppMode::Viewer(viewer);
                    self.status_message = format!("Visualizando: {}", entry.name);
                }
                Err(error) => {
                    self.status_message = format!("No se puede visualizar {}: {error}", entry.name);
                }
            }
        }
    }

    fn handle_viewer_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::F(1) => {
                self.show_help = true;
            }
            KeyCode::Esc | KeyCode::F(3) | KeyCode::Backspace => {
                self.mode = AppMode::Panels;
                self.status_message = "Volviendo a paneles".to_string();
            }
            KeyCode::Up => {
                if let AppMode::Viewer(viewer) = &mut self.mode {
                    viewer.scroll_up();
                }
            }
            KeyCode::Down => {
                if let AppMode::Viewer(viewer) = &mut self.mode {
                    viewer.scroll_down();
                }
            }
            KeyCode::PageUp => {
                if let AppMode::Viewer(viewer) = &mut self.mode {
                    for _ in 0..10 {
                        viewer.scroll_up();
                    }
                }
            }
            KeyCode::PageDown => {
                if let AppMode::Viewer(viewer) = &mut self.mode {
                    for _ in 0..10 {
                        viewer.scroll_down();
                    }
                }
            }
            KeyCode::F(10) | KeyCode::Char('q') => self.open_confirmation(PendingAction::Quit),
            _ => {}
        }
        Ok(())
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::F(1) => {
                self.show_help = false;
                self.status_message = "Ayuda cerrada".to_string();
            }
            KeyCode::F(10) | KeyCode::Char('q') => self.open_confirmation(PendingAction::Quit),
            _ => {}
        }

        Ok(())
    }

    fn handle_rename_input_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.rename_input = None;
                self.pending_rename = None;
                self.pending_overwrite = None;
                self.status_message = "Renombrado cancelado".to_string();
            }
            KeyCode::Enter => {
                let Some(dialog) = self.rename_input.take() else {
                    return Ok(());
                };
                let Some(plan) = self.build_rename_plan(dialog) else {
                    return Ok(());
                };
                let message = self.pending_rename_message(&plan);
                self.pending_rename = Some(plan);
                self.confirmation = Some(ConfirmationDialog {
                    action: PendingAction::Rename,
                    message,
                });
            }
            KeyCode::Backspace => {
                if let Some(dialog) = &mut self.rename_input {
                    dialog.input.pop();
                }
            }
            KeyCode::Char(c) => {
                if !c.is_control() {
                    if let Some(dialog) = &mut self.rename_input {
                        dialog.input.push(c);
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_mkdir_input_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mkdir_input = None;
                self.pending_mkdir = None;
                self.status_message = "Creacion de directorio cancelada".to_string();
            }
            KeyCode::Enter => {
                let Some(dialog) = self.mkdir_input.take() else {
                    return Ok(());
                };
                let raw_input = dialog.input.trim();
                if raw_input.is_empty() {
                    self.mkdir_input = Some(dialog);
                    self.status_message = "El nombre del directorio no puede estar vacio".to_string();
                    return Ok(());
                }

                let mut target = PathBuf::from(raw_input);
                if !target.is_absolute() {
                    target = dialog.base_dir.join(target);
                }

                self.pending_mkdir = Some(target.clone());
                self.confirmation = Some(ConfirmationDialog {
                    action: PendingAction::Mkdir,
                    message: format!("Crear directorio {}?", target.display()),
                });
            }
            KeyCode::Backspace => {
                if let Some(dialog) = &mut self.mkdir_input {
                    dialog.input.pop();
                }
            }
            KeyCode::Char(c) => {
                if !c.is_control() {
                    if let Some(dialog) = &mut self.mkdir_input {
                        dialog.input.push(c);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_confirmation_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.confirm_pending_action()?;
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                if self
                    .confirmation
                    .as_ref()
                    .is_some_and(|dialog| matches!(dialog.action, PendingAction::OverwriteConflict))
                {
                    self.confirmation = None;
                    self.handle_overwrite_decision(false)?;
                    return Ok(());
                }

                if self
                    .confirmation
                    .as_ref()
                    .is_some_and(|dialog| matches!(dialog.action, PendingAction::Rename | PendingAction::Copy))
                {
                    self.pending_rename = None;
                    self.pending_mkdir = None;
                    self.pending_overwrite = None;
                }

                if self
                    .confirmation
                    .as_ref()
                    .is_some_and(|dialog| matches!(dialog.action, PendingAction::DeleteSavedConnection))
                {
                    self.pending_saved_connection_delete = None;
                }
                self.confirmation = None;
                self.status_message = "Operacion cancelada".to_string();
            }
            _ => {}
        }
        Ok(())
    }

    fn open_confirmation(&mut self, action: PendingAction) {
        let items = self.active_panel().operation_targets();
        let list = if items.is_empty() {
            "sin elementos".to_string()
        } else {
            items.join(", ")
        };

        let message = match action {
            PendingAction::Copy => format!(
                "Copiar {} al directorio {}?",
                list,
                self.inactive_panel_cwd().display()
            ),
            PendingAction::Rename => format!("Renombrar o mover {}?", list),
            PendingAction::Mkdir => "Crear directorio?".to_string(),
            PendingAction::OverwriteConflict => {
                "Sobrescribir elemento existente?".to_string()
            }
            PendingAction::Delete => {
                let names = self.active_panel().operation_targets();
                let total = self.active_panel().operation_source_paths().len();
                let directories = self.active_panel().operation_directory_count();
                if total == 1 {
                    let name = names
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "elemento".to_string());
                    format!(
                        "Borrar '{}' (directorios incluidos: {})?",
                        name, directories
                    )
                } else {
                    format!(
                        "Borrar {} elemento(s), incluyendo {} directorio(s)?",
                        total, directories
                    )
                }
            }
            PendingAction::DeleteSavedConnection => {
                "Eliminar conexion guardada?".to_string()
            }
            PendingAction::Quit => "Salir de la aplicacion?".to_string(),
        };

        self.confirmation = Some(ConfirmationDialog { action, message });
    }

    fn confirm_pending_action(&mut self) -> Result<()> {
        let Some(dialog) = self.confirmation.take() else {
            return Ok(());
        };

        match dialog.action {
            PendingAction::Quit => {
                self.exit_dir = Some(self.compute_exit_directory());
                self.should_quit = true;
            }
            PendingAction::Copy => {
                self.confirm_copy()?;
            }
            PendingAction::Rename => {
                self.confirm_rename_move()?;
            }
            PendingAction::Mkdir => {
                self.confirm_mkdir()?;
            }
            PendingAction::OverwriteConflict => {
                self.handle_overwrite_decision(true)?;
            }
            PendingAction::Delete => {
                self.confirm_delete();
            }
            PendingAction::DeleteSavedConnection => {
                self.confirm_delete_saved_connection()?;
            }
        }

        Ok(())
    }

    fn confirm_delete_saved_connection(&mut self) -> Result<()> {
        let Some(index) = self.pending_saved_connection_delete.take() else {
            self.status_message = "No hay conexion pendiente de eliminar".to_string();
            return Ok(());
        };

        if index >= self.saved_connections.len() {
            self.status_message = "La conexion ya no existe".to_string();
            return Ok(());
        }

        let removed = self.saved_connections.remove(index);
        self.config_store
            .save_connections(&self.saved_connections)
            .with_context(|| {
                format!(
                    "No se pudo guardar conexiones en {}",
                    self.config_store.config_path().display()
                )
            })?;

        if let Some(dialog) = &mut self.remote_connection_input {
            if self.saved_connections.is_empty() {
                dialog.selected_saved = None;
                dialog.host.clear();
                dialog.port = "22".to_string();
                dialog.username.clear();
                dialog.password.clear();
                dialog.selected_field = RemoteConnectionField::Host;
            } else {
                let next = index.min(self.saved_connections.len().saturating_sub(1));
                dialog.selected_saved = Some(next);
                Self::apply_selected_saved_connection(dialog, &self.saved_connections, next);
            }
        }

        self.status_message = format!("Conexion eliminada: {}", removed.name);
        Ok(())
    }

    fn confirm_delete(&mut self) {
        let sources = self.active_panel().operation_source_paths();
        if sources.is_empty() {
            self.status_message = "No hay elementos validos para borrar".to_string();
            return;
        }

        let active_backend = self.active_panel().backend.clone();

        let mut deleted = 0usize;
        let mut failed = 0usize;

        for source in &sources {
            match self.remove_on_backend(&active_backend, source) {
                Ok(()) => deleted += 1,
                Err(_) => failed += 1,
            }
        }

        self.active_panel_mut().clear_marks();
        if let Err(error) = self.reload_panels() {
            self.status_message = format!(
                "Borrados {} y fallidos {}. Error al recargar paneles: {error}",
                deleted, failed
            );
            return;
        }

        self.status_message = format!(
            "Borrados {} y fallidos {} elemento(s)",
            deleted, failed
        );
    }

    fn confirm_copy(&mut self) -> Result<()> {
        if self.active_transfer.is_some() {
            self.status_message = "Ya hay una transferencia en curso".to_string();
            return Ok(());
        }

        let sources = self.active_panel().operation_source_paths();
        if sources.is_empty() {
            self.status_message = "No hay elementos validos para copiar".to_string();
            return Ok(());
        }

        let source_backend = self.active_panel().backend.clone();
        let destination_backend = self.inactive_panel().backend.clone();
        let destination_dir = self.inactive_panel_cwd().to_path_buf();

        if source_backend == PanelBackend::Local && destination_backend == PanelBackend::Local {
            self.pending_overwrite = Some(OverwriteBatchState {
                remaining_sources: sources,
                destination_dir,
                processed: 0,
                skipped: 0,
                current_conflict_source: None,
                operation: OverwriteOperation::Copy,
            });
            self.advance_overwrite_batch()?;
            return Ok(());
        }

        let source_transfer_backend = self.transfer_backend_from_panel_backend(&source_backend)?;
        let destination_transfer_backend =
            self.transfer_backend_from_panel_backend(&destination_backend)?;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let receiver = spawn_copy_worker(CopyJob {
            source_backend: source_transfer_backend,
            destination_backend: destination_transfer_backend,
            sources,
            destination_dir,
            cancel_flag: Arc::clone(&cancel_flag),
        });
        let source_panel = self.active_panel;
        self.active_transfer = Some(TransferState {
            receiver,
            cancel_flag,
            started_at: Instant::now(),
            source_panel,
            copied_bytes: 0,
            total_bytes: 0,
        });
        self.status_message = "Transferencia remota iniciada...".to_string();
        return Ok(());
    }

    fn open_mkdir_input(&mut self) {
        self.mkdir_input = Some(MkdirInputDialog {
            base_dir: self.active_panel().cwd.clone(),
            input: String::new(),
        });
        self.status_message = "F7: Ingrese nombre/ruta del directorio".to_string();
    }

    fn confirm_mkdir(&mut self) -> Result<()> {
        let Some(target) = self.pending_mkdir.take() else {
            self.status_message = "No hay creacion de directorio pendiente".to_string();
            return Ok(());
        };

        let backend = self.active_panel().backend.clone();

        if self.path_exists_on_backend(&backend, &target)? {
            self.status_message = format!("No se puede crear: {} ya existe", target.display());
            return Ok(());
        }

        self.mkdir_on_backend(&backend, &target)?;

        self.reload_panels()?;
        self.status_message = format!("Directorio creado: {}", target.display());
        Ok(())
    }

    fn open_rename_input(&mut self) {
        let sources = self.active_panel().operation_source_paths();
        if sources.is_empty() {
            self.status_message = "No hay elemento valido para renombrar".to_string();
            return;
        }
        let default_move_dir = self.inactive_panel_cwd().to_path_buf();
        let source_label = if sources.len() == 1 {
            sources[0].display().to_string()
        } else {
            format!("{} elementos seleccionados", sources.len())
        };

        self.rename_input = Some(RenameInputDialog {
            sources,
            source_label,
            default_move_dir,
            input: String::new(),
        });
        self.status_message =
            "F6: Enter mueve al otro panel; escriba para cambiar nombre".to_string();
    }

    fn build_rename_plan(&mut self, dialog: RenameInputDialog) -> Option<PendingRenamePlan> {
        let raw_input = dialog.input.trim();
        if raw_input.is_empty() {
            if dialog.sources.len() == 1 {
                let source = dialog.sources[0].clone();
                let Some(name) = source.file_name() else {
                    self.rename_input = Some(dialog);
                    self.status_message = "Origen invalido".to_string();
                    return None;
                };
                let name = name.to_owned();
                return Some(PendingRenamePlan::Single(RenamePlan {
                    source,
                    destination: dialog.default_move_dir.join(name),
                    destination_backend: self.inactive_panel().backend.clone(),
                }));
            }

            return Some(PendingRenamePlan::Multiple {
                sources: dialog.sources,
                destination_dir: dialog.default_move_dir,
                destination_backend: self.inactive_panel().backend.clone(),
            });
        }

        if dialog.sources.len() > 1 {
            self.rename_input = Some(dialog);
            self.status_message =
                "Con multiples, deje vacio y Enter para mover al panel opuesto".to_string();
            return None;
        }

        let source = dialog.sources[0].clone();
        let destination = {
            if raw_input.contains('/') || raw_input.contains('\\') {
                self.rename_input = Some(dialog);
                self.status_message =
                    "Para renombrar ingrese solo nombre (sin ruta)".to_string();
                return None;
            }

            let Some(parent) = source.parent() else {
                self.rename_input = Some(dialog);
                self.status_message = "Origen invalido".to_string();
                return None;
            };
            parent.join(raw_input)
        };

        Some(PendingRenamePlan::Single(RenamePlan {
            source,
            destination,
            destination_backend: self.active_panel().backend.clone(),
        }))
    }

    fn confirm_rename_move(&mut self) -> Result<()> {
        let Some(plan) = self.pending_rename.take() else {
            self.status_message = "No hay renombrado pendiente".to_string();
            return Ok(());
        };

        match plan {
            PendingRenamePlan::Single(plan) => {
                let source_backend = self.active_panel().backend.clone();

                if self.path_exists_on_backend(&plan.destination_backend, &plan.destination)? {
                    self.status_message = format!(
                        "No se puede mover: el destino {} ya existe",
                        plan.destination.display()
                    );
                    return Ok(());
                }

                let Some(parent) = plan.destination.parent() else {
                    self.status_message = "Destino invalido".to_string();
                    return Ok(());
                };

                if !self.path_exists_on_backend(&plan.destination_backend, parent)? {
                    self.status_message = format!(
                        "No se puede mover: el directorio {} no existe",
                        parent.display()
                    );
                    return Ok(());
                }

                self.move_between_backends(
                    &source_backend,
                    &plan.destination_backend,
                    &plan.source,
                    &plan.destination,
                )?;

                self.active_panel_mut().clear_marks();
                self.reload_panels()?;
                self.status_message = format!(
                    "Movido/renombrado: {} -> {}",
                    plan.source.display(),
                    plan.destination.display()
                );
            }
            PendingRenamePlan::Multiple {
                sources,
                destination_dir,
                destination_backend,
            } => {
                let source_backend = self.active_panel().backend.clone();

                if !self.path_exists_on_backend(&destination_backend, &destination_dir)? {
                    self.status_message = format!(
                        "No se puede mover: {} no es un directorio valido",
                        destination_dir.display()
                    );
                    return Ok(());
                }

                if source_backend == PanelBackend::Local && destination_backend == PanelBackend::Local {
                    self.pending_overwrite = Some(OverwriteBatchState {
                        remaining_sources: sources,
                        destination_dir,
                        processed: 0,
                        skipped: 0,
                        current_conflict_source: None,
                        operation: OverwriteOperation::Move,
                    });
                    self.advance_overwrite_batch()?;
                    return Ok(());
                }

                let mut moved = 0usize;
                let mut failed = 0usize;
                let mut skipped = 0usize;

                for source in &sources {
                    let Some(name) = source.file_name() else {
                        skipped += 1;
                        continue;
                    };
                    let destination = destination_dir.join(name);
                    if self.path_exists_on_backend(&destination_backend, &destination)? {
                        skipped += 1;
                        continue;
                    }

                    match self.move_between_backends(
                        &source_backend,
                        &destination_backend,
                        source,
                        &destination,
                    ) {
                        Ok(()) => moved += 1,
                        Err(_) => failed += 1,
                    }
                }

                self.active_panel_mut().clear_marks();
                self.reload_panels()?;
                self.status_message = format!(
                    "Movidos {} y omitidos {} (fallidos {}) elemento(s)",
                    moved, skipped, failed
                );
                return Ok(());
            }
        }

        Ok(())
    }

    fn pending_rename_message(&self, plan: &PendingRenamePlan) -> String {
        match plan {
            PendingRenamePlan::Single(plan) => format!(
                "Renombrar/mover {} a {}?",
                plan.source.display(),
                plan.destination.display()
            ),
            PendingRenamePlan::Multiple {
                sources,
                destination_dir,
                destination_backend: _,
            } => format!(
                "Mover {} elemento(s) a {}?",
                sources.len(),
                destination_dir.display()
            ),
        }
    }

    fn handle_overwrite_decision(&mut self, overwrite: bool) -> Result<()> {
        let Some(state) = &mut self.pending_overwrite else {
            self.status_message = "No hay sobreescritura pendiente".to_string();
            return Ok(());
        };

        let Some(source) = state.current_conflict_source.take() else {
            self.status_message = "No hay conflicto pendiente".to_string();
            return Ok(());
        };

        let Some(name) = source.file_name() else {
            state.skipped += 1;
            self.advance_overwrite_batch()?;
            return Ok(());
        };

        let destination = state.destination_dir.join(name);
        if overwrite {
            remove_path_recursive(&destination)?;
            apply_batch_operation(state.operation.clone(), &source, &destination)?;
            state.processed += 1;
        } else {
            state.skipped += 1;
        }

        self.advance_overwrite_batch()?;
        Ok(())
    }

    fn advance_overwrite_batch(&mut self) -> Result<()> {
        loop {
            let Some(state) = &mut self.pending_overwrite else {
                return Ok(());
            };

            if state.remaining_sources.is_empty() {
                let processed = state.processed;
                let skipped = state.skipped;
                let destination_dir = state.destination_dir.clone();
                let operation = state.operation.clone();
                self.pending_overwrite = None;

                self.active_panel_mut().clear_marks();
                self.reload_panels()?;
                let verb = match operation {
                    OverwriteOperation::Copy => "Copiados",
                    OverwriteOperation::Move => "Movidos",
                };
                self.status_message =
                    format!("{} {} y omitidos {} elemento(s) en {}", verb, processed, skipped, destination_dir.display());
                return Ok(());
            }

            let source = state.remaining_sources.remove(0);
            let Some(name) = source.file_name() else {
                state.skipped += 1;
                continue;
            };

            let destination = state.destination_for(&source).unwrap_or_else(|| state.destination_dir.join(name));
            if destination.exists() {
                state.current_conflict_source = Some(source);
                let prompt = match state.operation {
                    OverwriteOperation::Copy => "Sobrescribir copia",
                    OverwriteOperation::Move => "Sobrescribir movimiento",
                };
                self.confirmation = Some(ConfirmationDialog {
                    action: PendingAction::OverwriteConflict,
                    message: format!(
                        "El destino {} ya existe. {}?",
                        destination.display(),
                        prompt
                    ),
                });
                return Ok(());
            }

            apply_batch_operation(state.operation.clone(), &source, &destination)?;
            state.processed += 1;
        }
    }

    fn reload_panels(&mut self) -> Result<()> {
        self.reload_panel(ActivePanel::Left)?;
        self.reload_panel(ActivePanel::Right)?;
        Ok(())
    }

    fn inactive_panel_cwd(&self) -> &Path {
        match self.active_panel {
            ActivePanel::Left => &self.right.cwd,
            ActivePanel::Right => &self.left.cwd,
        }
    }

    fn panel_step(&self) -> usize {
        self.panel_page_size.max(1)
    }

    fn compute_exit_directory(&self) -> PathBuf {
        if self.active_panel().backend != PanelBackend::Local {
            if self.inactive_panel().backend == PanelBackend::Local {
                return self.inactive_panel().cwd.clone();
            }
            return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        }

        if let Some(entry) = self.active_panel().selected_entry() {
            if entry.is_dir {
                return entry.path.clone();
            }
        }
        self.active_panel().cwd.clone()
    }
}

fn read_dir_entries(path: &Path, sort_mode: SortMode, sort_order: SortOrder, show_hidden: bool) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    entries.push(FileEntry::parent_dir(path)?);

    let mut raw_entries = fs::read_dir(path)
        .with_context(|| format!("No se pudo leer el directorio {}", path.display()))?
        .collect::<io::Result<Vec<_>>>()?;

    raw_entries.retain(|entry| {
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy();
        show_hidden || !name.starts_with('.')
    });

    raw_entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let dir_order = a_is_dir.cmp(&b_is_dir).reverse();
        if dir_order != Ordering::Equal {
            return dir_order;
        }

        let order = match sort_mode {
            SortMode::Name => {
                let a_name = a.file_name().to_string_lossy().to_lowercase();
                let b_name = b.file_name().to_string_lossy().to_lowercase();
                a_name.cmp(&b_name)
            }
            SortMode::Size => {
                let a_size = a.metadata().map(|m| m.len()).unwrap_or(0);
                let b_size = b.metadata().map(|m| m.len()).unwrap_or(0);
                a_size.cmp(&b_size)
            }
            SortMode::Modified => {
                let a_modified = a.metadata().and_then(|m| m.modified()).ok();
                let b_modified = b.metadata().and_then(|m| m.modified()).ok();
                a_modified.cmp(&b_modified)
            }
            SortMode::Type => {
                let a_ext = a.path().extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                let b_ext = b.path().extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                a_ext.cmp(&b_ext)
            }
        };

        if sort_order == SortOrder::Descending {
            order.reverse()
        } else {
            order
        }
    });

    for entry in raw_entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("No se pudo leer metadata de {}", path.display()))?;
        #[cfg(unix)]
        let is_executable = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let is_executable = false;

        entries.push(FileEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            is_dir: metadata.is_dir(),
            is_executable,
            size_bytes: if metadata.is_file() {
                Some(metadata.len())
            } else {
                None
            },
            modified: metadata.modified().ok(),
        });
    }

    Ok(entries)
}

fn parse_search_query(raw: &str) -> (String, Option<String>) {
    let mut file_type = None;
    let mut pattern_parts = Vec::new();

    for token in raw.split_whitespace() {
        if let Some(ext) = token.strip_prefix("type:") {
            let ext = ext.trim_start_matches('.').to_lowercase();
            if !ext.is_empty() {
                file_type = Some(ext);
            }
        } else {
            pattern_parts.push(token);
        }
    }

    let pattern = if pattern_parts.is_empty() {
        "*".to_string()
    } else {
        pattern_parts.join(" ")
    };
    (pattern, file_type)
}

fn is_ignored_search_dir(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "bin" | "obj" | "target")
}

fn normalize_extension(ext: &str) -> String {
    ext.trim_start_matches('.').to_lowercase()
}

fn matches_search(name: &str, path: &Path, is_dir: bool, is_executable: bool, pattern: &str, file_type: &Option<String>) -> bool {
    let name = name.to_lowercase();
    let pattern = pattern.to_lowercase();
    let file_type_matches = match file_type.as_deref() {
        None => true,
        Some("dir") => is_dir,
        Some("exe") => !is_dir && is_executable,
        Some(ext) => {
            if is_dir {
                false
            } else {
                path.extension()
                    .map(|current| normalize_extension(&current.to_string_lossy()))
                    .as_deref()
                    == Some(ext)
            }
        }
    };

    if !file_type_matches {
        return false;
    }

    let uses_glob = pattern.contains('*') || pattern.contains('?') || pattern.contains('[');

    if uses_glob {
        match Pattern::new(&pattern) {
            Ok(pattern) => pattern.matches(&name),
            Err(_) => name.contains(&pattern),
        }
    } else {
        name.contains(&pattern)
    }
}
