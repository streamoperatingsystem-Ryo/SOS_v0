use std::fs;
use std::path::{Path, PathBuf};
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

// ===== Persistance mode moteur (obs | natif) + réglages de sortie =====

/// Réglages de sortie du moteur natif — bloc `sortie` de moteur.json.
/// Appliqués au démarrage d'un enregistrement ou d'un stream
/// (`moteur_sortie_demarrer` / `moteur_stream_demarrer`).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SortieNatifCfg {
    /// Codec vidéo : "h264" | "hevc" | "av1".
    #[serde(default = "sortie_codec_defaut")]
    pub codec: String,
    /// Conteneur du fichier : "mp4" (fragmenté) | "mkv" (matroska).
    /// N'intervient pas sur un flux réseau (le protocole fixe le format).
    #[serde(default = "sortie_conteneur_defaut")]
    pub conteneur: String,
    /// Hauteur de la résolution de sortie : `None` = canvas (historique).
    /// La largeur est dérivée du ratio du canvas au démarrage — jamais
    /// persistée, elle suivrait mal un changement de canvas.
    #[serde(default)]
    pub resolution: Option<u32>,
    /// Débit vidéo en kbps.
    #[serde(default = "sortie_debit_defaut")]
    pub debit_kbps: u32,
    #[serde(default = "sortie_fps_defaut")]
    pub fps: u32,
    /// Preset d'encodage : "rapide" | "equilibre" | "qualite".
    #[serde(default = "sortie_preset_defaut")]
    pub preset: String,
    #[serde(default = "vrai")]
    pub audio_loopback: bool,
    #[serde(default = "vrai")]
    pub audio_micro: bool,
    /// Gains en millièmes (1000 = 1.0, max 4000 — bornes `SortieCfg::valider`).
    #[serde(default = "sortie_gain_defaut")]
    pub gain_loopback: u32,
    #[serde(default = "sortie_gain_defaut")]
    pub gain_micro: u32,
}

fn sortie_codec_defaut() -> String {
    "h264".to_string()
}
fn sortie_conteneur_defaut() -> String {
    "mp4".to_string()
}
fn sortie_debit_defaut() -> u32 {
    6000
}
fn sortie_fps_defaut() -> u32 {
    60
}
fn sortie_preset_defaut() -> String {
    "equilibre".to_string()
}
fn vrai() -> bool {
    true
}
fn sortie_gain_defaut() -> u32 {
    1000
}

impl Default for SortieNatifCfg {
    /// Défauts produit : H.264, 6000 kbps, 60 fps, « equilibre », loopback +
    /// micro à gain unité — les valeurs historiques figées du lot 1.4.
    fn default() -> Self {
        Self {
            codec: sortie_codec_defaut(),
            conteneur: sortie_conteneur_defaut(),
            resolution: None,
            debit_kbps: sortie_debit_defaut(),
            fps: sortie_fps_defaut(),
            preset: sortie_preset_defaut(),
            audio_loopback: true,
            audio_micro: true,
            gain_loopback: sortie_gain_defaut(),
            gain_micro: sortie_gain_defaut(),
        }
    }
}

/// Lit moteur.json en objet générique : les clés inconnues (`encodeur_choisi`,
/// futures) sont préservées à la réécriture (doc §17 — toute clé inconnue
/// est ignorée, jamais effacée). Cassé/absent → objet vide (auto-guérison).
fn lire_moteur_json(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn ecrire_moteur_json(
    path: &Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let json = serde_json::to_string(&serde_json::Value::Object(obj.clone()))
        .map_err(|e| format!("Serialize moteur.json: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Écrire moteur.json: {}", e))
}

/// Lit le mode moteur persisté (moteur.json). `None` si absent.
pub fn lire_mode_moteur(app: &AppHandle) -> Result<Option<String>, String> {
    let dir = data_dir(app)?;
    let path = dir.join("moteur.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("Lire moteur.json: {}", e))?;
    let v: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Parse moteur.json: {}", e))?;
    Ok(v.get("mode")
        .and_then(|m| m.as_str())
        .map(str::to_string))
}

/// Sauve le mode moteur. Relit le fichier d'abord : le bloc `sortie` (et
/// toute clé inconnue) est préservé — l'écriture directe historique
/// l'écrasait.
pub fn sauver_mode_moteur(app: &AppHandle, mode: &str) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("moteur.json");
    let mut obj = lire_moteur_json(&path);
    obj.insert("mode".to_string(), serde_json::json!(mode));
    ecrire_moteur_json(&path, &obj)
}

/// Lit les réglages de sortie natifs (bloc `sortie` de moteur.json).
/// Bloc absent → défauts produit ; bloc présent mais invalide → erreur
/// honnête (jamais de repli muet sur une config écrite).
pub fn lire_sortie_moteur(app: &AppHandle) -> Result<SortieNatifCfg, String> {
    let dir = data_dir(app)?;
    let path = dir.join("moteur.json");
    let obj = lire_moteur_json(&path);
    match obj.get("sortie") {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| format!("Parse moteur.json sortie: {}", e)),
        None => Ok(SortieNatifCfg::default()),
    }
}

