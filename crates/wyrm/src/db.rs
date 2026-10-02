use rusqlite::{params, Connection, Result};
use std::fs;
use std::path::PathBuf;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn init() -> Result<Self> {
        let mut path = PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string()));
        path.push("wyrm");
        fs::create_dir_all(&path).ok();
        path.push("wyrm.db");

        let conn = Connection::open(path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS apps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                executable TEXT NOT NULL,
                args TEXT NOT NULL,
                cwd TEXT NOT NULL,
                env TEXT NOT NULL,
                status TEXT NOT NULL,
                restarts INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn save_app(&self, app: &crate::inspector::AppConfig) -> Result<()> {
        let args_json = serde_json::to_string(&app.args).unwrap_or_default();
        let env_json = serde_json::to_string(&app.env).unwrap_or_default();

        self.conn.execute(
            "INSERT INTO apps (name, executable, args, cwd, env, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'STOPPED')
             ON CONFLICT(name) DO UPDATE SET
                executable=excluded.executable,
                args=excluded.args,
                cwd=excluded.cwd,
                env=excluded.env",
            params![app.name, app.executable, args_json, app.cwd.to_str().unwrap(), env_json],
        )?;
        Ok(())
    }

    pub fn update_status(&self, name: &str, status: &str, increment_restarts: bool) -> Result<()> {
        if increment_restarts {
            self.conn.execute(
                "UPDATE apps SET status = ?1, restarts = restarts + 1 WHERE name = ?2",
                params![status, name],
            )?;
        } else {
            self.conn.execute(
                "UPDATE apps SET status = ?1 WHERE name = ?2",
                params![status, name],
            )?;
        }
        Ok(())
    }
}