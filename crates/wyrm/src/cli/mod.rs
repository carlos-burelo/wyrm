//! CLI: definición clap + dispatch.
//!
//! Cada subcomando vive en su módulo; `run()` despacha.

pub mod deploy;
pub mod doctor;
pub mod init;
pub mod list;
pub mod logs;
pub mod manage;
pub mod start;
pub mod status;
pub mod token;

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
        /// Inicia todas las apps de wyrm.json / ecosystem.json
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Ruta explícita al ecosystem file
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Genera wyrm.json inspeccionando el proyecto actual
    Init {
        #[arg(short, long)]
        name: Option<String>,
        #[arg(long, default_value_t = false)]
        force: bool,
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
    /// Despliega una app: hooks + git sync + restart
    Deploy {
        name: String,
        /// Rama/tag/SHA a desplegar (default: pull --ff-only)
        #[arg(long)]
        ref_: Option<String>,
    },
    /// Historial de deploys de una app
    Releases {
        name: String,
        #[arg(long, default_value_t = 10)]
        limit: i64,
    },
    /// Vuelve al último deploy ok (git reset + restart)
    Rollback { name: String },
    /// Muestra o rota el token Bearer de la API local
    Token {
        #[arg(long, default_value_t = false)]
        rotate: bool,
    },
    /// Diagnóstico del entorno (node, demonio, servicio, disco, logs)
    Doctor,
    /// TUI interactiva de primer nivel
    Top,
    /// Administra el servicio de Windows (install / uninstall)
    Service { action: String },
}

pub async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Some(Commands::Start {
            name,
            cwd,
            all,
            file,
        }) => {
            if all || file.is_some() {
                start::cmd_start_all(file).await?
            } else {
                start::cmd_start(name, cwd).await?
            }
        }
        Some(Commands::Init { name, force }) => init::cmd_init(name, force).await?,
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
        Some(Commands::Deploy { name, ref_ }) => deploy::cmd_deploy(&name, ref_).await?,
        Some(Commands::Releases { name, limit }) => deploy::cmd_releases(&name, limit).await?,
        Some(Commands::Rollback { name }) => deploy::cmd_rollback(&name).await?,
        Some(Commands::Token { rotate }) => token::cmd_token(rotate).await?,
        Some(Commands::Doctor) => doctor::cmd_doctor().await?,
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
