// =============================================================================
// Speedrun Splitter — Commandes Tauri exposées au frontend
// -----------------------------------------------------------------------------
//   - speedrun_charger_asl(path)      → charge un fichier .asl, retourne settings
//   - speedrun_demarrer(totalSplits)  → démarre la boucle de polling
//   - speedrun_arreter()              → arrête la boucle
//   - speedrun_maj_settings(start, split, reset) → met à jour les settings
//   - speedrun_est_actif()            → retourne true si le splitter tourne
//   - speedrun_action_manuelle(action) → start/split/skip/undo/reset/pause
//   - speedrun_aller_split(index)      → clic sur une ligne : arme l'index
//     (timer arrêté) ou saute à l'index en retry de segment (run en cours)
//
// Events émis vers le frontend :
//   - Tauri "speedrun:event" → store Svelte dashboard (écoute app.listen)
//   - WS :4321 "speedrun-etat" → diffusion.html (maj DOM directe, pattern input-viewer)
// =============================================================================
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

use super::asl::lss;
use super::engine::{ActionManuelle, AslEngine, AslLoadResult, AslSettings, SpeedrunEvent};

/// Instance globale du moteur ASL (singleton).
/// Initialisée paresseusement avec un callback qui émet vers le frontend.
static ASL_ENGINE: OnceLock<Mutex<Option<AslEngine>>> = OnceLock::new();

/// Slot pour le canal broadcast WS :4321 (diffusion OBS).
/// Initialisé par `set_chat_tx()` au boot (lib.rs setup), lu par le callback
/// du moteur pour pousser l'état du timer vers diffusion.html.
/// Pattern identique à input_viewer.rs.
fn chat_tx_slot() -> &'static Mutex<Option<broadcast::Sender<String>>> {
    static T: OnceLock<Mutex<Option<broadcast::Sender<String>>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(None))
}

/// Initialise le canal broadcast WS pour la diffusion. Appelé une fois au boot
/// depuis lib.rs setup (avant la 1re commande speedrun).
pub fn set_chat_tx(tx: broadcast::Sender<String>) {
    *chat_tx_slot().lock().unwrap() = Some(tx);
}

/// Publie un event vers la diffusion via WS :4321 (si un canal est branché).
/// Non-fatal si aucun canal (dashboard sans diffusion connectée).
fn emit_ws(event: &SpeedrunEvent) {
    if let Ok(guard) = chat_tx_slot().lock() {
        if let Some(tx) = guard.as_ref() {
            let json = serde_json::json!({
                "type": "speedrun-etat",
                "etat": event,
            })
            .to_string();
            let _ = tx.send(json);
        }
    }
}

/// Publie les métadonnées LSS (nom du jeu, catégorie, segments) vers la
/// diffusion via WS :4321. Le dashboard reçoit ces données via la valeur de
/// retour de la commande, mais la diffusion (page OBS séparée) n'a pas accès
/// aux commandes Tauri → on les diffuse par WS pour qu'elle puisse remplir
/// l'en-tête (jeu + catégorie) et la liste des segments (splits).
/// Stocke aussi la dernière run pour permettre un resync quand la diffusion
/// se reconnecte (elle demande l'état courant via WS).
fn emit_ws_lss(run: &lss::LssRun) {
    // Mémoriser la dernière run pour le resync diffusion.
    *last_lss_slot().lock().unwrap() = Some(run.clone());
    emit_ws_lss_json(run);
}

/// Émet les métadonnées LSS vers le WS sans toucher au slot (évite le
/// re-lock du Mutex — appelé par reemit_lss qui détient déjà le lock).
fn emit_ws_lss_json(run: &lss::LssRun) {
    if let Ok(guard) = chat_tx_slot().lock() {
        if let Some(tx) = guard.as_ref() {
            let json = serde_json::json!({
                "type": "speedrun-etat",
                "etat": {
                    "type": "lss",
                    "nomJeu": run.nom_jeu,
                    "nomCategorie": run.nom_categorie,
                    "segments": run.segments.iter().map(|s| serde_json::json!({
                        "nom": s.nom,
                        "pbRealTime": s.pb_real_time,
                        "pbGameTime": s.pb_game_time,
                    })).collect::<Vec<_>>(),
                },
            })
            .to_string();
            let _ = tx.send(json);
        }
    }
}

/// Dernière run LSS chargée (pour resync diffusion à la reconnexion WS).
fn last_lss_slot() -> &'static Mutex<Option<lss::LssRun>> {
    static T: OnceLock<Mutex<Option<lss::LssRun>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(None))
}

/// Chemin du dernier .lss chargé — poussé au moteur (cible de la
/// sauvegarde PB) à chaque chargement ET au démarrage du splitter (le
/// moteur peut être créé APRÈS le premier charger_lss : au boot, le widget
/// restaure ses chemins avant que l'utilisateur ne presse Démarrer).
fn last_lss_path_slot() -> &'static Mutex<Option<String>> {
    static T: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(None))
}

