/// Sources OBS "trou" : fenêtre / jeu placées SOUS la Browser Source
/// SOS-Diffusion, calées sur la géométrie d'un widget troué (x y w h).
/// La caméra (SOS-Caméra) et sa plaque noire (SOS-Caméra-Fond) sont AUSSI
/// SOUS SOS-Diffusion mais AU-DESSUS des SOS-Trou-* : le flux dshow apparaît
/// à travers le trou destination-out du canvas :4321 sans être masqué par
/// une capture qui recouvrirait le même pixel ; les widgets :4321 qui
/// chevauchent le rectangle caméra sont clippés côté diffusion.html.
///
/// Connexion one-shot par opération (énum / création / sync / suppression) :
/// connect → RPC(s) → close. Pas de client persistant. Si OBS offline → erreur
/// remontée au dashboard (message, pas de crash).
///
/// Sync transform : appelée au commitScene (pointerup) pour tous les widgets
/// avec obsSource=Some. Reconnect one-shot, SetSceneItemTransform par item.
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::mask_png;
use crate::obs::{connect, rpc, SOURCE_NAME, WsSink, WsStreamHalf};

use futures_util::SinkExt;

/// Résultat de scene_sos() : scène « SOS » (via sceneUuid) + ses items +
/// l'index de l'item SOS-Diffusion (index OBS 32.x : 0 = bas visuel).
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
/// dshow_input placée SOUS SOS-Diffusion, au-dessus de la plaque noire
/// (elle-même au-dessus des SOS-Trou-*), calée sur le widget.
/// Contrat : l'input n'est JAMAIS supprimé (RemoveInput) — la suppression du
/// widget cache l'item (SetSceneItemEnabled false), la recréation réutilise
/// l'input existant (évite les conflits d'accès exclusif du capteur Windows
/// et la perte des settings OBS : crop, color, buffer).
pub const SOS_CAMERA: &str = "SOS-Caméra";

/// Nom de la source OBS « plaque noire » placée sous "SOS-Caméra", au-dessus
/// des SOS-Trou-* (singleton, même contrat : jamais RemoveInput). color_source
/// noire opaque calée sur le même rectangle que le widget caméra : montre du
/// noir dans le trou quand le capteur est off ou occupé.
/// Item désactivé quand il n'y a plus de widget caméra.
pub const SOS_CAMERA_FOND: &str = "SOS-Caméra-Fond";

