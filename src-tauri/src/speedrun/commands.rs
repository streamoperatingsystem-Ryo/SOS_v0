// =============================================================================
// Speedrun Splitter — Commandes Tauri exposées au frontend
// -----------------------------------------------------------------------------
//   - speedrun_charger_asl(path)      → charge un fichier .asl, retourne settings
//   - speedrun_demarrer(totalSplits)  → démarre la boucle de polling
//   - speedrun_arreter()              → arrête la boucle
//   - speedrun_maj_settings(start, split, reset) → met à jour les settings
//   - speedrun_est_actif()            → retourne true si le splitter tourne
//   - speedrun_action_manuelle(action) → start/split/skip/undo/reset/pause
//   - speedrun_baseline_lire(chemin_lss) → baseline sauvegardée du .lss
//   - speedrun_baseline_enregistrer(chemin_lss, index, cumul_secs) → enregistre
//     le temps cumulé d'un split réalisé (référence « dernière fois »)
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
    // Baseline lue UNE fois dans son propre statement (piège Mutex non
    // réentrant — AGENTS.md). Elle voyage avec le payload lss pour que la
    // diffusion puisse calculer les deltas dès le chargement des segments.
    let baseline = baseline_slot().lock().unwrap().clone();
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
                    "baseline": baseline,
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

/// Chemin du dernier .lss chargé — clé de la baseline dans
/// speedrun_baselines.json (config.rs).
fn last_lss_path_slot() -> &'static Mutex<Option<String>> {
    static T: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(None))
}

/// Baseline de comparaison courante (temps de split cumulés en secondes par
/// segment) = sauvegarde « dernière fois » mergée avec les PB du .lss. C'est
/// la référence FIGÉE de la colonne delta pour le run en cours : rechargée au
/// chargement du .lss et à chaque event `started`. Les enregistrements de
/// splits en cours de run (speedrun_baseline_enregistrer) écrivent le fichier
/// sans toucher ce slot — ils préparent la référence du run SUIVANT.
fn baseline_slot() -> &'static Mutex<Vec<Option<f64>>> {
    static T: OnceLock<Mutex<Vec<Option<f64>>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(Vec::new()))
}

/// Fusionne la baseline sauvegardée (« dernière fois ») avec les PB du LSS :
/// priorité au temps enregistré, fallback PB cumulé du segment (real time
/// d'abord — même sémantique que l'ancienne colonne PB — puis game time).
fn merge_baseline(run: &lss::LssRun, saved: &[Option<f64>]) -> Vec<Option<f64>> {
    run.segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            saved
                .get(i)
                .copied()
                .flatten()
                .or(s.pb_real_time)
                .or(s.pb_game_time)
        })
        .collect()
}

/// Recharge la baseline mergée (fichier + PB du .lss courant) au départ d'un
/// run, la fige dans le slot (référence du run) et la pousse vers la
/// diffusion via WS. No-op si aucun .lss chargé.
fn rafraichir_et_emit_baseline(app: &AppHandle) {
    let path = last_lss_path_slot().lock().unwrap().clone();
    let run = last_lss_slot().lock().unwrap().clone();
    let (Some(path), Some(run)) = (path, run) else {
        return;
    };
    let saved = crate::config::lire_speedrun_baselines(app)
        .ok()
        .and_then(|cfg| cfg.par_lss.get(&path).cloned())
        .unwrap_or_default();
    let merged = merge_baseline(&run, &saved);
    *baseline_slot().lock().unwrap() = merged.clone();
    emit_ws_baseline(&merged);
}

/// Pousse la baseline (référence de la colonne delta) vers la diffusion via
/// WS :4321. Message `{"type":"speedrun-etat","etat":{"type":"baseline",
/// "splits":[...]}}` — émis au `started` du run : la diffusion la fige telle
/// quelle pendant tout le run.
fn emit_ws_baseline(baseline: &[Option<f64>]) {
    if let Ok(guard) = chat_tx_slot().lock() {
        if let Some(tx) = guard.as_ref() {
            let json = serde_json::json!({
                "type": "speedrun-etat",
                "etat": { "type": "baseline", "splits": baseline },
            })
            .to_string();
            let _ = tx.send(json);
        }
    }
}

