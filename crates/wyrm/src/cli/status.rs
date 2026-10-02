//! `wyrm status`: detalle JSON de una app.

pub async fn cmd_status(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    match crate::ipc::send_request("STATUS", serde_json::json!({ "name": name })).await {
        Ok(res) if res.is_ok() => {
            println!(
                "{}",
                serde_json::to_string_pretty(&res.data.unwrap_or_default())?
            );
        }
        Ok(res) => eprintln!("Error: {}", res.message),
        Err(e) => eprintln!("Daemon no disponible: {e}"),
    }
    Ok(())
}
