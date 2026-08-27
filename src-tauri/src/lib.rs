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

const MAX_IMG_BYTES: u64 = 10 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 80 * 1024 * 1024;
const IMG_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const VIDEO_EXT: &[&str] = &["mp4", "webm"];

/// Vérifie les magic bytes pour PNG/JPEG/GIF/WebP.
fn check_magic_image(bytes: &[u8]) -> bool {
    let p = |pre: &[u8]| bytes.starts_with(pre);
    p(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) // PNG
        || p(&[0xFF, 0xD8, 0xFF]) // JPEG
        || p(&[0x47, 0x49, 0x46, 0x38]) // GIF8
        || (bytes.len() >= 12
            && p(&[0x52, 0x49, 0x46, 0x46])
            && &bytes[8..12] == &[0x57, 0x45, 0x42, 0x50]) // RIFF...WEBP
}

/// Vérifie les magic bytes pour MP4 (boîte `ftyp` à l'offset 4) et WebM (EBML).
fn check_magic_video(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp" // MP4 / ISOBMFF
        || bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) // WebM / EBML
}

/// Helper commun d'import média (widget OU fond de scène) : dialog fichier
/// « Médias » → validation (extension + taille + magic bytes) → copie vers
/// AppData/StreamOS/medias/<uuid>.<ext>. Retourne `Some((rel, kind))` si
/// importé, `None` si dialog annulé. Image : png/jpg/jpeg/gif/webp ≤ 10 Mo.
/// Vidéo : mp4/webm ≤ 80 Mo. Aucune mutation de la scène — l'appelant mutera
/// le widget ciblé ou les champs fond après coup.
fn pick_and_copy_media(app: &AppHandle) -> Result<Option<(String, String)>, String> {
    use std::fs;
    use std::io::Read;
    use tauri_plugin_dialog::DialogExt;

    // 1. Dialog fichier (un seul filtre « Médias » : images + vidéos)
    let mut all_ext: Vec<&str> = IMG_EXT.to_vec();
    all_ext.extend_from_slice(VIDEO_EXT);
    let file = app
        .dialog()
        .file()
        .add_filter("Médias", &all_ext)
        .blocking_pick_file();
    let Some(file) = file else {
        return Ok(None); // dialog annulé
    };
    let src: std::path::PathBuf = file
        .into_path()
        .map_err(|e| format!("Chemin invalide: {}", e))?;

    // 2. Extension → kind + liste autorisée
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| "Extension manquante".to_string())?;

    let (kind, allowed_ext, max_bytes, magic_ok): (&str, &[&str], u64, fn(&[u8]) -> bool) =
        if IMG_EXT.contains(&ext.as_str()) {
            ("image", IMG_EXT, MAX_IMG_BYTES, check_magic_image)
        } else if VIDEO_EXT.contains(&ext.as_str()) {
            ("video", VIDEO_EXT, MAX_VIDEO_BYTES, check_magic_video)
        } else {
            return Err(format!("Extension .{} non autorisée", ext));
        };

    // 3. Taille ≤ limite du kind
    let meta = fs::metadata(&src).map_err(|e| format!("metadata: {}", e))?;
    if meta.len() > max_bytes {
        return Err(format!(
            "Fichier trop volumineux ({} octets > {} Mo)",
            meta.len(),
            max_bytes / 1024 / 1024
        ));
    }

    // 4. Magic bytes (selon le kind)
    let mut f = fs::File::open(&src).map_err(|e| format!("open: {}", e))?;
    let mut head = [0u8; 32];
    let n = f.read(&mut head).map_err(|e| format!("read: {}", e))?;
    if !magic_ok(&head[..n]) {
        return Err("Format non supporté (magic bytes invalides)".into());
    }

    // 5. (re-vérif extension déjà faite au §2 — allowed_ext cohérent avec kind)
    let _ = allowed_ext;

    // 6. Copie vers AppData/StreamOS/medias/<uuid>.<ext>
    let dir = config::data_dir(app)?;
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    let dest_name = format!("{}.{}", uuid, ext);
    let dest = dir.join("medias").join(&dest_name);
    fs::copy(&src, &dest).map_err(|e| format!("copy: {}", e))?;

    let rel = format!("medias/{}", dest_name);
    Ok(Some((rel, kind.to_string())))
}

