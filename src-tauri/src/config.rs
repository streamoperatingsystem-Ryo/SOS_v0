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

// ===== Persistance résolution canvas OBS (globale, pas par scène) =====

#[derive(Serialize, Deserialize)]
struct ObsCanvasConfig {
    w: u32,
    h: u32,
}

/// Lit la dernière résolution canvas OBS connue (obs_canvas.json).
/// None si jamais connecté à OBS.
pub fn lire_obs_canvas(app: &AppHandle) -> Result<Option<(u32, u32)>, String> {
    let dir = data_dir(app)?;
    let path = dir.join("obs_canvas.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire obs_canvas.json: {}", e))?;
    let cfg: ObsCanvasConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse obs_canvas.json: {}", e))?;
    if cfg.w == 0 || cfg.h == 0 {
        return Ok(None);
    }
    Ok(Some((cfg.w, cfg.h)))
}

/// Sauve la résolution canvas OBS (écrase si existe). Appelé à chaque
/// obs_connect réussi — la valeur est appliquée à TOUTES les scènes au
/// chargement (boot, création, bascule, import).
pub fn sauver_obs_canvas(app: &AppHandle, w: u32, h: u32) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("obs_canvas.json");
    let cfg = ObsCanvasConfig { w, h };
    let json = serde_json::to_string(&cfg)
        .map_err(|e| format!("Serialize obs_canvas.json: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Écrire obs_canvas.json: {}", e))
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

// ===== Persistance config Speedrun Splitter =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpeedrunSettings {
    pub start: bool,
    pub split: bool,
    pub reset: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SpeedrunConfig {
    #[serde(default)]
    pub chemin_asl: Option<String>,
    #[serde(default)]
    pub chemin_lss: Option<String>,
    #[serde(default)]
    pub settings: SpeedrunSettings,
    /// Settings ASL individuels (120+ pour MGS) persistés entre les sessions.
    /// Map : code-signature → bool (coché/décoché). Restaurés dans le Context
    /// Boa au chargement de l'ASL si l'utilisateur a déjà configuré les splits.
    /// Vide pour les ASL simples (type SOR) qui n'ont pas de settings individuels.
    #[serde(default)]
    pub settings_asl: std::collections::HashMap<String, bool>,
}

impl Default for SpeedrunSettings {
    fn default() -> Self {
        Self { start: true, split: true, reset: true }
    }
}

/// Lit la config speedrun sauvegardée. Defaults si absent.
pub fn lire_speedrun_config(app: &AppHandle) -> Result<SpeedrunConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("speedrun.json");
    if !path.exists() {
        return Ok(SpeedrunConfig::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire speedrun.json: {}", e))?;
    let cfg: SpeedrunConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse speedrun.json: {}", e))?;
    Ok(cfg)
}

/// Sauve la config speedrun (écrase si existe).
pub fn sauver_speedrun_config(app: &AppHandle, cfg: &SpeedrunConfig) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("speedrun.json");
    let json = serde_json::to_string(cfg)
        .map_err(|e| format!("Serialize speedrun.json: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Écrire speedrun.json: {}", e))
}

// ===== Persistance snapshot followers (détection unfollows) =====

/// Entrée minimale d'un follower pour le snapshot (comparaison au démarrage).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SnapshotFollower {
    pub user_id: String,
    pub login: String,
    pub followed_at: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub profile_image_url: String,
}

/// Snapshot complet : liste des followers + timestamp ISO du snapshot.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct FollowersSnapshot {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub liste: Vec<SnapshotFollower>,
}

/// Lit le snapshot des followers (followers_snapshot.json). None si absent.
pub fn lire_followers_snapshot(app: &AppHandle) -> Result<Option<FollowersSnapshot>, String> {
    let dir = data_dir(app)?;
    let path = dir.join("followers_snapshot.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire followers_snapshot.json: {}", e))?;
    let snap: FollowersSnapshot = serde_json::from_str(&content)
        .map_err(|e| format!("Parse followers_snapshot.json: {}", e))?;
    Ok(Some(snap))
}

/// Sauve le snapshot des followers (écrase si existe).
pub fn sauver_followers_snapshot(
    app: &AppHandle,
    snap: &FollowersSnapshot,
) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("followers_snapshot.json");
    let json = serde_json::to_string_pretty(snap)
        .map_err(|e| format!("Serialize followers_snapshot.json: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Écrire followers_snapshot.json: {}", e))
}

// ===== Persistance historique unfollows =====

/// Entrée d'un unfollow : qui a unfollow, et quand (détecté au démarrage).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UnfollowEntry {
    pub user_id: String,
    pub login: String,
    /// Date de détection (ISO 8601, = moment du démarrage StreamOS).
    pub date_unfollow: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub profile_image_url: String,
}

/// Historique des unfollows (unfollows.json). Append-only : on ajoute
/// les nouveaux unfollows détectés au démarrage, on ne supprime jamais.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct UnfollowsHistory {
    #[serde(default)]
    pub liste: Vec<UnfollowEntry>,
}

/// Lit l'historique des unfollows (unfollows.json). Vide si absent.
pub fn lire_unfollows(app: &AppHandle) -> Result<UnfollowsHistory, String> {
    let dir = data_dir(app)?;
    let path = dir.join("unfollows.json");
    if !path.exists() {
        return Ok(UnfollowsHistory::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire unfollows.json: {}", e))?;
    let hist: UnfollowsHistory = serde_json::from_str(&content)
        .map_err(|e| format!("Parse unfollows.json: {}", e))?;
    Ok(hist)
}

/// Ajoute des unfollows à l'historique (append, sauvegarde sur disque).
/// Déduplique : n'ajoute pas un user_id déjà présent dans l'historique.
pub fn ajouter_unfollows(
    app: &AppHandle,
    nouveaux: &[UnfollowEntry],
) -> Result<(), String> {
    if nouveaux.is_empty() {
        return Ok(());
    }
    let mut hist = lire_unfollows(app)?;
    // Set des user_id déjà présents (pour déduplication).
    let existants: std::collections::HashSet<String> =
        hist.liste.iter().map(|u| u.user_id.clone()).collect();
    for u in nouveaux {
        if !existants.contains(&u.user_id) {
            hist.liste.push(u.clone());
        }
    }
    let dir = data_dir(app)?;
    let path = dir.join("unfollows.json");
    let json = serde_json::to_string_pretty(&hist)
        .map_err(|e| format!("Serialize unfollows.json: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Écrire unfollows.json: {}", e))
}
