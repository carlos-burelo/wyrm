//! Logs por app: lectura, tail y vaciado.
//!
//! Fuente de verdad del path: `store::paths::log_path_for`.
//! Usado por `cli logs` y la TUI (preview + vista full).

use std::path::{Path, PathBuf};

pub fn path_for(name: &str) -> PathBuf {
    crate::store::paths::log_path_for(name)
}

/// Todas las líneas del log; si no existe, una línea placeholder.
pub fn read_lines(name: &str) -> Vec<String> {
    std::fs::read_to_string(path_for(name))
        .unwrap_or_else(|_| "(sin logs todavía)".into())
        .lines()
        .map(|s| s.to_string())
        .collect()
}

/// Últimas `n` líneas (para previews y `wyrm logs --lines`).
pub fn tail_lines(name: &str, n: usize) -> Vec<String> {
    let all = read_lines(name);
    let start = all.len().saturating_sub(n);
    all[start..].to_vec()
}

/// Trunca el log a cero bytes.
pub fn flush(name: &str) -> std::io::Result<()> {
    std::fs::write(path_for(name), "")
}

/// Tamaño máximo antes de rotar (10 MiB) y cuántos históricos conservar.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const KEEP: usize = 5;

/// Rota `path` si supera `max_bytes`: `app.log` → `app.log.1` … `app.log.KEEP`.
/// Se llama antes de abrir en append, así el proceso nunca escribe sin cota.
pub fn rotate_path(path: &Path, max_bytes: u64, keep: usize) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= max_bytes {
        return;
    }
    let oldest = PathBuf::from(format!("{}.{keep}", path.display()));
    let _ = std::fs::remove_file(&oldest);
    for i in (1..keep).rev() {
        let from = PathBuf::from(format!("{}.{i}", path.display()));
        let to = PathBuf::from(format!("{}.{i_plus}", path.display(), i_plus = i + 1));
        if from.exists() {
            let _ = std::fs::rename(&from, &to);
        }
    }
    let first = PathBuf::from(format!("{}.1", path.display()));
    let _ = std::fs::rename(path, &first);
}

/// Rota el log de `name` con los defaults (10 MiB, 5 históricos).
pub fn rotate_if_needed(name: &str) {
    rotate_path(&path_for(name), MAX_BYTES, KEEP);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rota_y_conserva_historicos() {
        let dir = std::env::temp_dir().join(format!(
            "wyrm-logrot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        std::fs::write(&path, vec![b'x'; 100]).unwrap();
        std::fs::write(dir.join("app.log.1"), "viejo").unwrap();

        rotate_path(&path, 10, 3);

        assert!(std::fs::metadata(&path).is_err());
        assert_eq!(std::fs::read(dir.join("app.log.2")).unwrap(), b"viejo");
        assert_eq!(std::fs::metadata(dir.join("app.log.1")).unwrap().len(), 100);
    }

    #[test]
    fn no_rota_si_no_excede() {
        let dir = std::env::temp_dir().join(format!(
            "wyrm-logrot2-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        std::fs::write(&path, "poco").unwrap();
        rotate_path(&path, 10 * 1024 * 1024, 5);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "poco");
    }
}
