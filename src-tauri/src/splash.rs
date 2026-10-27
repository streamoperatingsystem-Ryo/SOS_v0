//! Écran de démarrage (splash) façon Discord : petite fenêtre centrée,
//! sans décorations, topmost, affichée IMMÉDIATEMENT au boot pendant que
//! le dashboard charge en arrière-plan.
//!
//! Servi par le protocole custom `streamos-splash://` (même pattern que
//! `streamos-raccourcis`) — routage par path dans `repondre` :
//!   `/`          → HTML embarqué (placeholders __USER__ / __GREETING__)
//!   `/wallpaper` → octets du fond d'écran Windows de l'utilisateur
//!   `/mascotte`  → PNG Streamy embarqué
//!
//! Wallpaper : `SystemParametersInfoW(SPI_GETDESKWALLPAPER)` (Win10/Win11,
//! image fixe ET diaporama/Spotlight → pointe alors vers
//! TranscodedWallpaper), repli sur le fichier TranscodedWallpaper
//! (%APPDATA%\Microsoft\Windows\Themes\). Tout échec → 404 → le dégradé
//! CSS du splash reste — jamais bloquant, jamais fatal.
//!
//! Transition : la fenêtre `main` (visible:false dans tauri.conf.json) est
//! montrée quand les DEUX conditions sont remplies — splash affiché depuis
//! DUREE_SPLASH (4 s, consigne) ET event "frontend:pret" reçu (fin du
//! onMount d'App.svelte). Garde-fou : si "frontend:pret" n'arrive jamais,
//! TIMEOUT_FRONT après la durée minimale → main montrée quand même —
//! jamais de fenêtre invisible définitive.
//!
//! DEV = PROD (2026-10-26, consigne) : même petite fenêtre éphémère,
//! fermée après le même timer — le splash ne reste jamais ouvert.

use std::sync::Arc;
use tauri::{AppHandle, Listener, Manager};
use tokio::sync::Notify;

/// Label de la fenêtre splash.
pub const LABEL: &str = "splash";

/// Durée minimale d'affichage du splash (consigne : 4 s à l'écran).
const DUREE_SPLASH: std::time::Duration = std::time::Duration::from_secs(4);

/// Attente max de "frontend:pret" APRÈS la durée minimale — si le frontend
/// est mort, la fenêtre main est montrée quand même (splash total ≤ 12 s).
const TIMEOUT_FRONT: std::time::Duration = std::time::Duration::from_secs(8);

/// HTML embarqué du splash (include_str! — recompile si le fichier change).
const SPLASH_HTML: &str = include_str!("../resources/splash.html");

/// Mascotte Streamy « welcome » (fond transparent) servie sur /mascotte.
const MASCOTTE_PNG: &[u8] = include_bytes!("../../src/assets/streamy/streamy-welcome.png");

/// Crée et montre la fenêtre splash. Appelé en PREMIER dans setup() —
/// elle apparaît pendant que les états/serveur/auto-resume s'initialisent.
/// Non-fatal : échec de création → on continue, le boot reste utilisable
/// (armer_affichage montrera main comme si le splash n'existait pas).
///
/// Un seul visage, dev et prod : petite fenêtre topmost éphémère façon
/// Discord, fermée par armer_affichage après le même timer (2026-10-26).
pub fn demarrer(app: &AppHandle) {
    use tauri::webview::WebviewWindowBuilder;

    let url = match tauri::Url::parse("streamos-splash://localhost/") {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[Splash] ERR url: {}", e);
            return;
        }
    };
    let res = WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::CustomProtocol(url))
        .title("StreamOS")
        .inner_size(440.0, 280.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .build();
    if let Err(e) = res {
        eprintln!("[Splash] ERR création fenêtre: {}", e);
    }
}

/// Arme la transition splash → fenêtre principale. Appelé dans setup()
/// après `fenetre::appliquer_au_boot` (qui positionne main, encore
/// visible:false). La fenêtre main est montrée à max(DUREE_SPLASH,
/// réception de "frontend:pret") puis le splash est fermé.
/// `Notify` conserve un permis : un event reçu pendant le sleep est
/// consommé instantanément après — pas de course.
pub fn armer_affichage(app: &AppHandle) {
    let pret = Arc::new(Notify::new());
    let pret_listener = pret.clone();
    app.listen("frontend:pret", move |_| {
        pret_listener.notify_one();
    });

    let app_task = app.clone();
    tauri::async_runtime::spawn(async move {
        // Le splash reste au moins DUREE_SPLASH à l'écran.
        tokio::time::sleep(DUREE_SPLASH).await;
        // Puis on attend que le dashboard soit réellement monté — sinon on
        // réintroduirait le flash de webview blanche. Timeout = garde-fou
        // (frontend mort → on montre quand même).
        let _ = tokio::time::timeout(TIMEOUT_FRONT, pret.notified()).await;
        if let Some(main) = app_task.get_webview_window(crate::fenetre::LABEL) {
            let _ = main.show();
            let _ = main.set_focus();
        }
        if let Some(splash) = app_task.get_webview_window(LABEL) {
            // Dev comme prod : le splash se ferme après le timer (consigne
            // 2026-10-26 — plus de fenêtre splash persistante en dev).
            let _ = splash.close();
        }
    });
}

