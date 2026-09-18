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
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{broadcast, mpsc};
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
    /// Couleur du pseudo (tag IRCv3 `color` pour Twitch, hex #RRGGBB).
    /// None si la plateforme ne fournit pas de couleur → le frontend génère
    /// une couleur HSL déterministe depuis le hash du pseudo (fallback).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// ID du message (tag IRCv3 `id` pour Twitch, UUID). Utilisé pour la
    /// suppression de message via Helix DELETE /moderation/chat_messages.
    /// None pour les plateformes qui ne fournissent pas d'ID de message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Démarre la tâche IRC. Retourne le JoinHandle (pour arrêt propre).
/// `cancel` : passer un AtomicBool partagé — set true pour arrêter.
/// `cmd_rx` : canal de commandes IRC à envoyer (ex: "/clear", "/ban user").
///            None si pas de canal (rétrocompatibilité).
pub fn demarrer(
    app: AppHandle,
    chat_tx: broadcast::Sender<String>,
    token: String,
    login: String,
    cancel: Cancel,
    cmd_rx: Option<mpsc::Receiver<String>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Client HTTP partagé + cache infos user (login lowercase → (avatar, bio)).
        // Le cache persiste across reconnexions d'une même tâche IRC :
        // un chatter n'est fetché qu'une seule fois à vie (pas par re-rendu).
        let client = reqwest::Client::new();
        let mut avatar_cache: HashMap<String, (String, String)> = HashMap::new();
        let mut backoff = 1u64;
        let mut cmd_rx = cmd_rx;
        loop {
            if is_cancelled(&cancel) {
                return;
            }
            match connect_irc(&app, &chat_tx, &client, &mut avatar_cache, &token, &login, &cancel, cmd_rx.as_mut()).await {
                Ok(()) => {
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
/// `cmd_rx` : canal de commandes IRC à envoyer (ex: "/clear"). None si pas de canal.
/// Retourne Ok si la connexion se termine proprement (annulation).
#[allow(clippy::too_many_arguments)]
async fn connect_irc(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    client: &reqwest::Client,
    avatar_cache: &mut HashMap<String, (String, String)>,
    token: &str,
    login: &str,
    cancel: &Cancel,
    mut cmd_rx: Option<&mut mpsc::Receiver<String>>,
) -> Result<(), String> {
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

    loop {
        if is_cancelled(cancel) {
            let _ = write.close().await;
            return Ok(());
        }

        // Soit un message WebSocket arrive, soit une commande IRC à envoyer
        // (ex: "/clear" depuis la modale de modération). Les deux branches
        // utilisent le writer partagé. Une commande → continue (skip texte).
        let text = tokio::select! {
            msg = read.next() => match msg {
                Some(Ok(Message::Text(t))) => t.to_string(),
                Some(Ok(Message::Ping(p))) => {
                    let _ = write.send(Message::Pong(p)).await;
                    continue;
                }
                Some(Ok(Message::Close(_))) => return Err("connexion fermée".into()),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(format!("read: {}", e)),
                None => return Err("fin de stream".into()),
            },
            cmd = async {
                match &mut cmd_rx {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some(cmd) = cmd {
                    // Envoi via PRIVMSG : Twitch interprète les "/" comme commandes.
                    let irc_cmd = format!("PRIVMSG #{} :{}", login_lower, cmd);
                    let _ = write.send(Message::Text(irc_cmd.into())).await;
                }
                continue;
            }
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
            // USERNOTICE : événements de chaîne (sub/resub/subgift/raid).
            // Testé via la commande IRC réelle (pas contains — un message peut
            // contenir le mot "USERNOTICE"). Parsé AVANT PRIVMSG. Types non
            // gérés (ritual, announcement) → ignorés silencieusement.
            // ⚠️ Leçon ancienne app : USERNOTICE tombé dans un `_ =>` ignoré =
            // aucun raid/sub jamais émis (bug "raid sans alerte" en live).
            if irc_command(line) == "USERNOTICE" {
                if let Some(evt) = parse_usernotice(line) {
                    let _ = app.emit("chat:event", &evt);
                    if let Some(alertes) = app.try_state::<crate::alertes::AlertesState>() {
                        alertes.on_event(evt);
                    }
                }
                continue;
            }
            // PRIVMSG parse
            if let Some(mut payload) = parse_privmsg(line) {
                // Bits (cheers) : tag `bits=<n>` sur le PRIVMSG. Le message
                // continue de circuler normalement (un cheer EST un message).
                let tags = parse_irc_tags(line);
                let nb_bits = tags
                    .get("bits")
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0);
                if nb_bits > 0 {
                    let evt = crate::alertes::EventTwitch {
                        type_alerte: "bits".to_string(),
                        login: payload.pseudo.to_lowercase(),
                        pseudo: payload.pseudo.clone(),
                        user_id: tags.get("user-id").cloned().unwrap_or_default(),
                        nb_viewers: 0,
                        nb_bits,
                        niveau_sub: String::new(),
                        nb_mois: 0,
                        destinataire: String::new(),
                        system_msg: format!("{} a envoyé {} bits !", payload.pseudo, nb_bits),
                    };
                    let _ = app.emit("chat:event", &evt);
                    if let Some(alertes) = app.try_state::<crate::alertes::AlertesState>() {
                        alertes.on_event(evt);
                    }
                }
                // Avatar + bio Helix : cache par login (lowercase). Fetch sur miss
                // uniquement — jamais un GET par re-rendu. Échec → "" stocké
                // pour ne pas re-fetch à l'infini (message part sans avatar).
                let key = payload.pseudo.to_lowercase();
                if !avatar_cache.contains_key(&key) {
                    let infos = match fetch_user_infos(client, token, &key).await {
                        Ok(u) => u,
                        Err(e) => {
                            eprintln!("[Twitch] avatar miss login={} : {}", key, e);
                            (String::new(), String::new())
                        }
                    };
                    avatar_cache.insert(key.clone(), infos);
                }
                let (avatar_url, bio) = avatar_cache
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                payload.avatar = Some(avatar_url).filter(|s| !s.is_empty());
                let bio = Some(bio).filter(|s| !s.is_empty());
                let json_msg = json!({
                    "type": "chat",
                    "message": payload,
                })
                .to_string();
                let _ = app.emit("chat:message", &payload);
                let _ = chat_tx.send(json_msg);
                // Clip de bienvenue : détection 1er message d'un streamer enregistré.
                // Récupère WelcomeState depuis l'app (non-fatal si absent).
                if let Some(welcome) = app.try_state::<crate::welcome::WelcomeState>() {
                    welcome.on_message(
                        "twitch",
                        &payload.pseudo.to_lowercase(),
                        &payload.pseudo,
                        payload.avatar.clone(),
                        bio.clone(),
                    );
                }
                // Bandeau premier message : détection 1er message de n'importe
                // quel viewer (toutes plateformes). Non-fatal si absent.
                if let Some(bandeau) = app.try_state::<crate::bandeau::BandeauState>() {
                    bandeau.on_message(
                        "twitch",
                        &payload.pseudo,
                        &payload.pseudo,
                        payload.avatar.clone(),
                        &payload.texte,
                    );
                }
                // Commandes chat : "!commande" tapée par un viewer → overlay
                // diffusion (même moteur que les alertes). Non-fatal si absent.
                if let Some(commandes) = app.try_state::<crate::commandes::CommandesState>() {
                    commandes.on_message(&payload.pseudo, &payload.pseudo, &payload.texte);
                }
            }
        }
    }
}

/// Extrait la commande IRC d'une ligne (2e token : `[@tags] :prefix CMD ...`).
/// "PING :tmi.twitch.tv" → "PING". Ligne vide/malformée → "".
fn irc_command(line: &str) -> &str {
    let rest = match line.strip_prefix('@') {
        Some(r) => match r.split_once(' ') {
            Some((_, after)) => after,
            None => return "",
        },
        None => line,
    };
    let mut parts = rest.split_whitespace();
    let first = parts.next().unwrap_or("");
    if first.starts_with(':') {
        parts.next().unwrap_or("")
    } else {
        first
    }
}

/// Extrait les tags IRCv3 d'une ligne (`@k=v;k=v ...`) → HashMap.
/// Ligne sans tags → map vide. Valeurs vides ignorées (comme parse_privmsg).
fn parse_irc_tags(line: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(rest) = line.strip_prefix('@') else {
        return map;
    };
    let Some((tags, _)) = rest.split_once(' ') else {
        return map;
    };
    for tag in tags.split(';') {
        if let Some((k, v)) = tag.split_once('=') {
            if !v.is_empty() {
                map.insert(k.to_string(), v.to_string());
            }
        }
    }
    map
}

/// Parse une ligne IRC USERNOTICE (événements de chaîne Twitch).
/// Tag `msg-id` identifie le type : sub | resub | subgift | raid — les autres
/// (ritual, announcement, …) sont ignorés silencieusement. Tags `msg-param-*`
/// portent les détails (viewerCount raid, sub-plan, mois cumulés, destinataire).
fn parse_usernotice(line: &str) -> Option<crate::alertes::EventTwitch> {
    let tags = parse_irc_tags(line);
    let msg_id = tags.get("msg-id")?.as_str();
    let type_alerte = match msg_id {
        "sub" => "sub",
        "resub" => "resub",
        "subgift" => "subgift",
        "raid" => "raid",
        _ => return None,
    };

    // Login depuis le préfixe :<login>!<login>@<login>.tmi.twitch.tv
    let login = line
        .split(' ')
        .find(|p| p.starts_with(':'))
        .and_then(|p| p.strip_prefix(':'))
        .and_then(|p| p.split('!').next())
        .unwrap_or("")
        .to_string();
    let pseudo = tags
        .get("display-name")
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| login.clone());
    let user_id = tags.get("user-id").cloned().unwrap_or_default();

    let nb_viewers = if type_alerte == "raid" {
        tags.get("msg-param-viewerCount")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
    } else {
        0
    };
    let niveau_sub = if matches!(type_alerte, "sub" | "resub" | "subgift") {
        match tags.get("msg-param-sub-plan").map(|s| s.as_str()) {
            Some("Prime") => "Prime".to_string(),
            Some("1000") => "Tier 1".to_string(),
            Some("2000") => "Tier 2".to_string(),
            Some("3000") => "Tier 3".to_string(),
            _ => String::new(),
        }
    } else {
        String::new()
    };
    let nb_mois = if type_alerte == "resub" {
        tags.get("msg-param-cumulative-months")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
    } else {
        0
    };
    let destinataire = if type_alerte == "subgift" {
        tags.get("msg-param-recipient-display-name")
            .cloned()
            .unwrap_or_default()
    } else {
        String::new()
    };
    // system-msg : les espaces sont encodés \s dans IRCv3.
    let system_msg = tags
        .get("system-msg")
        .map(|s| s.replace("\\s", " "))
        .unwrap_or_default();

    Some(crate::alertes::EventTwitch {
        type_alerte: type_alerte.to_string(),
        login,
        pseudo,
        user_id,
        nb_viewers,
        nb_bits: 0,
        niveau_sub,
        nb_mois,
        destinataire,
        system_msg,
    })
}

/// Parse une ligne IRC PRIVMSG avec tags IRCv3.
/// Format : `@badges=...;... :<pseudo>!<pseudo>@<pseudo>.tmi.twitch.tv PRIVMSG #<chan> :<texte>`
/// Retourne ChatMessage si c'est un PRIVMSG, None sinon.
fn parse_privmsg(line: &str) -> Option<ChatMessage> {
    let mut rest = line;

    // Tags (commence par @)
    let mut badges: Option<String> = None;
    let mut color: Option<String> = None;
    let mut message_id: Option<String> = None;
    if let Some(stripped) = rest.strip_prefix('@') {
        let (tags, after) = stripped.split_once(' ')?;
        rest = after;
        // tags = "id=abc-123;badges=subscriber/12;color=#FF0000;mod=0;..."
        for tag in tags.split(';') {
            if let Some(val) = tag.strip_prefix("badges=") {
                if !val.is_empty() {
                    badges = Some(val.to_string());
                }
            } else if let Some(val) = tag.strip_prefix("color=") {
                if !val.is_empty() {
                    color = Some(val.to_string());
                }
            } else if let Some(val) = tag.strip_prefix("id=") {
                if !val.is_empty() {
                    message_id = Some(val.to_string());
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
        color,
        message_id,
    })
}

/// Récupère l'avatar (profile_image_url) + la bio (description) Helix d'un login.
/// `GET /users?login=<login>` avec `Authorization: Bearer <token>` +
/// `Client-Id: <CLIENT_ID>`. Retourne (avatar, bio) ou Err (l'appelant stocke "").
/// Pub : réutilisé par welcome_tester_clip (lib.rs) pour le test manuel.
pub async fn fetch_user_infos(
    client: &reqwest::Client,
    token: &str,
    login: &str,
) -> Result<(String, String), String> {
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

    let user = v["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| "Helix users: data[0] absent".to_string())?;
    let url = user["profile_image_url"].as_str().unwrap_or("").to_string();
    let bio = user["description"].as_str().unwrap_or("").to_string();
    Ok((url, bio))
}

async fn await_cancel(cancel: &Cancel) {
    loop {
        if is_cancelled(cancel) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
