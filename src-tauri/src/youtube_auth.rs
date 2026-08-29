/// Authentification YouTube — Device Code Flow OAuth2 Google + refresh + coffre keyring.
///
/// Coffre : Windows Credential Manager via la crate `keyring`.
/// Service = "streamos-v0-youtube", compte = "default".
///
/// Scopes : youtube.readonly (chat + chaîne) + youtube.channel-memberships.creator (members).
/// Endpoints : oauth2.googleapis.com/{device_code,token,revoke}, tokeninfo.
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

pub const CLIENT_ID: &str = "147877095803-2hlle8r03pr00htul2jltarrjmquqc64.apps.googleusercontent.com";
pub const CLIENT_SECRET: &str = "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";
const SCOPES: &str = "https://www.googleapis.com/auth/youtube.readonly";
const KEYRING_SERVICE: &str = "streamos-v0-youtube";
const KEYRING_ACCOUNT: &str = "default";

const DEVICE_URL: &str = "https://oauth2.googleapis.com/device/code";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const TOKENINFO_URL: &str = "https://www.googleapis.com/oauth2/v3/tokeninfo";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";

/// Données retournées par /device_code : code à afficher + URL à ouvrir.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceFlow {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Tokens persistés dans le coffre. channel_id/login viennent de /channels après token.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    /// sub (user_id Google) — obtenu via /tokeninfo.
    pub user_id: String,
    /// channel_id YouTube — obtenu via /channels?mine=true après token.
    pub channel_id: String,
    /// display_name de la chaîne — obtenu via /channels.
    pub login: String,
}

/// Réponse de /device_code (Google).
#[derive(Debug, Deserialize)]
struct DeviceResp {
    device_code: String,
    user_code: String,
    verification_url: String,
    expires_in: u64,
    interval: u64,
}

/// Réponse de /token (succès).
#[derive(Debug, Deserialize)]
struct TokenResp {
    access_token: String,
    refresh_token: String,
}

/// Réponse de /tokeninfo.
#[derive(Debug, Deserialize)]
struct TokeninfoResp {
    sub: String,
}

/// Démarre le Device Code Flow : POST /device_code → DeviceFlow (à afficher).
pub async fn demarrer_device_flow() -> Result<DeviceFlow, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(DEVICE_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("scope", SCOPES),
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

    eprintln!("[YouTube] device OK user_code={}", d.user_code);

    Ok(DeviceFlow {
        device_code: d.device_code,
        user_code: d.user_code,
        verification_url: d.verification_url,
        expires_in: d.expires_in,
        interval: d.interval,
    })
}

/// Poll /token jusqu'à obtention du token, expiration, erreur ou annulation.
/// Gère `authorization_pending` (retry après interval), `slow_down` (+5s).
/// Retourne les Tokens (user_id via /tokeninfo). channel_id/login restent vides
/// (remplis par l'appelant via resolve_channel après).
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
                ("client_secret", CLIENT_SECRET),
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
            // Valider → user_id (sub)
            let user_id = match valider_token(&t.access_token).await {
                Ok(v) => v,
                Err(e) => {
                    // Non-fatal : on garde le token même si tokeninfo échoue.
                    eprintln!("[YouTube] WARN tokeninfo échoué: {}", e);
                    String::new()
                }
            };
            let tokens = Tokens {
                access: t.access_token,
                refresh: t.refresh_token,
                user_id,
                channel_id: String::new(),
                login: String::new(),
            };
            return Ok(tokens);
        }

        // Erreur : Google renvoie "error" dans le body.
        let err = body["error"]
            .as_str()
            .unwrap_or("unknown");
        match err {
            "authorization_pending" => {
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
                let msg = body["error_description"].as_str().unwrap_or("");
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

/// Valide un access token via /tokeninfo → sub (user_id Google).
pub async fn valider_token(token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(TOKENINFO_URL)
        .query(&[("access_token", token)])
        .send()
        .await
        .map_err(|e| format!("Tokeninfo: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Tokeninfo HTTP {}: {}", status, body));
    }

    let v: TokeninfoResp = resp
        .json()
        .await
        .map_err(|e| format!("Tokeninfo parse: {}", e))?;
    eprintln!("[YouTube] token OK sub={}", v.sub);
    Ok(v.sub)
}

/// Rafraîchit un access token expiré via /token (grant_type=refresh_token).
/// Conserve channel_id/login des anciens tokens (inchangés).
pub async fn refresh_token(refresh: &str, old: &Tokens) -> Result<Tokens, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
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

    Ok(Tokens {
        access: t.access_token,
        refresh: t.refresh_token,
        user_id: old.user_id.clone(),
        channel_id: old.channel_id.clone(),
        login: old.login.clone(),
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

/// Révoque un access token côté Google (POST /revoke). Best-effort :
/// l'appelant efface le coffre quoi qu'il arrive. Non-fatal si échec réseau.
pub async fn revoke_token(token: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(REVOKE_URL)
        .form(&[("token", token)])
        .send()
        .await
        .map_err(|e| format!("Revoke: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Revoke HTTP {}: {}", status, body));
    }
    eprintln!("[YouTube] token révoqué");
    Ok(())
}
