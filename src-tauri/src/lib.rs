mod api_deck;
mod alertes;
mod commandes;
mod config;
mod pad_numerique;
mod input_viewer;
mod kick;
mod obs;
mod obs_trou;
mod pc_enum;
mod scene;
mod scenes;
mod server;
mod twitch_auth;
mod twitch_chat;
mod viewers;
mod twitch_helix;
mod youtube_auth;
mod youtube_chat;
mod youtube_data;
mod tiktok_chat;
mod twitch_clips;
mod welcome;
mod bandeau;
mod position_overlay;
mod speedrun;
// Les commandes speedrun sont définies dans speedrun::commands (réelles sur
// Windows, stubs sur les autres plateformes). Importées dans le scope pour
// generate_handler!.
use speedrun::commands::{
    speedrun_action_manuelle, speedrun_arreter, speedrun_charger_asl,
    speedrun_charger_lss, speedrun_demarrer, speedrun_est_actif,
    speedrun_lire_config, speedrun_maj_settings, speedrun_maj_settings_asl,
    speedrun_sauver_config,
};

use scenes::{ScenesState, SceneIndex};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::broadcast;

// ===== Commandes scène (lecture/écriture) =====

#[tauri::command]
fn get_scene(state: tauri::State<ScenesState>) -> scene::Scene {
    state.scene.lock().unwrap().clone()
}

#[tauri::command]
fn update_scene(app: AppHandle, state: tauri::State<ScenesState>, scene: scene::Scene) -> Result<(), String> {
    // 1. Muter l'état unique
    {
        let mut current = state.scene.lock().unwrap();
        *current = scene;
    }
    // 2. Sauver <current_id>.json + snapshot WS (chaîne unique)
    scenes::save_current(&app, &state)?;
    Ok(())
}

// ===== Commandes scènes (v0.14) =====

#[tauri::command]
fn scenes_lister(app: AppHandle) -> Result<Vec<SceneIndex>, String> {
    scenes::lister(&app)
}

#[tauri::command]
fn scene_creer(app: AppHandle, state: tauri::State<ScenesState>, nom: String) -> Result<SceneIndex, String> {
    scenes::creer(&app, &state, &nom)
}

#[tauri::command]
fn scene_ouvrir(app: AppHandle, state: tauri::State<ScenesState>, id: String) -> Result<SceneIndex, String> {
    scenes::ouvrir(&app, &state, &id)
}

#[tauri::command]
fn scene_renommer(app: AppHandle, id: String, nom: String) -> Result<(), String> {
    scenes::renommer(&app, &id, &nom)
}

/// Déplace une scène dans l'index (glisser-déposer barre « Vos scènes »).
#[tauri::command]
fn scene_deplacer(app: AppHandle, id: String, position: usize) -> Result<(), String> {
    scenes::deplacer(&app, &id, position)
}

/// Masque/affiche le titre d'une scène dans sa pastille (œil, onglet orange).
#[tauri::command]
fn scene_masquer_nom(app: AppHandle, id: String, masque: bool) -> Result<(), String> {
    scenes::masquer_nom(&app, &id, masque)
}

#[tauri::command]
fn scene_supprimer(app: AppHandle, state: tauri::State<ScenesState>, id: String) -> Result<(), String> {
    scenes::supprimer(&app, &state, &id)
}

#[tauri::command]
fn scene_courante(app: AppHandle, state: tauri::State<ScenesState>) -> Result<SceneIndex, String> {
    scenes::courante(&app, &state)
}

#[tauri::command]
fn scene_exporter(app: AppHandle, state: tauri::State<ScenesState>, nom_pack: String) -> Result<bool, String> {
    scenes::exporter(&app, &state, &nom_pack)
}

#[tauri::command]
fn scene_importer(app: AppHandle, state: tauri::State<ScenesState>) -> Result<Option<SceneIndex>, String> {
    scenes::importer(&app, &state)
}

// ===== Import média (widget + fond) =====

const MAX_IMG_BYTES: u64 = 10 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 80 * 1024 * 1024;
const MAX_AUDIO_BYTES: u64 = 10 * 1024 * 1024;
const IMG_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const VIDEO_EXT: &[&str] = &["mp4", "webm", "mkv"];
const AUDIO_EXT: &[&str] = &["mp3", "ogg", "wav"];

/// Vérifie les magic bytes pour PNG/JPEG/GIF/WebP.
fn check_magic_image(bytes: &[u8]) -> bool {
    let p = |pre: &[u8]| bytes.starts_with(pre);
    p(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) // PNG
        || p(&[0xFF, 0xD8, 0xFF]) // JPEG
        || p(&[0x47, 0x49, 0x46, 0x38]) // GIF8
        || (bytes.len() >= 12
            && p(&[0x52, 0x49, 0x46, 0x46])
            && bytes[8..12] == [0x57, 0x45, 0x42, 0x50]) // RIFF...WEBP
}

/// Vérifie les magic bytes pour MP4 (boîte `ftyp` à l'offset 4) et WebM/MKV
/// (EBML). MKV et WebM partagent le format conteneur Matroska (magic EBML
/// `1A 45 DF A3`) — la distinction se fait par extension, pas par magic bytes.
/// NB : Chromium ne lit pas le MKV dans un <video> → remux MKV→MP4 à l'import
/// (validate_and_copy_media) via le sidecar ffmpeg.
fn check_magic_video(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp" // MP4 / ISOBMFF
        || bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) // WebM / MKV / EBML
}

/// Vérifie les magic bytes pour MP3 (tag ID3 ou frame sync 0xFFEx), OGG
/// (OggS) et WAV (RIFF…WAVE).
fn check_magic_audio(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") // MP3 + tag ID3
        || (bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0) // frame MP3
        || bytes.starts_with(b"OggS") // OGG
        || (bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE") // WAV
}

/// (kind, extensions autorisées, taille max, validateur magic bytes) d'un média.
type MediaSpec = (&'static str, &'static [&'static str], u64, fn(&[u8]) -> bool);

/// Validation + copie d'un média vers AppData/StreamOS/medias/<uuid>.<ext>
/// (extension + taille + magic bytes). Retourne `(rel, kind)`. Utilisé par
/// pick_and_copy_media et import_alerte_media — aucune mutation de scène ici.
///
/// MKV : Chromium ne lit pas le MKV dans un <video> → remux en MP4 via le
/// sidecar ffmpeg (`-c copy -sn -movflags +faststart`, sans réencodage). Le
/// fichier stocké est `medias/<uuid>.mp4` (kind "video"). Échoue si un flux
/// n'est pas compatible MP4 (ex. HEVC vidéo, audio Opus hors profil) —
/// l'erreur ffmpeg est retournée telle quelle.
fn validate_and_copy_media(app: &AppHandle, src: &std::path::Path) -> Result<(String, String, String), String> {
    use std::fs;
    use std::io::Read;

    // 0. Nom original du fichier (pour affichage dans la carte d'édition).
    let nom_original = src
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("média")
        .to_string();

    // 1. Extension → kind + liste autorisée
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| "Extension manquante".to_string())?;

    let (kind, allowed_ext, max_bytes, magic_ok): MediaSpec =
        if IMG_EXT.contains(&ext.as_str()) {
            ("image", IMG_EXT, MAX_IMG_BYTES, check_magic_image)
        } else if VIDEO_EXT.contains(&ext.as_str()) {
            ("video", VIDEO_EXT, MAX_VIDEO_BYTES, check_magic_video)
        } else {
            return Err(format!("Extension .{} non autorisée", ext));
        };

    // 2. Taille ≤ limite du kind
    let meta = fs::metadata(src).map_err(|e| format!("metadata: {}", e))?;
    if meta.len() > max_bytes {
        return Err(format!(
            "Fichier trop volumineux ({} octets > {} Mo)",
            meta.len(),
            max_bytes / 1024 / 1024
        ));
    }

    // 3. Magic bytes (selon le kind)
    let mut f = fs::File::open(src).map_err(|e| format!("open: {}", e))?;
    let mut head = [0u8; 32];
    let n = f.read(&mut head).map_err(|e| format!("read: {}", e))?;
    if !magic_ok(&head[..n]) {
        return Err("Format non supporté (magic bytes invalides)".into());
    }

    // 4. (re-vérif extension déjà faite au §1 — allowed_ext cohérent avec kind)
    let _ = allowed_ext;

    // 5. Copie (ou remux MKV→MP4) vers AppData/StreamOS/medias/<uuid>.<dest_ext>
    let dir = config::data_dir(app)?;
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    // MKV → remux en MP4 (lisible navigateur) ; autres extensions → copie brute.
    let dest_ext: &str = if ext == "mkv" { "mp4" } else { ext.as_str() };
    let dest_name = format!("{}.{}", uuid, dest_ext);
    let dest = dir.join("medias").join(&dest_name);
    if ext == "mkv" {
        remux_to_mp4(app, src, &dest)?;
    } else {
        fs::copy(src, &dest).map_err(|e| format!("copy: {}", e))?;
    }

    let rel = format!("medias/{}", dest_name);
    Ok((rel, kind.to_string(), nom_original))
}

/// Remuxe un MKV en MP4 via le sidecar ffmpeg (`-c copy -sn -movflags
/// +faststart`). Sans réencodage — rapide, préserve la qualité. Échoue si un
/// flux n'est pas compatible MP4 (ex. HEVC, Opus hors profil). Blocking sync :
/// tourne sur un thread worker Tauri (pas le thread UI) → pas de gel.
fn remux_to_mp4(app: &AppHandle, src: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;

    let sidecar = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| format!("ffmpeg sidecar indisponible: {}", e))?;
    let out = std::process::Command::from(sidecar)
        .arg("-y")
        .arg("-i")
        .arg(src)
        .arg("-c")
        .arg("copy")
        .arg("-sn")
        .arg("-movflags")
        .arg("+faststart")
        .arg(dest)
        .output()
        .map_err(|e| format!("ffmpeg spawn: {}", e))?;
    if !out.status.success() {
        // Nettoyer le fichier partiel éventuel.
        let _ = std::fs::remove_file(dest);
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Remux MKV→MP4 échoué: {}", stderr.trim()));
    }
    Ok(())
}

/// Helper commun d'import média (widget OU fond de scène) : dialog fichier
/// « Médias » → validation (extension + taille + magic bytes) → copie (ou
/// remux MKV→MP4) vers AppData/StreamOS/medias/<uuid>.<ext>. Retourne
/// `Some((rel, kind))` si importé, `None` si dialog annulé.
/// Image : png/jpg/jpeg/gif/webp ≤ 10 Mo. Vidéo : mp4/webm/mkv ≤ 80 Mo
/// (mkv remuxé en mp4 via sidecar ffmpeg). Aucune mutation de la scène —
/// l'appelant mutera le widget ciblé ou les champs fond après coup.
fn pick_and_copy_media(app: &AppHandle) -> Result<Option<(String, String, String)>, String> {
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

    // 2-5. Validation + copie (helper commun).
    validate_and_copy_media(app, &src).map(Some)
}

/// Importe un média (image OU vidéo) pour un widget : dialog → validation
/// (extension + taille + magic bytes) → copie (ou remux MKV→MP4) vers
/// AppData/StreamOS/medias/<uuid>.<ext> → mutate scène (media + kind) →
/// save → snapshot. Retourne `Some(media_rel_path)` si importé, `None` si
/// dialog annulé.
/// Image : png/jpg/jpeg/gif/webp ≤ 10 Mo. Vidéo : mp4/webm/mkv ≤ 80 Mo
/// (mkv remuxé en mp4 via sidecar ffmpeg).
#[tauri::command]
fn import_media(
    app: AppHandle,
    state: tauri::State<ScenesState>,
    widget_id: String,
) -> Result<Option<(String, String)>, String> {
    // 1. Helper commun : dialog + validation + copie (pas de mutation scène).
    let Some((rel, kind, nom)) = pick_and_copy_media(&app)? else {
        return Ok(None); // dialog annulé
    };

    // 2. Mutate scène : set media + kind + mediaNom sur le widget ciblé
    {
        let mut current = state.scene.lock().unwrap();
        let w = current
            .widgets
            .iter_mut()
            .find(|w| w.id == widget_id)
            .ok_or_else(|| format!("Widget {} introuvable", widget_id))?;
        w.media = Some(rel.clone());
        w.kind = kind;
        w.mediaNom = Some(nom.clone());
    }

    // 3. Save <current_id>.json + push snapshot (chaîne unique)
    scenes::save_current(&app, &state)?;

    Ok(Some((rel, nom)))
}

/// Importe un média (image OU vidéo) comme fond de scène : dialog → validation
/// → copie vers medias/ → mutate scène (bgMedia + bgKind) → save → snapshot.
/// Retourne `Some(media_rel_path)` si importé, `None` si dialog annulé.
/// Mêmes limites que import_media (helper commun pick_and_copy_media).
#[tauri::command]
fn import_fond(
    app: AppHandle,
    state: tauri::State<ScenesState>,
) -> Result<Option<String>, String> {
    // 1. Helper commun : dialog + validation + copie (pas de mutation scène).
    let Some((rel, kind, _nom)) = pick_and_copy_media(&app)? else {
        return Ok(None); // dialog annulé
    };

    // 2. Mutate scène : set bgMedia + bgKind
    {
        let mut current = state.scene.lock().unwrap();
        current.bgMedia = rel.clone();
        current.bgKind = kind;
    }

    // 3. Save <current_id>.json + push snapshot (chaîne unique)
    scenes::save_current(&app, &state)?;

    Ok(Some(rel))
}

/// Importe un média (image OU vidéo) pour une alerte : dialog filtré selon le
/// kind attendu ("image" → Images, "video" → Vidéos) → validation (extension +
/// taille + magic bytes) → copie (ou remux MKV→MP4) vers
/// AppData/StreamOS/medias/<uuid>.<ext>. Retourne `Some(rel)` si importé,
/// `None` si dialog annulé. Le kind est vérifié contre `kind_attendu`
/// (l'appelant connaît déjà le kind demandé — pas besoin de le retourner).
/// Le média est servi par :4321 sur /medias/. Vidéo mkv acceptée (remuxée en
/// mp4 via sidecar ffmpeg).
#[tauri::command]
fn import_alerte_media(
    app: AppHandle,
    kind_attendu: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    // 1. Dialog fichier filtré selon le kind attendu.
    let (filtre_nom, exts): (&str, &[&str]) = if kind_attendu == "video" {
        ("Vidéos", VIDEO_EXT)
    } else {
        ("Images", IMG_EXT)
    };
    let file = app
        .dialog()
        .file()
        .add_filter(filtre_nom, exts)
        .blocking_pick_file();
    let Some(file) = file else {
        return Ok(None); // dialog annulé
    };
    let src: std::path::PathBuf = file
        .into_path()
        .map_err(|e| format!("Chemin invalide: {}", e))?;

    // 2. Validation + copie (helper commun) + garde-fou kind.
    let (rel, kind, _nom) = validate_and_copy_media(&app, &src)?;
    if kind != kind_attendu {
        return Err(format!(
            "Type de fichier inattendu : {} attendu, {} reçu",
            kind_attendu, kind
        ));
    }
    Ok(Some(rel))
}

/// Importe un son (mp3/ogg/wav ≤ 10 Mo) pour une alerte : dialog fichier
/// « Sons » → validation (extension + taille + magic bytes) → copie vers
/// AppData/StreamOS/medias/<uuid>.<ext>. Retourne `Some(rel)` si importé,
/// `None` si dialog annulé. Le son est servi par :4321 sur /medias/ (le même
/// dossier que les médias de scène).
#[tauri::command]
fn import_son(app: AppHandle) -> Result<Option<String>, String> {
    use std::fs;
    use std::io::Read;
    use tauri_plugin_dialog::DialogExt;

    // 1. Dialog fichier (filtre « Sons »)
    let file = app
        .dialog()
        .file()
        .add_filter("Sons", AUDIO_EXT)
        .blocking_pick_file();
    let Some(file) = file else {
        return Ok(None); // dialog annulé
    };
    let src: std::path::PathBuf = file
        .into_path()
        .map_err(|e| format!("Chemin invalide: {}", e))?;

    // 2. Extension autorisée
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| "Extension manquante".to_string())?;
    if !AUDIO_EXT.contains(&ext.as_str()) {
        return Err(format!("Extension .{} non autorisée", ext));
    }

    // 3. Taille ≤ limite
    let meta = fs::metadata(&src).map_err(|e| format!("metadata: {}", e))?;
    if meta.len() > MAX_AUDIO_BYTES {
        return Err(format!(
            "Fichier trop volumineux ({} octets > {} Mo)",
            meta.len(),
            MAX_AUDIO_BYTES / 1024 / 1024
        ));
    }

    // 4. Magic bytes
    let mut f = fs::File::open(&src).map_err(|e| format!("open: {}", e))?;
    let mut head = [0u8; 32];
    let n = f.read(&mut head).map_err(|e| format!("read: {}", e))?;
    if !check_magic_audio(&head[..n]) {
        return Err("Format audio non supporté (magic bytes invalides)".into());
    }

    // 5. Copie vers AppData/StreamOS/medias/<uuid>.<ext>
    let dir = config::data_dir(&app)?;
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    let dest_name = format!("{}.{}", uuid, ext);
    let dest = dir.join("medias").join(&dest_name);
    fs::copy(&src, &dest).map_err(|e| format!("copy: {}", e))?;

    Ok(Some(format!("medias/{}", dest_name)))
}

