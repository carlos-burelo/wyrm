use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

pub const PIPE_NAME: &str = r"\\.\pipe\wyrm_ipc";

pub async fn send_command(action: &str, payload: serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let mut client = ClientOptions::new().open(PIPE_NAME)?;
    let msg = serde_json::json!({
        "action": action,
        "payload": payload
    }).to_string();

    client.write_all(msg.as_bytes()).await?;

    let mut buffer = vec![0u8; 4096];
    let n = client.read(&mut buffer).await?;
    let response = String::from_utf8_lossy(&buffer[..n]).to_string();

    Ok(response)
}

pub async fn run_ipc_server() -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(PIPE_NAME)?;

        server.connect().await?;

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 4096];
            if let Ok(n) = server.read(&mut buffer).await {
                let request_str = String::from_utf8_lossy(&buffer[..n]);
                let response = if let Ok(val) = serde_json::from_str::<serde_json::Value>(&request_str) {
                    let action = val["action"].as_str().unwrap_or("UNKNOWN");
                    format!("{{\"status\":\"ok\",\"action\":{:?}}}", action)
                } else {
                    "{\"status\":\"error\",\"message\":\"Invalid JSON Payload\"}".to_string()
                };

                let _ = server.write_all(response.as_bytes()).await;
            }
        });
    }
}