/// Réponse du protocole `streamos-splash://` — routage par path. Appelé
/// par la closure `register_uri_scheme_protocol` de lib.rs (le type
/// UriSchemeContext n'a pas besoin d'être nommé ici).
pub fn repondre(app: &AppHandle, path: &str) -> tauri::http::Response<Vec<u8>> {
    match path {
        "/wallpaper" => servir_wallpaper(),
        "/mascotte" => tauri::http::Response::builder()
            .header("Content-Type", "image/png")
            .body(MASCOTTE_PNG.to_vec())
            .unwrap(),
        // "/" et tout le reste → la page splash.
        _ => servir_html(app),
    }
}

/// Route "/" : le HTML embarqué avec pseudo Windows et salutation traduite
/// injectés dans les placeholders __USER__ / __GREETING__.
fn servir_html(app: &AppHandle) -> tauri::http::Response<Vec<u8>> {
    let html = SPLASH_HTML
        .replace("__USER__", &pseudo_html())
        .replace("__GREETING__", salutation(app));
    tauri::http::Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .body(html.into_bytes())
        .unwrap()
}

/// Route /wallpaper : lit le fichier image du fond d'écran courant.
/// 404 si indisponible (couleur unie, fichier absent ou illisible) → le
/// dégradé CSS reste affiché, non bloquant par design.
fn servir_wallpaper() -> tauri::http::Response<Vec<u8>> {
    let introuvable = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap()
    };
    let Some(path) = chemin_wallpaper() else {
        return introuvable();
    };
    match std::fs::read(&path) {
        Ok(bytes) if !bytes.is_empty() => tauri::http::Response::builder()
            .header("Content-Type", content_type_image(&bytes))
            .body(bytes)
            .unwrap(),
        _ => introuvable(),
    }
}

/// Chemin du fond d'écran Windows courant.
/// Source 1 : `SystemParametersInfoW(SPI_GETDESKWALLPAPER)` — chemin
/// canonique (Win10/Win11 ; image fixe, diaporama et Spotlight renvoient
/// tous un chemin fichier). Chaîne vide = couleur unie → repli.
/// Source 2 : TranscodedWallpaper (%APPDATA%\Microsoft\Windows\Themes\) —
/// JPEG sans extension maintenu par Windows quand un thème/image est actif.
#[cfg(windows)]
fn chemin_wallpaper() -> Option<std::path::PathBuf> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    // MAX_PATH suffit : le wallpaper vit dans Themes/ ou un dossier user.
    let mut buf = [0u16; 260];
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            0,
            Some(buf.as_mut_ptr() as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
    };
    if ok {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let p = String::from_utf16_lossy(&buf[..len]);
        if !p.is_empty() {
            return Some(std::path::PathBuf::from(p));
        }
    }
    // Repli : TranscodedWallpaper.
    if let Ok(appdata) = std::env::var("APPDATA") {
        let t = std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Themes")
            .join("TranscodedWallpaper");
        if t.is_file() {
            return Some(t);
        }
    }
    None
}

/// Hors Windows : pas de source wallpaper → 404 → dégradé CSS.
#[cfg(not(windows))]
fn chemin_wallpaper() -> Option<std::path::PathBuf> {
    None
}

/// Content-Type par magic bytes (TranscodedWallpaper n'a pas d'extension).
fn content_type_image(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        "image/gif"
    } else if bytes.starts_with(&[0x42, 0x4D]) {
        "image/bmp"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Pseudo Windows (env USERNAME) échappé HTML — injecté dans le splash.
/// « Streamer » en repli si la variable est absente.
fn pseudo_html() -> String {
    let raw = std::env::var("USERNAME").unwrap_or_else(|_| "Streamer".to_string());
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Salutation selon la langue persistée (langue.json via lire_langue) —
/// mini-map des 8 langues de l'app. Repli "fr" (défaut de initLangue).
fn salutation(app: &AppHandle) -> &'static str {
    match crate::config::lire_langue(app).as_deref().unwrap_or("fr") {
        "en" => "Welcome",
        "de" => "Willkommen",
        "es" => "Bienvenido",
        "it" => "Benvenuto",
        "ru" => "Добро пожаловать",
        "zh" => "欢迎",
        "ja" => "ようこそ",
        _ => "Bienvenue",
    }
}
