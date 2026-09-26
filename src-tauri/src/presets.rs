/// Presets de scènes embarqués — modèles prêts à l'emploi (« user lambda »).
///
/// Un preset = un fichier JSON dans `resources/presets/`, embarqué à la
/// compilation via `include_str!` (même pattern que diffusion.html /
/// chat-popout.html — pas de ressource à bundler, recompile si changement).
/// Format : { id, nom, description, scene: Scene complète }.
///
/// Les presets ne contiennent QUE des widgets fonctionnels sans import média
/// (chat, camera, input-viewer, speedrun) + un cadreWidget — la scène est
/// utilisable immédiatement. Conçu pour un canvas 1920×1080 (appliquer_canvas_obs
/// ajuste canvasW/H si OBS est connu, sans rescaler les widgets — comportement
/// des scènes importées).
///
/// Instanciation : clone de la scène modèle + uuid régénérés par widget (deux
/// scènes issues du même preset ne partagent jamais leurs ids) + canvas OBS.
use crate::scene::Scene;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

const PRESET_GAMING: &str = include_str!("../resources/presets/gaming.json");
const PRESET_JUST_CHATTING: &str = include_str!("../resources/presets/just-chatting.json");
const PRESET_SPEEDRUN: &str = include_str!("../resources/presets/speedrun.json");
const PRESET_MINIMAL: &str = include_str!("../resources/presets/minimal.json");

/// Presets embarqués (id → JSON). L'ordre du tableau = ordre de la galerie.
const PRESETS: &[(&str, &str)] = &[
    ("gaming", PRESET_GAMING),
    ("just-chatting", PRESET_JUST_CHATTING),
    ("speedrun", PRESET_SPEEDRUN),
    ("minimal", PRESET_MINIMAL),
];

/// Métadonnées d'un preset pour la galerie (sans le contenu Scene).
#[derive(Debug, Clone, Serialize)]
pub struct PresetInfo {
    pub id: String,
    pub nom: String,
    pub description: String,
}

/// Preset complet : métadonnées + scène modèle.
#[derive(Debug, Clone, Deserialize)]
pub struct PresetFichier {
    pub id: String,
    pub nom: String,
    pub description: String,
    pub scene: Scene,
}

/// Liste les métadonnées des presets (galerie). Parse complet non-fatal :
/// un preset embarqué illisible est loggé + absent de la galerie, l'app
/// démarre normalement (les autres restent disponibles).
pub fn lister() -> Vec<PresetInfo> {
    PRESETS
        .iter()
        .filter_map(|(_, json)| match serde_json::from_str::<PresetFichier>(json) {
            Ok(p) => Some(PresetInfo {
                id: p.id,
                nom: p.nom,
                description: p.description,
            }),
            Err(e) => {
                log::error!("Preset embarqué illisible (ignoré) : {}", e);
                None
            }
        })
        .collect()
}

/// Charge un preset complet par id. Erreur si id inconnu ou JSON illisible.
pub fn charger(id: &str) -> Result<PresetFichier, String> {
    let json = PRESETS
        .iter()
        .find(|(pid, _)| *pid == id)
        .map(|(_, json)| *json)
        .ok_or_else(|| format!("Preset {} introuvable", id))?;
    serde_json::from_str::<PresetFichier>(json)
        .map_err(|e| format!("Preset {} illisible : {}", id, e))
}

/// Instancie la scène d'un preset : clone + uuid régénérés pour chaque widget
/// + résolution canvas OBS appliquée. Ne touche PAS au fichier JSON source.
pub fn instancier(app: &AppHandle, preset: &PresetFichier) -> Scene {
    let mut scene = preset.scene.clone();
    for w in &mut scene.widgets {
        w.id = uuid::Uuid::new_v4().to_string();
    }
    crate::scenes::appliquer_canvas_obs(app, &mut scene);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les 4 presets embarqués doivent parser en PresetFichier valide —
    /// sinon `lister()` les ignore silencieusement (galerie vide) : ce test
    /// fait sauter une régression de format dès `cargo test`.
    #[test]
    fn presets_embarques_valides() {
        for (id, json) in PRESETS {
            let p: PresetFichier = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("Preset {} illisible : {}", id, e));
            assert_eq!(&p.id, id, "id du fichier ≠ id du registre");
            assert!(!p.nom.is_empty(), "Preset {} sans nom", id);
            assert!(!p.scene.widgets.is_empty(), "Preset {} sans widget", id);
        }
    }
}
