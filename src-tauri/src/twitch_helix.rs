/// Communauté lecture — endpoints Helix (followers, subs, viewers, broadcaster).
///
/// Un seul helper central `helix_get` : lit les tokens du coffre, fait le GET,
/// gère 401 (refresh + 1 retry) et 403 (NeedReauth — JAMAIS de Device Code auto).
///
/// Endpoints :
///   GET /channels/followers?broadcaster_id=&first=100&after=  (moderator:read:followers)
///   GET /subscriptions?broadcaster_id=&first=100&after=       (channel:read:subscriptions)
///   GET /streams?user_id=                                      (user:read:broadcast)
///   GET /users?id=                                             (pas de scope spécial)
///
/// Pagination bornée à MAX_PAGES (10 pages × 100 = 1000 entrées max).
use crate::twitch_auth::{self, CLIENT_ID};
use serde::Serialize;
use std::sync::OnceLock;

const HELIX_BASE: &str = "https://api.twitch.tv/helix";
const MAX_PAGES: usize = 10;
const PAGE_SIZE: usize = 100;

/// Erreur Helix. NeedReauth = 403 (scopes manquants), Deconnecte = pas de token.
#[derive(Debug)]
pub enum HelixError {
    NeedReauth,
    Deconnecte,
    Autre(String),
}

/// Client HTTP partagé (un seul reqwest::Client pour tous les appels Helix).
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Helper central : GET Helix avec auth. Gère 401 (refresh + retry) et 403.
/// `access` = access token de la session courante (depuis TwitchState, pas keyring).
/// Pour le 401 refresh, on lit le refresh token depuis le keyring (rare).
/// Retourne le body JSON parsé. JAMAIS de Device Code auto au 403.
async fn helix_get(path: &str, query: &[(&str, &str)], access: &str) -> Result<serde_json::Value, HelixError> {
    let client = shared_client();

    // 1er essai avec l'access token de la session.
    let resp = client
        .get(format!("{}{}", HELIX_BASE, path))
        .header("Authorization", format!("Bearer {}", access))
        .header("Client-Id", CLIENT_ID)
        .query(query)
        .send()
        .await
        .map_err(|e| HelixError::Autre(format!("Helix GET: {}", e)))?;

    let status = resp.status();
    // 401 → refresh + 1 retry. On lit le refresh token depuis le keyring.
    if status == reqwest::StatusCode::UNAUTHORIZED {
        eprintln!("[Twitch] Helix 401 → refresh...");
        let tokens = twitch_auth::lire_tokens()
            .map_err(|e| HelixError::Autre(format!("Coffre: {}", e)))?
            .ok_or(HelixError::Deconnecte)?;
        let new_tokens = twitch_auth::refresh_token(&tokens.refresh)
            .await
            .map_err(|e| {
                eprintln!("[Twitch] Helix refresh échoué: {}", e);
                let _ = twitch_auth::effacer_tokens();
                HelixError::Deconnecte
            })?;
        twitch_auth::sauver_tokens(&new_tokens)
            .map_err(|e| HelixError::Autre(format!("Coffre save: {}", e)))?;

        // Retry avec le nouveau token.
        let resp2 = client
            .get(format!("{}{}", HELIX_BASE, path))
            .header("Authorization", format!("Bearer {}", new_tokens.access))
            .header("Client-Id", CLIENT_ID)
            .query(query)
            .send()
            .await
            .map_err(|e| HelixError::Autre(format!("Helix GET retry: {}", e)))?;

        return parse_resp(resp2).await;
    }

    // 403 → NeedReauth (scopes manquants). JAMAIS de Device Code auto.
    if status == reqwest::StatusCode::FORBIDDEN {
        eprintln!("[Twitch] Helix 403 → need_reauth (scopes manquants)");
        return Err(HelixError::NeedReauth);
    }

    parse_resp(resp).await
}

/// Parse la réponse HTTP en JSON (ou erreur HelixError).
async fn parse_resp(resp: reqwest::Response) -> Result<serde_json::Value, HelixError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(HelixError::Autre(format!("Helix HTTP {}: {}", status, body)));
    }
    resp.json()
        .await
        .map_err(|e| HelixError::Autre(format!("Helix parse: {}", e)))
}

// ===== Types de réponse =====

