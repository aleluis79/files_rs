use chrono::{DateTime, Local};
use ratatui::{
    prelude::*,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
};
use users::get_current_username;

use crate::{
    app::{ActivePanel, App, AppMode, PanelBackend, PanelState, RemoteConnectionField, SearchState},
    theme::ThemeColors,
    viewer::ViewerState,
};

fn text_with_blinking_cursor<'a>(input: &'a str, tick: u64, theme: &ThemeColors) -> Line<'a> {
    let cursor_char = "▌";
    let cursor_style = if tick % 2 == 0 {
        Style::default()
            .fg(theme.header_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_dim)
    };

    Line::from(vec![
        Span::raw(input),
        Span::styled(cursor_char, cursor_style),
    ])
}

fn fit_suffix(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep = max_chars - 3;
    let start = chars.len().saturating_sub(keep);
    let mut out = String::from("...");
    out.extend(chars[start..].iter());
    out
}

fn marquee_window(text: &str, width: usize, tick: u64) -> String {
    if width == 0 {
        return String::new();
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return " ".repeat(width);
    }

    if chars.len() <= width {
        let mut out: String = chars.iter().collect();
        let missing = width.saturating_sub(out.chars().count());
        if missing > 0 {
            out.push_str(&" ".repeat(missing));
        }
        return out;
    }

    let gap = [' ', ' ', ' '];
    let mut cycle = chars.clone();
    cycle.extend(gap);

    let pause_ticks = 24usize;
    let phase_len = cycle.len() + pause_ticks;
    let phase_pos = (tick as usize) % phase_len;
    let offset = if phase_pos >= cycle.len() {
        cycle.len().saturating_sub(1)
    } else {
        phase_pos
    };

    let mut out = String::with_capacity(width);
    for i in 0..width {
        out.push(cycle[(offset + i) % cycle.len()]);
    }
    out
}

pub fn render(frame: &mut Frame, app: &App) {
    if let AppMode::Viewer(viewer) = &app.mode {
        render_viewer(frame, viewer, &app.status_message, &app.theme, app.marquee_tick);
        render_mkdir_input(frame, app);
        render_rename_input(frame, app);
        render_search_input(frame, app);
        render_remote_connection_input(frame, app);
        render_confirmation(frame, app);
        render_transfer_overlay(frame, app);
        render_audio_cache_overlay(frame, app);
        render_help(frame, app);
        render_capibara(frame, app);
        return;
    }

    if let AppMode::AudioPlayer = &app.mode {
        if let Some(player) = &app.background_audio {
            render_audio_player(frame, player, &app.status_message, &app.theme, app.marquee_tick);
        }
        render_mkdir_input(frame, app);
        render_rename_input(frame, app);
        render_search_input(frame, app);
        render_remote_connection_input(frame, app);
        render_confirmation(frame, app);
        render_transfer_overlay(frame, app);
        render_audio_cache_overlay(frame, app);
        render_help(frame, app);
        render_capibara(frame, app);
        return;
    }

    if let AppMode::Search(state) = &app.mode {
        render_search(frame, state, &app.status_message, &app.theme);
        render_mkdir_input(frame, app);
        render_rename_input(frame, app);
        render_search_input(frame, app);
        render_remote_connection_input(frame, app);
        render_confirmation(frame, app);
        render_transfer_overlay(frame, app);
        render_audio_cache_overlay(frame, app);
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
                .fg(app.theme.header_fg)
                .bg(app.theme.panel_bg)
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
        &app.theme,
    );
    render_panel(
        frame,
        panels[1],
        &app.right,
        app.active_panel == ActivePanel::Right,
        "Derecha",
        app.marquee_tick,
        &app.theme,
    );

    let current = app.active_panel();
    let username = get_current_username()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "desconocido".to_string());
    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    let fixed_prefix = format!(
        "{} | {} | usuario: {} | marcados: {} | ",
        current.cwd.display(),
        now,
        username,
        current.marked_count(),
    );
    let moving_text = format!(
        "F1 Ayuda F2 Buscar F3 Ver F4 Editar M Playlist F5 Copiar F6 Renombrar F7 Mkdir F8 Borrar F9 Orden F12 SCP Shift+F12 Desconectar H Ocultos Esc Cancela copia F10 Salir | {}",
        app.status_message
    );

    let footer_inner_width = layout[2].width.saturating_sub(2) as usize;
    let min_marquee_width = 28usize.min(footer_inner_width);
    let max_fixed_width = footer_inner_width.saturating_sub(min_marquee_width);
    let fixed_visible = fit_suffix(&fixed_prefix, max_fixed_width);
    let marquee_width = footer_inner_width.saturating_sub(fixed_visible.chars().count());
    let moving_visible = marquee_window(&moving_text, marquee_width, app.marquee_tick);
    let footer_text = format!("{}{}", fixed_visible, moving_visible);

    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(app.theme.status_fg).bg(app.theme.panel_bg))
        .block(Block::default().borders(Borders::ALL).title("Estado"));
    frame.render_widget(footer, layout[2]);

    render_mkdir_input(frame, app);
    render_rename_input(frame, app);
    render_search_input(frame, app);
    render_remote_connection_input(frame, app);
    render_confirmation(frame, app);
    render_transfer_overlay(frame, app);
    render_audio_cache_overlay(frame, app);
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
        .style(Style::default().fg(app.theme.header_fg).bg(app.theme.panel_bg).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.text_accent))
                .title("SCP"),
        )
        .centered();
    frame.render_widget(Clear, area);
    frame.render_widget(title, gauge_area[0]);

    let progress = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progreso"))
        .gauge_style(Style::default().fg(app.theme.gauge_fill).bg(app.theme.gauge_bg))
        .ratio(ratio)
        .label(format!("{:.1}%", ratio * 100.0));
    frame.render_widget(progress, gauge_area[1]);

    let stats = Paragraph::new(format!(
        "{:.1}/{:.1} MiB | {:.1} MiB/s",
        copied, total, speed
    ))
    .style(Style::default().fg(app.theme.text_normal).bg(app.theme.panel_bg))
    .block(Block::default().borders(Borders::ALL).title("Velocidad"));
    frame.render_widget(stats, gauge_area[2]);

    let hint = Paragraph::new("Esc para cancelar transferencia")
        .style(Style::default().fg(app.theme.text_error).bg(app.theme.panel_bg))
        .block(Block::default().borders(Borders::ALL).title("Control"));
    frame.render_widget(hint, gauge_area[3]);
}

