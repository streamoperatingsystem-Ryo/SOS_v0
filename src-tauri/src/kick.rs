//! Helpers HTTP + WebSocket pour Kick.
//!
//! - `resolve_channel` : slug → (channel ID, chatroom ID) (hyper+Schannel, GET
//!   kick.com/api/v2)
//! - `run_ws` : client WebSocket Centrifuge vers le serveur temps réel de Kick
//!   (realtime.<region>.platform.kick.com).
//!
//! Kick a quitté le cloud Pusher hébergé (ws-us2.pusher.com → « 4001 App key
//! not in this cluster ») et le socket viewer/v1 ne sert qu'au comptage de
//! spectateurs. Le chat passe désormais par une connexion Centrifugo obtenue
//! via l'API web anonyme :
//!   1. POST web.kick.com/api/v1/realtime/channels/{channel_id}/chat/connection
//!      (body client éphémère + providers acceptés) → URL WS Centrifugo
//!   2. POST web.kick.com/api/v1/realtime/auth/connection {client_id} →
//!      jeton JWT « guest » (~45 min)
//!   3. WS : {"connect":{token}} → {"subscribe":{channel:"chatrooms.<id>.v2"}}
//!      → trames {"push":{"pub":{"data":{"event":"App\\Events\\ChatMessageEvent",
//!      "data":"<json>"}}}} — même payload qu'avant, parsing inchangé.
//!
//! Ping serveur = frame "{}" → répondre "{}". Jeton expirant → frame refresh.

use crate::scenes::ScenesState;
use crate::twitch_chat::ChatMessage;
use crate::KickState;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const KICK_API_BASE: &str = "https://kick.com/api/v2";

/// API web de Kick (hôte web.kick.com — pas kick.com ni websockets.kick.com).
/// Sert à obtenir la connexion temps réel et le jeton invité, anonymement.
/// Vérifié fonctionnel sans compte ni cookie (2026-09-22).
const KICK_REALTIME_BASE: &str = "https://web.kick.com/api/v1/realtime";

/// User-Agent simulant Chrome pour les requêtes HTTP vers kick.com (Cloudflare).
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Corps de requête HTTP vide ou JSON (hyper 1.x n'a plus de Body::empty()).
type KickBody = http_body_util::Full<hyper::body::Bytes>;

/// Client HTTP Kick : hyper + hyper-tls (Schannel Windows) — Cloudflare bloque
/// l'empreinte TLS de rustls sur kick.com (403), Schannel passe.
/// VOLONTAIREMENT séparé de reqwest : la feature native-tls de reqwest
/// changerait le connecteur par défaut de tout le backend (Twitch, YouTube…).
fn kick_http_client(
) -> hyper_util::client::legacy::Client<
    hyper_tls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
    KickBody,
> {
    hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(hyper_tls::HttpsConnector::new())
}

/// Lit le corps d'une réponse hyper en JSON, avec détection du challenge
/// Cloudflare (réponse HTML au lieu de JSON) et statut HTTP.
async fn read_json_body(
    resp: hyper::Response<hyper::body::Incoming>,
    contexte: &str,
) -> Result<serde_json::Value, String> {
    if !resp.status().is_success() {
        return Err(format!("{}: statut HTTP {}", contexte, resp.status()));
    }
    let content_type = resp
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if content_type.contains("text/html") {
        return Err(format!("{}: bloqué par Cloudflare (réponse HTML)", contexte));
    }
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("{} lecture corps: {}", contexte, e))?
        .to_bytes();
    serde_json::from_slice(&bytes).map_err(|e| format!("{} parse JSON: {}", contexte, e))
}

/// GET https://kick.com/api/v2/channels/<slug> → body JSON parsé.
/// Headers anti-Cloudflare (UA Chrome + Referer/Origin kick.com).
/// Partagé par resolve_channel (id + chatroom.id) et fetch_channel_stats.
async fn get_channel(slug: &str) -> Result<serde_json::Value, String> {
    let url = format!("{}/channels/{}", KICK_API_BASE, slug);
    let req = hyper::Request::builder()
        .uri(&url)
        .header("user-agent", UA)
        .header("accept", "application/json")
        .header("referer", "https://kick.com/")
        .header("origin", "https://kick.com")
        .body(KickBody::new(hyper::body::Bytes::new()))
        .map_err(|e| format!("Kick API requête: {}", e))?;
    let resp = kick_http_client()
        .request(req)
        .await
        .map_err(|e| format!("Kick API réseau: {}", e))?;
    read_json_body(resp, "Kick API").await
}

