//! CLI: definición clap + dispatch.
//!
//! Cada subcomando vive en su módulo; `run()` despacha.

pub mod list;
pub mod logs;
pub mod manage;
pub mod start;
pub mod status;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "wyrm")]
#[command(
    about = "El gestor de procesos definitivo para Windows Server",
    long_about = None
)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(long, hide = true)]
    pub daemon: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Inicia una aplicación (auto-detecta package.json)
    Start {
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        cwd: Option<PathBuf>,
    },
    /// Detiene una aplicación por nombre
    Stop { name: String },
    /// Reinicia una aplicación
    Restart { name: String },
    /// Elimina una aplicación de la supervisión
    Delete {
        name: String,
        #[arg(long, default_value_t = false)]
        yes: bool,
    },
    /// Muestra las aplicaciones en supervisión
    List {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Muestra el estado detallado de una app
    Status { name: String },
    /// Muestra las últimas líneas del log de una app
    Logs {
        name: String,
        #[arg(short, long, default_value_t = 50)]
        lines: usize,
        #[arg(short, long, default_value_t = false)]
        follow: bool,
    },
    /// Ejecuta el demonio en foreground (para debug / sin servicio)
    Daemon,
    /// TUI interactiva de primer nivel
    Top,
    /// Administra el servicio de Windows (install / uninstall)
    Service { action: String },
}

pub async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Some(Commands::Start { name, cwd }) => start::cmd_start(name, cwd).await?,
        Some(Commands::Stop { name }) => manage::cmd_simple("STOP", &name).await?,
        Some(Commands::Restart { name }) => manage::cmd_simple("RESTART", &name).await?,
        Some(Commands::Delete { name, yes }) => manage::cmd_delete(&name, yes).await?,
        Some(Commands::List { json }) => list::cmd_list(json).await?,
        Some(Commands::Status { name }) => status::cmd_status(&name).await?,
        Some(Commands::Logs {
            name,
            lines,
            follow,
        }) => logs::cmd_logs(&name, lines, follow).await?,
        Some(Commands::Daemon) => crate::daemon::run_foreground().await?,
        Some(Commands::Top) => crate::tui::run().await?,
        Some(Commands::Service { action }) => match action.as_str() {
            "install" => crate::runtime::service::install_service()?,
            "uninstall" => crate::runtime::service::uninstall_service()?,
            _ => println!("Uso: wyrm service <install|uninstall>"),
        },
        None => {
            // Sin args: intenta TUI si hay TTY, si no ayuda.
            if console_is_tty() {
                crate::tui::run().await?;
            } else {
                println!("Ejecute 'wyrm --help' para ver los comandos disponibles.");
            }
        }
    }

    Ok(())
}

fn console_is_tty() -> bool {
    // Heurística simple sin deps extra: si NO_TTY no está seteado y tenemos consola.
    std::env::var("NO_TTY").is_err() && atty_like()
}

#[cfg(windows)]
fn atty_like() -> bool {
    // En Windows, asumimos TTY salvo redirección evidente vía cargo test.
    !cfg!(test)
}

#[cfg(not(windows))]
fn atty_like() -> bool {
    true
}
