/// Chat IRC Twitch via WebSocket (wss://irc-ws.chat.twitch.tv:443).
///
/// Démarré seulement si connecté (token valide). Rejoint le canal du login
/// (#login). Parse PRIVMSG + tags IRCv3 (badges) → payload chat.
///
/// Double diffusion par message :
///   1. app.emit("chat:message", payload) → dashboard (store chatMessages).
///   2. chat_tx.send(json) → broadcast :4321 WS → diffusion.html.
///
/// Reconnexion avec backoff. Arrêt propre via Cancel (AtomicBool).
use crate::twitch_auth::{is_cancelled, Cancel, CLIENT_ID};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;

const IRC_URL: &str = "wss://irc-ws.chat.twitch.tv:443";
const HELIX_USERS_URL: &str = "https://api.twitch.tv/helix/users";

/// Payload chat émis vers le dashboard + la diffusion.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub plateforme: String,
    pub pseudo: String,
    pub texte: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badges: Option<String>,
    /// URL de l'avatar Helix (profile_image_url). None si non résolu.
    /// Récupéré via GET /users?login= sur cache miss (jamais par re-rendu).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// Démarre la tâche IRC. Retourne le JoinHandle (pour arrêt propre).
/// `cancel` : passer un AtomicBool partagé — set true pour arrêter.
pub fn demarrer(
    app: AppHandle,
    chat_tx: broadcast::Sender<String>,
    token: String,
    login: String,
    cancel: Cancel,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Client HTTP partagé + cache avatars (login lowercase → URL).
        // Le cache persiste across reconnexions d'une même tâche IRC :
        // un chatter n'est fetché qu'une seule fois à vie (pas par re-rendu).
        let client = reqwest::Client::new();
        let mut avatar_cache: HashMap<String, String> = HashMap::new();
        let mut backoff = 1u64;
        loop {
            if is_cancelled(&cancel) {
                eprintln!("[Twitch] IRC arrêt demandé");
                return;
            }
            match connect_irc(&app, &chat_tx, &client, &mut avatar_cache, &token, &login, &cancel).await {
                Ok(()) => {
                    eprintln!("[Twitch] IRC connexion terminée normalement");
                    return;
                }
                Err(e) => {
                    eprintln!("[Twitch] ERR {} — reconnexion dans {}s", e, backoff);
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(backoff)) => {}
                        _ = await_cancel(&cancel) => return,
                    }
                    backoff = (backoff * 2).min(30);
                }
            }
        }
    })
}

/// Une session IRC : connect, auth, join, boucle de lecture, PING/PONG.
/// Retourne Ok si la connexion se termine proprement (annulation).
async fn connect_irc(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    client: &reqwest::Client,
    avatar_cache: &mut HashMap<String, String>,
    token: &str,
    login: &str,
    cancel: &Cancel,
) -> Result<(), String> {
    eprintln!("[Twitch] IRC connecting...");
    let (ws, _) = tokio_tungstenite::connect_async(IRC_URL)
        .await
        .map_err(|e| format!("connect: {}", e))?;
    let (mut write, mut read) = ws.split();

    // Login en minuscules (Twitch exige NICK/JOIN lowercase).
    let login_lower = login.to_lowercase();

    // CAP REQ :twitch.tv/tags (pour les badges)
    write
        .send(Message::Text("CAP REQ :twitch.tv/tags".into()))
        .await
        .map_err(|e| format!("CAP: {}", e))?;
    // PASS oauth:<token>
    let pass = format!("PASS oauth:{}", token);
    write
        .send(Message::Text(pass.into()))
        .await
        .map_err(|e| format!("PASS: {}", e))?;
    // NICK <login lowercase>
    let nick = format!("NICK {}", login_lower);
    write
        .send(Message::Text(nick.into()))
        .await
        .map_err(|e| format!("NICK: {}", e))?;
    // JOIN #<login lowercase>
    let join = format!("JOIN #{}", login_lower);
    write
        .send(Message::Text(join.into()))
        .await
        .map_err(|e| format!("JOIN: {}", e))?;

    eprintln!("[Twitch] IRC joined #{}", login_lower);

    while let Some(msg) = read.next().await {
        if is_cancelled(cancel) {
            let _ = write.close().await;
            return Ok(());
        }
        let text = match msg {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Ping(p)) => {
                let _ = write.send(Message::Pong(p)).await;
                continue;
            }
            Ok(Message::Close(_)) => return Err("connexion fermée".into()),
            Ok(_) => continue,
            Err(e) => return Err(format!("read: {}", e)),
        };

        // Une frame IRC peut contenir plusieurs lignes séparées par \r\n.
        for line in text.split("\r\n") {
            if line.is_empty() {
                continue;
            }
            // PING :twitch.twitch.tv → PONG :twitch.twitch.tv
            if let Some(rest) = line.strip_prefix("PING ") {
                let pong = format!("PONG {}", rest);
                let _ = write.send(Message::Text(pong.into())).await;
                continue;
            }
            // PRIVMSG parse
            if let Some(mut payload) = parse_privmsg(line) {
                // Avatar Helix : cache par login (lowercase). Fetch sur miss
                // uniquement — jamais un GET par re-rendu. Échec → "" stocké
                // pour ne pas re-fetch à l'infini (message part sans avatar).
                let key = payload.pseudo.to_lowercase();
                if !avatar_cache.contains_key(&key) {
                    let url = match fetch_avatar(client, token, &key).await {
                        Ok(u) => u,
                        Err(e) => {
                            eprintln!("[Twitch] avatar miss login={} : {}", key, e);
                            String::new()
                        }
                    };
                    avatar_cache.insert(key.clone(), url);
                }
                payload.avatar = avatar_cache
                    .get(&key)
                    .cloned()
                    .filter(|s| !s.is_empty());
                eprintln!(
                    "[Twitch] PRIVMSG pseudo={} avatar={}",
                    payload.pseudo,
                    payload.avatar.as_deref().unwrap_or("(none)")
                );
                let json_msg = json!({
                    "type": "chat",
                    "message": payload,
                })
                .to_string();
                let _ = app.emit("chat:message", &payload);
                match chat_tx.send(json_msg) {
                    Ok(n) => eprintln!("[Twitch] WS chat envoyé pseudo={} receivers={}", payload.pseudo, n),
                    Err(_) => eprintln!("[Twitch] WS chat AUCUN client pseudo={}", payload.pseudo),
                }
            }
        }
    }

    Err("fin de stream".into())
}

