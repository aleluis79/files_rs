mod app;
mod audio;
mod config;
mod ops;
mod remote;
mod theme;
mod transfer;
mod ui;
mod viewer;

use std::{
    fs, io,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{app::App, audio::linux_audio_dependency_warning};

fn main() -> Result<()> {
    suppress_alsa_stderr_noise();
    let mut terminal = setup_terminal()?;
    let result = run_app(&mut terminal);
    restore_terminal(&mut terminal)?;
    let exit_dir = result?;
    handoff_exit_directory(&exit_dir)?;
    handoff_audio_dependency_guidance();
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
        app.advance_audio_cache()?;
        app.advance_audio()?;

        terminal.draw(|frame| ui::render(frame, &app))?;

        if event::poll(Duration::from_millis(33))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key)?,
                Event::Mouse(mouse) => {
                    let left_width = terminal.size()?.width / 2;
                    app.handle_mouse(mouse, left_width);
                }
                Event::Resize(_, _) => {}
                Event::FocusGained | Event::FocusLost => {}
                Event::Paste(data) => app.handle_paste(data)?,
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

fn handoff_audio_dependency_guidance() {
    let Some(warning) = linux_audio_dependency_warning() else {
        return;
    };

    let install_command = "sudo apt install libasound2-plugins alsa-utils";
    let description =
        "Instala los paquetes necesarios para habilitar la reproduccion de audio (ALSA) en files_rs.";

    println!();
    println!("Aviso de audio: {warning}");
    println!("Descripcion: {description}");
    println!("Comando de instalacion: {install_command}");

    if copy_to_clipboard(install_command) {
        println!("Comando copiado al portapapeles.");
    } else {
        println!(
            "No se pudo copiar automaticamente. Copia y ejecuta el comando de instalacion manualmente."
        );
    }
}

#[cfg(target_os = "linux")]
fn copy_to_clipboard(text: &str) -> bool {
    copy_to_clipboard_with("wl-copy", &[], text)
        || copy_to_clipboard_with("xclip", &["-selection", "clipboard"], text)
        || copy_to_clipboard_with("xsel", &["--clipboard", "--input"], text)
}

#[cfg(target_os = "linux")]
fn copy_to_clipboard_with(command: &str, args: &[&str], text: &str) -> bool {
    let mut child = match Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    if let Some(stdin) = child.stdin.as_mut() {
        if stdin.write_all(text.as_bytes()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
    }

    child.wait().map(|status| status.success()).unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn copy_to_clipboard(_text: &str) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn suppress_alsa_stderr_noise() {
    unsafe extern "C" fn no_op_alsa_error_handler(
        _file: *const std::ffi::c_char,
        _line: std::ffi::c_int,
        _func: *const std::ffi::c_char,
        _err: std::ffi::c_int,
        _fmt: *const std::ffi::c_char,
        _arg: *mut alsa_sys::__va_list_tag,
    ) {
    }

    unsafe {
        let _ = alsa_sys::snd_lib_error_set_local(Some(no_op_alsa_error_handler));
    }
}

#[cfg(not(target_os = "linux"))]
fn suppress_alsa_stderr_noise() {}
