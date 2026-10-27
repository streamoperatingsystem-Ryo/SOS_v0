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

use crate::alertes::{AlertesState, EventTwitch};
use crate::scenes::ScenesState;
use crate::twitch_chat::ChatMessage;
use crate::TiktokState;
use piratetok_live_rs::helpers::gift_streak::GiftStreakTracker;
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
        // Construire la connexion PirateTok. max_retries élevé pour résilience.
        let stream = match TikTokLive::builder(&username)
            .max_retries(50)
            .stale_timeout(std::time::Duration::from_secs(90))
            .connect()
            .await
        {
            Ok(s) => s,
            Err(e) => {
                let _ = app.emit("tiktok:erreur", &format!("{}", e));
                let _ = app.emit("tiktok:deconnecte", ());
                return;
            }
        };

        if let Some(ts) = app.try_state::<TiktokState>() {
            ts.set_connected(true);
        }
        let _ = app.emit("tiktok:connecte", &username);

        let mut stream = stream;
        // Dedup viewers : RoomUserSeq arrive en rafale — on ne publie vers
        // :4321 que quand le chiffre change.
        let mut dernier_viewers: Option<i64> = None;
        // Dedup gifts : les combos arrivent en rafale (repeat_count
        // cumulatif) — une seule alerte en fin de streak.
        let mut streaks = GiftStreakTracker::new();
        loop {
            if cancel.load(Ordering::SeqCst) {
                break;
            }

            let event = match stream.next_event().await {
                Some(e) => e,
                None => {
                    break;
                }
            };

            match event {
                TikTokLiveEvent::Connected { .. } => {}

                TikTokLiveEvent::Reconnecting { .. } => {}

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
                        emotes: None,
                    };
                    let _ = app.emit("chat:message", &chat_msg);

                    // chat_tx → diffusion :4321
                    if let Some(state) = app.try_state::<ScenesState>() {
                        let json_msg = serde_json::json!({
                            "type": "chat",
                            "message": chat_msg,
                        }).to_string();
                        let _ = state.chat_tx.send(json_msg);
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
                    // Accusé ignoré — pas de canal de réponse sur TikTok.
                    if let Some(commandes) = app.try_state::<crate::commandes::CommandesState>() {
                        let _ = commandes.on_message("tiktok", &pseudo, &pseudo, &texte);
                    }
                    // est_mod : TikTok n'expose pas de badge mod via
                    // PirateTok → streamer uniquement (unique_id ==
                    // username suivi). Partagé musique + arène.
                    let est_mod = msg
                        .user
                        .as_ref()
                        .map(|u| u.unique_id.eq_ignore_ascii_case(&username))
                        .unwrap_or(false);
                    // Musique : "!rs <titre>" → bibliothèque locale.
                    if let Some(musique) = app.try_state::<crate::musique::MusiqueState>() {
                        musique.on_message("tiktok", &pseudo, &texte, est_mod);
                    }
                    // Emoji Arena : "!fight"/coups → combat côté diffusion.
                    if let Some(fight) = app.try_state::<crate::fight::FightState>() {
                        fight.on_message("tiktok", &pseudo, &texte, est_mod);
                    }
                    // Cadres : "!cadres <couleur>" → couleur du cadre
                    // (retour ignoré — pas de canal de réponse).
                    if let Some(cadres) = app.try_state::<crate::cadres_cmd::CadresChatState>() {
                        let _ = cadres.on_message(&pseudo, &texte);
                    }
                }

                TikTokLiveEvent::RoomUserSeq(msg) => {
                    let viewers = msg.viewer_count;
                    // Push natif (zéro polling) — forward vers :4321 pour la
                    // pastille viewers du widget chat, seulement si ça change.
                    if Some(viewers) != dernier_viewers {
                        dernier_viewers = Some(viewers);
                        // Map partagée pour viewers-resync (pop-out chat /
                        // reload OBS arrivés en cours de live).
                        crate::viewers::noter_count("tiktok", Some(viewers));
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
                        // Objectifs (widgets goal métrique viewers) : même
                        // détection partagée que viewers.rs (crossing ≥ cible).
                        if let Some(state) = app.try_state::<ScenesState>() {
                            crate::viewers::verifier_objectifs(
                                &app,
                                &state.chat_tx,
                                "tiktok",
                                &[("viewers", viewers as u64)],
                            );
                        }
                    }
                }

                TikTokLiveEvent::Gift(msg) => {
                    // Combos : TikTok pousse un event par tick avec un
                    // repeat_count cumulatif — on n'alerte qu'en fin de
                    // streak (is_final) avec les totaux agrégés.
                    let streak = streaks.process(&msg);
                    if !streak.is_final {
                        continue;
                    }
                    let (pseudo, login, user_id) = msg
                        .user
                        .as_ref()
                        .map(|u| {
                            (
                                u.nickname.clone(),
                                u.unique_id.clone(),
                                u.user_id.to_string(),
                            )
                        })
                        .unwrap_or_else(|| {
                            ("unknown".to_string(), "unknown".to_string(), String::new())
                        });
                    let objet = msg
                        .gift_details
                        .as_ref()
                        .map(|g| g.gift_name.clone())
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| "gift".to_string());
                    if let Some(alertes) = app.try_state::<AlertesState>() {
                        alertes.on_event(EventTwitch {
                            type_alerte: "gift".into(),
                            plateforme: "tiktok".into(),
                            login,
                            pseudo,
                            user_id,
                            nb_viewers: 0,
                            nb_bits: 0,
                            niveau_sub: String::new(),
                            nb_mois: 0,
                            destinataire: String::new(),
                            system_msg: String::new(),
                            montant: format!("{} 💎", streak.total_diamond_count),
                            objet,
                            nb_objet: streak.total_gift_count.max(1) as i64,
                        });
                    }
                }

                TikTokLiveEvent::Follow(msg) => {
                    let (pseudo, login, user_id) = msg
                        .user
                        .as_ref()
                        .map(|u| {
                            (
                                u.nickname.clone(),
                                u.unique_id.clone(),
                                u.user_id.to_string(),
                            )
                        })
                        .unwrap_or_else(|| {
                            ("unknown".to_string(), "unknown".to_string(), String::new())
                        });
                    if let Some(alertes) = app.try_state::<AlertesState>() {
                        alertes.on_event(EventTwitch {
                            type_alerte: "follow".into(),
                            plateforme: "tiktok".into(),
                            login,
                            pseudo,
                            user_id,
                            nb_viewers: 0,
                            nb_bits: 0,
                            niveau_sub: String::new(),
                            nb_mois: 0,
                            destinataire: String::new(),
                            system_msg: String::new(),
                            montant: String::new(),
                            objet: String::new(),
                            nb_objet: 0,
                        });
                    }
                }

                TikTokLiveEvent::Like(_) => {
                    // Likes : pas de forward (trop bruyant).
                }

                TikTokLiveEvent::Disconnected => {
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
        // Déconnexion : efface la pastille viewers (parité `deconnecte` de
        // viewers.rs pour twitch/youtube/kick — count:null retire la
        // plateforme de la barre) + sort le count de la map resync.
        if dernier_viewers.is_some() {
            if let Some(state) = app.try_state::<ScenesState>() {
                let _ = state.chat_tx.send(
                    serde_json::json!({
                        "type": "viewers",
                        "plateforme": "tiktok",
                        "count": null,
                    })
                    .to_string(),
                );
            }
            let _ = app.emit(
                "viewers:update",
                serde_json::json!({"plateforme": "tiktok", "count": null}),
            );
            crate::viewers::noter_count("tiktok", None);
        }
        let _ = app.emit("tiktok:deconnecte", ());
    })
}
