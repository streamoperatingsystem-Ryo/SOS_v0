use crate::scene::Scene;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

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

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("config.json"))
}

/// Charge la scène depuis config.json. Retourne une scène vide si le fichier
/// n'existe pas encore.
pub fn load_scene(app: &AppHandle) -> Result<Scene, String> {
    let path = config_path(app)?;
    if !path.exists() {
        return Ok(Scene::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Erreur lecture config.json: {}", e))?;
    let scene: Scene =
        serde_json::from_str(&content).map_err(|e| format!("Erreur parse config.json: {}", e))?;
    Ok(scene)
}

/// Sauvegarde la scène dans config.json.
pub fn save_scene(app: &AppHandle, scene: &Scene) -> Result<(), String> {
    let path = config_path(app)?;
    let content =
        serde_json::to_string_pretty(scene).map_err(|e| format!("Erreur sérialisation: {}", e))?;
    fs::write(&path, content)
        .map_err(|e| format!("Erreur écriture config.json: {}", e))?;
    Ok(())
}
