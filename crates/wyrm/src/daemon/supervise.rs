//! Supervision: loop que detecta salidas y auto-reinicia con backoff.

use crate::runtime::inspector::AppConfig;
use crate::runtime::process::spawn_managed;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::{persist_status, Daemon};

impl Daemon {
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
                let log_path = crate::store::db::Database::log_path_for(&name);
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