async fn scene_sos(write: &mut WsSink, read: &mut WsStreamHalf) -> Result<Option<SceneSos>, String> {
    // 1. GetSceneList → trouver scène sceneName == "SOS" → garder sceneUuid.
    let scenes_resp = rpc(write, read, "GetSceneList", "scene_sos_list", json!({})).await?;
    let scenes = scenes_resp["scenes"]
        .as_array()
        .ok_or("OBS: GetSceneList réponse sans scenes")?;

    let sos_scene = scenes.iter().find(|s| s["sceneName"].as_str() == Some(SOS_SCENE_NAME));
    let Some(sos_scene) = sos_scene else {
        return Ok(None);
    };

    let scene_uuid = sos_scene["sceneUuid"]
        .as_str()
        .ok_or(format!("OBS: scène « {} » sans sceneUuid", SOS_SCENE_NAME))?
        .to_string();

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
        Err(_) => {
            return Ok(None);
        }
    };

    let arr = match items["sceneItems"].as_array() {
        Some(a) => a,
        None => {
            return Ok(None);
        }
    };

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
            Ok(Some(SceneSos {
                scene_uuid,
                scene_name: SOS_SCENE_NAME.to_string(),
                items: arr.clone(),
                sos_index: idx as i64,
            }))
        }
        None => {
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
    create_res?;

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

    // 6. Reorder global : Diffusion > Caméra > Fond > Trou (haut → bas).
    //    reorder_sos_sources est le SEUL endroit qui gère les indices.
    reorder_sos_sources(&mut write, &mut read, scene_uuid).await;

    let _ = write.close().await;
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

    // 5. Reorder global : Diffusion > Caméra > Fond > Trou (haut → bas).
    //    reorder_sos_sources est le SEUL endroit qui gère les indices.
    reorder_sos_sources(&mut write, &mut read, scene_uuid).await;

    let _ = write.close().await;
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

/// Crop centré (gauche/droite, haut/bas) en pixels SOURCE pour qu'une
/// capture reste DANS la boîte w×h du widget. Deux composantes cumulées :
///   - crop zoom (zoom ≥ 1 = recadrage centré, formule historique) ;
///   - crop ratio, UNIQUEMENT en fit « remplir » (cover) : rogne le côté
///     excédentaire pour que la zone visible ait EXACTEMENT le ratio w/h.
///     Sans ce crop, OBS_BOUNDS_SCALE_OUTER agrandit la source au-delà de la
///     boîte et le débord est visible à travers tout trou destination-out
///     voisin (caméra posée sur un widget troué). obs-websocket n'expose pas
///     « rogner selon la boîte englobante » → crop explicite.
///
/// Source non dimensionnée (0) → (0, 0).
fn crop_source(src_w: f64, src_h: f64, w: f64, h: f64, fit: &str, zoom: f64) -> (f64, f64) {
    if src_w <= 0.0 || src_h <= 0.0 {
        return (0.0, 0.0);
    }
    let zoom_crop = if zoom > 1.0 { zoom } else { 1.0 };
    let mut vw = src_w / zoom_crop;
    let mut vh = src_h / zoom_crop;
    if fit == "remplir" && w > 0.0 && h > 0.0 {
        let ratio = w / h;
        if vw / vh > ratio {
            vw = vh * ratio;
        } else {
            vh = vw / ratio;
        }
    }
    ((src_w - vw) / 2.0, (src_h - vh) / 2.0)
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
        let _ = write.close().await;
        return Ok(());
    };
    let scene_uuid = &sos.scene_uuid;
    let scene_items = &sos.items;

    for it in &items {
        let item_id = scene_items
            .iter()
            .find(|s| s["sourceName"].as_str() == Some(&it.source_name))
            .and_then(|s| s["sceneItemId"].as_i64());

        let Some(item_id) = item_id else {
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
        // Réponse obs-websocket v5 : { "sceneItemTransform": { sourceWidth, … } }.
        let tr = &cur["sceneItemTransform"];
        let src_w = tr["sourceWidth"].as_f64().unwrap_or(0.0);
        let src_h = tr["sourceHeight"].as_f64().unwrap_or(0.0);

        // 2. Crop centré : zoom ≥ 1 (recadrage) + ratio w/h en « remplir »
        //    (la source ne déborde plus de la boîte) — cf. crop_source.
        let (crop_lr, crop_tb) = crop_source(src_w, src_h, it.w, it.h, &it.fit, it.zoom);

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

        // 4. Plaque noire sous la caméra : même rectangle CALCULÉ (centre +
        //    bounds w×h + SCALE_OUTER + alignment 0), JAMAIS le crop dshow.
        //    Fond absent → skip (auto-guérison au prochain ensure_camera).
        if it.source_name == SOS_CAMERA {
            if let Some(fond_id) = scene_items
                .iter()
                .find(|s| s["sourceName"].as_str() == Some(SOS_CAMERA_FOND))
                .and_then(|s| s["sceneItemId"].as_i64())
            {
                if let Err(e) = rpc(
                    &mut write,
                    &mut read,
                    "SetSceneItemTransform",
                    "sync_fond_transform",
                    json!({
                        "sceneUuid": scene_uuid,
                        "sceneItemId": fond_id,
                        "sceneItemTransform": {
                            "positionX": cx,
                            "positionY": cy,
                            "boundsWidth": it.w,
                            "boundsHeight": it.h,
                            "boundsType": "OBS_BOUNDS_SCALE_OUTER",
                            "alignment": 0
                        }
                    }),
                )
                .await
                {
                    eprintln!("[OBS] sync fond transform: {}", e);
                }
            }
        }
    }

    let _ = write.close().await;
    Ok(())
}

// ===== Caméra (widget type "camera" → source OBS "SOS-Caméra") =====

/// Garantit que la source "SOS-Caméra" existe dans la scène « SOS », est
/// activée, calée sur le widget et placée sous SOS-Diffusion, au-dessus de
/// la plaque (via reorder_sos_sources, seul endroit qui gère les indices).
/// Garantit AUSSI la plaque noire "SOS-Caméra-Fond" (sous la caméra) :
/// le trou montre du noir quand le capteur est off ou occupé.
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

    // 0. Plaque noire d'abord : créée/réactivée/calée sous la caméra.
    //    Échec → Err propagé (STOP : la caméra n'est pas sync sur une base
    //    cassée ; l'appelant décide de la remontée).
    ensure_camera_fond(write, read, sos, x, y, w, h).await?;

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
        }
    }

    // 4. Transform : centre + SCALE_OUTER (cover) + alignment 0 (sync_trous)
    //    + crop ratio w/h (crop_source « remplir », zoom 1 = contrat caméra
    //    de commitScene) : la source ne déborde pas de la boîte dès la
    //    création. Capteur pas encore dimensionné (0×0) → crop 0, rattrapé
    //    au prochain commit (sync_trous).
    let cur = rpc(
        write,
        read,
        "GetSceneItemTransform",
        "cam_get_transform",
        json!({ "sceneUuid": scene_uuid, "sceneItemId": item_id }),
    )
    .await?;
    let tr = &cur["sceneItemTransform"];
    let src_w = tr["sourceWidth"].as_f64().unwrap_or(0.0);
    let src_h = tr["sourceHeight"].as_f64().unwrap_or(0.0);
    let (crop_lr, crop_tb) = crop_source(src_w, src_h, w, h, "remplir", 1.0);
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
                "alignment": 0,
                "cropLeft": crop_lr,
                "cropRight": crop_lr,
                "cropTop": crop_tb,
                "cropBottom": crop_tb
            }
        }),
    )
    .await?;

    // 5. Index géré uniquement par reorder_sos_sources (appelé à la fin de
    //    sync_scene_captures). ensure_camera ne touche PLUS à l'index pour
    //    éviter les races avec reorder quand sync_scene_captures est appelé
    //    deux fois en concurrent au boot.
    Ok(())
}

