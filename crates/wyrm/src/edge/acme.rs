//! Certificados ACME (Let's Encrypt, HTTP-01) para el edge.
//!
//! `wyrm cert issue <host> [--staging|--prod]`: crea orden, sirve el reto
//! vía `%ProgramData%/wyrm/certs/.http-01/` (requiere `wyrm edge` en :80),
//! finaliza y guarda `certs/<host>/{privkey,fullchain}.pem` + `meta.json`.
//! `wyrm cert list|renew`: inventario y re-emisión <30d (o --force).

use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, RetryPolicy,
};

pub fn certs_dir() -> std::path::PathBuf {
    crate::store::paths::data_dir().join("certs")
}

fn host_dir(host: &str) -> std::path::PathBuf {
    certs_dir().join(host.to_lowercase())
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '*')
        && !host.starts_with('-')
        && !host.starts_with('.')
}

fn directory_url(staging: bool) -> String {
    if staging {
        LetsEncrypt::Staging.url().to_string()
    } else {
        LetsEncrypt::Production.url().to_string()
    }
}

fn creds_path(staging: bool) -> std::path::PathBuf {
    certs_dir().join(if staging {
        "account-staging.json"
    } else {
        "account-prod.json"
    })
}

async fn account(staging: bool, email: Option<&str>) -> Result<Account, String> {
    let url = directory_url(staging);
    if let Ok(raw) = std::fs::read_to_string(creds_path(staging)) {
        if let Ok(creds) = serde_json::from_str::<AccountCredentials>(&raw) {
            let builder = Account::builder().map_err(|e| e.to_string())?;
            if let Ok(acc) = builder.from_credentials(creds).await {
                return Ok(acc);
            }
            // Credenciales rotas: sigue y crea cuenta nueva.
        }
    }
    let contact: Vec<String> = email.iter().map(|e| format!("mailto:{e}")).collect();
    let contact_refs: Vec<&str> = contact.iter().map(|s| s.as_str()).collect();
    let builder = Account::builder().map_err(|e| e.to_string())?;
    let (acc, creds) = builder
        .create(
            &NewAccount {
                contact: &contact_refs,
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            url,
            None,
        )
        .await
        .map_err(|e| e.to_string())?;
    if let Some(parent) = creds_path(staging).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(raw) = serde_json::to_string_pretty(&creds) {
        let _ = std::fs::write(creds_path(staging), raw);
    }
    Ok(acc)
}

pub async fn issue(host: &str, staging: bool, email: Option<&str>) -> Result<(), String> {
    let host = host.to_lowercase();
    if !valid_host(&host) {
        return Err(format!("host inválido: {host}"));
    }
    println!(
        "ACME {} para {host} (edge debe servir :80)",
        if staging { "staging" } else { "production" }
    );
    let acc = account(staging, email).await?;
    let mut order = acc
        .new_order(&NewOrder::new(&[Identifier::Dns(host.clone())]))
        .await
        .map_err(|e| e.to_string())?;

    let challenges = order.authorizations();
    futures_util::pin_mut!(challenges);
    while let Some(auth) = challenges.next().await {
        let mut auth = auth.map_err(|e| e.to_string())?;
        if !matches!(auth.status, AuthorizationStatus::Pending) {
            continue;
        }
        let mut chall = auth
            .challenge(ChallengeType::Http01)
            .ok_or("la CA no ofreció http-01")?;
        let key_auth = chall.key_authorization().as_str().to_string();
        let dir = crate::edge::challenges_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(&chall.token), &key_auth).map_err(|e| e.to_string())?;
        println!("reto listo, avisando a la CA…");
        chall.set_ready().await.map_err(|e| e.to_string())?;
    }

    order
        .poll_ready(&RetryPolicy::default())
        .await
        .map_err(|e| e.to_string())?;
    let key_pem = order.finalize().await.map_err(|e| e.to_string())?;
    let chain_pem = order
        .poll_certificate(&RetryPolicy::default())
        .await
        .map_err(|e| e.to_string())?;

    let dir = host_dir(&host);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("privkey.pem"), &key_pem).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("fullchain.pem"), &chain_pem).map_err(|e| e.to_string())?;
    let meta = serde_json::json!({
        "host": host,
        "kind": if staging { "staging" } else { "prod" },
        "issued_at_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "valid_days": 90,
    });
    let _ = std::fs::write(
        dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    );
    // Limpia el reto.
    for entry in std::fs::read_dir(crate::edge::challenges_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        let _ = std::fs::remove_file(entry.path());
    }
    println!("Certificado para {host} guardado en {}", dir.display());
    Ok(())
}

/// Cert autofirmado para dev/CI (misma carpeta que ACME, `kind: local`).
/// No valida nada: solo sirve para probar el TLS del edge sin DNS público.
pub fn self_signed(host: &str) -> Result<(), String> {
    let host = host.to_lowercase();
    if !valid_host(&host) || host.starts_with("*.") {
        return Err(format!("host inválido para self-signed: {host}"));
    }
    let key = rcgen::generate_simple_self_signed(vec![host.clone()]).map_err(|e| e.to_string())?;
    let dir = host_dir(&host);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("privkey.pem"), key.signing_key.serialize_pem())
        .map_err(|e| e.to_string())?;
    std::fs::write(dir.join("fullchain.pem"), key.cert.pem()).map_err(|e| e.to_string())?;
    let meta = serde_json::json!({
        "host": host,
        "kind": "local",
        "issued_at_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "valid_days": 90,
    });
    let _ = std::fs::write(
        dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    );
    println!("Self-signed para {host} en {}", dir.display());
    Ok(())
}

#[derive(Debug)]
pub struct CertInfo {
    pub host: String,
    pub kind: String,
    pub issued_at_unix: u64,
    pub days_left: i64,
}

fn read_meta(dir: &std::path::Path) -> (String, u64) {
    std::fs::read_to_string(dir.join("meta.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| {
            let kind = v
                .get("kind")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    if v.get("staging").and_then(|x| x.as_bool()).unwrap_or(false) {
                        "staging".into()
                    } else {
                        "prod".into()
                    }
                });
            let issued = v
                .get("issued_at_unix")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            (kind, issued)
        })
        .unwrap_or(("prod".into(), 0))
}

pub fn list() -> Vec<CertInfo> {
    let mut out = vec![];
    let Ok(entries) = std::fs::read_dir(certs_dir()) else {
        return out;
    };
    for entry in entries.flatten() {
        let host = entry.file_name().to_string_lossy().to_string();
        if host.starts_with('.') || !entry.path().join("fullchain.pem").exists() {
            continue;
        }
        let (kind, issued) = read_meta(&entry.path());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(CertInfo {
            host,
            kind,
            issued_at_unix: issued,
            days_left: 90 - now.saturating_sub(issued) as i64 / 86400,
        });
    }
    out.sort_by(|a, b| a.host.cmp(&b.host));
    out
}
