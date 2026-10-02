use rusqlite::{params, Connection, Result, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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

    pub fn to_app_config(&self) -> crate::runtime::inspector::AppConfig {
        crate::runtime::inspector::AppConfig {
            name: self.name.clone(),
            executable: self.executable.clone(),
            args: self.args.clone(),
            cwd: PathBuf::from(&self.cwd),
            env: self.env.clone(),
            policy: Default::default(),
        }
    }
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn log_path_for(name: &str) -> PathBuf {
        super::paths::log_path_for(name)
    }

    pub fn init() -> Result<Self> {
        let dir = super::paths::data_dir();
        fs::create_dir_all(&dir).ok();
        fs::create_dir_all(super::paths::logs_dir()).ok();
        let conn = Connection::open(super::paths::db_path())?;
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

    pub fn save_app(&self, app: &crate::runtime::inspector::AppConfig) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::inspector::AppConfig;
    use std::collections::HashMap;

    fn sample(name: &str) -> AppConfig {
        AppConfig::new(
            name.into(),
            "node.exe".into(),
            vec!["server.js".into()],
            PathBuf::from("C:\\tmp"),
            HashMap::new(),
        )
    }

    #[test]
    fn crud_roundtrip() {
        let db = Database::init_in_memory().unwrap();
        db.save_app(&sample("a")).unwrap();
        db.save_app(&sample("b")).unwrap();
        let all = db.list_apps().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "a");

        db.update_status("a", "RUNNING", true).unwrap();
        let a = db.get_app("a").unwrap().unwrap();
        assert_eq!(a.status, "RUNNING");
        assert_eq!(a.restarts, 1);

        assert!(db.delete_app("b").unwrap());
        assert!(db.get_app("b").unwrap().is_none());
        assert!(!db.delete_app("missing").unwrap());
    }
}
