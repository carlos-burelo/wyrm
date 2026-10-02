//! Políticas por app: restarts, memoria, shutdown y healthcheck.
//!
//! Viven en `AppConfig` y `wyrm.json` (`policy: {...}`). No se persisten en
//! SQLite (DB guarda ejecutable/args/cwd/env); el demonio las recibe en el
//! payload `START` y `restore_from_db` usa defaults.

use serde::{Deserialize, Serialize};

fn default_max_restarts() -> u32 {
    10
}
fn default_min_uptime() -> u64 {
    5
}
fn default_stop_timeout() -> u64 {
    5
}
fn default_health_secs() -> u64 {
    30
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Restarts totales antes de marcar ERRORED y dejar de reintentar.
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    /// Segundos mínimos de vida para considerar el arranque "estable".
    /// Salir antes cuenta como crash (crash-loop guard).
    #[serde(default = "default_min_uptime")]
    pub min_uptime_secs: u64,
    /// Ventana de espera al detener antes de dar por muerto al proceso.
    /// (En Windows el stop es terminate; ver `daemon`.)
    #[serde(default = "default_stop_timeout")]
    pub stop_timeout_secs: u64,
    /// Reinicia si la RSS supera este tope (MiB). None = sin límite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u64>,
    /// GET periódico; N fallos seguidos reinician la app. None = off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub healthcheck_url: Option<String>,
    #[serde(default = "default_health_secs")]
    pub healthcheck_secs: u64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_restarts: default_max_restarts(),
            min_uptime_secs: default_min_uptime(),
            stop_timeout_secs: default_stop_timeout(),
            max_memory_mb: None,
            healthcheck_url: None,
            healthcheck_secs: default_health_secs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_y_compat_con_json_viejo() {
        // Payloads/ecosystems sin `policy` deben seguir parseando.
        let p: Policy = serde_json::from_str("{}").unwrap();
        assert_eq!(p.max_restarts, 10);
        assert_eq!(p.min_uptime_secs, 5);
        assert!(p.max_memory_mb.is_none());
        assert!(p.healthcheck_url.is_none());
    }

    #[test]
    fn policy_custom_roundtrip() {
        let p = Policy {
            max_memory_mb: Some(512),
            healthcheck_url: Some("http://127.0.0.1:3000/health".into()),
            ..Default::default()
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: Policy = serde_json::from_str(&s).unwrap();
        assert_eq!(back.max_memory_mb, Some(512));
        assert!(back.healthcheck_url.unwrap().contains("3000"));
    }
}
