//! Acciones contra el demonio / DB.

pub(crate) async fn do_action(action: &str, name: &str) {
    let _ = crate::ipc::send_request(action, serde_json::json!({ "name": name })).await;
}

pub(crate) async fn do_start(name: &str) {
    // Revive STOPPED desde DB: carga AppConfig y envía START al demonio.
    // Ventaja sobre pm2: no necesitas cwd ni ecosystem a mano.
    let owned = name.to_string();
    let cfg = tokio::task::spawn_blocking(move || {
        crate::store::db::Database::init()
            .and_then(|db| db.get_app(&owned))
            .unwrap_or(None)
            .map(|r| r.to_app_config())
    })
    .await
    .unwrap_or(None);
    if let Some(cfg) = cfg {
        if let Ok(payload) = serde_json::to_value(&cfg) {
            let _ = crate::ipc::send_request("START", payload).await;
        }
    }
}

pub(crate) async fn do_delete(name: &str) {
    let _ = crate::ipc::send_request("DELETE", serde_json::json!({ "name": name })).await;
}
