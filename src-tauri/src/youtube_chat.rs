/// Chat YouTube Live — polling API (pas de WebSocket natif).
///
/// YouTube Live Chat se fait en polling HTTP : GET /liveChat/messages avec
/// pageToken + pollingIntervalMillis. Pas de WebSocket (le streamList gRPC
/// existe mais est complexe en Rust — on utilise le polling REST classique).
///
/// Étapes :
/// 1. GET /videos?part=liveStreamingDetails&mine=true → activeLiveChatId
///    (si pas live → retry 30s, emit youtube:erreur "Pas de live actif")
/// 2. Boucle polling GET /liveChat/messages?liveChatId=...&part=snippet,authorDetails
///    - Respecter pollingIntervalMillis (généralement 5-15s)
///    - Parser messages → ChatMessage { plateforme: "youtube", ... }
///    - Double diffusion : app.emit("chat:message") + chat_tx.send(json)
/// 3. Si plus de live (activeLiveChatId disparu) → retry resolve 30s
///
/// Reconnexion avec backoff. Arrêt propre via Cancel (AtomicBool).
use crate::twitch_chat::ChatMessage;
use crate::youtube_auth::{is_cancelled, Cancel};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

const YOUTUBE_API: &str = "https://www.googleapis.com/youtube/v3";

