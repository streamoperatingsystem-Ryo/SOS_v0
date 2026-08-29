/// Communauté YouTube — Data API v3 (channel info, members, live viewers).
///
/// Helper central `youtube_get` : GET avec auth Bearer, gère 401 (refresh + retry).
/// Endpoints :
///   GET /channels?part=snippet,statistics&mine=true  → channel info + stats
///   GET /members?part=snippet                         → liste members
///   GET /videos?part=liveStreamingDetails&mine=true   → concurrentViewers (si live)
use crate::youtube_auth;
use serde::Serialize;
use std::sync::OnceLock;

const YOUTUBE_API: &str = "https://www.googleapis.com/youtube/v3";

/// Erreur YouTube Data. Deconnecte = pas de token, Autre = erreur réseau/API.
#[derive(Debug)]
pub enum YoutubeError {
    Deconnecte,
    Autre(String),
}

impl std::fmt::Display for YoutubeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YoutubeError::Deconnecte => write!(f, "YouTube non connecté"),
            YoutubeError::Autre(s) => write!(f, "{}", s),
        }
    }
}

/// Client HTTP partagé.
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Helper central : GET YouTube Data API avec auth. Gère 401 (refresh + retry).
/// `access` = access token de la session courante.
async fn youtube_get(
    path: &str,
    query: &[(&str, &str)],
    access: &str,
) -> Result<serde_json::Value, YoutubeError> {
    let client = shared_client();

    let resp = client
        .get(format!("{}{}", YOUTUBE_API, path))
        .header("Authorization", format!("Bearer {}", access))
        .query(query)
        .send()
        .await
        .map_err(|e| YoutubeError::Autre(format!("YouTube GET: {}", e)))?;

    let status = resp.status();

    // 401 → refresh + 1 retry.
    if status == reqwest::StatusCode::UNAUTHORIZED {
        eprintln!("[YouTube] Data 401 → refresh...");
        let tokens = youtube_auth::lire_tokens()
            .map_err(|e| YoutubeError::Autre(format!("Coffre: {}", e)))?
            .ok_or(YoutubeError::Deconnecte)?;
        let new_tokens = youtube_auth::refresh_token(&tokens.refresh, &tokens)
            .await
            .map_err(|e| {
                eprintln!("[YouTube] refresh échoué: {}", e);
                let _ = youtube_auth::effacer_tokens();
                YoutubeError::Deconnecte
            })?;
        youtube_auth::sauver_tokens(&new_tokens)
            .map_err(|e| YoutubeError::Autre(format!("Coffre save: {}", e)))?;

        // Retry avec le nouveau token.
        let resp2 = client
            .get(format!("{}{}", YOUTUBE_API, path))
            .header("Authorization", format!("Bearer {}", new_tokens.access))
            .query(query)
            .send()
            .await
            .map_err(|e| YoutubeError::Autre(format!("YouTube GET retry: {}", e)))?;

        return parse_resp(resp2).await;
    }

    parse_resp(resp).await
}

/// Parse la réponse HTTP en JSON (ou erreur YoutubeError).
async fn parse_resp(resp: reqwest::Response) -> Result<serde_json::Value, YoutubeError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(YoutubeError::Autre(format!("YouTube HTTP {}: {}", status, body)));
    }
    resp.json()
        .await
        .map_err(|e| YoutubeError::Autre(format!("YouTube parse: {}", e)))
}

// ===== Types de réponse =====

