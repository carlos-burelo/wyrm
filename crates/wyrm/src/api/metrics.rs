//! Métricas Prometheus (texto 0.0.4). Función pura → testeable.

pub struct AppMetric {
    pub name: String,
    pub status: String,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub uptime_secs: u64,
    pub cpu: f64,
    pub mem_bytes: u64,
}

fn esc(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', "\\\"")
}

pub fn render(apps: &[AppMetric]) -> String {
    let mut out = String::new();
    out.push_str("# HELP wyrm_app_up Indicador 1 si la app está RUNNING.\n");
    out.push_str("# TYPE wyrm_app_up gauge\n");
    out.push_str("# HELP wyrm_app_uptime_seconds Segundos desde el arranque.\n");
    out.push_str("# TYPE wyrm_app_uptime_seconds gauge\n");
    out.push_str("# HELP wyrm_app_restarts_total Reinicios acumulados.\n");
    out.push_str("# TYPE wyrm_app_restarts_total counter\n");
    out.push_str("# HELP wyrm_app_cpu_percent CPU % del proceso.\n");
    out.push_str("# TYPE wyrm_app_cpu_percent gauge\n");
    out.push_str("# HELP wyrm_app_memory_bytes RSS del proceso.\n");
    out.push_str("# TYPE wyrm_app_memory_bytes gauge\n");
    for a in apps {
        let up = i32::from(a.status.starts_with("RUNNING"));
        let labels = format!("name=\"{}\" status=\"{}\"", esc(&a.name), esc(&a.status));
        out.push_str(&format!("wyrm_app_up{{{labels}}} {up}\n"));
        out.push_str(&format!(
            "wyrm_app_uptime_seconds{{{labels}}} {}\n",
            a.uptime_secs
        ));
        out.push_str(&format!(
            "wyrm_app_restarts_total{{{labels}}} {}\n",
            a.restarts
        ));
        out.push_str(&format!("wyrm_app_cpu_percent{{{labels}}} {:.1}\n", a.cpu));
        out.push_str(&format!(
            "wyrm_app_memory_bytes{{{labels}}} {}\n",
            a.mem_bytes
        ));
        let _ = a.pid;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formato_prometheus() {
        let out = render(&[AppMetric {
            name: "web\"x".into(),
            status: "RUNNING".into(),
            pid: Some(123),
            restarts: 2,
            uptime_secs: 60,
            cpu: 3.5,
            mem_bytes: 1024,
        }]);
        assert!(out.contains("wyrm_app_up{"));
        assert!(out.contains("name=\"web\\\"x\""));
        assert!(out.contains("wyrm_app_restarts_total{name=\"web\\\"x\" status=\"RUNNING\"} 2"));
    }
}