/// Parse une ligne IRC PRIVMSG avec tags IRCv3.
/// Format : `@badges=...;... :<pseudo>!<pseudo>@<pseudo>.tmi.twitch.tv PRIVMSG #<chan> :<texte>`
/// Retourne ChatMessage si c'est un PRIVMSG, None sinon.
fn parse_privmsg(line: &str) -> Option<ChatMessage> {
    let mut rest = line;

    // Tags (commence par @)
    let mut badges: Option<String> = None;
    if let Some(stripped) = rest.strip_prefix('@') {
        let (tags, after) = stripped.split_once(' ')?;
        rest = after;
        // tags = "badges=subscriber/12;mod=0;..."
        for tag in tags.split(';') {
            if let Some(val) = tag.strip_prefix("badges=") {
                if !val.is_empty() {
                    badges = Some(val.to_string());
                }
            }
        }
    }

    // Préfixe :<pseudo>!...
    let (prefix, after) = rest.split_once(' ')?;
    let pseudo = prefix.strip_prefix(':').and_then(|p| {
        // garder la partie avant !
        p.split('!').next()
    })?;
    rest = after;

    // PRIVMSG #<chan> :<texte>
    let (cmd, trailing) = rest.split_once(" :")?;
    let cmd_parts: Vec<&str> = cmd.split_whitespace().collect();
    if cmd_parts.first() != Some(&"PRIVMSG") {
        return None;
    }
    let _channel = cmd_parts.get(1)?;
    let texte = trailing.to_string();

    Some(ChatMessage {
        plateforme: "twitch".to_string(),
        pseudo: pseudo.to_string(),
        texte,
        badges,
        avatar: None,
    })
}

/// Récupère l'avatar Helix (profile_image_url) d'un login.
/// `GET /users?login=<login>` avec `Authorization: Bearer <token>` +
/// `Client-Id: <CLIENT_ID>`. Retourne l'URL ou Err (l'appelant stocke "").
async fn fetch_avatar(
    client: &reqwest::Client,
    token: &str,
    login: &str,
) -> Result<String, String> {
    let resp = client
        .get(HELIX_USERS_URL)
        .header("Authorization", format!("Bearer {}", token))
        .header("Client-Id", CLIENT_ID)
        .query(&[("login", login)])
        .send()
        .await
        .map_err(|e| format!("Helix users: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Helix users HTTP {}: {}", status, body));
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Helix users parse: {}", e))?;

    let url = v["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|u| u["profile_image_url"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Helix users: data[0].profile_image_url absent".to_string())?;
    Ok(url)
}

async fn await_cancel(cancel: &Cancel) {
    loop {
        if is_cancelled(cancel) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
