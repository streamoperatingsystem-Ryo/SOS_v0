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
    let mut resp = client
        .get(format!("{}{}", HELIX_BASE, path))
        .header("Authorization", format!("Bearer {}", access))
        .header("Client-Id", CLIENT_ID)
        .query(query)
        .send()
        .await
        .map_err(|e| HelixError::Autre(format!("Helix GET: {}", e)))?;

    // 401 → refresh + 1 retry. On lit le refresh token depuis le keyring.
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        let tokens = twitch_auth::lire_tokens()
            .map_err(|e| HelixError::Autre(format!("Coffre: {}", e)))?
            .ok_or(HelixError::Deconnecte)?;
        let new_tokens = twitch_auth::refresh_token(&tokens.refresh)
            .await
            .map_err(|_| {
                let _ = twitch_auth::effacer_tokens();
                HelixError::Deconnecte
            })?;
        twitch_auth::sauver_tokens(&new_tokens)
            .map_err(|e| HelixError::Autre(format!("Coffre save: {}", e)))?;

        // Retry avec le nouveau token (repasse par les gardes 403/404 après).
        resp = client
            .get(format!("{}{}", HELIX_BASE, path))
            .header("Authorization", format!("Bearer {}", new_tokens.access))
            .header("Client-Id", CLIENT_ID)
            .query(query)
            .send()
            .await
            .map_err(|e| HelixError::Autre(format!("Helix GET retry: {}", e)))?;
    }

    // 403 → NeedReauth (scopes manquants). JAMAIS de Device Code auto.
    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(HelixError::NeedReauth);
    }

    // 404 sur GET /schedule = la chaîne n'a JAMAIS créé de planning (un
    // planning vide est un 404, pas un array vide — comportement Twitch
    // documenté) : Null → schedule_get lit segments[]/vacation absents =
    // planning vide, l'onglet affiche une grille vide au lieu d'une erreur.
    if resp.status() == reqwest::StatusCode::NOT_FOUND && path == "/schedule" {
        return Ok(serde_json::Value::Null);
    }

    parse_resp(resp).await
}

/// Parse la réponse HTTP en JSON (ou erreur HelixError).
async fn parse_resp(resp: reqwest::Response) -> Result<serde_json::Value, HelixError> {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(HelixError::Autre(format!("Helix HTTP {}: {}", status, body)));
    }
    // Corps vide (204 No Content : PATCH /channels, annonces, DELETE raids…) :
    // resp.json() échouerait sur ce succès → Null.
    if body.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&body)
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

/// Helper central : PATCH Helix avec auth (délègue à helix_write).
async fn helix_patch(
    path: &str,
    query: &[(&str, &str)],
    body: &serde_json::Value,
    access: &str,
) -> Result<serde_json::Value, HelixError> {
    helix_write("PATCH", path, query, Some(body), access).await
}