/// Résout un slug de canal Kick (ex: "xqc") en (channel_id, chatroom_id).
/// GET https://kick.com/api/v2/channels/<slug> → data.id + data.chatroom.id.
/// channel_id sert au path /realtime/channels/{id}/chat/connection ;
/// chatroom_id au canal de souscription chatrooms.<id>.v2.
pub async fn resolve_channel(slug: &str) -> Result<(u64, u64), String> {
    let body = get_channel(slug).await?;
    let channel_id = body["id"]
        .as_u64()
        .ok_or("Kick API: id non trouvé dans la réponse")?;
    let chatroom_id = body["chatroom"]["id"]
        .as_u64()
        .ok_or("Kick API: chatroom.id non trouvé dans la réponse")?;
    Ok((channel_id, chatroom_id))
}

/// Viewers + followers en UN SEUL GET (api/v2/channels/<slug> porte les deux
/// champs — mutualisé, zéro coût supplémentaire pour les objectifs).
/// Retourne (viewers, followers) — chacun None si hors-ligne/champ absent.
pub async fn fetch_channel_stats(slug: &str) -> Result<(Option<u32>, Option<u64>), String> {
    let body = get_channel(slug).await?;
    let viewers = body["livestream"]["viewer_count"]
        .as_u64()
        .map(|n| n as u32);
    let followers = body["followersCount"]
        .as_u64()
        .or_else(|| body["followers_count"].as_u64());
    Ok((viewers, followers))
}

/// POST anonyme vers web.kick.com/api/v1/realtime/<path> → body JSON parsé.
/// En-têtes calqués sur le client web officiel (x-app-platform: web).
async fn realtime_post(path: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("{}{}", KICK_REALTIME_BASE, path))
        .header("user-agent", UA)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("x-app-platform", "web")
        .header("referer", "https://kick.com/")
        .header("origin", "https://kick.com")
        .body(KickBody::new(hyper::body::Bytes::from(body.to_string())))
        .map_err(|e| format!("Kick realtime requête: {}", e))?;
    let resp = kick_http_client()
        .request(req)
        .await
        .map_err(|e| format!("Kick realtime réseau: {}", e))?;
    read_json_body(resp, "Kick realtime").await
}

/// POST /realtime/channels/{channel_id}/chat/connection → URL du serveur
/// Centrifugo (data.connections[0].credentials.url). L'identité client est
/// un UUID éphémère — pas de compte, pas de cookie.
async fn fetch_chat_connection(channel_id: u64, client_id: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "client": {"id": client_id, "type": "web"},
        "capabilities": {"accepted_providers": [{"provider": "pusher"}, {"provider": "centrifugo"}]},
    });
    let resp = realtime_post(&format!("/channels/{}/chat/connection", channel_id), body).await?;
    // Le serveur choisit le provider ; on ne gère que centrifugo (cas observé).
    let conn = &resp["data"]["connections"][0];
    if conn["provider"].as_str() != Some("centrifugo") {
        return Err(format!(
            "Kick realtime: provider non supporté {:?}",
            conn["provider"]
        ));
    }
    conn["credentials"]["url"]
        .as_str()
        .map(|u| u.to_string())
        .ok_or_else(|| "Kick realtime: credentials.url absent".to_string())
}

/// POST /realtime/auth/connection {client_id} → jeton JWT « guest » (~45 min).
/// Optionnel en théorie (connexion anonyme possible), mais envoyé comme le
/// fait le client web officiel.
async fn fetch_connection_token(client_id: &str) -> Result<String, String> {
    let resp = realtime_post(
        "/auth/connection",
        serde_json::json!({"client_id": client_id}),
    )
    .await?;
    resp["data"]["token"]
        .as_str()
        .map(|t| t.to_string())
        .ok_or_else(|| "Kick realtime: data.token absent".to_string())
}

/// Connexion WS avec TLS Schannel (tokio-native-tls) + Origin navigateur.
/// Cloudflare bloque rustls sur les hotes Kick ; Schannel (comme curl) passe.
/// TcpStream + TLS manuels puis client_async : n'utilise PAS connect_async,
/// dont le connecteur par défaut est partagé avec Twitch/OBS (rustls).
async fn kick_ws_connect(
    url: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_native_tls::TlsStream<tokio::net::TcpStream>>,
    String,
> {
    let mut request = url
        .into_client_request()
        .map_err(|e| format!("WS request: {}", e))?;
    let host = request
        .uri()
        .host()
        .ok_or("WS: hôte absent de l'URL")?
        .to_string();
    let port = request.uri().port_u16().unwrap_or(443);
    request
        .headers_mut()
        .insert("Origin", "https://kick.com".parse().unwrap());
    request
        .headers_mut()
        .insert("User-Agent", UA.parse().unwrap());

    let tcp = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| format!("WS TCP {}:{}: {}", host, port, e))?;
    let connector = tokio_native_tls::TlsConnector::from(
        native_tls::TlsConnector::new().map_err(|e| format!("TLS: {}", e))?,
    );
    let tls_stream = connector
        .connect(&host, tcp)
        .await
        .map_err(|e| format!("WS TLS {}: {}", host, e))?;
    client_async(request, tls_stream)
        .await
        .map(|(ws, _resp)| ws)
        .map_err(|e| format!("WS connect: {}", e))
}