/// Connecte à OBS WebSocket (host:port, password), authentifie, lit la
/// résolution canvas OBS (GetVideoSettings → baseWidth/baseHeight, fallback
/// 1920×1080), s'assure que la scène "SOS" + source navigateur "SOS-Diffusion"
/// (dims = résolution OBS, URL :4321) existent sans doublon — one-shot.
/// Puis mute la scène (canvasW/canvasH) + save <current_id>.json + snapshot WS.
/// Pas de poll, pas de rescale des widgets existants.
#[tauri::command]
async fn obs_connect(
    app: AppHandle,
    state: tauri::State<'_, ScenesState>,
    host: String,
    port: u16,
    password: String,
) -> Result<(), String> {
    eprintln!("[OBS] obs_connect host={} port={} …", host, port);
    let (w, h) = match obs::connect_and_setup(&host, port, &password).await {
        Ok(dims) => {
            eprintln!("[OBS] obs_connect OK {}×{}", dims.0, dims.1);
            dims
        }
        Err(e) => {
            // Log court : l'erreur Windows complète (os error 10061) se répète
            // toutes les 20s tant OBS est fermé. On la raccourcit dans le log,
            // mais on retourne la chaîne complète au frontend (messageClair
            // vérifie "10061"/"refus" pour afficher le bon message).
            let court = if e.contains("os error 10061") {
                "injoignable (OBS fermé ou WebSocket désactivé)".to_string()
            } else {
                e.clone()
            };
            eprintln!("[OBS] obs_connect ERR {}", court);
            return Err(e);
        }
    };

    // Mémoriser la résolution OBS globalement (obs_canvas.json) : appliquée à
    // TOUTES les scènes au chargement (boot, création, bascule, import) — pas
    // seulement à la scène courante. Non-fatal si écriture échoue.
    if let Err(e) = config::sauver_obs_canvas(&app, w, h) {
        log::warn!("sauver_obs_canvas: {}", e);
    }

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
        scenes::save_current(&app, &state)?;
        log::info!("Canvas mis à jour : {}×{}", w, h);
    }

    Ok(())
}

/// Refresh standalone de la source navigateur "SOS-Diffusion" (refreshnocache).
/// Appelé par le frontend quand serveur :4321 ready + OBS connecté.
/// One-shot : connect → PressInputPropertiesButton → close. Non-fatal.
#[tauri::command]
async fn obs_refresh_diffusion(
    host: String,
    port: u16,
    password: String,
) -> Result<(), String> {
    obs::refresh_diffusion(&host, port, &password).await
}

// ===== Pop-out chat (Lot B) =====

/// HTML embarqué du pop-out chat (include_str! — recompile si le fichier change).
const CHAT_POPOUT_HTML: &str = include_str!("../resources/chat-popout.html");

/// Ouvre ou ferme la fenêtre pop-out chat (toggle).
/// - Si fermée → ouvre (always_on_top, 360×520, décorations simples).
/// - Si ouverte → ferme (destroy). IRC inchangé.
/// Retourne true si maintenant ouverte, false si fermée.
/// Quand l'utilisateur ferme via la croix → emit "popout-closed" au frontend
/// pour que le bouton sidebar redevienne « Détacher ».
///
/// Navigation : custom protocol streamos-chat://localhost/chat → HTML embarqué
/// servi par register_uri_scheme_protocol (vraie origine, IPC + WS fiables).
/// PAS d'about:blank + eval (webview pas prête → fenêtre blanche).
/// PAS d'URL externe localhost:4321 (Webview2 peut bloquer la navigation).
#[tauri::command]
async fn chat_popout_toggle(app: AppHandle) -> Result<bool, String> {
    use tauri::webview::WebviewWindowBuilder;

    const LABEL: &str = "chat-popout";

    // Si la fenêtre existe déjà → fermer (réattacher).
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
        return Ok(false);
    }

    // Création : custom protocol streamos-chat://localhost/chat.
    // Le HTML est servi par register_uri_scheme_protocol (déclaré dans run()).
    // ASYNC obligatoire : sur Windows, créer une webview dans une commande sync
    // deadlock (Webview2) → fenêtre blanche, navigation/protocol jamais déclenché.
    // always_on_top, 360×520, décorations simples (croix native), resizable.
    eprintln!("[Chat] pop-out création fenêtre (streamos-chat://)");
    let url = tauri::Url::parse("streamos-chat://localhost/chat")
        .map_err(|e| format!("URL invalide: {}", e))?;
    let win = WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::CustomProtocol(url))
        .title("Chat pop-out")
        .inner_size(360.0, 520.0)
        .always_on_top(true)
        .resizable(true)
        .decorations(true)
        .build()
        .map_err(|e| format!("Création fenêtre pop-out: {}", e))?;

    // CloseRequested : la croix native déclenche cet event.
    // On NE PASSE PAS prevent_close() → la fenêtre se ferme normalement.
    // On emit "popout-closed" pour que le bouton sidebar redevienne « Détacher ».
    let app_clone = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            let _ = app_clone.emit("popout-closed", ());
            eprintln!("[Chat] pop-out fermé (croix)");
        }
    });

    Ok(true)
}

/// Ferme explicitement la fenêtre pop-out chat si elle est ouverte.
/// Non-fatal si déjà fermée. IRC inchangé.
#[tauri::command]
fn chat_popout_fermer(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "chat-popout";
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
    }
    Ok(())
}

// ===== Contrôle de l'app (topbar) =====

/// Arrête l'application (quitte proprement).
#[tauri::command]
fn app_arreter(app: AppHandle) -> Result<(), String> {
    eprintln!("[App] arrêt demandé");
    app.exit(0);
    Ok(())
}

/// Redémarre l'application (relance le processus puis quitte).
/// NB : `app.restart()` ne retourne jamais (`!`) — coercé en Result.
#[tauri::command]
fn app_redemarrer(app: AppHandle) -> Result<(), String> {
    eprintln!("[App] redémarrage demandé");
    app.restart()
}

// ===== Captures liées à la sauvegarde (Lot C) =====

/// Synchronise les captures SOS-Trou-* d'OBS avec la scène chargée.
/// Active + sync transform les items liés à un widget.obsSource de la scène.
/// Cache (SetSceneItemEnabled false) les items SOS-Trou-* sans widget.
/// Non-fatal : si OBS offline → erreur remontée (pas de crash).
#[tauri::command]
async fn scene_sync_captures(
    state: tauri::State<'_, ScenesState>,
    host: String,
    port: u16,
    password: String,
) -> Result<(), String> {
    let widgets: Vec<scene::Widget> = {
        let s = state.scene.lock().unwrap();
        s.widgets.clone()
    };
    eprintln!("[OBS] scene_sync_captures: {} widget(s)", widgets.len());
    obs_trou::sync_scene_captures(&host, port, &password, &widgets).await
}

// ===== Sources OBS "trou" (Lot 2) =====

/// Énumère les cibles de capture OBS pour un type (camera/window/game).
/// Jeu → Vec vide (capture_any_foreground_window, pas de cible).
/// One-shot : connect OBS → CreateInput temp → GetInputPropertiesList →
/// DeleteInput temp → close. Erreur remontée au dashboard si OBS offline.
#[tauri::command]
async fn obs_enumerate_targets(
    host: String,
    port: u16,
    password: String,
    kind: obs_trou::CaptureKind,
) -> Result<Vec<String>, String> {
    obs_trou::enumerate_targets(&host, port, &password, kind).await
}

/// Crée une source OBS de capture sous SOS-Diffusion, calée sur le widget
/// (x y w h). Retourne le nom de la source créée. One-shot connect/disconnect.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn obs_create_trou_source(
    host: String,
    port: u16,
    password: String,
    source_name: String,
    kind: obs_trou::CaptureKind,
    target: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    obs_trou::create_trou_source(
        &host, port, &password, &source_name, kind, target, x, y, w, h,
    )
    .await
}

/// Synchronise les transforms OBS de toutes les sources trou en une connexion.
/// Appelée au commitScene (pointerup) pour les widgets avec obsSource=Some.
#[tauri::command]
async fn obs_sync_trous(
    host: String,
    port: u16,
    password: String,
    items: Vec<obs_trou::SyncItem>,
) -> Result<(), String> {
    obs_trou::sync_trous(&host, port, &password, items).await
}

/// Supprime une source trou d'OBS (RemoveInput). Idempotent : si la source
/// n'existe pas → Ok (considérée comme déjà supprimée).
#[tauri::command]
async fn obs_delete_trou_source(
    host: String,
    port: u16,
    password: String,
    source_name: String,
) -> Result<(), String> {
    obs_trou::delete_trou_source(&host, port, &password, &source_name).await
}

