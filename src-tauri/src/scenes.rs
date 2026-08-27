/// Stockage et logique des scènes (v0.14).
///
/// Disque :
///   data/scenes/index.json = [{ id, nom }]            (léger, ids + noms)
///   data/scenes/<id>.json  = contenu Scene complet    (widgets + fond + canvas)
///   config.json            = { sceneId }              (id courant seulement)
///
/// RAM : UNE seule scène (Arc<Mutex<Scene>>) + current_id. Pas de Vec<Scene>,
/// pas de préchargement « au cas où ».
///
/// Migration au boot : si index.json absent mais ancien config.json (full Scene)
/// présent → générer un id, écrire <id>.json + index.json + nouveau config.json.
use crate::scene::Scene;
use crate::config::data_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tokio::sync::broadcast;

/// Entrée de l'index des scènes (id + nom seulement, jamais le contenu).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneIndex {
    pub id: String,
    pub nom: String,
}

/// État partagé scènes : scène unique en RAM + id courant + canal snapshot.
/// Clonable (Arc internes) — passé aux commandes Tauri et au serveur :4321.
#[derive(Clone)]
pub struct ScenesState {
    pub scene: Arc<Mutex<Scene>>,
    pub current_id: Arc<Mutex<String>>,
    pub snapshot_tx: broadcast::Sender<String>,
}

/// Sérialise la scène en message snapshot WS.
fn snapshot_json(scene: &Scene) -> String {
    serde_json::json!({
        "type": "snapshot",
        "scene": scene
    })
    .to_string()
}

/// Pousse le snapshot courant vers tous les clients WS connectés.
pub fn push_snapshot(state: &ScenesState) {
    let scene = state.scene.lock().unwrap();
    let json = snapshot_json(&scene);
    // send_err = aucun client connecté, c'est OK
    let _ = state.snapshot_tx.send(json);
}

// ===== Chemins disque =====

fn scenes_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("scenes");
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("Impossible de créer scenes/: {}", e))?;
    }
    Ok(dir)
}

fn index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(scenes_dir(app)?.join("index.json"))
}

fn scene_file_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(scenes_dir(app)?.join(format!("{}.json", id)))
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("config.json"))
}

// ===== Index =====

/// Charge l'index des scènes. Retourne Vec vide si le fichier n'existe pas.
fn load_index(app: &AppHandle) -> Result<Vec<SceneIndex>, String> {
    let path = index_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Erreur lecture index.json: {}", e))?;
    let idx: Vec<SceneIndex> =
        serde_json::from_str(&content).map_err(|e| format!("Erreur parse index.json: {}", e))?;
    Ok(idx)
}

