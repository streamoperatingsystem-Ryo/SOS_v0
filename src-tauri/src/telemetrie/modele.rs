//! Modèle d'événement télémétrie — schéma v1 (une ligne JSONL par événement).
//!
//! Tous les champs `Option` sont ignorés à la sérialisation : de nouveaux
//! champs optionnels peuvent être ajoutés sans casser la lecture des anciens
//! fichiers (serde ignore les champs inconnus à la désérialisation).
//!
//! `plateforme` : "twitch" | "youtube" | "kick" | "tiktok" | "multi" | "local"
//! | "systeme". "multi" = overlay déclenchable depuis n'importe quelle
//! plateforme dont la frame ne porte pas l'origine (commande / welcome) ;
//! "local" = entrée locale (pad numérique) ; "systeme" = autre.
//!
//! `kind` : "connexion" | "deconnexion" | "chat" | "viewers" | "alerte"
//! | "commande" | "welcome" | "bandeau" | "pad" | "musique" | "stream-info"
//! | "event". `stream-info` = contexte du live (titre/jeu/tags, émis au
//! changement) ; `event` = alerte pré-filtre `chat:event` (chiffres
//! conservés : nb_viewers des raids, nb_bits, montant…).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version du schéma d'événement — à incrémenter en cas de rupture.
pub const SCHEMA_V: u8 = 1;

/// Événement normalisé — une ligne dans `telemetrie/events.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTelemetrie {
    /// Version du schéma (SCHEMA_V).
    pub v: u8,
    /// Horodatage ISO 8601 UTC posé à la réception de l'événement.
    pub ts: String,
    pub plateforme: String,
    pub kind: String,
    /// UUID de la session plateforme (fenêtre connexion → déconnexion).
    /// None si l'événement arrive hors session connue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Titre du stream — rempli par l'enrichissement stream_info (LOT 2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titre_stream: Option<String>,
    /// Jeu/catégorie du live (`game_name` Helix, verbatim) — estampillé
    /// quand un contexte stream-info est connu pour la plateforme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jeu: Option<String>,
    /// ID du jeu chez la source (game_id Twitch / category id Kick) — LOT 2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jeu_id: Option<String>,
    /// Pseudo du viewer à l'origine de l'événement, quand la frame le porte.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pseudo: Option<String>,
    /// Payload libre spécifique au kind (count, type d'alerte, id commande…).
    #[serde(default)]
    pub data: Value,
}

impl EventTelemetrie {
    /// Événement vierge pré-rempli (version + timestamp courant) — à compléter
    /// via `..EventTelemetrie::nouveau()`.
    pub fn nouveau() -> Self {
        Self {
            v: SCHEMA_V,
            ts: maintenant_iso(),
            plateforme: String::new(),
            kind: String::new(),
            session_id: None,
            titre_stream: None,
            jeu: None,
            jeu_id: None,
            pseudo: None,
            data: Value::Null,
        }
    }
}

/// Horodatage ISO 8601 UTC (millisecondes, suffixe Z).
pub fn maintenant_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
