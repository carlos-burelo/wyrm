//! Deploy git + hooks: `wyrm deploy <app> [--ref]`.
//!
//! Flujo: pre hook → git sync → post hook → restart. Cada paso que falla
//! aborta y queda registrado en la tabla `deploys`.

use std::path::Path;
use std::process::Stdio;

/// Ejecuta `cmd` con `cmd.exe /C` en `cwd`, devuelve stdout recortado.
pub fn run_hook(cwd: &Path, cmd: &str) -> Result<String, String> {
    let out = std::process::Command::new("cmd.exe")
        .args(["/C", cmd])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("no se pudo lanzar hook: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        Err(if stderr.is_empty() {
            format!("hook salió con {}", out.status)
        } else {
            stderr
        })
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|_| "git no está en PATH".to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn is_repo(cwd: &Path) -> bool {
    cwd.join(".git").exists()
}

/// SHA actual o None si no hay repo / falla.
pub fn git_sha(cwd: &Path) -> Option<String> {
    git(cwd, &["rev-parse", "HEAD"]).ok()
}

/// Actualiza el repo: con `ref_` hace fetch + checkout; sin ref, pull --ff-only.
/// Devuelve el SHA resultante.
pub fn git_sync(cwd: &Path, ref_: Option<&str>) -> Result<String, String> {
    if let Some(r) = ref_ {
        git(cwd, &["fetch", "origin"])?;
        git(cwd, &["checkout", r])?;
    } else {
        git(cwd, &["pull", "--ff-only"])?;
    }
    git(cwd, &["rev-parse", "HEAD"])
}

/// Vuelve al `sha` dado (rollback).
pub fn git_reset(cwd: &Path, sha: &str) -> Result<String, String> {
    if sha.trim().is_empty() {
        return Err("SHA vacío".into());
    }
    git(cwd, &["reset", "--hard", sha])?;
    git(cwd, &["rev-parse", "HEAD"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_echo_ok_y_fallo() {
        let dir = std::env::temp_dir();
        assert_eq!(run_hook(&dir, "echo hola").unwrap(), "hola");
        assert!(run_hook(&dir, "exit 1").is_err());
    }

    #[test]
    fn no_repo_no_sha() {
        let dir = std::env::temp_dir().join(format!(
            "wyrm-norepo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_repo(&dir));
        assert!(git_sha(&dir).is_none());
    }
}
