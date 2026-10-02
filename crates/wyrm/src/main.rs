mod db;
mod inspector;
mod ipc;
mod process;
mod service;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "wyrm")]
#[command(about = "El gestor de procesos definitivo para Windows Server", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(long, hide = true)]
    daemon: bool,
}

#[derive(Subcommand)]
enum Commands {
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
    /// Muestra las aplicaciones en supervisión
    List,
    /// Administra el servicio de Windows (install / uninstall)
    Service { action: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.daemon {
        return crate::service::start_service_dispatcher().map_err(|e| e.into());
    }

    match cli.command {
        Some(Commands::Start { name, cwd }) => {
            let target_dir = cwd.unwrap_or_else(|| std::env::current_dir().unwrap());
            let app_config = crate::inspector::inspect_and_configure(&target_dir, name)?;

            let db = crate::db::Database::init()?;
            db.save_app(&app_config)?;

            println!("Registrando aplicación: {}", app_config.name);
            let res = crate::ipc::send_command("START", serde_json::to_value(app_config)?).await?;
            println!("Respuesta del Demonio Wyrm: {}", res);
        }
        Some(Commands::List) => {
            let res = crate::ipc::send_command("LIST", serde_json::Value::Null).await?;
            println!("{}", res);
        }
        Some(Commands::Service { action }) => match action.as_str() {
            "install" => crate::service::install_service()?,
            _ => println!("Uso: wyrm service install"),
        },
        _ => {
            println!("Ejecute 'wyrm --help' para ver los comandos disponibles.");
        }
    }

    Ok(())
}