/// Re-émet la dernière run LSS vers la diffusion (resync à la reconnexion
/// WS), puis demande au moteur l'état consolidé (event `etat` : phase,
/// index — armé inclus —, temps de segments, chronos) pour qu'un reload
/// OBS en cours de run retrouve index + temps. Non-fatal si aucune run
/// chargée ou aucun moteur (la diffusion affiche « Aucun splits chargé »).
/// ⚠️ NE PAS appeler emit_ws_lss ici (re-lock last_lss_slot → deadlock Mutex
/// non réentrant). On clone la run UNE fois puis libère le lock avant d'émettre.
pub fn reemit_lss() {
    let run_opt = {
        let guard = last_lss_slot().lock().unwrap();
        guard.as_ref().cloned()
    };
    if let Some(run) = run_opt {
        emit_ws_lss_json(&run);
    }
    if let Some(lock) = ASL_ENGINE.get() {
        if let Some(engine) = lock.lock().unwrap().as_ref() {
            let _ = engine.emit_etat();
        }
    }
}

fn get_or_init_engine(app_handle: &AppHandle) -> &'static Mutex<Option<AslEngine>> {
    ASL_ENGINE.get_or_init(|| {
        let ah = app_handle.clone();
        let engine = AslEngine::new(move |event: SpeedrunEvent| {
            // 1. Event Tauri → store Svelte dashboard
            let _ = ah.emit("speedrun:event", &event);
            // 2. WS :4321 → diffusion.html (maj DOM directe)
            emit_ws(&event);
        });
        Mutex::new(Some(engine))
    })
}

/// Charge un fichier ASL et retourne la liste des settings.
/// async + spawn_blocking : load_asl attend la réponse du thread moteur via
/// resp_rx.recv() (parse + transpiler + compile Boa) — sync = thread
/// principal gelé → « Ne répond pas » (fix drag widget speedrun).
#[tauri::command]
pub async fn speedrun_charger_asl(
    app_handle: AppHandle,
    path: String,
) -> Result<AslLoadResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lock = get_or_init_engine(&app_handle);
        let guard = lock.lock().unwrap();
        let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
        let result = engine.load_asl(&path)?;
        // Script changé pendant que la boucle tourne → resync : reset de la
        // tentative + re-attach (les watchers doivent être reconstruits
        // depuis le NOUVEAU script). Le total (du .lss) est conservé. No-op
        // effectif si la boucle ne tourne pas.
        let _ = engine.resync();
        Ok(result)
    })
    .await
    .map_err(|e| format!("speedrun_charger_asl: {}", e))?
}

/// Charge un fichier .lss (LiveSplit Splits) et retourne les segments
/// (nom + PB real/game time). Émet aussi les métadonnées vers la diffusion
/// (WS :4321) pour que le widget speedrun côté OBS affiche l'en-tête (jeu +
/// catégorie) et la liste des segments — la diffusion n'a pas accès aux
/// commandes Tauri, elle reçoit l'état uniquement par WS.
#[tauri::command]
pub async fn speedrun_charger_lss(_app_handle: AppHandle, path: String) -> Result<super::asl::LssRun, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("Lecture fichier .lss: {}", e))?;
        let run = lss::parse(&source).map_err(|e| e.to_string())?;
        let nb_segments = run.segments.len() as i32;
        emit_ws_lss(&run);
        // Pousser le chemin au moteur : cible de la sauvegarde PB (fin de
        // run qui bat le PB) ET garde de resync — si le moteur tourne et que
        // le chemin CHANGE (autre catégorie/splitter), la tentative en cours
        // est resynchronisée avec le nouveau total (reset + re-attach).
        // Recharger le MÊME chemin (event saved) ne reset rien.
        *last_lss_path_slot().lock().unwrap() = Some(path.clone());
        if let Some(lock) = ASL_ENGINE.get() {
            if let Some(engine) = lock.lock().unwrap().as_ref() {
                let _ = engine.set_lss_path(Some(path), Some(nb_segments));
            }
        }
        Ok(run)
    })
    .await
    .map_err(|e| format!("speedrun_charger_lss: {}", e))?
}

/// Démarre le splitter avec un nombre total de splits.
#[tauri::command]
pub async fn speedrun_demarrer(
    app_handle: AppHandle,
    total_splits: i32,
    start: bool,
    split: bool,
    reset: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lock = get_or_init_engine(&app_handle);
        let guard = lock.lock().unwrap();
        let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
        // Pousser le chemin .lss courant au moteur AVANT le start : au boot
        // le widget restaure ses chemins avant que l'utilisateur ne presse
        // Démarrer — le moteur est créé ici et n'a encore aucun chemin (cible
        // de la sauvegarde PB). Le moteur ne tourne pas encore → aucun resync.
        let chemin_lss = last_lss_path_slot().lock().unwrap().clone();
        let _ = engine.set_lss_path(chemin_lss, None);
        let settings = AslSettings { start, split, reset };
        engine.start(total_splits, settings)
    })
    .await
    .map_err(|e| format!("speedrun_demarrer: {}", e))?
}

