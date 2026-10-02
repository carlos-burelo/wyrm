//! `wyrm doctor`: diagnóstico para Windows Server.
//!
//! Revisa node, demonio, servicio, DB, disco y logs. No modifica nada.

use colored::Colorize;

struct Check {
    name: String,
    ok: bool,
    detail: String,
    hint: String,
}

pub async fn cmd_doctor() -> Result<(), Box<dyn std::error::Error>> {
    let mut checks: Vec<Check> = vec![];

    // 1. Node.js
    match node_version().await {
        Some(v) => checks.push(Check {
            name: "node".into(),
            ok: true,
            detail: v,
            hint: String::new(),
        }),
        None => checks.push(Check {
            name: "node".into(),
            ok: false,
            detail: "no encontrado en PATH".into(),
            hint: "Instala Node LTS y reabre la terminal".into(),
        }),
    }

    // 2. Demonio
    match crate::ipc::send_request("LIST", serde_json::Value::Null).await {
        Ok(res) if res.is_ok() => {
            let n = res.data.map(|d| d.as_array().map(|a| a.len()).unwrap_or(0));
            checks.push(Check {
                name: "daemon".into(),
                ok: true,
                detail: format!("responde ({} apps)", n.unwrap_or(0)),
                hint: String::new(),
            });
        }
        _ => checks.push(Check {
            name: "daemon".into(),
            ok: false,
            detail: "no responde".into(),
            hint: "Arranca `wyrm daemon` (dev) o `wyrm service install`".into(),
        }),
    }

    // 3. Servicio Windows
    checks.push(service_check().await);

    // 4. DB
    match crate::store::db::Database::init().and_then(|db| db.list_apps()) {
        Ok(apps) => {
            let zombies: Vec<_> = apps
                .iter()
                .filter(|a| a.status == "RUNNING" || a.status == "ERRORED")
                .map(|a| a.name.clone())
                .collect();
            checks.push(Check {
                name: "db".into(),
                ok: true,
                detail: format!(
                    "{} apps, {} en {}",
                    apps.len(),
                    zombies.len(),
                    crate::store::paths::db_path().display()
                ),
                hint: if zombies.is_empty() {
                    String::new()
                } else {
                    format!("Revisa con `wyrm list`: {}", zombies.join(", "))
                },
            });
        }
        Err(e) => checks.push(Check {
            name: "db".into(),
            ok: false,
            detail: format!("ilegible: {e}"),
            hint: "Revisa permisos en %ProgramData%\\wyrm".into(),
        }),
    }

    // 5. Disco
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let data_dir = crate::store::paths::data_dir();
    let mut disk_line = "sin datos".to_string();
    let mut disk_ok = true;
    for d in disks.list() {
        let mount = d.mount_point().to_string_lossy().to_string();
        if data_dir.starts_with(d.mount_point()) {
            let free_gb = d.available_space() as f64 / 1_073_741_824.0;
            disk_line = format!("{mount} libres {free_gb:.1} GB");
            disk_ok = free_gb > 5.0;
        }
    }
    checks.push(Check {
        name: "disco".into(),
        ok: disk_ok,
        detail: disk_line,
        hint: if disk_ok {
            String::new()
        } else {
            "Libera espacio o rota logs".into()
        },
    });

    // 6. Logs
    let logs_dir = crate::store::paths::logs_dir();
    let mut total: u64 = 0;
    let mut files = 0;
    if let Ok(entries) = std::fs::read_dir(&logs_dir) {
        for e in entries.flatten() {
            if let Ok(m) = e.metadata() {
                total += m.len();
                files += 1;
            }
        }
    }
    let total_mb = total as f64 / 1_048_576.0;
    checks.push(Check {
        name: "logs".into(),
        ok: total_mb < 500.0,
        detail: format!(
            "{files} archivos, {total_mb:.1} MB en {}",
            logs_dir.display()
        ),
        hint: if total_mb < 500.0 {
            String::new()
        } else {
            "Vacía con TUI (F) o borra *.log.N viejos".into()
        },
    });

    // Render
    let mut fail = 0;
    for c in &checks {
        let mark = if c.ok {
            "OK ".green()
        } else {
            "FAIL".red().bold()
        };
        println!("{mark} {:<8} {}", c.name, c.detail);
        if !c.ok {
            fail += 1;
            if !c.hint.is_empty() {
                println!("          → {}", c.hint.dimmed());
            }
        } else if !c.hint.is_empty() {
            println!("          → {}", c.hint.dimmed());
        }
    }
    println!();
    if fail == 0 {
        println!("{}", "Todo sano.".green().bold());
    } else {
        println!(
            "{}",
            format!("{fail} problema(s) detectado(s).").red().bold()
        );
    }
    Ok(())
}

async fn node_version() -> Option<String> {
    for bin in ["node.exe", "node"] {
        if let Ok(out) = tokio::process::Command::new(bin)
            .arg("--version")
            .output()
            .await
        {
            if out.status.success() {
                return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
            }
        }
    }
    None
}

#[cfg(windows)]
async fn service_check() -> Check {
    let out = tokio::process::Command::new("sc.exe")
        .args(["query", "WyrmDaemon"])
        .output()
        .await;
    match out {
        Ok(o) => {
            let txt = String::from_utf8_lossy(&o.stdout).to_string();
            if txt.contains("RUNNING") {
                Check {
                    name: "servicio".into(),
                    ok: true,
                    detail: "WyrmDaemon RUNNING".into(),
                    hint: String::new(),
                }
            } else if txt.contains("STOPPED") || txt.contains("1060") {
                Check {
                    name: "servicio".into(),
                    ok: false,
                    detail: "WyrmDaemon detenido o inexistente".into(),
                    hint: "`wyrm service install` + arráncalo".into(),
                }
            } else {
                Check {
                    name: "servicio".into(),
                    ok: false,
                    detail: txt.lines().next().unwrap_or("desconocido").to_string(),
                    hint: "`wyrm service install`".into(),
                }
            }
        }
        Err(_) => Check {
            name: "servicio".into(),
            ok: false,
            detail: "sc.exe falló".into(),
            hint: "Ejecuta como Administrador".into(),
        },
    }
}

#[cfg(not(windows))]
async fn service_check() -> Check {
    Check {
        name: "servicio".into(),
        ok: false,
        detail: "solo Windows".into(),
        hint: "Usa `wyrm daemon` en dev".into(),
    }
}
