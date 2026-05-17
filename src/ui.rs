use chrono::{DateTime, Local};
use ratatui::{
    prelude::*,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
};
use users::get_current_username;

use crate::{
    app::{ActivePanel, App, AppMode, PanelBackend, PanelState, RemoteConnectionField, SearchState},
    viewer::ViewerState,
};

fn text_with_blinking_cursor<'a>(input: &'a str, tick: u64) -> Line<'a> {
    let cursor_char = "▌";
    let cursor_style = if tick % 2 == 0 {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };

    Line::from(vec![
        Span::raw(input),
        Span::styled(cursor_char, cursor_style),
    ])
}

pub fn render(frame: &mut Frame, app: &App) {
    if let AppMode::Viewer(viewer) = &app.mode {
        render_viewer(frame, viewer, &app.status_message);
        render_mkdir_input(frame, app);
        render_rename_input(frame, app);
        render_search_input(frame, app);
        render_remote_connection_input(frame, app);
        render_confirmation(frame, app);
        render_transfer_overlay(frame, app);
        render_help(frame, app);
        render_capibara(frame, app);
        return;
    }

    if let AppMode::Search(state) = &app.mode {
        render_search(frame, state, &app.status_message);
        render_mkdir_input(frame, app);
        render_rename_input(frame, app);
        render_search_input(frame, app);
        render_remote_connection_input(frame, app);
        render_confirmation(frame, app);
        render_transfer_overlay(frame, app);
        render_help(frame, app);
        render_capibara(frame, app);
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

    let header = Paragraph::new("Files RS")
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
        app.marquee_tick,
    );
    render_panel(
        frame,
        panels[1],
        &app.right,
        app.active_panel == ActivePanel::Right,
        "Derecha",
        app.marquee_tick,
    );

    let current = app.active_panel();
    let username = get_current_username()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "desconocido".to_string());
    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    let footer_text = format!(
        "{} | {} | usuario: {} | marcados: {} | F1 Ayuda F2 Buscar F3 Ver F5 Copiar F6 Renombrar F7 Mkdir F8 Borrar F9 Orden F12 SCP Shift+F12 Desconectar F4 Ocultos Esc Cancela copia F10 Salir | {}",
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

    render_mkdir_input(frame, app);
    render_rename_input(frame, app);
    render_search_input(frame, app);
    render_remote_connection_input(frame, app);
    render_confirmation(frame, app);
    render_transfer_overlay(frame, app);
    render_help(frame, app);
    render_capibara(frame, app);
}

fn render_transfer_overlay(frame: &mut Frame, app: &App) {
    let Some(transfer) = &app.active_transfer else {
        return;
    };

    let area = centered_rect(frame.area(), 72, 32);
    let copied = transfer.copied_bytes as f64 / 1024.0 / 1024.0;
    let total = transfer.total_bytes as f64 / 1024.0 / 1024.0;
    let ratio = if transfer.total_bytes == 0 {
        0.0
    } else {
        (transfer.copied_bytes as f64 / transfer.total_bytes as f64).clamp(0.0, 1.0)
    };
    let elapsed = transfer.started_at.elapsed().as_secs_f64().max(0.001);
    let speed = copied / elapsed;

    let gauge_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);

    let title = Paragraph::new("Transferencia remota en curso")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightCyan))
                .title("SCP"),
        )
        .centered();
    frame.render_widget(Clear, area);
    frame.render_widget(title, gauge_area[0]);

    let progress = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progreso"))
        .gauge_style(Style::default().fg(Color::Green).bg(Color::Black))
        .ratio(ratio)
        .label(format!("{:.1}%", ratio * 100.0));
    frame.render_widget(progress, gauge_area[1]);

    let stats = Paragraph::new(format!(
        "{:.1}/{:.1} MiB | {:.1} MiB/s",
        copied, total, speed
    ))
    .style(Style::default().fg(Color::White))
    .block(Block::default().borders(Borders::ALL).title("Velocidad"));
    frame.render_widget(stats, gauge_area[2]);

    let hint = Paragraph::new("Esc para cancelar transferencia")
        .style(Style::default().fg(Color::LightRed))
        .block(Block::default().borders(Borders::ALL).title("Control"));
    frame.render_widget(hint, gauge_area[3]);
}