/// Re-émet la dernière run LSS vers la diffusion (resync à la reconnexion WS).
/// Appelé par le serveur WS quand la diffusion demande un resync. Non-fatal
/// si aucune run n'a été chargée (la diffusion affiche « Aucun splits chargé »).
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
}

fn get_or_init_engine(app_handle: &AppHandle) -> &'static Mutex<Option<AslEngine>> {
    ASL_ENGINE.get_or_init(|| {
        let ah = app_handle.clone();
        let engine = AslEngine::new(move |event: SpeedrunEvent| {
            // 1. Event Tauri → store Svelte dashboard
            let _ = ah.emit("speedrun:event", &event);
            // 2. WS :4321 → diffusion.html (maj DOM directe)
            emit_ws(&event);
            // 3. Départ d'un run → figer + pousser la baseline de comparaison
            //    (référence de la colonne delta « dernière fois »).
            if matches!(event, SpeedrunEvent::Started) {
                rafraichir_et_emit_baseline(&ah);
            }
        });
        Mutex::new(Some(engine))
    })
}

/// Charge un fichier ASL et retourne la liste des settings.
#[tauri::command]
pub fn speedrun_charger_asl(
    app_handle: AppHandle,
    path: String,
) -> Result<AslLoadResult, String> {
    let lock = get_or_init_engine(&app_handle);
    let guard = lock.lock().unwrap();
    let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
    engine.load_asl(&path)
}

/// Charge un fichier .lss (LiveSplit Splits) et retourne les segments
/// (nom + PB real/game time). Émet aussi les métadonnées vers la diffusion
/// (WS :4321) pour que le widget speedrun côté OBS affiche l'en-tête (jeu +
/// catégorie) et la liste des segments — la diffusion n'a pas accès aux
/// commandes Tauri, elle reçoit l'état uniquement par WS.
#[tauri::command]
pub fn speedrun_charger_lss(app_handle: AppHandle, path: String) -> Result<super::asl::LssRun, String> {
    let source = std::fs::read_to_string(&path)
        .map_err(|e| format!("Lecture fichier .lss: {}", e))?;
    let run = lss::parse(&source).map_err(|e| e.to_string())?;
    // Baseline « dernière fois » mergée avec les PB → référence de la colonne
    // delta. Fige les slots (baseline + chemin) avant l'emit : le payload lss
    // transporte la baseline vers la diffusion.
    let saved = crate::config::lire_speedrun_baselines(&app_handle)
        .ok()
        .and_then(|cfg| cfg.par_lss.get(&path).cloned())
        .unwrap_or_default();
    *baseline_slot().lock().unwrap() = merge_baseline(&run, &saved);
    *last_lss_path_slot().lock().unwrap() = Some(path);
    emit_ws_lss(&run);
    Ok(run)
}

/// Démarre le splitter avec un nombre total de splits.
#[tauri::command]
pub fn speedrun_demarrer(
    app_handle: AppHandle,
    total_splits: i32,
    start: bool,
    split: bool,
    reset: bool,
) -> Result<(), String> {
    let lock = get_or_init_engine(&app_handle);
    let guard = lock.lock().unwrap();
    let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
    let settings = AslSettings { start, split, reset };
    engine.start(total_splits, settings)
}

/// Arrête le splitter.
#[tauri::command]
pub fn speedrun_arreter(app_handle: AppHandle) -> Result<(), String> {
    let lock = get_or_init_engine(&app_handle);
    let guard = lock.lock().unwrap();
    let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
    engine.stop()
}

/// Met à jour les settings du splitter (pendant qu'il tourne).
#[tauri::command]
pub fn speedrun_maj_settings(
    app_handle: AppHandle,
    start: bool,
    split: bool,
    reset: bool,
) -> Result<(), String> {
    let lock = get_or_init_engine(&app_handle);
    let guard = lock.lock().unwrap();
    let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
    let settings = AslSettings { start, split, reset };
    engine.update_settings(settings)
}

