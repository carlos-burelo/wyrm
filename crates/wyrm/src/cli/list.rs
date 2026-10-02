//! `wyrm list`: tabla coloreada o JSON, con fallback a DB.

use std::io::Write as _;

pub async fn cmd_list(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let rows: Vec<crate::daemon::AppStatus> =
        match crate::ipc::send_request("LIST", serde_json::Value::Null).await {
            Ok(res) if res.is_ok() => {
                serde_json::from_value(res.data.unwrap_or_default()).unwrap_or_default()
            }
            _ => {
                // Fallback DB.
                let db_rows = tokio::task::spawn_blocking(|| {
                    crate::store::db::Database::init()
                        .and_then(|db| db.list_apps())
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                db_rows
                    .into_iter()
                    .map(|r| crate::daemon::AppStatus {
                        name: r.name,
                        status: format!("{} (daemon off)", r.status),
                        pid: None,
                        restarts: r.restarts as u32,
                        uptime_secs: 0,
                        executable: r.executable,
                        cwd: r.cwd,
                    })
                    .collect()
            }
        };

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if rows.is_empty() {
        println!("Sin aplicaciones. Usa `wyrm start` en un proyecto Node.");
        return Ok(());
    }

    print_table(&rows);
    Ok(())
}

fn print_table(rows: &[crate::daemon::AppStatus]) {
    use colored::Colorize;
    let mut tw = tabwriter::TabWriter::new(std::io::stdout());
    let _ = writeln!(tw, "NAME\tSTATUS\tPID\tRESTARTS\tUPTIME\tCWD");
    for r in rows {
        let status = match r.status.as_str() {
            s if s.starts_with("RUNNING") => s.green().to_string(),
            s if s.starts_with("STOPPED") => s.dimmed().to_string(),
            s if s.starts_with("CRASHED") => s.red().bold().to_string(),
            s => s.yellow().to_string(),
        };
        let pid = r.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into());
        let uptime = format_uptime(r.uptime_secs);
        let _ = writeln!(
            tw,
            "{}\t{}\t{}\t{}\t{}\t{}",
            r.name, status, pid, r.restarts, uptime, r.cwd
        );
    }
    let _ = tw.flush();
}

pub fn format_uptime(secs: u64) -> String {
    if secs == 0 {
        return "-".into();
    }
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}
