/// Sources OBS "trou" : caméra / fenêtre / jeu placées SOUS la Browser Source
/// SOS-Diffusion, calées sur la géométrie d'un widget troué (x y w h).
///
/// Connexion one-shot par opération (énum / création / sync / suppression) :
/// connect → RPC(s) → close. Pas de client persistant. Si OBS offline → erreur
/// remontée au dashboard (message, pas de crash).
///
/// Sync transform : appelée au commitScene (pointerup) pour tous les widgets
/// avec obsSource=Some. Reconnect one-shot, SetSceneItemTransform par item.
use serde_json::{json, Value};

use crate::obs::{connect, rpc, SOURCE_NAME, WsSink, WsStreamHalf};

use futures_util::SinkExt;

/// Résultat de scene_sos() : scène « SOS » (via sceneUuid) + ses items +
/// l'index de l'item SOS-Diffusion (0 = top visuel en OBS v5).
struct SceneSos {
    scene_uuid: String,
    #[allow(dead_code)] // renvoyé par GetSceneList, non utilisé aujourd'hui
    scene_name: String,
    items: Vec<Value>,
    #[allow(dead_code)] // conservé pour debug ; l'ordre est géré par reorder_sos_sources
    sos_index: i64,
}

/// Helper unique : GetSceneList → trouver scène sceneName == "SOS" → garder
/// sceneUuid. GetSceneItemList avec sceneUuid (PAS sceneName). Tous les appels
/// ultérieurs utilisent sceneUuid. Interdit : SOS Web, Stream Operating System,
/// « Scène », Opera. Log `[OBS] scène SOS uuid=…`.
const SOS_SCENE_NAME: &str = "SOS";

/// Nom de la source OBS caméra (singleton : 1 widget caméra / scène).
/// dshow_input placée sous SOS-Diffusion, calée sur le widget.
/// Contrat : l'input n'est JAMAIS supprimé (RemoveInput) — la suppression du
/// widget cache l'item (SetSceneItemEnabled false), la recréation réutilise
/// l'input existant (évite les conflits d'accès exclusif du capteur Windows
/// et la perte des settings OBS : crop, color, buffer).
pub const SOS_CAMERA: &str = "SOS-Caméra";

async fn scene_sos(write: &mut WsSink, read: &mut WsStreamHalf) -> Result<Option<SceneSos>, String> {
    // 1. GetSceneList → trouver scène sceneName == "SOS" → garder sceneUuid.
    let scenes_resp = rpc(write, read, "GetSceneList", "scene_sos_list", json!({})).await?;
    let scenes = scenes_resp["scenes"]
        .as_array()
        .ok_or("OBS: GetSceneList réponse sans scenes")?;

    let sos_scene = scenes.iter().find(|s| s["sceneName"].as_str() == Some(SOS_SCENE_NAME));
    let Some(sos_scene) = sos_scene else {
        eprintln!("[OBS] scène « {} » introuvable dans GetSceneList (AUCUNE)", SOS_SCENE_NAME);
        return Ok(None);
    };

    let scene_uuid = sos_scene["sceneUuid"]
        .as_str()
        .ok_or(format!("OBS: scène « {} » sans sceneUuid", SOS_SCENE_NAME))?
        .to_string();
    eprintln!("[OBS] scène SOS uuid={} nom=\"{}\"", scene_uuid, SOS_SCENE_NAME);

    // 2. GetSceneItemList avec sceneUuid (PAS sceneName).
    let items_res = rpc(
        write,
        read,
        "GetSceneItemList",
        "scene_sos_items",
        json!({ "sceneUuid": scene_uuid }),
    )
    .await;

    let items = match items_res {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[OBS] GetSceneItemList uuid={} → {} (AUCUNE)", scene_uuid, e);
            return Ok(None);
        }
    };

    let arr = match items["sceneItems"].as_array() {
        Some(a) => a,
        None => {
            eprintln!("[OBS] uuid={} → réponse sans sceneItems (AUCUNE)", scene_uuid);
            return Ok(None);
        }
    };
    eprintln!("[OBS] items={} (scène « {} » uuid={})", arr.len(), SOS_SCENE_NAME, scene_uuid);

    // 3. Index de SOS-Diffusion : priorité SOURCE_NAME exact, sinon contains SOS
    //    && !starts_with("SOS-Trou-").
    let sos_idx = arr.iter().position(|it| {
        it["sourceName"].as_str() == Some(SOURCE_NAME)
    }).or_else(|| {
        arr.iter().position(|it| {
            it["sourceName"]
                .as_str()
                .map(|s| s.contains("SOS") && !s.starts_with("SOS-Trou-"))
                .unwrap_or(false)
        })
    });

    match sos_idx {
        Some(idx) => {
            eprintln!("[OBS] scène retenue=« {} » uuid={}", SOS_SCENE_NAME, scene_uuid);
            Ok(Some(SceneSos {
                scene_uuid,
                scene_name: SOS_SCENE_NAME.to_string(),
                items: arr.clone(),
                sos_index: idx as i64,
            }))
        }
        None => {
            eprintln!("[OBS] scène retenue=(AUCUNE) — « {} » sans « {} »", SOS_SCENE_NAME, SOURCE_NAME);
            Ok(None)
        }
    }
}

/// Type de capture supporté. "camera" = dshow_input, "window" = window_capture,
/// "game" = game_capture (capture_specific_window sur la fenêtre choisie).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureKind {
    Camera,
    Window,
    Game,
}

impl CaptureKind {
    fn input_kind(&self) -> &'static str {
        match self {
            CaptureKind::Camera => "dshow_input",
            CaptureKind::Window => "window_capture",
            CaptureKind::Game => "game_capture",
        }
    }

    /// Nom de la propriété OBS contenant la liste des cibles.
    /// Caméra = video_device_id, Fenêtre/Jeu = window (liste des fenêtres).
    fn property_name(&self) -> Option<&'static str> {
        match self {
            CaptureKind::Camera => Some("video_device_id"),
            CaptureKind::Window => Some("window"),
            CaptureKind::Game => Some("window"),
        }
    }

    /// Settings du temp input pour l'énumération (game a besoin de
    /// capture_mode=capture_specific_window pour peupler la liste window).
    fn enum_settings(&self) -> Value {
        match self {
            CaptureKind::Game => json!({ "capture_mode": "capture_specific_window" }),
            _ => json!({}),
        }
    }
}

