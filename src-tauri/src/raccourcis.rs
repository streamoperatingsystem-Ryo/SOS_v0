//! Barre de raccourcis : fenêtre TOPMOST dockée à un bord d'écran Windows.
//!
//! Fenêtre WebviewWindow dédiée (label "raccourcis") : sans décorations, non
//! redimensionnable, hors barre des tâches, non focusable (ne vole pas le focus
//! au jeu — les clics souris restent reçus), transparente (bande noire
//! translucide rendue par le HTML : rgba(0,0,0,0.7)).
//!
//! La barre n'a AUCUNE logique métier : les clics icônes émettent
//! "raccourcis:action" {id} → le dashboard (App.svelte) dispatche vers les
//! fonctions existantes des stores. Le bord courant est passé dans l'URL de
//! création (?bord=) puis poussé via emit_to("raccourcis:bord") à chaque
//! ré-application — le HTML adapte sa disposition (colonne G/D, rangée H/B).
//!
//! Limite connue : TOPMOST ≠ plein écran EXCLUSIF (DirectX flip exclusif
//! recouvre tout — accepté, pas d'overlay injecté). Cible = jeux en fenêtré
//! sans bordure + bureau. Un timer léger (2s) refait SetWindowPos(HWND_TOPMOST)
//! pour repasser devant les fenêtres topmost concurrentes (jeu borderless
//! topmost). AUCUN hook, AUCUNE injection — Win32 z-order public uniquement.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Label de la fenêtre barre de raccourcis.
pub const LABEL: &str = "raccourcis";

/// Épaisseur de la bande en pixels logiques (×scale_factor = physique).
const EPAISSEUR: f64 = 56.0;

/// HTML embarqué de la barre (include_str! — recompile si le fichier change).
pub const RACCOURCIS_HTML: &str = include_str!("../resources/raccourcis.html");

/// Info moniteur exposée au frontend (liste de la section Raccourcis).
#[derive(Serialize)]
pub struct RaccourcisMoniteur {
    pub index: usize,
    pub nom: Option<String>,
    pub largeur: u32,
    pub hauteur: u32,
    pub primaire: bool,
}

/// État exposé au frontend : config persistée + visibilité réelle.
#[derive(Serialize)]
pub struct RaccourcisEtat {
    /// La fenêtre existe (barre affichée).
    pub visible: bool,
    pub bord: String,
    pub monitor_index: usize,
}

/// Liste les moniteurs Windows pour le sélecteur de la Toolbar.
#[tauri::command]
pub fn raccourcis_moniteurs(app: AppHandle) -> Result<Vec<RaccourcisMoniteur>, String> {
    let moniteurs = app
        .available_monitors()
        .map_err(|e| format!("Enum moniteurs: {}", e))?;
    let primaire = app.primary_monitor().ok().flatten();
    Ok(moniteurs
        .iter()
        .enumerate()
        .map(|(i, m)| RaccourcisMoniteur {
            index: i,
            nom: m.name().cloned(),
            largeur: m.size().width,
            hauteur: m.size().height,
            primaire: primaire
                .as_ref()
                .map(|p| p.position() == m.position() && p.size() == m.size())
                .unwrap_or(false),
        })
        .collect())
}

/// État courant de la barre (config persistée + fenêtre réellement ouverte).
#[tauri::command]
pub fn raccourcis_etat(app: AppHandle) -> Result<RaccourcisEtat, String> {
    let cfg = crate::config::lire_raccourcis_config(&app)?;
    let (bord, monitor_index) = cfg
        .map(|c| (c.bord, c.monitor_index))
        .unwrap_or_else(|| ("droite".to_string(), 0));
    Ok(RaccourcisEtat {
        visible: app.get_webview_window(LABEL).is_some(),
        bord,
        monitor_index,
    })
}

/// Applique bord + moniteur : crée la fenêtre si absente, la positionne sur le
/// bord demandé, la montre et persiste la config. Async obligatoire : sur
/// Windows, créer une webview dans une commande sync deadlock (Webview2).
#[tauri::command]
pub async fn raccourcis_appliquer(
    app: AppHandle,
    bord: String,
    monitor_index: usize,
) -> Result<RaccourcisEtat, String> {
    let bord = bord_valide(&bord);
    creer_ou_appliquer(&app, bord, monitor_index).await?;
    crate::config::sauver_raccourcis_config(
        &app,
        &crate::config::RaccourcisConfig {
            visible: true,
            bord: bord.to_string(),
            monitor_index,
        },
    )?;
    Ok(RaccourcisEtat {
        visible: true,
        bord: bord.to_string(),
        monitor_index,
    })
}

/// Masque la barre (détruit la fenêtre — cohérent avec le pop-out chat, libère
/// le webview) + persiste visible:false. Non-fatal si déjà fermée.
#[tauri::command]
pub fn raccourcis_masquer(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
    }
    // Conserver bord/monitor_index persistés : on ne flippe que visible.
    if let Ok(Some(mut cfg)) = crate::config::lire_raccourcis_config(&app) {
        cfg.visible = false;
        let _ = crate::config::sauver_raccourcis_config(&app, &cfg);
    }
    Ok(())
}

