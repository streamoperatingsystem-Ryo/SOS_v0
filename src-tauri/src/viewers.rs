//! Compteur de viewers unifié — affiché dans le widget chat en diffusion.
//!
//! UNE seule tâche pour toutes les plateformes (pas de boucle par plateforme) :
//! tick toutes les 15s, chaque plateforme connectée est interrogée selon son
//! propre rythme — 60s si en live, 180s si hors-ligne (back-off : inutile de
//! frapper l'API pour un stream arrêté). Max ~3 GET/min au total.
//!
//! - Twitch  : Helix GET /streams (stream_viewers)
//! - YouTube : Data API liveStreamingDetails.concurrentViewers (live_viewers)
//! - Kick    : GET api/v2/channels/<slug> → livestream.viewer_count
//! - TikTok  : RIEN ici — le count est poussé par le WS Webcast (RoomUserSeq),
//!   forwardé vers chat_tx dans tiktok_chat.rs. Zéro polling.
//!
//! Publication : `{"type":"viewers","plateforme":p,"count":n|null}` sur chat_tx
//! (diffusion :4321) + app.emit("viewers:update"). Envoyé seulement quand le
//! chiffre change (dedup) ou qu'une plateforme passe hors-ligne / déconnectée.

use serde_json::json;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

/// Granularité de la boucle (pas le rythme d'appel API — voir LIVE_EVERY).
const TICK: Duration = Duration::from_secs(15);
/// Intervalle de fetch par plateforme EN LIVE.
const LIVE_EVERY: Duration = Duration::from_secs(60);
/// Intervalle de fetch par plateforme HORS-LIGNE ou en erreur.
const OFFLINE_EVERY: Duration = Duration::from_secs(180);

/// État par plateforme : prochain fetch autorisé + dernier count publié.
struct Plat {
    prochain: Instant,
    dernier: Option<u32>,
}

impl Plat {
    fn new(now: Instant) -> Self {
        Plat {
            prochain: now, // fetch dès le prochain tick (connexion fraîche)
            dernier: None,
        }
    }
}

/// Démarre la tâche viewers (durée de vie = app). Zéro coût si aucune
/// plateforme n'est connectée : la boucle dort 15s et ne fait aucun fetch.
pub fn demarrer(app: AppHandle, chat_tx: broadcast::Sender<String>) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        eprintln!("[Viewers] tâche démarrée (tick 15s, live 60s, off 180s)");
        let mut plats: HashMap<&'static str, Plat> = HashMap::new();
        let mut interval = tokio::time::interval(TICK);
        loop {
            interval.tick().await;
            let now = Instant::now();

            // --- Twitch (Helix /streams) ---
            if let Some((uid, access)) = twitch_creds(&app) {
                let p = plats.entry("twitch").or_insert_with(|| Plat::new(now));
                if now >= p.prochain {
                    match crate::twitch_helix::stream_viewers(&uid, &access).await {
                        Ok(v) => {
                            p.prochain = now + if v.is_some() { LIVE_EVERY } else { OFFLINE_EVERY };
                            publier(&app, &chat_tx, "twitch", v, &mut p.dernier);
                        }
                        Err(e) => {
                            eprintln!("[Viewers] twitch err: {:?}", e);
                            p.prochain = now + LIVE_EVERY;
                        }
                    }
                }
            } else {
                deconnecte(&app, &chat_tx, &mut plats, "twitch");
            }

            // --- YouTube (liveStreamingDetails.concurrentViewers) ---
            if let Some((cid, access)) = youtube_creds(&app) {
                let p = plats.entry("youtube").or_insert_with(|| Plat::new(now));
                if now >= p.prochain {
                    match crate::youtube_data::live_viewers(&access, &cid).await {
                        Ok(v) => {
                            p.prochain = now + if v.is_some() { LIVE_EVERY } else { OFFLINE_EVERY };
                            publier(&app, &chat_tx, "youtube", v, &mut p.dernier);
                        }
                        Err(e) => {
                            eprintln!("[Viewers] youtube err: {}", e);
                            p.prochain = now + OFFLINE_EVERY;
                        }
                    }
                }
            } else {
                deconnecte(&app, &chat_tx, &mut plats, "youtube");
            }

            // --- Kick (api/v2/channels → livestream.viewer_count) ---
            if let Some(slug) = kick_creds(&app) {
                let p = plats.entry("kick").or_insert_with(|| Plat::new(now));
                if now >= p.prochain {
                    match crate::kick::fetch_viewers(&slug).await {
                        Ok(v) => {
                            p.prochain = now + if v.is_some() { LIVE_EVERY } else { OFFLINE_EVERY };
                            publier(&app, &chat_tx, "kick", v, &mut p.dernier);
                        }
                        Err(e) => {
                            eprintln!("[Viewers] kick err: {}", e);
                            p.prochain = now + OFFLINE_EVERY;
                        }
                    }
                }
            } else {
                deconnecte(&app, &chat_tx, &mut plats, "kick");
            }
        }
    })
}

/// Envoie le count sur chat_tx (diffusion) + emit frontend — seulement si
/// la valeur a changé depuis la dernière publication.
fn publier(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    plat: &'static str,
    count: Option<u32>,
    dernier: &mut Option<u32>,
) {
    if *dernier == count {
        return;
    }
    *dernier = count;
    let msg = json!({"type": "viewers", "plateforme": plat, "count": count}).to_string();
    let _ = chat_tx.send(msg);
    let _ = app.emit("viewers:update", json!({"plateforme": plat, "count": count}));
    match count {
        Some(n) => eprintln!("[Viewers] {}={}", plat, n),
        None => eprintln!("[Viewers] {}=off", plat),
    }
}

/// Plateforme déconnectée : publie None une fois (efface la pastille) puis
/// retire l'entrée — le prochain `Plat::new` au retour fera un fetch immédiat.
fn deconnecte(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    plats: &mut HashMap<&'static str, Plat>,
    nom: &'static str,
) {
    if let Some(p) = plats.remove(nom) {
        if p.dernier.is_some() {
            let msg = json!({"type": "viewers", "plateforme": nom, "count": null}).to_string();
            let _ = chat_tx.send(msg);
            let _ = app.emit("viewers:update", json!({"plateforme": nom, "count": null}));
            eprintln!("[Viewers] {}=déconnecté", nom);
        }
    }
}

/// (user_id, access) si Twitch connecté. Un lock par statement, un mutex
/// différent à chaque fois (règle mutex non réentrant).
fn twitch_creds(app: &AppHandle) -> Option<(String, String)> {
    let st = app.try_state::<crate::TwitchState>()?;
    let connected = *st.connected.lock().ok()?;
    if !connected {
        return None;
    }
    let uid = st.user_id.lock().ok()?.clone()?;
    let access = st.access.lock().ok()?.clone()?;
    Some((uid, access))
}

/// (channel_id, access) si YouTube connecté.
fn youtube_creds(app: &AppHandle) -> Option<(String, String)> {
    let st = app.try_state::<crate::YoutubeState>()?;
    let connected = *st.connected.lock().ok()?;
    if !connected {
        return None;
    }
    let cid = st.channel_id.lock().ok()?.clone()?;
    let access = st.access.lock().ok()?.clone()?;
    Some((cid, access))
}

/// slug du canal si Kick connecté.
fn kick_creds(app: &AppHandle) -> Option<String> {
    let st = app.try_state::<crate::KickState>()?;
    let connected = *st.connected.lock().ok()?;
    if !connected {
        return None;
    }
    let slug = st.slug.lock().ok()?.clone();
    slug
}