/// Énumère les inputs OBS (GetInputList) filtrés par type de capture.
/// Caméra → dshow_input ; Fenêtre → window_capture ; Jeu → game_capture.
/// Exclut SOS-Diffusion, browser_source, audio, scènes, trous liés.
#[tauri::command]
async fn obs_enumerate_inputs_by_kind(
    host: String,
    port: u16,
    password: String,
    kind: obs_trou::CaptureKind,
) -> Result<Vec<obs_trou::InputEntry>, String> {
    obs_trou::enumerate_inputs_by_kind(&host, port, &password, kind).await
}

/// Lie une source OBS existante au widget trou (pas de création, juste
/// transform + reorder sous SOS-Diffusion).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn obs_link_existing_source(
    host: String,
    port: u16,
    password: String,
    source_name: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    obs_trou::link_existing_source(&host, port, &password, &source_name, x, y, w, h).await
}

// ===== Widget caméra (source OBS "SOS-Caméra" sous SOS-Diffusion) =====

/// Crée / met à jour la source "SOS-Caméra" (dshow_input) dans la scène « SOS »,
/// sous SOS-Diffusion, calée sur le widget (transform = sémantique sync_trous :
/// centre + OBS_BOUNDS_SCALE_OUTER + alignment 0).
/// device = Some(FriendlyName) → SetInputSettings video_device_id sur l'input
/// existant (changement de device explicite) ; None → device par défaut d'OBS.
/// Singleton : 1 widget caméra / scène → 1 input "SOS-Caméra".
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn camera_sync(
    host: String,
    port: u16,
    password: String,
    device: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    obs_trou::camera_sync(&host, port, &password, device.as_deref(), x, y, w, h).await
}

/// Cache l'item de scène "SOS-Caméra" (SetSceneItemEnabled false) à la
/// suppression du widget caméra. L'input OBS est CONSERVÉ (jamais RemoveInput
/// — la recréation du widget le réutilisera). Non-fatal si l'item est absent.
#[tauri::command]
async fn camera_hide(host: String, port: u16, password: String) -> Result<(), String> {
    obs_trou::camera_hide(&host, port, &password).await
}

// ===== Connexions réseau (Lot réseau) =====

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;
use twitch_auth::{Cancel, new_cancel};

/// État Twitch partagé : handle de la tâche IRC + jeton d'annulation (swappable)
/// + drapeau connecté + login/user_id/access token du compte connecté.
///
/// Géré via tauri::State. Le keyring reste le store persistant ; TwitchState
/// est la référence en mémoire pour la session courante (évite de relire le
/// keyring à chaque appel Helix).
#[derive(Clone)]
pub struct TwitchState {
    pub irc_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cancel: Arc<std::sync::Mutex<Cancel>>,
    pub connected: Arc<std::sync::Mutex<bool>>,
    pub login: Arc<std::sync::Mutex<Option<String>>>,
    pub user_id: Arc<std::sync::Mutex<Option<String>>>,
    pub access: Arc<std::sync::Mutex<Option<String>>>,
    /// Canal d'envoi de commandes IRC (ex: "/clear") vers la tâche IRC.
    /// None si l'IRC n'est pas démarré. Remplacé à chaque démarrage IRC.
    pub irc_cmd_tx: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::Sender<String>>>>,
}

impl TwitchState {
    fn new() -> Self {
        Self {
            irc_handle: Arc::new(std::sync::Mutex::new(None)),
            cancel: Arc::new(std::sync::Mutex::new(new_cancel())),
            connected: Arc::new(std::sync::Mutex::new(false)),
            login: Arc::new(std::sync::Mutex::new(None)),
            user_id: Arc::new(std::sync::Mutex::new(None)),
            access: Arc::new(std::sync::Mutex::new(None)),
            irc_cmd_tx: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn set_connected(&self, v: bool) {
        *self.connected.lock().unwrap() = v;
    }
    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
    fn set_login(&self, v: Option<String>) {
        *self.login.lock().unwrap() = v;
    }
    fn login_courant(&self) -> Option<String> {
        self.login.lock().unwrap().clone()
    }
    fn set_user_id(&self, v: Option<String>) {
        *self.user_id.lock().unwrap() = v;
    }
    fn user_id_courant(&self) -> Option<String> {
        self.user_id.lock().unwrap().clone()
    }
    fn set_access(&self, v: Option<String>) {
        *self.access.lock().unwrap() = v;
    }
    fn access_courant(&self) -> Option<String> {
        self.access.lock().unwrap().clone()
    }
    /// Arrête l'IRC en cours (si actif) : flag cancel + abort handle + clear canal cmd.
    fn stop_irc(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.irc_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
        // Vider le canal de commandes : la tâche IRC qui le consommait est arrêtée.
        *self.irc_cmd_tx.lock().unwrap() = None;
    }
    /// Prépare un nouveau cancel (fresh) pour un prochain démarrage IRC/flow.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = new_cancel();
    }
    /// Récupère un clone du cancel courant.
    fn cancel_clone(&self) -> Cancel {
        self.cancel.lock().unwrap().clone()
    }
    /// Envoie une commande IRC (ex: "/clear") vers la tâche IRC.
    /// Retourne false si l'IRC n'est pas démarré (canal absent).
    fn envoyer_commande_irc(&self, commande: &str) -> bool {
        let tx = self.irc_cmd_tx.lock().unwrap();
        match tx.as_ref() {
            Some(sender) => {
                let _ = sender.try_send(commande.to_string());
                true
            }
            None => false,
        }
    }
}

/// État Kick partagé : drapeau connecté + slug du canal + handle de la tâche
/// WS + jeton d'annulation (swappable). Le client WebSocket Pusher vit côté
/// Rust (tokio-tungstenite avec Origin: https://kick.com — nécessaire car le
/// webview est rejeté par CORS avec Origin: localhost:1420).
/// Géré via tauri::State. Clonable (Arc internes).
#[derive(Clone)]
pub struct KickState {
    pub connected: Arc<std::sync::Mutex<bool>>,
    pub slug: Arc<std::sync::Mutex<Option<String>>>,
    pub ws_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cancel: Arc<std::sync::Mutex<Arc<AtomicBool>>>,
}

impl KickState {
    fn new() -> Self {
        Self {
            connected: Arc::new(std::sync::Mutex::new(false)),
            slug: Arc::new(std::sync::Mutex::new(None)),
            ws_handle: Arc::new(std::sync::Mutex::new(None)),
            cancel: Arc::new(std::sync::Mutex::new(Arc::new(AtomicBool::new(false)))),
        }
    }
    fn set_connected(&self, v: bool) {
        *self.connected.lock().unwrap() = v;
    }
    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
    fn set_slug(&self, v: Option<String>) {
        *self.slug.lock().unwrap() = v;
    }
    /// Slug du canal Kick connecté (ex: "xqc"). Non utilisé —
    /// réservé pour l'affichage futur.
    #[allow(dead_code)]
    fn slug_courant(&self) -> Option<String> {
        self.slug.lock().unwrap().clone()
    }
    /// Arrête le WS en cours (si actif) : flag cancel + abort handle.
    fn stop_ws(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.ws_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
    }
    /// Prépare un nouveau cancel frais pour la prochaine connexion WS.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = Arc::new(AtomicBool::new(false));
    }
    /// Clone du cancel courant.
    fn cancel_clone(&self) -> Arc<AtomicBool> {
        self.cancel.lock().unwrap().clone()
    }
}

/// État YouTube partagé : handle de la tâche chat polling + jeton d'annulation
/// (swappable) + drapeau connecté + login/channel_id/access token du compte.
/// Géré via tauri::State. Même pattern que TwitchState.
#[derive(Clone)]
pub struct YoutubeState {
    pub chat_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cancel: Arc<std::sync::Mutex<youtube_auth::Cancel>>,
    pub connected: Arc<std::sync::Mutex<bool>>,
    pub login: Arc<std::sync::Mutex<Option<String>>>,
    pub channel_id: Arc<std::sync::Mutex<Option<String>>>,
    pub access: Arc<std::sync::Mutex<Option<String>>>,
}

impl YoutubeState {
    fn new() -> Self {
        Self {
            chat_handle: Arc::new(std::sync::Mutex::new(None)),
            cancel: Arc::new(std::sync::Mutex::new(youtube_auth::new_cancel())),
            connected: Arc::new(std::sync::Mutex::new(false)),
            login: Arc::new(std::sync::Mutex::new(None)),
            channel_id: Arc::new(std::sync::Mutex::new(None)),
            access: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn set_connected(&self, v: bool) {
        *self.connected.lock().unwrap() = v;
    }
    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
    fn set_login(&self, v: Option<String>) {
        *self.login.lock().unwrap() = v;
    }
    fn login_courant(&self) -> Option<String> {
        self.login.lock().unwrap().clone()
    }
    fn set_channel_id(&self, v: Option<String>) {
        *self.channel_id.lock().unwrap() = v;
    }
    fn set_access(&self, v: Option<String>) {
        *self.access.lock().unwrap() = v;
    }
    fn access_courant(&self) -> Option<String> {
        self.access.lock().unwrap().clone()
    }
    /// Arrête le chat polling en cours (si actif) : flag cancel + abort handle.
    fn stop_chat(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.chat_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
    }
    /// Prépare un nouveau cancel (fresh) pour un prochain démarrage/flow.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = youtube_auth::new_cancel();
    }
    /// Récupère un clone du cancel courant.
    fn cancel_clone(&self) -> youtube_auth::Cancel {
        self.cancel.lock().unwrap().clone()
    }
}

/// État TikTok partagé : handle de la tâche chat + jeton d'annulation
/// (swappable) + drapeau connecté + username du streamer suivi.
/// Géré via tauri::State. Même pattern que KickState.
#[derive(Clone)]
pub struct TiktokState {
    pub chat_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cancel: Arc<std::sync::Mutex<Arc<AtomicBool>>>,
    pub connected: Arc<std::sync::Mutex<bool>>,
    pub username: Arc<std::sync::Mutex<Option<String>>>,
}

impl TiktokState {
    fn new() -> Self {
        Self {
            chat_handle: Arc::new(std::sync::Mutex::new(None)),
            cancel: Arc::new(std::sync::Mutex::new(Arc::new(AtomicBool::new(false)))),
            connected: Arc::new(std::sync::Mutex::new(false)),
            username: Arc::new(std::sync::Mutex::new(None)),
        }
    }
    fn set_connected(&self, v: bool) {
        *self.connected.lock().unwrap() = v;
    }
    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
    fn set_username(&self, v: Option<String>) {
        *self.username.lock().unwrap() = v;
    }
    /// Arrête le chat en cours (si actif) : flag cancel + abort handle.
    fn stop_chat(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.chat_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
    }
    /// Prépare un nouveau cancel frais pour la prochaine connexion.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = Arc::new(AtomicBool::new(false));
    }
    /// Clone du cancel courant.
    fn cancel_clone(&self) -> Arc<AtomicBool> {
        self.cancel.lock().unwrap().clone()
    }
}

/// Démarre la connexion Twitch. Si un token valide existe en coffre →
/// valider/refresh + IRC + emit `twitch:connecte`. Sinon → Device Code Flow :
/// emit `twitch:device` (user_code + uri) → spawn poll cancellable → succès :
/// save + IRC + emit `twitch:connecte` ; expire/erreur : emit `twitch:erreur`.
#[tauri::command]
async fn twitch_connecter(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
) -> Result<(), String> {
    if state.is_connected() {
        return Ok(()); // déjà connecté
    }

    // 1. Token en coffre ? → valider (refresh si 401) + IRC direct.
    //    Si échec (token/refresh invalide, révoqué, expiré) : le coffre a été
    //    effacé dans connect_with_tokens, on enchaîne sur le Device Code Flow
    //    pour que l'utilisateur puisse se ré-authentifier (au lieu de rester
    //    bloqué sur un token mort).
    if let Ok(Some(tokens)) = twitch_auth::lire_tokens() {
        if connect_with_tokens(&app, state.inner(), tokens).await.is_ok() {
            return Ok(());
        }
        eprintln!("[Twitch] token coffre invalide → nouveau Device Code Flow");
    }

    // 2. Pas de token (ou token invalide) → Device Code Flow.
    let flow = match twitch_auth::demarrer_device_flow().await {
        Ok(f) => f,
        Err(e) => {
            let _ = app.emit("twitch:erreur", &e);
            return Err(e);
        }
    };

    // Préparer un cancel frais pour ce flow.
    state.reset_cancel();
    let cancel = state.cancel_clone();

    // Émettre les infos device vers le frontend (modal).
    let device_info = serde_json::json!({
        "user_code": flow.user_code,
        "verification_uri": flow.verification_uri,
        "expires_in": flow.expires_in,
    });
    let _ = app.emit("twitch:device", &device_info);

    // Poll en arrière-plan (cancellable). Sur succès → save + IRC + connecte.
    let app2 = app.clone();
    let state2 = state.inner().clone();
    let device_code = flow.device_code;
    let interval = flow.interval;
    let expires_in = flow.expires_in;
    tauri::async_runtime::spawn(async move {
        match twitch_auth::poll_token(device_code, interval, expires_in, cancel).await {
            Ok(tokens) => {
                if let Err(e) = twitch_auth::sauver_tokens(&tokens) {
                    eprintln!("[Twitch] ERR coffre save: {}", e);
                }
                let _ = connect_with_tokens(&app2, &state2, tokens).await;
            }
            Err(e) => {
                eprintln!("[Twitch] ERR poll token: {}", e);
                let _ = app2.emit("twitch:erreur", &e);
            }
        }
    });

    Ok(())
}

