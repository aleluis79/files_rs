use chrono::Local;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use users::get_current_username;

use crate::{
    app::{ActivePanel, App, AppMode, PanelState},
    viewer::ViewerState,
};

pub fn render(frame: &mut Frame, app: &App) {
    if let AppMode::Viewer(viewer) = &app.mode {
        render_viewer(frame, viewer, &app.status_message);
        render_rename_input(frame, app);
        render_confirmation(frame, app);
        render_help(frame, app);
        return;
    }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let header = Paragraph::new("Norton Commander RS")
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).title("Titulo"))
        .centered();
    frame.render_widget(header, layout[0]);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[1]);

    render_panel(
        frame,
        panels[0],
        &app.left,
        app.active_panel == ActivePanel::Left,
        "Izquierda",
    );
    render_panel(
        frame,
        panels[1],
        &app.right,
        app.active_panel == ActivePanel::Right,
        "Derecha",
    );

    let current = app.active_panel();
    let username = get_current_username()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "desconocido".to_string());
    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    let footer_text = format!(
        "{} | {} | usuario: {} | marcados: {} | F1 Ayuda F3 Ver F5 Copiar F6 Renombrar F8 Borrar F10 Salir | {}",
        current.cwd.display(),
        now,
        username,
        current.marked_count(),
        app.status_message
    );
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::ALL).title("Estado"));
    frame.render_widget(footer, layout[2]);

    render_rename_input(frame, app);
    render_confirmation(frame, app);
    render_help(frame, app);
}

fn render_panel(frame: &mut Frame, area: Rect, panel: &PanelState, is_active: bool, title: &str) {
    let border_style = if is_active {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let visible_rows = area.height.saturating_sub(2) as usize;
    let start_index = if visible_rows == 0 {
        0
    } else if panel.selected >= visible_rows {
        panel.selected + 1 - visible_rows
    } else {
        0
    };

    let items = panel
        .entries
        .iter()
        .enumerate()
        .skip(start_index)
        .take(visible_rows.max(1))
        .map(|(index, entry)| {
            let mark = if panel.marked.contains(&entry.path) {
                '*'
            } else {
                ' '
            };
            let marker = if entry.is_dir {
                "[DIR]"
            } else if entry.is_executable {
                "[EXE]"
            } else {
                "     "
            };
            let style = if index == panel.selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };
            ListItem::new(format!("{} {} {}", mark, marker, entry.name)).style(style)
        })
        .collect::<Vec<_>>();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(format!("{}: {}", title, panel.cwd.display())),
    );
    frame.render_widget(list, area);
}

fn render_viewer(frame: &mut Frame, viewer: &ViewerState, status_message: &str) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let header = Paragraph::new(format!("Visor: {}", viewer.path.display()))
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("F3 / Esc para volver"),
        );
    frame.render_widget(header, layout[0]);

    let height = layout[1].height.saturating_sub(2) as usize;
    let max_offset = viewer.lines.len().saturating_sub(height.max(1));
    let offset = viewer.scroll.min(max_offset);

    let content = viewer
        .lines
        .iter()
        .skip(offset)
        .take(height.max(1))
        .enumerate()
        .map(|(index, line)| Line::from(format!("{:>4} {}", offset + index + 1, line)))
        .collect::<Vec<_>>();

    let body =
        Paragraph::new(content).block(Block::default().borders(Borders::ALL).title("Contenido"));
    frame.render_widget(body, layout[1]);

    let footer = Paragraph::new(format!(
        "linea {} de {} | F1 Ayuda F3/Esc Volver | {}",
        offset.saturating_add(1),
        viewer.lines.len(),
        status_message
    ))
    .style(Style::default().fg(Color::Cyan))
    .block(Block::default().borders(Borders::ALL).title("Estado"));
    frame.render_widget(footer, layout[2]);
}

fn render_confirmation(frame: &mut Frame, app: &App) {
    let Some(message) = app.confirmation_message() else {
        return;
    };

    let area = centered_rect(frame.area(), 60, 20);
    let overlay = Paragraph::new(format!("{}\n\nEnter/Y confirmar, Esc/N cancelar", message))
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .title("Confirmacion"),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(Clear, area);
    frame.render_widget(overlay, area);
}

fn render_rename_input(frame: &mut Frame, app: &App) {
    let Some(dialog) = &app.rename_input else {
        return;
    };

    let area = centered_rect(frame.area(), 72, 30);
    let content = format!(
        "Origen: {}\n\nDestino por defecto (Enter): {}\n\nEscriba para cambiar nombre en el directorio actual:\n{}\n\nEnter confirmar, Esc cancelar, Backspace borrar",
        dialog.source.display(),
        dialog.default_move_dir.display(),
        dialog.input
    );

    let overlay = Paragraph::new(content)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightBlue))
                .title("F6 Renombrar/Mover"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(overlay, area);
}

fn render_help(frame: &mut Frame, app: &App) {
    if !app.show_help {
        return;
    }

    let area = centered_rect(frame.area(), 70, 55);
    let help_text = [
        "Ayuda de Teclas de Funcion",
        "",
        "F1  Ayuda",
        "F3  Visualizar archivo de texto/markdown",
        "F5  Copiar al panel opuesto (con confirmacion)",
        "F6  Enter mueve al panel opuesto; escribir cambia nombre",
        "F8  Borrar (pendiente)",
        "F10 Salir (con confirmacion)",
        "",
        "Tab cambia panel activo",
        "PageUp/PageDown desplaza por paginas en paneles",
        "Espacio marca/desmarca seleccion",
        "Enter abre directorio",
        "Backspace sube al directorio padre",
        "",
        "Esc o F1 para cerrar esta ayuda",
    ]
    .join("\n");

    let overlay = Paragraph::new(help_text)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title("Ayuda"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(overlay, area);
}

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}
