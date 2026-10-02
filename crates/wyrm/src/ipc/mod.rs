pub mod protocol;

use crate::ipc::protocol::{Request, Response};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

pub const PIPE_NAME: &str = r"\\.\pipe\wyrm_ipc";
const MAX_FRAME: usize = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn send_request(
    action: &str,
    payload: serde_json::Value,
) -> Result<Response, Box<dyn std::error::Error>> {
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
    let resp: Response =
        serde_json::from_slice(&buf).map_err(|e| format!("Respuesta inválida del demonio: {e}"))?;
    Ok(resp)
}

pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync + 'static>;

pub async fn run_ipc_server_with(handler: Handler) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Sin first_pipe_instance: varias instancias conviven mientras cada
        // handler atiende a su cliente. Errores transitorios (p. ej. otro
        // demonio con el pipe) reintentan en vez de matar al demonio.
        let server = match ServerOptions::new().create(PIPE_NAME) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[wyrm] pipe ocupado, reintentando: {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        if let Err(e) = server.connect().await {
            eprintln!("[wyrm] pipe connect: {e}");
            continue;
        }
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
