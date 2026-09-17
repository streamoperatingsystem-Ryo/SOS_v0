/// Authentification Twitch — Device Code Flow + refresh + coffre keyring.
///
/// Coffre : Windows Credential Manager via la crate `keyring`.
/// Service = "streamos-v0-twitch", compte = "default".
///
/// Scopes demandés : "chat:read" (lecture IRC) — pas d'envoi ce lot.
/// Endpoints : id.twitch.tv/oauth2/{device,token,validate}.
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Jeton d'annulation partagé (cancel = AtomicBool). Clone léger.
pub type Cancel = Arc<AtomicBool>;

pub fn new_cancel() -> Cancel {
    Arc::new(AtomicBool::new(false))
}

pub fn is_cancelled(c: &Cancel) -> bool {
    c.load(Ordering::SeqCst)
}

pub const CLIENT_ID: &str = "kn9vopxlpnhslxxklti8aegvn38arm";
const SCOPES: &str = "chat:read chat:edit moderator:read:followers channel:read:subscriptions user:read:broadcast moderator:manage:banned_users moderation:read channel:read:vips channel:manage:vips channel:manage:moderators moderator:manage:chat_messages";
const KEYRING_SERVICE: &str = "streamos-v0-twitch";
const KEYRING_ACCOUNT: &str = "default";

const DEVICE_URL: &str = "https://id.twitch.tv/oauth2/device";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const VALIDATE_URL: &str = "https://id.twitch.tv/oauth2/validate";
const REVOKE_URL: &str = "https://id.twitch.tv/oauth2/revoke";

/// Données retournées par /device : code à afficher + URL à ouvrir.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceFlow {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Tokens persistés dans le coffre. login/user_id viennent de /validate.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub login: String,
    pub user_id: String,
}

/// Réponse de /device (Twitch).
#[derive(Debug, Deserialize)]
struct DeviceResp {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

/// Réponse de /token (succès).
#[derive(Debug, Deserialize)]
struct TokenResp {
    access_token: String,
    refresh_token: String,
}

/// Réponse de /validate.
#[derive(Debug, Deserialize)]
pub struct ValidateResp {
    pub login: String,
    pub user_id: String,
}

/// Démarre le Device Code Flow : POST /device → DeviceFlow (à afficher).
pub async fn demarrer_device_flow() -> Result<DeviceFlow, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(DEVICE_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("scopes", SCOPES),
        ])
        .send()
        .await
        .map_err(|e| format!("Device flow: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Device flow HTTP {}: {}", status, body));
    }

    let d: DeviceResp = resp
        .json()
        .await
        .map_err(|e| format!("Device flow parse: {}", e))?;

    eprintln!("[Twitch] device OK user_code={}", d.user_code);

    Ok(DeviceFlow {
        device_code: d.device_code,
        user_code: d.user_code,
        verification_uri: d.verification_uri,
        expires_in: d.expires_in,
        interval: d.interval,
    })
}

