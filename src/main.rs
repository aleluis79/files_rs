mod app;
mod config;
mod ops;
mod remote;
mod transfer;
mod ui;
mod viewer;

use std::{fs, io, path::Path, time::Duration};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::app::App;

fn main() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run_app(&mut terminal);
    restore_terminal(&mut terminal)?;
    let exit_dir = result?;
    handoff_exit_directory(&exit_dir)?;
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<std::path::PathBuf> {
    let mut app = App::new()?;

    while !app.should_quit {
        let area = terminal.size()?;
        let panel_visible_rows = area.height.saturating_sub(8) as usize;
        app.set_panel_page_size(panel_visible_rows);
        app.advance_marquee();
        app.advance_search()?;
        app.advance_transfer()?;

        terminal.draw(|frame| ui::render(frame, &app))?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key)?,
                Event::Mouse(mouse) => {
                    let left_width = terminal.size()?.width / 2;
                    app.handle_mouse(mouse, left_width);
                }
                Event::Resize(_, _) => {}
                Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
            }
        }
    }

    Ok(app.exit_directory())
}

fn handoff_exit_directory(exit_dir: &Path) -> Result<()> {
    if let Ok(file_path) = std::env::var("NCRS_CHDIR_FILE") {
        fs::write(&file_path, format!("{}\n", exit_dir.display()))
            .with_context(|| format!("No se pudo escribir {}", file_path))?;
    }

    let quoted = shell_quote(exit_dir);
    println!("Directorio de salida sugerido: {}", exit_dir.display());
    println!("Ejecuta en tu shell: cd {}", quoted);
    Ok(())
}

fn shell_quote(path: &Path) -> String {
    let raw = path.display().to_string();
    format!("'{}'", raw.replace('\'', "'\\''"))
}