/// Sauve les réglages de sortie natifs (bloc `sortie` de moteur.json).
/// `mode` et les clés inconnues sont préservés.
pub fn sauver_sortie_moteur(app: &AppHandle, cfg: &SortieNatifCfg) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("moteur.json");
    let mut obj = lire_moteur_json(&path);
    let v = serde_json::to_value(cfg).map_err(|e| format!("Serialize sortie cfg: {}", e))?;
    obj.insert("sortie".to_string(), v);
    ecrire_moteur_json(&path, &obj)
}

/// Réglages du projecteur natif — bloc `apercu` de moteur.json. Consommés
/// au démarrage de la session aperçu (`moteur_apercu`) et injectés dans la
/// `SortieCfg` d'un enregistrement ou d'un stream (le projecteur sert alors
/// les frames de la boucle en cours).
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct ApercuNatifCfg {
    /// Largeur maximale du flux aperçu, en pixels — `0` = résolution canvas.
    #[serde(default = "apercu_largeur_defaut")]
    pub largeur: u32,
    /// Cadence d'émission des frames réduites (10 | 15 | 30 dans l'UI).
    #[serde(default = "apercu_fps_defaut")]
    pub fps: u32,
}

fn apercu_largeur_defaut() -> u32 {
    1280
}
fn apercu_fps_defaut() -> u32 {
    30
}

impl Default for ApercuNatifCfg {
    /// Défaut produit : 1280 px à 30 fps — miroir de `ApercuCfg::default`
    /// (sos-moteur-api) ; les deux doivent rester alignés.
    fn default() -> Self {
        Self {
            largeur: apercu_largeur_defaut(),
            fps: apercu_fps_defaut(),
        }
    }
}

/// Lit les réglages du projecteur (bloc `apercu` de moteur.json). Bloc
/// absent → défauts produit ; bloc invalide → erreur honnête.
pub fn lire_apercu_moteur(app: &AppHandle) -> Result<ApercuNatifCfg, String> {
    let dir = data_dir(app)?;
    let path = dir.join("moteur.json");
    let obj = lire_moteur_json(&path);
    match obj.get("apercu") {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| format!("Parse moteur.json apercu: {}", e)),
        None => Ok(ApercuNatifCfg::default()),
    }
}

/// Sauve les réglages du projecteur (bloc `apercu` de moteur.json).
/// `mode`, `sortie` et les clés inconnues sont préservés.
pub fn sauver_apercu_moteur(app: &AppHandle, cfg: &ApercuNatifCfg) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("moteur.json");
    let mut obj = lire_moteur_json(&path);
    let v = serde_json::to_value(cfg).map_err(|e| format!("Serialize apercu cfg: {}", e))?;
    obj.insert("apercu".to_string(), v);
    ecrire_moteur_json(&path, &obj)
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

// Type pur défini dans `sos-scene` (anneau 0) : ré-export pour que
// `crate::config::SpeedrunSettings` reste valable partout.
pub use sos_scene::SpeedrunSettings;

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
    /// Compteur de tentatives de run (« Partie n°N » de l'export splits).
    /// Incrémenté par le frontend à chaque event `started` (start manuel ou
    /// ASL) ; jamais décrémenté (undo/goto/reset ne comptent pas).
    #[serde(default)]
    pub nb_tentatives: u32,
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


// ===== Persistance barre de raccourcis =====

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RaccourcisConfig {
    #[serde(default)]
    pub visible: bool,
    /// Bord de l'écran : "gauche" | "droite" | "haut" | "bas".
    #[serde(default = "default_bord_raccourcis")]
    pub bord: String,
    /// Index dans available_monitors() au moment de la sauvegarde.
    #[serde(default)]
    pub monitor_index: usize,
}

fn default_bord_raccourcis() -> String {
    "droite".to_string()
}

/// Lit la config de la barre de raccourcis (raccourcis.json). None si absent.
pub fn lire_raccourcis_config(app: &AppHandle) -> Result<Option<RaccourcisConfig>, String> {
    let dir = data_dir(app)?;
    let path = dir.join("raccourcis.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire raccourcis.json: {}", e))?;
    let cfg: RaccourcisConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse raccourcis.json: {}", e))?;
    Ok(Some(cfg))
}