/// Helper central : PUT Helix avec auth (délègue à helix_write).
async fn helix_put(
    path: &str,
    query: &[(&str, &str)],
    body: &serde_json::Value,
    access: &str,
) -> Result<serde_json::Value, HelixError> {
    helix_write("PUT", path, query, Some(body), access).await
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
        let tokens = twitch_auth::lire_tokens()
            .map_err(|e| HelixError::Autre(format!("Coffre: {}", e)))?
            .ok_or(HelixError::Deconnecte)?;
        let new_tokens = twitch_auth::refresh_token(&tokens.refresh)
            .await
            .map_err(|_| {
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

    // 403 → NeedReauth (scopes manquants) — SAUF le 403 « chaîne non
    // affiliée » de POST /schedule/segment, dont le corps dit explicitement
    // "non-recurring" : remonté en Autre pour que l'appelant distingue un
    // scope manquant (reconnexion) d'un droit chaîne absent (repli récurrent).
    if status == reqwest::StatusCode::FORBIDDEN {
        let b = resp.text().await.unwrap_or_default();
        if b.contains("non-recurring") {
            return Err(HelixError::Autre(format!("Helix HTTP 403: {}", b)));
        }
        return Err(HelixError::NeedReauth);
    }

    parse_resp(resp).await
}

/// Crée l'abonnement EventSub WebSocket `channel.follow` v2 pour une session
/// (appelé par twitch_eventsub.rs après le session_welcome). Scope requis :
/// moderator:read:followers (déjà dans les scopes d'auth). Le broadcaster est
/// modérateur de son propre canal → broadcaster_user_id == moderator_user_id.
///
/// Idempotent : 409 Conflict (abonnement identique déjà actif — cas
/// session_reconnect où Twitch migre les abonnements vers la nouvelle
/// socket) = succès. Le format "Helix HTTP 409" vient de parse_resp (ce
/// fichier) — matcher la chaîne est sûr.
pub(crate) async fn eventsub_abonner_follows(
    session_id: &str,
    user_id: &str,
    access: &str,
) -> Result<(), HelixError> {
    let body = serde_json::json!({
        "type": "channel.follow",
        "version": "2",
        "condition": {
            "broadcaster_user_id": user_id,
            "moderator_user_id": user_id,
        },
        "transport": {
            "method": "websocket",
            "session_id": session_id,
        },
    });
    match helix_post("/eventsub/subscriptions", &[], &body, access).await {
        Ok(_) => Ok(()),
        Err(HelixError::Autre(m)) if m.contains("Helix HTTP 409") => Ok(()),
        Err(e) => Err(e),
    }
}

/// Abonnements EventSub `automod.message.hold` + `automod.message.update` v1
/// (file AutoMod — messages retenus). Même session_id que la socket follows.
/// Scope : moderator:manage:automod. 409 = déjà actif = succès (même modèle
/// que follows). Appelé en NON-FATAL par twitch_eventsub.rs : un échec ne
/// casse pas l'abonnement follows ni la socket.
pub(crate) async fn eventsub_abonner_automod(
    session_id: &str,
    user_id: &str,
    access: &str,
) -> Result<(), HelixError> {
    for sub_type in ["automod.message.hold", "automod.message.update"] {
        let body = serde_json::json!({
            "type": sub_type,
            "version": "1",
            "condition": {
                "broadcaster_user_id": user_id,
                "moderator_user_id": user_id,
            },
            "transport": {
                "method": "websocket",
                "session_id": session_id,
            },
        });
        match helix_post("/eventsub/subscriptions", &[], &body, access).await {
            Ok(_) => {}
            Err(HelixError::Autre(m)) if m.contains("Helix HTTP 409") => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Abonnements EventSub `channel.goal.begin` + `channel.goal.progress` +
/// `channel.goal.end` v1 — Creator Goals déclarés par le streamer dans son
/// Creator Dashboard (push temps réel de la progression, régressions
/// incluses : un unfollow fait redescendre le compteur Twitch). Même
/// session_id que la socket follows. Scope : channel:read:goals.
/// 409 = déjà actif = succès. Appelé en NON-FATAL par twitch_eventsub.rs
/// (même modèle qu'automod : un échec ne casse ni la socket ni les follows).
pub(crate) async fn eventsub_abonner_goals(
    session_id: &str,
    user_id: &str,
    access: &str,
) -> Result<(), HelixError> {
    for sub_type in ["channel.goal.begin", "channel.goal.progress", "channel.goal.end"] {
        let body = serde_json::json!({
            "type": sub_type,
            "version": "1",
            "condition": {
                "broadcaster_user_id": user_id,
            },
            "transport": {
                "method": "websocket",
                "session_id": session_id,
            },
        });
        match helix_post("/eventsub/subscriptions", &[], &body, access).await {
            Ok(_) => {}
            Err(HelixError::Autre(m)) if m.contains("Helix HTTP 409") => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
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
    /// Bannière/entête de chaîne (offline_image_url). Vide si non définie.
    pub offline_image_url: String,
    pub broadcaster_type: String,
    pub description: String,
}

/// Creator Goal Twitch — objectif DÉCLARÉ par le streamer dans son Creator
/// Dashboard (Twitch tient le compteur sur les vrais events). Vocabulaire
/// actuel/cible/atteint : les mêmes noms servent pour la commande Tauri
/// `twitch_goals_lister` ET la frame WS `goal-twitch` (zéro traduction).
/// PartialEq : dedup des frames WS dans viewers.rs (comparaison au tuple).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreatorGoal {
    /// id Helix du goal (liaison widget → `goalId`).
    pub id: String,
    /// Type Helix : "follower" | "subscription" | "subscription_count" |
    /// "new_subscription" | "new_subscription_count" | "new_bit" | "new_cheerer".
    /// Pas le même espace de noms que `goalMetrique` du widget (mapping UI).
    pub goal_type: String,
    /// Texte libre saisi par le streamer dans le Creator Dashboard
    /// (peut être vide → fallback label de type côté UI).
    pub description: String,
    pub actuel: u64,
    pub cible: u64,
    /// Via GET /goals : dérivé (actuel >= cible). Via EventSub end : le vrai
    /// is_achieved de Twitch.
    pub atteint: bool,
    /// true uniquement quand l'info vient de `channel.goal.end` (GET /goals
    /// ne retourne que des objectifs actifs → toujours false au fetch).
    #[serde(default)]
    pub termine: bool,
}

// ===== Fonctions publiques =====

/// Followers : GET /channels/followers?broadcaster_id=... — pagination curseur,
/// max MAX_PAGES pages. Retourne total + liste.
pub async fn followers(broadcaster_id: &str, access: &str) -> Result<FollowersResp, HelixError> {
    let mut liste: Vec<FollowerEntry> = Vec::new();
    let mut total: u64 = 0;
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
    }

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

    for _ in 0..MAX_PAGES {
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
    }

    Ok(SubsResp {
        total,
        points,
        liste,
    })
}

/// Total followers SEUL (1 appel, first=1 — aucune pagination, pas de liste).
/// Pour le polling objectifs/communauté (viewers.rs, cadence 60s) : la liste
/// complète paginée reste pour la modale Modération (fonction `followers`).
pub async fn followers_total(broadcaster_id: &str, access: &str) -> Result<u64, HelixError> {
    let body = helix_get(
        "/channels/followers",
        &[("broadcaster_id", broadcaster_id), ("first", "1")],
        access,
    )
    .await?;
    Ok(body["total"].as_u64().unwrap_or(0))
}

/// (total subs, sub points) SEULS — 1 appel, first=1, même raison que
/// `followers_total`. Les points : Tier1=1, Tier2=2, Tier3=6 (doc Twitch).
pub async fn subs_totaux(broadcaster_id: &str, access: &str) -> Result<(u64, u64), HelixError> {
    let body = helix_get(
        "/subscriptions",
        &[("broadcaster_id", broadcaster_id), ("first", "1")],
        access,
    )
    .await?;
    Ok((
        body["total"].as_u64().unwrap_or(0),
        body["points"].as_u64().unwrap_or(0),
    ))
}

/// Objectifs Twitch déclarés (Creator Goals) : GET /goals?broadcaster_id=...
/// Retourne UNIQUEMENT les objectifs ACTIFS (un par type max, déclarés dans
/// le Creator Dashboard — il n'existe aucune API d'écriture). Les compteurs
/// sont tenus par Twitch sur les vrais events. Scope : channel:read:goals
/// (403 → NeedReauth → reconnexion device flow au choix de l'utilisateur).
/// Entrée malformée ignorée (filter_map) : une ligne cassée ne vide pas la liste.
pub async fn creator_goals(
    broadcaster_id: &str,
    access: &str,
) -> Result<Vec<CreatorGoal>, HelixError> {
    let body = helix_get("/goals", &[("broadcaster_id", broadcaster_id)], access).await?;
    let data = body["data"]
        .as_array()
        .ok_or_else(|| HelixError::Autre("goals: data absent".into()))?;
    Ok(data
        .iter()
        .filter_map(|g| {
            let actuel = g["current_amount"].as_u64()?;
            let cible = g["target_amount"].as_u64()?;
            Some(CreatorGoal {
                id: g["id"].as_str()?.to_string(),
                goal_type: g["type"].as_str()?.to_string(),
                description: g["description"].as_str().unwrap_or_default().to_string(),
                actuel,
                cible,
                atteint: actuel >= cible,
                termine: false,
            })
        })
        .collect())
}

/// Infos du stream en cours — extraites de GET /streams (un seul appel :
/// viewers + titre + jeu + started_at + tags, tout arrive dans la même
/// réponse que le viewer_count).
#[derive(Debug, Clone)]
pub struct StreamInfo {
    /// viewer_count Twitch (approximatif par nature).
    pub viewers: u32,
    /// Titre du stream.
    pub titre: String,
    /// game_id Helix de la catégorie courante.
    pub jeu_id: String,
    /// Nom de la catégorie (game_name).
    pub jeu: String,
    /// started_at RFC3339 — début du live côté Twitch.
    pub debut: String,
    /// Tags du stream.
    pub tags: Vec<String>,
}

/// Viewers + contexte live : GET /streams?user_id=... — si data vide → None
/// (hors-ligne), sinon Some(StreamInfo).
pub async fn stream_viewers(
    broadcaster_id: &str,
    access: &str,
) -> Result<Option<StreamInfo>, HelixError> {
    let body = helix_get("/streams", &[("user_id", broadcaster_id)], access).await?;

    let data = body["data"]
        .as_array()
        .ok_or_else(|| HelixError::Autre("streams: data absent".into()))?;

    if data.is_empty() {
        return Ok(None);
    }

    let s = &data[0];
    let viewers = s["viewer_count"]
        .as_u64()
        .map(|v| v as u32)
        .ok_or_else(|| HelixError::Autre("streams: viewer_count absent".into()))?;

    Ok(Some(StreamInfo {
        viewers,
        titre: s["title"].as_str().unwrap_or_default().to_string(),
        jeu_id: s["game_id"].as_str().unwrap_or_default().to_string(),
        jeu: s["game_name"].as_str().unwrap_or_default().to_string(),
        debut: s["started_at"].as_str().unwrap_or_default().to_string(),
        tags: s["tags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }))
}

/// Broadcaster : GET /users?id=... — display_name, profile_image_url,
/// offline_image_url, broadcaster_type, description.
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
        offline_image_url: data["offline_image_url"]
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
    Ok(())
}

/// Ajouter un VIP : POST /channels/vips.
pub async fn add_vip(broadcaster_id: &str, access: &str, user_id: &str) -> Result<(), HelixError> {
    let body = serde_json::json!({
        "user_id": user_id
    });
    let query = [("broadcaster_id", broadcaster_id)];
    helix_post("/channels/vips", &query, &body, access).await?;
    Ok(())
}

/// Retirer un VIP : DELETE /channels/vips?user_id=...
pub async fn remove_vip(broadcaster_id: &str, access: &str, user_id: &str) -> Result<(), HelixError> {
    let query = [("broadcaster_id", broadcaster_id), ("user_id", user_id)];
    helix_delete("/channels/vips", &query, access).await?;
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
    Ok(())
}

// ===== Contrôles en direct (dashboard — Helix uniquement, jamais IRC) =====

#[derive(Debug, Clone, Serialize)]
pub struct ChannelInfo {
    pub title: String,
    pub game_id: String,
    pub game_name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryEntry {
    pub id: String,
    pub name: String,
    pub box_art_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelSearchEntry {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub is_live: bool,
    pub thumbnail_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatSettings {
    pub emote_mode: bool,
    pub follower_mode: bool,
    pub follower_mode_duration: Option<u32>,
    pub slow_mode: bool,
    pub slow_mode_wait_time: Option<u32>,
    pub subscriber_mode: bool,
}

/// Corps partiel pour PATCH /chat/settings (Option = champ non touché).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ChatSettingsPatch {
    pub emote_mode: Option<bool>,
    pub follower_mode: Option<bool>,
    pub follower_mode_duration: Option<u32>,
    pub slow_mode: Option<bool>,
    pub slow_mode_wait_time: Option<u32>,
    pub subscriber_mode: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnbanRequestEntry {
    pub id: String,
    pub user_id: String,
    pub login: String,
    pub display_name: String,
    pub text: String,
    pub created_at: String,
}

/// Infos de la chaîne : GET /channels?broadcaster_id=... — titre, jeu, tags.
/// Scope user:read:broadcast (déjà présent).
pub async fn channel_info(broadcaster_id: &str, access: &str) -> Result<ChannelInfo, HelixError> {
    let body = helix_get("/channels", &[("broadcaster_id", broadcaster_id)], access).await?;
    let d = body["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| HelixError::Autre("channels: data[0] absent".into()))?;
    Ok(ChannelInfo {
        title: d["title"].as_str().unwrap_or("").to_string(),
        game_id: d["game_id"].as_str().unwrap_or("").to_string(),
        game_name: d["game_name"].as_str().unwrap_or("").to_string(),
        tags: d["tags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Modifie titre/catégorie/tags : PATCH /channels?broadcaster_id=...
/// Scope channel:manage:broadcast. Option = champ non touché (PATCH partiel).
/// `tags` n'est jamais envoyé vide : Helix refuse/ignore tags:[] — le champ
/// est omis si l'appelant n'y a pas touché. Validation AVANT l'appel :
/// titre ≤140 caractères, tags ≤10 et ≤25 caractères sans espace.
pub async fn maj_channel(
    broadcaster_id: &str,
    access: &str,
    title: Option<&str>,
    game_id: Option<&str>,
    tags: Option<Vec<String>>,
) -> Result<(), HelixError> {
    if let Some(t) = title {
        if t.chars().count() > 140 {
            return Err(HelixError::Autre("Titre > 140 caractères".into()));
        }
    }
    if let Some(tags) = &tags {
        if tags.len() > 10 {
            return Err(HelixError::Autre("Plus de 10 tags".into()));
        }
        for tag in tags {
            if tag.chars().count() > 25 || tag.chars().any(|c| c.is_whitespace()) {
                return Err(HelixError::Autre(
                    "Tag invalide (≤25 caractères, sans espace)".into(),
                ));
            }
        }
    }
    let mut body = serde_json::json!({});
    if let Some(t) = title {
        body["title"] = serde_json::json!(t);
    }
    if let Some(g) = game_id {
        body["game_id"] = serde_json::json!(g);
    }
    if let Some(t) = tags {
        if !t.is_empty() {
            body["tags"] = serde_json::json!(t);
        }
    }
    if body.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return Ok(());
    }
    helix_patch("/channels", &[("broadcaster_id", broadcaster_id)], &body, access).await?;
    Ok(())
}

/// Recherche de catégorie : GET /search/categories?query=&first=10.
/// Aucun scope supplémentaire (token user existant suffit).
pub async fn search_categories(
    query: &str,
    access: &str,
) -> Result<Vec<CategoryEntry>, HelixError> {
    let body = helix_get(
        "/search/categories",
        &[("query", query), ("first", "10")],
        access,
    )
    .await?;
    let data = body["data"].as_array().cloned().unwrap_or_default();
    Ok(data
        .iter()
        .map(|c| CategoryEntry {
            id: c["id"].as_str().unwrap_or("").to_string(),
            name: c["name"].as_str().unwrap_or("").to_string(),
            box_art_url: c["box_art_url"].as_str().unwrap_or("").to_string(),
        })
        .collect())
}

/// Infos d'un jeu par id : GET /games?id=<id>. Sert à récupérer
/// `box_art_url` quand la section « En direct » s'ouvre avec un game_id
/// déjà connu (la recherche /search/categories reste la source au choix).
/// Aucun scope supplémentaire. `None` si l'id n'existe plus côté Twitch.
pub async fn game_info(
    game_id: &str,
    access: &str,
) -> Result<Option<CategoryEntry>, HelixError> {
    let body = helix_get("/games", &[("id", game_id)], access).await?;
    let data = body["data"].as_array().cloned().unwrap_or_default();
    Ok(data.first().map(|c| CategoryEntry {
        id: c["id"].as_str().unwrap_or("").to_string(),
        name: c["name"].as_str().unwrap_or("").to_string(),
        box_art_url: c["box_art_url"].as_str().unwrap_or("").to_string(),
    }))
}

/// Recherche de chaîne (suggestions raid) : GET /search/channels?query=&first=5.
pub async fn search_channels(
    query: &str,
    access: &str,
) -> Result<Vec<ChannelSearchEntry>, HelixError> {
    let body = helix_get(
        "/search/channels",
        &[("query", query), ("first", "5")],
        access,
    )
    .await?;
    let data = body["data"].as_array().cloned().unwrap_or_default();
    Ok(data
        .iter()
        .map(|c| ChannelSearchEntry {
            id: c["id"].as_str().unwrap_or("").to_string(),
            login: c["broadcaster_login"].as_str().unwrap_or("").to_string(),
            display_name: c["display_name"].as_str().unwrap_or("").to_string(),
            is_live: c["is_live"].as_bool().unwrap_or(false),
            thumbnail_url: c["thumbnail_url"].as_str().unwrap_or("").to_string(),
        })
        .collect())
}

/// Marqueur de stream : POST /streams/markers {user_id, description?}.
/// Scope channel:manage:broadcast. Hors-ligne → erreur Helix remontée
/// (message côté UI, pas de retry).
pub async fn create_marker(
    broadcaster_id: &str,
    access: &str,
    description: Option<&str>,
) -> Result<(), HelixError> {
    let mut body = serde_json::json!({"user_id": broadcaster_id});
    if let Some(d) = description {
        if !d.is_empty() {
            body["description"] = serde_json::json!(d);
        }
    }
    helix_post("/streams/markers", &[], &body, access).await?;
    Ok(())
}

/// Lancer une pub : POST /channels/commercial {broadcaster_id, length}.
/// Scope channel:edit:commercial. Durées Helix uniquement — refus avant
/// l'appel si hors ensemble. Hors-ligne → erreur Helix remontée.
pub async fn start_commercial(
    broadcaster_id: &str,
    access: &str,
    length: u32,
) -> Result<(), HelixError> {
    if ![30, 60, 90, 120, 150, 180].contains(&length) {
        return Err(HelixError::Autre(format!("Durée pub invalide : {}", length)));
    }
    let body = serde_json::json!({"broadcaster_id": broadcaster_id, "length": length});
    helix_post("/channels/commercial", &[], &body, access).await?;
    Ok(())
}

/// Lancer un raid : POST /raids?from_broadcaster_id=&to_broadcaster_id=.
/// Scope channel:manage:raids. Pas de corps (paramètres en query).
/// Pas en live → erreur Helix remontée (non fatale côté UI).
pub async fn start_raid(from_id: &str, to_id: &str, access: &str) -> Result<(), HelixError> {
    let query = [
        ("from_broadcaster_id", from_id),
        ("to_broadcaster_id", to_id),
    ];
    helix_write("POST", "/raids", &query, None, access).await?;
    Ok(())
}

/// Annuler un raid en cours : DELETE /raids?broadcaster_id=...
pub async fn cancel_raid(broadcaster_id: &str, access: &str) -> Result<(), HelixError> {
    helix_delete("/raids", &[("broadcaster_id", broadcaster_id)], access).await?;
    Ok(())
}

/// Modes de chat : GET /chat/settings?broadcaster_id&moderator_id.
/// Scope moderator:read:chat_settings. moderator_id = broadcaster_id.
pub async fn chat_settings(broadcaster_id: &str, access: &str) -> Result<ChatSettings, HelixError> {
    let query = [
        ("broadcaster_id", broadcaster_id),
        ("moderator_id", broadcaster_id),
    ];
    let body = helix_get("/chat/settings", &query, access).await?;
    let d = body["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| HelixError::Autre("chat/settings: data[0] absent".into()))?;
    Ok(ChatSettings {
        emote_mode: d["emote_mode"].as_bool().unwrap_or(false),
        follower_mode: d["follower_mode"].as_bool().unwrap_or(false),
        follower_mode_duration: d["follower_mode_duration"].as_u64().map(|v| v as u32),
        slow_mode: d["slow_mode"].as_bool().unwrap_or(false),
        slow_mode_wait_time: d["slow_mode_wait_time"].as_u64().map(|v| v as u32),
        subscriber_mode: d["subscriber_mode"].as_bool().unwrap_or(false),
    })
}

/// Modifier les modes de chat : PATCH /chat/settings (corps partiel — seuls
/// les champs Some sont envoyés). Scope moderator:manage:chat_settings.
/// Bornes Helix : slow_mode_wait_time 3–120, follower_mode_duration 0–129600.
pub async fn patch_chat_settings(
    broadcaster_id: &str,
    access: &str,
    patch: &ChatSettingsPatch,
) -> Result<(), HelixError> {
    if let Some(t) = patch.slow_mode_wait_time {
        if !(3..=120).contains(&t) {
            return Err(HelixError::Autre(
                "slow_mode_wait_time hors bornes (3-120)".into(),
            ));
        }
    }
    if let Some(d) = patch.follower_mode_duration {
        if d > 129600 {
            return Err(HelixError::Autre(
                "follower_mode_duration hors bornes (0-129600)".into(),
            ));
        }
    }
    let mut body = serde_json::json!({});
    if let Some(v) = patch.emote_mode {
        body["emote_mode"] = serde_json::json!(v);
    }
    if let Some(v) = patch.follower_mode {
        body["follower_mode"] = serde_json::json!(v);
    }
    if let Some(v) = patch.follower_mode_duration {
        body["follower_mode_duration"] = serde_json::json!(v);
    }
    if let Some(v) = patch.slow_mode {
        body["slow_mode"] = serde_json::json!(v);
    }
    if let Some(v) = patch.slow_mode_wait_time {
        body["slow_mode_wait_time"] = serde_json::json!(v);
    }
    if let Some(v) = patch.subscriber_mode {
        body["subscriber_mode"] = serde_json::json!(v);
    }
    if body.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return Ok(());
    }
    let query = [
        ("broadcaster_id", broadcaster_id),
        ("moderator_id", broadcaster_id),
    ];
    helix_patch("/chat/settings", &query, &body, access).await?;
    Ok(())
}

/// Annonce colorée : POST /chat/announcements?broadcaster_id&moderator_id
/// {message, color}. Scope moderator:manage:announcements.
/// Couleurs Helix uniquement : blue, green, orange, purple, primary.
pub async fn send_announcement(
    broadcaster_id: &str,
    access: &str,
    message: &str,
    color: &str,
) -> Result<(), HelixError> {
    const COULEURS: [&str; 5] = ["blue", "green", "orange", "purple", "primary"];
    if !COULEURS.contains(&color) {
        return Err(HelixError::Autre(format!(
            "Couleur d'annonce invalide : {}",
            color
        )));
    }
    let body = serde_json::json!({"message": message, "color": color});
    let query = [
        ("broadcaster_id", broadcaster_id),
        ("moderator_id", broadcaster_id),
    ];
    helix_post("/chat/announcements", &query, &body, access).await?;
    Ok(())
}

/// Shield Mode actif ? GET /moderation/shield_mode?broadcaster_id&moderator_id.
/// Scope moderator:read:shield_mode.
pub async fn shield_status(broadcaster_id: &str, access: &str) -> Result<bool, HelixError> {
    let query = [
        ("broadcaster_id", broadcaster_id),
        ("moderator_id", broadcaster_id),
    ];
    let body = helix_get("/moderation/shield_mode", &query, access).await?;
    Ok(body["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|d| d["is_active"].as_bool())
        .unwrap_or(false))
}

/// Activer/désactiver le Shield Mode : PUT /moderation/shield_mode
/// {is_active}. Scope moderator:manage:shield_mode. La confirmation avant
/// activation est côté UI (coupe les modes de chat et vide le chat).
pub async fn set_shield(
    broadcaster_id: &str,
    access: &str,
    actif: bool,
) -> Result<(), HelixError> {
    let body = serde_json::json!({"is_active": actif});
    let query = [
        ("broadcaster_id", broadcaster_id),
        ("moderator_id", broadcaster_id),
    ];
    helix_put("/moderation/shield_mode", &query, &body, access).await?;
    Ok(())
}

/// Demandes d'unban en attente : GET /moderation/unban_requests
/// ?broadcaster_id&moderator_id&status=pending. Scope
/// moderator:read:unban_requests. Pagination bornée MAX_PAGES.
pub async fn unban_requests(
    broadcaster_id: &str,
    access: &str,
) -> Result<Vec<UnbanRequestEntry>, HelixError> {
    let mut liste: Vec<UnbanRequestEntry> = Vec::new();
    let mut after: Option<String> = None;
    let first = PAGE_SIZE.to_string();

    for _ in 0..MAX_PAGES {
        let mut query: Vec<(&str, &str)> = vec![
            ("broadcaster_id", broadcaster_id),
            ("moderator_id", broadcaster_id),
            ("status", "pending"),
            ("first", &first),
        ];
        if let Some(cursor) = &after {
            query.push(("after", cursor));
        }
        let body = helix_get("/moderation/unban_requests", &query, access).await?;
        let data = body["data"].as_array().cloned().unwrap_or_default();
        for r in &data {
            liste.push(UnbanRequestEntry {
                id: r["id"].as_str().unwrap_or("").to_string(),
                user_id: r["user_id"].as_str().unwrap_or("").to_string(),
                login: r["user_login"].as_str().unwrap_or("").to_string(),
                display_name: r["user_name"].as_str().unwrap_or("").to_string(),
                text: r["text"].as_str().unwrap_or("").to_string(),
                created_at: r["created_at"].as_str().unwrap_or("").to_string(),
            });
        }
        after = body["pagination"]["cursor"].as_str().map(|s| s.to_string());
        if after.is_none() || data.is_empty() {
            break;
        }
    }
    Ok(liste)
}

/// Traiter une demande d'unban : PATCH /moderation/unban_requests
/// ?broadcaster_id&moderator_id&unban_request_id {status: approved|denied}.
/// Scope moderator:manage:unban_requests. approved = ban levé par Twitch.
pub async fn resolve_unban_request(
    broadcaster_id: &str,
    access: &str,
    unban_request_id: &str,
    approuver: bool,
) -> Result<(), HelixError> {
    let body = serde_json::json!({
        "status": if approuver { "approved" } else { "denied" },
    });
    let query = [
        ("broadcaster_id", broadcaster_id),
        ("moderator_id", broadcaster_id),
        ("unban_request_id", unban_request_id),
    ];
    helix_patch("/moderation/unban_requests", &query, &body, access).await?;
    Ok(())
}

/// AutoMod : autoriser/refuser un message retenu — POST
/// /moderation/automod/message {user_id, msg_id, action: ALLOW|DENY}.
/// Scope moderator:manage:automod. user_id = broadcaster (mod de sa chaîne).
/// La file arrive via EventSub automod.message.hold (twitch_eventsub.rs) —
/// pas d'endpoint GET, pas de poll.
pub async fn automod_resolve(
    broadcaster_id: &str,
    access: &str,
    msg_id: &str,
    autoriser: bool,
) -> Result<(), HelixError> {
    let body = serde_json::json!({
        "user_id": broadcaster_id,
        "msg_id": msg_id,
        "action": if autoriser { "ALLOW" } else { "DENY" },
    });
    helix_post("/moderation/automod/message", &[], &body, access).await?;
    Ok(())
}

// ===== Planning Twitch (schedule) — onglet Planning de la modale Interactions =====

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleSegment {
    /// id Helix — pour un récurrent, chaque occurrence hebdo a un id propre
    /// (encodé segmentID + semaine ISO) : PATCH/DELETE d'une occurrence
    /// récurrente impacte la série, pas seulement ce créneau.
    pub id: String,
    pub start_time: String,
    pub end_time: String,
    pub title: String,
    /// Catégorie liée (vide = segment sans jeu déclaré).
    #[serde(default)]
    pub category_id: String,
    #[serde(default)]
    pub category_name: String,
    pub is_recurring: bool,
    /// Some(date ISO) = annulé (récurrent : prochaine occurrence annulée).
    pub canceled_until: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleResp {
    pub segments: Vec<ScheduleSegment>,
    /// Vacances déclarées (dates ISO). Vides = pas de vacances.
    pub vacation_start: String,
    pub vacation_end: String,
}

/// Paramètres de création d'un segment (POST /schedule/segment).
/// Clés snake_case — le struct se désérialise tel quel depuis le frontend
/// (même convention que ChatSettingsPatch).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ScheduleSegmentNew {
    /// RFC3339.
    pub start_time: String,
    /// IANA (ex. Europe/Paris) — requis par Twitch.
    pub timezone: String,
    /// false = ponctuel (affiliés/partenaires uniquement — repli récurrent
    /// côté commande si Twitch refuse) ; true = toutes les semaines.
    pub is_recurring: bool,
    /// Minutes 30-1380 (sérialisées en CHAÎNE à l'envoi — format Twitch).
    pub duration_min: u32,
    pub category_id: Option<String>,
    pub title: Option<String>,
}

/// Corps partiel pour PATCH /schedule/segment (Option = champ non touché).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ScheduleSegmentPatch {
    /// RFC3339. Déplaçable uniquement sur les ponctuels (un récurrent ne se
    /// déplace pas : il se supprime + recrée — géré côté UI).
    pub start_time: Option<String>,
    /// Requis par Twitch quand start_time est envoyé (IANA, ex. Europe/Paris).
    pub timezone: Option<String>,
    /// Minutes 30-1380 (sérialisées en CHAÎNE — format Twitch).
    pub duration_min: Option<u32>,
    pub category_id: Option<String>,
    pub title: Option<String>,
    /// Annuler (true) / rétablir (false). Récurrent = prochaine occurrence.
    pub is_canceled: Option<bool>,
}

/// Parse un segment Helix (objet JSON) en ScheduleSegment.
fn parse_segment(s: &serde_json::Value) -> ScheduleSegment {
    ScheduleSegment {
        id: s["id"].as_str().unwrap_or("").to_string(),
        start_time: s["start_time"].as_str().unwrap_or("").to_string(),
        end_time: s["end_time"].as_str().unwrap_or("").to_string(),
        title: s["title"].as_str().unwrap_or("").to_string(),
        category_id: s["category"]["id"].as_str().unwrap_or("").to_string(),
        category_name: s["category"]["name"].as_str().unwrap_or("").to_string(),
        is_recurring: s["is_recurring"].as_bool().unwrap_or(false),
        canceled_until: s["canceled_until"].as_str().map(|v| v.to_string()),
    }
}

/// Lecture du planning : GET /schedule?broadcaster_id=&start_time=&first=25.
/// ⚠️ `data` est un OBJET {segments[], vacation} — pas un array comme les
/// autres endpoints ; le curseur de pagination est à la racine de la réponse.
/// Aucun scope requis (token user suffit). `start_time` RFC3339 : Twitch ne
/// renvoie que les segments à partir de cette date ; un récurrent se déroule
/// en occurrences hebdo (id propre à chacune). Pagination bornée MAX_PAGES.
pub async fn schedule_get(
    broadcaster_id: &str,
    start_time: Option<&str>,
    access: &str,
) -> Result<ScheduleResp, HelixError> {
    let mut segments: Vec<ScheduleSegment> = Vec::new();
    let mut vacation_start = String::new();
    let mut vacation_end = String::new();
    let mut after: Option<String> = None;

    for _ in 0..MAX_PAGES {
        let mut query: Vec<(&str, &str)> = vec![
            ("broadcaster_id", broadcaster_id),
            ("first", "25"),
        ];
        if let Some(st) = start_time {
            query.push(("start_time", st));
        }
        if let Some(cursor) = &after {
            query.push(("after", cursor));
        }

        let body = helix_get("/schedule", &query, access).await?;
        let data = &body["data"];

        if let Some(segs) = data["segments"].as_array() {
            for s in segs {
                segments.push(parse_segment(s));
            }
        }
        if !data["vacation"].is_null() {
            vacation_start = data["vacation"]["start_time"]
                .as_str()
                .unwrap_or("")
                .to_string();
            vacation_end = data["vacation"]["end_time"]
                .as_str()
                .unwrap_or("")
                .to_string();
        }

        after = body["pagination"]["cursor"].as_str().map(|s| s.to_string());
        if after.is_none() {
            break;
        }
    }

    Ok(ScheduleResp {
        segments,
        vacation_start,
        vacation_end,
    })
}

/// Créer un segment : POST /schedule/segment?broadcaster_id= {start_time,
/// timezone, is_recurring, duration, category_id?, title?}.
/// ⚠️ `duration` est une CHAÎNE JSON en minutes (bornes 30-1380), pas un
/// nombre — format documenté Twitch. `timezone` IANA requis. Non-affilié :
/// Twitch refuse les ponctuels avec un 403 "non-recurring" (distingué de
/// NeedReauth dans helix_write) — le repli récurrent est dans la commande.
/// Retourne le segment créé (data.segments[0]).
pub async fn schedule_segment_create(
    broadcaster_id: &str,
    access: &str,
    params: &ScheduleSegmentNew,
) -> Result<ScheduleSegment, HelixError> {
    if !(30..=1380).contains(&params.duration_min) {
        return Err(HelixError::Autre(format!(
            "Durée hors bornes (30-1380) : {}",
            params.duration_min
        )));
    }
    let mut body = serde_json::json!({
        "start_time": params.start_time,
        "timezone": params.timezone,
        "is_recurring": params.is_recurring,
        "duration": params.duration_min.to_string(),
    });
    if let Some(c) = &params.category_id {
        if !c.is_empty() {
            body["category_id"] = serde_json::json!(c);
        }
    }
    if let Some(t) = &params.title {
        if !t.is_empty() {
            body["title"] = serde_json::json!(t);
        }
    }
    let resp = helix_post(
        "/schedule/segment",
        &[("broadcaster_id", broadcaster_id)],
        &body,
        access,
    )
    .await?;
    let s = resp["data"]["segments"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| HelixError::Autre("schedule/segment: data.segments[0] absent".into()))?;
    Ok(parse_segment(s))
}

/// Modifier un segment : PATCH /schedule/segment?broadcaster_id&id=
/// (corps partiel — seuls les champs Some sont envoyés). ⚠️ Sur un segment
/// récurrent, title/category/duration s'appliquent à TOUTE la série et
/// start_time n'est pas modifiable — la confirmation est côté UI.
pub async fn schedule_segment_update(
    broadcaster_id: &str,
    access: &str,
    segment_id: &str,
    patch: &ScheduleSegmentPatch,
) -> Result<(), HelixError> {
    if let Some(d) = patch.duration_min {
        if !(30..=1380).contains(&d) {
            return Err(HelixError::Autre(format!(
                "Durée hors bornes (30-1380) : {}",
                d
            )));
        }
    }
    let mut body = serde_json::json!({});
    if let Some(s) = &patch.start_time {
        body["start_time"] = serde_json::json!(s);
    }
    if let Some(tz) = &patch.timezone {
        body["timezone"] = serde_json::json!(tz);
    }
    if let Some(d) = patch.duration_min {
        body["duration"] = serde_json::json!(d.to_string());
    }
    if let Some(c) = &patch.category_id {
        body["category_id"] = serde_json::json!(c);
    }
    if let Some(t) = &patch.title {
        body["title"] = serde_json::json!(t);
    }
    if let Some(b) = patch.is_canceled {
        body["is_canceled"] = serde_json::json!(b);
    }
    if body.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return Ok(());
    }
    let query = [("broadcaster_id", broadcaster_id), ("id", segment_id)];
    helix_patch("/schedule/segment", &query, &body, access).await?;
    Ok(())
}

/// Supprimer un segment : DELETE /schedule/segment?broadcaster_id&id=
/// ⚠️ Sur un récurrent → supprime TOUTE la série (la confirmation « toute
/// la série sera supprimée » est affichée côté UI).
pub async fn schedule_segment_delete(
    broadcaster_id: &str,
    access: &str,
    segment_id: &str,
) -> Result<(), HelixError> {
    let query = [("broadcaster_id", broadcaster_id), ("id", segment_id)];
    helix_delete("/schedule/segment", &query, access).await?;
    Ok(())
}

/// Vacances : PATCH /schedule/settings — tous les paramètres EN QUERY
/// (endpoint sans corps, helix_write PATCH sans body). actif=false → juste
/// is_vacation_enabled=false ; actif=true → dates RFC3339 + timezone requis.
pub async fn schedule_settings_vacances(
    broadcaster_id: &str,
    access: &str,
    actif: bool,
    debut: Option<&str>,
    fin: Option<&str>,
    timezone: &str,
) -> Result<(), HelixError> {
    if actif
        && (debut.map(|d| d.is_empty()).unwrap_or(true)
            || fin.map(|f| f.is_empty()).unwrap_or(true))
    {
        return Err(HelixError::Autre(
            "Vacances : dates début/fin requises".into(),
        ));
    }
    let actif_s = actif.to_string();
    let mut query: Vec<(&str, &str)> = vec![
        ("broadcaster_id", broadcaster_id),
        ("is_vacation_enabled", &actif_s),
    ];
    if actif {
        if let Some(d) = debut {
            query.push(("vacation_start_time", d));
        }
        if let Some(f) = fin {
            query.push(("vacation_end_time", f));
        }
        query.push(("timezone", timezone));
    }
    helix_write("PATCH", "/schedule/settings", &query, None, access).await?;
    Ok(())
}