#[derive(Debug, Clone, Serialize)]
pub struct ChannelInfo {
    pub display_name: String,
    pub profile_image_url: String,
    pub subscriber_count: u64,
    pub view_count: u64,
    pub video_count: u64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemberEntry {
    pub display_name: String,
    pub memberships_level: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MembersResp {
    pub total: u64,
    pub liste: Vec<MemberEntry>,
}

// ===== Fonctions publiques =====

/// Channel info : GET /channels?part=snippet,statistics&mine=true
/// → display_name, profile_image_url, subscriberCount, viewCount, videoCount, description.
pub async fn channel_info(access: &str) -> Result<ChannelInfo, YoutubeError> {
    let body = youtube_get(
        "/channels",
        &[("part", "snippet,statistics"), ("mine", "true")],
        access,
    )
    .await?;

    let items = body["items"]
        .as_array()
        .ok_or_else(|| YoutubeError::Autre("channels: items absent".into()))?;

    if items.is_empty() {
        return Err(YoutubeError::Autre("channels: aucun item (compte sans chaîne YouTube?)".into()));
    }

    let item = &items[0];

    let info = ChannelInfo {
        display_name: item["snippet"]["title"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        profile_image_url: item["snippet"]["thumbnails"]["default"]["url"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        subscriber_count: item["statistics"]["subscriberCount"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        view_count: item["statistics"]["viewCount"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        video_count: item["statistics"]["videoCount"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        description: item["snippet"]["description"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    };

    eprintln!("[YouTube] channel: {} subs={}", info.display_name, info.subscriber_count);
    Ok(info)
}

/// Members : GET /members?part=snippet → liste members (displayName, membershipsLevel).
/// Retourne total + liste. total = items.len() (l'API ne fournit pas de count direct).
pub async fn members(access: &str) -> Result<MembersResp, YoutubeError> {
    let body = youtube_get("/members", &[("part", "snippet")], access).await?;

    let items = body["items"]
        .as_array()
        .ok_or_else(|| YoutubeError::Autre("members: items absent".into()))?;

    let mut liste = Vec::new();
    for m in items {
        let display_name = m["snippet"]["memberDetails"]["displayName"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let memberships_level = m["snippet"]["membershipsDetails"]["highestAccessibleMembershipLevel"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if !display_name.is_empty() {
            liste.push(MemberEntry {
                display_name,
                memberships_level,
            });
        }
    }

    let total = liste.len() as u64;

    eprintln!("[YouTube] members: total={} liste={}", total, liste.len());
    Ok(MembersResp { total, liste })
}

/// Live viewers : search?channelId=...&eventType=live → videoId →
/// /videos?part=liveStreamingDetails&id=<videoId> → concurrentViewers (si live).
/// None si pas de live.
pub async fn live_viewers(access: &str, channel_id: &str) -> Result<Option<u32>, YoutubeError> {
    // 1. Search : trouver le video ID du live actuel
    let search_body = youtube_get(
        "/search",
        &[
            ("part", "id"),
            ("channelId", channel_id),
            ("eventType", "live"),
            ("type", "video"),
            ("maxResults", "1"),
        ],
        access,
    )
    .await?;

    let search_items = search_body["items"]
        .as_array()
        .ok_or_else(|| YoutubeError::Autre("search: items absent".into()))?;

    if search_items.is_empty() {
        eprintln!("[YouTube] live: pas de live (search vide)");
        return Ok(None);
    }

    let video_id = search_items[0]["id"]["videoId"]
        .as_str()
        .ok_or_else(|| YoutubeError::Autre("search: videoId absent".into()))?;

    // 2. Videos : concurrentViewers
    let video_body = youtube_get(
        "/videos",
        &[("part", "liveStreamingDetails"), ("id", video_id)],
        access,
    )
    .await?;

    let video_items = video_body["items"]
        .as_array()
        .ok_or_else(|| YoutubeError::Autre("videos: items absent".into()))?;

    if video_items.is_empty() {
        eprintln!("[YouTube] live: pas de vidéo (hors-ligne)");
        return Ok(None);
    }

    let viewers = video_items[0]["liveStreamingDetails"]["concurrentViewers"]
        .as_str()
        .and_then(|s| s.parse::<u32>().ok());

    if viewers.is_some() {
        eprintln!("[YouTube] live: viewers={}", viewers.unwrap());
    } else {
        eprintln!("[YouTube] live: pas de concurrentViewers (hors-ligne ou pas live)");
    }

    Ok(viewers)
}

/// Resolve channel_id + login : GET /channels?part=snippet&mine=true
/// Utilisé après obtention du token pour remplir Tokens.channel_id + login.
pub async fn resolve_channel(access: &str) -> Result<(String, String), YoutubeError> {
    let body = youtube_get(
        "/channels",
        &[("part", "snippet"), ("mine", "true")],
        access,
    )
    .await?;

    let items = body["items"]
        .as_array()
        .ok_or_else(|| YoutubeError::Autre("channels: items absent".into()))?;

    if items.is_empty() {
        return Err(YoutubeError::Autre("channels: aucun item (compte sans chaîne YouTube?)".into()));
    }

    let channel_id = items[0]["id"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let login = items[0]["snippet"]["title"]
        .as_str()
        .unwrap_or("")
        .to_string();

    eprintln!("[YouTube] channel résolu: id={} login={}", channel_id, login);
    Ok((channel_id, login))
}