fn render_audio_cache_overlay(frame: &mut Frame, app: &App) {
    let Some(cache) = &app.active_audio_cache else {
        return;
    };

    let area = centered_rect(frame.area(), 64, 30);
    let ratio = if cache.total_items == 0 {
        0.0
    } else {
        (cache.cached_items as f64 / cache.total_items as f64).clamp(0.0, 1.0)
    };
    let spinner_frames = ["|", "/", "-", "\\"];
    let spinner = spinner_frames[(app.marquee_tick as usize) % spinner_frames.len()];

    let overlay = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);

    frame.render_widget(Clear, area);

    let title = Paragraph::new(format!(
        "{} Cacheando audio remoto",
        spinner
    ))
    .style(
        Style::default()
            .fg(app.theme.header_fg)
            .bg(app.theme.panel_bg)
            .add_modifier(Modifier::BOLD),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.text_accent))
            .title("SCP Audio"),
    )
    .centered();
    frame.render_widget(title, overlay[0]);

    let progress = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progreso"))
        .gauge_style(Style::default().fg(app.theme.gauge_fill).bg(app.theme.gauge_bg))
        .ratio(ratio)
        .label(format!("{}/{}", cache.cached_items, cache.total_items));
    frame.render_widget(progress, overlay[1]);

    let info = Paragraph::new(format!(
        "Archivo seleccionado: {}",
        cache.selected_label
    ))
    .style(Style::default().fg(app.theme.text_normal).bg(app.theme.panel_bg))
    .block(Block::default().borders(Borders::ALL).title("Detalle"));
    frame.render_widget(info, overlay[2]);

    let hint = Paragraph::new("Esc para cancelar")
        .style(Style::default().fg(app.theme.text_error).bg(app.theme.panel_bg))
        .block(Block::default().borders(Borders::ALL).title("Control"));
    frame.render_widget(hint, overlay[3]);
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
        Style::default().fg(app.theme.header_fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.theme.text_normal)
    };
    let port_style = if matches!(dialog.selected_field, RemoteConnectionField::Port) {
        Style::default().fg(app.theme.header_fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.theme.text_normal)
    };
    let user_style = if matches!(dialog.selected_field, RemoteConnectionField::Username) {
        Style::default().fg(app.theme.header_fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.theme.text_normal)
    };
    let pass_style = if matches!(dialog.selected_field, RemoteConnectionField::Password) {
        Style::default().fg(app.theme.header_fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.theme.text_normal)
    };
    let save_style = if matches!(dialog.selected_field, RemoteConnectionField::Save) {
        Style::default().fg(app.theme.header_fg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.theme.text_normal)
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
            Style::default().fg(app.theme.text_accent),
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
            Style::default().fg(app.theme.text_dim),
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
                    Style::default().fg(app.theme.text_success)
                } else {
                    Style::default().fg(app.theme.text_dim)
                },
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Tab/Shift+Tab cambia campo | Espacio alterna guardar"));
    lines.push(Line::from("Delete elimina conexion guardada (con confirmacion)"));
    lines.push(Line::from("Enter guarda y conecta | Esc cancelar"));

    let overlay = Paragraph::new(Text::from(lines))
        .style(Style::default().fg(app.theme.text_normal).bg(app.theme.panel_bg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.text_accent))
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
        text_with_blinking_cursor(&dialog.input, app.marquee_tick, &app.theme),
        Line::from(""),
        Line::from("Enter confirmar, Esc cancelar, Backspace borrar"),
    ]);

    let overlay = Paragraph::new(content)
        .style(Style::default().fg(app.theme.text_normal).bg(app.theme.panel_bg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.text_success))
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
    theme: &ThemeColors,
) {
    let border_style = if is_active {
        Style::default()
            .fg(theme.border_active)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.border_inactive)
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
                Style::default().bg(theme.selected_bg).fg(theme.selected_fg)
            } else {
                Style::default().fg(theme.panel_fg).bg(theme.panel_bg)
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
    let list = List::new(items)
        .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg))
        .block(
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

fn syntax_highlight_line(line: &str, extension: &str, theme: &ThemeColors) -> Vec<Span<'static>> {
    if extension == "md" {
        if line.trim_start().starts_with('#') {
            return vec![Span::styled(
                line.to_string(),
                Style::default()
                    .fg(theme.header_fg)
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
            spans.push(Span::styled(string, Style::default().fg(theme.text_success)));
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
            spans.push(Span::styled(number, Style::default().fg(theme.text_warning)));
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
                spans.push(Span::styled(token.to_string(), Style::default().fg(theme.text_accent).add_modifier(Modifier::BOLD)));
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
        spans.push(Span::styled(comment.to_string(), Style::default().fg(theme.text_dim).add_modifier(Modifier::ITALIC)));
    }

    spans
}

fn push_rendered_segment(
    spans: &mut Vec<Span<'static>>,
    segment: &str,
    selected: bool,
    extension: &str,
    theme: &ThemeColors,
) {
    if segment.is_empty() {
        return;
    }

    if selected {
        spans.push(Span::styled(
            segment.to_string(),
            Style::default().bg(theme.selected_bg).fg(theme.selected_fg),
        ));
    } else {
        spans.extend(syntax_highlight_line(segment, extension, theme));
    }
}

fn render_editor_line(
    line: &str,
    cursor_col: usize,
    extension: &str,
    theme: &ThemeColors,
    is_cursor_line: bool,
    tick: u64,
    selection_range: Option<(usize, usize)>,
) -> Line<'static> {
    let chars = line.chars().collect::<Vec<_>>();
    let char_count = chars.len();
    let cursor_col = cursor_col.min(char_count);

    let selection_start = selection_range.map(|(start, _)| start).unwrap_or(usize::MAX);
    let selection_end = selection_range.map(|(_, end)| end).unwrap_or(0);

    let mut spans = Vec::new();
    let mut current_segment = String::new();
    let mut current_selected = false;
    let mut cursor_inserted = false;

    for (idx, ch) in chars.iter().enumerate() {
        if is_cursor_line && idx == cursor_col && !cursor_inserted {
            if !current_segment.is_empty() {
                push_rendered_segment(
                    &mut spans,
                    &current_segment,
                    current_selected,
                    extension,
                    theme,
                );
                current_segment.clear();
            }

            let cursor_style = if tick % 2 == 0 {
                Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_dim)
            };
            spans.push(Span::styled("▌", cursor_style));
            cursor_inserted = true;
        }

        let selected = selection_range.is_some() && selection_start <= idx && idx < selection_end;
        if selected != current_selected {
            if !current_segment.is_empty() {
                push_rendered_segment(
                    &mut spans,
                    &current_segment,
                    current_selected,
                    extension,
                    theme,
                );
                current_segment.clear();
            }
            current_selected = selected;
        }

        current_segment.push(*ch);
    }

    if !current_segment.is_empty() {
        push_rendered_segment(&mut spans, &current_segment, current_selected, extension, theme);
    }

    if is_cursor_line && !cursor_inserted && cursor_col == char_count {
        let cursor_style = if tick % 2 == 0 {
            Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_dim)
        };
        spans.push(Span::styled("▌", cursor_style));
    }

    if spans.is_empty() {
        return Line::from(vec![Span::raw("")]);
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_editor_line_highlights_the_selected_range_on_the_cursor_line() {
        let theme = ThemeColors::dark();
        let line = render_editor_line("abc", 1, "", &theme, true, 0, Some((1, 2)));

        assert!(line.spans.iter().any(|span| {
            span.content.contains('b') && span.style.bg == Some(theme.selected_bg)
        }));
    }
}

fn render_viewer(frame: &mut Frame, viewer: &ViewerState, status_message: &str, theme: &ThemeColors, tick: u64) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let header = Paragraph::new(format!("{}: {}", if viewer.is_editing() { "Editor" } else { "Visor" }, viewer.path.display()))
        .style(
            Style::default()
                .fg(theme.header_fg)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if viewer.is_editing() { "F4 / Enter / Backspace editar | Esc/F3 guardar o descartar" } else { "F3 / Esc para volver | F4 editar" }),
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
                Style::default().fg(theme.text_dim),
            ));
            let is_cursor_line = viewer.is_editing() && offset + index == viewer.cursor.0;
            let cursor_col = if is_cursor_line { viewer.cursor.1 } else { 0 };
            let visible_line = if viewer.scroll_x > 0 {
                line.chars().skip(viewer.scroll_x).collect::<String>()
            } else {
                line.clone()
            };
            let selection_range = viewer.selection_anchor.and_then(|anchor| {
                let cursor = viewer.cursor;
                if anchor == cursor {
                    None
                } else {
                    let (start, end) = if (anchor.0, anchor.1) <= (cursor.0, cursor.1) {
                        (anchor, cursor)
                    } else {
                        (cursor, anchor)
                    };
                    if start.0 == offset + index && start.0 == end.0 {
                        Some((start.1, end.1))
                    } else if start.0 == offset + index {
                        Some((start.1, line.chars().count()))
                    } else if end.0 == offset + index {
                        Some((0, end.1))
                    } else if start.0 < offset + index && offset + index < end.0 {
                        Some((0, line.chars().count()))
                    } else {
                        None
                    }
                }
            });
            let line_with_cursor = render_editor_line(
                &visible_line,
                if is_cursor_line {
                    cursor_col.saturating_sub(viewer.scroll_x)
                } else {
                    0
                },
                &extension,
                theme,
                is_cursor_line,
                tick,
                selection_range,
            );
            spans.extend(line_with_cursor.spans);
            Line::from(spans)
        })
        .collect::<Vec<_>>();

    let body = Paragraph::new(content).block(Block::default().borders(Borders::ALL).title("Contenido"));
    frame.render_widget(body, layout[1]);

    let current_line = if viewer.is_editing() {
        viewer.cursor.0.saturating_add(1)
    } else {
        0
    };

    let current_column = if viewer.is_editing() {
        viewer.cursor.1.saturating_add(1)
    } else {
        0
    };

    let total_columns = if viewer.is_editing() {
        viewer
            .lines
            .get(viewer.cursor.0)
            .map(|line| line.chars().count().max(1))
            .unwrap_or(1)
    } else {
        viewer
            .lines
            .first()
            .map(|line| line.chars().count().max(1))
            .unwrap_or(1)
    };

    let footer_text = if viewer.is_editing() {
        format!(
            "linea {} de {} | col {} de {} | F1 Ayuda F3/Esc Volver F4 Editar | {}",
            current_line,
            viewer.lines.len(),
            current_column,
            total_columns,
            status_message
        )
    } else {
        format!(
            "lineas {} | cols {} | F1 Ayuda F3/Esc Volver F4 Editar | {}",
            viewer.lines.len(),
            total_columns,
            status_message
        )
    };

    let footer = Paragraph::new(footer_text)
    .style(Style::default().fg(theme.status_fg))
    .block(Block::default().borders(Borders::ALL).title("Estado"));
    frame.render_widget(footer, layout[2]);
}