/// Énumère les cibles de capture disponibles dans OBS pour un type donné.
/// Caméra → devices dshow ; Fenêtre → fenêtres ; Jeu → fenêtres (game_capture
/// en mode capture_specific_window pour peupler la liste).
/// One-shot : connect → CreateInput temp → GetInputPropertiesList → DeleteInput
/// → close. Retourne la liste des valeurs de cibles.
pub async fn enumerate_targets(
    host: &str,
    port: u16,
    password: &str,
    kind: CaptureKind,
) -> Result<Vec<String>, String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    let prop = kind.property_name().ok_or("OBS: kind sans propriété de cible")?;

    let temp_name = "SOS-Temp-Enum";

    // 1. Créer un input temporaire du bon kind pour pouvoir interroger ses
    //    propriétés (GetInputPropertiesList nécessite une instance d'input).
    //    Game a besoin de capture_mode=capture_specific_window pour peupler
    //    la liste des fenêtres. Scène = celle contenant SOS-Diffusion (helper).
    let sos = scene_sos(&mut write, &mut read).await?
        .ok_or("OBS: scène « SOS » introuvable ou sans SOS-Diffusion")?;
    rpc(
        &mut write,
        &mut read,
        "CreateInput",
        "enum_create",
        json!({
            "sceneUuid": sos.scene_uuid,
            "inputName": temp_name,
            "inputKind": kind.input_kind(),
            "inputSettings": kind.enum_settings(),
            "sceneItemEnabled": false
        }),
    )
    .await?;

    // 2. GetInputPropertiesList → propriété = liste des cibles.
    let props = rpc(
        &mut write,
        &mut read,
        "GetInputPropertiesList",
        "enum_props",
        json!({ "inputName": temp_name, "propertyName": prop }),
    )
    .await;

    // 3. Supprimer l'input temp (non-fatal si échec).
    let _ = rpc(
        &mut write,
        &mut read,
        "RemoveInput",
        "enum_remove",
        json!({ "inputName": temp_name }),
    )
    .await;

    let _ = write.close().await;

    let props = props?;
    // Réponse : { "propertyItems": [ { "itemValue": "...", "itemEnabled": true }, ... ] }
    let items = props["propertyItems"]
        .as_array()
        .ok_or("OBS: GetInputPropertiesList réponse sans propertyItems")?;
    let targets: Vec<String> = items
        .iter()
        .filter_map(|it| {
            // itemEnabled peut être absent (tout activé) ou false (device occupé).
            let enabled = it["itemEnabled"].as_bool().unwrap_or(true);
            if !enabled {
                return None;
            }
            it["itemValue"].as_str().map(|s| s.to_string())
        })
        .collect();

    log::info!(
        "OBS trou: énumération {:?} → {} cible(s)",
        kind,
        targets.len()
    );
    Ok(targets)
}

/// Crée une source OBS de capture sous SOS-Diffusion, calée sur le widget.
/// Retourne le nom de la source créée ("SOS-Trou-<id8>").
/// One-shot : connect → CreateInput → GetSceneItemList (trouver itemId +
/// index de SOS-Diffusion) → SetSceneItemIndex (sous SOS-Diffusion) →
/// SetSceneItemTransform (position + taille = widget) → close.
#[allow(clippy::too_many_arguments)]
pub async fn create_trou_source(
    host: &str,
    port: u16,
    password: &str,
    source_name: &str,
    kind: CaptureKind,
    target: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    // 1. Trouver la scène « SOS » via sceneUuid (helper unique).
    let sos = scene_sos(&mut write, &mut read).await?
        .ok_or("OBS: scène « SOS » introuvable ou sans SOS-Diffusion")?;
    let scene_uuid = &sos.scene_uuid;

    // 2. Settings selon le kind + cible.
    let mut settings = serde_json::Map::new();
    match &kind {
        CaptureKind::Camera => {
            if let Some(dev) = &target {
                settings.insert("video_device_id".to_string(), Value::String(dev.clone()));
            }
        }
        CaptureKind::Window => {
            if let Some(win) = &target {
                settings.insert("window".to_string(), Value::String(win.clone()));
            }
        }
        CaptureKind::Game => {
            // capture_specific_window sur la fenêtre choisie (comme window_capture).
            settings.insert("capture_mode".to_string(), Value::String("capture_specific_window".to_string()));
            if let Some(win) = &target {
                settings.insert("window".to_string(), Value::String(win.clone()));
            }
        }
    }

    // 3. CreateInput dans la scène SOS (via sceneUuid).
    let create_res = rpc(
        &mut write,
        &mut read,
        "CreateInput",
        "trou_create",
        json!({
            "sceneUuid": scene_uuid,
            "inputName": source_name,
            "inputKind": kind.input_kind(),
            "inputSettings": Value::Object(settings),
            "sceneItemEnabled": true
        }),
    )
    .await;
    match &create_res {
        Ok(_) => eprintln!("[OBS] CreateInput ok nom=\"{}\"", source_name),
        Err(e) => eprintln!("[OBS] CreateInput err=\"{}\"", e),
    }
    create_res?;
    eprintln!("OBS trou: input \"{}\" créé ({:?})", source_name, kind);

    // 4. GetSceneItemList via sceneUuid → trouver itemId de la nouvelle source.
    let items = rpc(
        &mut write,
        &mut read,
        "GetSceneItemList",
        "trou_items",
        json!({ "sceneUuid": scene_uuid }),
    )
    .await?;

    let scene_items = items["sceneItems"]
        .as_array()
        .ok_or("OBS: GetSceneItemList réponse sans sceneItems")?;

    // itemId de la source trou (trouvée par sourceName).
    let trou_item_id = scene_items
        .iter()
        .find(|it| it["sourceName"].as_str() == Some(source_name))
        .and_then(|it| it["sceneItemId"].as_i64())
        .ok_or(format!(
            "OBS: source \"{}\" introuvable dans la scène après création",
            source_name
        ))?;

    // 5. SetSceneItemTransform via sceneUuid : position + taille = widget.
    rpc(
        &mut write,
        &mut read,
        "SetSceneItemTransform",
        "trou_transform",
        json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": trou_item_id,
            "sceneItemTransform": {
                "positionX": x,
                "positionY": y,
                "boundsWidth": w,
                "boundsHeight": h,
                "boundsType": "OBS_BOUNDS_STRETCH",
                "alignment": 5
            }
        }),
    )
    .await?;

    // 6. Reorder global : Diffusion (top) > Trou > Caméra (bottom).
    //    reorder_sos_sources est le SEUL endroit qui gère les indices.
    reorder_sos_sources(&mut write, &mut read, scene_uuid).await;

    let _ = write.close().await;
    eprintln!(
        "OBS trou: source \"{}\" placée sous \"{}\" à ({},{}) {}x{}",
        source_name, SOURCE_NAME, x, y, w, h
    );
    Ok(source_name.to_string())
}

/// Input OBS énuméré via GetInputList (nom + kind), filtré par type de capture.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InputEntry {
    pub input_name: String,
    pub input_kind: String,
}