/// Backoff de reconnexion : 2s, 4s, 8s, 16s, 30s (plafond).
fn backoff(tentative: u32) -> std::time::Duration {
    let secs = (2u64 << tentative.min(4)).min(30);
    std::time::Duration::from_secs(secs)
}

/// Démarre le client WebSocket Kick (protocole Centrifuge) en arrière-plan.
/// Retourne le JoinHandle (pour arrêt propre via abort).
/// `cancel` : AtomicBool partagé — set true pour arrêter.
///
/// Étapes (boucle de reconnexion, max 5 échecs consécutifs) :
/// 1. POST realtime chat/connection → URL du serveur Centrifugo
/// 2. POST realtime auth/connection → jeton JWT invité (~45 min, expirant)
/// 3. WS : envoie {"connect":{token}} → ack → envoie {"subscribe":
///    {channel:"chatrooms.<id>.v2"}} → ack → emit kick:connecte
/// 4. Boucle : répond "{}" aux pings serveur "{}", forward les messages
///    chat (push.pub.data.event = App\Events\ChatMessageEvent), refresh le
///    jeton avant expiration (frame {"refresh":{token}})
/// 5. Sur close/error → reconnecte. Sur cancel ou épuisement des tentatives
///    → emit kick:deconnecte
pub fn run_ws(
    app: AppHandle,
    channel_id: u64,
    chatroom_id: u64,
    cancel: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Identité éphémère réutilisée entre reconnexions (guest stable).
        let client_id = uuid::Uuid::new_v4().to_string();
        let channel = format!("chatrooms.{}.v2", chatroom_id);
        let mut tentatives: u32 = 0;

        'reconnect: loop {
            if cancel.load(Ordering::SeqCst) {
                break;
            }

            // 1. URL du serveur Centrifugo (choisi côté Kick, peut changer).
            let url = match fetch_chat_connection(channel_id, &client_id).await {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("[Kick] ERR chat connection: {}", e);
                    tentatives += 1;
                    if tentatives >= 5 {
                        break;
                    }
                    tokio::time::sleep(backoff(tentatives)).await;
                    continue;
                }
            };

            // 2. Jeton invité « guest » — REQUIS : Centrifugo répond
            //    « bad request » à un connect sans token (vérifié 2026-09-22).
            let token = match fetch_connection_token(&client_id).await {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[Kick] ERR jeton invité: {}", e);
                    tentatives += 1;
                    if tentatives >= 5 {
                        break;
                    }
                    tokio::time::sleep(backoff(tentatives)).await;
                    continue;
                }
            };

            // 3. Connexion WS au serveur Centrifugo (Schannel + Origin).
            let mut ws = match kick_ws_connect(&url).await {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("[Kick] ERR WS connect: {}", e);
                    tentatives += 1;
                    if tentatives >= 5 {
                        break;
                    }
                    tokio::time::sleep(backoff(tentatives)).await;
                    continue;
                }
            };
            tentatives = 0;

            let mut next_id: u64 = 1;
            let mut subscribed = false;
            let mut refresh_deadline: Option<tokio::time::Instant> = None;

            // Frame connect (protocole Centrifuge JSON) — jeton obligatoire.
            let connect_frame =
                serde_json::json!({"id": next_id, "connect": {"token": token, "name": "js"}});
            next_id += 1;
            if ws
                .send(WsMessage::Text(connect_frame.to_string().into()))
                .await
                .is_err()
            {
                tokio::time::sleep(backoff(1)).await;
                continue 'reconnect;
            }

            loop {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }

                tokio::select! {
                    // Refresh du jeton invité avant expiration (ttl serveur).
                    _ = async {
                        match refresh_deadline {
                            Some(d) => tokio::time::sleep_until(d).await,
                            None => std::future::pending().await,
                        }
                    } => {
                        refresh_deadline = None;
                        match fetch_connection_token(&client_id).await {
                            Ok(t) => {
                                let frame = serde_json::json!({"id": next_id, "refresh": {"token": t}});
                                next_id += 1;
                                if ws.send(WsMessage::Text(frame.to_string().into())).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                eprintln!("[Kick] ERR refresh jeton: {}", e);
                                break;
                            }
                        }
                    }

                    // Message entrant
                    msg = ws.next() => {
                        let Some(msg_result) = msg else {
                            break;
                        };
                        let frame = match msg_result {
                            Ok(f) => f,
                            Err(e) => {
                                eprintln!("[Kick] ERR WS frame: {}", e);
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
                                break;
                            }
                            _ => continue,
                        };

                        // Parser la trame Centrifuge (JSON)
                        let msg: serde_json::Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        // Ping applicatif serveur : "{}" → répondre "{}".
                        if msg.as_object().is_some_and(|o| o.is_empty()) {
                            let _ = ws.send(WsMessage::Text("{}".into())).await;
                            continue;
                        }

                        // Déconnexion demandée par le serveur.
                        if let Some(disc) = msg.get("disconnect") {
                            eprintln!("[Kick] disconnect serveur: {}", disc);
                            break;
                        }

                        // Erreur sur une commande (id + error).
                        if let Some(err) = msg.get("error") {
                            eprintln!("[Kick] ERR trame: {}", text);
                            let _ = err;
                            continue;
                        }

                        // Ack connect → programmer le refresh + subscribe.
                        if let Some(connect) = msg.get("connect") {
                            if connect["expires"].as_bool() == Some(true) {
                                if let Some(ttl) = connect["ttl"].as_u64() {
                                    refresh_deadline = Some(
                                        tokio::time::Instant::now()
                                            + std::time::Duration::from_secs(
                                                ttl.saturating_sub(60),
                                            ),
                                    );
                                }
                            }
                            let frame = serde_json::json!({
                                "id": next_id,
                                "subscribe": {"channel": &channel}
                            });
                            next_id += 1;
                            if ws.send(WsMessage::Text(frame.to_string().into())).await.is_err() {
                                break;
                            }
                            continue;
                        }

                        // Ack refresh → reprogrammer avec le nouveau ttl.
                        if let Some(refresh) = msg.get("refresh") {
                            if refresh["expires"].as_bool() == Some(true) {
                                if let Some(ttl) = refresh["ttl"].as_u64() {
                                    refresh_deadline = Some(
                                        tokio::time::Instant::now()
                                            + std::time::Duration::from_secs(
                                                ttl.saturating_sub(60),
                                            ),
                                    );
                                }
                            } else {
                                refresh_deadline = None;
                            }
                            continue;
                        }

                        // Ack subscribe → marquer connecté.
                        if msg.get("subscribe").is_some() {
                            if !subscribed {
                                subscribed = true;
                                if let Some(ks) = app.try_state::<KickState>() {
                                    ks.set_connected(true);
                                }
                                let _ = app.emit("kick:connecte", &channel);
                            }
                            continue;
                        }

                        // Push : publication sur un canal.
                        if let Some(push) = msg.get("push") {
                            let data = &push["pub"]["data"];
                            if data["event"].as_str() != Some("App\\Events\\ChatMessageEvent") {
                                continue;
                            }
                            // data.data est double-encodée (string JSON dans string JSON)
                            let data_str = data["data"].as_str().unwrap_or("{}");
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
                                    "kick",
                                    pseudo,
                                    pseudo,
                                    None,
                                    texte,
                                );
                            }
                            // Commandes chat : "!commande" → overlay diffusion.
                            if let Some(commandes) =
                                app.try_state::<crate::commandes::CommandesState>()
                            {
                                commandes.on_message(pseudo, pseudo, texte);
                            }
                            continue;
                        }
                    }
                }
            }

            // Fin de session WS : fermer proprement puis décider du sort.
            let _ = ws.close(None).await;
            if let Some(ks) = app.try_state::<KickState>() {
                ks.set_connected(false);
            }
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            let _ = app.emit("kick:deconnecte", ());
            // Session jamais subscribée = échec → compte dans le backoff.
            // Session saine qui se termine = reconnect libre (tentatives à 0).
            if !subscribed {
                tentatives += 1;
                if tentatives >= 5 {
                    break;
                }
            }
            tokio::time::sleep(backoff(tentatives.max(1))).await;
        }

        // Nettoyage final : emit déconnecté (la fermeture WS est déjà faite
        // en fin de session ; ce emit couvre aussi l'arrêt par cancel).
        if let Some(ks) = app.try_state::<KickState>() {
            ks.set_connected(false);
        }
        let _ = app.emit("kick:deconnecte", ());
    })
}
