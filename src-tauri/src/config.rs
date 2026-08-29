use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use serde::{Deserialize, Serialize};

/// Retourne le dossier de données de l'app (%APPDATA%/StreamOS/).
/// Le crée s'il n'existe pas, ainsi que le sous-dossier medias/.
pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Impossible d'obtenir app_data_dir: {}", e))?;

    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Impossible de créer app_data_dir: {}", e))?;
    }

    let medias = dir.join("medias");
    if !medias.exists() {
        fs::create_dir_all(&medias)
            .map_err(|e| format!("Impossible de créer medias/: {}", e))?;
    }

    Ok(dir)
}

// ===== Persistance slug Kick =====

#[derive(Serialize, Deserialize)]
struct KickConfig {
    slug: String,
}

/// Lit le slug Kick sauvegardé. None si absent.
pub fn lire_kick_slug(app: &AppHandle) -> Result<Option<String>, String> {
    let dir = data_dir(app)?;
    let path = dir.join("kick.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire kick.json: {}", e))?;
    let cfg: KickConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse kick.json: {}", e))?;
    Ok(Some(cfg.slug))
}

/// Sauve le slug Kick (écrase si existe).
pub fn sauver_kick_slug(app: &AppHandle, slug: &str) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("kick.json");
    let cfg = KickConfig { slug: slug.to_string() };
    let json = serde_json::to_string(&cfg)
        .map_err(|e| format!("Serialize kick.json: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Écrire kick.json: {}", e))
}

/// Efface le slug Kick sauvegardé. Idempotent.
pub fn effacer_kick_slug(app: &AppHandle) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("kick.json");
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("Effacer kick.json: {}", e))?;
    }
    Ok(())
}

// ===== Persistance username TikTok =====

#[derive(Serialize, Deserialize)]
struct TiktokConfig {
    username: String,
}

/// Lit le username TikTok sauvegardé. None si absent.
pub fn lire_tiktok_username(app: &AppHandle) -> Result<Option<String>, String> {
    let dir = data_dir(app)?;
    let path = dir.join("tiktok.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire tiktok.json: {}", e))?;
    let cfg: TiktokConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse tiktok.json: {}", e))?;
    Ok(Some(cfg.username))
}

/// Sauve le username TikTok (écrase si existe).
pub fn sauver_tiktok_username(app: &AppHandle, username: &str) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("tiktok.json");
    let cfg = TiktokConfig { username: username.to_string() };
    let json = serde_json::to_string(&cfg)
        .map_err(|e| format!("Serialize tiktok.json: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Écrire tiktok.json: {}", e))
}

/// Efface le username TikTok sauvegardé. Idempotent.
pub fn effacer_tiktok_username(app: &AppHandle) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("tiktok.json");
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("Effacer tiktok.json: {}", e))?;
    }
    Ok(())
}
