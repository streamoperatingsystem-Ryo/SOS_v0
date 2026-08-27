/// Client OBS WebSocket v5 — one-shot : connecte, authentifie, s'assure que
/// la scène "SOS" et la source navigateur "SOS-Diffusion" (1920×1080, URL :4321)
/// existent sans doublon, puis se déconnecte.
///
/// IMPORTANT : la scène et la source ont des noms DIFFÉRENTS pour éviter
/// l'ambiguïté côté OBS (GetSceneItemList attend un nom de scène, pas d'input).
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::Message;

const SCENE_NAME: &str = "SOS";
const SOURCE_NAME: &str = "SOS-Diffusion";
const SOURCE_URL: &str = "http://127.0.0.1:4321/";
const SOURCE_W: u32 = 1920;
const SOURCE_H: u32 = 1080;

/// Connecte à OBS WebSocket, authentifie, et s'assure que la scène "SOS"
/// + source navigateur "SOS-Diffusion" existent — idempotent, sans doublon.
pub async fn connect_and_setup(host: &str, port: u16, password: &str) -> Result<(), String> {
    let url = format!("ws://{}:{}", host, port);
    let (ws_stream, _response) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("Connexion OBS échouée ({}): {}", url, e))?;

    let (mut write, mut read) = ws_stream.split();

    // 1. Hello (op=0)
    let hello = recv_msg(&mut read).await?;
    if hello["op"] != 0 {
        return Err(format!("OBS: attendu Hello (op=0), reçu op={}", hello["op"]));
    }

    // 2. Identify (op=1) — avec auth si le serveur exige un mot de passe
    let identify = if let Some(auth) = hello["d"]["authentication"].as_object() {
        let challenge = auth["challenge"]
            .as_str()
            .ok_or("OBS: challenge d'auth manquant")?;
        let salt = auth["salt"]
            .as_str()
            .ok_or("OBS: salt d'auth manquant")?;

        // secret = base64(sha256(password + salt))
        let mut h1 = Sha256::new();
        h1.update(password.as_bytes());
        h1.update(salt.as_bytes());
        let secret = STANDARD.encode(h1.finalize());

        // response = base64(sha256(secret + challenge))
        let mut h2 = Sha256::new();
        h2.update(secret.as_bytes());
        h2.update(challenge.as_bytes());
        let response = STANDARD.encode(h2.finalize());

        json!({ "op": 1, "d": { "rpcVersion": 1, "authentication": response } })
    } else {
        json!({ "op": 1, "d": { "rpcVersion": 1 } })
    };
    send_msg(&mut write, &identify).await?;

    // 3. Identified (op=2) — sinon auth échouée
    let identified = recv_msg(&mut read).await?;
    if identified["op"] != 2 {
        return Err("OBS: authentification échouée (mot de passe incorrect ?)".into());
    }

    // 4. GetSceneList → trouver la scène "SOS" + son sceneUuid
    let scenes_resp = rpc(
        &mut write,
        &mut read,
        "GetSceneList",
        "get_scenes",
        json!({}),
    )
    .await?;

    let scene_obj = scenes_resp["scenes"]
        .as_array()
        .and_then(|scenes| scenes.iter().find(|s| s["sceneName"] == SCENE_NAME));

    let scene_uuid: String = if let Some(s) = scene_obj {
        let uuid = s["sceneUuid"]
            .as_str()
            .unwrap_or("")
            .to_string();
        log::info!("OBS: scène \"{}\" trouvée, uuid={}", SCENE_NAME, uuid);
        uuid
    } else {
        // Créer la scène
        let create_resp = rpc(
            &mut write,
            &mut read,
            "CreateScene",
            "create_scene",
            json!({ "sceneName": SCENE_NAME }),
        )
        .await?;
        let uuid = create_resp["sceneUuid"]
            .as_str()
            .unwrap_or("")
            .to_string();
        log::info!("OBS: scène \"{}\" créée, uuid={}", SCENE_NAME, uuid);
        uuid
    };

    // 5. GetSceneItemList avec sceneName + sceneUuid — NON FATAL
    //    Si échec : logger et continuer (source_in_scene = false)
    let mut source_in_scene = false;
    let items_result = rpc(
        &mut write,
        &mut read,
        "GetSceneItemList",
        "get_items",
        json!({
            "sceneName": SCENE_NAME,
            "sceneUuid": scene_uuid
        }),
    )
    .await;

    match &items_result {
        Ok(items_resp) => {
            source_in_scene = items_resp["sceneItems"]
                .as_array()
                .map(|items| items.iter().any(|i| i["sourceName"] == SOURCE_NAME))
                .unwrap_or(false);
            log::info!(
                "OBS: scène \"{}\" — {} item(s), source \"{}\" présente: {}",
                SCENE_NAME,
                items_resp["sceneItems"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0),
                SOURCE_NAME,
                source_in_scene
            );
        }
        Err(e) => {
            // GetSceneItemList échoue MAIS la scène existe → ne PAS abort
            log::warn!(
                "OBS: GetSceneItemList échoué (scène \"{}\" existe pourtant): {} — on continue",
                SCENE_NAME,
                e
            );
        }
    }

    // 6. GetInputList → input "SOS-Diffusion" existe ?
    let inputs_resp = rpc(
        &mut write,
        &mut read,
        "GetInputList",
        "get_inputs",
        json!({}),
    )
    .await?;
    let input_exists = inputs_resp["inputs"]
        .as_array()
        .map(|inputs| inputs.iter().any(|i| i["inputName"] == SOURCE_NAME))
        .unwrap_or(false);

    if input_exists {
        // Input existe → SetInputSettings (sync URL/taille) — erreur fatale
        rpc(
            &mut write,
            &mut read,
            "SetInputSettings",
            "set_input_settings",
            json!({
                "inputName": SOURCE_NAME,
                "inputSettings": {
                    "url": SOURCE_URL,
                    "width": SOURCE_W,
                    "height": SOURCE_H
                }
            }),
        )
        .await?;
        log::info!("OBS: input \"{}\" existant — settings synchronisés", SOURCE_NAME);

        // Si pas dans la scène SOS → AddSceneItem avec sceneName + sceneUuid
        if !source_in_scene {
            rpc(
                &mut write,
                &mut read,
                "AddSceneItem",
                "add_item",
                json!({
                    "sceneName": SCENE_NAME,
                    "sceneUuid": scene_uuid,
                    "sourceName": SOURCE_NAME,
                    "sceneItemEnabled": true
                }),
            )
            .await?;
            log::info!(
                "OBS: input \"{}\" ajouté à la scène \"{}\"",
                SOURCE_NAME,
                SCENE_NAME
            );
        }
    } else {
        // Input absent → CreateInput OBLIGATOIRE avec sceneName + sceneUuid
        rpc(
            &mut write,
            &mut read,
            "CreateInput",
            "create_input",
            json!({
                "sceneName": SCENE_NAME,
                "sceneUuid": scene_uuid,
                "inputName": SOURCE_NAME,
                "inputKind": "browser_source",
                "inputSettings": {
                    "url": SOURCE_URL,
                    "width": SOURCE_W,
                    "height": SOURCE_H
                },
                "sceneItemEnabled": true
            }),
        )
        .await?;
        log::info!(
            "OBS: input \"{}\" créé (browser_source) dans la scène \"{}\"",
            SOURCE_NAME,
            SCENE_NAME
        );
    }

    // 7. Refresh de la source navigateur (PressInputPropertiesButton)
    //    Non-fatal : si échec, on log mais on ne faille pas la connexion.
    let refresh_result = rpc(
        &mut write,
        &mut read,
        "PressInputPropertiesButton",
        "refresh",
        json!({
            "inputName": SOURCE_NAME,
            "propertyName": "refreshnocache"
        }),
    )
    .await;

    match &refresh_result {
        Ok(_) => log::info!("OBS: source \"{}\" rafraîchie (refreshnocache)", SOURCE_NAME),
        Err(e) => log::warn!("OBS: refresh \"{}\" échoué (non-fatal): {}", SOURCE_NAME, e),
    }

    let _ = write.close().await;
    Ok(())
}