/// Helper : valide/refresh les tokens, démarre IRC, emit `twitch:connecte`.
async fn connect_with_tokens(
    app: &AppHandle,
    state: &TwitchState,
    mut tokens: twitch_auth::Tokens,
) -> Result<(), String> {
    // Valider le token ; si 401 → refresh.
    match twitch_auth::valider_token(&tokens.access).await {
        Ok(v) => {
            tokens.login = v.login;
            tokens.user_id = v.user_id;
        }
        Err(e) if e.contains("401") => {
            eprintln!("[Twitch] token expiré, refresh...");
            match twitch_auth::refresh_token(&tokens.refresh).await {
                Ok(t) => {
                    tokens = t;
                    twitch_auth::sauver_tokens(&tokens)
                        .map_err(|e| format!("coffre save: {}", e))?;
                }
                // Refresh impossible (refresh token révoqué/expiré/tourné) :
                // on efface le coffre pour ne pas rester bloqué sur un token
                // mort, et on propage l'erreur. L'appelant (twitch_connecter)
                // peut alors enchaîner sur un nouveau Device Code Flow.
                Err(e) => {
                    eprintln!("[Twitch] refresh échoué, effacement coffre: {}", e);
                    let _ = twitch_auth::effacer_tokens();
                    let _ = app.emit("twitch:erreur", &e);
                    return Err(e);
                }
            }
        }
        Err(e) => {
            // Token invalide et refresh impossible → effacer + erreur.
            let _ = twitch_auth::effacer_tokens();
            let _ = app.emit("twitch:erreur", &e);
            return Err(e);
        }
    }

    // Démarrer IRC.
    state.reset_cancel();
    let cancel = state.cancel_clone();
    let chat_tx = app
        .try_state::<ScenesState>()
        .ok_or("ScenesState absent")?
        .chat_tx
        .clone();
    // Créer le canal de commandes IRC (ex: "/clear" depuis la modale de modération).
    let (irc_cmd_tx, irc_cmd_rx) = tokio::sync::mpsc::channel::<String>(32);
    {
        *state.irc_cmd_tx.lock().unwrap() = Some(irc_cmd_tx);
    }
    let handle = twitch_chat::demarrer(
        app.clone(),
        chat_tx,
        tokens.access.clone(),
        tokens.login.clone(),
        cancel,
        Some(irc_cmd_rx),
    );
    {
        let mut h = state.irc_handle.lock().unwrap();
        *h = Some(handle);
    }
    state.set_connected(true);
    state.set_login(Some(tokens.login.clone()));
    state.set_user_id(Some(tokens.user_id.clone()));
    state.set_access(Some(tokens.access.clone()));
    let _ = app.emit("twitch:connecte", &tokens.login);
    eprintln!("[Twitch] connecté en tant que {}", tokens.login);
    Ok(())
}

/// Annule le Device Code Flow en cours (bouton Annuler du modal).
#[tauri::command]
fn twitch_annuler_device_flow(state: tauri::State<'_, TwitchState>) -> Result<(), String> {
    let c = state.cancel.lock().unwrap();
    c.store(true, Ordering::SeqCst);
    Ok(())
}

/// Déconnecte Twitch : arrête IRC + révoque le token (best-effort) + efface le
/// coffre + emit `twitch:deconnecte`. Revoke non-fatal (réseau) — on efface
/// le coffre quoi qu'il arrive.
#[tauri::command]
async fn twitch_deconnecter(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
) -> Result<(), String> {
    state.stop_irc();
    state.set_connected(false);
    state.set_login(None);
    state.set_user_id(None);
    state.set_access(None);
    // Revoke best-effort : lire le token, révoquer, puis effacer le coffre.
    if let Ok(Some(tokens)) = twitch_auth::lire_tokens() {
        if let Err(e) = twitch_auth::revoke_token(&tokens.access).await {
            eprintln!("[Twitch] WARN revoke échoué (on efface quand même): {}", e);
        }
    }
    let _ = twitch_auth::effacer_tokens();
    let _ = app.emit("twitch:deconnecte", ());
    eprintln!("[Twitch] déconnecté");
    Ok(())
}

/// Retourne l'état de connexion Twitch (true/false).
#[tauri::command]
fn twitch_etat(state: tauri::State<'_, TwitchState>) -> bool {
    state.is_connected()
}

/// Retourne le login du compte Twitch connecté (ou null si déconnecté).
#[tauri::command]
fn twitch_login_courant(state: tauri::State<'_, TwitchState>) -> Option<String> {
    state.login_courant()
}

/// Force une reconnexion Twitch : arrête IRC + révoque le token + efface le
/// coffre + emit `twitch:deconnecte`, puis enchaîne directement le Device Code
/// Flow (même client_id, même coffre, scopes étendus). Utilisé quand les
/// tokens existants n'ont pas les scopes communauté (Helix 403).
#[tauri::command]
async fn twitch_reconnecter(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
) -> Result<(), String> {
    // 1. Déconnexion propre : stop IRC + revoke + efface coffre.
    state.stop_irc();
    state.set_connected(false);
    state.set_login(None);
    state.set_user_id(None);
    state.set_access(None);
    if let Ok(Some(tokens)) = twitch_auth::lire_tokens() {
        if let Err(e) = twitch_auth::revoke_token(&tokens.access).await {
            eprintln!("[Twitch] WARN revoke (reconnect): {}", e);
        }
    }
    let _ = twitch_auth::effacer_tokens();
    let _ = app.emit("twitch:deconnecte", ());
    eprintln!("[Twitch] reconnect: ancien token révoqué+effacé, lancement Device Code");

    // 2. Device Code Flow avec les scopes étendus (même logique que twitch_connecter).
    let flow = match twitch_auth::demarrer_device_flow().await {
        Ok(f) => f,
        Err(e) => {
            let _ = app.emit("twitch:erreur", &e);
            return Err(e);
        }
    };

    state.reset_cancel();
    let cancel = state.cancel_clone();

    let device_info = serde_json::json!({
        "user_code": flow.user_code,
        "verification_uri": flow.verification_uri,
        "expires_in": flow.expires_in,
    });
    let _ = app.emit("twitch:device", &device_info);

    let app2 = app.clone();
    let state2 = state.inner().clone();
    let device_code = flow.device_code;
    let interval = flow.interval;
    let expires_in = flow.expires_in;
    tauri::async_runtime::spawn(async move {
        match twitch_auth::poll_token(device_code, interval, expires_in, cancel).await {
            Ok(tokens) => {
                if let Err(e) = twitch_auth::sauver_tokens(&tokens) {
                    eprintln!("[Twitch] ERR coffre save (reconnect): {}", e);
                }
                let _ = connect_with_tokens(&app2, &state2, tokens).await;
            }
            Err(e) => {
                eprintln!("[Twitch] ERR poll token (reconnect): {}", e);
                let _ = app2.emit("twitch:erreur", &e);
            }
        }
    });

    Ok(())
}

// ===== Communauté lecture (Helix) =====

/// Mappe HelixError → String pour le frontend. "need_reauth" est distinct
/// pour que le frontend affiche le banner de reconnexion.
fn helix_err_to_string(e: twitch_helix::HelixError) -> String {
    match e {
        twitch_helix::HelixError::NeedReauth => "need_reauth".to_string(),
        twitch_helix::HelixError::Deconnecte => "Pas connecté".to_string(),
        twitch_helix::HelixError::Autre(s) => s,
    }
}

