/// Récupération et résolution des clips Twitch pour les clips de bienvenue.
///
/// Deux fonctions publiques :
///   - `liste_clips(broadcaster_id, access)` → GET /clips?broadcaster_id= (Helix).
///     Scopes existants suffisants (aucun nouveau scope requis).
///   - `resoudre_mp4(slug)` → URL MP4 signée via GQL Twitch (VideoAccessToken_Clip).
///     Cache en mémoire (slug → URL signée, TTL 10 min).
///
/// La résolution MP4 utilise la même technique que l'ancienne app (Streamlabs) :
///   1. Fallback regex `thumbnailToMp4` pour les clips anciens (format -preview-WxH.jpg).
///   2. GQL `VideoAccessToken_Clip` pour les clips modernes (URL signée obligatoire).
///
/// Le `<video>` de diffusion.html streame ensuite depuis le CDN Twitch : lecture
/// immédiate, zéro chrome, zéro stockage local.
use crate::twitch_auth::CLIENT_ID;
use serde::Serialize;
use std::sync::OnceLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const HELIX_BASE: &str = "https://api.twitch.tv/helix";
const GQL_URL: &str = "https://gql.twitch.tv/gql";
/// Client-Id public GQL (même que l'ancienne app + Streamlabs).
const GQL_CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";
/// Hash de la persisted query VideoAccessToken_Clip.
const GQL_CLIP_HASH: &str = "36b89d2507fce29e5ca551df756d27c1cfe079e2609642b4390aa4c35796eb11";
const CACHE_TTL: Duration = Duration::from_secs(600); // 10 min

/// Client HTTP partagé (un seul reqwest::Client pour tous les appels).
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

// ===== Types publics =====

/// Un clip Twitch (champs utiles pour l'UI + la résolution MP4).
#[derive(Debug, Clone, Serialize)]
pub struct ClipInfo {
    pub id: String,
    pub title: String,
    pub url: String,           // URL twitch.tv/clips/<id> (page web)
    pub thumbnail_url: String,
    pub duration: f64,         // secondes
    pub created_at: String,
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    pub broadcaster_name: String,
}

/// Clip résolu : ClipInfo + URL MP4 signée prête à être lue par <video>.
#[derive(Debug, Clone, Serialize)]
pub struct ClipResolu {
    #[serde(flatten)]
    pub info: ClipInfo,
    pub mp4_url: String,
    pub duree_ms: u64,
}

// ===== Cache URL signée =====

struct CacheEntry {
    url: String,
    expires: Instant,
}

/// Cache global slug → URL signée (TTL 10 min). OnceLock pour init paresseuse.
fn url_cache() -> &'static std::sync::Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

// ===== Helix : liste des clips d'un broadcaster =====