/// Importe un média (image OU vidéo) pour un widget : dialog → validation
/// (extension + taille + magic bytes) → copie vers
/// AppData/StreamOS/medias/<uuid>.<ext> → mutate scène (media + kind) →
/// save → snapshot. Retourne `Some(media_rel_path)` si importé, `None` si
/// dialog annulé.
/// Image : png/jpg/jpeg/gif/webp ≤ 10 Mo. Vidéo : mp4/webm ≤ 80 Mo.
#[tauri::command]
fn import_media(
    app: AppHandle,
    state: tauri::State<AppState>,
    widget_id: String,
) -> Result<Option<String>, String> {
    // 1. Helper commun : dialog + validation + copie (pas de mutation scène).
    let Some((rel, kind)) = pick_and_copy_media(&app)? else {
        return Ok(None); // dialog annulé
    };

    // 2. Mutate scène : set media + kind sur le widget ciblé
    {
        let mut current = state.scene.lock().unwrap();
        let w = current
            .widgets
            .iter_mut()
            .find(|w| w.id == widget_id)
            .ok_or_else(|| format!("Widget {} introuvable", widget_id))?;
        w.media = Some(rel.clone());
        w.kind = kind;
    }

    // 3. Save config.json + push snapshot (chaîne unique)
    let snap = state.scene.lock().unwrap().clone();
    config::save_scene(&app, &snap)?;
    push_snapshot(&state);

    Ok(Some(rel))
}

/// Importe un média (image OU vidéo) comme fond de scène : dialog → validation
/// → copie vers medias/ → mutate scène (bgMedia + bgKind) → save → snapshot.
/// Retourne `Some(media_rel_path)` si importé, `None` si dialog annulé.
/// Mêmes limites que import_media (helper commun pick_and_copy_media).
#[tauri::command]
fn import_fond(
    app: AppHandle,
    state: tauri::State<AppState>,
) -> Result<Option<String>, String> {
    // 1. Helper commun : dialog + validation + copie (pas de mutation scène).
    let Some((rel, kind)) = pick_and_copy_media(&app)? else {
        return Ok(None); // dialog annulé
    };

    // 2. Mutate scène : set bgMedia + bgKind
    {
        let mut current = state.scene.lock().unwrap();
        current.bgMedia = rel.clone();
        current.bgKind = kind;
    }

    // 3. Save config.json + push snapshot (chaîne unique)
    let snap = state.scene.lock().unwrap().clone();
    config::save_scene(&app, &snap)?;
    push_snapshot(&state);

    Ok(Some(rel))
}

/// Connecte à OBS WebSocket (host:port, password), authentifie, lit la
/// résolution canvas OBS (GetVideoSettings → baseWidth/baseHeight, fallback
/// 1920×1080), s'assure que la scène "SOS" + source navigateur "SOS-Diffusion"
/// (dims = résolution OBS, URL :4321) existent sans doublon — one-shot.
/// Puis mute la scène (canvasW/canvasH) + save config.json + snapshot WS.
/// Pas de poll, pas de rescale des widgets existants.
#[tauri::command]
async fn obs_connect(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    host: String,
    port: u16,
    password: String,
) -> Result<(), String> {
    let (w, h) = obs::connect_and_setup(&host, port, &password).await?;

    // Muter canvasW/canvasH si changement → save + snapshot (chaîne unique).
    let changed = {
        let mut current = state.scene.lock().unwrap();
        if current.canvasW == w && current.canvasH == h {
            false
        } else {
            current.canvasW = w;
            current.canvasH = h;
            true
        }
    };

    if changed {
        let snap = state.scene.lock().unwrap().clone();
        config::save_scene(&app, &snap)?;
        push_snapshot(&state);
        log::info!("Canvas mis à jour : {}×{}", w, h);
    }

    Ok(())
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
        .invoke_handler(tauri::generate_handler![get_scene, update_scene, import_media, import_fond, obs_connect])
        .run(tauri::generate_context!())
        .expect("erreur lors du lancement de StreamOS v0");
}
