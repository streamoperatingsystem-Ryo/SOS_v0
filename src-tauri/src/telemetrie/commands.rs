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
