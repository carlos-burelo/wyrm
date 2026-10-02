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
