mod daemon;
mod ipc;
mod runtime;
mod store;
mod tui;

// Shims temporales durante la migración a full layout (se eliminan al final).
pub(crate) use ipc::protocol;
pub(crate) use runtime::inspector;
pub(crate) use runtime::process;
pub(crate) use runtime::service;
pub(crate) use store::db;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "wyrm")]
#[command(about = "El gestor de procesos definitivo para Windows Server", long_about = None)]
#[command(version)]
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.daemon {
        return crate::service::start_service_dispatcher().map_err(|e| e.into());
    }

    match cli.command {
        Some(Commands::Start { name, cwd }) => cmd_start(name, cwd).await?,
        Some(Commands::Stop { name }) => cmd_simple("STOP", &name).await?,
        Some(Commands::Restart { name }) => cmd_simple("RESTART", &name).await?,
        Some(Commands::Delete { name, yes }) => cmd_delete(&name, yes).await?,
        Some(Commands::List { json }) => cmd_list(json).await?,
        Some(Commands::Status { name }) => cmd_status(&name).await?,
        Some(Commands::Logs {
            name,
            lines,
            follow,
        }) => cmd_logs(&name, lines, follow).await?,
        Some(Commands::Daemon) => crate::daemon::run_foreground().await?,
        Some(Commands::Top) => crate::tui::run().await?,
        Some(Commands::Service { action }) => match action.as_str() {
            "install" => crate::service::install_service()?,
            "uninstall" => crate::service::uninstall_service()?,
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

async fn cmd_start(
    name: Option<String>,
    cwd: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_dir = cwd
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let app_config = crate::inspector::inspect_and_configure(&target_dir, name)?;

    {
        let db = crate::db::Database::init()?;
        db.save_app(&app_config)?;
    }

    println!("Registrando aplicación: {}", app_config.name);
    match crate::ipc::send_request("START", serde_json::to_value(&app_config)?).await {
        Ok(res) if res.is_ok() => println!("OK {} -> {}", app_config.name, res.message),
        Ok(res) => eprintln!("Demonio respondió error: {}", res.message),
        Err(e) => {
            eprintln!("Aviso: {e}");
            eprintln!("La app quedó guardada en DB. Arranca el demonio con `wyrm daemon` o instala el servicio con `wyrm service install`.");
        }
    }
    Ok(())
}

async fn cmd_simple(action: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    match crate::ipc::send_request(action, serde_json::json!({ "name": name })).await {
        Ok(res) if res.is_ok() => {
            println!("{} {}: {}", action, name, res.message);
            if let Some(d) = res.data {
                println!("{d}");
            }
        }
        Ok(res) => eprintln!("Error: {}", res.message),
        Err(e) => {
            // Fallback DB para STOP: marca STOPPED aunque el daemon esté caído.
            if action == "STOP" {
                if let Ok(db) = crate::db::Database::init() {
                    let _ = db.update_status(name, "STOPPED", false);
                }
            }
            eprintln!("No se pudo contactar al demonio: {e}");
        }
    }
    Ok(())
}

async fn cmd_delete(name: &str, yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !yes {
        eprintln!("Confirma con `wyrm delete {name} --yes`");
        return Ok(());
    }
    match crate::ipc::send_request("DELETE", serde_json::json!({ "name": name })).await {
        Ok(res) => println!("{}", res.message),
        Err(e) => {
            // Fallback: borra de DB.
            match crate::db::Database::init()?.delete_app(name) {
                Ok(true) => println!("{name} eliminada de DB (daemon no disponible)."),
                _ => eprintln!("No se pudo eliminar: {e}"),
            }
        }
    }
    Ok(())
}

async fn cmd_list(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let rows: Vec<crate::daemon::AppStatus> =
        match crate::ipc::send_request("LIST", serde_json::Value::Null).await {
            Ok(res) if res.is_ok() => {
                serde_json::from_value(res.data.unwrap_or_default()).unwrap_or_default()
            }
            _ => {
                // Fallback DB.
                let db_rows = tokio::task::spawn_blocking(|| {
                    crate::db::Database::init()
                        .and_then(|db| db.list_apps())
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                db_rows
                    .into_iter()
                    .map(|r| crate::daemon::AppStatus {
                        name: r.name,
                        status: format!("{} (daemon off)", r.status),
                        pid: None,
                        restarts: r.restarts as u32,
                        uptime_secs: 0,
                        executable: r.executable,
                        cwd: r.cwd,
                    })
                    .collect()
            }
        };

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if rows.is_empty() {
        println!("Sin aplicaciones. Usa `wyrm start` en un proyecto Node.");
        return Ok(());
    }

    print_table(&rows);
    Ok(())
}

fn print_table(rows: &[crate::daemon::AppStatus]) {
    use colored::Colorize;
    let mut tw = tabwriter::TabWriter::new(std::io::stdout());
    let _ = writeln!(tw, "NAME\tSTATUS\tPID\tRESTARTS\tUPTIME\tCWD");
    for r in rows {
        let status = match r.status.as_str() {
            s if s.starts_with("RUNNING") => s.green().to_string(),
            s if s.starts_with("STOPPED") => s.dimmed().to_string(),
            s if s.starts_with("CRASHED") => s.red().bold().to_string(),
            s => s.yellow().to_string(),
        };
        let pid = r.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into());
        let uptime = format_uptime(r.uptime_secs);
        let _ = writeln!(
            tw,
            "{}\t{}\t{}\t{}\t{}\t{}",
            r.name, status, pid, r.restarts, uptime, r.cwd
        );
    }
    let _ = tw.flush();
}

fn format_uptime(secs: u64) -> String {
    if secs == 0 {
        return "-".into();
    }
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

async fn cmd_status(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    match crate::ipc::send_request("STATUS", serde_json::json!({ "name": name })).await {
        Ok(res) if res.is_ok() => {
            println!(
                "{}",
                serde_json::to_string_pretty(&res.data.unwrap_or_default())?
            );
        }
        Ok(res) => eprintln!("Error: {}", res.message),
        Err(e) => eprintln!("Daemon no disponible: {e}"),
    }
    Ok(())
}

async fn cmd_logs(
    name: &str,
    lines: usize,
    follow: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = crate::db::Database::log_path_for(name);
    if !path.exists() {
        eprintln!("Sin logs en {} (¿la app existe?)", path.display());
        return Ok(());
    }
    if follow {
        println!("Siguiendo {} (Ctrl+C para salir)…", path.display());
        let mut last = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len < last {
                last = 0;
            }
            if len > last {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let bytes = content.as_bytes();
                // Imprime solo lo nuevo de forma aproximada.
                let from = usize::try_from(last).unwrap_or(0).min(bytes.len());
                print!("{}", &content[from.min(content.len())..]);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                last = len;
                let _ = bytes;
            }
        }
    } else {
        let content = std::fs::read_to_string(&path)?;
        let all: Vec<&str> = content.lines().collect();
        let start = all.len().saturating_sub(lines);
        for l in &all[start..] {
            println!("{l}");
        }
    }
    Ok(())
}

use std::io::Write as _;