/// Arrête le splitter.
#[tauri::command]
pub async fn speedrun_arreter(app_handle: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lock = get_or_init_engine(&app_handle);
        let guard = lock.lock().unwrap();
        let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
        engine.stop()
    })
    .await
    .map_err(|e| format!("speedrun_arreter: {}", e))?
}

/// Met à jour les settings du splitter (pendant qu'il tourne).
#[tauri::command]
pub async fn speedrun_maj_settings(
    app_handle: AppHandle,
    start: bool,
    split: bool,
    reset: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lock = get_or_init_engine(&app_handle);
        let guard = lock.lock().unwrap();
        let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
        let settings = AslSettings { start, split, reset };
        engine.update_settings(settings)
    })
    .await
    .map_err(|e| format!("speedrun_maj_settings: {}", e))?
}

/// Retourne true si le splitter est actif (boucle de polling en cours).
#[tauri::command]
pub async fn speedrun_est_actif() -> bool {
    tauri::async_runtime::spawn_blocking(|| {
        let lock = match ASL_ENGINE.get() {
            Some(l) => l,
            None => return false,
        };
        let guard = lock.lock().unwrap();
        guard.is_some()
    })
    .await
    .unwrap_or(false)
}

/// Saute vers un split (clic sur une ligne du widget) :
///   - timer arrêté → arme l'index (le prochain start ASL/manuel part de
///     là), chrono à zéro, ne démarre pas ;
///   - run en cours → retry de segment : saute à l'index, chrono de segment
///     à zéro, tous les temps de la tentative effacés (pas un reset de run).
#[tauri::command]
pub async fn speedrun_aller_split(
    app_handle: AppHandle,
    index: i32,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lock = get_or_init_engine(&app_handle);
        let guard = lock.lock().unwrap();
        let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
        engine.action_manuelle(ActionManuelle::GoTo { index })
    })
    .await
    .map_err(|e| format!("speedrun_aller_split: {}", e))?
}

/// Envoie une action manuelle au splitter (contrôles manuels).
/// Actions : "start", "split", "skip", "undo", "reset", "pause".
/// L'action est exécutée dans le thread moteur (pas d'accès concurrent au timer).
#[tauri::command]
pub async fn speedrun_action_manuelle(
    app_handle: AppHandle,
    action: String,
) -> Result<(), String> {
    let action = match action.as_str() {
        "start" => ActionManuelle::Start,
        "split" => ActionManuelle::Split,
        "skip" => ActionManuelle::Skip,
        "undo" => ActionManuelle::Undo,
        "reset" => ActionManuelle::Reset,
        "pause" => ActionManuelle::Pause,
        other => return Err(format!("Action manuelle inconnue: {}", other)),
    };
    tauri::async_runtime::spawn_blocking(move || {
        let lock = get_or_init_engine(&app_handle);
        let guard = lock.lock().unwrap();
        let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
        engine.action_manuelle(action)
    })
    .await
    .map_err(|e| format!("speedrun_action_manuelle: {}", e))?
}

/// Lit la config speedrun sauvegardée (speedrun.json). Retourne les defaults
/// si le fichier n'existe pas. Contient les chemins ASL/LSS + settings.
#[tauri::command]
pub async fn speedrun_lire_config(app_handle: AppHandle) -> Result<crate::config::SpeedrunConfig, String> {
    tauri::async_runtime::spawn_blocking(move || crate::config::lire_speedrun_config(&app_handle))
        .await
        .map_err(|e| format!("speedrun_lire_config: {}", e))?
}

/// Sauve la config speedrun (speedrun.json). Persiste les chemins ASL/LSS +
/// settings pour rechargement automatique au prochain démarrage.
#[tauri::command]
pub async fn speedrun_sauver_config(
    app_handle: AppHandle,
    config: crate::config::SpeedrunConfig,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::config::sauver_speedrun_config(&app_handle, &config)
    })
    .await
    .map_err(|e| format!("speedrun_sauver_config: {}", e))?
}

/// Met à jour les settings ASL individuels (120+ pour MGS) dans le Context
/// Boa. Appelé quand l'utilisateur valide la modale de configuration. Les
/// valeurs sont appliquées en place → F.SettingEnabled(code) les verra au
/// prochain appel de split(). La persistance (speedrun.json) est gérée
/// séparément par speedrun_sauver_config.
#[tauri::command]
pub async fn speedrun_maj_settings_asl(
    app_handle: AppHandle,
    valeurs: std::collections::HashMap<String, bool>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lock = get_or_init_engine(&app_handle);
        let guard = lock.lock().unwrap();
        let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
        engine.maj_settings_asl(valeurs)
    })
    .await
    .map_err(|e| format!("speedrun_maj_settings_asl: {}", e))?
}