fn render_audio_player(
    frame: &mut Frame,
    player: &crate::audio::AudioPlayerState,
    status_message: &str,
    theme: &ThemeColors,
    tick: u64,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let header = Paragraph::new(format!("Reproductor: {}", player.path.display()))
        .style(
            Style::default()
                .fg(theme.header_fg)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Espacio pausar/reanudar | S detener | R reiniciar | ↑/↓ o N/P siguiente/anterior | ←/→ seek | L loop on/off | Esc/F3/M volver"),
        );
    frame.render_widget(header, layout[0]);

    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(layout[1]);

    let elapsed = player.position();
    let elapsed_text = format!(
        "{:02}:{:02}",
        elapsed.as_secs() / 60,
        elapsed.as_secs() % 60
    );
    let total_text = player
        .total_duration()
        .map(|total| format!("{:02}:{:02}", total.as_secs() / 60, total.as_secs() % 60))
        .unwrap_or_else(|| "--:--".to_string());
    let metadata = player.metadata();
    let title = metadata
        .title
        .as_deref()
        .unwrap_or("(sin titulo)");
    let artist = metadata
        .artist
        .as_deref()
        .unwrap_or("(sin artista)");
    let album = metadata
        .album
        .as_deref()
        .unwrap_or("(sin album)");
    let genre = metadata
        .genre
        .as_deref()
        .unwrap_or("(sin genero)");
    let spectrum = build_spectrum_line(
        44,
        player.position().as_secs_f64() + tick as f64 * 0.03,
        player.status_label() == "Reproduciendo",
        player.current_track_number(),
    );

    let body_text = Text::from(vec![
        Line::from(vec![
            Span::styled("Estado: ", Style::default().fg(theme.text_dim)),
            Span::styled(player.status_label(), Style::default().fg(theme.text_accent).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Tema: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!("{} / {}", player.current_track_number(), player.total_tracks()),
                Style::default().fg(theme.text_normal),
            ),
        ]),
        Line::from(vec![
            Span::styled("Archivo: ", Style::default().fg(theme.text_dim)),
            Span::styled(player.current_track_name(), Style::default().fg(theme.text_normal)),
        ]),
        Line::from(vec![
            Span::styled("Titulo: ", Style::default().fg(theme.text_dim)),
            Span::styled(title, Style::default().fg(theme.text_normal)),
        ]),
        Line::from(vec![
            Span::styled("Artista: ", Style::default().fg(theme.text_dim)),
            Span::styled(artist, Style::default().fg(theme.text_normal)),
        ]),
        Line::from(vec![
            Span::styled("Album: ", Style::default().fg(theme.text_dim)),
            Span::styled(album, Style::default().fg(theme.text_normal)),
        ]),
        Line::from(vec![
            Span::styled("Genero: ", Style::default().fg(theme.text_dim)),
            Span::styled(genre, Style::default().fg(theme.text_normal)),
        ]),
        Line::from(vec![
            Span::styled("Tiempo: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!("{} / {}", elapsed_text, total_text),
                Style::default().fg(theme.text_normal),
            ),
        ]),
        Line::from(vec![
            Span::styled("Loop: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                if player.loop_enabled() { "ON" } else { "OFF" },
                Style::default().fg(theme.text_normal),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Espectro: ", Style::default().fg(theme.text_dim)),
            Span::styled(spectrum, Style::default().fg(theme.text_accent)),
        ]),
        Line::from(""),
        Line::from("Formatos compatibles: mp3, wav, flac, ogg, m4a, aac, opus"),
    ]);

    let body = Paragraph::new(body_text)
        .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg))
        .block(Block::default().borders(Borders::ALL).title("Audio"));
    frame.render_widget(body, body_layout[0]);

    let current_idx = player.current_track_index();
    let playlist_items = player
        .playlist_track_names()
        .iter()
        .enumerate()
        .map(|(idx, name)| {
            let style = if idx == current_idx {
                Style::default().bg(theme.selected_bg).fg(theme.selected_fg)
            } else {
                Style::default().fg(theme.panel_fg).bg(theme.panel_bg)
            };

            ListItem::new(format!("{:>2}. {}", idx + 1, name)).style(style)
        })
        .collect::<Vec<_>>();

    let playlist = List::new(playlist_items)
        .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg))
        .block(Block::default().borders(Borders::ALL).title("Playlist"));
    frame.render_widget(playlist, body_layout[1]);

    let footer = Paragraph::new(format!(
        "F1 Ayuda F3/Esc/M Volver Espacio Pausa/Reanuda S Stop R Reiniciar ↑/↓ o N/P Tema ←/→ Seek 10s L Loop | {}",
        status_message
    ))
    .style(Style::default().fg(theme.status_fg))
    .block(Block::default().borders(Borders::ALL).title("Estado"));
    frame.render_widget(footer, layout[2]);
}