/// Vrai si inputKind correspond au type de capture (filtrage souple, includes).
/// Caméra → dshow / video / av_capture (PAS wasapi = audio) ;
/// Fenêtre → window ; Jeu → game.
fn kind_matches(kind: &CaptureKind, ikind: &str) -> bool {
    let k = ikind.to_lowercase();
    match kind {
        CaptureKind::Camera => {
            (k.contains("dshow") || k.contains("video") || k.contains("av_capture"))
                && !k.contains("wasapi")
        }
        CaptureKind::Window => k.contains("window"),
        CaptureKind::Game => k.contains("game"),
    }
}

/// Énumère les inputs OBS (GetInputList) filtrés par type de capture.
/// Caméra → dshow_input ; Fenêtre → window_capture ; Jeu → game_capture.
/// Exclut SOS-Diffusion, browser_source, sources audio, scènes, trous liés.
/// One-shot : connect → GetInputList → close.
pub async fn enumerate_inputs_by_kind(
    host: &str,
    port: u16,
    password: &str,
    kind: CaptureKind,
) -> Result<Vec<InputEntry>, String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    let resp = rpc(
        &mut write,
        &mut read,
        "GetInputList",
        "enum_inputs",
        json!({}),
    )
    .await?;

    let _ = write.close().await;

    let inputs = resp["inputs"]
        .as_array()
        .ok_or("OBS: GetInputList réponse sans inputs")?;

    // Log brut : nombre d'inputs + kinds (debug si liste vide).
    let all_kinds: Vec<String> = inputs
        .iter()
        .filter_map(|it| it["inputKind"].as_str().map(|s| s.to_string()))
        .collect();
    log::info!(
        "OBS trou: GetInputList → {} input(s) total, kinds = [{}]",
        all_kinds.len(),
        all_kinds.join(", ")
    );

    let entries: Vec<InputEntry> = inputs
        .iter()
        .filter_map(|it| {
            let name = it["inputName"].as_str()?;
            // Skip inputName vide (pas de ligne inventée).
            if name.is_empty() {
                return None;
            }
            // Exclure SOS-Diffusion + trous liés.
            if name == SOURCE_NAME || name.starts_with("SOS-Trou-") {
                return None;
            }
            // kind : fallback sur unversionedInputKind si inputKind absent.
            let ikind = it["inputKind"]
                .as_str()
                .or_else(|| it["unversionedInputKind"].as_str())
                .unwrap_or("");
            // Garder seulement les kinds correspondants (souple, includes).
            if !kind_matches(&kind, ikind) {
                return None;
            }
            Some(InputEntry {
                input_name: name.to_string(),
                input_kind: ikind.to_string(),
            })
        })
        .collect();

    log::info!(
        "OBS trou: {:?} → {} input(s) après filtre",
        kind,
        entries.len()
    );
    Ok(entries)
}

/// Lie une source OBS existante au widget trou : place CETTE source sous
/// SOS-Diffusion dans la scène qui CONTIENT la Browser Source SOS-Diffusion
/// (découverte via scene_sos(), PAS GetCurrentProgramScene, PAS de hardcodage).
/// Si l'input n'est pas encore dans la scène → CreateSceneItem (pas de recréation
/// de l'input). Puis SetSceneItemTransform + SetSceneItemIndex (sous SOS = indexSOS+1).
/// One-shot : connect → scene_sos() → (CreateSceneItem si absent) →
/// SetSceneItemTransform → SetSceneItemIndex → close.
#[allow(clippy::too_many_arguments)]
pub async fn link_existing_source(
    host: &str,
    port: u16,
    password: &str,
    source_name: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    // 1. Trouver la scène « SOS » via sceneUuid (helper unique).
    let sos = scene_sos(&mut write, &mut read).await?
        .ok_or("OBS: scène « SOS » introuvable ou sans SOS-Diffusion")?;
    let scene_uuid = &sos.scene_uuid;

    // 2. itemId si la source cliquée est déjà dans la scène retenue.
    let existing_item_id = sos.items
        .iter()
        .find(|it| it["sourceName"].as_str() == Some(source_name))
        .and_then(|it| it["sceneItemId"].as_i64());

    // 3. Si absent → CreateSceneItem (ajouter l'input existant à la scène).
    let item_id = match existing_item_id {
        Some(id) => id,
        None => {
            eprintln!(
                "OBS trou link: \"{}\" pas dans « SOS » → CreateSceneItem",
                source_name
            );
            let create_res = rpc(
                &mut write,
                &mut read,
                "CreateSceneItem",
                "link_create_item",
                json!({
                    "sceneUuid": scene_uuid,
                    "sourceName": source_name,
                    "sceneItemEnabled": true
                }),
            )
            .await;
            match &create_res {
                Ok(_) => eprintln!("[OBS] CreateSceneItem ok nom=\"{}\"", source_name),
                Err(e) => eprintln!("[OBS] CreateSceneItem err=\"{}\"", e),
            }
            let created = create_res?;
            created["sceneItemId"]
                .as_i64()
                .ok_or("OBS: CreateSceneItem réponse sans sceneItemId")?
        }
    };

    // 4. SetSceneItemTransform via sceneUuid : position + taille = widget.
    rpc(
        &mut write,
        &mut read,
        "SetSceneItemTransform",
        "link_transform",
        json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": item_id,
            "sceneItemTransform": {
                "positionX": x,
                "positionY": y,
                "boundsWidth": w,
                "boundsHeight": h,
                "boundsType": "OBS_BOUNDS_STRETCH",
                "alignment": 5
            }
        }),
    )
    .await?;

    // 5. Reorder global : Diffusion (top) > Trou > Caméra (bottom).
    //    reorder_sos_sources est le SEUL endroit qui gère les indices.
    reorder_sos_sources(&mut write, &mut read, scene_uuid).await;

    let _ = write.close().await;
    eprintln!(
        "OBS trou: source \"{}\" liée dans « SOS » à ({},{}) {}x{}",
        source_name, x, y, w, h
    );
    Ok(source_name.to_string())
}

/// Item à synchroniser (un par widget avec obsSource=Some).
/// rename_all = camelCase : le TS envoie sourceName (convention JS).
/// fit/zoom/rot pilotent SetSceneItemTransform de la source OBS du trou
/// (boundsType + scale/bounds + rotation). Defaults sûrs : fit="etirer"
/// (STRETCH = ancien comportement), zoom=1.0, rot=0.0.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncItem {
    pub source_name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default = "default_fit_sync")]
    pub fit: String,
    #[serde(default = "default_zoom_sync")]
    pub zoom: f64,
    #[serde(default)]
    pub rot: f64,
    #[serde(default)]
    pub offset_x: f64,
    #[serde(default)]
    pub offset_y: f64,
}

fn default_fit_sync() -> String {
    "etirer".to_string()
}

fn default_zoom_sync() -> f64 {
    1.0
}

