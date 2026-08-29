//! Helpers HTTP + WebSocket pour Kick.
//!
//! - `resolve_chatroom` : slug → chatroom ID (reqwest, GET kick.com/api/v2)
//! - `run_ws` : client WebSocket Pusher (tokio-tungstenite) vers le cloud Pusher
//!   public (ws-us2.pusher.com) — pas de Cloudflare, pas de token, pas de cookies.
//!
//! Kick utilise le cloud Pusher public (cluster us2, app key 32cbd69e4b950bf97679)
//! pour son chat en temps réel. Le WS vit côté Rust car le protocole Pusher
//! nécessite des ping/pong périodiques et un parsing des events — plus simple
//! à gérer côté Rust qu'en JS. Le cloud Pusher n'a pas de protection Cloudflare,
//! donc pas besoin de headers spéciaux, cookies, ou Origin.

use crate::scenes::ScenesState;
use crate::twitch_chat::ChatMessage;
use crate::KickState;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const KICK_API_BASE: &str = "https://kick.com/api/v2";

/// Cloud Pusher public utilisé par Kick (cluster us2, app key publique).
/// Hardcodé dans le bundle web de Kick (kick.com). Ces constantes sont publiques
/// (visibles dans le code source du site Kick). Si le chat cesse de fonctionner,
/// vérifier si Kick a changé de cluster ou d'app key (re-capturer depuis kick.com).
const PUSHER_WS_URL: &str = "wss://ws-us2.pusher.com/app/32cbd69e4b950bf97679?protocol=7&client=js&version=8.4.0-rc2&flash=false";