/// Sauve la config de la barre de raccourcis (écrase si existe).
pub fn sauver_raccourcis_config(app: &AppHandle, cfg: &RaccourcisConfig) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("raccourcis.json");
    let json = serde_json::to_string(cfg)
        .map_err(|e| format!("Serialize raccourcis.json: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Écrire raccourcis.json: {}", e))
}

// ===== Persistance fenêtre principale (écran + mode au démarrage) =====

/// Dernière géométrie fenêtrée connue (px physiques) — restaurée quand le
/// mode "fenetre" est choisi. Sauvegardée à la fermeture de l'app et à chaque
/// `fenetre_appliquer` tant que la fenêtre est en mode fenêtré.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FenetreGeo {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FenetreConfig {
    /// Index dans available_monitors() au moment de la sauvegarde.
    /// Hors bornes (écran débranché) → primaire (même règle que raccourcis).
    #[serde(default)]
    pub monitor_index: usize,
    /// "fenetre" | "maximise" | "fullscreen" — défaut "maximise"
    /// (= comportement historique de tauri.conf.json).
    #[serde(default = "default_mode_fenetre")]
    pub mode: String,
    /// Géométrie fenêtrée persistée. None tant que l'app n'a jamais été
    /// quittée/configurée en mode fenêtré → centrage 1280×800 au restore.
    #[serde(default)]
    pub geo: Option<FenetreGeo>,
}

fn default_mode_fenetre() -> String {
    "maximise".to_string()
}

/// Lit la config de la fenêtre principale (fenetre.json). None si absent.
pub fn lire_fenetre_config(app: &AppHandle) -> Result<Option<FenetreConfig>, String> {
    let dir = data_dir(app)?;
    let path = dir.join("fenetre.json");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire fenetre.json: {}", e))?;
    let cfg: FenetreConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse fenetre.json: {}", e))?;
    Ok(Some(cfg))
}

/// Sauve la config de la fenêtre principale (écrase si existe).
pub fn sauver_fenetre_config(app: &AppHandle, cfg: &FenetreConfig) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("fenetre.json");
    let json = serde_json::to_string(cfg)
        .map_err(|e| format!("Serialize fenetre.json: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Écrire fenetre.json: {}", e))
}

// ===== Persistance onboarding presets =====

/// Lit le flag « onboarding fait » (onboarding.json). Absent = false (premier
/// lancement → montrer la galerie de presets). Sémantique « sûr » en cas
/// d'erreur de lecture (fichier illisible, data_dir KO) : true = ne pas
/// re-afficher l'onboarding à chaque boot si on ne peut pas le lire.
pub fn lire_onboarding_fait(app: &AppHandle) -> bool {
    let Ok(dir) = data_dir(app) else {
        return true;
    };
    let path = dir.join("onboarding.json");
    if !path.exists() {
        return false;
    }
    let Ok(content) = fs::read_to_string(&path) else {
        return true;
    };
    serde_json::from_str::<serde_json::Value>(&content)
        .ok()
        .and_then(|v| v.get("fait").and_then(|f| f.as_bool()))
        .unwrap_or(true)
}

/// Écrit onboarding.json { "fait": true } (écrase si existe).
pub fn marquer_onboarding_fait(app: &AppHandle) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("onboarding.json");
    let json = serde_json::json!({ "fait": true }).to_string();
    fs::write(&path, json).map_err(|e| format!("Écrire onboarding.json: {}", e))
}

// ===== Persistance langue UI (i18n dashboard) =====

/// Lit la langue de l'UI (langue.json { "langue": "fr" }). None si absent ou
/// illisible → le frontend détecte alors navigator.language (fallback "fr").
/// Le code n'est PAS validé ici : la whitelist des langues vit côté front
/// (stores/langue.ts) — un code inconnu retombe sur "fr" naturellement.
pub fn lire_langue(app: &AppHandle) -> Option<String> {
    let dir = data_dir(app).ok()?;
    let path = dir.join("langue.json");
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("langue")
        .and_then(|l| l.as_str())
        .map(|s| s.to_string())
}

/// Écrit langue.json { "langue": "<code>" } (écrase si existe).
pub fn sauver_langue(app: &AppHandle, langue: &str) -> Result<(), String> {
    let dir = data_dir(app)?;
    let path = dir.join("langue.json");
    let json = serde_json::json!({ "langue": langue }).to_string();
    fs::write(&path, json).map_err(|e| format!("Écrire langue.json: {}", e))
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
