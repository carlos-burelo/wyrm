//! `wyrm init`: genera `wyrm.json` inspeccionando el proyecto actual.

pub async fn cmd_init(name: Option<String>, force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::current_dir()?;
    let path = dir.join("wyrm.json");
    if path.exists() && !force {
        eprintln!("Ya existe wyrm.json (usa --force para sobrescribir).");
        return Ok(());
    }
    let (eco, cfg) = crate::ecosystem::Ecosystem::from_inspect(&dir, name)?;
    eco.save(&path)?;
    println!(
        "wyrm.json creado para '{}' ({} {:?} en {})",
        cfg.name,
        cfg.executable,
        cfg.args,
        dir.display()
    );
    println!("Edítalo para multi-app y arranca todo con `wyrm start --all`.");
    Ok(())
}