/// Mapping mode d'affichage widget → boundsType OBS.
/// ajuster → SCALE_INNER (contain) ; remplir → SCALE_OUTER (cover) ;
/// etendre → MAX_ONLY ; etirer → STRETCH ; centrer → NONE (alignment centre,
/// zoom via scale) ; vignette → SCALE_INNER ; autre → STRETCH (sécurité).
fn fit_to_bounds(fit: &str) -> &'static str {
    match fit {
        "ajuster" => "OBS_BOUNDS_SCALE_INNER",
        "remplir" => "OBS_BOUNDS_SCALE_OUTER",
        "etendre" => "OBS_BOUNDS_MAX_ONLY",
        "etirer" => "OBS_BOUNDS_STRETCH",
        "centrer" => "OBS_BOUNDS_NONE",
        "vignette" => "OBS_BOUNDS_SCALE_INNER",
        _ => "OBS_BOUNDS_STRETCH",
    }
}

/// Synchronise les transforms OBS de toutes les sources trou en une seule
/// connexion. One-shot : connect → scene_sos() → pour chaque item trouvé
/// → SetSceneItemTransform → close. Source absente dans OBS → ignorée (log).
/// Si aucune scène SOS-Diffusion → skip silencieux (Ok, pas d'erreur boucle).
pub async fn sync_trous(
    host: &str,
    port: u16,
    password: &str,
    items: Vec<SyncItem>,
) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }

    let (mut write, mut read) = connect(host, port, password).await?;

    // scene_sos() : si scène « SOS » introuvable → skip silencieux.
    let Some(sos) = scene_sos(&mut write, &mut read).await? else {
        eprintln!("OBS trou sync: pas de scène « SOS » → skip (silencieux)");
        let _ = write.close().await;
        return Ok(());
    };
    let scene_uuid = &sos.scene_uuid;
    let scene_items = &sos.items;

    let mut synced = 0u32;
    let mut missing = 0u32;
    for it in &items {
        let item_id = scene_items
            .iter()
            .find(|s| s["sourceName"].as_str() == Some(&it.source_name))
            .and_then(|s| s["sceneItemId"].as_i64());

        let Some(item_id) = item_id else {
            eprintln!("OBS trou sync: source \"{}\" absente de « SOS » — ignorée", it.source_name);
            missing += 1;
            continue;
        };

        // Transform OBS du trou : la capture RESTE dans le rectangle du widget.
        // boundsWidth/Height = w/h (JAMAIS w*zoom — ça déborderait du trou).
        // alignment 0 (centre) pour rotation centrée. position = centre widget.
        // Zoom ≥ 1 = crop centré de la source (recadrage, pas agrandissement) :
        // plus le zoom est fort, plus on recadre ; la boîte reste w×h.
        // Rotation dans cette même boîte (coins coupés = OK, le trou clip).

        // 1. GetSceneItemTransform → sourceWidth/sourceHeight (pour le crop).
        let cur = rpc(
            &mut write,
            &mut read,
            "GetSceneItemTransform",
            "sync_get_transform",
            json!({
                "sceneUuid": scene_uuid,
                "sceneItemId": item_id
            }),
        )
        .await?;
        let src_w = cur["sourceWidth"].as_f64().unwrap_or(0.0);
        let src_h = cur["sourceHeight"].as_f64().unwrap_or(0.0);

        // 2. Crop centré pour zoom ≥ 1. zoom < 1 → pas de crop (zoom crop = 1).
        //    visible = source / zoom_crop → on garde le centre, on coupe les bords.
        let zoom_crop = if it.zoom > 1.0 { it.zoom } else { 1.0 };
        let (crop_lr, crop_tb) = if src_w > 0.0 && src_h > 0.0 && zoom_crop > 1.0 {
            let vw = src_w / zoom_crop;
            let vh = src_h / zoom_crop;
            ((src_w - vw) / 2.0, (src_h - vh) / 2.0)
        } else {
            (0.0, 0.0)
        };

        // 3. SetSceneItemTransform : bounds = w×h (fixes), boundsType = fit,
        //    crop centré, rotation, alignment 0, position = centre.
        let bounds = fit_to_bounds(&it.fit);
        let cx = it.x + it.w / 2.0 + it.offset_x;
        let cy = it.y + it.h / 2.0 + it.offset_y;
        rpc(
            &mut write,
            &mut read,
            "SetSceneItemTransform",
            "sync_transform",
            json!({
                "sceneUuid": scene_uuid,
                "sceneItemId": item_id,
                "sceneItemTransform": {
                    "positionX": cx,
                    "positionY": cy,
                    "boundsWidth": it.w,
                    "boundsHeight": it.h,
                    "boundsType": bounds,
                    "alignment": 0,
                    "rotation": it.rot,
                    "cropLeft": crop_lr,
                    "cropRight": crop_lr,
                    "cropTop": crop_tb,
                    "cropBottom": crop_tb
                }
            }),
        )
        .await?;
        synced += 1;
    }

    let _ = write.close().await;
    eprintln!(
        "OBS trou sync: {} transform(s) MAJ, {} absente(s)",
        synced,
        missing
    );
    Ok(())
}

// ===== Caméra (widget type "camera" → source OBS "SOS-Caméra") =====