/// Récupère les clips récents d'un broadcaster (GET /clips?broadcaster_id=).
/// `first` = nombre de clips (max 100, défaut 20). Retourne la liste triée
/// par date décroissante (ordre Helix par défaut).
pub async fn liste_clips(
    broadcaster_id: &str,
    access: &str,
    first: u32,
) -> Result<Vec<ClipInfo>, String> {
    let client = shared_client();
    let first_str = first.to_string();

    let resp = client
        .get(format!("{}/clips", HELIX_BASE))
        .header("Authorization", format!("Bearer {}", access))
        .header("Client-Id", CLIENT_ID)
        .query(&[
            ("broadcaster_id", broadcaster_id),
            ("first", first_str.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("Helix clips GET: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Helix clips HTTP {}: {}", status, body));
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Helix clips parse: {}", e))?;

    let data = v["data"]
        .as_array()
        .ok_or_else(|| "Helix clips: data absent".to_string())?;

    let mut clips = Vec::with_capacity(data.len());
    for c in data {
        clips.push(ClipInfo {
            id: c["id"].as_str().unwrap_or("").to_string(),
            title: c["title"].as_str().unwrap_or("").to_string(),
            url: c["url"].as_str().unwrap_or("").to_string(),
            thumbnail_url: c["thumbnail_url"].as_str().unwrap_or("").to_string(),
            duration: c["duration"].as_f64().unwrap_or(0.0),
            created_at: c["created_at"].as_str().unwrap_or("").to_string(),
            broadcaster_id: c["broadcaster_id"].as_str().unwrap_or("").to_string(),
            broadcaster_login: c["broadcaster_login"].as_str().unwrap_or("").to_string(),
            broadcaster_name: c["broadcaster_name"].as_str().unwrap_or("").to_string(),
        });
    }

    Ok(clips)
}

// ===== Résolution MP4 =====

/// Dérive l'URL MP4 d'un clip ancien depuis son thumbnail_url.
/// Format ancien : ...-preview-480x272.jpg → ....mp4
/// Retourne None si le format ne correspond pas (clip moderne → GQL nécessaire).
/// (Non appelé pour l'instant — gardé pour la résolution GQL des clips modernes.)
#[allow(dead_code)]
pub fn thumbnail_to_mp4(thumbnail_url: &str) -> Option<String> {
    if thumbnail_url.is_empty() {
        return None;
    }
    // Chercher le suffixe -preview-WxH.jpg (sans regex, manipulation manuelle).
    let marker = "-preview-";
    let pos = thumbnail_url.rfind(marker)?;
    let after = &thumbnail_url[pos + marker.len()..];
    // after doit être "480x272.jpg" → on cherche le .jpg final.
    let jpg_pos = after.rfind(".jpg")?;
    let dims = &after[..jpg_pos];
    // dims doit contenir un 'x' (ex: "480x272").
    if !dims.contains('x') {
        return None;
    }
    // Remplacer -preview-WxH.jpg par .mp4.
    let base = &thumbnail_url[..pos];
    Some(format!("{}.mp4", base))
}

/// Résout l'URL MP4 signée d'un clip Twitch via GQL (VideoAccessToken_Clip).
/// D'abord tente le cache, puis le fallback regex (clips anciens), puis GQL.
/// Retourne l'URL signée ou Err si tout échoue.
pub async fn resoudre_mp4(slug: &str) -> Result<String, String> {
    if slug.is_empty() {
        return Err("slug vide".to_string());
    }

    // 1. Cache
    {
        let cache = url_cache().lock().unwrap();
        if let Some(entry) = cache.get(slug) {
            if entry.expires > Instant::now() {
                return Ok(entry.url.clone());
            }
        }
    }

    // 2. GQL (résout aussi les clips anciens — pas besoin du fallback regex
    //    en première intention, GQL couvre tous les formats).
    match gql_clip_mp4(slug).await {
        Ok(url) => {
            // Stocker dans le cache.
            {
                let mut cache = url_cache().lock().unwrap();
                cache.insert(
                    slug.to_string(),
                    CacheEntry {
                        url: url.clone(),
                        expires: Instant::now() + CACHE_TTL,
                    },
                );
            }
            Ok(url)
        }
        Err(e) => {
            Err(format!("Résolution MP4 échouée pour {}: {}", slug, e))
        }
    }
}

/// Récupère un clip + son URL MP4 signée (liste_clips + resoudre_mp4).
/// Utilisé par l'attribution automatique : fetch les clips du broadcaster,
/// prend un clip aléatoire, résout son MP4.
pub async fn clip_aleatoire_resolu(
    broadcaster_id: &str,
    access: &str,
) -> Result<Option<ClipResolu>, String> {
    let clips = liste_clips(broadcaster_id, access, 20).await?;
    if clips.is_empty() {
        return Ok(None);
    }
    // Clip aléatoire parmi la liste.
    let idx = (uuid::Uuid::new_v4().as_u128() as usize) % clips.len();
    let clip = clips[idx].clone();
    let mp4_url = resoudre_mp4(&clip.id).await?;
    let duree_ms = (clip.duration * 1000.0).round() as u64;
    Ok(Some(ClipResolu {
        info: clip,
        mp4_url,
        duree_ms,
    }))
}

// ===== GQL interne =====

/// Appel GQL VideoAccessToken_Clip → URL MP4 signée.
/// Retourne sourceURL?sig=...&token=... (qualité 1080 ou 720, sinon la 1ère).
async fn gql_clip_mp4(slug: &str) -> Result<String, String> {
    let client = shared_client();

    // 1er essai : persisted query (hash).
    let body = serde_json::json!([{
        "operationName": "VideoAccessToken_Clip",
        "variables": { "slug": slug },
        "extensions": {
            "persistedQuery": {
                "version": 1,
                "sha256Hash": GQL_CLIP_HASH
            }
        }
    }]);

    let resp = client
        .post(GQL_URL)
        .header("Client-Id", GQL_CLIENT_ID)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("GQL POST: {}", e))?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("GQL body: {}", e))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("GQL parse (status {}): {}", status, e))?;

    // Si PersistedQueryNotFound → retry avec query inline.
    let errors = json[0]["errors"].as_array();
    let needs_inline = errors
        .map(|arr| {
            arr.iter().any(|e| {
                e["message"]
                    .as_str()
                    .map(|m| m == "PersistedQueryNotFound")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let clip_data = if needs_inline {
        gql_clip_mp4_inline(slug).await?
    } else {
        json[0]["data"]["clip"]
            .clone()
    };

    if clip_data.is_null() {
        return Err("clip null dans la réponse GQL".to_string());
    }

    extraire_url_signee(&clip_data)
}

/// Retry GQL avec query inline (si persisted query non trouvée).
async fn gql_clip_mp4_inline(slug: &str) -> Result<serde_json::Value, String> {
    let client = shared_client();
    let query = r#"query VideoAccessToken_Clip($slug: ID!) {
        clip(slug: $slug) {
            id slug title durationSeconds url
            videoQualities { quality frameRate sourceURL }
            playbackAccessToken(params: {platform: "web", playerBackend: "mediaplayer", playerType: "site"}) {
                signature value
            }
            thumbnailURL
        }
    }"#;

    let body = serde_json::json!([{
        "operationName": "VideoAccessToken_Clip",
        "variables": { "slug": slug },
        "query": query
    }]);

    let resp = client
        .post(GQL_URL)
        .header("Client-Id", GQL_CLIENT_ID)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("GQL inline POST: {}", e))?;

    let text = resp.text().await.map_err(|e| format!("GQL inline body: {}", e))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("GQL inline parse: {}", e))?;

    Ok(json[0]["data"]["clip"].clone())
}