fn render_remote_connection_input(frame: &mut Frame, app: &App) {
    let Some(dialog) = &app.remote_connection_input else {
        return;
    };

    let cursor_char = if app.marquee_tick % 2 == 0 { "|" } else { " " };

    let area = centered_rect(frame.area(), 72, 56);
    let selected_label = dialog
        .selected_saved
        .and_then(|index| app.saved_connections.get(index))
        .map(|item| item.name.clone())
        .unwrap_or_else(|| "ninguna".to_string());

    let host_style = if matches!(dialog.selected_field, RemoteConnectionField::Host) {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let port_style = if matches!(dialog.selected_field, RemoteConnectionField::Port) {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let user_style = if matches!(dialog.selected_field, RemoteConnectionField::Username) {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let pass_style = if matches!(dialog.selected_field, RemoteConnectionField::Password) {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let save_style = if matches!(dialog.selected_field, RemoteConnectionField::Save) {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let masked_password = "*".repeat(dialog.password.chars().count());
    let save_mark = if dialog.save_connection { "[x]" } else { "[ ]" };

    let host_value = if matches!(dialog.selected_field, RemoteConnectionField::Host) {
        format!("{}{}", dialog.host, cursor_char)
    } else {
        dialog.host.clone()
    };
    let port_value = if matches!(dialog.selected_field, RemoteConnectionField::Port) {
        format!("{}{}", dialog.port, cursor_char)
    } else {
        dialog.port.clone()
    };
    let user_value = if matches!(dialog.selected_field, RemoteConnectionField::Username) {
        format!("{}{}", dialog.username, cursor_char)
    } else {
        dialog.username.clone()
    };
    let pass_value = if matches!(dialog.selected_field, RemoteConnectionField::Password) {
        format!("{}{}", masked_password, cursor_char)
    } else {
        masked_password
    };
    let save_value = if matches!(dialog.selected_field, RemoteConnectionField::Save) {
        format!("{} {}", save_mark, cursor_char)
    } else {
        save_mark.to_string()
    };

    let mut lines = vec![
        Line::from(Span::raw("Conexiones guardadas (Up/Down para cargar):")),
        Line::from(Span::styled(
            format!("Seleccionada: {}", selected_label),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Host: ", host_style),
            Span::styled(host_value, host_style),
        ]),
        Line::from(vec![
            Span::styled("Puerto: ", port_style),
            Span::styled(port_value, port_style),
        ]),
        Line::from(vec![
            Span::styled("Usuario: ", user_style),
            Span::styled(user_value, user_style),
        ]),
        Line::from(vec![
            Span::styled("Contrasena: ", pass_style),
            Span::styled(pass_value, pass_style),
        ]),
        Line::from(vec![
            Span::styled("Guardar conexion: ", save_style),
            Span::styled(save_value, save_style),
        ]),
        Line::from(""),
    ];

    if app.saved_connections.is_empty() {
        lines.push(Line::from(Span::styled(
            "No hay conexiones guardadas aun",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::raw("Conexiones:")));
        for (index, item) in app.saved_connections.iter().enumerate().take(6) {
            let prefix = if Some(index) == dialog.selected_saved {
                ">"
            } else {
                " "
            };
            lines.push(Line::from(Span::styled(
                format!("{} {}", prefix, item.name),
                if Some(index) == dialog.selected_saved {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Gray)
                },
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Tab/Shift+Tab cambia campo | Espacio alterna guardar"));
    lines.push(Line::from("Delete elimina conexion guardada (con confirmacion)"));
    lines.push(Line::from("Enter guarda y conecta | Esc cancelar"));

    let overlay = Paragraph::new(Text::from(lines))
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightCyan))
                .title("F12 Conexion SCP"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(overlay, area);
}

fn render_mkdir_input(frame: &mut Frame, app: &App) {
    let Some(dialog) = &app.mkdir_input else {
        return;
    };

    let area = centered_rect(frame.area(), 68, 28);
    let content = Text::from(vec![
        Line::from(Span::raw(format!("Base: {}", dialog.base_dir.display()))),
        Line::from(""),
        Line::from("Nombre o ruta de directorio:"),
        text_with_blinking_cursor(&dialog.input, app.marquee_tick),
        Line::from(""),
        Line::from("Enter confirmar, Esc cancelar, Backspace borrar"),
    ]);

    let overlay = Paragraph::new(content)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightGreen))
                .title("F7 Crear Directorio"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(overlay, area);
}

fn render_panel(
    frame: &mut Frame,
    area: Rect,
    panel: &PanelState,
    is_active: bool,
    title: &str,
    marquee_tick: u64,
) {
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
    } else {
        let half = visible_rows / 2;
        let max_start = panel.entries.len().saturating_sub(visible_rows);
        panel.selected.saturating_sub(half).min(max_start)
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
            let inner_width = area.width.saturating_sub(2) as usize;
            let fixed_without_name = 35usize;
            let name_width = inner_width.saturating_sub(fixed_without_name).max(6);
            let selected_active = is_active && index == panel.selected;
            let name_text = visible_name(&entry.name, name_width, selected_active, marquee_tick);
            let size_text = if entry.is_dir {
                "<DIR>".to_string()
            } else {
                format_size(entry.size_bytes.unwrap_or(0))
            };
            let date_text = entry
                .modified
                .map(format_modified)
                .unwrap_or_else(|| "---- -- -- --:--".to_string());
            ListItem::new(format!(
                "{} {} {} {:>9} {}",
                mark, marker, name_text, size_text, date_text
            ))
            .style(style)
        })
        .collect::<Vec<_>>();

    let hidden_label = if panel.show_hidden { "H:ON" } else { "H:OFF" };
    let backend_label = match &panel.backend {
        PanelBackend::Local => "LOCAL".to_string(),
        PanelBackend::Remote { connection_name } => format!("SCP {}", connection_name),
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(format!(
                "{} [{}]: {} [{} {} {}]",
                title,
                backend_label,
                panel.cwd.display(),
                panel.sort_mode.label(),
                panel.sort_order.symbol(),
                hidden_label
            )),
    );
    frame.render_widget(list, area);
}

fn visible_name(name: &str, width: usize, selected_active: bool, tick: u64) -> String {
    if width == 0 {
        return String::new();
    }

    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= width {
        return format!("{:<width$}", name, width = width);
    }

    if selected_active {
        return scrolling_window(&chars, width, tick);
    }

    truncate_name(&chars, width)
}

fn truncate_name(chars: &[char], width: usize) -> String {
    if width <= 3 {
        return ".".repeat(width);
    }

    let keep = width - 3;
    let mut out = chars.iter().take(keep).collect::<String>();
    out.push_str("...");
    out
}

fn scrolling_window(chars: &[char], width: usize, tick: u64) -> String {
    let max_offset = chars.len().saturating_sub(width);
    if max_offset == 0 {
        return chars.iter().collect();
    }

    // Faster movement (1 tick per step) with a small pause at both ends.
    let pause_ticks = 10usize;
    let move_ticks = max_offset;
    let cycle = pause_ticks + move_ticks + pause_ticks;
    let phase = (tick as usize) % cycle;

    let start = if phase < pause_ticks {
        0
    } else if phase < pause_ticks + move_ticks {
        phase - pause_ticks
    } else {
        max_offset
    };

    let mut strip = Vec::with_capacity(chars.len());
    strip.extend_from_slice(chars);
    (0..width)
        .map(|i| strip[(start + i) % strip.len()])
        .collect()
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.1}G", b / GB)
    } else if b >= MB {
        format!("{:.1}M", b / MB)
    } else if b >= KB {
        format!("{:.1}K", b / KB)
    } else {
        format!("{}B", bytes)
    }
}

fn format_modified(time: std::time::SystemTime) -> String {
    let dt: DateTime<Local> = time.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn syntax_highlight_line(line: &str, extension: &str) -> Vec<Span<'static>> {
    if extension == "md" {
        if line.trim_start().starts_with('#') {
            return vec![Span::styled(
                line.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )];
        }
    }

    let comment_marker = match extension {
        "rs" | "c" | "cpp" | "h" | "hpp" | "java" | "js" | "ts" | "jsx" | "tsx" | "go" | "dart" => "//",
        "py" | "sh" | "bash" | "zsh" | "toml" | "yaml" | "yml" => "#",
        "sql" => "--",
        _ => "",
    };

    let mut spans = Vec::new();
    let (code_part, comment_part) = if !comment_marker.is_empty() {
        let mut in_single = false;
        let mut in_double = false;
        let mut idx = 0;
        let mut comment_start = None;

        while idx < line.len() {
            let ch = line[idx..].chars().next().unwrap();
            if ch == '"' && !in_single {
                in_double = !in_double;
            } else if ch == '\'' && !in_double {
                in_single = !in_single;
            }

            if !in_single && !in_double && line[idx..].starts_with(comment_marker) {
                comment_start = Some(idx);
                break;
            }

            idx += ch.len_utf8();
        }

        if let Some(start) = comment_start {
            (&line[..start], Some(&line[start..]))
        } else {
            (line, None)
        }
    } else {
        (line, None)
    };

    let keywords: &[&str] = if extension == "rs" {
        &["fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "match", "if", "else", "loop", "while", "for", "in", "return", "const", "static", "true", "false", "self", "super", "crate", "async", "await", "unsafe", "type", "where", "as"]
    } else if extension == "py" {
        &["def", "class", "import", "from", "as", "if", "elif", "else", "for", "while", "try", "except", "finally", "with", "return", "yield", "lambda", "pass", "break", "continue", "True", "False", "None", "and", "or", "not", "is", "in", "global", "nonlocal", "assert", "async", "await"]
    } else if matches!(extension, "js" | "ts" | "jsx" | "tsx") {
        &["function", "const", "let", "var", "if", "else", "for", "while", "switch", "case", "return", "async", "await", "import", "from", "export", "class", "extends", "new", "try", "catch", "finally", "true", "false", "null", "undefined", "this"]
    } else if extension == "json" {
        &["true", "false", "null"]
    } else if matches!(extension, "toml" | "yaml" | "yml") {
        &["true", "false", "null"]
    } else {
        &[]
    };

    let mut idx = 0;
    let mut buffer = String::new();

    fn flush_buffer(spans: &mut Vec<Span<'static>>, buffer: &mut String, style: Style) {
        if !buffer.is_empty() {
            spans.push(Span::styled(buffer.clone(), style));
            buffer.clear();
        }
    }

    while idx < code_part.len() {
        let ch = code_part[idx..].chars().next().unwrap();

        if ch == '"' || ch == '\'' || (ch == '`' && matches!(extension, "sh" | "bash" | "zsh" | "js" | "ts" | "jsx" | "tsx")) {
            flush_buffer(&mut spans, &mut buffer, Style::default());
            let quote = ch;
            let mut string = String::new();
            string.push(ch);
            idx += ch.len_utf8();
            let mut escaped = false;
            while idx < code_part.len() {
                let c = code_part[idx..].chars().next().unwrap();
                string.push(c);
                idx += c.len_utf8();
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == quote {
                    break;
                }
            }
            spans.push(Span::styled(string, Style::default().fg(Color::Green)));
            continue;
        }

        if ch.is_ascii_digit() {
            flush_buffer(&mut spans, &mut buffer, Style::default());
            let mut number = String::new();
            while idx < code_part.len() {
                let c = code_part[idx..].chars().next().unwrap();
                if c.is_ascii_digit() || c == '.' || c == 'x' || c == 'X' || c == 'b' || c == 'B' || c == '_' {
                    number.push(c);
                    idx += c.len_utf8();
                } else {
                    break;
                }
            }
            spans.push(Span::styled(number, Style::default().fg(Color::Yellow)));
            continue;
        }

        if ch.is_alphanumeric() || ch == '_' {
            let start = idx;
            while idx < code_part.len() {
                let c = code_part[idx..].chars().next().unwrap();
                if c.is_alphanumeric() || c == '_' {
                    idx += c.len_utf8();
                } else {
                    break;
                }
            }
            let token = &code_part[start..idx];
            if keywords.iter().any(|kw| *kw == token) {
                flush_buffer(&mut spans, &mut buffer, Style::default());
                spans.push(Span::styled(token.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
            } else {
                buffer.push_str(token);
            }
            continue;
        }

        buffer.push(ch);
        idx += ch.len_utf8();
    }

    flush_buffer(&mut spans, &mut buffer, Style::default());

    if let Some(comment) = comment_part {
        spans.push(Span::styled(comment.to_string(), Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC)));
    }

    spans
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

    let extension = viewer
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    let content = viewer
        .lines
        .iter()
        .skip(offset)
        .take(height.max(1))
        .enumerate()
        .map(|(index, line)| {
            let mut spans = Vec::new();
            spans.push(Span::styled(
                format!("{:>4} ", offset + index + 1),
                Style::default().fg(Color::DarkGray),
            ));
            spans.extend(syntax_highlight_line(line, &extension));
            Line::from(spans)
        })
        .collect::<Vec<_>>();

    let body = Paragraph::new(content).block(Block::default().borders(Borders::ALL).title("Contenido"));
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
    let content = Text::from(vec![
        Line::from(Span::raw(format!("Origen: {}", dialog.source_label))),
        Line::from(""),
        Line::from(Span::raw(format!("Destino por defecto (Enter): {}", dialog.default_move_dir.display()))),
        Line::from(""),
        Line::from("Si hay un solo elemento, escriba para cambiar nombre en el directorio actual:"),
        text_with_blinking_cursor(&dialog.input, app.marquee_tick),
        Line::from(""),
        Line::from("Enter confirmar, Esc cancelar, Backspace borrar"),
    ]);

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

fn render_search_input(frame: &mut Frame, app: &App) {
    let Some(dialog) = &app.search_input else {
        return;
    };

    let area = centered_rect(frame.area(), 68, 24);
    let content = Text::from(vec![
        Line::from(Span::raw(format!("Buscar en: {}", dialog.root_dir.display()))),
        Line::from(""),
        Line::from("Ingrese texto o patron (*.rs, foo*bar) y opcional type:<ext>:"),
        text_with_blinking_cursor(&dialog.input, app.marquee_tick),
        Line::from(""),
        Line::from("Enter para buscar, Esc cancelar, Backspace borrar"),
    ]);

    let overlay = Paragraph::new(content)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightMagenta))
                .title("F2 Buscar archivos"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(overlay, area);
}

fn render_search(frame: &mut Frame, state: &SearchState, status_message: &str) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let header = Paragraph::new(format!("Buscar: '{}' en {}", state.query, state.root_dir.display()))
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).title("Busqueda"))
        .centered();
    frame.render_widget(header, layout[0]);

    let progress = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if state.finished {
                    "Busqueda completada"
                } else {
                    "Progreso de busqueda"
                }),
        )
        .gauge_style(Style::default().fg(Color::Magenta).bg(Color::Black))
        .ratio(state.progress_fraction());
    frame.render_widget(progress, layout[1]);

    let items = if state.entries.is_empty() {
        vec![ListItem::new("No se encontraron resultados").style(Style::default().fg(Color::Gray))]
    } else {
        state
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let marker = if entry.is_dir {
                    "[DIR]"
                } else if entry.is_executable {
                    "[EXE]"
                } else {
                    "     "
                };
                let style = if index == state.selected {
                    Style::default().bg(Color::Blue).fg(Color::White)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{} {} - {}", marker, entry.name, entry.path.display()))
                    .style(style)
            })
            .collect()
    };

    let body = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Resultados ({})", state.entries.len())),
    );
    frame.render_widget(body, layout[2]);

    let footer_text = if state.finished {
        format!(
            "{} | Enter ir al directorio / F3 ver contenido / Esc volver | {} resultados",
            status_message,
            state.entries.len()
        )
    } else {
        format!(
            "{} | Buscando {} directorio(s) pendientes | Esc detiene busqueda | {} resultados",
            status_message,
            state.pending_dirs.len(),
            state.entries.len()
        )
    };

    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::ALL).title("Estado"));
    frame.render_widget(footer, layout[3]);
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
        "F2  Buscar archivos desde el directorio activo",
        "F3  Visualizar archivo de texto/markdown",
        "F4  Alternar archivos ocultos",
        "F5  Copiar al panel opuesto (con confirmacion)",
        "F6  Enter mueve al panel opuesto; escribir cambia nombre",
        "F7  Crear directorio (con entrada y confirmacion)",
        "F8  Borrar seleccion (con confirmacion)",
        "F9  Cambiar modo de orden",
        "Shift+F9 Cambiar direccion del orden",
        "F12 Conexion SCP (conexiones guardadas)",
        "Shift+F12 Desconecta SCP y vuelve a local",
        "Delete Elimina conexion SCP guardada (con confirmacion)",
        "Esc Cancela transferencia en curso",
        "F10 Salir (con confirmacion)",
        "",
        "Tab cambia panel activo",
        "PageUp/PageDown desplaza por paginas en paneles",
        "Espacio marca/desmarca seleccion",
        "Enter abre directorio o selecciona archivo en su carpeta",
        "F3 ver archivo de texto en resultados",
        "Usa comodines: *.rs, foo*bar",
        "Backspace sube al directorio padre o cierra busqueda",
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

