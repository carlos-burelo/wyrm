//! Daemon supervisor: estado en memoria + IPC + restore.

pub mod health;
pub mod state;
pub mod supervise;

pub use state::AppStatus;
use state::ManagedApp;

use crate::ipc::protocol::{Request, Response};
use crate::runtime::inspector::AppConfig;
use crate::runtime::process::spawn_managed;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;

pub struct Daemon {
    apps: Mutex<HashMap<String, ManagedApp>>,
}

impl Daemon {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            apps: Mutex::new(HashMap::new()),
        })
    }

    pub async fn restore_from_db(self: &Arc<Self>) {
        let apps = tokio::task::spawn_blocking(|| {
            crate::store::db::Database::init()
                .and_then(|db| db.list_apps())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        let mut guard = self.apps.lock().await;
        for rec in apps {
            // Solo auto-arranca lo que quedó RUNNING al apagar.
            if rec.status == "RUNNING" {
                let cfg = rec.to_app_config();
                let mut app = ManagedApp::new(cfg.clone());
                match spawn_managed(&cfg, &crate::store::db::Database::log_path_for(&cfg.name)) {
                    Ok(child) => {
                        app.child = Some(child);
                        app.status = "RUNNING".to_string();
                        app.started_at = Some(SystemTime::now());
                        app.restarts = rec.restarts as u32;
                    }
                    Err(e) => {
                        eprintln!("[wyrm] no se pudo restaurar {}: {e}", cfg.name);
                        app.status = "CRASHED".to_string();
                    }
                }
                guard.insert(app.config.name.clone(), app);
            }
        }
    }

    pub async fn handle(self: &Arc<Self>, req: Request) -> Response {
        match req.action.as_str() {
            "START" => {
                let cfg: Result<AppConfig, _> = serde_json::from_value(req.payload);
                match cfg {
                    Ok(cfg) => self.start(cfg).await,
                    Err(e) => Response::err(format!("Payload START inválido: {e}")),
                }
            }
            "STOP" => {
                let name = req
                    .payload
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.stop(name).await
            }
            "RESTART" => {
                let name = req
                    .payload
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.restart(name).await
            }
            "DELETE" => {
                let name = req
                    .payload
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.delete(name).await
            }
            "LIST" => self.list().await,
            "STATUS" => {
                let name = req
                    .payload
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.status(name).await
            }
            other => Response::err(format!("Acción desconocida: {other}")),
        }
    }

    async fn start(self: &Arc<Self>, cfg: AppConfig) -> Response {
        let mut guard = self.apps.lock().await;
        if let Some(existing) = guard.get_mut(&cfg.name) {
            if existing.status == "RUNNING" {
                return Response::err(format!("{} ya está corriendo", cfg.name));
            }
            // Re-start de una app conocida: actualiza config.
            existing.config = cfg.clone();
        } else {
            guard.insert(cfg.name.clone(), ManagedApp::new(cfg.clone()));
        }
        let name = cfg.name.clone();
        let log_path = crate::store::db::Database::log_path_for(&name);
        match spawn_managed(&cfg, &log_path) {
            Ok(child) => {
                let app = guard.get_mut(&name).unwrap();
                app.child = Some(child);
                app.status = "RUNNING".to_string();
                app.started_at = Some(SystemTime::now());
                app.last_heartbeat = SystemTime::now();
                app.unstable = 0;
                drop(guard);
                persist_status(&name, "RUNNING", false);
                Response::ok(
                    "START",
                    serde_json::json!({ "name": name, "status": "RUNNING" }),
                )
            }
            Err(e) => {
                let app = guard.get_mut(&name).unwrap();
                app.status = "CRASHED".to_string();
                Response::err(format!("No se pudo iniciar {name}: {e}"))
            }
        }
    }

    async fn stop(self: &Arc<Self>, name: &str) -> Response {
        if name.is_empty() {
            return Response::err("Falta `name`");
        }
        let mut guard = self.apps.lock().await;
        let Some(app) = guard.get_mut(name) else {
            return Response::err(format!("App desconocida: {name}"));
        };
        if let Some(mut child) = app.child.take() {
            // Ventana de gracia configurable antes de dar por muerto al proceso.
            // Nota Windows: el stop es terminate; CTRL+BREAK elegante queda futuro.
            let timeout = app.config.policy.stop_timeout_secs.max(1);
            let _ = child.child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(timeout), child.child.wait()).await;
        }
        app.status = "STOPPED".to_string();
        app.started_at = None;
        app.crash_count = 0;
        app.unstable = 0;
        drop(guard);
        persist_status(name, "STOPPED", false);
        Response::ok(
            "STOP",
            serde_json::json!({ "name": name, "status": "STOPPED" }),
        )
    }

    async fn restart(self: &Arc<Self>, name: &str) -> Response {
        if name.is_empty() {
            return Response::err("Falta `name`");
        }
        let cfg = {
            let guard = self.apps.lock().await;
            guard.get(name).map(|a| a.config.clone())
        };
        let Some(cfg) = cfg else {
            return Response::err(format!("App desconocida: {name}"));
        };
        let _ = self.stop(name).await;
        // Pequeña espera para liberar puerto.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let mut guard = self.apps.lock().await;
        let log_path = crate::store::db::Database::log_path_for(&name);
        match spawn_managed(&cfg, &log_path) {
            Ok(child) => {
                let app = guard.get_mut(name).unwrap();
                app.child = Some(child);
                app.status = "RUNNING".to_string();
                app.started_at = Some(SystemTime::now());
                app.restarts += 1;
                app.unstable = 0;
                let restarts = app.restarts;
                drop(guard);
                persist_status(name, "RUNNING", true);
                Response::ok(
                    "RESTART",
                    serde_json::json!({ "name": name, "status": "RUNNING", "restarts": restarts }),
                )
            }
            Err(e) => Response::err(format!("No se pudo reiniciar {name}: {e}")),
        }
    }

    async fn delete(self: &Arc<Self>, name: &str) -> Response {
        if name.is_empty() {
            return Response::err("Falta `name`");
        }
        let owned = name.to_string();
        let _ = self.stop(&owned).await;
        {
            let mut guard = self.apps.lock().await;
            guard.remove(&owned);
        }
        let name2 = owned.clone();
        let ok = tokio::task::spawn_blocking(move || {
            crate::store::db::Database::init()
                .and_then(|db| db.delete_app(&name2))
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);
        if ok {
            Response::ok_msg(format!("{owned} eliminada"))
        } else {
            // Igual la quitamos de memoria; avisamos.
            Response::ok_msg(format!("{owned} eliminada de memoria (no estaba en DB)"))
        }
    }

    async fn list(&self) -> Response {
        // Base: DB para incluir apps STOPPED no cargadas en memoria.
        let db_apps = tokio::task::spawn_blocking(|| {
            crate::store::db::Database::init()
                .and_then(|db| db.list_apps())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        let guard = self.apps.lock().await;
        let mut out: Vec<AppStatus> = Vec::new();
        for rec in &db_apps {
            if let Some(live) = guard.get(&rec.name) {
                out.push(live.to_status());
            } else {
                out.push(AppStatus {
                    name: rec.name.clone(),
                    status: rec.status.clone(),
                    pid: None,
                    restarts: rec.restarts as u32,
                    uptime_secs: 0,
                    executable: rec.executable.clone(),
                    cwd: rec.cwd.clone(),
                });
            }
        }
        // Apps solo en memoria (recién creadas).
        for (name, app) in guard.iter() {
            if !db_apps.iter().any(|r| &r.name == name) {
                out.push(app.to_status());
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Response::ok("LIST", serde_json::to_value(&out).unwrap_or_default())
    }

    async fn status(&self, name: &str) -> Response {
        if name.is_empty() {
            return Response::err("Falta `name`");
        }
        let guard = self.apps.lock().await;
        if let Some(app) = guard.get(name) {
            return Response::ok(
                "STATUS",
                serde_json::to_value(app.to_status()).unwrap_or_default(),
            );
        }
        drop(guard);
        // Fallback DB.
        let name_owned = name.to_string();
        let rec = tokio::task::spawn_blocking(move || {
            crate::store::db::Database::init()
                .and_then(|db| db.get_app(&name_owned))
                .unwrap_or(None)
        })
        .await
        .unwrap_or(None);
        match rec {
            Some(r) => Response::ok(
                "STATUS",
                serde_json::json!({
                    "name": r.name, "status": r.status, "pid": null,
                    "restarts": r.restarts, "uptime_secs": 0,
                    "executable": r.executable, "cwd": r.cwd
                }),
            ),
            None => Response::err(format!("App desconocida: {name}")),
        }
    }
}

fn persist_status(name: &str, status: &str, inc: bool) {
    let name = name.to_string();
    let status = status.to_string();
    std::thread::spawn(move || {
        if let Ok(db) = crate::store::db::Database::init() {
            let _ = db.update_status(&name, &status, inc);
        }
    });
}

pub fn blocking_handler(daemon: Arc<Daemon>) -> crate::ipc::Handler {
    std::sync::Arc::new(move |req: Request| {
        let d = daemon.clone();
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(d.handle(req)))
    })
}

/// Punto de entrada del demonio en foreground (`wyrm daemon`).
pub async fn run_foreground() -> Result<(), Box<dyn std::error::Error>> {
    let daemon = Daemon::new();
    daemon.restore_from_db().await;
    let d2 = daemon.clone();
    tokio::spawn(async move { d2.supervise().await });
    let d3 = daemon.clone();
    tokio::spawn(async move { health::health_loop(d3).await });
    let d4 = daemon.clone();
    tokio::spawn(async move {
        let _ = crate::api::serve(d4).await;
    });

    let handler = blocking_handler(daemon);

    println!("Wyrm daemon escuchando en {}", crate::ipc::PIPE_NAME);
    crate::ipc::run_ipc_server_with(handler).await
}