/// Garantit que la source "SOS-Caméra" existe dans la scène « SOS », est
/// activée, calée sur le widget et placée juste sous SOS-Diffusion.
/// Connexion déjà ouverte (write/read) — partagée avec sync_scene_captures
/// et camera_sync.
///
/// `apply_device` = true UNIQUEMENT sur création explicite / changement de
/// device (SetInputSettings video_device_id sur l'input existant). Au boot
/// (sync_scene_captures) on ne touche JAMAIS aux settings d'un input existant
/// (pas de restart du capteur dshow).
///
/// Transform : même sémantique que sync_trous (position = centre du widget,
/// bounds w×h, OBS_BOUNDS_SCALE_OUTER = cover, alignment 0) → aucun saut
/// entre la création et le premier move/resize.
#[allow(clippy::too_many_arguments)]
async fn ensure_camera(
    write: &mut WsSink,
    read: &mut WsStreamHalf,
    sos: &SceneSos,
    device: Option<&str>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    apply_device: bool,
) -> Result<(), String> {
    let scene_uuid = &sos.scene_uuid;

    // 1. L'input "SOS-Caméra" existe-t-il globalement ?
    let inputs_resp = rpc(write, read, "GetInputList", "cam_inputs", json!({})).await?;
    let input_exists = inputs_resp["inputs"]
        .as_array()
        .map(|inputs| {
            inputs
                .iter()
                .any(|i| i["inputName"].as_str() == Some(SOS_CAMERA))
        })
        .unwrap_or(false);

    let item_id: i64 = if !input_exists {
        // 2a. Input absent → CreateInput (crée aussi l'item de scène).
        //     Device fourni → video_device_id, sinon device par défaut d'OBS.
        let mut settings = serde_json::Map::new();
        if let Some(d) = device {
            settings.insert("video_device_id".to_string(), Value::String(d.to_string()));
        }
        let create_res = rpc(
            write,
            read,
            "CreateInput",
            "cam_create",
            json!({
                "sceneUuid": scene_uuid,
                "inputName": SOS_CAMERA,
                "inputKind": "dshow_input",
                "inputSettings": Value::Object(settings),
                "sceneItemEnabled": true
            }),
        )
        .await?;
        eprintln!("[OBS] caméra: input \"{}\" créé (dshow)", SOS_CAMERA);
        create_res["sceneItemId"]
            .as_i64()
            .ok_or("OBS: CreateInput caméra sans sceneItemId")?
    } else {
        // 2b. Input existant → item dans la scène « SOS » ?
        let existing = sos
            .items
            .iter()
            .find(|it| it["sourceName"].as_str() == Some(SOS_CAMERA))
            .and_then(|it| it["sceneItemId"].as_i64());

        if let Some(id) = existing {
            // Item présent → réactiver (recréation du widget caméra).
            let _ = rpc(
                write,
                read,
                "SetSceneItemEnabled",
                &format!("cam_enable_{}", id),
                json!({
                    "sceneUuid": scene_uuid,
                    "sceneItemId": id,
                    "sceneItemEnabled": true
                }),
            )
            .await;
            id
        } else {
            // Item absent → CreateSceneItem (input réutilisé, jamais dupliqué).
            let cs_res = rpc(
                write,
                read,
                "CreateSceneItem",
                "cam_create_item",
                json!({
                    "sceneUuid": scene_uuid,
                    "sourceName": SOS_CAMERA,
                    "sceneItemEnabled": true
                }),
            )
            .await?;
            eprintln!(
                "[OBS] caméra: input \"{}\" réutilisé (existant), item recréé",
                SOS_CAMERA
            );
            cs_res["sceneItemId"]
                .as_i64()
                .ok_or("OBS: CreateSceneItem caméra sans sceneItemId")?
        }
    };

    // 3. Changement de device explicite → SetInputSettings (SEULEMENT si
    //    apply_device — contrat : jamais de restart du capteur au boot).
    if input_exists && apply_device {
        if let Some(d) = device {
            rpc(
                write,
                read,
                "SetInputSettings",
                "cam_set_device",
                json!({
                    "inputName": SOS_CAMERA,
                    "inputSettings": { "video_device_id": d }
                }),
            )
            .await?;
            eprintln!("[OBS] caméra: device changé → \"{}\"", d);
        }
    }

    // 4. Transform : centre + SCALE_OUTER (cover) + alignment 0 (sync_trous).
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    rpc(
        write,
        read,
        "SetSceneItemTransform",
        "cam_transform",
        json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": item_id,
            "sceneItemTransform": {
                "positionX": cx,
                "positionY": cy,
                "boundsWidth": w,
                "boundsHeight": h,
                "boundsType": "OBS_BOUNDS_SCALE_OUTER",
                "alignment": 0
            }
        }),
    )
    .await?;

    // 5. Index géré uniquement par reorder_sos_sources (appelé à la fin de
    //    sync_scene_captures). ensure_camera ne touche PLUS à l'index pour
    //    éviter les races avec reorder quand sync_scene_captures est appelé
    //    deux fois en concurrent au boot.
    eprintln!(
        "[OBS] caméra: \"{}\" transform OK à ({},{}) {}x{} (index géré par reorder)",
        SOS_CAMERA, x, y, w, h
    );
    Ok(())
}

/// Commande frontend : crée / met à jour la source "SOS-Caméra" dans OBS.
/// Appelée à la création du widget caméra (device=None → device par défaut
/// d'OBS) et au changement de device (apply_device → SetInputSettings).
/// One-shot : connect → scene_sos() → ensure_camera() → close.
#[allow(clippy::too_many_arguments)]
pub async fn camera_sync(
    host: &str,
    port: u16,
    password: &str,
    device: Option<&str>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    let (mut write, mut read) = connect(host, port, password).await?;
    let sos = scene_sos(&mut write, &mut read).await?
        .ok_or("OBS: scène « SOS » introuvable ou sans SOS-Diffusion")?;
    ensure_camera(&mut write, &mut read, &sos, device, x, y, w, h, device.is_some()).await?;
    let _ = write.close().await;
    Ok(())
}

/// Commande frontend : cache l'item "SOS-Caméra" (SetSceneItemEnabled false).
/// L'input OBS est CONSERVÉ (contrat : jamais RemoveInput — la recréation du
/// widget le réutilisera). Non-fatal si l'item est absent (déjà caché).
/// One-shot : connect → scene_sos() → SetSceneItemEnabled → close.
pub async fn camera_hide(host: &str, port: u16, password: &str) -> Result<(), String> {
    let (mut write, mut read) = connect(host, port, password).await?;
    let Some(sos) = scene_sos(&mut write, &mut read).await? else {
        let _ = write.close().await;
        return Ok(());
    };
    if let Some(item_id) = sos
        .items
        .iter()
        .find(|it| it["sourceName"].as_str() == Some(SOS_CAMERA))
        .and_then(|it| it["sceneItemId"].as_i64())
    {
        let _ = rpc(
            &mut write,
            &mut read,
            "SetSceneItemEnabled",
            "cam_hide",
            json!({
                "sceneUuid": sos.scene_uuid,
                "sceneItemId": item_id,
                "sceneItemEnabled": false
            }),
        )
        .await;
        eprintln!("[OBS] caméra: item \"{}\" caché (input conservé)", SOS_CAMERA);
    }
    let _ = write.close().await;
    Ok(())
}

