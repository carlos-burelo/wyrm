//! `wyrm deploy <app> [--ref]`: hooks + git sync + restart.
//!
//! Lee `pre_deploy`/`post_deploy` del `wyrm.json` que haya junto al `cwd`
//! de la app. Todo queda en la tabla `deploys` (`releases`, `rollback`).

use std::path::PathBuf;

fn app_cwd(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let rec = crate::store::db::Database::init()?
        .get_app(name)?
        .ok_or(format!("App desconocida: {name} (`wyrm list` para ver)"))?;
    Ok(PathBuf::from(rec.cwd))
}

fn hooks_for(cwd: &std::path::Path, name: &str) -> (Option<String>, Option<String>) {
    let path = match crate::ecosystem::Ecosystem::find(cwd) {
        Some(p) => p,
        None => return (None, None),
    };
    match crate::ecosystem::Ecosystem::load(&path) {
        Ok(eco) => eco
            .apps
            .iter()
            .find(|a| a.name == name)
            .map(|a| (a.pre_deploy.clone(), a.post_deploy.clone()))
            .unwrap_or((None, None)),
        Err(_) => (None, None),
    }
}

fn fail(
    db: &crate::store::db::Database,
    name: &str,
    before: &str,
    step: &str,
    msg: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("FAIL {name} [{step}]: {msg}");
    let _ = db.record_deploy(name, before, before, &format!("failed@{step}"));
    Err(format!("deploy abortado en {step}").into())
}

pub async fn cmd_deploy(
    name: &str,
    ref_: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = crate::store::db::Database::init()?;
    let cwd = app_cwd(name)?;
    let (pre, post) = hooks_for(&cwd, name);
    let before = crate::deploy::git_sha(&cwd).unwrap_or_else(|| "(no-git)".into());
    println!("Deploy {name} {} (antes {before})", cwd.display());

    if let Some(cmd) = pre {
        println!("pre: {cmd}");
        if let Err(e) = crate::deploy::run_hook(&cwd, &cmd) {
            return fail(&db, name, &before, "pre", &e);
        }
    }

    let after = if crate::deploy::is_repo(&cwd) {
        match crate::deploy::git_sync(&cwd, ref_.as_deref()) {
            Ok(sha) => {
                println!("git → {sha}");
                sha
            }
            Err(e) => return fail(&db, name, &before, "sync", &e),
        }
    } else {
        println!("(sin .git: solo hooks + restart)");
        before.clone()
    };

    if let Some(cmd) = post {
        println!("post: {cmd}");
        if let Err(e) = crate::deploy::run_hook(&cwd, &cmd) {
            return fail(&db, name, &before, "post", &e);
        }
    }

    match crate::ipc::send_request("RESTART", serde_json::json!({ "name": name })).await {
        Ok(res) if res.is_ok() => println!("restart OK"),
        Ok(res) => eprintln!("Aviso restart: {}", res.message),
        Err(e) => eprintln!("Aviso: demonio no disponible ({e}); el código ya está actualizado."),
    }

    let _ = db.record_deploy(name, &before, &after, "ok");
    println!("Deploy ok: {before} → {after}");
    Ok(())
}
