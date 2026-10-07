//! Fenêtre principale : écran de démarrage + mode d'affichage (section
//! « Ergonomie » de la Toolbar).
//!
//! Préférence persistée (fenetre.json : monitor_index + mode + dernière
//! géométrie fenêtrée) appliquée au boot dans setup() ET en direct via la
//! commande `fenetre_appliquer` — le changement saute immédiatement et reste
//! pour le prochain démarrage (même pattern que raccourcis_appliquer).
//!
//! Modes : "fenetre" (géométrie persistée restaurée, sinon centrage 1280×800
//! = taille tauri.conf.json) | "maximise" (défaut historique) | "fullscreen"
//! (borderless — pas de barre de titre). Une fenêtre maximisée ou fullscreen
//! ne se déplace pas par set_position : on normalise (set_fullscreen(false)
//! + unmaximize()) AVANT de la poser dans le moniteur cible — maximize() et
//!   set_fullscreen(true) s'appliquent ensuite sur l'écran qui la contient.

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// Label de la fenêtre principale (1re fenêtre de tauri.conf.json).
pub const LABEL: &str = "main";

/// Taille de repli en mode fenêtré sans géométrie persistée (logique,
/// = width/height de tauri.conf.json).
const DEF_W: f64 = 1280.0;
const DEF_H: f64 = 800.0;

/// État exposé au frontend : préférence persistée (config, pas l'état live —
/// même contrat que raccourcis_etat pour bord/monitor_index).
#[derive(Serialize)]
pub struct FenetreEtat {
    pub monitor_index: usize,
    pub mode: String,
}

/// État courant : config persistée (ou défauts historiques si absent).
#[tauri::command]
pub fn fenetre_etat(app: AppHandle) -> Result<FenetreEtat, String> {
    let cfg = crate::config::lire_fenetre_config(&app)?;
    let (monitor_index, mode) = cfg
        .map(|c| (c.monitor_index, c.mode))
        .unwrap_or_else(|| (0, "maximise".to_string()));
    Ok(FenetreEtat {
        monitor_index,
        mode,
    })
}

/// Applique écran + mode IMMÉDIATEMENT à la fenêtre main et persiste
/// fenetre.json (préférence de démarrage). La géométrie fenêtrée persistée
/// est rafraîchie : post-apply si le nouveau mode est "fenetre", pré-apply
/// si on quitte le mode fenêtré (la fenêtre bougeait depuis la dernière
/// sauvegarde), sinon la valeur existante est conservée.
#[tauri::command]
pub fn fenetre_appliquer(
    app: AppHandle,
    monitor_index: usize,
    mode: String,
) -> Result<FenetreEtat, String> {
    let mode = mode_valide(&mode).to_string();
    let geo_avant = capturer_geo(&app);
    let geo_persiste = crate::config::lire_fenetre_config(&app)?.and_then(|c| c.geo);
    appliquer(&app, monitor_index, &mode, geo_avant.clone().or(geo_persiste.clone()))?;
    let geo = capturer_geo(&app).or(geo_avant).or(geo_persiste);
    crate::config::sauver_fenetre_config(
        &app,
        &crate::config::FenetreConfig {
            monitor_index,
            mode: mode.clone(),
            geo,
        },
    )?;
    Ok(FenetreEtat { monitor_index, mode })
}

/// Appelé dans setup() : lit fenetre.json, applique écran + mode à la fenêtre
/// main (créée visible:false par tauri.conf.json — anti-flash). NE montre
/// PAS la fenêtre : c'est `splash::armer_affichage` qui décide du show()
/// (après 4 s de splash + event "frontend:pret" du frontend).
/// Enregistre aussi le hook CloseRequested → sauvegarde de la géométrie
/// fenêtrée à la fermeture par la croix.
pub fn appliquer_au_boot(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    match crate::config::lire_fenetre_config(app) {
        Ok(Some(cfg)) => {
            if let Err(e) = appliquer(app, cfg.monitor_index, &cfg.mode, cfg.geo) {
                eprintln!("[Fenetre] ERR apply boot: {}", e);
            }
        }
        Ok(None) => {}
        Err(e) => eprintln!("[Fenetre] ERR lecture config: {}", e),
    }
    // PAS de show() ici : l'affichage de main est chorégraphié par
    // splash::armer_affichage (4 s de splash + event "frontend:pret",
    // garde-fou si le frontend ne répond pas).
    // Sauver la géométrie fenêtrée quand la fenêtre se ferme par la croix.
    // capturer_geo ignore fullscreen/maximisé — pas de pollution de geo.
    // (app.exit()/restart() couverts par sauver_geo appelé dans app_arreter/
    // app_redemarrer, lib.rs.)
    let app_clone = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            sauver_geo(&app_clone);
        }
    });
}