fn build_spectrum_line(columns: usize, t: f64, active: bool, seed: usize) -> String {
    let levels = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut output = String::with_capacity(columns);
    let base_gain = if active { 1.0 } else { 0.2 };
    let seed = seed as f64 * 0.17;

    for i in 0..columns {
        let x = i as f64;
        let wave_a = ((x * 0.22 + t * 2.4 + seed).sin() * 0.5) + 0.5;
        let wave_b = ((x * 0.11 - t * 1.6 + seed * 0.7).cos() * 0.5) + 0.5;
        let wave_c = ((x * 0.31 + t * 0.8).sin() * 0.5) + 0.5;
        let combined = ((wave_a * 0.5) + (wave_b * 0.35) + (wave_c * 0.15)) * base_gain;
        let idx = (combined * (levels.len() as f64 - 1.0)).round() as usize;
        output.push(levels[idx.min(levels.len() - 1)]);
    }

    output
}

fn render_confirmation(frame: &mut Frame, app: &App) {
    let Some(message) = app.confirmation_message() else {
        return;
    };

    let area = centered_rect(frame.area(), 60, 20);
    let overlay = Paragraph::new(format!("{}\n\nEnter/Y confirmar, Esc/N cancelar", message))
        .style(Style::default().fg(app.theme.text_normal).bg(app.theme.panel_bg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.text_error))
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
        text_with_blinking_cursor(&dialog.input, app.marquee_tick, &app.theme),
        Line::from(""),
        Line::from("Enter confirmar, Esc cancelar, Backspace borrar"),
    ]);

    let overlay = Paragraph::new(content)
        .style(Style::default().fg(app.theme.text_normal).bg(app.theme.panel_bg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.text_accent))
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
        text_with_blinking_cursor(&dialog.input, app.marquee_tick, &app.theme),
        Line::from(""),
        Line::from("Enter para buscar, Esc cancelar, Backspace borrar"),
    ]);

    let overlay = Paragraph::new(content)
        .style(Style::default().fg(app.theme.text_normal).bg(app.theme.panel_bg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.text_warning))
                .title("F2 Buscar archivos"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(overlay, area);
}

