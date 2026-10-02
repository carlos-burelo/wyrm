//! `wyrm start`: auto-detecta el proyecto y registra la app.

use std::path::PathBuf;

pub async fn cmd_start(
    name: Option<String>,
    cwd: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_dir = cwd
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let app_config = crate::runtime::inspector::inspect_and_configure(&target_dir, name)?;

    {
        let db = crate::store::db::Database::init()?;
        db.save_app(&app_config)?;
    }

    println!("Registrando aplicación: {}", app_config.name);
    match crate::ipc::send_request("START", serde_json::to_value(&app_config)?).await {
        Ok(res) if res.is_ok() => println!("OK {} -> {}", app_config.name, res.message),
        Ok(res) => eprintln!("Demonio respondió error: {}", res.message),
        Err(e) => {
            eprintln!("Aviso: {e}");
            eprintln!("La app quedó guardada en DB. Arranca el demonio con `wyrm daemon` o instala el servicio con `wyrm service install`.");
        }
    }
    Ok(())
}

/// `wyrm start --all`: levanta todas las apps del ecosystem file.
pub async fn cmd_start_all(file: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let path = match file {
        Some(p) => p,
        None => crate::ecosystem::Ecosystem::find(&cwd).ok_or(
            "No se encontró wyrm.json ni ecosystem.json en el directorio actual (usa --file).",
        )?,
    };
    let base = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| cwd.clone());
    let eco = crate::ecosystem::Ecosystem::load(&path)?;
    println!("Ecosystem {}: {} app(s)", path.display(), eco.apps.len());

    if !eco.routes.is_empty() {
        if let Ok(db) = crate::store::db::Database::init() {
            for r in &eco.routes {
                if r.target.starts_with("http://") || r.target.starts_with("https://") {
                    let _ = db.upsert_route(&r.host, &r.target);
                    println!("Ruta {} → {}", r.host, r.target);
                } else {
                    eprintln!("Ruta {} ignorada: target debe ser http(s)", r.host);
                }
            }
        }
    }

    let mut ok = 0;
    for app in &eco.apps {
        match crate::ecosystem::resolve(&base, app) {
            Ok(cfg) => {
                if let Ok(db) = crate::store::db::Database::init() {
                    let _ = db.save_app(&cfg);
                }
                match crate::ipc::send_request("START", serde_json::to_value(&cfg)?).await {
                    Ok(res) if res.is_ok() => {
                        println!("OK {} -> {}", cfg.name, res.message);
                        ok += 1;
                    }
                    Ok(res) => eprintln!("FAIL {}: {}", cfg.name, res.message),
                    Err(e) => eprintln!(
                        "FAIL {}: demonio no disponible ({e}); quedó guardada en DB.",
                        cfg.name
                    ),
                }
            }
            Err(e) => eprintln!("FAIL {}: {e}", app.name),
        }
    }
    println!("{ok}/{} apps iniciadas.", eco.apps.len());
    Ok(())
}