/// Sauvegarde l'index des scènes.
fn save_index(app: &AppHandle, idx: &[SceneIndex]) -> Result<(), String> {
    let path = index_path(app)?;
    let content =
        serde_json::to_string_pretty(idx).map_err(|e| format!("Erreur sérialisation index: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("Erreur écriture index.json: {}", e))?;
    Ok(())
}

// ===== Scène fichier =====

fn save_scene_file(app: &AppHandle, id: &str, scene: &Scene) -> Result<(), String> {
    let path = scene_file_path(app, id)?;
    let content =
        serde_json::to_string_pretty(scene).map_err(|e| format!("Erreur sérialisation scène: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("Erreur écriture {}.json: {}", id, e))?;
    Ok(())
}

fn load_scene_file(app: &AppHandle, id: &str) -> Result<Scene, String> {
    let path = scene_file_path(app, id)?;
    if !path.exists() {
        return Err(format!("Scène {} introuvable", id));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Erreur lecture {}.json: {}", id, e))?;
    let scene: Scene =
        serde_json::from_str(&content).map_err(|e| format!("Erreur parse {}.json: {}", id, e))?;
    Ok(scene)
}

// ===== config.json (id courant) =====

fn save_current_id_config(app: &AppHandle, id: &str) -> Result<(), String> {
    let path = config_path(app)?;
    let content = serde_json::json!({ "sceneId": id }).to_string();
    fs::write(&path, content).map_err(|e| format!("Erreur écriture config.json: {}", e))?;
    Ok(())
}

fn load_current_id_config(app: &AppHandle) -> Result<Option<String>, String> {
    let path = config_path(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Erreur lecture config.json: {}", e))?;
    // Tente nouveau format { sceneId }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
        if let Some(id) = v.get("sceneId").and_then(|s| s.as_str()) {
            return Ok(Some(id.to_string()));
        }
    }
    Ok(None)
}

// ===== Boot + migration =====

/// Génère un id court (8 chars hex).
fn nouvel_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// Boot : migration + chargement initial. Retourne (Scene, current_id).
///
/// 1. Si index.json existe → lire config.json sceneId (ou 1ère scène de l'index)
///    → charger <id>.json.
/// 2. Sinon (premier lancement ou migration) :
///    - Lire ancien config.json comme full Scene (fallback Scene::new()).
///    - Générer un id, écrire <id>.json + index.json [{id, "Défaut"}] + config.json.
pub fn boot_scenes(app: &AppHandle) -> Result<(Scene, String), String> {
    let index = load_index(app)?;

    if !index.is_empty() {
        // Index existe → charger scène courante (config.json sceneId ou 1ère)
        let current_id = load_current_id_config(app)?
            .filter(|id| index.iter().any(|s| s.id == *id))
            .unwrap_or_else(|| index[0].id.clone());
        let scene = load_scene_file(app, &current_id).unwrap_or_else(|e| {
            log::warn!("Scène {} illisible ({}): scène vide", current_id, e);
            Scene::new()
        });
        // Re-sauver config.json au cas où sceneId était absent/invalide
        let _ = save_current_id_config(app, &current_id);
        return Ok((scene, current_id));
    }

    // Pas d'index → migration depuis ancien config.json (full Scene) ou scène vide
    let (scene, nom) = migrer_ancien_config(app)?;
    let id = nouvel_id();
    save_scene_file(app, &id, &scene)?;
    save_index(app, &[SceneIndex { id: id.clone(), nom: nom.clone() }])?;
    save_current_id_config(app, &id)?;
    log::info!("Migration scènes : scène « {} » créée (id={})", nom, id);
    Ok((scene, id))
}

/// Lit l'ancien config.json (full Scene) pour migration. Retourne (Scene, nom).
/// Si ancien config.json absent ou non-Scene → Scene::new() + "Défaut".
fn migrer_ancien_config(app: &AppHandle) -> Result<(Scene, String), String> {
    let path = config_path(app)?;
    if !path.exists() {
        return Ok((Scene::new(), "Défaut".to_string()));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Erreur lecture ancien config.json: {}", e))?;
    // Tente de parser comme Scene (ancien format)
    match serde_json::from_str::<Scene>(&content) {
        Ok(scene) => {
            let nom = if scene.widgets.is_empty() && scene.bgMedia.is_empty() {
                "Défaut".to_string()
            } else {
                "Défaut".to_string()
            };
            Ok((scene, nom))
        }
        Err(_) => {
            // Nouveau format { sceneId } sans index → scène vide
            log::warn!("Ancien config.json non-Scene et index absent → scène vide");
            Ok((Scene::new(), "Défaut".to_string()))
        }
    }
}

// ===== Logique scènes (utilisée par UI + API deck) =====

/// Sauve la scène courante sur disque (<current_id>.json) + pousse snapshot.
/// Remplace l'ancien config::save_scene.
pub fn save_current(app: &AppHandle, state: &ScenesState) -> Result<(), String> {
    let (scene, id) = {
        let s = state.scene.lock().unwrap().clone();
        let i = state.current_id.lock().unwrap().clone();
        (s, i)
    };
    save_scene_file(app, &id, &scene)?;
    push_snapshot(state);
    Ok(())
}

/// Liste l'index des scènes (ids + noms seulement).
pub fn lister(app: &AppHandle) -> Result<Vec<SceneIndex>, String> {
    load_index(app)
}

/// Retourne l'entrée de l'index pour un id donné.
fn trouver_entree<'a>(idx: &'a [SceneIndex], id: &str) -> Option<&'a SceneIndex> {
    idx.iter().find(|s| s.id == id)
}

/// Crée une nouvelle scène vide (1920×1080, 0 widget, pas de fond), bascule
/// dessus. Sauve la courante d'abord. Retourne l'entrée créée.
pub fn creer(app: &AppHandle, state: &ScenesState, nom: &str) -> Result<SceneIndex, String> {
    // 1. Sauver la courante
    save_current(app, state)?;

    // 2. Nouvelle scène vide + id unique
    let id = nouvel_id();
    let scene = Scene::new();
    let entry = SceneIndex { id: id.clone(), nom: nom.to_string() };

    // 3. Écriture disque : <id>.json + index += + config.json
    save_scene_file(app, &id, &scene)?;
    let mut idx = load_index(app)?;
    idx.push(entry.clone());
    save_index(app, &idx)?;
    save_current_id_config(app, &id)?;

    // 4. Bascule RAM + snapshot
    {
        let mut s = state.scene.lock().unwrap();
        *s = scene;
        let mut cid = state.current_id.lock().unwrap();
        *cid = id.clone();
    }
    push_snapshot(state);

    log::info!("Scène créée : « {} » (id={})", nom, id);
    Ok(entry)
}

/// Ouvre une scène : sauve la courante, charge <id>.json, bascule, snapshot.
pub fn ouvrir(app: &AppHandle, state: &ScenesState, id: &str) -> Result<SceneIndex, String> {
    let idx = load_index(app)?;
    let entry = trouver_entree(&idx, id)
        .ok_or_else(|| format!("Scène {} introuvable dans l'index", id))?
        .clone();

    // 1. Sauver la courante (si id différent)
    let current_id = state.current_id.lock().unwrap().clone();
    if current_id != id {
        save_current(app, state)?;
    }

    // 2. Charger la nouvelle scène
    let scene = load_scene_file(app, id)?;

    // 3. Bascule RAM + config + snapshot
    {
        let mut s = state.scene.lock().unwrap();
        *s = scene;
        let mut cid = state.current_id.lock().unwrap();
        *cid = id.to_string();
    }
    save_current_id_config(app, id)?;
    push_snapshot(state);

    log::info!("Scène ouverte : « {} » (id={})", entry.nom, id);
    Ok(entry)
}

/// Renomme une scène dans l'index (pas de re-sauvegarde du contenu).
pub fn renommer(app: &AppHandle, id: &str, nom: &str) -> Result<(), String> {
    let mut idx = load_index(app)?;
    let entry = idx.iter_mut().find(|s| s.id == id)
        .ok_or_else(|| format!("Scène {} introuvable", id))?;
    entry.nom = nom.to_string();
    save_index(app, &idx)?;
    log::info!("Scène {} renommée : « {} »", id, nom);
    Ok(())
}

/// Retourne l'entrée courante ({ id, nom }).
pub fn courante(app: &AppHandle, state: &ScenesState) -> Result<SceneIndex, String> {
    let id = state.current_id.lock().unwrap().clone();
    let idx = load_index(app)?;
    let entry = trouver_entree(&idx, &id)
        .map(|e| e.clone())
        .unwrap_or_else(|| SceneIndex { id: id.clone(), nom: "Défaut".to_string() });
    Ok(entry)
}

/// Collecte les chemins médias référencés par une scène (widgets.media +
/// bgMedia), dédupliqués. Retourne une liste de chaînes `medias/<file>`.
fn collecter_medias(scene: &Scene) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for w in &scene.widgets {
        if let Some(m) = &w.media {
            if !m.is_empty() {
                set.insert(m.clone());
            }
        }
    }
    if !scene.bgMedia.is_empty() {
        set.insert(scene.bgMedia.clone());
    }
    set.into_iter().collect()
}

/// Exporte la scène courante comme pack dossier portable.
///
/// `<parent>/<nom_pack>/scene.json` (scène, chemins médias relatifs au pack)
/// `<parent>/<nom_pack>/medias/<fichiers réellement utilisés>`
///
/// `nom_pack` est fourni par l'UI (input texte, défaut = nom de scène).
/// Nom vide → erreur. Dialog = dossier PARENT seulement.
/// Copie SEULEMENT les fichiers référencés (widgets.media + bgMedia).
/// Si un média manque sur disque → export quand même, log warn, le chemin
/// reste dans scene.json (import saura qu'il est attendu → case vide).
/// Si `<nom_pack>/` existe déjà → erreur (pas d'écrasement silencieux).
/// Retourne true si exporté, false si dialog annulé.
pub fn exporter(app: &AppHandle, state: &ScenesState, nom_pack: &str) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;

    let nom_pack = nom_pack.trim();
    if nom_pack.is_empty() {
        return Err("Nom du pack vide".into());
    }

    let (scene, id) = {
        let s = state.scene.lock().unwrap().clone();
        let i = state.current_id.lock().unwrap().clone();
        (s, i)
    };
    let idx = load_index(app)?;
    let nom_scene = trouver_entree(&idx, &id).map(|e| e.nom.clone()).unwrap_or_else(|| "scene".to_string());

    // 1. Dialog dossier parent
    let folder = app.dialog().file().blocking_pick_folder();
    let Some(folder) = folder else {
        return Ok(false); // dialog annulé
    };
    let parent: PathBuf = folder.into_path().map_err(|e| format!("Chemin invalide: {}", e))?;

    // 2. <parent>/<nom_pack>/ — erreur si existe déjà
    let pack_dir = parent.join(nom_pack);
    if pack_dir.exists() {
        return Err(format!("Le dossier « {} » existe déjà dans {}", nom_pack, parent.display()));
    }
    let medias_dir = pack_dir.join("medias");
    fs::create_dir_all(&medias_dir).map_err(|e| format!("Création dossier pack: {}", e))?;

    // 3. Copier les médias référencés (seulement ceux qui existent)
    let data_medias = data_dir(app)?.join("medias");
    let refs = collecter_medias(&scene);
    let mut copies = 0u32;
    let mut manquants = 0u32;
    for rel in &refs {
        // rel = "medias/<file>" ; le fichier source est data_dir/medias/<file>
        let file_name = rel.strip_prefix("medias/").unwrap_or(rel);
        let src = data_medias.join(file_name);
        if src.exists() {
            let dest = medias_dir.join(file_name);
            fs::copy(&src, &dest).map_err(|e| format!("Copie média {}: {}", file_name, e))?;
            copies += 1;
        } else {
            manquants += 1;
            log::warn!("Export : média manquant sur disque ({}), chemin gardé dans scene.json", rel);
        }
    }

    // 4. scene.json = Scene + champ nom (chemins médias inchangés, relatifs au pack)
    let mut v = serde_json::to_value(&scene).map_err(|e| format!("Sérialisation: {}", e))?;
    if let Some(obj) = v.as_object_mut() {
        obj.insert("nom".to_string(), serde_json::Value::String(nom_scene.clone()));
    }
    let content = serde_json::to_string_pretty(&v).map_err(|e| format!("Sérialisation: {}", e))?;
    let scene_json = pack_dir.join("scene.json");
    fs::write(&scene_json, content).map_err(|e| format!("Écriture scene.json: {}", e))?;

    log::info!(
        "Pack exporté : {:?} ({} médias copiés, {} manquants)",
        pack_dir, copies, manquants
    );
    Ok(true)
}

/// Importe une scène depuis un dossier pack (celui qui contient scene.json)
/// ou un legacy .json seul (sans medias/ → cases vides).
///
/// - Trouve scene.json dans le dossier ; sinon cherche un seul .json à la racine (legacy).
/// - Valide widgets[] (schéma min).
/// - Pour chaque média référencé : si <dossier>/medias/<file> existe → copier
///   vers data/medias/<new-uuid>.<ext> (anti-collision) → réécrire le chemin.
///   Si manque → garde le chemin original (case vide au rendu, 404 sur :4321).
/// - Nouvel id, index +=, bascule, snapshot.
/// Retourne Some(entry) si importé, None si dialog annulé.
pub fn importer(app: &AppHandle, state: &ScenesState) -> Result<Option<SceneIndex>, String> {
    use tauri_plugin_dialog::DialogExt;

    // 1. Dialog dossier
    let folder = app.dialog().file().blocking_pick_folder();
    let Some(folder) = folder else {
        return Ok(None); // dialog annulé
    };
    let dossier: PathBuf = folder.into_path().map_err(|e| format!("Chemin invalide: {}", e))?;

    // 2. Trouver scene.json, sinon un seul .json à la racine (legacy)
    let scene_json_path = {
        let direct = dossier.join("scene.json");
        if direct.exists() {
            direct
        } else {
            // Chercher .json à la racine du dossier
            let mut jsons: Vec<PathBuf> = Vec::new();
            if let Ok(entries) = fs::read_dir(&dossier) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("json") && p.is_file() {
                        jsons.push(p);
                    }
                }
            }
            if jsons.len() == 1 {
                jsons[0].clone()
            } else if jsons.is_empty() {
                return Err("Aucun scene.json (ni .json) trouvé dans le dossier".into());
            } else {
                return Err("Plusieurs .json trouvés : renommer la scène en scene.json".into());
            }
        }
    };

    // 3. Lecture + validation schéma min
    let content = fs::read_to_string(&scene_json_path)
        .map_err(|e| format!("Lecture scene.json: {}", e))?;
    let v: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("JSON invalide: {}", e))?;
    if !v.get("widgets").and_then(|w| w.as_array()).is_some() {
        return Err("Schéma invalide : champ widgets[] manquant".into());
    }
    let mut scene: Scene =
        serde_json::from_value(v.clone()).map_err(|e| format!("Schéma scène invalide: {}", e))?;
    let nom = v.get("nom").and_then(|n| n.as_str()).map(|s| s.to_string())
        .unwrap_or_else(|| {
            dossier.file_name().and_then(|s| s.to_str()).unwrap_or("Importée").to_string()
        });

    // 4. Copier les médias du pack vers data/medias/ (nouveau uuid anti-collision)
    //    + réécrire les chemins dans la scène.
    let data_medias = data_dir(app)?.join("medias");
    let pack_medias = dossier.join("medias");
    let refs = collecter_medias(&scene);
    let mut copies = 0u32;
    let mut manquants = 0u32;

    // Map ancien chemin → nouveau chemin (pour réécrire widgets + bg)
    let mut remap: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for rel in &refs {
        let file_name = rel.strip_prefix("medias/").unwrap_or(rel);
        let src = pack_medias.join(file_name);
        if src.exists() {
            // Nouveau nom uuid.<ext> anti-collision
            let ext = std::path::Path::new(file_name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("dat");
            let uuid_str = uuid::Uuid::new_v4().simple().to_string();
            let new_name = format!("{}.{}", uuid_str, ext);
            let dest = data_medias.join(&new_name);
            fs::copy(&src, &dest).map_err(|e| format!("Copie média {}: {}", file_name, e))?;
            let new_rel = format!("medias/{}", new_name);
            remap.insert(rel.clone(), new_rel);
            copies += 1;
        } else {
            manquants += 1;
            log::warn!("Import : média manquant dans le pack ({}), chemin original gardé → case vide", rel);
        }
    }

    // 5. Réécrire les chemins dans la scène
    for w in &mut scene.widgets {
        if let Some(m) = &w.media {
            if let Some(new) = remap.get(m) {
                w.media = Some(new.clone());
            }
        }
    }
    if !scene.bgMedia.is_empty() {
        if let Some(new) = remap.get(&scene.bgMedia) {
            scene.bgMedia = new.clone();
        }
    }

    // 6. Sauver la courante d'abord
    save_current(app, state)?;

    // 7. Nouvel id + écriture <id>.json + index += + config
    let id = nouvel_id();
    save_scene_file(app, &id, &scene)?;
    let entry = SceneIndex { id: id.clone(), nom: nom.clone() };
    let mut idx = load_index(app)?;
    idx.push(entry.clone());
    save_index(app, &idx)?;
    save_current_id_config(app, &id)?;

    // 8. Bascule RAM + snapshot
    {
        let mut s = state.scene.lock().unwrap();
        *s = scene;
        let mut cid = state.current_id.lock().unwrap();
        *cid = id.clone();
    }
    push_snapshot(state);

    log::info!(
        "Scène importée : « {} » (id={}, {} médias copiés, {} manquants)",
        nom, id, copies, manquants
    );
    Ok(Some(entry))
}
