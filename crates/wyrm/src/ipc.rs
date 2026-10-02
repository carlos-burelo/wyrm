use crate::protocol::{Request, Response};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

pub const PIPE_NAME: &str = r"\\.\pipe\wyrm_ipc";
const MAX_FRAME: usize = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn send_request(action: &str, payload: serde_json::Value) -> Result<Response, Box<dyn std::error::Error>> {
    let mut client = ClientOptions::new().open(PIPE_NAME).map_err(|e| {
        format!("No se pudo conectar al demonio Wyrm ({PIPE_NAME}): {e}. ¿Está corriendo `wyrm daemon` o el servicio?")
    })?;

    let req = Request::new(action, payload);
    let mut line = serde_json::to_string(&req)?;
    line.push('\n');

    tokio::time::timeout(IO_TIMEOUT, client.write_all(line.as_bytes()))
        .await
        .map_err(|_| "Timeout escribiendo al demonio")??;

    let mut reader = BufReader::new(client);
    let mut buf = Vec::with_capacity(4096);
    tokio::time::timeout(IO_TIMEOUT, reader.read_until(b'\n', &mut buf))
        .await
        .map_err(|_| "Timeout esperando respuesta del demonio")??;

    if buf.is_empty() {
        return Err("El demonio cerró la conexión sin responder".into());
    }
    let resp: Response = serde_json::from_slice(&buf)
        .map_err(|e| format!("Respuesta inválida del demonio: {e}"))?;
    Ok(resp)
}

/// Compat: devuelve string legible para la CLI vieja.
pub async fn send_command(action: &str, payload: serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let r = send_request(action, payload).await?;
    if r.is_ok() {
        Ok(r.data.map(|d| d.to_string()).unwrap_or(r.message))
    } else {
        Err(r.message.into())
    }
}

pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync + 'static>;

pub async fn run_ipc_server_with(handler: Handler) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(PIPE_NAME)?;

        server.connect().await?;
        let h = handler.clone();

        tokio::spawn(async move {
            let mut server = server;
            let mut reader = BufReader::new(&mut server);
            let mut line = Vec::with_capacity(4096);
            let response = match reader.read_until(b'\n', &mut line).await {
                Ok(0) | Ok(_) if line.is_empty() => Response::err("Petición vacía"),
                Ok(_) => {
                    if line.len() > MAX_FRAME {
                        Response::err("Petición demasiado grande")
                    } else {
                        match serde_json::from_slice::<Request>(&line) {
                            Ok(req) => (h)(req),
                            Err(e) => Response::err(format!("JSON inválido: {e}")),
                        }
                    }
                }
                Err(e) => Response::err(format!("Error leyendo pipe: {e}")),
            };
            drop(reader);
            if let Ok(mut s) = serde_json::to_string(&response) {
                s.push('\n');
                let _ = server.write_all(s.as_bytes()).await;
            }
            let _ = server.disconnect();
        });
    }
}

/// Servidor dummy para compatibilidad cuando no hay daemon (eco).
pub async fn run_ipc_server() -> Result<(), Box<dyn std::error::Error>> {
    let echo: Handler = Arc::new(|req: Request| Response::ok(&req.action, req.payload));
    run_ipc_server_with(echo).await
}
