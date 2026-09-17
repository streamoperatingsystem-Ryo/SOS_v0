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

// ===== Helpers d'écriture (POST/DELETE/PATCH) =====

/// Helper central : POST Helix avec auth. Gère 401 (refresh + retry) et 403.
/// `body` = corps JSON sérialisé. Retourne le body JSON de la réponse.
async fn helix_post(
    path: &str,
    query: &[(&str, &str)],
    body: &serde_json::Value,
    access: &str,
) -> Result<serde_json::Value, HelixError> {
    helix_write("POST", path, query, Some(body), access).await
}

/// Helper central : DELETE Helix avec auth. Gère 401 (refresh + retry) et 403.
/// Pas de corps (DELETE Helix n'envoie pas de body).
async fn helix_delete(
    path: &str,
    query: &[(&str, &str)],
    access: &str,
) -> Result<serde_json::Value, HelixError> {
    helix_write("DELETE", path, query, None, access).await
}

/// Helper central pour les méthodes d'écriture (POST/DELETE). Gère 401 refresh
/// + retry et 403 → NeedReauth. `body` = None pour DELETE.
async fn helix_write(
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    body: Option<&serde_json::Value>,
    access: &str,
) -> Result<serde_json::Value, HelixError> {
    let client = shared_client();
    let url = format!("{}{}", HELIX_BASE, path);

    let req = client
        .request(reqwest::Method::from_bytes(method.as_bytes()).unwrap(), &url)
        .header("Authorization", format!("Bearer {}", access))
        .header("Client-Id", CLIENT_ID)
        .query(query);
    let req = if let Some(b) = body {
        req.json(b)
    } else {
        req
    };

    let resp = req
        .send()
        .await
        .map_err(|e| HelixError::Autre(format!("Helix {}: {}", method, e)))?;

    let status = resp.status();
    // 401 → refresh + 1 retry (même logique que helix_get).
    if status == reqwest::StatusCode::UNAUTHORIZED {
        eprintln!("[Twitch] Helix {} 401 → refresh...", method);
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

        let req2 = client
            .request(reqwest::Method::from_bytes(method.as_bytes()).unwrap(), &url)
            .header("Authorization", format!("Bearer {}", new_tokens.access))
            .header("Client-Id", CLIENT_ID)
            .query(query);
        let req2 = if let Some(b) = body {
            req2.json(b)
        } else {
            req2
        };
        let resp2 = req2
            .send()
            .await
            .map_err(|e| HelixError::Autre(format!("Helix {} retry: {}", method, e)))?;
        return parse_resp(resp2).await;
    }

    // 403 → NeedReauth (scopes manquants).
    if status == reqwest::StatusCode::FORBIDDEN {
        eprintln!("[Twitch] Helix {} 403 → need_reauth (scopes manquants)", method);
        return Err(HelixError::NeedReauth);
    }

    parse_resp(resp).await
}

// ===== Types de réponse =====