/// Retourne true si le splitter est actif (boucle de polling en cours).
#[tauri::command]
pub fn speedrun_est_actif() -> bool {
    let lock = match ASL_ENGINE.get() {
        Some(l) => l,
        None => return false,
    };
    let guard = lock.lock().unwrap();
    guard.is_some()
}

/// Envoie une action manuelle au splitter (contrôles manuels).
/// Actions : "start", "split", "skip", "undo", "reset", "pause".
/// L'action est exécutée dans le thread moteur (pas d'accès concurrent au timer).
#[tauri::command]
pub fn speedrun_action_manuelle(
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
    let lock = get_or_init_engine(&app_handle);
    let guard = lock.lock().unwrap();
    let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
    engine.action_manuelle(action)
}

/// Lit la config speedrun sauvegardée (speedrun.json). Retourne les defaults
/// si le fichier n'existe pas. Contient les chemins ASL/LSS + settings.
#[tauri::command]
pub fn speedrun_lire_config(app_handle: AppHandle) -> Result<crate::config::SpeedrunConfig, String> {
    crate::config::lire_speedrun_config(&app_handle)
}

/// Sauve la config speedrun (speedrun.json). Persiste les chemins ASL/LSS +
/// settings pour rechargement automatique au prochain démarrage.
#[tauri::command]
pub fn speedrun_sauver_config(
    app_handle: AppHandle,
    config: crate::config::SpeedrunConfig,
) -> Result<(), String> {
    crate::config::sauver_speedrun_config(&app_handle, &config)
}

/// Met à jour les settings ASL individuels (120+ pour MGS) dans le Context
/// Boa. Appelé quand l'utilisateur valide la modale de configuration. Les
/// valeurs sont appliquées en place → F.SettingEnabled(code) les verra au
/// prochain appel de split(). La persistance (speedrun.json) est gérée
/// séparément par speedrun_sauver_config.
#[tauri::command]
pub fn speedrun_maj_settings_asl(
    app_handle: AppHandle,
    valeurs: std::collections::HashMap<String, bool>,
) -> Result<(), String> {
    let lock = get_or_init_engine(&app_handle);
    let guard = lock.lock().unwrap();
    let engine = guard.as_ref().ok_or("Moteur non initialisé")?;
    engine.maj_settings_asl(valeurs)
}

/// Lit la baseline sauvegardée d'un fichier .lss (temps de split cumulés en
/// secondes par segment). Tableau vide si aucune run n'a encore été
/// enregistrée pour ce .lss — le frontend merge alors avec les PB du .lss.
#[tauri::command]
pub fn speedrun_baseline_lire(
    app_handle: AppHandle,
    chemin_lss: String,
) -> Result<Vec<Option<f64>>, String> {
    let cfg = crate::config::lire_speedrun_baselines(&app_handle)?;
    Ok(cfg.par_lss.get(&chemin_lss).cloned().unwrap_or_default())
}

/// Enregistre le temps cumulé (secondes) d'un segment dans la baseline du
/// .lss. Appelé par le frontend à chaque split réalisé : la baseline devient
/// la référence « dernière fois » du prochain run. Ne touche PAS le slot de
/// comparaison — la référence affichée reste figée pendant le run en cours.
#[tauri::command]
pub fn speedrun_baseline_enregistrer(
    app_handle: AppHandle,
    chemin_lss: String,
    index: usize,
    cumul_secs: f64,
) -> Result<(), String> {
    let mut cfg = crate::config::lire_speedrun_baselines(&app_handle)?;
    let splits = cfg.par_lss.entry(chemin_lss).or_default();
    while splits.len() <= index {
        splits.push(None);
    }
    splits[index] = Some(cumul_secs);
    crate::config::sauver_speedrun_baselines(&app_handle, &cfg)
}