/// Garantit que la plaque noire "SOS-Caméra-Fond" existe dans la scène « SOS »,
/// est activée et calée sur le même rectangle que le widget caméra.
/// Miroir simplifié d'ensure_camera : pas de device, jamais RemoveInput,
/// jamais SetInputSettings sur l'input existant (couleur fixée à la création).
///
/// Kind : color_source_v3 (OBS 28+). Fallback UNIQUE color_source_v2 si le
/// CreateInput v3 échoue ; si v2 échoue aussi → Err (STOP, pas de 3e kind).
/// Noir opaque = 0xFF000000 (alpha 255 + RGB 0 — l'ordre ARGB/ABGR est sans
/// importance puisque les 3 canaux sont à 0). width/height = taille du widget
/// (acceptés par v3, ignorés par v2) pour une taille native non nulle.
///
/// Transform : même sémantique que la caméra (position = centre du widget,
/// bounds w×h, OBS_BOUNDS_SCALE_OUTER, alignment 0) — pas de crop.
async fn ensure_camera_fond(
    write: &mut WsSink,
    read: &mut WsStreamHalf,
    sos: &SceneSos,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    let scene_uuid = &sos.scene_uuid;

    // 1. L'input "SOS-Caméra-Fond" existe-t-il globalement ?
    let inputs_resp = rpc(write, read, "GetInputList", "fond_inputs", json!({})).await?;
    let input_exists = inputs_resp["inputs"]
        .as_array()
        .map(|inputs| {
            inputs
                .iter()
                .any(|i| i["inputName"].as_str() == Some(SOS_CAMERA_FOND))
        })
        .unwrap_or(false);

    let item_id: i64 = if !input_exists {
        // 2a. Input absent → CreateInput color_source_v3 (crée aussi l'item).
        //     Échec (kind inconnu, OBS < 28) → UN SEUL fallback v2, puis STOP.
        let settings = json!({
            "color": 4278190080i64,
            "width": w.round() as i64,
            "height": h.round() as i64
        });
        let mut create_res = rpc(
            write,
            read,
            "CreateInput",
            "fond_create_v3",
            json!({
                "sceneUuid": scene_uuid,
                "inputName": SOS_CAMERA_FOND,
                "inputKind": "color_source_v3",
                "inputSettings": settings.clone(),
                "sceneItemEnabled": true
            }),
        )
        .await;
        if create_res.is_err() {
            create_res = rpc(
                write,
                read,
                "CreateInput",
                "fond_create_v2",
                json!({
                    "sceneUuid": scene_uuid,
                    "inputName": SOS_CAMERA_FOND,
                    "inputKind": "color_source_v2",
                    "inputSettings": settings,
                    "sceneItemEnabled": true
                }),
            )
            .await;
        }
        let created = create_res.map_err(|e| {
            format!("OBS: création \"{}\" impossible (v3 + v2): {}", SOS_CAMERA_FOND, e)
        })?;
        created["sceneItemId"]
            .as_i64()
            .ok_or("OBS: CreateInput fond sans sceneItemId")?
    } else {
        // 2b. Input existant → item dans la scène « SOS » ?
        let existing = sos
            .items
            .iter()
            .find(|it| it["sourceName"].as_str() == Some(SOS_CAMERA_FOND))
            .and_then(|it| it["sceneItemId"].as_i64());

        if let Some(id) = existing {
            // Item présent → réactiver.
            let _ = rpc(
                write,
                read,
                "SetSceneItemEnabled",
                &format!("fond_enable_{}", id),
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
                "fond_create_item",
                json!({
                    "sceneUuid": scene_uuid,
                    "sourceName": SOS_CAMERA_FOND,
                    "sceneItemEnabled": true
                }),
            )
            .await?;
            cs_res["sceneItemId"]
                .as_i64()
                .ok_or("OBS: CreateSceneItem fond sans sceneItemId")?
        }
    };

    // 3. Transform : centre + SCALE_OUTER (cover) + alignment 0 (caméra).
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    rpc(
        write,
        read,
        "SetSceneItemTransform",
        "fond_transform",
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

    // Index géré uniquement par reorder_sos_sources (plaque sous la caméra,
    // au-dessus des trous).
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
    // Ordre des items : Diffusion en haut, caméra + plaque au-dessus des
    // trous, dès la création (CreateInput place les nouveaux items en haut
    // de scène).
    // reorder_sos_sources est le SEUL endroit qui gère les indices.
    reorder_sos_sources(&mut write, &mut read, &sos.scene_uuid).await;
    let _ = write.close().await;
    Ok(())
}

/// Commande frontend : cache les items "SOS-Caméra" ET "SOS-Caméra-Fond"
/// (SetSceneItemEnabled false) à la suppression du widget caméra — le trou
/// n'existe plus, une plaque enabled résiduelle resterait une source allumée
/// pour rien (visible à travers les autres trous destination-out). Les inputs OBS sont CONSERVÉS (contrat : jamais
/// RemoveInput — la recréation du widget les réutilisera). Non-fatal si un
/// item est absent (déjà caché ou jamais créé).
/// One-shot : connect → scene_sos() → SetSceneItemEnabled × 2 → close.
pub async fn camera_hide(host: &str, port: u16, password: &str) -> Result<(), String> {
    let (mut write, mut read) = connect(host, port, password).await?;
    let Some(sos) = scene_sos(&mut write, &mut read).await? else {
        let _ = write.close().await;
        return Ok(());
    };
    for name in [SOS_CAMERA, SOS_CAMERA_FOND] {
        if let Some(item_id) = sos
            .items
            .iter()
            .find(|it| it["sourceName"].as_str() == Some(name))
            .and_then(|it| it["sceneItemId"].as_i64())
        {
            let _ = rpc(
                &mut write,
                &mut read,
                "SetSceneItemEnabled",
                &format!("cam_hide_{}", item_id),
                json!({
                    "sceneUuid": sos.scene_uuid,
                    "sceneItemId": item_id,
                    "sceneItemEnabled": false
                }),
            )
            .await;
        }
    }
    let _ = write.close().await;
    Ok(())
}

// ===== Masque caméra — découpe SOS-Caméra / SOS-Caméra-Fond à la forme du cadre =====

/// Nom du filtre « Image mask/Blend » posé sur SOS-Caméra et SOS-Caméra-Fond.
/// Mode alpha : l'alpha du PNG multiplie l'alpha de la source
/// (mask_alpha_filter.effect), `stretch` étire le PNG à la taille de base
/// de la source → le masque est généré en pixels de la source.
const SOS_MASQUE_FILTRE: &str = "SOS-Masque";

/// Kind OBS du filtre. ⚠️ obs-websocket expose DEUX kinds (vérifié OBS 32.2.2 /
/// obs-websocket 5.7.4 via GetSourceFilterKindList) : "mask_filter" = v1
/// OBSOLÈTE (`opacity` ENTIER 0-100 → 1.0 lu comme 1 % → source quasi
/// transparente, plaque noire visible à travers) et "mask_filter_v2"
/// (`opacity` double 0-1, sRGB). On crée TOUJOURS la v2 ; un filtre v1
/// résiduel est remplacé (le kind ne se change pas par SetSourceFilterSettings).
const SOS_MASQUE_KIND: &str = "mask_filter_v2";

/// Réglages du filtre : alpha du PNG, image étirée, aucune atténuation
/// (color 0xFFFFFF + opacity 1.0 → vec4(1,1,1,1), cf. mask_alpha_filter.effect).
fn reglages_masque(chemin_png: &str) -> Value {
    json!({
        "type": "mask_alpha_filter.effect",
        "image_path": chemin_png,
        "color": 16_777_215,
        "opacity": 1.0,
        "stretch": true
    })
}

/// Hash court (16 hex) d'octets : nom de fichier de masque versionné par
/// contenu — même masque → même nom (écriture sautée), masque changé →
/// nouveau fichier (changement de chemin = cache d'images OBS contourné).
fn hex_court(bytes: &[u8]) -> String {
    bytes.iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

/// Taille de base (native) d'un item de scène : GetSceneItemTransform →
/// sourceWidth/sourceHeight. None si l'item est absent de la scène ou si la
/// source n'est pas encore dimensionnée (capteur dshow en cours de démarrage).
async fn taille_base_item(
    write: &mut WsSink,
    read: &mut WsStreamHalf,
    scene_uuid: &str,
    items: &[Value],
    source_name: &str,
) -> Option<(f64, f64)> {
    let id = items
        .iter()
        .find(|it| it["sourceName"].as_str() == Some(source_name))
        .and_then(|it| it["sceneItemId"].as_i64())?;
    let tr = rpc(
        write,
        read,
        "GetSceneItemTransform",
        &format!("masque_taille_{}", id),
        json!({ "sceneUuid": scene_uuid, "sceneItemId": id }),
    )
    .await
    .ok()?;
    // Réponse obs-websocket v5 : { "sceneItemTransform": { sourceWidth, … } }.
    let tr = &tr["sceneItemTransform"];
    let w = tr["sourceWidth"].as_f64().unwrap_or(0.0);
    let h = tr["sourceHeight"].as_f64().unwrap_or(0.0);
    if w > 0.0 && h > 0.0 {
        Some((w, h))
    } else {
        None
    }
}

/// Chemin du PNG actuellement référencé par le filtre SOS-Masque d'un input
/// (pour le nettoyage : ne jamais supprimer un fichier encore utilisé).
async fn chemin_filtre_actuel(
    write: &mut WsSink,
    read: &mut WsStreamHalf,
    input_name: &str,
) -> Option<String> {
    let liste = rpc(
        write,
        read,
        "GetSourceFilterList",
        "masque_chemin",
        json!({ "sourceName": input_name }),
    )
    .await
    .ok()?;
    liste["filters"]
        .as_array()?
        .iter()
        .find(|f| f["filterName"].as_str() == Some(SOS_MASQUE_FILTRE))
        .and_then(|f| f["filterSettings"]["image_path"].as_str().map(str::to_string))
}

/// Mappe les points du polygone (coords widget-local 0..w × 0..h) en pixels
/// de base de la source, en inversant le pipeline de sync_trous : crop zoom
/// centré puis bounds fit alignés au centre (alignment 0). La rotation de
/// l'item est volontairement ignorée : le filtre s'applique AVANT le
/// transform de l'item → la forme tourne avec la source, exactement comme
/// le cadre SVG tourne avec le widget dans la page de diffusion.
fn points_en_espace_source(
    points: &[(f64, f64)],
    src_w: f64,
    src_h: f64,
    w: f64,
    h: f64,
    fit: &str,
    zoom: f64,
) -> Vec<(f64, f64)> {
    // Crop centré zoom + ratio (MÊME fonction que sync_trous / ensure_camera).
    let (crop_lr, crop_tb) = crop_source(src_w, src_h, w, h, fit, zoom);
    let wc = (src_w - 2.0 * crop_lr).max(1.0);
    let hc = (src_h - 2.0 * crop_tb).max(1.0);
    // Échelle selon le boundsType (miroir de fit_to_bounds).
    let (sx, sy) = match fit_to_bounds(fit) {
        "OBS_BOUNDS_SCALE_OUTER" => {
            let s = (w / wc).max(h / hc);
            (s, s)
        }
        "OBS_BOUNDS_SCALE_INNER" => {
            let s = (w / wc).min(h / hc);
            (s, s)
        }
        "OBS_BOUNDS_MAX_ONLY" => {
            let s = (w / wc).min(h / hc).min(1.0);
            (s, s)
        }
        "OBS_BOUNDS_NONE" => (1.0, 1.0),
        _ => (w / wc, h / hc), // STRETCH
    };
    points
        .iter()
        .map(|&(px, py)| {
            (
                crop_lr + wc / 2.0 + (px - w / 2.0) / sx,
                crop_tb + hc / 2.0 + (py - h / 2.0) / sy,
            )
        })
        .collect()
}

/// Crée ou met à jour le filtre SOS-Masque sur un input OBS. Skip interne si
/// le filtre référence déjà ce PNG (pas de rechargement d'image inutile).
async fn appliquer_filtre_masque(
    write: &mut WsSink,
    read: &mut WsStreamHalf,
    input_name: &str,
    chemin_png: &str,
) -> Result<(), String> {
    let liste = rpc(
        write,
        read,
        "GetSourceFilterList",
        "masque_filtres",
        json!({ "sourceName": input_name }),
    )
    .await?;
    let existant = liste["filters"]
        .as_array()
        .and_then(|fs| fs.iter().find(|f| f["filterName"].as_str() == Some(SOS_MASQUE_FILTRE)));
    // Filtre d'un autre kind (v1 obsolète posé par une version antérieure)
    // → retiré puis recréé en v2 ci-dessous.
    let existant = match existant {
        Some(f) if f["filterKind"].as_str() != Some(SOS_MASQUE_KIND) => {
            retirer_filtre_masque(write, read, input_name).await;
            None
        }
        autre => autre,
    };
    if let Some(f) = existant {
        if f["filterSettings"]["image_path"].as_str() == Some(chemin_png) {
            return Ok(());
        }
        rpc(
            write,
            read,
            "SetSourceFilterSettings",
            "masque_set",
            json!({
                "sourceName": input_name,
                "filterName": SOS_MASQUE_FILTRE,
                "filterSettings": reglages_masque(chemin_png),
                "overlay": true
            }),
        )
        .await?;
    } else {
        rpc(
            write,
            read,
            "CreateSourceFilter",
            "masque_create",
            json!({
                "sourceName": input_name,
                "filterName": SOS_MASQUE_FILTRE,
                "filterKind": SOS_MASQUE_KIND,
                "filterSettings": reglages_masque(chemin_png)
            }),
        )
        .await?;
    }
    Ok(())
}

/// Retire le filtre SOS-Masque d'un input (cadre sans forme → caméra
/// rectangulaire, comportement d'avant le masque). Non-fatal si absent.
async fn retirer_filtre_masque(write: &mut WsSink, read: &mut WsStreamHalf, input_name: &str) {
    let _ = rpc(
        write,
        read,
        "RemoveSourceFilter",
        "masque_remove",
        json!({ "sourceName": input_name, "filterName": SOS_MASQUE_FILTRE }),
    )
    .await;
}

/// Commande frontend : découpe SOS-Caméra ET SOS-Caméra-Fond à la forme du
/// cadre SVG du widget caméra. `points` = polygone en coords widget-local
/// (source de vérité : genererClipPathCadre côté dashboard — même forme
/// que le trou destination-out de diffusion.html). `points` null/vide =
/// cadre sans forme (carre/sansCadre) → filtres retirés, caméra
/// rectangulaire.
///
/// Le filtre OBS s'applique à la résolution NATIVE de la source (avant le
/// transform de l'item) et `stretch` étire le PNG à la taille de base → le
/// masque est généré en pixels de la source, en mappant le polygone par
/// l'inverse du pipeline sync_trous (points_en_espace_source). La position
/// (x, y) du widget n'intervient PAS : un déplacement ne change pas le masque.
///
/// Retour : true si la caméra a été masquée (ou les filtres retirés),
/// false si la source n'était pas encore dimensionnée (capteur dshow en
/// démarrage) → le dashboard ne met pas en cache et retente au prochain
/// commit (auto-guérison).
///
/// One-shot : connect → scene_sos → par input (taille → mapping → PNG →
/// filtre) → nettoyage → close. PNG écrit dans `dossier_masques` (<data>/obs)
/// sous un nom versionné par hash du contenu. Erreur filtre → log +
/// continuation (la source reste rectangulaire), jamais d'échec global.
#[allow(clippy::too_many_arguments)]
pub async fn camera_mask_sync(
    host: &str,
    port: u16,
    password: &str,
    points: Option<Vec<(f64, f64)>>,
    w: f64,
    h: f64,
    fit: &str,
    zoom: f64,
    dossier_masques: &Path,
) -> Result<bool, String> {
    let (mut write, mut read) = connect(host, port, password).await?;
    let sos = scene_sos(&mut write, &mut read).await?
        .ok_or("OBS: scène « SOS » introuvable ou sans SOS-Diffusion")?;
    let scene_uuid = &sos.scene_uuid;

    // Cadre sans forme → caméra rectangulaire : retirer les filtres.
    let Some(points) = points.filter(|p| p.len() >= 3) else {
        for input in [SOS_CAMERA, SOS_CAMERA_FOND] {
            retirer_filtre_masque(&mut write, &mut read, input).await;
        }
        let _ = write.close().await;
        return Ok(true);
    };

    fs::create_dir_all(dossier_masques)
        .map_err(|e| format!("OBS masque: création du dossier impossible: {}", e))?;

    let mut fichiers_courants: Vec<PathBuf> = Vec::new();
    let mut camera_masquee = false;
    for input in [SOS_CAMERA, SOS_CAMERA_FOND] {
        // Taille de base : capteur dshow tout juste créé (sourceWidth encore
        // 0) → un retry court ; item absent ou toujours rien → skip.
        let mut taille =
            taille_base_item(&mut write, &mut read, scene_uuid, &sos.items, input).await;
        if taille.is_none()
            && input == SOS_CAMERA
            && sos.items.iter().any(|it| it["sourceName"].as_str() == Some(SOS_CAMERA))
        {
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            taille = taille_base_item(&mut write, &mut read, scene_uuid, &sos.items, input).await;
        }
        let Some((src_w, src_h)) = taille else {
            eprintln!("[OBS] masque: « {} » sans taille de base → skip", input);
            continue;
        };
        let (rw, rh) = (src_w.round().max(1.0) as u32, src_h.round().max(1.0) as u32);
        // Caméra : fit/zoom du widget. Plaque noire : toujours cover sans
        // crop (même transform qu'ensure_camera_fond / sync_trous).
        let (fit_src, zoom_src) = if input == SOS_CAMERA {
            (fit, zoom)
        } else {
            ("remplir", 1.0)
        };
        let pts_src =
            points_en_espace_source(&points, rw as f64, rh as f64, w, h, fit_src, zoom_src);
        let couverture = mask_png::rasteriser_polygone(&pts_src, rw, rh);
        let png = mask_png::encoder_png_rgba(&couverture, rw, rh);

        let mut hacheur = Sha256::new();
        hacheur.update(&png);
        let chemin = dossier_masques.join(format!(
            "masque-{}-{}.png",
            if input == SOS_CAMERA { "camera" } else { "fond" },
            hex_court(&hacheur.finalize())
        ));
        if !chemin.exists() {
            fs::write(&chemin, &png)
                .map_err(|e| format!("OBS masque: écriture {}: {}", chemin.display(), e))?;
        }

        if let Err(e) =
            appliquer_filtre_masque(&mut write, &mut read, input, &chemin.to_string_lossy()).await
        {
            eprintln!("[OBS] masque: filtre « {} »: {}", input, e);
            continue;
        }
        fichiers_courants.push(chemin);
        if input == SOS_CAMERA {
            camera_masquee = true;
        }
    }

    // Nettoyage des masques orphelins (générations précédentes) : on garde
    // les fichiers courants + ceux encore référencés par un filtre (input
    // sauté ou apply échoué → son ancien PNG doit survivre).
    let mut chemins_gardes = fichiers_courants;
    for input in [SOS_CAMERA, SOS_CAMERA_FOND] {
        if let Some(chemin) = chemin_filtre_actuel(&mut write, &mut read, input).await {
            chemins_gardes.push(PathBuf::from(chemin));
        }
    }
    if let Ok(entrees) = fs::read_dir(dossier_masques) {
        for entree in entrees.flatten() {
            let chemin = entree.path();
            let est_masque = chemin
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("masque-") && n.ends_with(".png"));
            if est_masque && !chemins_gardes.contains(&chemin) {
                let _ = fs::remove_file(&chemin);
            }
        }
    }

    let _ = write.close().await;
    Ok(camera_masquee)
}

/// Synchronise les captures SOS-Trou-* et SOS-Fond-* avec la scène chargée.
/// Au chargement d'une scène :
///   - items SOS dont le nom commence par "SOS-Trou-" ET n'est dans AUCUN
///     widget.obsSource → SetSceneItemEnabled false (cacher, pas DeleteInput).
///   - items SOS-Trou-* qui SONT dans un widget.obsSource → enable true + sync
///     transform (position + taille = widget).
///   - items SOS-Fond-* : == scene.bgObsSource → enable true + transform
///     PLEIN CANVAS (fond de scène = capture app/jeu) ; sinon (autres
///     scènes) → cacher. Prend la Scene complète (widgets + bgObsSource
///     + canvasW/H).
///
/// One-shot : connect → scene_sos() → pour chaque item SOS-Trou-* / SOS-Fond-* →
/// SetSceneItemEnabled (+ SetSceneItemTransform si lié) → close.
/// Non-fatal : si OBS offline ou scène SOS absente → Ok silencieux.
pub async fn sync_scene_captures(
    host: &str,
    port: u16,
    password: &str,
    scene: &crate::scene::Scene,
) -> Result<(), String> {
    let widgets = &scene.widgets;
    let (mut write, mut read) = connect(host, port, password).await?;

    let Some(sos) = scene_sos(&mut write, &mut read).await? else {
        let _ = write.close().await;
        return Ok(());
    };
    let scene_uuid = &sos.scene_uuid;

    // Index des widgets par obsSource pour lookup rapide.
    use std::collections::HashMap;
    let widget_by_source: HashMap<&str, &crate::scene::Widget> = widgets
        .iter()
        .filter_map(|w| w.obsSource.as_deref().map(|s| (s, w)))
        .collect();

    // ===== Caméra : auto-guérison (contrat 2) =====
    // Widget caméra présent → garantir la plaque "SOS-Caméra-Fond" puis la
    // source "SOS-Caméra" (créer si absente, réactiver l'item, caler transform
    // + index sous SOS-Diffusion via reorder_sos_sources).
    // apply_device=false → on ne touche PAS aux settings d'un input existant
    // (pas de restart du capteur dshow au boot).
    // Pas de widget caméra → cacher les items orphelins caméra + fond
    // (inputs conservés, jamais RemoveInput).
    if let Some(cam) = widgets.iter().find(|w| w.widget_type == "camera") {
        if let Err(e) = ensure_camera(
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
        .await
        {
            eprintln!("[OBS] sync captures caméra/fond: {}", e);
        }
    } else {
        for name in [SOS_CAMERA, SOS_CAMERA_FOND] {
            if let Some(item_id) = sos
                .items
                .iter()
                .find(|it| it["sourceName"].as_str() == Some(name))
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
            }
        }
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
        }
    }

    // ===== Fond capture (SOS-Fond-<sceneId8>) =====
    // Item lié au bgObsSource de la scène chargée → enable + transform PLEIN
    // CANVAS (0,0,canvasW,canvasH, STRETCH, alignment 5 = top-left — même
    // convention que le sync des SOS-Trou-*). Les SOS-Fond-* des AUTRES
    // scènes → cacher (orphan hiding, même logique que SOS-Trou-*).
    let bg_cw = scene.canvasW as f64;
    let bg_ch = scene.canvasH as f64;
    for item in &sos.items {
        let Some(name) = item["sourceName"].as_str() else { continue };
        if !name.starts_with("SOS-Fond-") { continue; }
        let Some(item_id) = item["sceneItemId"].as_i64() else { continue };

        if scene.bgObsSource.as_deref() == Some(name) {
            let _ = rpc(
                &mut write,
                &mut read,
                "SetSceneItemEnabled",
                &format!("sync_bg_enable_{}", item_id),
                json!({
                    "sceneUuid": scene_uuid,
                    "sceneItemId": item_id,
                    "sceneItemEnabled": true
                }),
            )
            .await;
            let _ = rpc(
                &mut write,
                &mut read,
                "SetSceneItemTransform",
                &format!("sync_bg_transform_{}", item_id),
                json!({
                    "sceneUuid": scene_uuid,
                    "sceneItemId": item_id,
                    "sceneItemTransform": {
                        "positionX": 0,
                        "positionY": 0,
                        "boundsWidth": bg_cw,
                        "boundsHeight": bg_ch,
                        "boundsType": "OBS_BOUNDS_STRETCH",
                        "alignment": 5
                    }
                }),
            )
            .await;
        } else {
            let _ = rpc(
                &mut write,
                &mut read,
                "SetSceneItemEnabled",
                &format!("sync_bg_disable_{}", item_id),
                json!({
                    "sceneUuid": scene_uuid,
                    "sceneItemId": item_id,
                    "sceneItemEnabled": false
                }),
            )
            .await;
        }
    }

    // ===== Vérification/Maj ordre des sources au démarrage =====
    // Ordre visuel souhaité (bas → haut) : SOS-Fond-* < SOS-Trou-* <
    // SOS-Caméra-Fond < SOS-Caméra < SOS-Diffusion.
    // Reorder one-shot : GetSceneItemList → SetSceneItemIndex (bottom-up).
    reorder_sos_sources(&mut write, &mut read, scene_uuid).await;

    let _ = write.close().await;
    Ok(())
}

