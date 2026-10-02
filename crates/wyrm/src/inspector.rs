use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    pub fn get_cmd(&self) -> &'static str {
        match self {
            PackageManager::Npm => "npm.cmd",
            PackageManager::Pnpm => "pnpm.cmd",
            PackageManager::Yarn => "yarn.cmd",
            PackageManager::Bun => "bun.exe",
        }
    }

    pub fn detect(dir: &Path) -> Self {
        if dir.join("pnpm-lock.yaml").exists() {
            PackageManager::Pnpm
        } else if dir.join("yarn.lock").exists() {
            PackageManager::Yarn
        } else if dir.join("bun.lockb").exists() || dir.join("bun.lock").exists() {
            PackageManager::Bun
        } else {
            PackageManager::Npm
        }
    }
}

#[derive(Deserialize)]
struct PackageJson {
    pub name: Option<String>,
    pub scripts: Option<HashMap<String, String>>,
    pub dependencies: Option<HashMap<String, String>>,
    pub dev_dependencies: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
}

pub fn inspect_and_configure(
    project_dir: &Path,
    custom_name: Option<String>,
) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let pkg_path = project_dir.join("package.json");
    if !pkg_path.exists() {
        return Err("No se encontró package.json en el directorio especificado.".into());
    }

    let pkg_content = fs::read_to_string(&pkg_path)?;
    let pkg: PackageJson = serde_json::from_str(&pkg_content)?;

    let app_name = custom_name.unwrap_or_else(|| {
        pkg.name.filter(|n| !n.is_empty()).unwrap_or_else(|| {
            project_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("app")
                .to_string()
        })
    });

    let pm = PackageManager::detect(project_dir);
    let deps = pkg.dependencies.unwrap_or_default();
    let dev_deps = pkg.dev_dependencies.unwrap_or_default();

    let mut env_vars = HashMap::new();
    let env_prod = project_dir.join(".env.production");
    let env_default = project_dir.join(".env");

    let env_to_load = if env_prod.exists() {
        Some(env_prod)
    } else if env_default.exists() {
        Some(env_default)
    } else {
        None
    };
    if let Some(path) = env_to_load {
        if let Ok(iter) = dotenvy::from_path_iter(path) {
            for item in iter.flatten() {
                env_vars.insert(item.0, item.1);
            }
        }
    }

    let next_standalone = project_dir
        .join(".next")
        .join("standalone")
        .join("server.js");
    if next_standalone.exists() {
        return Ok(AppConfig {
            name: app_name,
            executable: "node.exe".to_string(),
            args: vec!["server.js".to_string()],
            cwd: project_dir.join(".next").join("standalone"),
            env: env_vars,
        });
    }

    let astro_entry = project_dir.join("dist").join("server").join("entry.mjs");
    if (deps.contains_key("astro") || dev_deps.contains_key("astro")) && astro_entry.exists() {
        return Ok(AppConfig {
            name: app_name,
            executable: "node.exe".to_string(),
            args: vec!["dist/server/entry.mjs".to_string()],
            cwd: project_dir.to_path_buf(),
            env: env_vars,
        });
    }

    let scripts = pkg.scripts.unwrap_or_default();
    if scripts.contains_key("start") {
        Ok(AppConfig {
            name: app_name,
            executable: pm.get_cmd().to_string(),
            args: vec!["run".to_string(), "start".to_string()],
            cwd: project_dir.to_path_buf(),
            env: env_vars,
        })
    } else if project_dir.join("dist").join("main.js").exists() {
        Ok(AppConfig {
            name: app_name,
            executable: "node.exe".to_string(),
            args: vec!["dist/main.js".to_string()],
            cwd: project_dir.to_path_buf(),
            env: env_vars,
        })
    } else {
        Err(
            "No se pudo detectar el comando de inicio. Define un script 'start' en package.json."
                .into(),
        )
    }
}
