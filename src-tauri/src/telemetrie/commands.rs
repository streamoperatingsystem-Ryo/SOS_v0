//! Commandes Tauri du module télémétrie.

use crate::telemetrie::modele::EventTelemetrie;
use crate::telemetrie::TelemetrieState;

/// État du collecteur : compteurs par kind/plateforme, sessions actives,
/// timestamp du dernier événement, chemin du fichier JSONL.
#[tauri::command]
pub fn telemetrie_etat(state: tauri::State<'_, TelemetrieState>) -> serde_json::Value {
    state.etat()
}

/// N derniers événements en mémoire (plus récent en premier), bornés à 500.
/// Utile pour vérifier la collecte en dev sans ouvrir le fichier JSONL.
#[tauri::command]
pub fn telemetrie_derniers(
    limit: Option<usize>,
    state: tauri::State<'_, TelemetrieState>,
) -> Vec<EventTelemetrie> {
    state.derniers(limit.unwrap_or(50).min(500))
}

/// Onglet Audience — sessions live reconstruites depuis events.jsonl +
/// agrégats (top jeux/titres/créneaux/pics, totaux events). Le parse du
/// fichier tourne dans spawn_blocking : jamais sur le thread du runtime.
#[tauri::command]
pub async fn audience_sessions(
    depuis_jours: u32,
    app: tauri::AppHandle,
) -> Result<crate::telemetrie::analyse::AudienceSessions, String> {
    let fichier = crate::telemetrie::stockage::fichier_events(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::telemetrie::analyse::audience_sessions(&fichier, "twitch", depuis_jours)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Onglet Audience — détail d'une session : série rééchantillonnée 1 pt/min,
/// segments jeu, pics attribués, timeline d'events.
#[tauri::command]
pub async fn audience_session(
    session_id: String,
    app: tauri::AppHandle,
) -> Result<crate::telemetrie::analyse::SessionDetail, String> {
    let fichier = crate::telemetrie::stockage::fichier_events(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::telemetrie::analyse::audience_session(&fichier, "twitch", &session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