/// User-Agent simulant Chrome pour les requêtes HTTP vers kick.com (Cloudflare).
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Résout un slug de canal Kick (ex: "xqc") en ID de chatroom (entier).
/// GET https://kick.com/api/v2/channels/<slug> → data.chatroom.id.
pub async fn resolve_chatroom(slug: &str) -> Result<u64, String> {
    eprintln!("[Kick] resolve_chatroom: slug={}", slug);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Client build: {}", e))?;
    let url = format!("{}/channels/{}", KICK_API_BASE, slug);
    eprintln!("[Kick] resolve_chatroom: GET {}", url);
    let resp = client
        .get(&url)
        .header("User-Agent", UA)
        .header("Accept", "application/json")
        .header("Referer", "https://kick.com/")
        .header("Origin", "https://kick.com")
        .send()
        .await
        .map_err(|e| format!("Kick API réseau: {}", e))?;

    eprintln!("[Kick] resolve_chatroom: statut={}", resp.status());
    if !resp.status().is_success() {
        return Err(format!("Kick API: statut HTTP {}", resp.status()));
    }

    // Détecter Cloudflare (réponse HTML au lieu de JSON)
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if content_type.contains("text/html") {
        return Err("Kick API: bloqué par Cloudflare (réponse HTML)".to_string());
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Kick API parse JSON: {}", e))?;

    let chatroom_id = body["chatroom"]["id"]
        .as_u64()
        .ok_or("Kick API: chatroom.id non trouvé dans la réponse")?;
    Ok(chatroom_id)
}

/// Démarre le client WebSocket Pusher Kick en arrière-plan.
/// Retourne le JoinHandle (pour arrêt propre via abort).
/// `cancel` : AtomicBool partagé — set true pour arrêter.
///
/// Étapes :
/// 1. Ouvre WS vers le cloud Pusher public (ws-us2.pusher.com) — pas de
///    Cloudflare, pas de token, pas de cookies nécessaires.
/// 2. Attend pusher:connection_established
/// 3. Envoie pusher:subscribe pour chatrooms.<id>.v2
/// 4. Attend pusher_internal:subscription_succeeded → emit kick:connecte
/// 5. Boucle : lit les messages, gère ping/pong, forward ChatMessageEvent
/// 6. Sur close/error → emit kick:deconnecte
pub fn run_ws(
    app: AppHandle,
    chatroom_id: u64,
    cancel: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        eprintln!("[Kick] WS connecting to Pusher cloud");
        let (mut ws, resp) = match connect_async(PUSHER_WS_URL).await {
            Ok((ws, resp)) => (ws, resp),
            Err(e) => {
                eprintln!("[Kick] WS connect error: {}", e);
                let _ = app.emit("kick:deconnecte", ());
                return;
            }
        };
        eprintln!("[Kick] WS connected, HTTP status: {}", resp.status());

        let channel = format!("chatrooms.{}.v2", chatroom_id);
        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(20));
        ping_interval.tick().await; // skip first immediate tick

        loop {
            if cancel.load(Ordering::SeqCst) {
                eprintln!("[Kick] WS arrêt demandé");
                break;
            }

            tokio::select! {
                // Ping périodique (garder la connexion vivante)
                _ = ping_interval.tick() => {
                    let ping = serde_json::json!({"event":"pusher:ping","data":{}});
                    if ws.send(WsMessage::Text(ping.to_string().into())).await.is_err() {
                        eprintln!("[Kick] WS send ping error, closing");
                        break;
                    }
                }

                // Message entrant
                msg = ws.next() => {
                    let Some(msg_result) = msg else {
                        eprintln!("[Kick] WS stream ended");
                        break;
                    };
                    let frame = match msg_result {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("[Kick] WS read error: {}", e);
                            break;
                        }
                    };
                    let text = match frame {
                        WsMessage::Text(t) => t.to_string(),
                        WsMessage::Binary(b) => String::from_utf8_lossy(&b).to_string(),
                        WsMessage::Ping(data) => {
                            let _ = ws.send(WsMessage::Pong(data)).await;
                            continue;
                        }
                        WsMessage::Close(_) => {
                            eprintln!("[Kick] WS close frame received");
                            break;
                        }
                        _ => continue,
                    };

                    // Parser le message Pusher
                    let pusher_msg: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let event = pusher_msg["event"].as_str().unwrap_or("");

                    match event {
                        // 1. Connexion établie → subscribe
                        "pusher:connection_established" => {
                            eprintln!("[Kick] Pusher connection established");
                            let subscribe = serde_json::json!({
                                "event": "pusher:subscribe",
                                "data": {"auth": "", "channel": &channel}
                            });
                            if ws.send(WsMessage::Text(subscribe.to_string().into())).await.is_err() {
                                eprintln!("[Kick] WS send subscribe error");
                                break;
                            }
                        }

                        // 2. Subscription confirmée → marquer connecté
                        "pusher_internal:subscription_succeeded" => {
                            eprintln!("[Kick] subscribed to {}", channel);
                            if let Some(ks) = app.try_state::<KickState>() {
                                ks.set_connected(true);
                            }
                            let _ = app.emit("kick:connecte", &channel);
                        }

                        // 3. Ping du serveur → répondre pong
                        "pusher:ping" => {
                            let pong = serde_json::json!({"event":"pusher:pong","data":{}});
                            let _ = ws.send(WsMessage::Text(pong.to_string().into())).await;
                        }

                        // 4. Message chat → forward
                        "App\\Events\\ChatMessageEvent" => {
                            // data est double-encodée (string JSON dans string JSON)
                            let data_str = pusher_msg["data"].as_str().unwrap_or("{}");
                            let payload: serde_json::Value = match serde_json::from_str(data_str) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            let pseudo = payload["sender"]["username"].as_str().unwrap_or("unknown");
                            let texte = payload["content"].as_str().unwrap_or("");
                            if texte.is_empty() { continue; }

                            let chat_msg = ChatMessage {
                                plateforme: "kick".to_string(),
                                pseudo: pseudo.to_string(),
                                texte: texte.to_string(),
                                badges: None,
                                avatar: None,
                            };
                            let _ = app.emit("chat:message", &chat_msg);

                            // chat_tx → diffusion :4321
                            if let Some(state) = app.try_state::<ScenesState>() {
                                let json_msg = serde_json::json!({
                                    "type": "chat",
                                    "message": chat_msg,
                                }).to_string();
                                match state.chat_tx.send(json_msg) {
                                    Ok(n) => eprintln!("[Kick] WS chat envoyé pseudo={} receivers={}", pseudo, n),
                                    Err(_) => eprintln!("[Kick] WS chat AUCUN client pseudo={}", pseudo),
                                }
                            }
                        }

                        // Autres events → ignorer
                        _ => {
                            if !event.starts_with("pusher") && !event.starts_with("pusher_internal") {
                                eprintln!("[Kick] event ignoré: {}", event);
                            }
                        }
                    }
                }
            }
        }

        // Nettoyage : fermer le WS + emit déconnecté
        let _ = ws.close(None).await;
        if let Some(ks) = app.try_state::<KickState>() {
            ks.set_connected(false);
        }
        let _ = app.emit("kick:deconnecte", ());
        eprintln!("[Kick] WS task terminée");
    })
}
