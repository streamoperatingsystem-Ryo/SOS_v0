/// Client OBS WebSocket v5 — one-shot : connecte, authentifie, lit la
/// résolution de canvas OBS (GetVideoSettings → baseWidth/baseHeight, fallback
/// 1920×1080), s'assure que la scène "SOS" et la source navigateur
/// "SOS-Diffusion" (URL :4321, dims = résolution OBS) existent sans doublon,
/// active la scène "SOS" en program SAUF si un stream/record tourne (garde
/// live : jamais de bascule à l'antenne), puis se déconnecte.
/// Retourne (canvasW, canvasH).
///
/// IMPORTANT : la scène et la source ont des noms DIFFÉRENTS pour éviter
/// l'ambiguïté côté OBS (GetSceneItemList attend un nom de scène, pas d'input).
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::MaybeTlsStream;

pub(crate) type WsSink = futures_util::stream::SplitSink<
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
pub(crate) type WsStreamHalf = futures_util::stream::SplitStream<
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
>;

pub(crate) const SCENE_NAME: &str = "SOS";
pub(crate) const SOURCE_NAME: &str = "SOS-Diffusion";
const SOURCE_URL: &str = "http://127.0.0.1:4321/";
const FALLBACK_W: u32 = 1920;
const FALLBACK_H: u32 = 1080;

/// Connecte a OBS WebSocket et authentifie. Retourne (write, read) authentifie.
/// Helper partage par connect_and_setup et obs_trou::*.
pub(crate) async fn connect(
    host: &str,
    port: u16,
    password: &str,
) -> Result<(WsSink, WsStreamHalf), String> {
    let url = format!("ws://{}:{}", host, port);
    let (ws_stream, _response) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("Connexion OBS echouee ({}): {}", url, e))?;

    let (mut write, mut read) = ws_stream.split();

    // 1. Hello (op=0)
    let hello = recv_msg(&mut read).await?;
    if hello["op"] != 0 {
        return Err(format!("OBS: attendu Hello (op=0), recu op={}", hello["op"]));
    }

    // 2. Identify (op=1) avec auth si le serveur exige un mot de passe
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

    // 3. Identified (op=2) sinon auth echouee
    let identified = recv_msg(&mut read).await?;
    if identified["op"] != 2 {
        return Err("OBS: authentification echouee (mot de passe incorrect ?)".into());
    }

    Ok((write, read))
}

