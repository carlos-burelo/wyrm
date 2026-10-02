//! TLS del edge: SNI → cert de `certs/<host>/`, recarga cada 60s.
//!
//! Acepta con tokio-rustls y sirve el mismo Router axum vía hyper http1.
//! Sin cert para el SNI → handshake fallido (el :80 sigue proxyando).

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tokio_rustls::rustls::{
    self,
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use tower::ServiceExt;

type CertMap = HashMap<String, Arc<CertifiedKey>>;

struct SniStore {
    certs_dir: std::path::PathBuf,
    map: RwLock<CertMap>,
    loaded_at: RwLock<SystemTime>,
}

impl SniStore {
    fn load(dir: &std::path::Path) -> CertMap {
        let mut map = CertMap::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return map;
        };
        for entry in entries.flatten() {
            let host = entry.file_name().to_string_lossy().to_lowercase();
            if host.starts_with('.') {
                continue;
            }
            let chain_path = entry.path().join("fullchain.pem");
            let key_path = entry.path().join("privkey.pem");
            if !chain_path.exists() || !key_path.exists() {
                continue;
            }
            match load_keyed(&chain_path, &key_path) {
                Ok(key) => {
                    map.insert(host, Arc::new(key));
                }
                Err(e) => eprintln!("[edge-tls] {host}: {e}"),
            }
        }
        map
    }

    fn maybe_reload(&self) {
        let stale = self
            .loaded_at
            .read()
            .map(|t| t.elapsed().map(|d| d.as_secs() >= 60).unwrap_or(true))
            .unwrap_or(true);
        if !stale {
            return;
        }
        let fresh = Self::load(&self.certs_dir);
        if let Ok(mut map) = self.map.write() {
            *map = fresh;
        }
        if let Ok(mut t) = self.loaded_at.write() {
            *t = SystemTime::now();
        }
    }
}

fn load_keyed(chain: &std::path::Path, key: &std::path::Path) -> Result<CertifiedKey, String> {
    let chain_pem = std::fs::read(chain).map_err(|e| e.to_string())?;
    let key_pem = std::fs::read(key).map_err(|e| e.to_string())?;
    let mut chain_cur = std::io::Cursor::new(chain_pem);
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut chain_cur)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| c.into_owned())
            .collect();
    if certs.is_empty() {
        return Err("fullchain sin certificados".into());
    }
    let mut key_cur = std::io::Cursor::new(key_pem);
    let key_der = rustls_pemfile::private_key(&mut key_cur)
        .map_err(|e| e.to_string())?
        .ok_or("privkey sin clave PKCS8/RSA/SEC1".to_string())?;
    let provider = rustls::crypto::ring::default_provider();
    let signing = provider
        .key_provider
        .load_private_key(key_der)
        .map_err(|e| format!("clave inválida: {e}"))?;
    Ok(CertifiedKey::new(certs, signing))
}

impl std::fmt::Debug for SniStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SniStore").finish()
    }
}

impl SniStore {
    fn has_certs(&self) -> bool {
        self.map.read().map(|m| !m.is_empty()).unwrap_or(false)
    }
}

impl ResolvesServerCert for SniStore {
    fn resolve(&self, hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        self.maybe_reload();
        let name = hello.server_name()?.to_lowercase();
        let map = self.map.read().ok()?;
        if let Some(k) = map.get(&name) {
            return Some(k.clone());
        }
        // Wildcard *.example.com
        if let Some((_, parent)) = name.split_once('.') {
            if let Some(k) = map.get(&format!("*.{parent}")) {
                return Some(k.clone());
            }
        }
        None
    }
}

/// Sirve HTTPS en `port` con el mismo `router` del proxy :80.
pub async fn serve_tls(router: axum::Router, port: u16) -> Result<(), String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let store = Arc::new(SniStore {
        certs_dir: super::acme::certs_dir(),
        map: RwLock::new(SniStore::load(&super::acme::certs_dir())),
        loaded_at: RwLock::new(SystemTime::now()),
    });
    if !store.has_certs() {
        return Err("sin certificados (wyrm cert issue <host>)".into());
    }
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(store);
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("no se pudo escuchar en {addr}: {e}"))?;
    println!("Wyrm edge TLS en https://{addr} (SNI)");
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let acceptor = acceptor.clone();
        let router = router.clone();
        tokio::spawn(async move {
            let Ok(tls) = acceptor.accept(stream).await else {
                return;
            };
            let svc = service_fn(move |req: axum::http::Request<Incoming>| {
                let router = router.clone();
                async move {
                    router
                        .oneshot(req)
                        .await
                        .map_err(|e| format!("edge-tls: {e}"))
                }
            });
            let _ = http1::Builder::new()
                .serve_connection(TokioIo::new(tls), svc)
                .await;
        });
    }
}
