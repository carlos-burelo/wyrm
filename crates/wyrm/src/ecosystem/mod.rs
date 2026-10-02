//! Ecosystem file (`wyrm.json`): multi-app con un solo archivo.
//!
//! ```json
//! { "apps": [{ "name": "web", "cwd": ".", "env": { "PORT": "3000" } }] }
//! ```
//! `executable`/`args` opcionales: si faltan se auto-detectan con el
//! inspector sobre `cwd`. `cwd` relativo al directorio del archivo.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const FILE_NAMES: &[&str] = &["wyrm.json", "ecosystem.json"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcosystemApp {
    pub name: String,
    #[serde(default = "default_cwd")]
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    /// Políticas de supervisión (todas opcionales, con defaults).
    #[serde(default, skip_serializing_if = "is_default_policy")]
    pub policy: crate::runtime::policy::Policy,
}

fn is_default_policy(p: &crate::runtime::policy::Policy) -> bool {
    *p == Default::default()
}

fn default_cwd() -> PathBuf {
    PathBuf::from(".")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ecosystem {
    #[serde(default)]
    pub apps: Vec<EcosystemApp>,
}

impl Ecosystem {
    pub fn find(dir: &Path) -> Option<PathBuf> {
        FILE_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file())
    }

    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let eco: Self = serde_json::from_str(&content)?;
        if eco.apps.is_empty() {
            return Err("Ecosystem sin apps (array `apps` vacío)".into());
        }
        let mut seen = std::collections::HashSet::new();
        for app in &eco.apps {
            if app.name.trim().is_empty() {
                return Err("Hay una app sin `name`".into());
            }
            if !seen.insert(app.name.clone()) {
                return Err(format!("App duplicada: {}", app.name).into());
            }
        }
        Ok(eco)
    }

    /// Genera un ecosystem inspeccionando `dir` (una app).
    pub fn from_inspect(
        dir: &Path,
        name: Option<String>,
    ) -> Result<(Self, crate::runtime::inspector::AppConfig), Box<dyn std::error::Error>> {
        let cfg = crate::runtime::inspector::inspect_and_configure(dir, name)?;
        let eco = Self {
            apps: vec![EcosystemApp {
                name: cfg.name.clone(),
                cwd: PathBuf::from("."),
                executable: None,
                args: None,
                env: cfg.env.clone(),
                policy: Default::default(),
            }],
        };
        Ok((eco, cfg))
    }

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content + "\n")?;
        Ok(())
    }
}

/// Resuelve una app del ecosystem a `AppConfig` ejecutable.
pub fn resolve(
    base_dir: &Path,
    app: &EcosystemApp,
) -> Result<crate::runtime::inspector::AppConfig, Box<dyn std::error::Error>> {
    let dir = if app.cwd.is_absolute() {
        app.cwd.clone()
    } else {
        base_dir.join(&app.cwd)
    };
    let mut cfg = crate::runtime::inspector::inspect_and_configure(&dir, Some(app.name.clone()))?;
    if let Some(exe) = &app.executable {
        cfg.executable = exe.clone();
    }
    if let Some(args) = &app.args {
        cfg.args = args.clone();
    }
    // El env del ecosystem gana sobre el auto-detectado (.env).
    cfg.env.extend(app.env.clone());
    // La policy del ecosystem sustituye a la default del inspector.
    cfg.policy = app.policy.clone();
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_validate() {
        let eco = Ecosystem {
            apps: vec![EcosystemApp {
                name: "web".into(),
                cwd: ".".into(),
                executable: None,
                args: None,
                env: HashMap::from([("PORT".to_string(), "3000".to_string())]),
                policy: Default::default(),
            }],
        };
        let s = serde_json::to_string(&eco).unwrap();
        let back: Ecosystem = serde_json::from_str(&s).unwrap();
        assert_eq!(back.apps[0].env["PORT"], "3000");

        let dir = std::env::temp_dir().join(format!(
            "wyrm-eco-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wyrm.json");
        eco.save(&path).unwrap();
        let loaded = Ecosystem::load(&path).unwrap();
        assert_eq!(loaded.apps.len(), 1);
        assert!(Ecosystem::find(&dir).is_some());
    }
}
