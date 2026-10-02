//! `wyrm logs`: tail + follow de `%ProgramData%/wyrm/logs/<name>.log`.

pub async fn cmd_logs(
    name: &str,
    lines: usize,
    follow: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = crate::store::db::Database::log_path_for(name);
    if !path.exists() {
        eprintln!("Sin logs en {} (¿la app existe?)", path.display());
        return Ok(());
    }
    if follow {
        println!("Siguiendo {} (Ctrl+C para salir)…", path.display());
        let mut last = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len < last {
                last = 0;
            }
            if len > last {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let bytes = content.as_bytes();
                // Imprime solo lo nuevo de forma aproximada.
                let from = usize::try_from(last).unwrap_or(0).min(bytes.len());
                print!("{}", &content[from.min(content.len())..]);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                last = len;
                let _ = bytes;
            }
        }
    } else {
        let content = std::fs::read_to_string(&path)?;
        let all: Vec<&str> = content.lines().collect();
        let start = all.len().saturating_sub(lines);
        for l in &all[start..] {
            println!("{l}");
        }
    }
    Ok(())
}
