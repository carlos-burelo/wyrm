//! Handlers: salud, apps y métricas. Hablan con el Daemon vía `handle()`.

use super::ApiState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

pub async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "wyrm" }))
}

fn daemon_data(
    res: crate::ipc::protocol::Response,
) -> Result<serde_json::Value, (StatusCode, String)> {
    if res.is_ok() {
        Ok(res.data.unwrap_or(serde_json::Value::Null))
    } else {
        Err((StatusCode::BAD_REQUEST, res.message))
    }
}

pub async fn list_apps(State(s): State<ApiState>) -> impl IntoResponse {
    let req = crate::ipc::protocol::Request::new("LIST", serde_json::Value::Null);
    match daemon_data(s.daemon.handle(req).await) {
        Ok(d) => (StatusCode::OK, Json(d)).into_response(),
        Err((c, m)) => (c, m).into_response(),
    }
}

pub async fn app_status(State(s): State<ApiState>, Path(name): Path<String>) -> impl IntoResponse {
    let req = crate::ipc::protocol::Request::new("STATUS", json!({ "name": name }));
    match daemon_data(s.daemon.handle(req).await) {
        Ok(d) => (StatusCode::OK, Json(d)).into_response(),
        Err((c, m)) => (c, m).into_response(),
    }
}

async fn action(
    s: &ApiState,
    action: &str,
    name: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let req = crate::ipc::protocol::Request::new(action, json!({ "name": name }));
    daemon_data(s.daemon.handle(req).await)
}

pub async fn app_stop(State(s): State<ApiState>, Path(name): Path<String>) -> impl IntoResponse {
    match action(&s, "STOP", &name).await {
        Ok(d) => (StatusCode::OK, Json(d)).into_response(),
        Err((c, m)) => (c, m).into_response(),
    }
}

pub async fn app_restart(State(s): State<ApiState>, Path(name): Path<String>) -> impl IntoResponse {
    match action(&s, "RESTART", &name).await {
        Ok(d) => (StatusCode::OK, Json(d)).into_response(),
        Err((c, m)) => (c, m).into_response(),
    }
}

pub async fn app_start(State(s): State<ApiState>, Path(name): Path<String>) -> impl IntoResponse {
    let cfg = tokio::task::spawn_blocking(move || {
        crate::store::db::Database::init()
            .and_then(|db| db.get_app(&name))
            .unwrap_or(None)
            .map(|r| r.to_app_config())
    })
    .await
    .unwrap_or(None);
    let Some(cfg) = cfg else {
        return (StatusCode::NOT_FOUND, format!("App desconocida")).into_response();
    };
    let payload = match serde_json::to_value(&cfg) {
        Ok(p) => p,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let req = crate::ipc::protocol::Request::new("START", payload);
    match daemon_data(s.daemon.handle(req).await) {
        Ok(d) => (StatusCode::OK, Json(d)).into_response(),
        Err((c, m)) => (c, m).into_response(),
    }
}

pub async fn metrics(State(s): State<ApiState>) -> impl IntoResponse {
    let req = crate::ipc::protocol::Request::new("LIST", serde_json::Value::Null);
    let apps: Vec<crate::daemon::AppStatus> = s
        .daemon
        .handle(req)
        .await
        .data
        .and_then(|d| serde_json::from_value(d).ok())
        .unwrap_or_default();

    let enriched = tokio::task::spawn_blocking(move || {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes();
        apps.into_iter()
            .map(|a| {
                let (cpu, mem) = a
                    .pid
                    .and_then(|p| sys.process(sysinfo::Pid::from_u32(p)))
                    .map(|proc| (proc.cpu_usage() as f64, proc.memory()))
                    .unwrap_or((0.0, 0));
                super::metrics::AppMetric {
                    name: a.name,
                    status: a.status,
                    pid: a.pid,
                    restarts: a.restarts,
                    uptime_secs: a.uptime_secs,
                    cpu,
                    mem_bytes: mem,
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        super::metrics::render(&enriched),
    )
        .into_response()
}
