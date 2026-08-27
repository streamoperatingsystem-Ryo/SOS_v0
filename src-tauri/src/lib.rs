mod config;
mod obs;
mod scene;
mod server;

use scene::Scene;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tokio::sync::broadcast;

/// État global partagé : scène unique (source de vérité) + canal snapshot.
struct AppState {
    scene: Arc<Mutex<Scene>>,
    snapshot_tx: broadcast::Sender<String>,
}

/// Sérialise la scène en message snapshot WS.
fn snapshot_json(scene: &Scene) -> String {
    serde_json::json!({
        "type": "snapshot",
        "scene": scene
    })
    .to_string()
}

/// Pousse le snapshot courant vers tous les clients WS connectés.
fn push_snapshot(state: &AppState) {
    let scene = state.scene.lock().unwrap();
    let json = snapshot_json(&scene);
    // send_err = aucun client connecté, c'est OK
    let _ = state.snapshot_tx.send(json);
}

#[tauri::command]
fn get_scene(state: tauri::State<AppState>) -> Scene {
    state.scene.lock().unwrap().clone()
}

#[tauri::command]
fn update_scene(app: AppHandle, state: tauri::State<AppState>, scene: Scene) -> Result<(), String> {
    // 1. Muter l'état unique
    {
        let mut current = state.scene.lock().unwrap();
        *current = scene;
    }

    // 2. Sauvegarder config.json
    let scene_snapshot = state.scene.lock().unwrap().clone();
    config::save_scene(&app, &scene_snapshot)?;

    // 3. Pousser le snapshot WS (après save, pas d'état parallèle)
    push_snapshot(&state);

    Ok(())
}

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const IMG_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

/// Vérifie les magic bytes pour PNG/JPEG/GIF/WebP.
fn check_magic(bytes: &[u8]) -> bool {
    let p = |pre: &[u8]| bytes.starts_with(pre);
    p(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) // PNG
        || p(&[0xFF, 0xD8, 0xFF]) // JPEG
        || p(&[0x47, 0x49, 0x46, 0x38]) // GIF8
        || (bytes.len() >= 12
            && p(&[0x52, 0x49, 0x46, 0x46])
            && &bytes[8..12] == &[0x57, 0x45, 0x42, 0x50]) // RIFF...WEBP
}

/// Importe une image pour un widget : dialog → validation → copie vers
/// AppData/StreamOS/medias/<uuid>.<ext> → mutate scène → save → snapshot.
/// Retourne `Some(media_rel_path)` si importé, `None` si dialog annulé.
#[tauri::command]
fn import_media(
    app: AppHandle,
    state: tauri::State<AppState>,
    widget_id: String,
) -> Result<Option<String>, String> {
    use std::fs;
    use std::io::Read;
    use tauri_plugin_dialog::DialogExt;

    // 1. Dialog fichier (filtre images)
    let file = app
        .dialog()
        .file()
        .add_filter("Images", IMG_EXT)
        .blocking_pick_file();
    let Some(file) = file else {
        return Ok(None); // dialog annulé
    };
    let src: std::path::PathBuf = file
        .into_path()
        .map_err(|e| format!("Chemin invalide: {}", e))?;

    // 2. Taille ≤ 10 Mo
    let meta = fs::metadata(&src).map_err(|e| format!("metadata: {}", e))?;
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "Fichier trop volumineux ({} octets > 10 Mo)",
            meta.len()
        ));
    }

    // 3. Magic bytes
    let mut f = fs::File::open(&src).map_err(|e| format!("open: {}", e))?;
    let mut head = [0u8; 12];
    let n = f.read(&mut head).map_err(|e| format!("read: {}", e))?;
    if !check_magic(&head[..n]) {
        return Err("Format non supporté (magic bytes invalides)".into());
    }

    // 4. Extension autorisée
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| "Extension manquante".to_string())?;
    if !IMG_EXT.contains(&ext.as_str()) {
        return Err(format!("Extension .{} non autorisée", ext));
    }

    // 5. Copie vers AppData/StreamOS/medias/<uuid>.<ext>
    let dir = config::data_dir(&app)?;
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    let dest_name = format!("{}.{}", uuid, ext);
    let dest = dir.join("medias").join(&dest_name);
    fs::copy(&src, &dest).map_err(|e| format!("copy: {}", e))?;

    let rel = format!("medias/{}", dest_name);

    // 6. Mutate scène : set media sur le widget ciblé
    {
        let mut current = state.scene.lock().unwrap();
        let w = current
            .widgets
            .iter_mut()
            .find(|w| w.id == widget_id)
            .ok_or_else(|| format!("Widget {} introuvable", widget_id))?;
        w.media = Some(rel.clone());
    }

    // 7. Save config.json + push snapshot (chaîne unique)
    let snap = state.scene.lock().unwrap().clone();
    config::save_scene(&app, &snap)?;
    push_snapshot(&state);

    Ok(Some(rel))
}

/// Connecte à OBS WebSocket (host:port, password), authentifie, et s'assure
/// que la scène "SOS" + source navigateur "SOS" (1920×1080, URL :4321)
/// existent — idempotent, sans doublon. One-shot : se déconnecte après.
#[tauri::command]
async fn obs_connect(host: String, port: u16, password: String) -> Result<(), String> {
    obs::connect_and_setup(&host, port, &password).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Charge la scène depuis config.json (ou scène vide)
            let scene = config::load_scene(&handle).unwrap_or_else(|e| {
                log::error!("Erreur load_scene: {}", e);
                Scene::new()
            });

            let scene = Arc::new(Mutex::new(scene));

            // Canal broadcast pour les snapshots WS
            let (snapshot_tx, _snapshot_rx) = broadcast::channel::<String>(64);

            let state = AppState {
                scene: scene.clone(),
                snapshot_tx: snapshot_tx.clone(),
            };
            app.manage(state);

            // Démarre le serveur :4321 en arrière-plan.
            // run_server émet server_ready (bind OK) ou server_error (port pris)
            // directement — pas de probe externe, pas de fallback port.
            let server_handle = handle.clone();
            let server_scene = scene.clone();
            tauri::async_runtime::spawn(async move {
                match server::run_server(server_handle, snapshot_tx, server_scene).await {
                    Ok(()) => {
                        log::info!("Serveur :4321 arrêté normalement");
                    }
                    Err(e) => {
                        log::error!("Erreur serveur :4321: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_scene, update_scene, import_media, obs_connect])
        .run(tauri::generate_context!())
        .expect("erreur lors du lancement de StreamOS v0");
}