/// Synchronise les captures SOS-Trou-* avec les widgets de la scène chargée.
/// Au chargement d'une scène :
///   - items SOS dont le nom commence par "SOS-Trou-" ET n'est dans AUCUN
///     widget.obsSource → SetSceneItemEnabled false (cacher, pas DeleteInput).
///   - items SOS-Trou-* qui SONT dans un widget.obsSource → enable true + sync
///     transform (position + taille = widget).
///
/// One-shot : connect → scene_sos() → pour chaque item SOS-Trou-* →
/// SetSceneItemEnabled (+ SetSceneItemTransform si lié) → close.
/// Non-fatal : si OBS offline ou scène SOS absente → Ok silencieux.
pub async fn sync_scene_captures(
    host: &str,
    port: u16,
    password: &str,
    widgets: &[crate::scene::Widget],
) -> Result<(), String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    let Some(sos) = scene_sos(&mut write, &mut read).await? else {
        eprintln!("[OBS] sync_captures: pas de scène « SOS » → skip");
        let _ = write.close().await;
        return Ok(());
    };
    let scene_uuid = &sos.scene_uuid;

    let mut enabled = 0u32;
    let mut disabled = 0u32;
    let mut synced = 0u32;

    // Index des widgets par obsSource pour lookup rapide.
    use std::collections::HashMap;
    let widget_by_source: HashMap<&str, &crate::scene::Widget> = widgets
        .iter()
        .filter_map(|w| w.obsSource.as_deref().map(|s| (s, w)))
        .collect();

    // ===== Caméra : auto-guérison (contrat 2) =====
    // Widget caméra présent → garantir la source "SOS-Caméra" (créer si
    // absente, réactiver l'item, caler transform + index sous SOS-Diffusion).
    // apply_device=false → on ne touche PAS aux settings d'un input existant
    // (pas de restart du capteur dshow au boot).
    // Pas de widget caméra → cacher l'item orphelin (l'input est conservé).
    if let Some(cam) = widgets.iter().find(|w| w.widget_type == "camera") {
        let _ = ensure_camera(
            &mut write,
            &mut read,
            &sos,
            cam.cameraDeviceId.as_deref(),
            cam.x,
            cam.y,
            cam.largeur,
            cam.hauteur,
            false,
        )
        .await;
        enabled += 1;
    } else if let Some(item_id) = sos
        .items
        .iter()
        .find(|it| it["sourceName"].as_str() == Some(SOS_CAMERA))
        .and_then(|it| it["sceneItemId"].as_i64())
    {
        let _ = rpc(
            &mut write,
            &mut read,
            "SetSceneItemEnabled",
            &format!("sync_cap_cam_hide_{}", item_id),
            json!({
                "sceneUuid": scene_uuid,
                "sceneItemId": item_id,
                "sceneItemEnabled": false
            }),
        )
        .await;
        disabled += 1;
    }

    for item in &sos.items {
        let Some(name) = item["sourceName"].as_str() else { continue };
        if !name.starts_with("SOS-Trou-") { continue; }

        let item_id = match item["sceneItemId"].as_i64() {
            Some(id) => id,
            None => continue,
        };

        if let Some(w) = widget_by_source.get(name) {
            // Item lié à un widget de cette scène → enable + sync transform.
            let _ = rpc(
                &mut write,
                &mut read,
                "SetSceneItemEnabled",
                &format!("sync_cap_enable_{}", item_id),
                json!({
                    "sceneUuid": scene_uuid,
                    "sceneItemId": item_id,
                    "sceneItemEnabled": true
                }),
            )
            .await;
            enabled += 1;

            // Sync transform : position + taille = widget (bounds STRETCH).
            let _ = rpc(
                &mut write,
                &mut read,
                "SetSceneItemTransform",
                &format!("sync_cap_transform_{}", item_id),
                json!({
                    "sceneUuid": scene_uuid,
                    "sceneItemId": item_id,
                    "sceneItemTransform": {
                        "positionX": w.x,
                        "positionY": w.y,
                        "boundsWidth": w.largeur,
                        "boundsHeight": w.hauteur,
                        "boundsType": "OBS_BOUNDS_STRETCH",
                        "alignment": 5
                    }
                }),
            )
            .await;
            synced += 1;
        } else {
            // Item SOS-Trou-* sans widget dans cette scène → cacher (pas delete).
            let _ = rpc(
                &mut write,
                &mut read,
                "SetSceneItemEnabled",
                &format!("sync_cap_disable_{}", item_id),
                json!({
                    "sceneUuid": scene_uuid,
                    "sceneItemId": item_id,
                    "sceneItemEnabled": false
                }),
            )
            .await;
            disabled += 1;
        }
    }

    // ===== Vérification/Maj ordre des sources au démarrage =====
    // Ordre visuel souhaité : SOS-Diffusion (top) > SOS-Trou-* > SOS-Caméra (bottom).
    // Reorder one-shot : GetSceneItemList → SetSceneItemIndex (bottom-up).
    reorder_sos_sources(&mut write, &mut read, scene_uuid).await;

    let _ = write.close().await;
    eprintln!(
        "[OBS] sync_captures: {} activé(s), {} transform(s) MAJ, {} caché(s)",
        enabled, synced, disabled
    );
    Ok(())
}

/// Force l'ordre visuel des sources SOS dans la scène :
///   Index 0 (BAS)     : SOS-Caméra
///   Index 1 (MILIEU)  : SOS-Trou-* (tous les trous, ordre relatif conservé)
///   Index N (HAUT)    : SOS-Diffusion
///
/// Dans OBS 32.x, index 0 = bas visuel, index max = haut visuel.
/// SetSceneItemIndex du HAUT vers le BAS pour éviter de déplacer les items
/// déjà positionnés. One-shot : GetSceneItemList → SetSceneItemIndex x N.
/// Non-fatal : si OBS offline ou scène absente → skip silencieux.
async fn reorder_sos_sources(write: &mut WsSink, read: &mut WsStreamHalf, scene_uuid: &str) {
    // Récupérer la liste fraîche des items (après enable/disable/transform).
    let items = match rpc(write, read, "GetSceneItemList", "reorder_items", json!({
        "sceneUuid": scene_uuid
    })).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[OBS] reorder: GetSceneItemList échec → {}", e);
            return;
        }
    };

    let arr = match items["sceneItems"].as_array() {
        Some(a) => a,
        None => {
            eprintln!("[OBS] reorder: réponse sans sceneItems → skip");
            return;
        }
    };

    // Collecter les IDs par catégorie (ordre relatif actuel conservé).
    let mut diffusion_id: Option<i64> = None;
    let mut trou_ids: Vec<i64> = Vec::new();
    let mut camera_id: Option<i64> = None;

    for it in arr {
        let name = it["sourceName"].as_str().unwrap_or("");
        let id = it["sceneItemId"].as_i64();
        let Some(id) = id else { continue };
        if name == SOURCE_NAME {
            diffusion_id = Some(id);
        } else if name == SOS_CAMERA {
            camera_id = Some(id);
        } else if name.starts_with("SOS-Trou-") {
            trou_ids.push(id);
        }
    }

    // OBS 32.x : index 0 = BAS visuel, index max = HAUT visuel.
    // Ordre souhaité (du bas vers le haut) : Caméra > Trou > Diffusion.
    // SetSceneItemIndex du HAUT vers le BAS (diffusion d'abord, puis trous,
    // puis caméra) pour ne pas déplacer les items déjà positionnés.
    let trou_count = trou_ids.len() as i64;
    let top_index = 1 + trou_count; // index le plus haut = Diffusion

    // 1. Diffusion → index le plus haut (HAUT visuel)
    if let Some(diff_id) = diffusion_id {
        let _ = rpc(write, read, "SetSceneItemIndex", "reorder_diff", json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": diff_id,
            "sceneItemIndex": top_index
        })).await;
        eprintln!("[OBS] reorder: diffusion → index {} (top)", top_index);
    }

    // 2. Trous du premier au dernier : indices décroissants (du haut vers le bas
    //    parmi les trous, pour ne pas déplacer ceux déjà positionnés).
    for (i, tid) in trou_ids.iter().enumerate() {
        let idx = trou_count - i as i64; // trou_count = juste sous Diffusion
        let _ = rpc(write, read, "SetSceneItemIndex", &format!("reorder_trou_{}", tid), json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": tid,
            "sceneItemIndex": idx
        })).await;
    }
    if !trou_ids.is_empty() {
        eprintln!("[OBS] reorder: {} trou(s) → indices 1..{}", trou_ids.len(), trou_count);
    }

    // 3. Caméra → index 0 (BAS visuel)
    if let Some(cam_id) = camera_id {
        let _ = rpc(write, read, "SetSceneItemIndex", "reorder_cam", json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": cam_id,
            "sceneItemIndex": 0
        })).await;
        eprintln!("[OBS] reorder: caméra → index 0 (bas)");
    }

    // Vérification : relire l'ordre final pour confirmer.
    if let Ok(final_items) = rpc(write, read, "GetSceneItemList", "reorder_verify", json!({
        "sceneUuid": scene_uuid
    })).await {
        if let Some(arr) = final_items["sceneItems"].as_array() {
            let order: Vec<String> = arr.iter().filter_map(|it| {
                it["sourceName"].as_str().map(|s| s.to_string())
            }).collect();
            eprintln!("[OBS] reorder: ordre final = {:?}", order);
        }
    }
}

