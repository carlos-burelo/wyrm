//! TUI Ratatui: dashboard superior a `pm2 monit`.
//!
//! - `state`: estado, tabs, orden.
//! - `data`: refresh daemon/DB, logs, métricas.
//! - `actions`: stop/start/restart/delete contra el demonio.
//! - `events`: loop de teclado.
//! - `theme`: colores por estado/nivel.
//! - `views_*`: render.

pub mod actions;
pub mod data;
pub mod events;
pub mod state;
pub mod theme;
pub mod views_chrome;
pub mod views_dashboard;
pub mod views_logs;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut st = state::TuiState::new();
    data::refresh_apps(&mut st).await;

    let res = events::event_loop(&mut terminal, &mut st).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("TUI error: {e}");
    }
    Ok(())
}