/// Connecte à OBS WebSocket, authentifie, lit la résolution OBS, et s'assure
/// que la scène "SOS" + source navigateur "SOS-Diffusion" (dims = résolution
/// OBS) existent — idempotent, sans doublon. Retourne (canvasW, canvasH).
pub async fn connect_and_setup(host: &str, port: u16, password: &str) -> Result<(u32, u32), String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    // 4. GetVideoSettings → baseWidth/baseHeight (résolution canvas OBS).
    //    Non-fatal : si échec, fallback 1920×1080. Pas de poll — lu une seule
    //    fois au connect. Les widgets existants ne sont pas rescalés.
    let (canvas_w, canvas_h) = match rpc(
        &mut write,
        &mut read,
        "GetVideoSettings",
        "video_settings",
        json!({}),
    )
    .await
    {
        Ok(data) => {
            let w = data["baseWidth"].as_u64().unwrap_or(0) as u32;
            let h = data["baseHeight"].as_u64().unwrap_or(0) as u32;
            if w > 0 && h > 0 {
                log::info!("OBS: résolution canvas = {}×{}", w, h);
                (w, h)
            } else {
                log::warn!(
                    "OBS: GetVideoSettings OK mais baseWidth/baseHeight invalides ({}, {}) → fallback {}×{}",
                    w, h, FALLBACK_W, FALLBACK_H
                );
                (FALLBACK_W, FALLBACK_H)
            }
        }
        Err(e) => {
            log::warn!(
                "OBS: GetVideoSettings échoué: {} → fallback {}×{}",
                e, FALLBACK_W, FALLBACK_H
            );
            (FALLBACK_W, FALLBACK_H)
        }
    };

    // 5. GetSceneList → trouver la scène "SOS" + son sceneUuid
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

    // 6. GetSceneItemList avec sceneName + sceneUuid — NON FATAL
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

    // 7. GetInputList → input "SOS-Diffusion" existe ?
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
                    "width": canvas_w,
                    "height": canvas_h
                }
            }),
        )
        .await?;
        log::info!(
            "OBS: input \"{}\" existant — settings synchronisés ({}×{})",
            SOURCE_NAME, canvas_w, canvas_h
        );

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
                    "width": canvas_w,
                    "height": canvas_h
                },
                "sceneItemEnabled": true
            }),
        )
        .await?;
        log::info!(
            "OBS: input \"{}\" créé (browser_source, {}×{}) dans la scène \"{}\"",
            SOURCE_NAME, canvas_w, canvas_h, SCENE_NAME
        );
    }

    // 7bis. Reset du transform du scene item "SOS-Diffusion" : position (0,0),
    //    scale 1, rotation 0, crop 0, alignement haut-gauche, sans bounds.
    //    Garantit un alignement pixel-perfect avec le canvas OBS même si la
    //    source a été déplacée/redimensionnée à la main dans OBS, ou si la
    //    résolution du canvas OBS a changé depuis la création de la source.
    //    Non-fatal : si échec, on log et on continue.
    let transform_result = async {
        let id_resp = rpc(
            &mut write,
            &mut read,
            "GetSceneItemId",
            "get_item_id",
            json!({
                "sceneName": SCENE_NAME,
                "sceneUuid": scene_uuid,
                "sourceName": SOURCE_NAME
            }),
        )
        .await?;
        let item_id = id_resp["sceneItemId"]
            .as_i64()
            .ok_or_else(|| "OBS: sceneItemId manquant".to_string())?;
        rpc(
            &mut write,
            &mut read,
            "SetSceneItemTransform",
            "set_item_transform",
            json!({
                "sceneName": SCENE_NAME,
                "sceneUuid": scene_uuid,
                "sceneItemId": item_id,
                "sceneItemTransform": {
                    "positionX": 0.0,
                    "positionY": 0.0,
                    "scaleX": 1.0,
                    "scaleY": 1.0,
                    "rotation": 0.0,
                    "alignment": 5,
                    "cropLeft": 0,
                    "cropRight": 0,
                    "cropTop": 0,
                    "cropBottom": 0,
                    "boundsType": "OBS_BOUNDS_NONE"
                }
            }),
        )
        .await
    }
    .await;

    match &transform_result {
        Ok(_) => log::info!(
            "OBS: transform de \"{}\" resynchronisé (0,0 / scale 1 / crop 0)",
            SOURCE_NAME
        ),
        Err(e) => log::warn!(
            "OBS: reset transform \"{}\" échoué (non-fatal): {}",
            SOURCE_NAME, e
        ),
    }

    // 8. Refresh de la source navigateur (PressInputPropertiesButton)
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

    // 9. Activer la scène "SOS" côté OBS (SetCurrentProgramScene) : créée dans
    //    la liste ne suffit pas — sans bascule, l'utilisateur reste sur sa
    //    scène courante et ne voit rien du setup. GARDE LIVE : si un stream
    //    OU un record tourne, on ne touche jamais le program (une bascule =
    //    coupure à l'antenne). Non-fatal : échec → warn + on termine quand
    //    même la connexion en "connected".
    let activation = async {
        let stream = rpc(
            &mut write,
            &mut read,
            "GetStreamStatus",
            "stream_status_scene",
            json!({}),
        )
        .await?;
        let record = rpc(
            &mut write,
            &mut read,
            "GetRecordStatus",
            "record_status_scene",
            json!({}),
        )
        .await?;
        let live = stream["outputActive"].as_bool().unwrap_or(false)
            || record["outputActive"].as_bool().unwrap_or(false);
        if !live {
            rpc(
                &mut write,
                &mut read,
                "SetCurrentProgramScene",
                "set_program_scene",
                json!({ "sceneName": SCENE_NAME }),
            )
            .await?;
        }
        Ok::<bool, String>(live)
    }
    .await;
    match activation {
        Ok(true) => log::info!(
            "OBS: stream/record actif → scène \"{}\" NON activée (program préservé)",
            SCENE_NAME
        ),
        Ok(false) => log::info!("OBS: scène \"{}\" activée (program)", SCENE_NAME),
        Err(e) => log::warn!(
            "OBS: activation scène \"{}\" échouée (non-fatal): {}",
            SCENE_NAME,
            e
        ),
    }

    let _ = write.close().await;
    Ok((canvas_w, canvas_h))
}

