//! Edge: proxy reverso HTTP host → target + retos ACME HTTP-01.
//!
//! `wyrm edge` escucha en `WYRM_EDGE_PORT` (default 80) y reenvía según la
//! tabla `routes` (`wyrm route add <host> <target>`). Bufferiza cuerpo
//! (sin websockets todavía). Los retos `/.well-known/acme-challenge/*` se
//! sirven desde `%ProgramData%/wyrm/certs/.http-01/` para `wyrm cert`.

pub mod acme;

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

#[derive(Clone)]
struct EdgeState {
    client: reqwest::Client,
}

pub fn challenges_dir() -> std::path::PathBuf {
    crate::store::paths::data_dir()
        .join("certs")
        .join(".http-01")
}

fn hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

async fn proxy(State(st): State<EdgeState>, req: axum::http::Request<Body>) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let path = req.uri().path().to_string();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();

    // Reto ACME HTTP-01 (lo escribe `wyrm cert issue`).
    if let Some(token) = path.strip_prefix("/.well-known/acme-challenge/") {
        if !token.contains('/') && !token.contains('\\') {
            let file = challenges_dir().join(token);
            if let Ok(body) = std::fs::read(&file) {
                return (StatusCode::OK, body).into_response();
            }
        }
        return (StatusCode::NOT_FOUND, "no such challenge").into_response();
    }

    if host.is_empty() {
        return (StatusCode::BAD_REQUEST, "falta Host").into_response();
    }
    let lookup = host.clone();
    let target: Option<String> = tokio::task::spawn_blocking(move || {
        crate::store::db::Database::init()
            .and_then(|db| db.get_route(&lookup))
            .unwrap_or(None)
    })
    .await
    .unwrap_or(None);
    let Some(target) = target else {
        return (
            StatusCode::NOT_FOUND,
            format!("host desconocido: registra con `wyrm route add {host} <target>`"),
        )
            .into_response();
    };

    let url = format!("{base}{path}{query}", base = target.trim_end_matches('/'));
    let method = req.method().clone();
    let mut headers = HeaderMap::new();
    for (k, v) in req.headers().iter() {
        let name = k.as_str().to_lowercase();
        if name == "host" || hop_by_hop(&name) {
            continue;
        }
        headers.append(k, v.clone());
    }
    headers.insert("x-forwarded-proto", "http".parse().unwrap());
    headers.insert(
        "x-forwarded-host",
        host.parse().unwrap_or_else(|_| "unknown".parse().unwrap()),
    );
    let body = match axum::body::to_bytes(req.into_body(), 16 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "cuerpo > 16MB").into_response(),
    };

    let res = st
        .client
        .request(method, &url)
        .headers(headers)
        .body(body)
        .send()
        .await;
    match res {
        Ok(up) => {
            let status = up.status();
            let mut headers = HeaderMap::new();
            for (k, v) in up.headers().iter() {
                if hop_by_hop(&k.as_str().to_lowercase()) {
                    continue;
                }
                headers.append(k, v.clone());
            }
            let bytes = up.bytes().await.unwrap_or_default();
            (status, headers, bytes).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("target {target} no responde: {e}"),
        )
            .into_response(),
    }
}

/// Proxy en foreground. Puerto `WYRM_EDGE_PORT` (default 80, requiere admin).
pub async fn run_edge() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("WYRM_EDGE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);
    std::fs::create_dir_all(challenges_dir()).ok();
    let state = EdgeState {
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?,
    };
    let app = axum::Router::new().fallback(proxy).with_state(state);
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("No se pudo escuchar en {addr} (¿admin? ¿puerto libre?): {e}"))?;
    println!("Wyrm edge en http://{addr} (rutas con `wyrm route list`)");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Edge caído: {e}").into())
}