/// Crée la fenêtre si absente puis applique la géométrie dockée au bord.
/// Partagé entre la commande `raccourcis_appliquer` et le restore au boot.
pub async fn creer_ou_appliquer(
    app: &AppHandle,
    bord: &str,
    monitor_index: usize,
) -> Result<(), String> {
    let bord = bord_valide(bord);
    let moniteurs = app
        .available_monitors()
        .map_err(|e| format!("Enum moniteurs: {}", e))?;
    if moniteurs.is_empty() {
        return Err("Aucun moniteur détecté".to_string());
    }
    // Index hors bornes (moniteur débranché depuis la sauvegarde) → primaire.
    let m = moniteurs.get(monitor_index).cloned().or_else(|| {
        app.primary_monitor()
            .ok()
            .flatten()
            .or_else(|| moniteurs.first().cloned())
    });
    let Some(m) = m else {
        return Err("Aucun moniteur détecté".to_string());
    };
    let (x, y, w, h) = geometrie(m.position(), m.size(), m.scale_factor(), bord);

    let win = match app.get_webview_window(LABEL) {
        Some(win) => {
            // Fenêtre existante : pousser le nouveau bord (le HTML bascule
            // colonne ↔ rangée) puis repositionner.
            let _ = app.emit_to(LABEL, "raccourcis:bord", bord);
            win
        }
        None => creer_fenetre(app, bord)?,
    };

    win.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(w, h)))
        .map_err(|e| format!("set_size raccourcis: {}", e))?;
    win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x, y)))
        .map_err(|e| format!("set_position raccourcis: {}", e))?;
    win.show()
        .map_err(|e| format!("show raccourcis: {}", e))?;
    Ok(())
}

/// Crée la fenêtre barre (invisible → positionnée puis show() par l'appelant).
/// Bord initial passé dans l'URL (?bord=) : le HTML le lit au chargement, avant
/// tout event. Le handler CloseRequested émet "raccourcis-closed" (garde la
/// Toolbar honnête si la fenêtre est fermée par une voie externe).
fn creer_fenetre(app: &AppHandle, bord: &str) -> Result<tauri::WebviewWindow, String> {
    use tauri::webview::WebviewWindowBuilder;

    let url = tauri::Url::parse(&format!(
        "streamos-raccourcis://localhost/bar?bord={}",
        bord
    ))
    .map_err(|e| format!("URL invalide: {}", e))?;
    let win = WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::CustomProtocol(url))
        .title("StreamOS — Raccourcis")
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // Ne vole pas le focus au jeu (WS_EX_NOACTIVATE). Les clics souris
        // restent reçus — requis pour les icônes.
        .focusable(false)
        // Fond transparent : la bande noire translucide est rendue en CSS
        // (rgba(0,0,0,0.7)) — sans ça le webview composite sur fond opaque.
        .transparent(true)
        .shadow(false)
        .visible(false)
        .build()
        .map_err(|e| format!("Création fenêtre raccourcis: {}", e))?;

    let app_clone = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            let _ = app_clone.emit("raccourcis-closed", ());
        }
    });

    demarrer_topmost_timer(app.clone());
    Ok(win)
}

/// Timer léger (2s) : remet la barre en tête de la bande topmost.
/// always_on_top pose WS_EX_TOPMOST une fois ; un jeu borderless lui aussi
/// topmost peut passer devant — SetWindowPos(HWND_TOPMOST) nous remet dessus.
/// La tâche vit tant que la fenêtre existe ; s'arrête à sa destruction.
fn demarrer_topmost_timer(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            interval.tick().await;
            let Some(win) = app.get_webview_window(LABEL) else {
                break;
            };
            if !win.is_visible().unwrap_or(false) {
                continue;
            }
            #[cfg(windows)]
            if let Ok(hwnd) = win.hwnd() {
                remonter_topmost(windows::Win32::Foundation::HWND(hwnd.0));
            }
        }
    });
}

/// SetWindowPos(HWND_TOPMOST) sans toucher position/taille ni activer la fenêtre.
#[cfg(windows)]
fn remonter_topmost(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Normalise un bord venant du frontend/disque : valeurs connues, sinon "droite".
fn bord_valide(bord: &str) -> &str {
    match bord {
        "gauche" | "haut" | "bas" => bord,
        _ => "droite",
    }
}

/// Géométrie physique (x, y, w, h) de la bande dockée au bord du moniteur.
/// Rect moniteur COMPLET (pas work_area) : collée au bord de l'écran, la barre
/// topmost recouvre la taskbar si elle est sur ce bord — choix volontaire.
fn geometrie(
    pos: &tauri::PhysicalPosition<i32>,
    size: &tauri::PhysicalSize<u32>,
    scale: f64,
    bord: &str,
) -> (i32, i32, u32, u32) {
    let t = (EPAISSEUR * scale).round().max(1.0) as u32;
    match bord {
        "gauche" => (pos.x, pos.y, t, size.height),
        "haut" => (pos.x, pos.y, size.width, t),
        "bas" => (
            pos.x,
            pos.y + size.height as i32 - t as i32,
            size.width,
            t,
        ),
        // droite (défaut)
        _ => (
            pos.x + size.width as i32 - t as i32,
            pos.y,
            t,
            size.height,
        ),
    }
}