#[derive(Debug, Clone, Serialize)]
pub struct FollowerEntry {
    pub login: String,
    pub user_id: String,
    pub followed_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FollowersResp {
    pub total: u64,
    pub liste: Vec<FollowerEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubEntry {
    pub login: String,
    pub user_id: String,
    pub tier: String,
    pub is_gift: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubsResp {
    pub total: u64,
    pub points: u64,
    pub liste: Vec<SubEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BroadcasterInfo {
    pub display_name: String,
    pub profile_image_url: String,
    pub broadcaster_type: String,
    pub description: String,
}

// ===== Fonctions publiques =====

/// Followers : GET /channels/followers?broadcaster_id=... — pagination curseur,
/// max MAX_PAGES pages. Retourne total + liste.
pub async fn followers(broadcaster_id: &str, access: &str) -> Result<FollowersResp, HelixError> {
    let mut liste: Vec<FollowerEntry> = Vec::new();
    let mut total: u64 = 0;
    let mut after: Option<String> = None;
    let first = PAGE_SIZE.to_string();

    for page in 0..MAX_PAGES {
        let mut query: Vec<(&str, &str)> = vec![
            ("broadcaster_id", broadcaster_id),
            ("first", &first),
        ];
        if let Some(cursor) = &after {
            query.push(("after", cursor));
        }

        let body = helix_get("/channels/followers", &query, access).await?;

        // total est dans la réponse racine.
        if let Some(t) = body["total"].as_u64() {
            total = t;
        }

        // data = array de followers.
        let data = body["data"]
            .as_array()
            .ok_or_else(|| HelixError::Autre("followers: data absent".into()))?;

        for f in data {
            liste.push(FollowerEntry {
                login: f["user_login"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                user_id: f["user_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                followed_at: f["followed_at"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            });
        }

        // Pagination : after cursor.
        after = body["pagination"]["cursor"]
            .as_str()
            .map(|s| s.to_string());

        if after.is_none() || data.is_empty() {
            break;
        }
        eprintln!("[Twitch] Helix followers page {} ({} entrées)", page + 1, data.len());
    }

    eprintln!(
        "[Twitch] Helix followers: total={} liste={}",
        total,
        liste.len()
    );
    Ok(FollowersResp { total, liste })
}

/// Subs : GET /subscriptions?broadcaster_id=... — pagination, max MAX_PAGES.
/// Retourne total + points (sub points) + liste.
pub async fn subs(broadcaster_id: &str, access: &str) -> Result<SubsResp, HelixError> {
    let mut liste: Vec<SubEntry> = Vec::new();
    let mut total: u64 = 0;
    let mut points: u64 = 0;
    let mut after: Option<String> = None;
    let first = PAGE_SIZE.to_string();

    for page in 0..MAX_PAGES {
        let mut query: Vec<(&str, &str)> = vec![
            ("broadcaster_id", broadcaster_id),
            ("first", &first),
        ];
        if let Some(cursor) = &after {
            query.push(("after", cursor));
        }

        let body = helix_get("/subscriptions", &query, access).await?;

        if let Some(t) = body["total"].as_u64() {
            total = t;
        }
        if let Some(p) = body["points"].as_u64() {
            points = p;
        }

        let data = body["data"]
            .as_array()
            .ok_or_else(|| HelixError::Autre("subs: data absent".into()))?;

        for s in data {
            let user_id = s["user_id"].as_str().unwrap_or("");
            // L'API Twitch inclut le broadcaster dans data mais pas dans total.
            // On le filtre pour que liste.len() == total.
            if user_id == broadcaster_id {
                continue;
            }
            liste.push(SubEntry {
                login: s["user_login"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                user_id: user_id.to_string(),
                tier: s["tier"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                is_gift: s["is_gift"].as_bool().unwrap_or(false),
            });
        }

        after = body["pagination"]["cursor"]
            .as_str()
            .map(|s| s.to_string());

        if after.is_none() || data.is_empty() {
            break;
        }
        eprintln!("[Twitch] Helix subs page {} ({} entrées)", page + 1, data.len());
    }

    eprintln!(
        "[Twitch] Helix subs: total={} points={} liste={}",
        total,
        points,
        liste.len()
    );
    Ok(SubsResp {
        total,
        points,
        liste,
    })
}

/// Viewers live : GET /streams?user_id=... — si data vide → None (hors-ligne),
/// sinon Some(viewer_count).
pub async fn stream_viewers(broadcaster_id: &str, access: &str) -> Result<Option<u32>, HelixError> {
    let body = helix_get("/streams", &[("user_id", broadcaster_id)], access).await?;

    let data = body["data"]
        .as_array()
        .ok_or_else(|| HelixError::Autre("streams: data absent".into()))?;

    if data.is_empty() {
        eprintln!("[Twitch] Helix streams: hors-ligne");
        return Ok(None);
    }

    let viewers = data[0]["viewer_count"]
        .as_u64()
        .map(|v| v as u32)
        .ok_or_else(|| HelixError::Autre("streams: viewer_count absent".into()))?;

    eprintln!("[Twitch] Helix streams: viewers={}", viewers);
    Ok(Some(viewers))
}

/// Broadcaster : GET /users?id=... — display_name, profile_image_url,
/// broadcaster_type, description.
pub async fn broadcaster(user_id: &str, access: &str) -> Result<BroadcasterInfo, HelixError> {
    let body = helix_get("/users", &[("id", user_id)], access).await?;

    let data = body["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| HelixError::Autre("users: data[0] absent".into()))?;

    let info = BroadcasterInfo {
        display_name: data["display_name"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        profile_image_url: data["profile_image_url"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        broadcaster_type: data["broadcaster_type"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        description: data["description"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    };

    eprintln!(
        "[Twitch] Helix broadcaster: {} ({})",
        info.display_name, info.broadcaster_type
    );
    Ok(info)
}
