//! API HTTP local: plano de control para el futuro PAAS.
//!
//! `127.0.0.1:8379` (env `WYRM_API_ADDR`), auth Bearer con token en
//! `%ProgramData%/wyrm/token` (`wyrm token [--rotate]`). `/health` público,
//! resto autenticado. Si el bind falla, el demonio sigue sin API.

pub mod metrics;
pub mod routes;

use axum::{
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct ApiState {
    pub daemon: Arc<crate::daemon::Daemon>,
    pub token: String,
}

pub fn token_path() -> std::path::PathBuf {
    crate::store::paths::data_dir().join("token")
}

fn new_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Lee el token o lo crea (64 hex). Falla solo si no hay FS.
pub fn load_or_create_token() -> Result<String, Box<dyn std::error::Error>> {
    let path = token_path();
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if t.len() >= 16 {
            return Ok(t);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let t = new_token();
    std::fs::write(&path, format!("{t}\n"))?;
    Ok(t)
}

pub fn rotate_token() -> Result<String, Box<dyn std::error::Error>> {
    let path = token_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let t = new_token();
    std::fs::write(&path, format!("{t}\n"))?;
    Ok(t)
}

async fn auth(
    state: axum::extract::State<ApiState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if req.uri().path() == "/health" {
        return next.run(req).await;
    }
    let ok = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v == format!("Bearer {}", state.token))
        .unwrap_or(false);
    if ok {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "unauthorized: usa `wyrm token`").into_response()
    }
}

pub async fn serve(daemon: Arc<crate::daemon::Daemon>) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("WYRM_NO_API").is_ok() {
        return Ok(());
    }
    let addr = std::env::var("WYRM_API_ADDR").unwrap_or_else(|_| "127.0.0.1:8379".into());
    let token = match load_or_create_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[wyrm] API desactivada (token): {e}");
            return Ok(());
        }
    };
    let state = ApiState { daemon, token };
    let app = axum::Router::new()
        .route("/health", axum::routing::get(routes::health))
        .route("/apps", axum::routing::get(routes::list_apps))
        .route("/apps/{name}", axum::routing::get(routes::app_status))
        .route("/apps/{name}/start", axum::routing::post(routes::app_start))
        .route("/apps/{name}/stop", axum::routing::post(routes::app_stop))
        .route(
            "/apps/{name}/restart",
            axum::routing::post(routes::app_restart),
        )
        .route("/metrics", axum::routing::get(routes::metrics))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[wyrm] API no disponible en {addr}: {e}");
            return Ok(());
        }
    };
    println!(
        "Wyrm API en http://{addr} (token en {})",
        token_path().display()
    );
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("API caída: {e}").into())
}