/// Refresh standalone de la source navigateur "SOS-Diffusion" via
/// PressInputPropertiesButton (refreshnocache) — équivalent au clic
/// "Actualiser" d'OBS. One-shot : connect → refresh → close.
/// Appelé quand serveur :4321 ready + OBS connecté (boot + reconnect).
pub async fn refresh_diffusion(host: &str, port: u16, password: &str) -> Result<(), String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    let refresh_result = rpc(
        &mut write,
        &mut read,
        "PressInputPropertiesButton",
        "refresh_diffusion",
        json!({
            "inputName": SOURCE_NAME,
            "propertyName": "refreshnocache"
        }),
    )
    .await;

    let _ = write.close().await;

    refresh_result.map(|_| ())
}

/// Met le streaming OBS dans l'état demandé. One-shot : connect → statut →
/// Start/Stop UNIQUEMENT si nécessaire → re-statut → close. JAMAIS
/// ToggleStream : la direction vient de l'UI (l'utilisateur a confirmé
/// « Démarrer » ou « Arrêter ») — un toggle agit sur l'état réel d'OBS,
/// potentiellement l'inverse du libellé affiché si OBS a été touché
/// entre-temps (bouton OBS, stream tombé, record stoppé hors StreamOS).
/// Idempotent : déjà dans l'état voulu → aucune action envoyée.
/// Retourne outputActive RELU (Start*/Stop* répondent un responseData vide).
pub async fn set_stream(host: &str, port: u16, password: &str, actif: bool) -> Result<bool, String> {
    let (mut write, mut read) = connect(host, port, password).await?;
    let result = set_output(
        &mut write,
        &mut read,
        "GetStreamStatus",
        if actif { "StartStream" } else { "StopStream" },
        actif,
        "stream",
    )
    .await;
    let _ = write.close().await;
    result
}

/// Met l'enregistrement OBS dans l'état demandé. Même contrat que
/// set_stream (Start/StopRecord, idempotent, état relu après action).
pub async fn set_record(host: &str, port: u16, password: &str, actif: bool) -> Result<bool, String> {
    let (mut write, mut read) = connect(host, port, password).await?;
    let result = set_output(
        &mut write,
        &mut read,
        "GetRecordStatus",
        if actif { "StartRecord" } else { "StopRecord" },
        actif,
        "record",
    )
    .await;
    let _ = write.close().await;
    result
}

/// Lit le statut d'une sortie OBS et n'envoie action_type QUE si elle n'est
/// pas déjà dans l'état voulu, puis relit le statut. Helper partagé de
/// set_stream / set_record — même connexion one-shot, requestIds préfixés.
async fn set_output(
    write: &mut WsSink,
    read: &mut WsStreamHalf,
    status_type: &str,
    action_type: &str,
    voulu: bool,
    id_prefix: &str,
) -> Result<bool, String> {
    let status = rpc(
        write,
        read,
        status_type,
        &format!("{}_status_avant", id_prefix),
        json!({}),
    )
    .await?;
    if status["outputActive"].as_bool().unwrap_or(false) == voulu {
        return Ok(voulu);
    }
    rpc(
        write,
        read,
        action_type,
        &format!("{}_action", id_prefix),
        json!({}),
    )
    .await?;
    let status = rpc(
        write,
        read,
        status_type,
        &format!("{}_status_apres", id_prefix),
        json!({}),
    )
    .await?;
    Ok(status["outputActive"].as_bool().unwrap_or(false))
}