// --- Helpers WebSocket ---

async fn send_msg<W>(write: &mut W, msg: &Value) -> Result<(), String>
where
    W: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let text = msg.to_string();
    write
        .send(Message::Text(text.into()))
        .await
        .map_err(|e| format!("OBS send: {}", e))?;
    Ok(())
}

/// Lit le prochain message JSON (ignore ping/pong/binary). Erreur si Close.
async fn recv_msg<R>(read: &mut R) -> Result<Value, String>
where
    R: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let next = read
            .next()
            .await
            .ok_or("OBS: connexion fermée")?
            .map_err(|e| format!("OBS read: {}", e))?;
        match next {
            Message::Text(t) => {
                let v: Value =
                    serde_json::from_str(&t).map_err(|e| format!("OBS parse: {}", e))?;
                return Ok(v);
            }
            Message::Close(_) => {
                return Err("OBS: connexion fermée par le serveur".into());
            }
            _ => {} // ping, pong, binary — ignorés
        }
    }
}

/// Envoie une requête RPC (op=6) et attend la réponse correspondante (op=7).
/// Ignore les événements (op=5) reçus entre-temps.
/// Log la requête ET la réponse (status + comment) pour le debug.
async fn rpc<W, R>(
    write: &mut W,
    read: &mut R,
    request_type: &str,
    request_id: &str,
    request_data: Value,
) -> Result<Value, String>
where
    W: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    R: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let msg = json!({
        "op": 6,
        "d": {
            "requestType": request_type,
            "requestId": request_id,
            "requestData": request_data
        }
    });
    log::info!("OBS → {} ({}): {}", request_type, request_id, msg["d"]["requestData"]);
    send_msg(write, &msg).await?;

    loop {
        let v = recv_msg(read).await?;
        if v["op"] == 7 && v["d"]["requestId"] == request_id {
            let status = &v["d"]["requestStatus"];
            let result = status["result"].as_bool() == Some(true);
            let comment = status["comment"]
                .as_str()
                .unwrap_or("");
            log::info!(
                "OBS ← {} ({}): result={}, comment=\"{}\", data={}",
                request_type,
                request_id,
                result,
                comment,
                v["d"]["responseData"]
            );
            if result {
                return Ok(v["d"]["responseData"].clone());
            } else {
                return Err(format!(
                    "OBS {} échec: {}",
                    request_type,
                    if comment.is_empty() {
                        "erreur inconnue"
                    } else {
                        comment
                    }
                ));
            }
        }
        // op=5 (events) ou autres — ignorés
    }
}