/// Poll /token jusqu'à obtention du token, expiration, erreur ou annulation.
/// Gère `authorization_pending` (retry après interval), `slow_down` (+5s).
/// Retourne les Tokens (login/user_id via /validate post-token).
pub async fn poll_token(
    device_code: String,
    interval: u64,
    expires_in: u64,
    cancel: Cancel,
) -> Result<Tokens, String> {
    let client = reqwest::Client::new();
    let mut elapsed = 0u64;
    let mut wait = interval;

    loop {
        if is_cancelled(&cancel) {
            return Err("Annulé".into());
        }
        if elapsed >= expires_in {
            return Err("Code expiré".into());
        }

        let resp = client
            .post(TOKEN_URL)
            .form(&[
                ("client_id", CLIENT_ID),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", &device_code),
            ])
            .send()
            .await
            .map_err(|e| format!("Poll token: {}", e))?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Poll token parse: {}", e))?;

        if status.is_success() {
            let t: TokenResp = serde_json::from_value(body)
                .map_err(|e| format!("Token resp: {}", e))?;
            // Valider → login + user_id
            let v = valider_token(&t.access_token).await?;
            let tokens = Tokens {
                access: t.access_token,
                refresh: t.refresh_token,
                login: v.login,
                user_id: v.user_id,
            };
            return Ok(tokens);
        }

        // Erreur : Twitch peut renvoyer "error" OU "message" (sans "error").
        // On lit les deux pour déterminer le type d'erreur.
        let err = body["error"]
            .as_str()
            .or_else(|| body["message"].as_str())
            .unwrap_or("unknown");
        match err {
            "authorization_pending" => {
                // Pas une erreur : l'utilisateur n'a pas encore autorisé.
                // Sleep interval puis retry. JAMAIS d'eprintln ERR ici.
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
                    _ = await_cancel(&cancel) => return Err("Annulé".into()),
                }
                elapsed += wait;
            }
            "slow_down" => {
                wait += 5;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
                    _ = await_cancel(&cancel) => return Err("Annulé".into()),
                }
                elapsed += wait;
            }
            "expired_token" => return Err("Code expiré".into()),
            "access_denied" => return Err("Accès refusé par l'utilisateur".into()),
            other => {
                let msg = body["message"].as_str().unwrap_or("");
                return Err(format!("Token: {} {}", other, msg));
            }
        }
    }
}

/// Attend que le drapeau d'annulation passe à true (polling léger 200ms).
async fn await_cancel(cancel: &Cancel) {
    loop {
        if is_cancelled(cancel) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Valide un access token via /validate → login + user_id.
pub async fn valider_token(token: &str) -> Result<ValidateResp, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(VALIDATE_URL)
        .header("Authorization", format!("OAuth {}", token))
        .send()
        .await
        .map_err(|e| format!("Validate: {}", e))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Token invalide (401)".into());
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Validate HTTP {}: {}", status, body));
    }

    let v: ValidateResp = resp
        .json()
        .await
        .map_err(|e| format!("Validate parse: {}", e))?;
    eprintln!("[Twitch] token OK login={}", v.login);
    Ok(v)
}

/// Rafraîchit un access token expiré via /token (grant_type=refresh_token).
pub async fn refresh_token(refresh: &str) -> Result<Tokens, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
        ])
        .send()
        .await
        .map_err(|e| format!("Refresh: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Refresh HTTP {}: {}", status, body));
    }

    let t: TokenResp = resp
        .json()
        .await
        .map_err(|e| format!("Refresh parse: {}", e))?;

    let v = valider_token(&t.access_token).await?;
    Ok(Tokens {
        access: t.access_token,
        refresh: t.refresh_token,
        login: v.login,
        user_id: v.user_id,
    })
}

/// Sauve les tokens dans le coffre keyring (JSON sérialisé).
pub fn sauver_tokens(tokens: &Tokens) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("Keyring entry: {}", e))?;
    let json = serde_json::to_string(tokens)
        .map_err(|e| format!("Serialize tokens: {}", e))?;
    entry.set_password(&json).map_err(|e| format!("Keyring set: {}", e))
}

/// Lit les tokens depuis le coffre. None si absent.
pub fn lire_tokens() -> Result<Option<Tokens>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("Keyring entry: {}", e))?;
    match entry.get_password() {
        Ok(json) => {
            let tokens: Tokens = serde_json::from_str(&json)
                .map_err(|e| format!("Deserialize tokens: {}", e))?;
            Ok(Some(tokens))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Keyring get: {}", e)),
    }
}

/// Efface les tokens du coffre. Idempotent (déjà absent = Ok).
pub fn effacer_tokens() -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("Keyring entry: {}", e))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Keyring delete: {}", e)),
    }
}

/// Révoque un access token côté Twitch (POST /revoke). Best-effort :
/// l'appelant efface le coffre quoi qu'il arrive. Non-fatal si échec réseau.
pub async fn revoke_token(token: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(REVOKE_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("token", token),
        ])
        .send()
        .await
        .map_err(|e| format!("Revoke: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Revoke HTTP {}: {}", status, body));
    }
    eprintln!("[Twitch] token révoqué");
    Ok(())
}
