use crate::inspector::AppConfig;
use crate::process::{spawn_managed, ManagedChild};
use crate::protocol::{Request, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatus {
    pub name: String,
    pub status: String,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub uptime_secs: u64,
    pub executable: String,
    pub cwd: String,
}

struct ManagedApp {
    config: AppConfig,
    child: Option<ManagedChild>,
    status: String,
    restarts: u32,
    started_at: Option<SystemTime>,
    crash_count: u32,
    last_heartbeat: SystemTime,
}

impl ManagedApp {
    fn new(config: AppConfig) -> Self {
        Self {
            config,
            child: None,
            status: "STOPPED".to_string(),
            restarts: 0,
            started_at: None,
            crash_count: 0,
            last_heartbeat: SystemTime::now(),
        }
    }

    fn to_status(&self) -> AppStatus {
        let uptime = self
            .started_at
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        AppStatus {
            name: self.config.name.clone(),
            status: self.status.clone(),
            pid: self.child.as_ref().and_then(|c| c.pid()),
            restarts: self.restarts,
            uptime_secs: if self.status == "RUNNING" { uptime } else { 0 },
            executable: self.config.executable.clone(),
            cwd: self.config.cwd.to_string_lossy().to_string(),
        }
    }
}

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
            crate::db::Database::init()
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
                match spawn_managed(&cfg, &crate::db::Database::log_path_for(&cfg.name)) {
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
        let log_path = crate::db::Database::log_path_for(&name);
        match spawn_managed(&cfg, &log_path) {
            Ok(child) => {
                let app = guard.get_mut(&name).unwrap();
                app.child = Some(child);
                app.status = "RUNNING".to_string();
                app.started_at = Some(SystemTime::now());
                app.last_heartbeat = SystemTime::now();
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
            let _ = child.child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(5), child.child.wait()).await;
        }
        app.status = "STOPPED".to_string();
        app.started_at = None;
        app.crash_count = 0;
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
        let log_path = crate::db::Database::log_path_for(&name);
        match spawn_managed(&cfg, &log_path) {
            Ok(child) => {
                let app = guard.get_mut(name).unwrap();
                app.child = Some(child);
                app.status = "RUNNING".to_string();
                app.started_at = Some(SystemTime::now());
                app.restarts += 1;
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
            crate::db::Database::init()
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
            crate::db::Database::init()
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
            crate::db::Database::init()
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

    /// Loop de supervisión: detecta salidas y auto-reinicia con backoff.
    pub async fn supervise(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let mut to_restart: Vec<AppConfig> = Vec::new();
            {
                let mut guard = self.apps.lock().await;
                for app in guard.values_mut() {
                    if app.status != "RUNNING" {
                        continue;
                    }
                    if let Some(child) = app.child.as_mut() {
                        match child.child.try_wait() {
                            Ok(Some(exit)) => {
                                let code = exit.code();
                                eprintln!(
                                    "[wyrm] {} salió (code={:?}), reiniciando…",
                                    app.config.name, code
                                );
                                app.child = None;
                                app.status = "CRASHED".to_string();
                                app.crash_count += 1;
                                // Backoff: 1s * crash_count hasta 30s.
                                let wait = std::cmp::min(app.crash_count, 30) as u64;
                                let cfg = app.config.clone();
                                let name = cfg.name.clone();
                                tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_secs(wait)).await;
                                    let _ = name;
                                });
                                let _ = wait;
                                to_restart.push(cfg);
                            }
                            Ok(None) => {
                                app.last_heartbeat = SystemTime::now();
                            }
                            Err(e) => {
                                eprintln!("[wyrm] try_wait {}: {e}", app.config.name);
                            }
                        }
                    }
                }
            }
            for cfg in to_restart {
                let name = cfg.name.clone();
                let log_path = crate::db::Database::log_path_for(&name);
                match spawn_managed(&cfg, &log_path) {
                    Ok(child) => {
                        let mut guard = self.apps.lock().await;
                        if let Some(app) = guard.get_mut(&name) {
                            app.child = Some(child);
                            app.status = "RUNNING".to_string();
                            app.started_at = Some(SystemTime::now());
                            app.restarts += 1;
                            drop(guard);
                            persist_status(&name, "RUNNING", true);
                        }
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        eprintln!("[wyrm] auto-restart {name} falló: {msg}");
                        let mut guard = self.apps.lock().await;
                        if let Some(app) = guard.get_mut(&name) {
                            app.status = "CRASHED".to_string();
                        }
                        persist_status(&name, "CRASHED", false);
                    }
                }
            }
        }
    }
}

fn persist_status(name: &str, status: &str, inc: bool) {
    let name = name.to_string();
    let status = status.to_string();
    std::thread::spawn(move || {
        if let Ok(db) = crate::db::Database::init() {
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

    let handler = blocking_handler(daemon);

    println!("Wyrm daemon escuchando en {}", crate::ipc::PIPE_NAME);
    crate::ipc::run_ipc_server_with(handler).await
}
