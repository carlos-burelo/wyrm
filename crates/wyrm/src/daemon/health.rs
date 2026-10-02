//! Healthchecks HTTP: GET periódico, 3 fallos seguidos matan el proceso
//! para que el loop de supervisión lo reinicie por el crash-path normal
//! (respeta min_uptime/max_restarts).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::Daemon;

const FAILS_TO_KILL: u32 = 3;

pub async fn health_loop(daemon: Arc<Daemon>) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[wyrm] healthchecks deshabilitados: {e}");
            return;
        }
    };
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let due: Vec<(String, String)> = {
            let guard = daemon.apps.lock().await;
            guard
                .values()
                .filter(|a| a.status == "RUNNING")
                .filter_map(|a| {
                    let url = a.config.policy.healthcheck_url.clone()?;
                    let secs = a.config.policy.healthcheck_secs.max(5);
                    let due = a
                        .last_health
                        .and_then(|t| t.elapsed().ok())
                        .map(|d| d.as_secs() >= secs)
                        .unwrap_or(true);
                    due.then(|| (a.config.name.clone(), url))
                })
                .collect()
        };
        for (name, url) in due {
            let ok = client
                .get(&url)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false);
            let mut guard = daemon.apps.lock().await;
            let Some(app) = guard.get_mut(&name) else {
                continue;
            };
            if app.status != "RUNNING" {
                continue;
            }
            if app.config.policy.healthcheck_url.is_none() {
                app.health_fail = 0;
                continue;
            }
            app.last_health = Some(SystemTime::now());
            if ok {
                app.health_fail = 0;
            } else {
                app.health_fail += 1;
                eprintln!(
                    "[wyrm] health {name} fallo {}/{}",
                    app.health_fail, FAILS_TO_KILL
                );
                if app.health_fail >= FAILS_TO_KILL {
                    app.health_fail = 0;
                    // Mata; supervise() lo ve salir y lo reinicia.
                    if let Some(child) = app.child.as_mut() {
                        let _ = child.child.start_kill();
                    }
                }
            }
        }
    }
}
