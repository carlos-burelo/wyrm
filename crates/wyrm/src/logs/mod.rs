//! Logs por app: lectura, tail y vaciado.
//!
//! Fuente de verdad del path: `store::paths::log_path_for`.
//! Usado por `cli logs` y la TUI (preview + vista full).

use std::path::PathBuf;

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
