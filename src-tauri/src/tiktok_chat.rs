//! Client TikTok Live via PirateTok (reverse engineering du protocole Webcast).
//!
//! Aucune API key, aucun serveur de signature, aucune authentification.
//! Juste un username TikTok → résolution room ID → WebSocket → events protobuf.
//!
//! Pattern identique à Kick (kick.rs) :
//! - `demarrer(app, chat_tx, username, cancel)` → spawn tokio task → JoinHandle
//! - Events `Chat` → ChatMessage { plateforme: "tiktok", pseudo, texte, avatar }
//! - Double diffusion : app.emit("chat:message") + chat_tx.send(json) pour :4321
//! - Events `RoomUserSeq` → emit "tiktok:viewers" (viewer_count)
//! - Lifecycle : Connected → emit "tiktok:connecte", Disconnected → emit "tiktok:deconnecte"

use crate::scenes::ScenesState;
use crate::twitch_chat::ChatMessage;
use crate::TiktokState;
use piratetok_live_rs::structs::TikTokLiveEvent;
use piratetok_live_rs::TikTokLive;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Démarre la connexion TikTok Live en arrière-plan.
/// Retourne le JoinHandle (pour arrêt propre via abort).
/// `cancel` : AtomicBool partagé — set true pour arrêter (en plus du Drop).
pub fn demarrer(
    app: AppHandle,
    _chat_tx: tokio::sync::broadcast::Sender<String>,
    username: String,
    cancel: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        eprintln!("[TikTok] connecting to @{}", username);

        // Construire la connexion PirateTok. max_retries élevé pour résilience.
        let stream = match TikTokLive::builder(&username)
            .max_retries(50)
            .stale_timeout(std::time::Duration::from_secs(90))
            .connect()
            .await
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[TikTok] ERR connect: {}", e);
                let _ = app.emit("tiktok:erreur", &format!("{}", e));
                let _ = app.emit("tiktok:deconnecte", ());
                return;
            }
        };

        eprintln!("[TikTok] connecté à @{}", username);
        if let Some(ts) = app.try_state::<TiktokState>() {
            ts.set_connected(true);
        }
        let _ = app.emit("tiktok:connecte", &username);

        let mut stream = stream;
        // Dedup viewers : RoomUserSeq arrive en rafale — on ne publie vers
        // :4321 que quand le chiffre change.
        let mut dernier_viewers: Option<i64> = None;
        loop {
            if cancel.load(Ordering::SeqCst) {
                eprintln!("[TikTok] arrêt demandé");
                break;
            }

            let event = match stream.next_event().await {
                Some(e) => e,
                None => {
                    eprintln!("[TikTok] stream ended (None)");
                    break;
                }
            };

            match event {
                TikTokLiveEvent::Connected { room_id } => {
                    eprintln!("[TikTok] Connected room_id={}", room_id);
                }

                TikTokLiveEvent::Reconnecting { attempt, max_retries, delay_secs } => {
                    eprintln!("[TikTok] reconnecting {}/{} in {}s", attempt, max_retries, delay_secs);
                }

                TikTokLiveEvent::Chat(msg) => {
                    let pseudo = msg.user.as_ref()
                        .map(|u| u.nickname.clone())
                        .unwrap_or_else(|| "unknown".to_string());
                    let texte = msg.comment.clone();
                    if texte.is_empty() { continue; }

                    // Avatar : première URL de avatar_thumb.url_list
                    let avatar = msg.user.as_ref()
                        .and_then(|u| u.avatar_thumb.as_ref())
                        .and_then(|img| img.url_list.first().cloned());

                    let chat_msg = ChatMessage {
                        plateforme: "tiktok".to_string(),
                        pseudo: pseudo.clone(),
                        texte: texte.clone(),
                        badges: None,
                        avatar,
                        color: None,
                        message_id: None,
                    };
                    let _ = app.emit("chat:message", &chat_msg);

                    // chat_tx → diffusion :4321
                    if let Some(state) = app.try_state::<ScenesState>() {
                        let json_msg = serde_json::json!({
                            "type": "chat",
                            "message": chat_msg,
                        }).to_string();
                        match state.chat_tx.send(json_msg) {
                            Ok(n) => eprintln!("[TikTok] chat envoyé pseudo={} receivers={}", pseudo, n),
                            Err(_) => eprintln!("[TikTok] chat AUCUN client pseudo={}", pseudo),
                        }
                    }
                    // Bandeau premier message : détection 1er message.
                    if let Some(bandeau) = app.try_state::<crate::bandeau::BandeauState>() {
                        bandeau.on_message(
                            "tiktok",
                            &pseudo,
                            &pseudo,
                            chat_msg.avatar.clone(),
                            &texte,
                        );
                    }
                    // Commandes chat : "!commande" → overlay diffusion.
                    if let Some(commandes) = app.try_state::<crate::commandes::CommandesState>() {
                        commandes.on_message(&pseudo, &pseudo, &texte);
                    }
                }

                TikTokLiveEvent::RoomUserSeq(msg) => {
                    let viewers = msg.viewer_count;
                    // Push natif (zéro polling) — forward vers :4321 pour la
                    // pastille viewers du widget chat, seulement si ça change.
                    if Some(viewers) != dernier_viewers {
                        dernier_viewers = Some(viewers);
                        eprintln!("[TikTok] viewers={}", viewers);
                        let _ = app.emit("tiktok:viewers", &viewers);
                        if let Some(state) = app.try_state::<ScenesState>() {
                            let _ = state.chat_tx.send(
                                serde_json::json!({
                                    "type": "viewers",
                                    "plateforme": "tiktok",
                                    "count": viewers,
                                })
                                .to_string(),
                            );
                        }
                        let _ = app.emit(
                            "viewers:update",
                            serde_json::json!({"plateforme": "tiktok", "count": viewers}),
                        );
                    }
                }

                TikTokLiveEvent::Gift(msg) => {
                    let pseudo = msg.user.as_ref()
                        .map(|u| u.nickname.clone())
                        .unwrap_or_else(|| "unknown".to_string());
                    let gift_name = msg.gift_details.as_ref()
                        .map(|g| g.gift_name.clone())
                        .unwrap_or_else(|| "gift".to_string());
                    let diamonds = msg.gift_details.as_ref()
                        .map(|g| g.diamond_count)
                        .unwrap_or(0);
                    eprintln!("[TikTok] gift {} de {} ({} diamants)", gift_name, pseudo, diamonds);
                    // Pour l'instant on ne forward pas les gifts dans le chat.
                    // Pour plus tard : event dédié "tiktok:gift" + UI alerte.
                }

                TikTokLiveEvent::Like(msg) => {
                    // Likes : pas de forward (trop bruyant). Log discret.
                    let _ = msg.total_like_count;
                }

                TikTokLiveEvent::Disconnected => {
                    eprintln!("[TikTok] Disconnected");
                    break;
                }

                // Autres events : ignorer (64 types au total, on gère l'essentiel)
                _ => {}
            }
        }

        // Nettoyage
        if let Some(ts) = app.try_state::<TiktokState>() {
            ts.set_connected(false);
        }
        let _ = app.emit("tiktok:deconnecte", ());
        eprintln!("[TikTok] task terminée");
    })
}
