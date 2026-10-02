//! `wyrm cert`: issue/list/renew de certificados ACME para el edge.

pub async fn cmd_cert(action: &str, rest: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        "selfsigned" => {
            let Some(host) = rest.first() else {
                eprintln!("Uso: wyrm cert selfsigned <host>  (solo dev, sin validación)");
                return Ok(());
            };
            match crate::edge::acme::self_signed(host) {
                Ok(()) => println!("OK: self-signed para {host}"),
                Err(e) => eprintln!("FAIL: {e}"),
            }
        }
        "issue" => {
            let Some(host) = rest.first() else {
                eprintln!("Uso: wyrm cert issue <host> [--staging|--prod] [--email x@y]");
                return Ok(());
            };
            let staging = !rest.iter().any(|a| a == "--prod");
            let email = rest
                .iter()
                .position(|a| a == "--email")
                .and_then(|i| rest.get(i + 1))
                .map(|s| s.as_str());
            if !staging && email.is_none() {
                eprintln!("Tip: --email te avisa de expiraciones (opcional).");
            }
            match crate::edge::acme::issue(host, staging, email).await {
                Ok(()) => println!("OK: cert para {host}"),
                Err(e) => eprintln!("FAIL cert {host}: {e}"),
            }
        }
        "renew" => {
            let force = rest.iter().any(|a| a == "--force");
            let want = if rest.iter().any(|a| a == "--prod") {
                "prod"
            } else {
                "staging"
            };
            let mut n = 0;
            for info in crate::edge::acme::list() {
                if info.kind != want {
                    continue;
                }
                if !force && info.days_left > 30 {
                    println!("{}: quedan {}d, skip", info.host, info.days_left);
                    continue;
                }
                println!("Renovando {}…", info.host);
                match crate::edge::acme::issue(&info.host, want == "staging", None).await {
                    Ok(()) => {
                        n += 1;
                        println!("OK {}", info.host);
                    }
                    Err(e) => eprintln!("FAIL {}: {e}", info.host),
                }
            }
            println!("{n} renovado(s).");
        }
        _ => {
            let rows = crate::edge::acme::list();
            if rows.is_empty() {
                println!("Sin certificados. `wyrm cert issue <host> [--staging|--prod]`.");
                return Ok(());
            }
            println!("{:<30} {:<8} {:<12} {}", "HOST", "KIND", "EMITIDO", "DIAS");
            for r in rows {
                println!(
                    "{:<30} {:<8} {:<12} {}",
                    r.host, r.kind, r.issued_at_unix, r.days_left
                );
            }
        }
    }
    Ok(())
}
