use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

use crate::viewer::ViewerState;

pub enum AppMode {
    Panels,
    Viewer(ViewerState),
}

#[derive(Clone, Debug)]
pub struct RenameInputDialog {
    pub source: PathBuf,
    pub default_move_dir: PathBuf,
    pub input: String,
}

#[derive(Clone, Debug)]
pub struct RenamePlan {
    pub source: PathBuf,
    pub destination: PathBuf,
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    Copy,
    Rename,
    Delete,
    Quit,
}

#[derive(Clone, Debug)]
pub struct ConfirmationDialog {
    pub action: PendingAction,
    pub message: String,
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
        })
    }
}

#[derive(Clone, Debug)]
pub struct PanelState {
    pub cwd: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub marked: BTreeSet<PathBuf>,
}

impl PanelState {
    pub fn new(cwd: PathBuf) -> Result<Self> {
        let mut panel = Self {
            cwd,
            entries: Vec::new(),
            selected: 0,
            marked: BTreeSet::new(),
        };
        panel.reload()?;
        Ok(panel)
    }

    pub fn reload(&mut self) -> Result<()> {
        self.entries = read_dir_entries(&self.cwd)?;
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
    pub show_help: bool,
    pub rename_input: Option<RenameInputDialog>,
    pub pending_rename: Option<RenamePlan>,
    pub confirmation: Option<ConfirmationDialog>,
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
            show_help: false,
            rename_input: None,
            pending_rename: None,
            confirmation: None,
            should_quit: false,
            status_message: "Listo".to_string(),
        })
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.rename_input.is_some() {
            return self.handle_rename_input_key(key);
        }

        if self.show_help {
            return self.handle_help_key(key);
        }

        if self.confirmation.is_some() {
            return self.handle_confirmation_key(key);
        }

        if matches!(self.mode, AppMode::Viewer(_)) {
            return self.handle_viewer_key(key);
        }

        match key.code {
            KeyCode::F(1) => {
                self.show_help = true;
            }
            KeyCode::F(10) | KeyCode::Char('q') => self.open_confirmation(PendingAction::Quit),
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
            KeyCode::F(8) => self.open_confirmation(PendingAction::Delete),
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
                self.status_message = "Renombrado cancelado".to_string();
            }
            KeyCode::Enter => {
                let Some(dialog) = self.rename_input.take() else {
                    return Ok(());
                };
                let Some(plan) = self.build_rename_plan(dialog) else {
                    return Ok(());
                };
                let message = format!(
                    "Renombrar/mover {} a {}?",
                    plan.source.display(),
                    plan.destination.display()
                );
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

    fn handle_confirmation_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.confirm_pending_action()?;
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                if self
                    .confirmation
                    .as_ref()
                    .is_some_and(|dialog| matches!(dialog.action, PendingAction::Rename))
                {
                    self.pending_rename = None;
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
            PendingAction::Delete => format!("Borrar {}?", list),
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
                self.should_quit = true;
            }
            PendingAction::Copy => {
                self.confirm_copy()?;
            }
            PendingAction::Rename => {
                self.confirm_rename_move()?;
            }
            PendingAction::Delete => {
                self.status_message = format!("Borrado confirmado: {}", dialog.message);
            }
        }

        Ok(())
    }

    fn confirm_copy(&mut self) -> Result<()> {
        let sources = self.active_panel().operation_source_paths();
        if sources.is_empty() {
            self.status_message = "No hay elementos validos para copiar".to_string();
            return Ok(());
        }

        let destination_dir = self.inactive_panel_cwd().to_path_buf();
        let mut copied = 0usize;

        for source in &sources {
            let Some(name) = source.file_name() else {
                continue;
            };
            let destination = destination_dir.join(name);

            if destination.exists() {
                self.status_message = format!(
                    "No se copio {}: ya existe en destino",
                    destination.display()
                );
                return Ok(());
            }

            copy_path_recursive(source, &destination)?;
            copied += 1;
        }

        self.active_panel_mut().clear_marks();
        self.reload_panels()?;
        self.status_message = format!("Copiados {} elemento(s) a {}", copied, destination_dir.display());
        Ok(())
    }

    fn open_rename_input(&mut self) {
        let sources = self.active_panel().operation_source_paths();
        if sources.is_empty() {
            self.status_message = "No hay elemento valido para renombrar".to_string();
            return;
        }

        if sources.len() > 1 {
            self.status_message =
                "F6 actualmente permite un solo elemento; desmarque multiples".to_string();
            return;
        }

        let source = sources[0].clone();
        let default_move_dir = self.inactive_panel_cwd().to_path_buf();

        self.rename_input = Some(RenameInputDialog {
            source,
            default_move_dir,
            input: String::new(),
        });
        self.status_message =
            "F6: Enter mueve al otro panel; escriba para cambiar nombre".to_string();
    }

    fn build_rename_plan(&mut self, dialog: RenameInputDialog) -> Option<RenamePlan> {
        let raw_input = dialog.input.trim();
        let destination = if raw_input.is_empty() {
            let Some(name) = dialog.source.file_name() else {
                self.rename_input = Some(dialog);
                self.status_message = "Origen invalido".to_string();
                return None;
            };
            dialog.default_move_dir.join(name)
        } else {
            if raw_input.contains('/') || raw_input.contains('\\') {
                self.rename_input = Some(dialog);
                self.status_message =
                    "Para renombrar ingrese solo nombre (sin ruta)".to_string();
                return None;
            }

            let Some(parent) = dialog.source.parent() else {
                self.rename_input = Some(dialog);
                self.status_message = "Origen invalido".to_string();
                return None;
            };
            parent.join(raw_input)
        };

        Some(RenamePlan {
            source: dialog.source,
            destination,
        })
    }

    fn confirm_rename_move(&mut self) -> Result<()> {
        let Some(plan) = self.pending_rename.take() else {
            self.status_message = "No hay renombrado pendiente".to_string();
            return Ok(());
        };

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
        Ok(())
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
}

fn read_dir_entries(path: &Path) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    entries.push(FileEntry::parent_dir(path)?);

    let mut raw_entries = fs::read_dir(path)
        .with_context(|| format!("No se pudo leer el directorio {}", path.display()))?
        .collect::<io::Result<Vec<_>>>()?;

    raw_entries.sort_by_key(|entry| entry.file_name());

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
        });
    }

    Ok(entries)
}

fn copy_path_recursive(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("No se pudo leer metadata de {}", source.display()))?;

    if metadata.is_dir() {
        fs::create_dir(destination)
            .with_context(|| format!("No se pudo crear {}", destination.display()))?;
        for entry in fs::read_dir(source)
            .with_context(|| format!("No se pudo listar {}", source.display()))?
        {
            let entry = entry?;
            let child_source = entry.path();
            let child_destination = destination.join(entry.file_name());
            copy_path_recursive(&child_source, &child_destination)?;
        }
        return Ok(());
    }

    fs::copy(source, destination).with_context(|| {
        format!(
            "No se pudo copiar {} a {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}