/// Extrait l'URL signée depuis les données GQL d'un clip.
/// Choisit la qualité 1080 ou 720, sinon la 1ère disponible.
fn extraire_url_signee(clip: &serde_json::Value) -> Result<String, String> {
    let token = clip["playbackAccessToken"]
        .as_object()
        .ok_or_else(|| "playbackAccessToken absent".to_string())?;
    let signature = token["signature"]
        .as_str()
        .ok_or_else(|| "signature absent".to_string())?;
    let value = token["value"]
        .as_str()
        .ok_or_else(|| "value absent".to_string())?;

    let qualities = clip["videoQualities"]
        .as_array()
        .ok_or_else(|| "videoQualities absent".to_string())?;
    if qualities.is_empty() {
        return Err("videoQualities vide".to_string());
    }

    // Préférer 1080, sinon 720, sinon la 1ère.
    let best = qualities
        .iter()
        .find(|q| q["quality"].as_str() == Some("1080"))
        .or_else(|| {
            qualities
                .iter()
                .find(|q| q["quality"].as_str() == Some("720"))
        })
        .unwrap_or(&qualities[0]);

    let source_url = best["sourceURL"]
        .as_str()
        .ok_or_else(|| "sourceURL absent".to_string())?;

    // Construire l'URL signée : sourceURL?sig=...&token=...
    // (sans crate url — append manuel selon présence de '?').
    let sep = if source_url.contains('?') { "&" } else { "?" };
    Ok(format!(
        "{}{}sig={}&token={}",
        source_url, sep, signature, value
    ))
}
