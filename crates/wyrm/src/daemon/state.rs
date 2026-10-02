//! Estado del supervisor: tipos AppStatus y ManagedApp.

use crate::runtime::inspector::AppConfig;
use crate::runtime::process::ManagedChild;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

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

pub(crate) struct ManagedApp {
    pub(crate) config: AppConfig,
    pub(crate) child: Option<ManagedChild>,
    pub(crate) status: String,
    pub(crate) restarts: u32,
    pub(crate) started_at: Option<SystemTime>,
    pub(crate) crash_count: u32,
    /// Salidas consecutivas antes de `min_uptime_secs` (crash-loop).
    /// Un arranque estable lo resetea; al llegar a `max_restarts` → ERRORED.
    pub(crate) unstable: u32,
    pub(crate) last_heartbeat: SystemTime,
}

impl ManagedApp {
    pub(crate) fn new(config: AppConfig) -> Self {
        Self {
            config,
            child: None,
            status: "STOPPED".to_string(),
            restarts: 0,
            started_at: None,
            crash_count: 0,
            unstable: 0,
            last_heartbeat: SystemTime::now(),
        }
    }

    pub(crate) fn to_status(&self) -> AppStatus {
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
