//! `wyrm route`: alta/baja/lista de rutas host → target del edge.

pub async fn cmd_route(action: &str, rest: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let db = crate::store::db::Database::init()?;
    match action {
        "add" => {
            let (Some(host), Some(target)) = (rest.first(), rest.get(1)) else {
                eprintln!("Uso: wyrm route add <host> <http://127.0.0.1:3000>");
                return Ok(());
            };
            if !(target.starts_with("http://") || target.starts_with("https://")) {
                return Err("target debe empezar con http:// o https://".into());
            }
            db.upsert_route(host, target)?;
            println!("Ruta {host} → {target}");
        }
        "rm" | "del" | "remove" => {
            let Some(host) = rest.first() else {
                eprintln!("Uso: wyrm route rm <host>");
                return Ok(());
            };
            if db.delete_route(host)? {
                println!("Ruta {host} eliminada");
            } else {
                eprintln!("Sin ruta para {host}");
            }
        }
        _ => {
            let rows = db.list_routes()?;
            if rows.is_empty() {
                println!("Sin rutas. `wyrm route add <host> <target>`.");
                return Ok(());
            }
            println!("{:<30} {}", "HOST", "TARGET");
            for r in rows {
                println!("{:<30} {}", r.host, r.target);
            }
        }
    }
    Ok(())
}

pub async fn cmd_edge() -> Result<(), Box<dyn std::error::Error>> {
    crate::edge::run_edge().await
}
