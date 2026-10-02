use rusqlite::{params, Connection, Result, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRecord {
    pub name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: HashMap<String, String>,
    pub status: String,
    pub restarts: i64,
    pub created_at: String,
}

impl AppRecord {
    fn from_row(row: &Row) -> Result<Self> {
        let args_json: String = row.get(2)?;
        let env_json: String = row.get(4)?;
        Ok(Self {
            name: row.get(0)?,
            executable: row.get(1)?,
            args: serde_json::from_str(&args_json).unwrap_or_default(),
            cwd: row.get(3)?,
            env: serde_json::from_str(&env_json).unwrap_or_default(),
            status: row.get(5)?,
            restarts: row.get(6)?,
            created_at: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        })
    }

    pub fn to_app_config(&self) -> crate::inspector::AppConfig {
        crate::inspector::AppConfig {
            name: self.name.clone(),
            executable: self.executable.clone(),
            args: self.args.clone(),
            cwd: PathBuf::from(&self.cwd),
            env: self.env.clone(),
        }
    }
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn data_dir() -> PathBuf {
        let mut path = PathBuf::from(
            std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string()),
        );
        path.push("wyrm");
        path
    }

    pub fn db_path() -> PathBuf {
        Self::data_dir().join("wyrm.db")
    }

    pub fn logs_dir() -> PathBuf {
        Self::data_dir().join("logs")
    }

    pub fn init() -> Result<Self> {
        let dir = Self::data_dir();
        fs::create_dir_all(&dir).ok();
        fs::create_dir_all(Self::logs_dir()).ok();
        let conn = Connection::open(Self::db_path())?;
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

    #[cfg(test)]
    pub fn init_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute(
            "CREATE TABLE apps (
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
        let args_json = serde_json::to_string(&app.args).unwrap_or_else(|_| "[]".into());
        let env_json = serde_json::to_string(&app.env).unwrap_or_else(|_| "{}".into());
        let cwd = app.cwd.to_string_lossy().to_string();

        self.conn.execute(
            "INSERT INTO apps (name, executable, args, cwd, env, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'STOPPED')
             ON CONFLICT(name) DO UPDATE SET
                executable=excluded.executable,
                args=excluded.args,
                cwd=excluded.cwd,
                env=excluded.env",
            params![app.name, app.executable, args_json, cwd, env_json],
        )?;
        Ok(())
    }

    pub fn list_apps(&self) -> Result<Vec<AppRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, executable, args, cwd, env, status, restarts, created_at FROM apps ORDER BY name",
        )?;
        let rows = stmt.query_map([], AppRecord::from_row)?;
        rows.collect()
    }

    pub fn get_app(&self, name: &str) -> Result<Option<AppRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, executable, args, cwd, env, status, restarts, created_at FROM apps WHERE name = ?1",
        )?;
        let mut rows = stmt.query_map(params![name], AppRecord::from_row)?;
        Ok(rows.next().transpose()?)
    }

    pub fn delete_app(&self, name: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM apps WHERE name = ?1", params![name])?;
        Ok(n > 0)
    }

    pub fn log_path_for(name: &str) -> PathBuf {
        Self::logs_dir().join(format!("{name}.log"))
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

    pub fn reset_restarts(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE apps SET restarts = 0 WHERE name = ?1",
            params![name],
        )?;
        Ok(())
    }

    pub fn data_dir_exists(path: &Path) -> bool {
        path.exists()
    }
}