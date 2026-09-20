//! Collecte passive — listeners Tauri + tap sur le canal `chat_tx`.
//!
//! Aucun module émetteur n'est modifié : la télémétrie observe les events
//! `*:connecte` / `*:deconnecte` et les frames déjà broadcastées vers la
//! diffusion :4321. Whitelist FERMÉE de types de frames — le reste est du
//! bruit de synchro UI (`*-config`, `*-stop`, `input-viewer-etat` 30 Hz,
//! `speedrun-etat`, `position-overlay-config`, `snapshot`) : ne pas tout
//! capter, ajouter un type ici = décision documentée.

use crate::telemetrie::TelemetrieState;
use serde_json::{json, Value};
use tauri::{AppHandle, Listener};
use tokio::sync::broadcast;

/// Types de frames `chat_tx` enregistrés → kind d'événement.
pub fn kind_depuis_frame(t: &str) -> Option<&'static str> {
    match t {
        "chat" => Some("chat"),
        "viewers" => Some("viewers"),
        "alerte-play" => Some("alerte"),
        "commande-play" => Some("commande"),
        "welcome-clip-play" => Some("welcome"),
        "bandeau-premier-message-play" => Some("bandeau"),
        "pad-play" => Some("pad"),
        _ => None,
    }
}

/// Plateforme d'origine d'une frame. Ordre : `plateforme` direct (viewers),
/// `message.plateforme` (chat), `viewer.plateforme` (bandeau). Fallbacks
/// documentés quand la frame ne la porte pas :
/// - "alerte" → "twitch" : seul émetteur d'`alerte_declencher` aujourd'hui ;
/// - "commande" / "welcome" → "multi" : déclenchables depuis les 4
///   plateformes, attribution exacte impossible depuis la frame seule ;
/// - "pad" → "local" : entrée clavier locale, sans plateforme.
pub fn plateforme_frame(kind: &str, frame: &Value) -> String {
    for ptr in ["/plateforme", "/message/plateforme", "/viewer/plateforme"] {
        if let Some(p) = frame.pointer(ptr).and_then(|v| v.as_str()) {
            return p.to_string();
        }
    }
    match kind {
        "alerte" => "twitch",
        "pad" => "local",
        "commande" | "welcome" => "multi",
        _ => "systeme",
    }
    .to_string()
}

/// Extrait (pseudo, data) d'une frame whitelistée — champs réduits au
/// signal utile (le texte du message, le count viewers, le type d'alerte…).
pub fn extraire(kind: &str, frame: &Value) -> (Option<String>, Value) {
    let champ = |ptr: &str| {
        frame
            .pointer(ptr)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    match kind {
        "chat" => (
            champ("/message/pseudo"),
            json!({ "texte": frame.pointer("/message/texte") }),
        ),
        "viewers" => (None, json!({ "count": frame.get("count") })),
        "alerte" => (None, json!({ "alerte_type": frame.pointer("/alerte/type") })),
        "commande" => (None, json!({ "commande_id": frame.pointer("/commande/id") })),
        "welcome" => (
            champ("/viewer/login"),
            json!({ "clip_titre": frame.pointer("/clip/titre") }),
        ),
        "bandeau" => (
            champ("/viewer/display_name"),
            json!({ "message": frame.get("message") }),
        ),
        "pad" => (None, json!({ "media_type": frame.pointer("/pad/media_type") })),
        _ => (None, Value::Null),
    }
}

/// Enregistre les listeners plateforme + la subscription chat_tx.
/// Appelé une fois depuis `TelemetrieState::demarrer` (setup).
pub fn demarrer(app: &AppHandle, state: TelemetrieState, chat_tx: broadcast::Sender<String>) {
    // Bornes de session — events émis par les modules de connexion
    // (lib.rs pour twitch/youtube, kick.rs / tiktok_chat.rs en interne).
    for plat in ["twitch", "youtube", "kick", "tiktok"] {
        let st = state.clone();
        app.listen_any(format!("{plat}:connecte"), move |_| {
            st.connexion(plat);
        });
        let st = state.clone();
        app.listen_any(format!("{plat}:deconnecte"), move |_| {
            st.deconnexion(plat);
        });
    }

    // Tap passif : même canal broadcast que le WS :4321 — zéro impact sur
    // les émetteurs. Lagged = canal saturé → on log et continue (server.rs).
    let mut rx = chat_tx.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(raw) => state.on_frame(&raw),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("[Telemetrie] chat_tx lagged ({} perdus) — continue", n);
                }
                Err(_) => break,
            }
        }
    });
}