/// Lit l'état des sorties OBS (GetStreamStatus + GetRecordStatus) en UNE
/// connexion. One-shot : connect → 2 requests → close.
/// Retourne (stream_actif, record_actif). outputPaused n'est pas exposé :
/// le dashboard n'affiche que actif/inactif.
pub async fn outputs_status(
    host: &str,
    port: u16,
    password: &str,
) -> Result<(bool, bool), String> {
    let (mut write, mut read) = connect(host, port, password).await?;
    let stream = rpc(
        &mut write,
        &mut read,
        "GetStreamStatus",
        "stream_status",
        json!({}),
    )
    .await
    .map(|d| d["outputActive"].as_bool().unwrap_or(false));
    let record = rpc(
        &mut write,
        &mut read,
        "GetRecordStatus",
        "record_status",
        json!({}),
    )
    .await
    .map(|d| d["outputActive"].as_bool().unwrap_or(false));
    let _ = write.close().await;
    Ok((stream?, record?))
}

/// Liste les moniteurs vus par OBS (GetMonitorList) — sélecteur de la section
/// Prévisualisation de la Toolbar. One-shot : connect → request → close.
/// ⚠️ Les index sont ceux d'OBS (monitorIndex), PAS ceux de Win32
/// (raccourcis_moniteurs) — le select doit être alimenté par CETTE liste.
/// Retourne le tableau brut { monitorIndex, monitorName, monitorWidth,
/// monitorHeight, monitorPositionX, monitorPositionY } sérialisé tel quel.
pub async fn moniteurs_liste(
    host: &str,
    port: u16,
    password: &str,
) -> Result<Vec<Value>, String> {
    let (mut write, mut read) = connect(host, port, password).await?;
    let result = rpc(
        &mut write,
        &mut read,
        "GetMonitorList",
        "get_monitors",
        json!({}),
    )
    .await;
    let _ = write.close().await;
    let data = result?;
    Ok(data["monitors"].as_array().cloned().unwrap_or_default())
}

/// Ouvre un projecteur OBS sur la prévisualisation — équivalent du clic droit
/// « Ouvrir le projecteur (prévisualisation) » d'OBS. Requête
/// OpenVideoMixProjector avec mix OBS_VIDEOMIX_TYPE_PREVIEW.
/// monitor_index = index OBS (moniteurs_liste) ; -1 = projecteur fenêtré.
/// One-shot : connect → request → close. Ouvre un projecteur (ce n'est PAS un
/// toggle : le fermer se fait dans Windows/OBS, pas via cette API).
pub async fn ouvrir_projecteur_preview(
    host: &str,
    port: u16,
    password: &str,
    monitor_index: i32,
) -> Result<(), String> {
    // Snapshot AVANT : la fenêtre projecteur est créée en différé sur le
    // thread UI d'OBS → on repérera la NOUVELLE fenêtre OBSProjector pour la
    // remonter devant (elle s'ouvre sinon derrière le dashboard).
    #[cfg(windows)]
    let avant = hwnds_projecteurs();

    let (mut write, mut read) = connect(host, port, password).await?;
    let result = rpc(
        &mut write,
        &mut read,
        "OpenVideoMixProjector",
        "open_preview_projector",
        json!({
            // Enum obs-websocket : OBS_WEBSOCKET_VIDEO_MIX_TYPE_* (PREVIEW /
            // PROGRAM / MULTIVIEW) — PAS OBS_VIDEOMIX_TYPE_* (refusé :
            // "invalid enum value").
            "videoMixType": "OBS_WEBSOCKET_VIDEO_MIX_TYPE_PREVIEW",
            "monitorIndex": monitor_index
        }),
    )
    .await;
    let _ = write.close().await;
    result?;

    #[cfg(windows)]
    {
        // Poll ~2 s : la fenêtre apparaît quand la file UI d'OBS la traite.
        // Cible = nouvelle fenêtre OBSProjector, fallback dernière connue
        // (cas d'un projecteur déjà ouvert → re-raise, pas de création).
        for _ in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let apres = hwnds_projecteurs();
            let cible = apres
                .iter()
                .copied()
                .find(|h| !avant.contains(h))
                .or_else(|| apres.last().copied());
            if let Some(h) = cible {
                projecteur_premier_plan(h);
                break;
            }
        }
    }
    Ok(())
}

