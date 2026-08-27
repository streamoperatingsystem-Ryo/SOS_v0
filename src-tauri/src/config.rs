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
