//! `wyrm token`: muestra o rota el Bearer de la API local.

pub async fn cmd_token(rotate: bool) -> Result<(), Box<dyn std::error::Error>> {
    if rotate {
        let t = crate::api::rotate_token()?;
        println!("Token rotado. Nuevo valor (guárdalo, solo se muestra aquí):");
        println!("{t}");
        println!("Úsalo como `Authorization: Bearer <token>`.");
        return Ok(());
    }
    match crate::api::load_or_create_token() {
        Ok(t) => {
            println!("Token API ({}):", crate::api::token_path().display());
            println!("{t}");
        }
        Err(e) => eprintln!("No se pudo leer el token: {e}"),
    }
    Ok(())
}