/// Force l'ordre visuel des sources SOS dans la scène :
///   HAUT de scène (index max, du haut vers le bas) : SOS-Diffusion >
///     SOS-Caméra > SOS-Caméra-Fond > SOS-Trou-* (ordre relatif conservé) —
///     le bloc SOS est AU-DESSUS de toutes les sources de l'utilisateur.
///   BAS de scène (indices 0..F-1) : SOS-Fond-* — captures plein canvas,
///     SOUS les sources de l'utilisateur aussi (sinon elles les couvrent).
///
/// Diffusion AU-DESSUS de tout : les widgets :4321 (chat/médias + cadres
/// SVG) passent devant les captures. La caméra apparaît via le trou
/// destination-out du canvas :4321 (les widgets qui chevauchent son
/// rectangle sont clippés côté diffusion.html) ; AU-DESSUS des trous pour
/// qu'une capture qui recouvre son rectangle ne masque pas le flux. La
/// plaque sous la caméra (mais au-dessus des trous) montre du noir quand
/// le capteur est off. Les trous fenêtre/jeu restent SOUS Diffusion
/// (destination-out inchangé). Les SOS-Fond-* (fond de scène = capture)
/// sont TOUT EN BAS : un widget trou qui recouvre le fond gagne dans son
/// rect, la plaque caméra reste au-dessus du fond capturé.
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
    let mut fond_id: Option<i64> = None;
    // Captures de FOND de scène (SOS-Fond-<sceneId8>) — tout en bas.
    let mut bg_capture_ids: Vec<i64> = Vec::new();

    for it in arr {
        let name = it["sourceName"].as_str().unwrap_or("");
        let id = it["sceneItemId"].as_i64();
        let Some(id) = id else { continue };
        if name == SOURCE_NAME {
            diffusion_id = Some(id);
        } else if name == SOS_CAMERA {
            camera_id = Some(id);
        } else if name == SOS_CAMERA_FOND {
            fond_id = Some(id);
        } else if name.starts_with("SOS-Trou-") {
            trou_ids.push(id);
        } else if name.starts_with("SOS-Fond-") {
            bg_capture_ids.push(id);
        }
    }

    // OBS 32.x : index 0 = BAS visuel, index max = HAUT visuel.
    // Ordre souhaité (du bas vers le haut) : Trou-* < Fond < Caméra < Diffusion.
    // Caméra AU-DESSUS des trous : un SOS-Trou-* qui recouvre le rectangle
    // caméra ne masque plus le flux dshow ; la plaque reste sous la caméra
    // mais au-dessus des trous (noir, pas le jeu, quand le capteur est off).
    // SetSceneItemIndex du HAUT vers le BAS (diffusion, caméra, fond, puis
    // trous) pour ne pas déplacer les items déjà positionnés.
    // Liste ordonnée haut→bas : gère les absences (caméra/fond/diffusion
    // optionnels) — chaque item présent prend le prochain index décroissant.
    let mut top_to_bottom: Vec<i64> = Vec::new();
    if let Some(id) = diffusion_id {
        top_to_bottom.push(id);
    }
    if let Some(id) = camera_id {
        top_to_bottom.push(id);
    }
    if let Some(id) = fond_id {
        top_to_bottom.push(id);
    }
    top_to_bottom.extend(trou_ids.iter().copied());
    // SOS-Fond-* PAS dans le bloc du haut : une capture de fond est PLEIN
    // CANVAS — au-dessus des sources utilisateur elle les masquerait
    // entièrement. Elle reste aux indices 0..F-1 (tout en bas de la scène).

    // Départ = index MAX de la scène (arr.len()-1), PAS le nombre d'items SOS :
    // le bloc SOS occupe le HAUT de la scène, au-dessus des sources de
    // l'utilisateur (captures, médias). Avec top_to_bottom.len()-1 il se
    // retrouvait rangé en BAS (indices 0..N) → toute source non-SOS passait
    // devant SOS-Diffusion.
    let mut idx = arr.len() as i64 - 1; // index le plus haut = Diffusion
    for id in top_to_bottom {
        let _ = rpc(write, read, "SetSceneItemIndex", &format!("reorder_{}", id), json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": id,
            "sceneItemIndex": idx
        })).await;
        idx -= 1;
    }
    // Captures de fond : indices 0..F-1, SOUS toutes les sources (bloc SOS
    // et sources utilisateur).
    for (i, id) in bg_capture_ids.iter().enumerate() {
        let _ = rpc(write, read, "SetSceneItemIndex", &format!("reorder_bg_{}", id), json!({
            "sceneUuid": scene_uuid,
            "sceneItemId": id,
            "sceneItemIndex": i as i64
        })).await;
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
            settings.insert("capture_mode".to_string(), Value::String("capture_specific_window".to_string()));
            if let Some(win) = &target {
                settings.insert("window".to_string(), Value::String(win.clone()));
            }
        }
    }

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
            set_res?;
        } else {
            // Input existe ailleurs (SOS Web) → nouveau nom avec suffixe.
            effective_name = format!("{}-sos", source_name);
            // Si effective_name existe déjà aussi → SetInputSettings, sinon CreateInput.
            if global_inputs.iter().any(|n| n == &effective_name) {
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
                set_res?;
            } else {
                let create_json = json!({
                    "sceneUuid": scene_uuid,
                    "inputName": effective_name,
                    "inputKind": kind.input_kind(),
                    "inputSettings": Value::Object(settings.clone()),
                    "sceneItemEnabled": true
                });
                let create_res = rpc(
                    &mut write,
                    &mut read,
                    "CreateInput",
                    "pc_create",
                    create_json,
                )
                .await;
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
        let create_res = rpc(
            &mut write,
            &mut read,
            "CreateInput",
            "pc_create",
            create_json,
        )
        .await;
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

            match cs_res {
                Ok(created) => {
                    created["sceneItemId"]
                        .as_i64()
                        .ok_or("OBS: CreateSceneItem réponse sans sceneItemId")?
                }
                Err(_) => {
                    // CreateSceneItem échoué → CreateInput avec nom unique timestamp.
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let unique_name = format!("{}-{}", source_name, ts);
                    effective_name = unique_name.clone();
                    let create_json = json!({
                        "sceneUuid": scene_uuid,
                        "inputName": unique_name,
                        "inputKind": kind.input_kind(),
                        "inputSettings": Value::Object(settings.clone()),
                        "sceneItemEnabled": true
                    });
                    let create_res = rpc(
                        &mut write,
                        &mut read,
                        "CreateInput",
                        "pc_create_fallback",
                        create_json,
                    )
                    .await;
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

    // 6. Reorder global : Diffusion > Caméra > Fond > Trou (haut → bas).
    //    reorder_sos_sources est le SEUL endroit qui gère les indices.
    reorder_sos_sources(&mut write, &mut read, scene_uuid).await;

    let _ = write.close().await;
    Ok(effective_name)
}