/// Capture la géométrie fenêtrée courante et l'écrit dans fenetre.json
/// (conserve monitor_index/mode). No-op si la fenêtre n'est pas en mode
/// fenêtré — une taille maximisée/fullscreen n'est PAS à restaurer.
pub fn sauver_geo(app: &AppHandle) {
    let Some(geo) = capturer_geo(app) else {
        return;
    };
    let mut cfg = crate::config::lire_fenetre_config(app)
        .ok()
        .flatten()
        .unwrap_or(crate::config::FenetreConfig {
            monitor_index: 0,
            mode: "maximise".to_string(),
            geo: None,
        });
    cfg.geo = Some(geo);
    let _ = crate::config::sauver_fenetre_config(app, &cfg);
}

/// Applique monitor_index + mode à la fenêtre main. Partagé entre la
/// commande `fenetre_appliquer` et le boot.
fn appliquer(
    app: &AppHandle,
    monitor_index: usize,
    mode: &str,
    geo: Option<crate::config::FenetreGeo>,
) -> Result<(), String> {
    let win = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "Fenêtre main introuvable".to_string())?;
    let moniteurs = app
        .available_monitors()
        .map_err(|e| format!("Enum moniteurs: {}", e))?;
    if moniteurs.is_empty() {
        return Err("Aucun moniteur détecté".to_string());
    }
    // Index hors bornes (écran débranché depuis la sauvegarde) → primaire.
    let m = moniteurs.get(monitor_index).cloned().or_else(|| {
        app.primary_monitor()
            .ok()
            .flatten()
            .or_else(|| moniteurs.first().cloned())
    });
    let Some(m) = m else {
        return Err("Aucun moniteur détecté".to_string());
    };

    // Normaliser : une fenêtre maximisée ou fullscreen ne se déplace pas.
    if win.is_fullscreen().unwrap_or(false) {
        let _ = win.set_fullscreen(false);
    }
    if win.is_maximized().unwrap_or(false) {
        let _ = win.unmaximize();
    }

    // Poser la fenêtre dans le moniteur cible : maximize() et
    // set_fullscreen(true) s'appliquent sur l'écran qui la contient.
    win.set_position(tauri::Position::Physical(*m.position()))
        .map_err(|e| format!("set_position main: {}", e))?;

    match mode_valide(mode) {
        "fenetre" => {
            let (pos, size) = match geo {
                Some(g) if geo_intersecte(&g, m.position(), m.size()) => (
                    tauri::PhysicalPosition::new(g.x, g.y),
                    tauri::PhysicalSize::new(g.w.max(1), g.h.max(1)),
                ),
                // Pas de géométrie persistée, ou sauvée sur un autre écran
                // (pointerait hors écran) → centrage à la taille conf.
                _ => {
                    let w = (DEF_W * m.scale_factor()).round().max(1.0) as i32;
                    let h = (DEF_H * m.scale_factor()).round().max(1.0) as i32;
                    (
                        tauri::PhysicalPosition::new(
                            m.position().x + (m.size().width as i32 - w) / 2,
                            m.position().y + (m.size().height as i32 - h) / 2,
                        ),
                        tauri::PhysicalSize::new(w as u32, h as u32),
                    )
                }
            };
            win.set_size(tauri::Size::Physical(size))
                .map_err(|e| format!("set_size main: {}", e))?;
            win.set_position(tauri::Position::Physical(pos))
                .map_err(|e| format!("set_position main: {}", e))?;
        }
        "fullscreen" => {
            win.set_fullscreen(true)
                .map_err(|e| format!("set_fullscreen main: {}", e))?;
        }
        // "maximise" (défaut, mode_valide retombe dessus)
        _ => {
            win.maximize()
                .map_err(|e| format!("maximize main: {}", e))?;
        }
    }
    Ok(())
}

/// Géométrie courante si la fenêtre est en mode fenêtré (ni fullscreen ni
/// maximisée). outer_position (cadre) + inner_size (restauré par set_size —
/// pas de dérive de décorations au fil des cycles sauvegarde/restauration).
fn capturer_geo(app: &AppHandle) -> Option<crate::config::FenetreGeo> {
    let win = app.get_webview_window(LABEL)?;
    if win.is_fullscreen().unwrap_or(false) || win.is_maximized().unwrap_or(false) {
        return None;
    }
    let pos = win.outer_position().ok()?;
    let size = win.inner_size().ok()?;
    Some(crate::config::FenetreGeo {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    })
}

/// Vrai si la géométrie persistée recouvre au moins partiellement le
/// moniteur cible — sinon elle pointerait hors écran (sauvée sur un autre
/// moniteur) → l'appelant retombe sur le centrage.
fn geo_intersecte(
    g: &crate::config::FenetreGeo,
    pos: &tauri::PhysicalPosition<i32>,
    size: &tauri::PhysicalSize<u32>,
) -> bool {
    g.x < pos.x + size.width as i32
        && g.x + g.w as i32 > pos.x
        && g.y < pos.y + size.height as i32
        && g.y + g.h as i32 > pos.y
}

/// Normalise un mode venant du frontend/disque : valeurs connues, sinon
/// "maximise" (= comportement historique de tauri.conf.json).
fn mode_valide(mode: &str) -> &str {
    match mode {
        "fenetre" | "fullscreen" => mode,
        _ => "maximise",
    }
}