/// Followers : liste + total (pagination curseur, max 10 pages).
/// Enrichit avec photos de profil (batch /users) et détecte les unfollows
/// par comparaison avec le snapshot précédent (sauvé sur disque).
#[tauri::command]
async fn twitch_communaute_followers(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::FollowersResp, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut resp = twitch_helix::followers(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;

    // Enrichir avec photos de profil (batch /users, max 100 par requête).
    // Non-fatal : si ça échoue, on garde les followers sans avatar.
    if let Err(e) = twitch_helix::enrichir_avatars(&mut resp.liste, &access).await {
        eprintln!("[Communauté] enrichir_avatars: {:?} (non-fatal)", e);
    }

    // Détection des unfollows : comparaison avec le snapshot précédent.
    let maintenant = chrono::Utc::now().to_rfc3339();
    let snap_precedent = match config::lire_followers_snapshot(&app) {
        Ok(Some(s)) => Some(s),
        Ok(None) => None,
        Err(e) => {
            eprintln!("[Communauté] lire_followers_snapshot: {} (non-fatal)", e);
            None
        }
    };

    // Si on a un snapshot précédent (non vide), détecter les manquants.
    if let Some(ref snap) = snap_precedent {
        if !snap.liste.is_empty() {
            let courants: std::collections::HashSet<String> =
                resp.liste.iter().map(|f| f.user_id.clone()).collect();
            let unfollows: Vec<config::UnfollowEntry> = snap
                .liste
                .iter()
                .filter(|f| !courants.contains(&f.user_id))
                .map(|f| config::UnfollowEntry {
                    user_id: f.user_id.clone(),
                    login: f.login.clone(),
                    date_unfollow: maintenant.clone(),
                    display_name: f.display_name.clone(),
                    profile_image_url: f.profile_image_url.clone(),
                })
                .collect();
            if !unfollows.is_empty() {
                eprintln!(
                    "[Communauté] {} unfollow(s) détecté(s)",
                    unfollows.len()
                );
                for u in &unfollows {
                    eprintln!("  → {} ({})", u.login, u.date_unfollow);
                }
                if let Err(e) = config::ajouter_unfollows(&app, &unfollows) {
                    eprintln!("[Communauté] ajouter_unfollows: {} (non-fatal)", e);
                }
            }
        }
    }

    // Sauver le nouveau snapshot (écrase l'ancien).
    let snap = config::FollowersSnapshot {
        timestamp: maintenant,
        liste: resp
            .liste
            .iter()
            .map(|f| config::SnapshotFollower {
                user_id: f.user_id.clone(),
                login: f.login.clone(),
                followed_at: f.followed_at.clone(),
                display_name: f.display_name.clone(),
                profile_image_url: f.profile_image_url.clone(),
            })
            .collect(),
    };
    if let Err(e) = config::sauver_followers_snapshot(&app, &snap) {
        eprintln!("[Communauté] sauver_followers_snapshot: {} (non-fatal)", e);
    }

    Ok(resp)
}

/// Followers allégé (user_id + followed_at uniquement) pour le polling des
/// alertes (60s). Pas d'enrichissement d'avatars (batch /users), pas de
/// détection d'unfollows, pas de snapshot disque — la commande lourde
/// `twitch_communaute_followers` s'en charge au boot et au refresh manuel.
/// Évite la double pagination + enrichissement quand le poll des alertes
/// tourne en parallèle du chargement communauté.
#[tauri::command]
async fn twitch_followers_light(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::FollowerLight>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let resp = twitch_helix::followers(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    Ok(resp
        .liste
        .into_iter()
        .map(|f| twitch_helix::FollowerLight {
            user_id: f.user_id,
            login: f.login,
            followed_at: f.followed_at,
        })
        .collect())
}

/// Subs : liste + total + points (pagination, max 10 pages).
#[tauri::command]
async fn twitch_communaute_subs(
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::SubsResp, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut resp = twitch_helix::subs(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    // Enrichir avec photos de profil (non-fatal).
    let ids: Vec<String> = resp.liste.iter().map(|s| s.user_id.clone()).collect();
    if let Ok(map) = twitch_helix::fetch_users_batch(&ids, &access).await {
        for s in &mut resp.liste {
            if let Some((dn, url)) = map.get(&s.user_id) {
                if !dn.is_empty() {
                    s.display_name = dn.clone();
                }
                s.profile_image_url = url.clone();
            }
        }
    }
    Ok(resp)
}

/// Viewers live : chiffre si en live, None si hors-ligne.
#[tauri::command]
async fn twitch_communaute_viewers(
    state: tauri::State<'_, TwitchState>,
) -> Result<Option<u32>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::stream_viewers(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Broadcaster : display_name, avatar, type, description.
#[tauri::command]
async fn twitch_broadcaster(
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::BroadcasterInfo, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::broadcaster(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Unfollows : historique des unfollows détectés au démarrage (lecture disque).
#[tauri::command]
fn twitch_communaute_unfollows(
    app: AppHandle,
) -> Result<Vec<config::UnfollowEntry>, String> {
    let hist = config::lire_unfollows(&app)?;
    // Tri : plus récent en premier.
    let mut liste = hist.liste;
    liste.sort_by(|a, b| b.date_unfollow.cmp(&a.date_unfollow));
    Ok(liste)
}

// ===== Modération Twitch (Helix lecture + écriture) =====

/// Liste les VIPs de la chaîne.
#[tauri::command]
async fn twitch_lister_vips(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::VipEntry>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut liste = twitch_helix::list_vips(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    let ids: Vec<String> = liste.iter().map(|v| v.user_id.clone()).collect();
    if let Ok(map) = twitch_helix::fetch_users_batch(&ids, &access).await {
        for v in &mut liste {
            if let Some((_, url)) = map.get(&v.user_id) {
                v.profile_image_url = url.clone();
            }
        }
    }
    Ok(liste)
}

/// Liste les modérateurs de la chaîne.
#[tauri::command]
async fn twitch_lister_moderateurs(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::ModEntry>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut liste = twitch_helix::list_moderators(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    let ids: Vec<String> = liste.iter().map(|m| m.user_id.clone()).collect();
    if let Ok(map) = twitch_helix::fetch_users_batch(&ids, &access).await {
        for m in &mut liste {
            if let Some((_, url)) = map.get(&m.user_id) {
                m.profile_image_url = url.clone();
            }
        }
    }
    Ok(liste)
}

/// Liste les utilisateurs bannis/timeout de la chaîne.
#[tauri::command]
async fn twitch_lister_bannis(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::BannedEntry>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut liste = twitch_helix::list_banned(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    let ids: Vec<String> = liste.iter().map(|b| b.user_id.clone()).collect();
    if let Ok(map) = twitch_helix::fetch_users_batch(&ids, &access).await {
        for b in &mut liste {
            if let Some((dn, url)) = map.get(&b.user_id) {
                if !dn.is_empty() {
                    b.display_name = dn.clone();
                }
                b.profile_image_url = url.clone();
            }
        }
    }
    Ok(liste)
}

/// Résout un login Twitch en user_id (pour la recherche par pseudo).
#[tauri::command]
async fn twitch_resoudre_user(
    state: tauri::State<'_, TwitchState>,
    login: String,
) -> Result<Option<twitch_helix::ResolvedUser>, String> {
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::resolve_user_id(&login, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Bannir ou timeout un utilisateur. `duree` = None → ban permanent,
/// Some(n) → timeout de n secondes.
#[tauri::command]
async fn twitch_bannir(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
    raison: String,
    duree: Option<u32>,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::ban_user(&uid, &access, &user_id, &raison, duree)
        .await
        .map_err(helix_err_to_string)
}

/// Débannir un utilisateur.
#[tauri::command]
async fn twitch_debannir(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::unban_user(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Ajouter un VIP.
#[tauri::command]
async fn twitch_ajouter_vip(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::add_vip(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Retirer un VIP.
#[tauri::command]
async fn twitch_retirer_vip(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::remove_vip(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Ajouter un modérateur.
#[tauri::command]
async fn twitch_ajouter_moderateur(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::add_moderator(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Retirer un modérateur.
#[tauri::command]
async fn twitch_retirer_moderateur(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::remove_moderator(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Supprimer un message de chat (par message_id).
#[tauri::command]
async fn twitch_supprimer_message(
    state: tauri::State<'_, TwitchState>,
    message_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::delete_chat_message(&uid, &access, &message_id)
        .await
        .map_err(helix_err_to_string)
}

/// Envoie une commande IRC au chat Twitch (ex: "/clear", "/ban user").
/// Non-fatal si l'IRC n'est pas démarré (retourne false).
#[tauri::command]
fn twitch_envoyer_commande_chat(
    state: tauri::State<'_, TwitchState>,
    commande: String,
) -> bool {
    state.envoyer_commande_irc(&commande)
}

// ===== Kick (lecture seule, WS côté Rust) =====

/// Connecte au chat Kick : résout le slug → chatroom ID, fetch un viewer token,
/// ouvre le WebSocket Pusher (côté Rust avec Origin: https://kick.com), subscribe
/// au channel chatrooms.<id>.v2. Le WS tourne en arrière-plan (tokio::spawn).
/// emit `kick:connecte` quand subscribed, `kick:deconnecte` sur close/error.
#[tauri::command]
async fn kick_connecter(
    app: AppHandle,
    state: tauri::State<'_, KickState>,
    slug: String,
) -> Result<(), String> {
    if state.is_connected() {
        return Ok(()); // déjà connecté
    }

    // Arrêter un éventuel WS précédent.
    state.stop_ws();
    state.reset_cancel();

    // 1. Résoudre slug → chatroom ID
    eprintln!("[Kick] connecter: resolve slug={}", slug);
    let chatroom_id = kick::resolve_chatroom(&slug).await?;

    // 2. Démarrer le WS Pusher cloud en arrière-plan
    //    (pas besoin de viewer token ni cookies — le cloud Pusher public
    //    n'a pas de protection Cloudflare)
    let cancel = state.cancel_clone();
    let handle = kick::run_ws(app.clone(), chatroom_id, cancel);

    {
        let mut h = state.ws_handle.lock().unwrap();
        *h = Some(handle);
    }
    state.set_slug(Some(slug.clone()));
    // Persister le slug pour l'auto-resume au prochain boot.
    if let Err(e) = crate::config::sauver_kick_slug(&app, &slug) {
        eprintln!("[Kick] WARN sauver slug: {}", e);
    }
    // set_connected(true) se fait dans run_ws quand subscription_succeeded.
    Ok(())
}

/// Déconnecte Kick : arrête le WS (cancel + abort) + emit `kick:deconnecte`.
#[tauri::command]
fn kick_deconnecter(
    app: AppHandle,
    state: tauri::State<'_, KickState>,
) -> Result<(), String> {
    state.stop_ws();
    state.set_connected(false);
    state.set_slug(None);
    // Effacer le slug persisté (déconnexion volontaire).
    if let Err(e) = crate::config::effacer_kick_slug(&app) {
        eprintln!("[Kick] WARN effacer slug: {}", e);
    }
    let _ = app.emit("kick:deconnecte", ());
    eprintln!("[Kick] déconnecté");
    Ok(())
}

/// Retourne l'état de connexion Kick (true/false).
#[tauri::command]
fn kick_etat(state: tauri::State<'_, KickState>) -> bool {
    state.is_connected()
}

/// Retourne le slug Kick sauvegardé (pour pré-remplir l'input).
/// None si pas de slug sauvegardé.
#[tauri::command]
fn kick_slug_courant(app: AppHandle) -> Option<String> {
    crate::config::lire_kick_slug(&app).ok().flatten()
}

// ===== YouTube (chat polling + communauté Data API v3) =====

/// Connecte YouTube : si token valide en coffre → valider + resolve channel +
/// start chat polling + emit `youtube:connecte`. Sinon → Device Code Flow :
/// emit `youtube:device` (user_code + uri) → spawn poll cancellable → succès :
/// save + resolve channel + start chat + emit `youtube:connecte`.
#[tauri::command]
async fn youtube_connecter(
    app: AppHandle,
    state: tauri::State<'_, YoutubeState>,
) -> Result<(), String> {
    if state.is_connected() {
        return Ok(()); // déjà connecté
    }

    // 1. Token en coffre ? → valider (refresh si 401) + chat direct.
    if let Ok(Some(tokens)) = youtube_auth::lire_tokens() {
        return connect_with_tokens_youtube(&app, state.inner(), tokens).await;
    }

    // 2. Pas de token → Device Code Flow.
    let flow = match youtube_auth::demarrer_device_flow().await {
        Ok(f) => f,
        Err(e) => {
            let _ = app.emit("youtube:erreur", &e);
            return Err(e);
        }
    };

    // Préparer un cancel frais pour ce flow.
    state.reset_cancel();
    let cancel = state.cancel_clone();

    // Émettre les infos device vers le frontend (modal).
    let device_info = serde_json::json!({
        "user_code": flow.user_code,
        "verification_uri": flow.verification_url,
        "expires_in": flow.expires_in,
    });
    let _ = app.emit("youtube:device", &device_info);

    // Poll en arrière-plan (cancellable). Sur succès → save + chat + connecte.
    let app2 = app.clone();
    let state2 = state.inner().clone();
    let device_code = flow.device_code;
    let interval = flow.interval;
    let expires_in = flow.expires_in;
    tauri::async_runtime::spawn(async move {
        match youtube_auth::poll_token(device_code, interval, expires_in, cancel).await {
            Ok(mut tokens) => {
                // Resolve channel_id + login via Data API.
                match youtube_data::resolve_channel(&tokens.access).await {
                    Ok((channel_id, login)) => {
                        tokens.channel_id = channel_id;
                        tokens.login = login;
                    }
                    Err(e) => {
                        eprintln!("[YouTube] WARN resolve channel échoué: {}", e);
                        // Non-fatal : on continue sans channel_id/login.
                    }
                }
                if let Err(e) = youtube_auth::sauver_tokens(&tokens) {
                    eprintln!("[YouTube] ERR coffre save: {}", e);
                }
                let _ = connect_with_tokens_youtube(&app2, &state2, tokens).await;
            }
            Err(e) => {
                eprintln!("[YouTube] ERR poll token: {}", e);
                let _ = app2.emit("youtube:erreur", &e);
            }
        }
    });

    Ok(())
}

/// Helper : valide/refresh les tokens, resolve channel, démarre chat polling,
/// emit `youtube:connecte`.
async fn connect_with_tokens_youtube(
    app: &AppHandle,
    state: &YoutubeState,
    mut tokens: youtube_auth::Tokens,
) -> Result<(), String> {
    // Valider le token via tokeninfo (non-fatal si échec).
    if tokens.user_id.is_empty() {
        if let Ok(uid) = youtube_auth::valider_token(&tokens.access).await {
            tokens.user_id = uid;
        }
    }

    // Resolve channel_id + login si manquants.
    if tokens.channel_id.is_empty() || tokens.login.is_empty() {
        match youtube_data::resolve_channel(&tokens.access).await {
            Ok((channel_id, login)) => {
                tokens.channel_id = channel_id;
                tokens.login = login;
                // Re-sauver avec les infos complètes.
                if let Err(e) = youtube_auth::sauver_tokens(&tokens) {
                    eprintln!("[YouTube] WARN coffre save (resolve): {}", e);
                }
            }
            Err(e) => {
                eprintln!("[YouTube] WARN resolve channel: {}", e);
                // Non-fatal : on continue sans channel_id/login.
            }
        }
    }

    // NE PAS démarrer le chat polling automatiquement — l'utilisateur doit
    // cliquer sur "Chat live ON" manuellement (bouton dans la Toolbar).
    // Le chat polling consomme de l'API quota YouTube, on évite le polling
    // inutile quand l'utilisateur ne stream pas.
    state.set_connected(true);
    state.set_login(Some(tokens.login.clone()));
    state.set_channel_id(Some(tokens.channel_id.clone()));
    state.set_access(Some(tokens.access.clone()));
    let _ = app.emit("youtube:connecte", &tokens.login);
    eprintln!("[YouTube] connecté en tant que {} (chat OFF par défaut)", tokens.login);
    Ok(())
}

/// Démarre manuellement le chat polling YouTube Live. Appelé par le bouton
/// "Chat live ON" dans la Toolbar. Si pas de live actif → emit youtube:pas-de-live
/// et la tâche s'arrête immédiatement (l'utilisateur réessaiera quand il stream).
#[tauri::command]
fn youtube_demarrer_chat(
    app: AppHandle,
    state: tauri::State<'_, YoutubeState>,
) -> Result<(), String> {
    if !state.is_connected() {
        return Err("YouTube non connecté".to_string());
    }
    // Arrêter un éventuel chat précédent.
    state.stop_chat();
    state.reset_cancel();
    let cancel = state.cancel_clone();
    let access = state.access_courant().ok_or("Pas de token access")?;
    let channel_id = state
        .login
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|_| state.channel_id.lock().unwrap().clone())
        .ok_or("Pas de channel_id")?;
    let chat_tx = app
        .try_state::<ScenesState>()
        .ok_or("ScenesState absent")?
        .chat_tx
        .clone();
    let handle = youtube_chat::demarrer(
        app.clone(),
        chat_tx,
        access,
        channel_id,
        cancel,
    );
    {
        let mut h = state.chat_handle.lock().unwrap();
        *h = Some(handle);
    }
    eprintln!("[YouTube] chat polling démarré manuellement");
    Ok(())
}

/// Arrête le chat polling YouTube Live sans déconnecter le compte
/// (bouton "Chat live OFF"). Le token et les infos chaîne restent en mémoire.
#[tauri::command]
fn youtube_arreter_chat(
    app: AppHandle,
    state: tauri::State<'_, YoutubeState>,
) -> Result<(), String> {
    state.stop_chat();
    state.reset_cancel();
    let _ = app.emit("youtube:pas-de-live", ());
    eprintln!("[YouTube] chat polling arrêté manuellement");
    Ok(())
}

/// Annule le Device Code Flow YouTube en cours.
#[tauri::command]
fn youtube_annuler_device_flow(state: tauri::State<'_, YoutubeState>) -> Result<(), String> {
    let c = state.cancel.lock().unwrap();
    c.store(true, Ordering::SeqCst);
    Ok(())
}

/// Déconnecte YouTube : arrête chat + révoque token (best-effort) + efface coffre
/// + emit `youtube:deconnecte`.
#[tauri::command]
async fn youtube_deconnecter(
    app: AppHandle,
    state: tauri::State<'_, YoutubeState>,
) -> Result<(), String> {
    state.stop_chat();
    state.set_connected(false);
    state.set_login(None);
    state.set_channel_id(None);
    state.set_access(None);
    // Revoke best-effort.
    if let Ok(Some(tokens)) = youtube_auth::lire_tokens() {
        if let Err(e) = youtube_auth::revoke_token(&tokens.access).await {
            eprintln!("[YouTube] WARN revoke échoué (on efface quand même): {}", e);
        }
    }
    let _ = youtube_auth::effacer_tokens();
    let _ = app.emit("youtube:deconnecte", ());
    eprintln!("[YouTube] déconnecté");
    Ok(())
}

/// Retourne l'état de connexion YouTube (true/false).
#[tauri::command]
fn youtube_etat(state: tauri::State<'_, YoutubeState>) -> bool {
    state.is_connected()
}

/// Retourne le login de la chaîne YouTube connectée (ou null si déconnecté).
#[tauri::command]
fn youtube_login_courant(state: tauri::State<'_, YoutubeState>) -> Option<String> {
    state.login_courant()
}

/// Channel info : GET /channels?part=snippet,statistics&mine=true.
#[tauri::command]
async fn youtube_communaute_channel(
    state: tauri::State<'_, YoutubeState>,
) -> Result<youtube_data::ChannelInfo, String> {
    let access = state
        .access_courant()
        .ok_or("YouTube non connecté")?;
    youtube_data::channel_info(&access)
        .await
        .map_err(|e| match e {
            youtube_data::YoutubeError::Deconnecte => "YouTube non connecté".to_string(),
            youtube_data::YoutubeError::Autre(s) => s,
        })
}

/// Members : GET /members?part=snippet.
#[tauri::command]
async fn youtube_communaute_members(
    state: tauri::State<'_, YoutubeState>,
) -> Result<youtube_data::MembersResp, String> {
    let access = state
        .access_courant()
        .ok_or("YouTube non connecté")?;
    youtube_data::members(&access)
        .await
        .map_err(|e| match e {
            youtube_data::YoutubeError::Deconnecte => "YouTube non connecté".to_string(),
            youtube_data::YoutubeError::Autre(s) => s,
        })
}

/// Live viewers : search?channelId=...&eventType=live → /videos → concurrentViewers.
#[tauri::command]
async fn youtube_communaute_viewers(
    state: tauri::State<'_, YoutubeState>,
) -> Result<Option<u32>, String> {
    let access = state
        .access_courant()
        .ok_or("YouTube non connecté")?;
    let channel_id = state
        .channel_id
        .lock().unwrap()
        .clone()
        .ok_or("YouTube non connecté")?;
    youtube_data::live_viewers(&access, &channel_id)
        .await
        .map_err(|e| match e {
            youtube_data::YoutubeError::Deconnecte => "YouTube non connecté".to_string(),
            youtube_data::YoutubeError::Autre(s) => s,
        })
}

// ===== TikTok (chat live via PirateTok — reverse engineering Webcast) =====

/// Connecte au chat TikTok Live d'un streamer (username sans @).
/// Démarre le WebSocket PirateTok en arrière-plan (tokio::spawn).
/// emit `tiktok:connecte` quand connecté, `tiktok:deconnecte` sur close/error,
/// `chat:message` pour chaque message chat, `tiktok:viewers` pour le count.
#[tauri::command]
async fn tiktok_connecter(
    app: AppHandle,
    state: tauri::State<'_, TiktokState>,
    username: String,
) -> Result<(), String> {
    if state.is_connected() {
        return Ok(()); // déjà connecté
    }

    // Arrêter un éventuel chat précédent.
    state.stop_chat();
    state.reset_cancel();

    // Récupérer le chat_tx (broadcast vers :4321).
    let chat_tx = app
        .try_state::<ScenesState>()
        .map(|s| s.chat_tx.clone())
        .ok_or("ScenesState non initialisé")?;

    // Démarrer le WebSocket PirateTok en arrière-plan.
    let cancel = state.cancel_clone();
    let handle = tiktok_chat::demarrer(app.clone(), chat_tx, username.clone(), cancel);

    {
        let mut h = state.chat_handle.lock().unwrap();
        *h = Some(handle);
    }
    state.set_username(Some(username.clone()));
    // Persister le username pour l'auto-resume au prochain boot.
    if let Err(e) = crate::config::sauver_tiktok_username(&app, &username) {
        eprintln!("[TikTok] WARN sauver username: {}", e);
    }
    // set_connected(true) se fait via l'event tiktok:connecte (frontend listener).
    // Mais on le set aussi ici car l'event arrive async.
    Ok(())
}

/// Déconnecte TikTok : arrête le chat (cancel + abort) + emit `tiktok:deconnecte`.
#[tauri::command]
fn tiktok_deconnecter(
    app: AppHandle,
    state: tauri::State<'_, TiktokState>,
) -> Result<(), String> {
    state.stop_chat();
    state.set_connected(false);
    state.set_username(None);
    if let Err(e) = crate::config::effacer_tiktok_username(&app) {
        eprintln!("[TikTok] WARN effacer username: {}", e);
    }
    let _ = app.emit("tiktok:deconnecte", ());
    eprintln!("[TikTok] déconnecté");
    Ok(())
}

/// Retourne l'état de connexion TikTok (true/false).
#[tauri::command]
fn tiktok_etat(state: tauri::State<'_, TiktokState>) -> bool {
    state.is_connected()
}

/// Retourne le username TikTok sauvegardé (pour pré-remplir l'input).
/// None si pas de username sauvegardé.
#[tauri::command]
fn tiktok_username_courant(app: AppHandle) -> Option<String> {
    crate::config::lire_tiktok_username(&app).ok().flatten()
}

// ===== Clips de bienvenue (welcome) =====

/// Retourne l'état courant de la file d'attente welcome (clip en cours + queue).
#[tauri::command]
fn welcome_etat(state: tauri::State<'_, welcome::WelcomeState>) -> welcome::QueueEtat {
    state.etat()
}

/// Retourne le registre Twitch complet (login → config viewer).
#[tauri::command]
fn welcome_registre_twitch(
    state: tauri::State<'_, welcome::WelcomeState>,
) -> Vec<(String, welcome::ViewerConfig)> {
    state.lire_registre_twitch()
}

/// Sauvegarde la config d'un viewer Twitch dans le registre + disque.
#[tauri::command]
fn welcome_sauver_viewer_twitch(
    state: tauri::State<'_, welcome::WelcomeState>,
    login: String,
    config: welcome::ViewerConfig,
) -> Result<(), String> {
    state.sauver_viewer_twitch(&login, config)
}

/// Supprime un viewer Twitch du registre + disque.
#[tauri::command]
fn welcome_supprimer_viewer_twitch(
    state: tauri::State<'_, welcome::WelcomeState>,
    login: String,
) -> Result<(), String> {
    state.supprimer_viewer_twitch(&login)
}

/// Active/désactive la config globale des clips de bienvenue.
#[tauri::command]
fn welcome_config_globale_actif(
    state: tauri::State<'_, welcome::WelcomeState>,
    actif: bool,
) -> Result<(), String> {
    state.set_config_globale_actif(actif)
}

/// Met à jour la config de l'overlay welcome (position/taille côté diffusion).
#[tauri::command]
fn welcome_set_overlay_config(
    state: tauri::State<'_, welcome::WelcomeState>,
    config: welcome::OverlayConfig,
) -> Result<(), String> {
    state.set_overlay_config(config)
}

/// Test manuel : lance un clip côté diffusion (bypass queue). Résout MP4 +
/// émet welcome-clip-play. Utilisé par la modale Interaction viewer pour
/// tester un clip au clic.
#[tauri::command]
async fn welcome_tester_clip(
    state: tauri::State<'_, welcome::WelcomeState>,
    twitch: tauri::State<'_, TwitchState>,
    clip_id: String,
    clip_titre: String,
    clip_duree_ms: u64,
    display_name: String,
) -> Result<(), String> {
    // Avatar + bio via Helix /users (best-effort : None si Twitch déconnecté
    // ou fetch échoué — la carte s'affiche alors sans avatar/bio).
    let access_opt = { twitch.access.lock().unwrap().clone() };
    let (avatar, bio) = match access_opt {
        Some(access) => {
            let client = reqwest::Client::new();
            match twitch_chat::fetch_user_infos(&client, &access, &display_name.to_lowercase()).await {
                Ok((av, b)) => (
                    Some(av).filter(|s| !s.is_empty()),
                    Some(b).filter(|s| !s.is_empty()),
                ),
                Err(e) => {
                    eprintln!("[Welcome] tester_clip: fetch avatar/bio échoué : {}", e);
                    (None, None)
                }
            }
        }
        None => (None, None),
    };
    state
        .tester_clip(&clip_id, &clip_titre, clip_duree_ms, &display_name, avatar, bio)
        .await
}

/// Définit la durée d'affichage globale des clips (0 = durée naturelle).
#[tauri::command]
fn welcome_set_duree_affichage(
    state: tauri::State<'_, welcome::WelcomeState>,
    ms: u64,
) -> Result<(), String> {
    state.set_duree_affichage(ms)
}

/// Stop le clip courant + vide la queue.
#[tauri::command]
fn welcome_stop(state: tauri::State<'_, welcome::WelcomeState>) {
    state.stop();
}

/// Skip le clip courant → passe au suivant.
#[tauri::command]
fn welcome_skip(state: tauri::State<'_, welcome::WelcomeState>) {
    state.skip();
}

/// Retire un item spécifique de la queue (par id).
#[tauri::command]
fn welcome_retirer(
    state: tauri::State<'_, welcome::WelcomeState>,
    id: String,
) {
    state.retirer(&id);
}

/// Remonte un item dans la queue (vers le début).
#[tauri::command]
fn welcome_remonter(
    state: tauri::State<'_, welcome::WelcomeState>,
    id: String,
) {
    state.remonter(&id);
}

/// Descend un item dans la queue (vers la fin).
#[tauri::command]
fn welcome_descendre(
    state: tauri::State<'_, welcome::WelcomeState>,
    id: String,
) {
    state.descendre(&id);
}

/// Vide la queue (sans stopper le clip courant).
#[tauri::command]
fn welcome_vider(state: tauri::State<'_, welcome::WelcomeState>) {
    state.vider();
}

/// Reset le seen set (nouveau stream → tous les streamers redeviennent éligibles).
#[tauri::command]
fn welcome_reset_session(state: tauri::State<'_, welcome::WelcomeState>) {
    state.reset_session();
}

/// Liste les clips récents d'un broadcaster Twitch (GET /clips Helix).
/// `broadcaster_id` = user_id du streamer. `first` = nombre de clips (max 100).
#[tauri::command]
async fn welcome_lister_clips_streamer(
    state: tauri::State<'_, TwitchState>,
    broadcaster_id: String,
    first: u32,
) -> Result<Vec<twitch_clips::ClipInfo>, String> {
    let access = state
        .access
        .lock()
        .unwrap()
        .clone()
        .ok_or("Twitch non connecté")?;
    twitch_clips::liste_clips(&broadcaster_id, &access, first).await
}

/// Résout l'URL MP4 signée d'un clip Twitch (slug) via GQL.
#[tauri::command]
async fn welcome_resoudre_mp4(slug: String) -> Result<String, String> {
    twitch_clips::resoudre_mp4(&slug).await
}

/// Attribution automatique : pour chaque follower, fetch ses clips → clip aléatoire
/// → résout MP4 → sauve config. Retourne { succes, echecs }.
/// `followers` = liste de { login, user_id, display_name }.
#[tauri::command]
async fn welcome_attribuer_auto(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
    welcome_state: tauri::State<'_, welcome::WelcomeState>,
    followers: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let access = state
        .access
        .lock()
        .unwrap()
        .clone()
        .ok_or("Twitch non connecté")?;

    let mut succes = 0u32;
    let mut echecs = 0u32;

    for f in &followers {
        let user_id = f["user_id"].as_str().unwrap_or("");
        let login = f["login"].as_str().unwrap_or("").to_lowercase();
        let display_name = f["display_name"]
            .as_str()
            .or_else(|| f["login"].as_str())
            .unwrap_or("")
            .to_string();

        if user_id.is_empty() || login.is_empty() {
            echecs += 1;
            continue;
        }

        match twitch_clips::clip_aleatoire_resolu(user_id, &access).await {
            Ok(Some(clip)) => {
                let config = welcome::ViewerConfig {
                    actif: true,
                    clip_id: clip.info.id.clone(),
                    clip_titre: clip.info.title.clone(),
                    clip_thumbnail: clip.info.thumbnail_url.clone(),
                    clip_mp4_url: clip.mp4_url.clone(),
                    clip_duree_ms: clip.duree_ms,
                    message: format!("Bienvenue {} !", display_name),
                    display_name: display_name.clone(),
                    avatar: None,
                    bio: None,
                };
                if let Err(e) = welcome_state.sauver_viewer_twitch(&login, config) {
                    eprintln!("[Welcome] attribuer_auto: erreur save {} : {}", login, e);
                    echecs += 1;
                } else {
                    succes += 1;
                }
            }
            Ok(None) => {
                eprintln!("[Welcome] attribuer_auto: aucun clip pour {}", login);
                echecs += 1;
            }
            Err(e) => {
                eprintln!("[Welcome] attribuer_auto: erreur {} : {}", login, e);
                echecs += 1;
            }
        }
        // Émettre progression vers le dashboard (non-fatal).
        let _ = app.emit(
            "welcome:attribution-progress",
            serde_json::json!({ "succes": succes, "echecs": echecs, "login": login }),
        );
    }

    eprintln!("[Welcome] attribuer_auto terminé : {} succes, {} echecs", succes, echecs);
    Ok(serde_json::json!({ "succes": succes, "echecs": echecs }))
}

// ===== Bandeau premier message =====

/// Retourne l'état courant du bandeau (config : actif, duree, position).
#[tauri::command]
fn bandeau_etat(state: tauri::State<'_, bandeau::BandeauState>) -> bandeau::BandeauEtat {
    state.etat()
}

/// Met à jour la config du bandeau (merge partiel : actif/duree_ms/position).
/// Persiste + émet la config vers diffusion.html + l'état vers le dashboard.
#[tauri::command]
fn bandeau_set_config(
    state: tauri::State<'_, bandeau::BandeauState>,
    actif: Option<bool>,
    duree_ms: Option<u64>,
    position: Option<String>,
) -> Result<(), String> {
    state.set_config(actif, duree_ms, position)
}

/// Stop immédiat du bandeau courant (annule timer + émet stop vers diffusion).
#[tauri::command]
fn bandeau_stop(state: tauri::State<'_, bandeau::BandeauState>) {
    state.stop();
}

/// Reset le seen set (nouveau stream → tous les viewers redeviennent éligibles).
#[tauri::command]
fn bandeau_reset_session(state: tauri::State<'_, bandeau::BandeauState>) {
    state.reset_session();
}

/// Test manuel : lance un bandeau côté diffusion (bypass détection).
#[tauri::command]
fn bandeau_tester(
    state: tauri::State<'_, bandeau::BandeauState>,
    display_name: String,
    message: String,
) -> Result<(), String> {
    state.tester(&display_name, &message)
}

// ===== Commandes chat (!commande → overlay diffusion) =====

/// Liste complète des commandes chat (pour l'UI dashboard).
#[tauri::command]
fn commandes_etat(
    state: tauri::State<'_, commandes::CommandesState>,
) -> Vec<commandes::CommandeConfig> {
    state.etat()
}

/// Ajoute/remplace une commande chat (upsert par id) + persiste + push WS.
/// Retourne la config stockée (id généré si vide).
#[tauri::command]
fn commande_set_config(
    state: tauri::State<'_, commandes::CommandesState>,
    config: commandes::CommandeConfig,
) -> Result<commandes::CommandeConfig, String> {
    state.set_config(config)
}

/// Supprime une commande chat + persiste + push WS diffusion.
#[tauri::command]
fn commande_supprimer(
    state: tauri::State<'_, commandes::CommandesState>,
    id: String,
) -> Result<(), String> {
    state.supprimer(&id)
}

/// Test manuel : déclenche une commande côté diffusion (bypass cooldowns).
#[tauri::command]
fn commande_tester(
    state: tauri::State<'_, commandes::CommandesState>,
    id: String,
) -> Result<(), String> {
    state.tester(&id)
}

// ===== Input Viewer (capture globale clavier + souris → overlay diffusion) =====

/// Active/désactive la capture globale des entrées (clavier + souris).
/// ON → thread poll clavier 60Hz + hook souris WH_MOUSE_LL (zéro coût quand
/// OFF). L'état est émis directement sur le WS :4321 (input-viewer-etat).
#[tauri::command]
fn input_viewer_set_actif(
    state: tauri::State<'_, input_viewer::InputViewerState>,
    actif: bool,
) -> Result<(), String> {
    eprintln!("[InputViewer] commande set_actif({}) appelée", actif);
    state.set_actif(actif)
}

/// true si la capture Input Viewer est active (pour l'état du bouton ON/OFF).
#[tauri::command]
fn input_viewer_etat_actif(state: tauri::State<'_, input_viewer::InputViewerState>) -> bool {
    state.est_actif()
}

// ===== Pad numérique (16 touches Numpad × 3 plages → overlay diffusion) =====

/// Retourne la config complète du pad (actif, plage, touches).
#[tauri::command]
fn pad_etat(state: tauri::State<'_, pad_numerique::PadNumeriqueState>) -> pad_numerique::PadConfig {
    state.etat()
}

/// Active/désactive le pad + démarre/arrête la capture clavier globale + persiste.
#[tauri::command]
fn pad_set_actif(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    actif: bool,
) -> Result<(), String> {
    state.set_actif(actif)
}

/// Remplace la config d'une touche (upsert) + persiste + émet état.
#[tauri::command]
fn pad_set_touche(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    code: String,
    plage: u32,
    config: pad_numerique::ToucheConfig,
) -> Result<(), String> {
    state.set_touche(&code, plage, config)
}

/// Supprime (reset) la config d'une touche + persiste + émet état.
#[tauri::command]
fn pad_supprimer_touche(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    code: String,
    plage: u32,
) -> Result<(), String> {
    state.supprimer_touche(&code, plage)
}

/// Change la plage courante (0-2) + persiste + émet état.
#[tauri::command]
fn pad_set_plage(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    plage: u32,
) -> Result<(), String> {
    state.set_plage(plage)
}

/// Test manuel : déclenche une touche côté diffusion (bypass capture).
#[tauri::command]
fn pad_tester_touche(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    code: String,
    plage: u32,
) -> Result<(), String> {
    state.tester_touche(&code, plage)
}

/// Importe un média (image OU vidéo) pour une touche du pad : dialog filtré
/// selon le kind attendu → validation + copie (ou remux MKV→MP4) vers
/// AppData/StreamOS/medias/<uuid>.<ext>. Retourne `Some(rel)` si importé,
/// `None` si dialog annulé. Même mécanique que `import_alerte_media`.
#[tauri::command]
fn pad_importer_media(
    app: AppHandle,
    kind_attendu: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (filtre_nom, exts): (&str, &[&str]) = if kind_attendu == "video" {
        ("Vidéos", VIDEO_EXT)
    } else {
        ("Images", IMG_EXT)
    };
    let file = app
        .dialog()
        .file()
        .add_filter(filtre_nom, exts)
        .blocking_pick_file();
    let Some(file) = file else {
        return Ok(None);
    };
    let src: std::path::PathBuf = file
        .into_path()
        .map_err(|e| format!("Chemin invalide: {}", e))?;

    let (rel, kind, _nom) = validate_and_copy_media(&app, &src)?;
    if kind != kind_attendu {
        return Err(format!(
            "Type de fichier inattendu : {} attendu, {} reçu",
            kind_attendu, kind
        ));
    }
    Ok(Some(rel))
}

/// Importe un son (mp3/ogg/wav ≤ 10 Mo) pour une touche du pad : dialog
/// « Sons » → validation + copie vers medias/. Retourne `Some(rel)` ou
/// `None` si dialog annulé. Réutilise `import_son` (même mécanique).
/// NB : on expose une commande dédiée pour la clarté de l'API frontend,
/// mais elle délègue à la logique commune de `import_son`.
#[tauri::command]
fn pad_importer_son(app: AppHandle) -> Result<Option<String>, String> {
    // Délègue à import_son (même logique de validation + copie).
    import_son(app)
}

// ===== Alertes (follow/raid/sub/resub/subgift/bits) =====

/// Config complète des alertes (pour l'UI dashboard).
#[tauri::command]
fn alertes_etat(state: tauri::State<'_, alertes::AlertesState>) -> alertes::AlertesConfig {
    state.etat()
}

/// Remplace la config d'un type d'alerte + persiste + push WS diffusion.
#[tauri::command]
fn alertes_set_config(
    state: tauri::State<'_, alertes::AlertesState>,
    type_alerte: String,
    config: alertes::AlerteTypeConfig,
) -> Result<(), String> {
    state.set_config_type(&type_alerte, config)
}

/// Test manuel : déclenche une alerte avec données factices (bypass cooldowns).
#[tauri::command]
fn alertes_tester(
    state: tauri::State<'_, alertes::AlertesState>,
    type_alerte: String,
) -> Result<(), String> {
    state.tester(&type_alerte)
}

/// Remplace la position/taille de l'overlay alerte (même mécanique que
/// welcome_set_overlay_config) + persiste + push WS diffusion.
#[tauri::command]
fn alertes_set_overlay_config(
    state: tauri::State<'_, alertes::AlertesState>,
    x: f64,
    y: f64,
    largeur: f64,
    hauteur: f64,
) -> Result<(), String> {
    state.set_overlay_config(x, y, largeur, hauteur)
}

/// Retourne la config du squelette de position unifié (source de vérité
/// partagée par les overlays "clip de bienvenue" et "alertes").
#[tauri::command]
fn position_overlay_etat(
    state: tauri::State<'_, position_overlay::PositionOverlayState>,
) -> position_overlay::PositionOverlayConfig {
    state.etat()
}

/// Remplace la config du squelette de position unifié + persiste
/// (position_overlay.json) + push WS `position-overlay-config` vers la
/// diffusion (applique aux deux overlays en live).
#[tauri::command]
fn position_overlay_set(
    state: tauri::State<'_, position_overlay::PositionOverlayState>,
    config: position_overlay::PositionOverlayConfig,
) -> Result<(), String> {
    state.set(config)
}

/// Déclenche une alerte depuis le frontend. Utilisé par la diff des follows
/// (pas d'event IRC pour les follows) — et c'est le point d'extension prévu
/// pour les futures plateformes (Kick/YouTube/TikTok appelleront la même
/// commande avec leurs propres événements).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn alerte_declencher(
    state: tauri::State<'_, alertes::AlertesState>,
    type_alerte: String,
    pseudo: String,
    user_id: String,
    nb_viewers: i64,
    nb_bits: i64,
    nb_mois: i64,
    destinataire: String,
) -> Result<(), String> {
    if !alertes::TYPES_ALERTE.contains(&type_alerte.as_str()) {
        return Err(format!("Type d'alerte inconnu : {}", type_alerte));
    }
    state.on_event(alertes::EventTwitch {
        type_alerte,
        login: pseudo.to_lowercase(),
        pseudo,
        user_id,
        nb_viewers,
        nb_bits,
        nb_mois,
        destinataire,
        niveau_sub: String::new(),
        system_msg: String::new(),
    });
    Ok(())
}

// ===== Énumération PC + création capture depuis SOS (Lot 3) =====

/// Énumère les caméras du PC (PnP/DirectShow, SANS OBS).
#[tauri::command]
fn pc_enumerate_cameras() -> Vec<String> {
    pc_enum::enumerate_cameras()
}

/// Énumère les fenêtres visibles du PC (EnumWindows, SANS OBS).
#[tauri::command]
fn pc_enumerate_windows() -> Vec<pc_enum::WindowEntry> {
    pc_enum::enumerate_windows()
}

/// Énumère les jeux/fenêtres du PC (EnumWindows, SANS OBS).
#[tauri::command]
fn pc_enumerate_games() -> Vec<pc_enum::WindowEntry> {
    pc_enum::enumerate_games()
}

/// Crée une source OBS de capture depuis une cible PC (caméra/fenêtre/jeu)
/// dans la scène contenant SOS-Diffusion. Si SOS-Trou-<id> existe déjà →
/// SetInputSettings + transform (pas de doublon). Sinon → CreateInput.
/// Puis SetSceneItemIndex sous SOS + transform = widget.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn obs_create_trou_from_pc(
    host: String,
    port: u16,
    password: String,
    source_name: String,
    kind: obs_trou::CaptureKind,
    target: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    eprintln!("[OBS] COMMANDE APPELÉE kind={:?} source=\"{}\" target={:?} x={} y={} w={} h={}",
        kind, source_name, target, x, y, w, h);
    let res = obs_trou::create_trou_from_pc(
        &host, port, &password, &source_name, kind, target, x, y, w, h,
    )
    .await;
    match &res {
        Ok(s) => eprintln!("[OBS] COMMANDE OK source=\"{}\"", s),
        Err(e) => eprintln!("[OBS] COMMANDE ERR=\"{}\"", e),
    }
    res
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        // Sidecar ffmpeg : remux MKV→MP4 à l'import média (lecture navigateur).
        .plugin(tauri_plugin_shell::init())
        // Pop-out chat : custom protocol servant le HTML embarqué (include_str!).
        // Toute URL streamos-chat://localhost/* → CHAT_POPOUT_HTML. Vraie origine
        // (http://streamos-chat.localhost sur Windows) → invoke + WS fiables.
        .register_uri_scheme_protocol("streamos-chat", |_ctx, _request| {
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(CHAT_POPOUT_HTML.as_bytes().to_vec())
                .unwrap()
        })
        .setup(|app| {
            let handle = app.handle().clone();

            // Boot scènes : migration + chargement initial (UNE scène en RAM).
            let (scene, current_id) = scenes::boot_scenes(&handle).unwrap_or_else(|e| {
                log::error!("Erreur boot_scenes: {}", e);
                (scene::Scene::new(), "defaut".to_string())
            });

            let scene = Arc::new(std::sync::Mutex::new(scene));
            let current_id = Arc::new(std::sync::Mutex::new(current_id));

            // Canal broadcast pour les snapshots WS
            let (snapshot_tx, _snapshot_rx) = broadcast::channel::<String>(64);
            // Canal broadcast pour les messages chat (IRC → diffusion :4321)
            let (chat_tx, _chat_rx) = broadcast::channel::<String>(256);

            let state = ScenesState {
                scene: scene.clone(),
                current_id: current_id.clone(),
                snapshot_tx: snapshot_tx.clone(),
                chat_tx: chat_tx.clone(),
            };
            app.manage(state.clone());

            // État Squelette de position unifié (source de vérité unique pour
            // la position/taille des overlays "clip de bienvenue" et "alertes").
            // Doit être enregistré AVANT welcome_state et alertes_state car ils
            // y accèdent via app.state::<PositionOverlayState>() au boot.
            let position_state =
                position_overlay::PositionOverlayState::new(handle.clone(), chat_tx.clone());
            app.manage(position_state.clone());

            // État Welcome (clips de bienvenue : registre + seen + queue).
            // Récupère chat_tx depuis ScenesState pour émettre vers :4321.
            let welcome_state = welcome::WelcomeState::new(handle.clone(), chat_tx.clone());
            app.manage(welcome_state.clone());

            // État Bandeau premier message (détection 1er message tous viewers).
            // Récupère chat_tx pour émettre vers diffusion :4321.
            let bandeau_state = bandeau::BandeauState::new(handle.clone(), chat_tx.clone());
            app.manage(bandeau_state.clone());

            // État Commandes chat ("!commande" tapée par un viewer → overlay
            // diffusion, même moteur que les alertes : file + cooldowns + timer).
            let commandes_state = commandes::CommandesState::new(handle.clone(), chat_tx.clone());
            app.manage(commandes_state.clone());

            // État Input Viewer (capture globale clavier + souris → overlay
            // diffusion). Inactif au boot — démarré par le bouton ON/OFF des
            // options du widget. Émission WS directe (input-viewer-etat).
            let input_viewer_state =
                input_viewer::InputViewerState::new(chat_tx.clone());
            app.manage(input_viewer_state.clone());

            // État Pad numérique (16 touches Numpad × 3 plages → overlay
            // diffusion). Capture clavier globale dédiée (GetAsyncKeyState).
            // Démarré au boot si la config était active au dernier arrêt.
            let pad_state =
                pad_numerique::PadNumeriqueState::new(handle.clone(), chat_tx.clone());
            app.manage(pad_state.clone());

            // Speedrun Splitter — initialise le canal WS pour la diffusion.
            // Le moteur émet l'état du timer via chat_tx (speedrun-etat) vers
            // diffusion.html (maj DOM directe, pattern input-viewer).
            speedrun::commands::set_chat_tx(chat_tx.clone());

            // État Alertes (follow/raid/sub/resub/subgift/bits → overlay diffusion).
            // Récupère chat_tx pour émettre vers diffusion :4321. Les événements
            // arrivent de twitch_chat.rs (USERNOTICE + bits) et de la commande
            // alerte_declencher (diff follows frontend + futures plateformes).
            let alertes_state = alertes::AlertesState::new(handle.clone(), chat_tx.clone());
            app.manage(alertes_state.clone());

            // État Twitch (handle IRC + cancel + drapeau connecté).
            let twitch_state = TwitchState::new();
            app.manage(twitch_state.clone());

            // État Kick (drapeau connecté + slug canal). Le WS est côté frontend (LOT 2).
            let kick_state = KickState::new();
            app.manage(kick_state.clone());

            // État YouTube (handle chat polling + cancel + drapeau connecté + login).
            let youtube_state = YoutubeState::new();
            app.manage(youtube_state.clone());

            // État TikTok (handle chat + cancel + drapeau connecté + username).
            let tiktok_state = TiktokState::new();
            app.manage(tiktok_state.clone());

            // Compteur viewers — UNE tâche pour toutes les plateformes
            // (tick 15s, fetch 60s live / 180s off, dedup, push via chat_tx).
            viewers::demarrer(handle.clone(), chat_tx.clone());

            // Auto-resume : si token valide en coffre → IRC + point vert.
            // Pas de modal. Si token absent/invalide → reste déconnecté.
            let resume_app = handle.clone();
            let resume_state = twitch_state.clone();
            tauri::async_runtime::spawn(async move {
                match twitch_auth::lire_tokens() {
                    Ok(Some(tokens)) => {
                        eprintln!("[Twitch] auto-resume: token trouvé en coffre...");
                        if let Err(e) =
                            connect_with_tokens(&resume_app, &resume_state, tokens).await
                        {
                            eprintln!("[Twitch] ERR auto-resume échoué: {}", e);
                        }
                    }
                    Ok(None) => {
                        eprintln!("[Twitch] aucun token en coffre (déconnecté)");
                    }
                    Err(e) => {
                        eprintln!("[Twitch] ERR lecture coffre: {}", e);
                    }
                }
            });

            // Auto-resume Kick : si slug sauvegardé → reconnecter automatiquement.
            // Non-fatal si échec (slug invalide, réseau) — l'utilisateur reconnectera.
            let kick_app = handle.clone();
            let kick_state_clone = kick_state.clone();
            tauri::async_runtime::spawn(async move {
                match crate::config::lire_kick_slug(&kick_app) {
                    Ok(Some(slug)) => {
                        eprintln!("[Kick] auto-resume: slug trouvé ({})", slug);
                        match kick::resolve_chatroom(&slug).await {
                            Ok(chatroom_id) => {
                                kick_state_clone.reset_cancel();
                                let cancel = kick_state_clone.cancel_clone();
                                let handle = kick::run_ws(kick_app.clone(), chatroom_id, cancel);
                                {
                                    let mut h = kick_state_clone.ws_handle.lock().unwrap();
                                    *h = Some(handle);
                                }
                                kick_state_clone.set_slug(Some(slug));
                                // set_connected(true) se fait dans run_ws quand subscribed.
                            }
                            Err(e) => {
                                eprintln!("[Kick] ERR auto-resume resolve: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        eprintln!("[Kick] aucun slug sauvegardé (déconnecté)");
                    }
                    Err(e) => {
                        eprintln!("[Kick] ERR lecture slug: {}", e);
                    }
                }
            });

            // Auto-resume YouTube : si token valide en coffre → valider + chat polling.
            // Non-fatal si échec (token expiré, réseau) — l'utilisateur reconnectera.
            let yt_app = handle.clone();
            let yt_state = youtube_state.clone();
            tauri::async_runtime::spawn(async move {
                match youtube_auth::lire_tokens() {
                    Ok(Some(tokens)) => {
                        eprintln!("[YouTube] auto-resume: token trouvé en coffre...");
                        if let Err(e) =
                            connect_with_tokens_youtube(&yt_app, &yt_state, tokens).await
                        {
                            eprintln!("[YouTube] ERR auto-resume échoué: {}", e);
                        }
                    }
                    Ok(None) => {
                        eprintln!("[YouTube] aucun token en coffre (déconnecté)");
                    }
                    Err(e) => {
                        eprintln!("[YouTube] ERR lecture coffre: {}", e);
                    }
                }
            });

            // Auto-resume TikTok : si username sauvegardé → reconnecter le chat.
            // Non-fatal si échec (streamer hors-ligne, réseau) — l'utilisateur reconnectera.
            let tt_app = handle.clone();
            let tt_state = tiktok_state.clone();
            tauri::async_runtime::spawn(async move {
                match crate::config::lire_tiktok_username(&tt_app) {
                    Ok(Some(username)) => {
                        eprintln!("[TikTok] auto-resume: username trouvé ({})", username);
                        let chat_tx = tt_app
                            .try_state::<ScenesState>()
                            .map(|s| s.chat_tx.clone());
                        if let Some(chat_tx) = chat_tx {
                            tt_state.stop_chat();
                            tt_state.reset_cancel();
                            let cancel = tt_state.cancel_clone();
                            let handle = tiktok_chat::demarrer(
                                tt_app.clone(),
                                chat_tx,
                                username.clone(),
                                cancel,
                            );
                            {
                                let mut h = tt_state.chat_handle.lock().unwrap();
                                *h = Some(handle);
                            }
                            tt_state.set_username(Some(username));
                        } else {
                            eprintln!("[TikTok] ERR auto-resume: ScenesState non trouvé");
                        }
                    }
                    Ok(None) => {
                        eprintln!("[TikTok] aucun username sauvegardé (déconnecté)");
                    }
                    Err(e) => {
                        eprintln!("[TikTok] ERR lecture username: {}", e);
                    }
                }
            });

            // Démarre le serveur :4321 en arrière-plan.
            // run_server émet server_ready (bind OK) ou server_error (port pris)
            // directement — pas de probe externe, pas de fallback port.
            let server_handle = handle.clone();
            let server_state = state.clone();
            tauri::async_runtime::spawn(async move {
                match server::run_server(server_handle, server_state).await {
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
        .invoke_handler(tauri::generate_handler![
            get_scene,
            update_scene,
            import_media,
            import_fond,
            import_son,
            import_alerte_media,
            obs_connect,
            obs_refresh_diffusion,
            chat_popout_toggle,
            chat_popout_fermer,
            app_arreter,
            app_redemarrer,
            scene_sync_captures,
            scenes_lister,
            scene_creer,
            scene_ouvrir,
            scene_renommer,
            scene_deplacer,
            scene_masquer_nom,
            scene_supprimer,
            scene_courante,
            scene_exporter,
            scene_importer,
            obs_enumerate_targets,
            obs_create_trou_source,
            obs_sync_trous,
            obs_delete_trou_source,
            obs_enumerate_inputs_by_kind,
            obs_link_existing_source,
            camera_sync,
            camera_hide,
            pc_enumerate_cameras,
            pc_enumerate_windows,
            pc_enumerate_games,
            obs_create_trou_from_pc,
            twitch_connecter,
            twitch_annuler_device_flow,
            twitch_deconnecter,
            twitch_etat,
            twitch_login_courant,
            twitch_reconnecter,
            twitch_communaute_followers,
            twitch_followers_light,
            twitch_communaute_subs,
            twitch_communaute_viewers,
            twitch_communaute_unfollows,
            twitch_broadcaster,
            twitch_lister_vips,
            twitch_lister_moderateurs,
            twitch_lister_bannis,
            twitch_resoudre_user,
            twitch_bannir,
            twitch_debannir,
            twitch_ajouter_vip,
            twitch_retirer_vip,
            twitch_ajouter_moderateur,
            twitch_retirer_moderateur,
            twitch_supprimer_message,
            twitch_envoyer_commande_chat,
            kick_connecter,
            kick_deconnecter,
            kick_etat,
            kick_slug_courant,
            youtube_connecter,
            youtube_deconnecter,
            youtube_etat,
            youtube_login_courant,
            youtube_annuler_device_flow,
            youtube_demarrer_chat,
            youtube_arreter_chat,
            youtube_communaute_channel,
            youtube_communaute_members,
            youtube_communaute_viewers,
            tiktok_connecter,
            tiktok_deconnecter,
            tiktok_etat,
            tiktok_username_courant,
            welcome_etat,
            welcome_registre_twitch,
            welcome_sauver_viewer_twitch,
            welcome_supprimer_viewer_twitch,
            welcome_config_globale_actif,
            welcome_set_overlay_config,
            welcome_tester_clip,
            welcome_set_duree_affichage,
            welcome_stop,
            welcome_skip,
            welcome_retirer,
            welcome_remonter,
            welcome_descendre,
            welcome_vider,
            welcome_reset_session,
            welcome_lister_clips_streamer,
            welcome_resoudre_mp4,
            welcome_attribuer_auto,
            bandeau_etat,
            bandeau_set_config,
            bandeau_stop,
            bandeau_reset_session,
            bandeau_tester,
            commandes_etat,
            commande_set_config,
            commande_supprimer,
            commande_tester,
            input_viewer_set_actif,
            input_viewer_etat_actif,
            pad_etat,
            pad_set_actif,
            pad_set_touche,
            pad_supprimer_touche,
            pad_set_plage,
            pad_tester_touche,
            pad_importer_media,
            pad_importer_son,
            alertes_etat,
            alertes_set_config,
            alertes_set_overlay_config,
            alertes_tester,
            alerte_declencher,
            position_overlay_etat,
            position_overlay_set,
            speedrun_charger_asl,
            speedrun_charger_lss,
            speedrun_demarrer,
            speedrun_arreter,
            speedrun_maj_settings,
            speedrun_est_actif,
            speedrun_action_manuelle,
            speedrun_lire_config,
            speedrun_sauver_config,
            speedrun_maj_settings_asl
        ])
        .run(tauri::generate_context!())
        .expect("erreur lors du lancement de StreamOS v0");
}