#[derive(Debug, Clone, Serialize)]
pub struct FollowerEntry {
    pub login: String,
    pub user_id: String,
    pub followed_at: String,
    /// Display name (récupéré depuis user_name dans la réponse Helix).
    #[serde(default)]
    pub display_name: String,
    /// URL de la photo de profil (enrichi via batch /users?id=...).
    #[serde(default)]
    pub profile_image_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FollowersResp {
    pub total: u64,
    pub liste: Vec<FollowerEntry>,
}

/// Version allégée de FollowerEntry pour le polling des alertes :
/// user_id + followed_at uniquement (pas d'avatars, pas de display_name).
/// Évite la double pagination + enrichissement /users quand le poll des
/// alertes (60s) tourne en parallèle du chargement communauté (boot).
#[derive(Debug, Clone, Serialize)]
pub struct FollowerLight {
    pub user_id: String,
    pub login: String,
    pub followed_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubEntry {
    pub login: String,
    pub user_id: String,
    pub tier: String,
    pub is_gift: bool,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub profile_image_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubsResp {
    pub total: u64,
    pub points: u64,
    pub liste: Vec<SubEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BroadcasterInfo {
    pub user_id: String,
    pub login: String,
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
                display_name: f["user_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                profile_image_url: String::new(),
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

/// Récupère display_name + profile_image_url pour une liste de user_ids
/// via batch GET /users?id=... (max 100 par requête). Retourne une map
/// user_id -> (display_name, profile_image_url).
pub async fn fetch_users_batch(
    user_ids: &[String],
    access: &str,
) -> Result<std::collections::HashMap<String, (String, String)>, HelixError> {
    use std::collections::HashMap;
    let mut map: HashMap<String, (String, String)> = HashMap::new();
    if user_ids.is_empty() {
        return Ok(map);
    }
    eprintln!(
        "[Twitch] Helix batch /users : {} utilisateur(s) à enrichir",
        user_ids.len()
    );
    for chunk in user_ids.chunks(PAGE_SIZE) {
        let query: Vec<(&str, &str)> = chunk
            .iter()
            .map(|id| ("id", id.as_str()))
            .collect::<Vec<_>>();
        let body = helix_get("/users", &query, access).await?;
        let data = body["data"]
            .as_array()
            .ok_or_else(|| HelixError::Autre("batch /users: data absent".into()))?;
        for u in data {
            let uid = u["id"].as_str().unwrap_or("").to_string();
            if uid.is_empty() {
                continue;
            }
            map.insert(
                uid,
                (
                    u["display_name"].as_str().unwrap_or("").to_string(),
                    u["profile_image_url"].as_str().unwrap_or("").to_string(),
                ),
            );
        }
    }
    eprintln!("[Twitch] Helix batch /users : {} enrichi(s)", map.len());
    Ok(map)
}

/// Enrichit une liste de followers avec les photos de profil via batch /users.
/// Remplit `profile_image_url` et `display_name` pour chaque follower trouvé.
pub async fn enrichir_avatars(
    liste: &mut [FollowerEntry],
    access: &str,
) -> Result<(), HelixError> {
    let ids: Vec<String> = liste.iter().map(|f| f.user_id.clone()).collect();
    let map = fetch_users_batch(&ids, access).await?;
    for f in liste {
        if let Some((dn, url)) = map.get(&f.user_id) {
            if !dn.is_empty() {
                f.display_name = dn.clone();
            }
            f.profile_image_url = url.clone();
        }
    }
    Ok(())
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
                display_name: s["user_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                profile_image_url: String::new(),
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
        user_id: data["id"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        login: data["login"]
            .as_str()
            .unwrap_or("")
            .to_string(),
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

// ===== Modération (lecture + écriture) =====

#[derive(Debug, Clone, Serialize)]
pub struct VipEntry {
    pub user_id: String,
    pub login: String,
    pub display_name: String,
    #[serde(default)]
    pub profile_image_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModEntry {
    pub user_id: String,
    pub login: String,
    pub display_name: String,
    #[serde(default)]
    pub profile_image_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BannedEntry {
    pub user_id: String,
    pub login: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub profile_image_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedUser {
    pub user_id: String,
    pub login: String,
    pub display_name: String,
}

/// Liste les VIPs : GET /channels/vips?broadcaster_id=... — pagination.
pub async fn list_vips(broadcaster_id: &str, access: &str) -> Result<Vec<VipEntry>, HelixError> {
    let mut liste: Vec<VipEntry> = Vec::new();
    let mut after: Option<String> = None;
    let first = PAGE_SIZE.to_string();

    for _ in 0..MAX_PAGES {
        let mut query: Vec<(&str, &str)> = vec![
            ("broadcaster_id", broadcaster_id),
            ("first", &first),
        ];
        if let Some(cursor) = &after {
            query.push(("after", cursor));
        }
        let body = helix_get("/channels/vips", &query, access).await?;
        let data = body["data"]
            .as_array()
            .ok_or_else(|| HelixError::Autre("vips: data absent".into()))?;
        for v in data {
            liste.push(VipEntry {
                user_id: v["user_id"].as_str().unwrap_or("").to_string(),
                login: v["user_login"].as_str().unwrap_or("").to_string(),
                display_name: v["user_name"].as_str().unwrap_or("").to_string(),
                profile_image_url: String::new(),
            });
        }
        after = body["pagination"]["cursor"].as_str().map(|s| s.to_string());
        if after.is_none() || data.is_empty() {
            break;
        }
    }
    eprintln!("[Twitch] Helix vips: {} entrées", liste.len());
    Ok(liste)
}

/// Liste les modérateurs : GET /moderation/moderators?broadcaster_id=... — pagination.
pub async fn list_moderators(
    broadcaster_id: &str,
    access: &str,
) -> Result<Vec<ModEntry>, HelixError> {
    let mut liste: Vec<ModEntry> = Vec::new();
    let mut after: Option<String> = None;
    let first = PAGE_SIZE.to_string();

    for _ in 0..MAX_PAGES {
        let mut query: Vec<(&str, &str)> = vec![
            ("broadcaster_id", broadcaster_id),
            ("first", &first),
        ];
        if let Some(cursor) = &after {
            query.push(("after", cursor));
        }
        let body = helix_get("/moderation/moderators", &query, access).await?;
        let data = body["data"]
            .as_array()
            .ok_or_else(|| HelixError::Autre("moderators: data absent".into()))?;
        for m in data {
            liste.push(ModEntry {
                user_id: m["user_id"].as_str().unwrap_or("").to_string(),
                login: m["user_login"].as_str().unwrap_or("").to_string(),
                display_name: m["user_name"].as_str().unwrap_or("").to_string(),
                profile_image_url: String::new(),
            });
        }
        after = body["pagination"]["cursor"].as_str().map(|s| s.to_string());
        if after.is_none() || data.is_empty() {
            break;
        }
    }
    eprintln!("[Twitch] Helix moderators: {} entrées", liste.len());
    Ok(liste)
}

/// Liste les bannis/timeout : GET /moderation/bans?broadcaster_id=... — pagination.
pub async fn list_banned(
    broadcaster_id: &str,
    access: &str,
) -> Result<Vec<BannedEntry>, HelixError> {
    let mut liste: Vec<BannedEntry> = Vec::new();
    let mut after: Option<String> = None;
    let first = PAGE_SIZE.to_string();

    for _ in 0..MAX_PAGES {
        let mut query: Vec<(&str, &str)> = vec![
            ("broadcaster_id", broadcaster_id),
            ("first", &first),
        ];
        if let Some(cursor) = &after {
            query.push(("after", cursor));
        }
        let body = helix_get("/moderation/bans", &query, access).await?;
        let data = body["data"]
            .as_array()
            .ok_or_else(|| HelixError::Autre("bans: data absent".into()))?;
        for b in data {
            // expires_at = null → ban permanent, sinon ISO timestamp → timeout.
            let expires_at = b["expires_at"].as_str().map(|s| s.to_string());
            liste.push(BannedEntry {
                user_id: b["user_id"].as_str().unwrap_or("").to_string(),
                login: b["user_login"].as_str().unwrap_or("").to_string(),
                created_at: b["created_at"].as_str().unwrap_or("").to_string(),
                expires_at,
                reason: b["reason"].as_str().unwrap_or("").to_string(),
                display_name: b["user_name"].as_str().unwrap_or("").to_string(),
                profile_image_url: String::new(),
            });
        }
        after = body["pagination"]["cursor"].as_str().map(|s| s.to_string());
        if after.is_none() || data.is_empty() {
            break;
        }
    }
    eprintln!("[Twitch] Helix bans: {} entrées", liste.len());
    Ok(liste)
}

/// Résout un login en user_id : GET /users?login=<login>.
/// Retourne None si l'utilisateur n'existe pas.
pub async fn resolve_user_id(
    login: &str,
    access: &str,
) -> Result<Option<ResolvedUser>, HelixError> {
    let body = helix_get("/users", &[("login", login)], access).await?;
    let data = body["data"]
        .as_array()
        .and_then(|arr| arr.first());
    match data {
        Some(u) => Ok(Some(ResolvedUser {
            user_id: u["id"].as_str().unwrap_or("").to_string(),
            login: u["login"].as_str().unwrap_or("").to_string(),
            display_name: u["display_name"].as_str().unwrap_or("").to_string(),
        })),
        None => Ok(None),
    }
}

/// Bannir ou timeout un utilisateur : POST /moderation/bans.
/// `duration` = None → ban permanent, Some(n) → timeout de n secondes.
/// `moderator_id` = broadcaster_id (le streamer est mod de sa propre chaîne).
pub async fn ban_user(
    broadcaster_id: &str,
    access: &str,
    user_id: &str,
    reason: &str,
    duration: Option<u32>,
) -> Result<(), HelixError> {
    let mut data = serde_json::json!({
        "user_id": user_id,
        "reason": reason,
    });
    if let Some(d) = duration {
        data["duration"] = serde_json::json!(d);
    }
    let body = serde_json::json!({
        "data": data
    });
    let query = [("broadcaster_id", broadcaster_id), ("moderator_id", broadcaster_id)];
    helix_post("/moderation/bans", &query, &body, access).await?;
    eprintln!(
        "[Twitch] Helix ban: user_id={} duration={:?}",
        user_id, duration
    );
    Ok(())
}

/// Débannir un utilisateur : DELETE /moderation/bans?user_id=...
pub async fn unban_user(broadcaster_id: &str, access: &str, user_id: &str) -> Result<(), HelixError> {
    let query = [
        ("broadcaster_id", broadcaster_id),
        ("moderator_id", broadcaster_id),
        ("user_id", user_id),
    ];
    helix_delete("/moderation/bans", &query, access).await?;
    eprintln!("[Twitch] Helix unban: user_id={}", user_id);
    Ok(())
}

/// Ajouter un VIP : POST /channels/vips.
pub async fn add_vip(broadcaster_id: &str, access: &str, user_id: &str) -> Result<(), HelixError> {
    let body = serde_json::json!({
        "user_id": user_id
    });
    let query = [("broadcaster_id", broadcaster_id)];
    helix_post("/channels/vips", &query, &body, access).await?;
    eprintln!("[Twitch] Helix add_vip: user_id={}", user_id);
    Ok(())
}

/// Retirer un VIP : DELETE /channels/vips?user_id=...
pub async fn remove_vip(broadcaster_id: &str, access: &str, user_id: &str) -> Result<(), HelixError> {
    let query = [("broadcaster_id", broadcaster_id), ("user_id", user_id)];
    helix_delete("/channels/vips", &query, access).await?;
    eprintln!("[Twitch] Helix remove_vip: user_id={}", user_id);
    Ok(())
}

/// Ajouter un modérateur : POST /moderation/moderators.
pub async fn add_moderator(
    broadcaster_id: &str,
    access: &str,
    user_id: &str,
) -> Result<(), HelixError> {
    let body = serde_json::json!({
        "user_id": user_id
    });
    let query = [("broadcaster_id", broadcaster_id)];
    helix_post("/moderation/moderators", &query, &body, access).await?;
    eprintln!("[Twitch] Helix add_moderator: user_id={}", user_id);
    Ok(())
}

/// Retirer un modérateur : DELETE /moderation/moderators?user_id=...
pub async fn remove_moderator(
    broadcaster_id: &str,
    access: &str,
    user_id: &str,
) -> Result<(), HelixError> {
    let query = [("broadcaster_id", broadcaster_id), ("user_id", user_id)];
    helix_delete("/moderation/moderators", &query, access).await?;
    eprintln!("[Twitch] Helix remove_moderator: user_id={}", user_id);
    Ok(())
}

/// Supprimer un message de chat : DELETE /moderation/chat_messages?message_id=...
pub async fn delete_chat_message(
    broadcaster_id: &str,
    access: &str,
    message_id: &str,
) -> Result<(), HelixError> {
    let query = [
        ("broadcaster_id", broadcaster_id),
        ("moderator_id", broadcaster_id),
        ("message_id", message_id),
    ];
    helix_delete("/moderation/chat_messages", &query, access).await?;
    eprintln!("[Twitch] Helix delete_message: id={}", message_id);
    Ok(())
}