/// HWND des fenêtres projecteur d'OBS (preview, programme, multiview,
/// source). ⚠️ La classe Win32 est la classe Qt GÉNÉRIQUE de toutes les
/// fenêtres OBS ("Qt<ver>QWindowIcon") — pas "OBSProjector" : on identifie
/// donc par titre contenant "project" (Projector/Projecteur/Proyector/
/// Projektor…) ET processus obs64.exe (évite les faux positifs d'autres
/// apps dont le titre contient "project").
#[cfg(windows)]
fn hwnds_projecteurs() -> Vec<isize> {
    use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows::core::PWSTR;

    /// exe du processus = obs64.exe ?
    fn est_obs(pid: u32) -> bool {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(h);
            ok && String::from_utf16_lossy(&buf[..len as usize])
                .to_lowercase()
                .ends_with("obs64.exe")
        }
    }

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<isize>);
        if IsWindowVisible(hwnd).as_bool() {
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, &mut buf);
            if n > 0
                && String::from_utf16_lossy(&buf[..n as usize])
                    .to_lowercase()
                    .contains("project")
            {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != 0 && est_obs(pid) {
                    out.push(hwnd.0 as isize);
                }
            }
        }
        BOOL(1)
    }

    let mut v: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// Ferme toutes les fenêtres projecteur OBS (WM_CLOSE). Utilisé au boot
/// quand l'option « projecteur au démarrage » est OFF (défaut) : OBS
/// restaure ses projecteurs d'une session à l'autre — StreamOS impose
/// « fermé » par défaut. Local Win32, pas besoin de WebSocket.
/// Retourne le nombre de fenêtres fermées.
#[cfg(windows)]
pub fn fermer_projecteurs() -> u32 {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    let hwnds = hwnds_projecteurs();
    let n = hwnds.len() as u32;
    for h in hwnds {
        unsafe {
            let _ = PostMessageW(
                HWND(h as *mut std::ffi::c_void),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
    n
}

#[cfg(not(windows))]
pub fn fermer_projecteurs() -> u32 {
    0
}

/// Remonte la fenêtre projecteur au premier plan : SetWindowPos(HWND_TOP)
/// pour le z-order (pas de topmost permanent), BringWindowToTop +
/// SetForegroundWindow en best-effort (Windows peut refuser le focus volé
/// depuis un process tiers — l'appelant est en foreground au clic, ça passe
/// en général ; sinon HWND_TOP la rend déjà visible).
#[cfg(windows)]
fn projecteur_premier_plan(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, SetForegroundWindow, SetWindowPos, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE,
    };
    unsafe {
        let h = HWND(hwnd as *mut std::ffi::c_void);
        let _ = SetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        let _ = BringWindowToTop(h);
        let _ = SetForegroundWindow(h);
    }
}

// --- Helpers WebSocket (pub(crate) pour obs_trou) ---

pub(crate) async fn send_msg<W>(write: &mut W, msg: &Value) -> Result<(), String>
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
pub(crate) async fn recv_msg<R>(read: &mut R) -> Result<Value, String>
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
            Message::Close(frame) => {
                // 4009 = auth échouée (obs-websocket v5) → message actionnable
                // (le champ mot de passe de la Toolbar est vide/invalide).
                if let Some(f) = frame.as_ref() {
                    if u16::from(f.code) == 4009 {
                        return Err(
                            "OBS: authentification échouée — mot de passe WebSocket invalide \
                             (Toolbar → section OBS → champ mot de passe)"
                                .into(),
                        );
                    }
                }
                let detail = frame
                    .map(|f| format!("code={} raison=\"{}\"", f.code, f.reason))
                    .unwrap_or_else(|| "sans détail".to_string());
                return Err(format!("OBS: connexion fermée par le serveur ({})", detail));
            }
            _ => {} // ping, pong, binary — ignorés
        }
    }
}

/// Envoie une requête RPC (op=6) et attend la réponse correspondante (op=7).
/// Ignore les événements (op=5) reçus entre-temps.
/// Log la requête ET la réponse (status + comment) pour le debug.
pub(crate) async fn rpc<W, R>(
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
                // Boîte noire : horodate le dernier RPC réussi — point de
                // passage UNIQUE de tous les appels obs-websocket, donc la
                // seule mesure fiable de « StreamOS parle encore à OBS ? ».
                crate::conso::marquer_obs_ok();
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
