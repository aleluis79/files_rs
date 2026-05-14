use std::{
    cmp::Ordering,
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use glob::Pattern;

use crate::{
    ops::{OverwriteBatchState, OverwriteOperation, apply_batch_operation, remove_path_recursive},
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
}

#[derive(Clone, Debug)]
pub enum PendingRenamePlan {
    Single(RenamePlan),
    Multiple {
        sources: Vec<PathBuf>,
        destination_dir: PathBuf,
    },
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    Copy,
    Rename,
    Mkdir,
    OverwriteConflict,
    Delete,
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
    pub cwd: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub marked: BTreeSet<PathBuf>,
    pub sort_mode: SortMode,
    pub sort_order: SortOrder,
    pub show_hidden: bool,
}

impl PanelState {
    pub fn new(cwd: PathBuf) -> Result<Self> {
        let mut panel = Self {
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
        self.entries = read_dir_entries(&self.cwd, self.sort_mode, self.sort_order, self.show_hidden)?;
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn cycle_sort_mode(&mut self) -> Result<()> {
        self.sort_mode = self.sort_mode.next();
        self.reload()
    }

    pub fn toggle_sort_order(&mut self) -> Result<()> {
        self.sort_order = self.sort_order.toggle();
        self.reload()
    }

    pub fn toggle_show_hidden(&mut self) -> Result<()> {
        self.show_hidden = !self.show_hidden;
        self.reload()
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
    pub rename_input: Option<RenameInputDialog>,
    pub mkdir_input: Option<MkdirInputDialog>,
    pub search_input: Option<SearchInputDialog>,
    pub pending_rename: Option<PendingRenamePlan>,
    pub pending_mkdir: Option<PathBuf>,
    pub pending_overwrite: Option<OverwriteBatchState>,
    pub confirmation: Option<ConfirmationDialog>,
    pub exit_dir: Option<PathBuf>,
    pub should_quit: bool,
    pub status_message: String,
}

impl App {
    pub fn new() -> Result<Self> {
        let cwd = std::env::current_dir().context("No se pudo obtener el directorio actual")?;
        Ok(Self {
            left: PanelState::new(cwd.clone())?,
            right: PanelState::new(cwd)?,
            active_panel: ActivePanel::Left,
            mode: AppMode::Panels,
            panel_page_size: 10,
            marquee_tick: 0,
            show_help: false,
            rename_input: None,
            mkdir_input: None,
            search_input: None,
            pending_rename: None,
            pending_mkdir: None,
            pending_overwrite: None,
            confirmation: None,
            exit_dir: None,
            should_quit: false,
            status_message: "Listo".to_string(),
        })
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

        if self.search_input.is_some() {
            return self.handle_search_input_key(key);
        }

        if matches!(self.mode, AppMode::Viewer(_)) {
            return self.handle_viewer_key(key);
        }

        if matches!(self.mode, AppMode::Search(_)) {
            return self.handle_search_key(key);
        }

        match key.code {
            KeyCode::F(2) => self.open_search_input(),
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
            KeyCode::F(5) => self.open_confirmation(PendingAction::Copy),
            KeyCode::F(6) => self.open_rename_input(),
            KeyCode::F(7) => self.open_mkdir_input(),
            KeyCode::F(8) => self.open_confirmation(PendingAction::Delete),
            KeyCode::F(9) => {
                let panel = self.active_panel_mut();
                panel.cycle_sort_mode()?;
                self.status_message = format!("Orden: {} {}", panel.sort_mode.label(), panel.sort_order.symbol());
            }
            KeyCode::F(4) => {
                let panel = self.active_panel_mut();
                panel.toggle_show_hidden()?;
                self.status_message = if panel.show_hidden {
                    "Ocultos visibles".to_string()
                } else {
                    "Ocultos ocultos".to_string()
                };
            }
            KeyCode::F(12) => {
                let panel = self.active_panel_mut();
                panel.toggle_sort_order()?;
                self.status_message = format!("Orden: {} {}", panel.sort_mode.label(), panel.sort_order.symbol());
            }
            KeyCode::F(10) | KeyCode::Char('q') => self.open_confirmation(PendingAction::Quit),
            _ => {}
        }
        Ok(())
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

    pub fn active_panel_mut(&mut self) -> &mut PanelState {
        match self.active_panel {
            ActivePanel::Left => &mut self.left,
            ActivePanel::Right => &mut self.right,
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
                let panel = self.active_panel_mut();
                panel.cwd = entry.path;
                panel.selected = 0;
                panel.reload()?;
            } else {
                self.status_message = format!("Seleccionado: {}", entry.name);
            }
        }
        Ok(())
    }

    fn go_parent(&mut self) -> Result<()> {
        let panel = self.active_panel_mut();
        if let Some(parent) = panel.cwd.parent() {
            panel.cwd = parent.to_path_buf();
            panel.selected = 0;
            panel.reload()?;
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

        let panel = self.active_panel_mut();
        if entry.is_dir {
            panel.cwd = entry.path;
            panel.selected = 0;
            panel.reload()?;
            self.status_message = format!("Directorio abierto: {}", panel.cwd.display());
            return Ok(());
        }

        let parent = entry
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| state.root_dir.clone());
        panel.cwd = parent;
        panel.selected = 0;
        panel.reload()?;

        if let Some(index) = panel.entries.iter().position(|item| item.path == entry.path) {
            panel.selected = index;
        }

        self.status_message = format!(
            "Directorio abierto: {} (archivo seleccionado: {})",
            panel.cwd.display(),
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

            match ViewerState::open(&entry.path) {
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
        }

        Ok(())
    }

    fn confirm_delete(&mut self) {
        let sources = self.active_panel().operation_source_paths();
        if sources.is_empty() {
            self.status_message = "No hay elementos validos para borrar".to_string();
            return;
        }

        let mut deleted = 0usize;
        let mut failed = 0usize;

        for source in &sources {
            match remove_path_recursive(source) {
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
        let sources = self.active_panel().operation_source_paths();
        if sources.is_empty() {
            self.status_message = "No hay elementos validos para copiar".to_string();
            return Ok(());
        }

        self.pending_overwrite = Some(OverwriteBatchState {
            remaining_sources: sources,
            destination_dir: self.inactive_panel_cwd().to_path_buf(),
            processed: 0,
            skipped: 0,
            current_conflict_source: None,
            operation: OverwriteOperation::Copy,
        });
        self.advance_overwrite_batch()?;
        Ok(())
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

        if target.exists() {
            self.status_message = format!("No se puede crear: {} ya existe", target.display());
            return Ok(());
        }

        fs::create_dir_all(&target)
            .with_context(|| format!("No se pudo crear directorio {}", target.display()))?;

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
                }));
            }

            return Some(PendingRenamePlan::Multiple {
                sources: dialog.sources,
                destination_dir: dialog.default_move_dir,
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
        }))
    }

    fn confirm_rename_move(&mut self) -> Result<()> {
        let Some(plan) = self.pending_rename.take() else {
            self.status_message = "No hay renombrado pendiente".to_string();
            return Ok(());
        };

        match plan {
            PendingRenamePlan::Single(plan) => {
                if plan.destination.exists() {
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

                if !parent.exists() {
                    self.status_message = format!(
                        "No se puede mover: el directorio {} no existe",
                        parent.display()
                    );
                    return Ok(());
                }

                fs::rename(&plan.source, &plan.destination).with_context(|| {
                    format!(
                        "No se pudo mover {} a {}",
                        plan.source.display(),
                        plan.destination.display()
                    )
                })?;

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
            } => {
                if !destination_dir.exists() || !destination_dir.is_dir() {
                    self.status_message = format!(
                        "No se puede mover: {} no es un directorio valido",
                        destination_dir.display()
                    );
                    return Ok(());
                }

                self.pending_overwrite = Some(OverwriteBatchState {
                    remaining_sources: sources,
                    destination_dir,
                    processed: 0,
                    skipped: 0,
                    current_conflict_source: None,
                    operation: OverwriteOperation::Move,
                });
                self.advance_overwrite_batch()?;
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
        self.left.reload()?;
        self.right.reload()?;
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
