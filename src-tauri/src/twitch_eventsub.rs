/// EventSub WebSocket Twitch — follows en temps réel (channel.follow v2).
///
/// Remplace le poll Helix 60 s des follows (retard moyen 30 s + jusqu'à
/// 10 GET/min) : Twitch PUSHE l'événement sur une connexion WebSocket
/// persistante. Démarré/arrêté avec la connexion Twitch (même jeton
/// d'annulation que l'IRC — voir twitch_connecter / stop_irc).
///
/// Protocole :
///   1. connect wss://eventsub.wss.twitch.tv:443/ws → `session_welcome`
///      (session_id + keepalive_seconds).
///   2. Abonner `channel.follow` v2 via Helix POST /eventsub/subscriptions
///      (scope moderator:read:followers déjà demandé à l'auth). Fenêtre
///      Twitch : dans les 10 s suivant le welcome, sinon socket fermée.
///      409 = abonnement identique déjà actif = OK (session_reconnect migre
///      les abonnements vers la nouvelle socket).
///   3. Boucle de lecture : `notification` (channel.follow) →
///      alertes.on_event + emit "chat:event" (parité IRC) ;
///      `session_keepalive` (~10 s) → read timeout = keepalive×2+30 ;
///      `session_reconnect` → reconnexion IMMÉDIATE à reconnect_url.
///
/// Reconnexion avec backoff (1 s → ×2 → max 30 s), arrêt propre via Cancel.
use crate::twitch_auth::{is_cancelled, Cancel};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio_tungstenite::tungstenite::Message;

const EVENTSUB_WS_URL: &str = "wss://eventsub.wss.twitch.tv:443/ws";

/// Démarre la tâche EventSub. Retourne le JoinHandle (pour arrêt propre).
/// `cancel` : même jeton que l'IRC — l'arrêt de la connexion Twitch arrête
/// les deux tâches.
pub fn demarrer(
    app: AppHandle,
    token: String,
    user_id: String,
    cancel: Cancel,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = 1u64;
        let mut url = EVENTSUB_WS_URL.to_string();
        loop {
            if is_cancelled(&cancel) {
                return;
            }
            match connect_session(&app, &token, &user_id, &cancel, &url).await {
                // Annulation propre (déconnexion Twitch).
                Ok(None) => return,
                // session_reconnect : reconnexion gracieuse, sans backoff.
                Ok(Some(reconnect_url)) => {
                    log::info!("[EventSub] session_reconnect → reconnexion immédiate");
                    url = reconnect_url;
                    backoff = 1; // session établie avec succès → reset
                }
                Err(e) => {
                    log::warn!("[EventSub] {} — reconnexion dans {}s", e, backoff);
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                        _ = await_cancel(&cancel) => return,
                    }
                    backoff = (backoff * 2).min(30);
                    url = EVENTSUB_WS_URL.to_string();
                }
            }
        }
    })
}