/// Supprime une source trou d'OBS (RemoveInput supprime l'input + son item de
/// scène). One-shot : connect → RemoveInput → close. Non-fatal si la source
/// n'existe pas (déjà supprimée côté OBS).
pub async fn delete_trou_source(
    host: &str,
    port: u16,
    password: &str,
    source_name: &str,
) -> Result<(), String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    // RemoveInput : supprime l'input globalement (+ son item de scène).
    // Si l'input n'existe pas → erreur OBS, qu'on traite comme succès (idempotent).
    let res = rpc(
        &mut write,
        &mut read,
        "RemoveInput",
        "trou_delete",
        json!({ "inputName": source_name }),
    )
    .await;

    let _ = write.close().await;

    match res {
        Ok(_) => {
            log::info!("OBS trou: source \"{}\" supprimée", source_name);
            Ok(())
        }
        Err(e) => {
            // Source déjà absente → idempotent, pas une erreur pour l'appelant.
            log::warn!(
                "OBS trou: suppression \"{}\" → {} (considéré comme déjà supprimée)",
                source_name,
                e
            );
            Ok(())
        }
    }
}

/// Crée une source OBS de capture depuis une cible PC (caméra/fenêtre/jeu)
/// dans la scène qui contient SOS-Diffusion (découverte via GetSceneList,
/// PAS de hardcodage sceneName="SOS"). Si la source SOS-Trou-<id> existe déjà
/// → SetInputSettings + transform (pas de doublon). Sinon → CreateInput.
/// Puis SetSceneItemIndex SOUS SOS-Diffusion (indexSOS+1) + transform = widget.
/// One-shot : connect → GetSceneList → (GetSceneItemList par scène) →
/// GetInputList → (CreateInput OU SetInputSettings) → GetSceneItemList →
/// SetSceneItemTransform → SetSceneItemIndex → close.
#[allow(clippy::too_many_arguments)]
pub async fn create_trou_from_pc(
    host: &str,
    port: u16,
    password: &str,
    source_name: &str,
    kind: CaptureKind,
    target: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    let (mut write, mut read) = connect(host, port, password).await?;

    // 1. Trouver la scène « SOS » via sceneUuid (helper unique).
    let sos = scene_sos(&mut write, &mut read).await?
        .ok_or("SOS-Diffusion introuvable: scène « SOS » introuvable ou sans SOS-Diffusion")?;
    let scene_uuid = &sos.scene_uuid;
    eprintln!("[OBS] scène SOS uuid={} (confirmée dans create_trou_from_pc)", scene_uuid);

    // 2. Settings selon le kind + cible.
    let mut settings = serde_json::Map::new();
    match &kind {
        CaptureKind::Camera => {
            if let Some(dev) = &target {
                settings.insert("video_device_id".to_string(), Value::String(dev.clone()));
            }
        }
        CaptureKind::Window => {
            if let Some(win) = &target {
                eprintln!("[OBS] window=\"{}\" (format OBS title:class:exe)", win);
                settings.insert("window".to_string(), Value::String(win.clone()));
            }
        }
        CaptureKind::Game => {
            settings.insert("capture_mode".to_string(), Value::String("capture_specific_window".to_string()));
            if let Some(win) = &target {
                eprintln!("[OBS] game window=\"{}\"", win);
                settings.insert("window".to_string(), Value::String(win.clone()));
            }
        }
    }
    eprintln!("[OBS] inputSettings={}", Value::Object(settings.clone()));

    // 3. GetInputList → la source SOS-Trou-<id> existe déjà GLOBALEMENT ?
    //    Si oui et qu'elle n'est pas dans la scène « SOS » → on crée un NOUVEL
    //    input avec suffixe "-sos" (jamais toucher à l'ancien SOS Web).
    let inputs_resp = rpc(
        &mut write,
        &mut read,
        "GetInputList",
        "pc_inputs",
        json!({}),
    )
    .await?;
    let global_inputs = inputs_resp["inputs"]
        .as_array()
        .map(|inputs| inputs.iter().filter_map(|i| i["inputName"].as_str().map(|s| s.to_string())).collect::<Vec<_>>())
        .unwrap_or_default();

    // Nom effectif : si source_name existe déjà globalement mais pas dans la
    // scène SOS → utiliser source_name + "-sos" pour créer un nouvel input.
    let mut effective_name = source_name.to_string();
    if global_inputs.iter().any(|n| n == source_name) {
        // Vérifier si cet input est déjà dans la scène SOS.
        let already_in_sos = sos.items.iter().any(|it| it["sourceName"].as_str() == Some(source_name));
        if already_in_sos {
            // Input existe et est dans la scène → SetInputSettings (update).
            eprintln!("[OBS] input \"{}\" existe et est dans « SOS » → SetInputSettings", source_name);
            let set_res = rpc(
                &mut write,
                &mut read,
                "SetInputSettings",
                "pc_set_settings",
                json!({
                    "inputName": source_name,
                    "inputSettings": Value::Object(settings.clone()),
                }),
            )
            .await;
            match &set_res {
                Ok(resp) => eprintln!("[OBS] SetInputSettings ok nom=\"{}\" resp={}", source_name, resp),
                Err(e) => eprintln!("[OBS] SetInputSettings err=\"{}\"", e),
            }
            set_res?;
        } else {
            // Input existe ailleurs (SOS Web) → nouveau nom avec suffixe.
            effective_name = format!("{}-sos", source_name);
            eprintln!("[OBS] input \"{}\" existe ailleurs → nouveau nom=\"{}\"", source_name, effective_name);
            // Si effective_name existe déjà aussi → SetInputSettings, sinon CreateInput.
            if global_inputs.iter().any(|n| n == &effective_name) {
                eprintln!("[OBS] input \"{}\" existe déjà → SetInputSettings", effective_name);
                let set_res = rpc(
                    &mut write,
                    &mut read,
                    "SetInputSettings",
                    "pc_set_settings",
                    json!({
                        "inputName": effective_name,
                        "inputSettings": Value::Object(settings.clone()),
                    }),
                )
                .await;
                match &set_res {
                    Ok(resp) => eprintln!("[OBS] SetInputSettings ok nom=\"{}\" resp={}", effective_name, resp),
                    Err(e) => eprintln!("[OBS] SetInputSettings err=\"{}\"", e),
                }
                set_res?;
            } else {
                let create_json = json!({
                    "sceneUuid": scene_uuid,
                    "inputName": effective_name,
                    "inputKind": kind.input_kind(),
                    "inputSettings": Value::Object(settings.clone()),
                    "sceneItemEnabled": true
                });
                eprintln!("[OBS] OBS → CreateInput JSON={}", create_json);
                let create_res = rpc(
                    &mut write,
                    &mut read,
                    "CreateInput",
                    "pc_create",
                    create_json,
                )
                .await;
                match &create_res {
                    Ok(resp) => eprintln!("[OBS] OBS ← CreateInput result=ok nom=\"{}\" resp={}", effective_name, resp),
                    Err(e) => eprintln!("[OBS] OBS ← CreateInput result=ERR comment=\"{}\"", e),
                }
                create_res?;
            }
        }
    } else {
        // Source absente globalement → CreateInput dans la scène SOS (via uuid).
        let create_json = json!({
            "sceneUuid": scene_uuid,
            "inputName": effective_name,
            "inputKind": kind.input_kind(),
            "inputSettings": Value::Object(settings.clone()),
            "sceneItemEnabled": true
        });
        eprintln!("[OBS] OBS → CreateInput JSON={}", create_json);
        let create_res = rpc(
            &mut write,
            &mut read,
            "CreateInput",
            "pc_create",
            create_json,
        )
        .await;
        match &create_res {
            Ok(resp) => eprintln!("[OBS] OBS ← CreateInput result=ok nom=\"{}\" resp={}", effective_name, resp),
            Err(e) => eprintln!("[OBS] OBS ← CreateInput result=ERR comment=\"{}\"", e),
        }
        create_res?;
    }

    // 4. GetSceneItemList avec sceneUuid → itemId de la source effective.
    //    Si l'input existe globalement mais n'est pas un item de la scène SOS
    //    → CreateSceneItem pour l'ajouter. Si CreateSceneItem échoue →
    //    CreateInput avec nom unique timestamp (jamais réutiliser input autre scène).
    let items = rpc(
        &mut write,
        &mut read,
        "GetSceneItemList",
        "pc_scene_items",
        json!({ "sceneUuid": scene_uuid }),
    )
    .await?;

    let scene_items = items["sceneItems"]
        .as_array()
        .ok_or("OBS: GetSceneItemList réponse sans sceneItems")?;

    let item_id = scene_items
        .iter()
        .find(|it| it["sourceName"].as_str() == Some(&effective_name))
        .and_then(|it| it["sceneItemId"].as_i64());

    let item_id = match item_id {
        Some(id) => id,
        None => {
            // Input existe globalement mais pas dans la scène SOS → CreateSceneItem.
            eprintln!(
                "[OBS] input \"{}\" absent de « SOS » → CreateSceneItem",
                effective_name
            );
            let cs_res = rpc(
                &mut write,
                &mut read,
                "CreateSceneItem",
                "pc_create_item",
                json!({
                    "sceneUuid": scene_uuid,
                    "sourceName": effective_name,
                    "sceneItemEnabled": true
                }),
            )
            .await;
            match &cs_res {
                Ok(resp) => eprintln!("[OBS] CreateSceneItem ok nom=\"{}\" resp={}", effective_name, resp),
                Err(e) => eprintln!("[OBS] CreateSceneItem err=\"{}\"", e),
            }

            match cs_res {
                Ok(created) => {
                    created["sceneItemId"]
                        .as_i64()
                        .ok_or("OBS: CreateSceneItem réponse sans sceneItemId")?
                }
                Err(e) => {
                    // CreateSceneItem échoué → CreateInput avec nom unique timestamp.
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let unique_name = format!("{}-{}", source_name, ts);
                    eprintln!(
                        "[OBS] CreateSceneItem échoué (\"{}\") → CreateInput unique=\"{}\"",
                        e, unique_name
                    );
                    effective_name = unique_name.clone();
                    let create_json = json!({
                        "sceneUuid": scene_uuid,
                        "inputName": unique_name,
                        "inputKind": kind.input_kind(),
                        "inputSettings": Value::Object(settings.clone()),
                        "sceneItemEnabled": true
                    });
                    eprintln!("[OBS] OBS → CreateInput JSON={}", create_json);
                    let create_res = rpc(
                        &mut write,
                        &mut read,
                        "CreateInput",
                        "pc_create_fallback",
                        create_json,
                    )
                    .await;
                    match &create_res {
                        Ok(resp) => eprintln!("[OBS] OBS ← CreateInput result=ok nom=\"{}\" resp={}", unique_name, resp),
                        Err(e2) => eprintln!("[OBS] OBS ← CreateInput result=ERR comment=\"{}\"", e2),
                    }
                    let created = create_res?;
                    // CreateInput crée directement l'item → sceneItemId dans la réponse.
                    created["sceneItemId"]
                        .as_i64()
                        .ok_or("OBS: CreateInput fallback réponse sans sceneItemId")?
                }
            }
        }
    };

    // 5. SetSceneItemTransform avec sceneUuid : position + taille = widget.
    rpc(
        &mut write,
        &mut read,
        "SetSceneItemTransform",
        "pc_transform",
        json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": item_id,
            "sceneItemTransform": {
                "positionX": x,
                "positionY": y,
                "boundsWidth": w,
                "boundsHeight": h,
                "boundsType": "OBS_BOUNDS_STRETCH",
                "alignment": 5
            }
        }),
    )
    .await?;

    // 6. Reorder global : Diffusion (top) > Trou > Caméra (bottom).
    //    reorder_sos_sources est le SEUL endroit qui gère les indices.
    reorder_sos_sources(&mut write, &mut read, scene_uuid).await;

    let _ = write.close().await;
    eprintln!(
        "[OBS] source \"{}\" créée/liée dans « SOS » (uuid={}) à ({},{}) {}x{}",
        effective_name, scene_uuid, x, y, w, h
    );
    Ok(effective_name)
}
