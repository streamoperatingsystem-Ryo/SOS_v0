mod api_deck;
mod config;
mod obs;
mod obs_trou;
mod pc_enum;
mod scene;
mod scenes;
mod server;
mod twitch_auth;
mod twitch_chat;
mod twitch_helix;

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
    state: tauri::State<ScenesState>,
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

    // 3. Save <current_id>.json + push snapshot (chaîne unique)
    scenes::save_current(&app, &state)?;

    Ok(Some(rel))
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
    let Some((rel, kind)) = pick_and_copy_media(&app)? else {
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
            eprintln!("[OBS] obs_connect ERR {}", e);
            return Err(e);
        }
    };

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

// ===== Connexions réseau (Lot réseau) =====

use std::sync::atomic::Ordering;
use tauri::Emitter;
use twitch_auth::{Cancel, new_cancel};

/// État Twitch partagé : handle de la tâche IRC + jeton d'annulation (swappable)
/// + drapeau connecté + login/user_id/access token du compte connecté.
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
    /// Arrête l'IRC en cours (si actif) : flag cancel + abort handle.
    fn stop_irc(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.irc_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
    }
    /// Prépare un nouveau cancel (fresh) pour un prochain démarrage IRC/flow.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = new_cancel();
    }
    /// Récupère un clone du cancel courant.
    fn cancel_clone(&self) -> Cancel {
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
    if let Ok(Some(tokens)) = twitch_auth::lire_tokens() {
        return connect_with_tokens(&app, state.inner(), tokens).await;
    }

    // 2. Pas de token → Device Code Flow.
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
            tokens = twitch_auth::refresh_token(&tokens.refresh).await?;
            twitch_auth::sauver_tokens(&tokens)
                .map_err(|e| format!("coffre save: {}", e))?;
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
    let handle = twitch_chat::demarrer(
        app.clone(),
        chat_tx,
        tokens.access.clone(),
        tokens.login.clone(),
        cancel,
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
#[tauri::command]
async fn twitch_communaute_followers(
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::FollowersResp, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::followers(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Subs : liste + total + points (pagination, max 10 pages).
#[tauri::command]
async fn twitch_communaute_subs(
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::SubsResp, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::subs(&uid, &access)
        .await
        .map_err(helix_err_to_string)
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

            // État Twitch (handle IRC + cancel + drapeau connecté).
            let twitch_state = TwitchState::new();
            app.manage(twitch_state.clone());

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
            obs_connect,
            obs_refresh_diffusion,
            chat_popout_toggle,
            chat_popout_fermer,
            scene_sync_captures,
            scenes_lister,
            scene_creer,
            scene_ouvrir,
            scene_renommer,
            scene_courante,
            scene_exporter,
            scene_importer,
            obs_enumerate_targets,
            obs_create_trou_source,
            obs_sync_trous,
            obs_delete_trou_source,
            obs_enumerate_inputs_by_kind,
            obs_link_existing_source,
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
            twitch_communaute_subs,
            twitch_communaute_viewers,
            twitch_broadcaster
        ])
        .run(tauri::generate_context!())
        .expect("erreur lors du lancement de StreamOS v0");
}
