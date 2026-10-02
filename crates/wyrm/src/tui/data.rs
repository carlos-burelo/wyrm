use crate::daemon::AppStatus;
use std::collections::VecDeque;
use std::time::Instant;

use super::state::*;

pub(crate) async fn refresh_apps(st: &mut TuiState) {
    st.sys.refresh_all();
    st.cpu = st.sys.global_cpu_info().cpu_usage();
    st.mem_used_gb = st.sys.used_memory() as f64 / 1_073_741_824.0;
    st.mem_total_gb = st.sys.total_memory() as f64 / 1_073_741_824.0;

    match crate::ipc::send_request("LIST", serde_json::Value::Null).await {
        Ok(res) if res.is_ok() => {
            st.apps = serde_json::from_value(res.data.unwrap_or_default()).unwrap_or_default();
            st.daemon_on = true;
            st.error = None;
        }
        Ok(res) => {
            st.daemon_on = true;
            st.error = Some(res.message);
        }
        Err(_) => {
            st.daemon_on = false;
            let db_rows: Vec<crate::store::db::AppRecord> = tokio::task::spawn_blocking(|| {
                crate::store::db::Database::init()
                    .and_then(|db| db.list_apps())
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            st.apps = db_rows
                .into_iter()
                .map(|r| AppStatus {
                    name: r.name,
                    status: "STOPPED (daemon off)".into(),
                    pid: None,
                    restarts: r.restarts as u32,
                    uptime_secs: 0,
                    executable: r.executable,
                    cwd: r.cwd,
                })
                .collect();
            st.apps.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }
    st.last_refresh = Instant::now();
    if st.selected >= st.filtered().len() && !st.filtered().is_empty() {
        st.selected = st.filtered().len() - 1;
    }
    // Historial global (60 puntos).
    push_hist(&mut st.cpu_hist, st.cpu.max(0.0) as u64);
    push_hist(&mut st.mem_hist, (st.mem_used_gb.max(0.0) * 1024.0) as u64);
    // Historial por app (solo las visibles para no crecer sin cota).
    for app in st.apps.clone() {
        let (cpu_f, mem_mb) = app_live_metrics(&st.sys, app.pid);
        push_hist(
            st.app_cpu_hist.entry(app.name.clone()).or_default(),
            cpu_f as u64,
        );
        push_hist(
            st.app_mem_hist.entry(app.name.clone()).or_default(),
            mem_mb as u64,
        );
    }
    // Preview del seleccionado para el panel derecho.
    if let Some(app) = st.selected_app() {
        st.preview_lines = crate::logs::tail_lines(&app.name, 12);
    } else {
        st.preview_lines.clear();
    }
}

pub(crate) fn push_hist(hist: &mut VecDeque<u64>, v: u64) {
    if hist.len() >= 60 {
        hist.pop_front();
    }
    hist.push_back(v);
}

pub(crate) fn app_live_metrics(sys: &sysinfo::System, pid: Option<u32>) -> (f32, f64) {
    let Some(pid) = pid else {
        return (0.0, 0.0);
    };
    match sys.process(sysinfo::Pid::from_u32(pid)) {
        Some(p) => (p.cpu_usage(), p.memory() as f64 / 1_048_576.0),
        None => (0.0, 0.0),
    }
}

pub(crate) fn load_logs(st: &mut TuiState) {
    st.log_lines = crate::logs::read_lines(&st.log_name);
    if st.log_follow {
        st.log_scroll = 0;
    }
}

pub(crate) fn flush_logs(st: &mut TuiState) {
    let _ = crate::logs::flush(&st.log_name);
    st.log_lines.clear();
    st.log_scroll = 0;
    st.error = Some(format!("Logs de {} vaciados", st.log_name));
}

pub(crate) fn open_logs_for_selected(st: &mut TuiState) {
    if let Some(app) = st.selected_app() {
        st.log_name = app.name.clone();
        st.log_follow = true;
        st.log_scroll = 0;
        load_logs(st);
        st.tab = Tab::Logs;
    }
}

pub(crate) fn refresh_preview(st: &mut TuiState) {
    if let Some(app) = st.selected_app() {
        st.preview_lines = crate::logs::tail_lines(&app.name, 12);
    }
}

pub(crate) fn proc_metrics(sys: &sysinfo::System, pid: Option<u32>) -> (String, String) {
    let (cpu, mem) = app_live_metrics(sys, pid);
    if pid.is_none() {
        return ("-".into(), "-".into());
    }
    let id = sysinfo::Pid::from_u32(pid.unwrap_or(0));
    if sys.process(id).is_none() {
        return ("-".into(), "-".into());
    }
    (format!("{cpu:.1}"), format!("{mem:.0}M"))
}

pub(crate) fn proc_cpu(sys: &sysinfo::System, pid: Option<u32>) -> f32 {
    app_live_metrics(sys, pid).0
}

pub(crate) fn proc_mem_mb(sys: &sysinfo::System, pid: Option<u32>) -> f64 {
    app_live_metrics(sys, pid).1
}

pub(crate) fn fmt_uptime(secs: u64) -> String {
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