fn render_search(frame: &mut Frame, state: &SearchState, status_message: &str, theme: &ThemeColors) {
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
                .fg(theme.header_fg)
                .bg(theme.panel_bg)
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
        .gauge_style(Style::default().fg(theme.text_error).bg(theme.gauge_bg))
        .ratio(state.progress_fraction());
    frame.render_widget(progress, layout[1]);

    let items = if state.entries.is_empty() {
        vec![ListItem::new("No se encontraron resultados").style(Style::default().fg(theme.text_dim))]
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
                    Style::default().bg(theme.selected_bg).fg(theme.selected_fg)
                } else {
                    Style::default().fg(theme.panel_fg).bg(theme.panel_bg)
                };
                ListItem::new(format!("{} {} - {}", marker, entry.name, entry.path.display()))
                    .style(style)
            })
            .collect()
    };

    let body = List::new(items)
        .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg))
        .block(
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
        .style(Style::default().fg(theme.status_fg).bg(theme.panel_bg))
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
        "F4  Editar el archivo activo",
        "M   Abrir playlist de audio de la carpeta del archivo seleccionado",
        "H  Alternar archivos ocultos",
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
        "F3 en audio: reproduce solo el archivo seleccionado",
        "M en audio: abre/reabre reproductor; misma carpeta continua, carpeta nueva recarga playlist",
        "Usa comodines: *.rs, foo*bar",
        "Backspace sube al directorio padre o cierra busqueda",
        "",
        "Esc o F1 para cerrar esta ayuda",
    ]
    .join("\n");

    let overlay = Paragraph::new(help_text)
        .style(Style::default().fg(app.theme.text_normal).bg(app.theme.panel_bg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.header_fg))
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
        .style(Style::default().fg(app.theme.text_warning))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.text_error))
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
