//! Rutas de datos de Wyrm (%ProgramData%/wyrm).
//!
//! Centraliza `data_dir`, `db_path`, `logs_dir` y `log_path_for` para que el
//! resto del código no dependa de `std::env` directamente.

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let mut path = PathBuf::from(
        std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string()),
    );
    path.push("wyrm");
    path
}

pub fn db_path() -> PathBuf {
    data_dir().join("wyrm.db")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn log_path_for(name: &str) -> PathBuf {
    logs_dir().join(format!("{name}.log"))
}
