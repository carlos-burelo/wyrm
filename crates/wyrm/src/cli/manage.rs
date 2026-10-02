//! `wyrm stop/restart/delete`: acciones simples contra el demonio con
//! fallback a DB cuando el demonio está caído.

pub async fn cmd_simple(action: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    match crate::ipc::send_request(action, serde_json::json!({ "name": name })).await {
        Ok(res) if res.is_ok() => {
            println!("{} {}: {}", action, name, res.message);
            if let Some(d) = res.data {
                println!("{d}");
            }
        }
        Ok(res) => eprintln!("Error: {}", res.message),
        Err(e) => {
            // Fallback DB para STOP: marca STOPPED aunque el daemon esté caído.
            if action == "STOP" {
                if let Ok(db) = crate::store::db::Database::init() {
                    let _ = db.update_status(name, "STOPPED", false);
                }
            }
            eprintln!("No se pudo contactar al demonio: {e}");
        }
    }
    Ok(())
}

pub async fn cmd_delete(name: &str, yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !yes {
        eprintln!("Confirma con `wyrm delete {name} --yes`");
        return Ok(());
    }
    match crate::ipc::send_request("DELETE", serde_json::json!({ "name": name })).await {
        Ok(res) => println!("{}", res.message),
        Err(e) => {
            // Fallback: borra de DB.
            match crate::store::db::Database::init()?.delete_app(name) {
                Ok(true) => println!("{name} eliminada de DB (daemon no disponible)."),
                _ => eprintln!("No se pudo eliminar: {e}"),
            }
        }
    }
    Ok(())
}