/// Démarre la tâche de polling chat. Retourne le JoinHandle (pour arrêt propre).
/// `cancel` : passer un AtomicBool partagé — set true pour arrêter.
pub fn demarrer(
    app: AppHandle,
    chat_tx: broadcast::Sender<String>,
    token: String,
    channel_id: String,
    cancel: Cancel,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut backoff = 1u64;
        loop {
            if is_cancelled(&cancel) {
                return;
            }
            match run_polling(&app, &chat_tx, &client, &token, &channel_id, &cancel).await {
                Ok(()) => {
                    return;
                }
                Err(e) => {
                    eprintln!("[YouTube] ERR {} — reconnexion dans {}s", e, backoff);
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

/// Une session de polling : resolve liveChatId → boucle polling → fin si plus live.
/// Toutes les branches retournent (pas de retry ici — la reconnexion est gérée
/// par l'appelant `demarrer`). NB : PAS de boucle externe — clippy never_loop.
async fn run_polling(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    client: &reqwest::Client,
    token: &str,
    channel_id: &str,
    cancel: &Cancel,
) -> Result<(), String> {
    if is_cancelled(cancel) {
        return Ok(());
    }

    // 1. Résoudre activeLiveChatId via search → videos
    let live_chat_id = match resolve_live_chat_id(client, token, channel_id).await {
        Ok(Some(id)) => {
            id
        }
        Ok(None) => {
            // Pas de live actif → arrêter la tâche (pas de retry automatique).
            // L'utilisateur relancera manuellement via le bouton "Chat live ON".
            let _ = app.emit("youtube:pas-de-live", ());
            return Ok(());
        }
        Err(e) => {
            return Err(format!("resolve live chat: {}", e));
        }
    };

    // 2. Boucle polling
    let mut page_token: Option<String> = None;
    loop {
        if is_cancelled(cancel) {
            return Ok(());
        }

        let (messages, next_token, poll_interval) =
            fetch_messages(client, token, &live_chat_id, page_token.as_deref()).await?;

        for msg in messages {
            let _ = app.emit("chat:message", &msg);
            let json_msg = json!({
                "type": "chat",
                "message": msg,
            })
            .to_string();
            let _ = chat_tx.send(json_msg);
            // Bandeau premier message : détection 1er message (tous viewers).
            if let Some(bandeau) = app.try_state::<crate::bandeau::BandeauState>() {
                bandeau.on_message(
                    "youtube",
                    &msg.pseudo,
                    &msg.pseudo,
                    msg.avatar.clone(),
                    &msg.texte,
                );
            }
            // Commandes chat : "!commande" → overlay diffusion.
            if let Some(commandes) = app.try_state::<crate::commandes::CommandesState>() {
                commandes.on_message(&msg.pseudo, &msg.pseudo, &msg.texte);
            }
        }

        page_token = next_token;

        // Respecter pollingIntervalMillis (défaut 5s si absent).
        let wait = poll_interval.unwrap_or(5000);
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(wait)) => {}
            _ = await_cancel(cancel) => return Ok(()),
        }

        // Si plus de pageToken → le live est terminé, arrêter la tâche.
        if page_token.is_none() {
            let _ = app.emit("youtube:pas-de-live", ());
            return Ok(());
        }
    }
}

/// Resolve activeLiveChatId : /search?channelId=...&eventType=live&type=video
/// → videoId → /videos?part=liveStreamingDetails&id=<videoId> → activeLiveChatId.
/// Retourne None si pas de live actif.
async fn resolve_live_chat_id(
    client: &reqwest::Client,
    token: &str,
    channel_id: &str,
) -> Result<Option<String>, String> {
    // 1. Search : trouver le video ID du live actuel
    let search_resp = client
        .get(format!("{}/search", YOUTUBE_API))
        .header("Authorization", format!("Bearer {}", token))
        .query(&[
            ("part", "id"),
            ("channelId", channel_id),
            ("eventType", "live"),
            ("type", "video"),
            ("maxResults", "1"),
        ])
        .send()
        .await
        .map_err(|e| format!("search GET: {}", e))?;

    if !search_resp.status().is_success() {
        let status = search_resp.status();
        let body = search_resp.text().await.unwrap_or_default();
        return Err(format!("search HTTP {}: {}", status, body));
    }

    let search_v: serde_json::Value = search_resp
        .json()
        .await
        .map_err(|e| format!("search parse: {}", e))?;

    let search_items = search_v["items"]
        .as_array()
        .ok_or("search: items absent")?;

    if search_items.is_empty() {
        return Ok(None); // pas de live
    }

    let video_id = search_items[0]["id"]["videoId"]
        .as_str()
        .ok_or("search: videoId absent")?;

    // 2. Videos : récupérer activeLiveChatId
    let video_resp = client
        .get(format!("{}/videos", YOUTUBE_API))
        .header("Authorization", format!("Bearer {}", token))
        .query(&[
            ("part", "liveStreamingDetails"),
            ("id", video_id),
        ])
        .send()
        .await
        .map_err(|e| format!("videos GET: {}", e))?;

    if !video_resp.status().is_success() {
        let status = video_resp.status();
        let body = video_resp.text().await.unwrap_or_default();
        return Err(format!("videos HTTP {}: {}", status, body));
    }

    let video_v: serde_json::Value = video_resp
        .json()
        .await
        .map_err(|e| format!("videos parse: {}", e))?;

    let video_items = video_v["items"]
        .as_array()
        .ok_or("videos: items absent")?;

    if video_items.is_empty() {
        return Ok(None);
    }

    let chat_id = video_items[0]["liveStreamingDetails"]["activeLiveChatId"]
        .as_str()
        .map(|s| s.to_string());

    Ok(chat_id)
}

/// GET /liveChat/messages?liveChatId=...&part=snippet,authorDetails
/// Retourne (messages, nextPageToken, pollingIntervalMillis).
async fn fetch_messages(
    client: &reqwest::Client,
    token: &str,
    live_chat_id: &str,
    page_token: Option<&str>,
) -> Result<(Vec<ChatMessage>, Option<String>, Option<u64>), String> {
    let mut query = vec![
        ("liveChatId", live_chat_id.to_string()),
        ("part", "snippet,authorDetails".to_string()),
        ("maxResults", "200".to_string()),
    ];
    if let Some(pt) = page_token {
        query.push(("pageToken", pt.to_string()));
    }

    let resp = client
        .get(format!("{}/liveChat/messages", YOUTUBE_API))
        .header("Authorization", format!("Bearer {}", token))
        .query(&query)
        .send()
        .await
        .map_err(|e| format!("liveChat GET: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("liveChat HTTP {}: {}", status, body));
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("liveChat parse: {}", e))?;

    let poll_interval = v["pollingIntervalMillis"]
        .as_u64();

    let next_token = v["nextPageToken"]
        .as_str()
        .map(|s| s.to_string());

    let items = v["items"]
        .as_array()
        .ok_or("liveChat: items absent")?;

    let mut messages = Vec::new();
    for item in items {
        // Filtrer : seulement textMessageEvent (ignorer superChat, newSponsor, etc.)
        let msg_type = item["snippet"]["type"]
            .as_str()
            .unwrap_or("");
        if msg_type != "textMessageEvent" {
            continue;
        }

        let pseudo = item["authorDetails"]["displayName"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let texte = item["snippet"]["textMessageDetails"]["messageText"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if texte.is_empty() {
            continue;
        }

        let avatar = item["authorDetails"]["profileImageUrl"]
            .as_str()
            .map(|s| s.to_string());

        // Badges : isChatModerator, isChatOwner, isChatSponsor
        let mut badges: Vec<&str> = Vec::new();
        if item["authorDetails"]["isChatModerator"].as_bool() == Some(true) {
            badges.push("moderator");
        }
        if item["authorDetails"]["isChatOwner"].as_bool() == Some(true) {
            badges.push("broadcaster");
        }
        if item["authorDetails"]["isChatSponsor"].as_bool() == Some(true) {
            badges.push("subscriber");
        }
        let badges_str = if badges.is_empty() {
            None
        } else {
            Some(badges.join(","))
        };

        messages.push(ChatMessage {
            plateforme: "youtube".to_string(),
            pseudo,
            texte,
            badges: badges_str,
            avatar,
            color: None,
            message_id: None,
        });
    }

    Ok((messages, next_token, poll_interval))
}

async fn await_cancel(cancel: &Cancel) {
    loop {
        if is_cancelled(cancel) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