fn render_capibara(frame: &mut Frame, app: &App) {
    if !app.show_capibara {
        return;
    }

    let area = centered_rect(frame.area(), 30, 50);
    let content = Text::from(vec![
        Line::from(r#"                    -:     :."#),
        Line::from(r#"                .:-=++===-=--"#),
        Line::from(r#"            .:-====++=--=++=-"#),
        Line::from(r#"          .---===++++-:-+++++-"#),
        Line::from(r#"          :=---++++++++++++==+:"#),
        Line::from(r#"          :++++++++++++++++==++:"#),
        Line::from(r#"           --==++++++++++==+++++=:"#),
        Line::from(r#"            .:-=====+++++++++++++++-."#),
        Line::from(r#"                 -===++++++++++++++++-"#),
        Line::from(r#"                 =++++++++++++++++=+++="#),
        Line::from(r#"                 :+++++++++++++=+++++++-"#),
        Line::from(r#"                  :==+=+++++++=++++++++-"#),
        Line::from(r#"                  .----+++=====++++++==."#),
        Line::from(r#"                 .::: .---------=====-."#),
        Line::from(r#"                     .:::  ..::....."#),
        Line::from(r#""#),
        Line::from(r#"                 Capibara en pantalla!"#),
        Line::from(r#""#),
        Line::from(r#"          Presiona Shift+F1 o Esc para cerrar"#),
    ]);

    let overlay = Paragraph::new(content)
        .style(Style::default().fg(Color::LightYellow))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::LightMagenta))
                .title("Easter Egg"),
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