/// Une session EventSub : connect, welcome, abonnement follow, boucle de
/// lecture. Retourne Ok(None) si annulation propre, Ok(Some(url)) si Twitch
/// demande une reconnexion gracieuse (session_reconnect), Err sinon.
async fn connect_session(
    app: &AppHandle,
    token: &str,
    user_id: &str,
    cancel: &Cancel,
    url: &str,
) -> Result<Option<String>, String> {
    // 1. Connexion WebSocket
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| format!("connect: {}", e))?;
    let (mut write, mut read) = ws.split();

    // 2. session_welcome : session_id + keepalive (premier message).
    let text = match read.next().await {
        Some(Ok(Message::Text(t))) => t.to_string(),
        Some(Ok(Message::Ping(p))) => {
            // Le welcome ne peut pas être un ping — traiter et relire.
            let _ = write.send(Message::Pong(p)).await;
            match read.next().await {
                Some(Ok(Message::Text(t))) => t.to_string(),
                Some(Ok(_)) => return Err("welcome inattendu (non texte)".into()),
                Some(Err(e)) => return Err(format!("welcome: {}", e)),
                None => return Err("welcome absent (fin de stream)".into()),
            }
        }
        Some(Ok(_)) => return Err("welcome inattendu (non texte)".into()),
        Some(Err(e)) => return Err(format!("welcome: {}", e)),
        None => return Err("welcome absent (fin de stream)".into()),
    };
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("welcome parse: {}", e))?;
    let session = &v["payload"]["session"];
    let session_id = session["id"]
        .as_str()
        .ok_or("session_id absent")?
        .to_string();
    let keepalive_s = session["keepalive_seconds"].as_u64().unwrap_or(10).max(1);

    // 3. Abonnement channel.follow v2 (idempotent : 409 = déjà actif = OK).
    //    Fenêtre Twitch : immédiatement après le welcome (10 s max).
    crate::twitch_helix::eventsub_abonner_follows(&session_id, user_id, token)
        .await
        .map_err(err_txt)?;
    log::info!(
        "[EventSub] session {} prête — follows abonnés (keepalive {}s)",
        session_id,
        keepalive_s
    );

    // 4. Boucle de lecture. Read timeout = keepalive×2+30 : sans message
    //    (keepalive OU notification) pendant ce délai → socket morte.
    let timeout = Duration::from_secs(keepalive_s * 2 + 30);
    loop {
        let msg = tokio::select! {
            m = tokio::time::timeout(timeout, read.next()) => match m {
                Ok(msg) => msg,
                Err(_) => {
                    return Err(format!(
                        "silence > {}s (keepalive manquant — socket morte)",
                        timeout.as_secs()
                    ))
                }
            },
            _ = await_cancel(cancel) => {
                let _ = write.close().await;
                return Ok(None);
            }
        };
        let text = match msg {
            Some(Ok(Message::Text(t))) => t.to_string(),
            Some(Ok(Message::Ping(p))) => {
                let _ = write.send(Message::Pong(p)).await;
                continue;
            }
            Some(Ok(Message::Close(_))) => return Err("connexion fermée".into()),
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(format!("read: {}", e)),
            None => return Err("fin de stream".into()),
        };

        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue, // frame non-JSON ignorée
        };
        match v["metadata"]["message_type"].as_str().unwrap_or("") {
            "notification" => {
                if v["metadata"]["subscription_type"].as_str() == Some("channel.follow") {
                    let evt = &v["payload"]["event"];
                    let login = evt["user_login"].as_str().unwrap_or_default().to_string();
                    let pseudo = match evt["user_name"].as_str() {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => login.clone(),
                    };
                    let user_id_evt = evt["user_id"].as_str().unwrap_or_default().to_string();
                    let evt_alerte = crate::alertes::EventTwitch {
                        type_alerte: "follow".to_string(),
                        login,
                        pseudo,
                        user_id: user_id_evt,
                        nb_viewers: 0,
                        nb_bits: 0,
                        niveau_sub: String::new(),
                        nb_mois: 0,
                        destinataire: String::new(),
                        system_msg: String::new(),
                    };
                    // Même double diffusion que l'IRC (twitch_chat.rs).
                    let _ = app.emit("chat:event", &evt_alerte);
                    if let Some(alertes) = app.try_state::<crate::alertes::AlertesState>() {
                        alertes.on_event(evt_alerte);
                    }
                }
            }
            // Le keepalive réarme le timeout (message reçu = socket vivante).
            "session_keepalive" => {}
            "session_reconnect" => {
                return match v["payload"]["session"]["reconnect_url"].as_str() {
                    Some(u) => Ok(Some(u.to_string())),
                    None => Err("session_reconnect sans reconnect_url".into()),
                };
            }
            "revocation" => {
                log::warn!("[EventSub] abonnement révoqué : {}", text);
            }
            _ => {} // message_type non géré → ignoré
        }
    }
}

/// HelixError → texte loggable (HelixError n'implémente pas Display).
fn err_txt(e: crate::twitch_helix::HelixError) -> String {
    match e {
        crate::twitch_helix::HelixError::NeedReauth => "NeedReauth (scopes manquants)".into(),
        crate::twitch_helix::HelixError::Deconnecte => "Deconnecte (token invalide)".into(),
        crate::twitch_helix::HelixError::Autre(s) => s,
    }
}

/// Attend l'annulation (poll léger du flag — même pattern que twitch_chat.rs).
async fn await_cancel(cancel: &Cancel) {
    loop {
        if is_cancelled(cancel) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
