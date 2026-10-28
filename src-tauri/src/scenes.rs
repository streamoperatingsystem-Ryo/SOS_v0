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
use crate::scene::{CadreConfig, Scene};
use crate::config::{data_dir, lire_langue};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tokio::sync::broadcast;

/// Entrée de l'index des scènes (id + nom seulement, jamais le contenu).
/// `nomMasque` : scène marquée « masquée » — titre coloré orange dans sa
/// pastille « Vos scènes » + confirmation avant ouverture. Rétro-compat :
/// ancien index.json sans le champ → false.
/// NB : champ camelCase VOLONTAIRE — contrat JSON direct avec le frontend
/// (même convention que scene.rs). D'où le allow(non_snake_case).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneIndex {
    pub id: String,
    pub nom: String,
    #[serde(default)]
    pub nomMasque: bool,
}

/// Override couleur posé par la commande chat « !cadres <couleur> ».
/// ÉPHÉMÈRE : RAM uniquement, JAMAIS écrit dans scenes/<id>.json — la
/// couleur du streamer prime au redémarrage (contrat : la commande viewer
/// change le direct, pas la sauvegarde). Injecté dans les snapshots WS à
/// la sérialisation (snapshot_json). `scene_id` interdit qu'un override
/// survive à une bascule de scène, et l'override est aussi purgé à chaque
/// remplacement de scène (purge_cadres_chat) + à toute édition du cadre
/// par le streamer (update_scene).
#[derive(Debug, Clone)]
pub struct CadresChatOverride {
    /// Scène visée — l'override ne s'applique qu'à elle.
    pub scene_id: String,
    /// Base du cadre — NOIR (#000000), le « trait principal atténué » du
    /// rendu par défaut (cadres_cmd::signature_couleurs).
    pub couleur: String,
    /// Vive du cadre — couleur viewer brute.
    pub couleur_fin: String,
}

/// État partagé scènes : scène unique en RAM + id courant + canal snapshot.
/// Clonable (Arc internes) — passé aux commandes Tauri et au serveur :4321.
#[derive(Clone)]
pub struct ScenesState {
    pub scene: Arc<Mutex<Scene>>,
    pub current_id: Arc<Mutex<String>>,
    /// Override « !cadres » courant — None la plupart du temps. Jamais
    /// persisté : voir CadresChatOverride.
    pub cadres_chat: Arc<Mutex<Option<CadresChatOverride>>>,
    pub snapshot_tx: broadcast::Sender<String>,
    /// Canal broadcast pour les messages chat (IRC Twitch → diffusion :4321).
    /// Le handler WS s'y abonne et forward `{"type":"chat","message":...}`.
    pub chat_tx: broadcast::Sender<String>,
    /// Module équaliseur : capture WASAPI (loopback sortie + micro optionnel)
    /// → frames eq-spectrum sur chat_tx → overlay diffusion. Zéro thread tant
    /// que le module n'est pas actif ; sync() appelée par push_snapshot.
    pub eq: crate::equaliseur::EqualiseurState,
}

/// Sérialise la scène en message snapshot WS. `sceneId` permet à la
/// diffusion :4321 de distinguer un CHANGEMENT de scène (fondu au noir)
/// d'une simple édition de widgets (même scène, rendu direct).
/// `cadres_chat` : override couleur « !cadres » injecté ICI seulement,
/// sur une copie — state.scene et le fichier sur disque gardent les
/// couleurs du streamer.
fn snapshot_json(id: &str, scene: &Scene, cadres_chat: Option<&CadresChatOverride>) -> String {
    let mut scene_override;
    let scene = match cadres_chat {
        Some(ov) if ov.scene_id == id => {
            scene_override = scene.clone();
            if let Some(c) = scene_override.cadreWidget.as_mut() {
                c.couleur = ov.couleur.clone();
                c.couleurFin = ov.couleur_fin.clone();
            }
            &scene_override
        }
        _ => scene,
    };
    serde_json::json!({
        "type": "snapshot",
        "sceneId": id,
        "scene": scene
    })
    .to_string()
}

/// Snapshot courant sérialisé (override « !cadres » injecté s'il vise la
/// scène courante). Locks internes : scene → current_id → cadres_chat —
/// ne JAMAIS appeler avec un mutex de scène déjà tenu (Mutex non
/// réentrant). Utilisé par push_snapshot et le snapshot initial du
/// serveur :4321.
pub fn snapshot_courant(state: &ScenesState) -> String {
    let scene = state.scene.lock().unwrap();
    let id = state.current_id.lock().unwrap();
    let ov = state.cadres_chat.lock().unwrap();
    snapshot_json(&id, &scene, ov.as_ref())
}

/// Purge l'override couleur « !cadres » — à appeler à chaque REMPLACEMENT
/// de la scène courante (ouvrir / creer_avec / supprimer / importer /
/// preset / restauration zip) : la couleur viewer ne survit jamais à un
/// changement de scène.
fn purge_cadres_chat(state: &ScenesState) {
    *state.cadres_chat.lock().unwrap() = None;
}

/// Pousse le snapshot courant vers tous les clients WS connectés.
pub fn push_snapshot(state: &ScenesState) {
    // ⚠️ Ordre des locks : scene PUIS current_id PUIS cadres_chat — même
    // convention que ouvrir/creer/supprimer/save_current (push_snapshot
    // est appelé depuis plusieurs threads UI + serveur :4321, l'ordre
    // inverse = deadlock).
    let eq_config;
    {
        let scene = state.scene.lock().unwrap();
        let id = state.current_id.lock().unwrap();
        let ov = state.cadres_chat.lock().unwrap();
        let json = snapshot_json(&id, &scene, ov.as_ref());
        // send_err = aucun client connecté, c'est OK
        let _ = state.snapshot_tx.send(json);
        eq_config = scene.modules.as_ref().and_then(|m| m.equaliseur.clone());
    }
    // Synchronisation équaliseur APRÈS la libération des locks : la capture
    // démarre/arrête ses threads selon modules.equaliseur — démarrer un thread
    // sous les mutex de scène ralentirait le chemin de commit.
    state.eq.sync(eq_config.as_ref());
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
    let mut scene: Scene =
        serde_json::from_str(&content).map_err(|e| format!("Erreur parse {}.json: {}", id, e))?;
    // Nettoyage : retirer les anciens widgets "welcome-clip" (devenus overlay
    // autonome côté diffusion, plus gérés comme widgets de scène). Le filtre
    // à la lecture suffit : à la prochaine sauvegarde (update_scene/save_current),
    // le widget est naturellement éliminé du fichier sur disque.
    let before = scene.widgets.len();
    scene.widgets.retain(|w| w.widget_type != "welcome-clip");
    let removed = before - scene.widgets.len();
    if removed > 0 {
        log::info!(
            "Scène {} : {} widget(s) welcome-clip retiré(s) (overlay autonome)",
            id,
            removed
        );
    }
    // Migration cadre unifié : cadreApp legacy → cadreWidget (widget actif
    // gagne, sinon app repris). cadreApp vidé — plus de vérité séparée.
    scene.migrer_cadres();
    // Migration titres multi-positions : titre/titrePosition legacy →
    // titreHaut/titreBas (idem bgTitre → bgTitreHaut/bgTitreBas). Champs
    // legacy vidés — le JSON se nettoie à la prochaine sauvegarde.
    scene.migrer_titres();
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

// ===== Canvas OBS global =====

/// Bruit toléré sur la résolution OBS, avant de considérer qu'il y a
/// changement. OBS renvoie parfois la même résolution arrondie différemment
/// (1360×768 puis 1361×768) : en dessous de ce seuil, **rien** n'est touché
/// (ni les dimensions, ni la géométrie) — sinon la scène serait re-sauvée
/// pour un pixel.
///
/// ⚠️ Ne pas confondre avec un seuil de protection contre la dérive : c'est
/// `canvasDesignW/H` qui garantit l'absence de dérive (chaque adaptation
/// repart de l'espace de conception, donc un aller-retour est exact).
pub(crate) const EPSILON_CANVAS: f64 = 0.001;

/// Applique la dernière résolution canvas OBS connue (obs_canvas.json) à une
/// scène. canvasW/canvasH sont un miroir de la résolution OBS — PAS une
/// propriété par scène. Sans ça, une scène créée/importée avec le défaut
/// 1920×1080 alors qu'OBS est en (ex.) 1842×1036 rend une page :4321 plus
/// large que la source navigateur → bords droit/bas coupés dans OBS.
///
/// Depuis le 2026-10-28, la géométrie **suit** un vrai changement de
/// résolution (voir [`adapter_canvas`]) : sans ça, les widgets restaient aux
/// coordonnées de l'ancien canvas et débordaient.
pub(crate) fn appliquer_canvas_obs(app: &AppHandle, scene: &mut Scene) {
    match crate::config::lire_obs_canvas(app) {
        Ok(Some((w, h))) => {
            let (avant_w, avant_h) = (scene.canvasW, scene.canvasH);
            if adapter_canvas(scene, w, h) {
                log::info!(
                    "Canvas scène {}×{} → {}×{} (résolution OBS connue)",
                    avant_w,
                    avant_h,
                    w,
                    h
                );
            }
        }
        Ok(None) => {} // jamais connecté à OBS → dims de la scène inchangées
        Err(e) => log::warn!("lire_obs_canvas: {} (dims scène inchangées)", e),
    }
}

/// Met la scène aux dimensions `w × h` en repartant de son **espace de
/// conception** (`canvasDesignW/H`).
///
/// Pourquoi repartir de la conception : un rescale direct `ancien → nouveau`
/// arrondit à chaque passage, donc 1920 → 1360 → 1920 ne retrouve pas le
/// layout d'origine. Ici la géométrie est d'abord ramenée dans l'espace de
/// conception (inverse de la transformation courante), puis projetée sur le
/// nouveau canvas : l'aller-retour est exact (aux arrondis des champs
/// entiers près, ±1 px au pire).
///
/// Facteur **uniforme** (`min` des deux ratios) + centrage (letterbox) : un
/// passage 16:9 → 4:3 ne déforme jamais les widgets, il laisse des bandes.
/// Retourne `true` si les dimensions du canvas ont changé.
pub(crate) fn adapter_canvas(scene: &mut Scene, w: u32, h: u32) -> bool {
    if w == 0 || h == 0 || (scene.canvasW == w && scene.canvasH == h) {
        return false;
    }
    let (courant_w, courant_h) = (scene.canvasW as f64, scene.canvasH as f64);
    let sx = w as f64 / courant_w;
    let sy = h as f64 / courant_h;
    if (sx - 1.0).abs() <= EPSILON_CANVAS && (sy - 1.0).abs() <= EPSILON_CANVAS {
        return false;
    }

    // Espace de conception — migration d'une vieille scène : les coordonnées
    // actuelles sont, par définition, exprimées dans le canvas courant.
    let (design_w, design_h) = match (scene.canvasDesignW, scene.canvasDesignH) {
        (Some(dw), Some(dh)) if dw > 0 && dh > 0 => (dw as f64, dh as f64),
        _ => {
            scene.canvasDesignW = Some(scene.canvasW);
            scene.canvasDesignH = Some(scene.canvasH);
            (courant_w, courant_h)
        }
    };

    // 1) Retour dans l'espace de conception (inverse de `echelle_scene`).
    let (facteur, dx, dy) = transformation(design_w, design_h, courant_w, courant_h);
    echelle_scene(scene, 1.0 / facteur, -dx / facteur, -dy / facteur);

    // 2) Projection sur le nouveau canvas.
    let (facteur, dx, dy) = transformation(design_w, design_h, w as f64, h as f64);
    echelle_scene(scene, facteur, dx, dy);

    scene.canvasW = w;
    scene.canvasH = h;
    true
}

/// Transformation conception → canvas : facteur uniforme + centrage.
///
/// `x_canvas = x_conception × f + dx`, `y_canvas = y_conception × f + dy`.
fn transformation(design_w: f64, design_h: f64, cw: f64, ch: f64) -> (f64, f64, f64) {
    let facteur = (cw / design_w).min(ch / design_h);
    (
        facteur,
        (cw - design_w * facteur) / 2.0,
        (ch - design_h * facteur) / 2.0,
    )
}

/// Applique `facteur` + décalage de centrage à toute la géométrie en px canvas.
///
/// Ne touche qu'aux grandeurs **en pixels canvas** : positions, tailles,
/// décalages, tailles de police, épaisseurs de contour, flou. Les valeurs
/// sans dimension (zoom, rotations en degrés, durées, compteurs) sont
/// laissées intactes.
fn echelle_scene(scene: &mut Scene, facteur: f64, dx: f64, dy: f64) {
    for w in &mut scene.widgets {
        w.x = w.x * facteur + dx;
        w.y = w.y * facteur + dy;
        w.largeur *= facteur;
        w.hauteur *= facteur;
        w.mediaOffsetX *= facteur;
        w.mediaOffsetY *= facteur;
        w.mediaFlou *= facteur;
        w.taillePolice = echelle_px(w.taillePolice, facteur);
        w.titreTaille = echelle_px(w.titreTaille, facteur);
        w.titreContourEpaisseur = echelle_px(w.titreContourEpaisseur, facteur);
        w.titreEspacement = w.titreEspacement.map(|v| v * facteur);
        w.srTaillePolice = echelle_px(w.srTaillePolice, facteur);
        w.srSegBordureEpaisseur = echelle_px(w.srSegBordureEpaisseur, facteur);
        w.srSegBordureRayon = echelle_px(w.srSegBordureRayon, facteur);
        w.srHauteurLigne = echelle_px(w.srHauteurLigne, facteur);
    }

    if let Some(modules) = scene.modules.as_mut() {
        for module in [
            modules.horloge.as_mut(),
            modules.musique.as_mut(),
            modules.objectifs.as_mut(),
            modules.equaliseur.as_mut(),
            modules.spots.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            module.x = module.x * facteur + dx;
            module.y = module.y * facteur + dy;
            module.largeur *= facteur;
            module.hauteur *= facteur;
            module.taille = echelle_px(module.taille, facteur);
        }
    }

    scene.bgOffsetX *= facteur;
    scene.bgOffsetY *= facteur;
    scene.bgFlou *= facteur;
    scene.bgTitreTaille = echelle_px(scene.bgTitreTaille, facteur);
    scene.bgTitreContourEpaisseur = echelle_px(scene.bgTitreContourEpaisseur, facteur);
    scene.bgTitreEspacement = scene.bgTitreEspacement.map(|v| v * facteur);
}

/// Échelle d'une valeur entière en px. `0` reste `0` (contour désactivé,
/// taille absente) : jamais de « 0 devient 1 ».
fn echelle_px(valeur: Option<u32>, facteur: f64) -> Option<u32> {
    valeur.map(|v| {
        if v == 0 {
            0
        } else {
            ((v as f64 * facteur).round() as u32).max(1)
        }
    })
}

// ===== Boot + migration =====

/// Génère un id court (8 chars hex).
fn nouvel_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// Nom de la scène auto-créée au 1er boot — suit la langue UI (lire_langue).
/// FR = source de vérité ("Défaut") ; installateur setup-base0007 écrit
/// langue.json "en" avant le 1er run → "Default" chez les utilisateurs EN.
fn nom_scene_defaut(app: &AppHandle) -> String {
    match lire_langue(app).as_deref() {
        Some("en") => "Default",
        Some("es") => "Predeterminada",
        Some("it") => "Predefinita",
        Some("zh") => "默认",
        _ => "Défaut",
    }
    .to_string()
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
        let mut scene = load_scene_file(app, &current_id).unwrap_or_else(|e| {
            log::warn!("Scène {} illisible ({}): scène vide", current_id, e);
            Scene::new()
        });
        appliquer_canvas_obs(app, &mut scene);
        // Re-sauver config.json au cas où sceneId était absent/invalide
        let _ = save_current_id_config(app, &current_id);
        return Ok((scene, current_id));
    }

    // Pas d'index → migration depuis ancien config.json (full Scene) ou scène vide
    let (mut scene, nom) = migrer_ancien_config(app)?;
    appliquer_canvas_obs(app, &mut scene);
    let id = nouvel_id();
    save_scene_file(app, &id, &scene)?;
    save_index(app, &[SceneIndex { id: id.clone(), nom: nom.clone(), nomMasque: false }])?;
    save_current_id_config(app, &id)?;
    log::info!("Migration scènes : scène « {} » créée (id={})", nom, id);
    Ok((scene, id))
}

/// Lit l'ancien config.json (full Scene) pour migration. Retourne (Scene, nom).
/// Si ancien config.json absent ou non-Scene → Scene::new() + "Défaut".
fn migrer_ancien_config(app: &AppHandle) -> Result<(Scene, String), String> {
    let path = config_path(app)?;
    if !path.exists() {
        return Ok((Scene::new(), nom_scene_defaut(app)));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Erreur lecture ancien config.json: {}", e))?;
    // Tente de parser comme Scene (ancien format)
    match serde_json::from_str::<Scene>(&content) {
        Ok(mut scene) => {
            // Migration cadre unifié (même règle que load_scene_file).
            scene.migrer_cadres();
            // Migration titres multi-positions (même règle que load_scene_file).
            scene.migrer_titres();
            let nom = nom_scene_defaut(app);
            Ok((scene, nom))
        }
        Err(_) => {
            // Nouveau format { sceneId } sans index → scène vide
            log::warn!("Ancien config.json non-Scene et index absent → scène vide");
            Ok((Scene::new(), nom_scene_defaut(app)))
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
    let mut scene = Scene::new();
    appliquer_canvas_obs(app, &mut scene);
    creer_avec(app, state, nom, scene)
}

/// Crée une nouvelle scène depuis une scène fournie (preset embarqué — ids
/// déjà régénérés + canvas OBS déjà appliqués par presets::instancier),
/// bascule dessus. Sauve la courante d'abord. Retourne l'entrée créée.
/// L'existante n'est jamais touchée.
pub fn creer_avec(
    app: &AppHandle,
    state: &ScenesState,
    nom: &str,
    scene: Scene,
) -> Result<SceneIndex, String> {
    // 1. Sauver la courante
    save_current(app, state)?;

    // 2. Id unique
    let id = nouvel_id();
    let entry = SceneIndex { id: id.clone(), nom: nom.to_string(), nomMasque: false };

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
    purge_cadres_chat(state);
    push_snapshot(state);

    log::info!("Scène créée : « {} » (id={})", nom, id);
    Ok(entry)
}

/// Duplique une scène : charge <id>.json, régénère les uuid des widgets
/// (politique presets::instancier — deux scènes ne partagent jamais leurs
/// ids), CONSERVE obsSource (la source OBS est partagée entre les deux
/// scènes, comme « SOS-Caméra » — kind/cible de capture non stockés dans le
/// JSON de scène, la source ne peut pas être recréée), applique le canvas
/// OBS, puis crée la copie via creer_avec (sauvegarde courante + fin de
/// liste + bascule RAM + snapshot).
pub fn dupliquer(
    app: &AppHandle,
    state: &ScenesState,
    id: &str,
    nom: &str,
) -> Result<SceneIndex, String> {
    let mut scene = load_scene_file(app, id)?;
    for w in &mut scene.widgets {
        w.id = uuid::Uuid::new_v4().to_string();
    }
    appliquer_canvas_obs(app, &mut scene);
    let entry = creer_avec(app, state, nom, scene)?;
    log::info!("Scène dupliquée : {} → « {} » (id={})", id, nom, entry.id);
    Ok(entry)
}

/// ONBOARDING : initialise la scène de départ depuis un preset.
///
/// ⚠️ GARDE DE SÉCURITÉ « scène vierge » (2026-09-20) : le remplacement +
/// renommage n'a lieu QUE si la scène courante est provably vide (aucun
/// widget, fond, cadre, titre, morph). Sinon — utilisateur EXISTANT qui voit
/// l'onboarding au premier boot après mise à jour (flag onboarding.json
/// absent) avec une scène en cours — la commande NE TOUCHE À RIEN et crée
/// une NOUVELLE scène (même comportement que le bouton « + Scène »).
/// L'onboarding ne peut ainsi jamais détruire du travail.
pub fn initialiser_depuis_preset(
    app: &AppHandle,
    state: &ScenesState,
    preset: &crate::presets::PresetFichier,
) -> Result<SceneIndex, String> {
    // 0. Garde : scène courante non vierge → NOUVELLE scène, rien n'est touché.
    let courante_vierge = {
        let s = state.scene.lock().unwrap();
        scene_vierge(&s)
    };
    let scene = crate::presets::instancier(app, preset);
    if !courante_vierge {
        log::info!(
            "Onboarding preset « {} » : scène courante non vierge → nouvelle scène (rien n'est écrasé)",
            preset.nom
        );
        return creer_avec(app, state, &preset.nom, scene);
    }

    // 1. Remplacer la scène courante VIERGE en RAM (ordre des locks : scene
    //    PUIS current_id — même convention que push_snapshot/ouvrir).
    let current_id = {
        let mut s = state.scene.lock().unwrap();
        *s = scene;
        state.current_id.lock().unwrap().clone()
    };
    purge_cadres_chat(state);

    // 2. Persister le contenu + renommer l'entrée d'index au nom du preset
    save_current(app, state)?;
    renommer(app, &current_id, &preset.nom)?;

    // 3. Snapshot + retour
    push_snapshot(state);
    log::info!(
        "Scène de départ initialisée depuis le preset « {} » (id={})",
        preset.nom,
        current_id
    );
    Ok(SceneIndex {
        id: current_id,
        nom: preset.nom.clone(),
        nomMasque: false,
    })
}

/// Une scène est « vierge » si elle ne contient AUCUN contenu utilisateur :
/// pas de widget, pas de média de fond, pas de cadre AUTRE QUE le défaut
/// auto posé à la naissance par Scene::new (cadreWidget ==
/// CadreConfig::defaut_scene() compte comme « pas de cadre choisi » — sinon
/// l'onboarding créerait une 2e scène au lieu de remplacer la vierge), pas
/// de titre de fond (4 positions + legacy), pas de morphs de fond, pas de
/// modules HUD. Les dims canvas ne comptent pas (miroir de la résolution
/// OBS). Garde de initialiser_depuis_preset.
fn scene_vierge(s: &Scene) -> bool {
    s.widgets.is_empty()
        && s.modules.is_none()
        && s.bgMedia.is_empty()
        && s.cadreWidget
            .as_ref()
            .map_or(true, |c| *c == CadreConfig::defaut_scene())
        && s.cadreApp.is_none()
        && s.bgTitreHaut.is_none()
        && s.bgTitreBas.is_none()
        && s.bgTitreGauche.is_none()
        && s.bgTitreDroite.is_none()
        && s.bgTitre.is_none()
        && s.bgMorphs.as_ref().map_or(true, |m| m.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La garde « scène vierge » doit détecter TOUT contenu utilisateur —
    /// c'est elle qui empêche l'onboarding d'écraser une scène existante.
    /// Widget à (x, y) = centre, comme `clampWidget` côté frontend.
    fn scene_avec_widget(x: f64, y: f64, largeur: f64, hauteur: f64) -> Scene {
        let json = format!(
            r#"{{"widgets":[{{"id":"w","type":"media","x":{x},"y":{y},
                "largeur":{largeur},"hauteur":{hauteur},"z":0,
                "titreTaille":48,"titreContourEpaisseur":0,"mediaOffsetX":20}}]}}"#
        );
        serde_json::from_str(&json).expect("scène JSON")
    }

    #[test]
    fn adapter_canvas_rescale_1920_vers_1360() {
        let mut s = scene_avec_widget(960.0, 540.0, 400.0, 200.0);
        assert!(adapter_canvas(&mut s, 1360, 768));
        assert_eq!((s.canvasW, s.canvasH), (1360, 768));
        // Facteur uniforme = min(1360/1920, 768/1080) = 0.70833 ; le contenu
        // (1920×1080 × f = 1360×765) est centré → dy = 1.5 px.
        let f = 1360.0 / 1920.0;
        let dy = (768.0 - 1080.0 * f) / 2.0;
        let w = &s.widgets[0];
        assert!((w.x - 960.0 * f).abs() < 0.01, "x = {}", w.x);
        assert!((w.y - (540.0 * f + dy)).abs() < 0.01, "y = {}", w.y);
        assert!((w.largeur - 400.0 * f).abs() < 0.01);
        assert!((w.hauteur - 200.0 * f).abs() < 0.01);
        assert!((w.mediaOffsetX - 20.0 * f).abs() < 0.01);
        assert_eq!(w.titreTaille, Some(34), "48 × 0.708 = 34");
        assert_eq!(w.titreContourEpaisseur, Some(0), "0 = contour désactivé");
        // L'espace de conception est mémorisé : la scène a été dessinée en
        // 1920×1080, même si elle s'affiche maintenant en 1360×768.
        assert_eq!((s.canvasDesignW, s.canvasDesignH), (Some(1920), Some(1080)));
    }

    #[test]
    fn adapter_canvas_ignore_le_bruit_obs() {
        // 1919×1080 = 0,05 % d'écart : bruit d'arrondi, rien n'est touché —
        // ni les dimensions (sinon la scène serait re-sauvée pour 1 px), ni
        // la géométrie.
        let mut s = scene_avec_widget(960.0, 540.0, 400.0, 200.0);
        assert!(!adapter_canvas(&mut s, 1919, 1080));
        assert_eq!((s.canvasW, s.canvasH), (1920, 1080));
        assert_eq!(s.widgets[0].x, 960.0);
        assert_eq!(s.canvasDesignW, None, "aucune migration pour du bruit");
    }

    #[test]
    fn adapter_canvas_aller_retour_exact() {
        let mut s = scene_avec_widget(960.0, 540.0, 400.0, 200.0);
        let (x0, y0, l0, h0) = (
            s.widgets[0].x,
            s.widgets[0].y,
            s.widgets[0].largeur,
            s.widgets[0].hauteur,
        );
        assert!(adapter_canvas(&mut s, 1360, 768));
        assert!(adapter_canvas(&mut s, 1920, 1080));
        let w = &s.widgets[0];
        assert_eq!((s.canvasW, s.canvasH), (1920, 1080));
        assert!((w.x - x0).abs() < 1e-9, "x {} ≠ {}", w.x, x0);
        assert!((w.y - y0).abs() < 1e-9, "y {} ≠ {}", w.y, y0);
        assert!((w.largeur - l0).abs() < 1e-9);
        assert!((w.hauteur - h0).abs() < 1e-9);
        assert_eq!(w.titreTaille, Some(48), "champ entier restauré");
    }

    #[test]
    fn adapter_canvas_aller_retour_exact_4_3() {
        let mut s = scene_avec_widget(960.0, 540.0, 400.0, 200.0);
        assert!(adapter_canvas(&mut s, 1440, 1080));
        assert!(adapter_canvas(&mut s, 1920, 1080));
        let w = &s.widgets[0];
        assert!((w.x - 960.0).abs() < 1e-9);
        assert!((w.largeur - 400.0).abs() < 1e-9);
        assert!((w.hauteur - 200.0).abs() < 1e-9);
    }

    #[test]
    fn adapter_canvas_garde_l_espace_de_conception() {
        let mut s = scene_avec_widget(960.0, 540.0, 400.0, 200.0);
        assert!(adapter_canvas(&mut s, 1360, 768));
        assert!(adapter_canvas(&mut s, 2560, 1440));
        assert_eq!((s.canvasW, s.canvasH), (2560, 1440));
        assert_eq!(
            (s.canvasDesignW, s.canvasDesignH),
            (Some(1920), Some(1080)),
            "la conception ne bouge jamais"
        );
        let w = &s.widgets[0];
        let f = 2560.0 / 1920.0;
        assert!((w.x - 960.0 * f).abs() < 0.01, "projeté depuis 1920×1080");
    }

    #[test]
    fn adapter_canvas_est_idempotent() {
        let mut s = scene_avec_widget(960.0, 540.0, 400.0, 200.0);
        assert!(adapter_canvas(&mut s, 1360, 768));
        let (x, largeur) = (s.widgets[0].x, s.widgets[0].largeur);
        assert!(!adapter_canvas(&mut s, 1360, 768), "même dims = no-op");
        assert_eq!(s.widgets[0].x, x);
        assert_eq!(s.widgets[0].largeur, largeur);
    }

    #[test]
    fn adapter_canvas_letterbox_si_le_ratio_change() {
        // 1920×1080 → 1440×1080 (4:3, même hauteur) : facteur uniforme 0.75,
        // contenu 1440×810 centré verticalement → bandes de 135 px en haut et
        // en bas, aucune déformation.
        let mut s = scene_avec_widget(960.0, 540.0, 400.0, 200.0);
        assert!(adapter_canvas(&mut s, 1440, 1080));
        let w = &s.widgets[0];
        assert!((w.largeur - 300.0).abs() < 0.01, "largeur × 0.75");
        assert!((w.hauteur - 150.0).abs() < 0.01, "hauteur × 0.75");
        assert!((w.x - 720.0).abs() < 0.01, "pas de bande horizontale");
        assert!((w.y - 540.0).abs() < 0.01, "540 × 0.75 + 135 = 540");
    }

    #[test]
    fn adapter_canvas_refuse_des_dims_nulles() {
        let mut s = scene_avec_widget(960.0, 540.0, 400.0, 200.0);
        assert!(!adapter_canvas(&mut s, 0, 768));
        assert_eq!((s.canvasW, s.canvasH), (1920, 1080));
    }

    #[test]
    fn adapter_canvas_rescale_les_modules() {
        let mut s: Scene = serde_json::from_str(
            r#"{"widgets":[],"modules":{"horloge":{"x":1520,"y":40,"largeur":360,"hauteur":60,"taille":30}}}"#,
        )
        .expect("scène JSON");
        assert!(adapter_canvas(&mut s, 1360, 768));
        let h = s.modules.as_ref().and_then(|m| m.horloge.as_ref()).unwrap();
        let f = 1360.0 / 1920.0;
        assert!((h.x - 1520.0 * f).abs() < 0.01);
        assert!((h.largeur - 360.0 * f).abs() < 0.01);
        assert_eq!(h.taille, Some(21));
    }

    #[test]
    fn scene_vierge_detecte_contenu() {
        let s = Scene::new();
        assert!(scene_vierge(&s), "scène neuve = vierge");

        let mut s = Scene::new();
        s.bgMedia = "medias/x.png".to_string();
        assert!(!scene_vierge(&s), "fond média = pas vierge");

        let mut s = Scene::new();
        s.cadreWidget = Some(crate::scene::CadreConfig::default());
        assert!(!scene_vierge(&s), "cadre widget = pas vierge");

        let mut s = Scene::new();
        s.bgTitre = Some("Titre".to_string());
        assert!(!scene_vierge(&s), "titre de fond legacy = pas vierge");

        let mut s = Scene::new();
        s.bgTitreHaut = Some("Titre".to_string());
        assert!(!scene_vierge(&s), "titre de fond haut = pas vierge");

        let mut s = Scene::new();
        s.bgTitreDroite = Some("Titre".to_string());
        assert!(!scene_vierge(&s), "titre de fond droite = pas vierge");

        let mut s = Scene::new();
        s.bgMorphs = Some(vec![crate::scene::MorphPoint::default()]);
        assert!(!scene_vierge(&s), "morph de fond = pas vierge");

        let mut s = Scene::new();
        s.modules = Some(crate::scene::ScenesModules::default());
        assert!(!scene_vierge(&s), "modules HUD = pas vierge");
    }

    /// La racine d'une sauvegarde zip est repérée par le suffixe
    /// « scenes/index.json » : avec dossier racine (notre format),
    /// sans racine (vieux zips), et rejet d'une archive sans index.
    #[test]
    fn detecter_racine_zip_variantes() {
        use crate::zip_store::{ZipEcrivain, ZipLecteur};
        use std::io::BufWriter;

        let faire_zip = |noms: &[&str]| -> PathBuf {
            let path = std::env::temp_dir()
                .join(format!("sos_racine_{}.zip", uuid::Uuid::new_v4().simple()));
            let mut zip = ZipEcrivain::new(BufWriter::new(fs::File::create(&path).unwrap()));
            for n in noms {
                zip.ajouter_octets(n, b"[]").unwrap();
            }
            zip.terminer().unwrap();
            path
        };

        let p = faire_zip(&["ma-sauvegarde/scenes/index.json", "ma-sauvegarde/scenes/a1.json"]);
        let l = ZipLecteur::ouvrir(&p).unwrap();
        assert_eq!(detecter_racine_zip(&l).unwrap(), "ma-sauvegarde/");
        let _ = fs::remove_file(&p);

        let p = faire_zip(&["scenes/index.json", "medias/x.png"]);
        let l = ZipLecteur::ouvrir(&p).unwrap();
        assert_eq!(detecter_racine_zip(&l).unwrap(), "");
        let _ = fs::remove_file(&p);

        let p = faire_zip(&["readme.txt"]);
        let l = ZipLecteur::ouvrir(&p).unwrap();
        assert!(detecter_racine_zip(&l).is_err());
        let _ = fs::remove_file(&p);
    }

    /// Migration titres multi-positions : legacy `titre`+`titrePosition` →
    /// `titreHaut`/`titreBas` (idem fond), champs legacy vidés, nouveau
    /// champ déjà présent gagne.
    #[test]
    fn migrer_titres_migre_legacy() {
        let scene_json = serde_json::json!({
            "widgets": [
                { "id": "w1", "type": "media", "x": 0, "y": 0, "largeur": 10,
                  "hauteur": 10, "z": 1, "titre": "Au-dessus", "titrePosition": "dessus" },
                { "id": "w2", "type": "media", "x": 0, "y": 0, "largeur": 10,
                  "hauteur": 10, "z": 1, "titre": "En dessous", "titrePosition": "dessous" },
                { "id": "w3", "type": "media", "x": 0, "y": 0, "largeur": 10,
                  "hauteur": 10, "z": 1, "titre": "Ignoré", "titrePosition": "dessus",
                  "titreHaut": "Déjà pris" }
            ],
            "bgTitre": "Fond", "bgTitrePosition": "bas"
        });
        let mut scene: Scene = serde_json::from_value(scene_json).unwrap();
        scene.migrer_titres();

        let w1 = &scene.widgets[0];
        assert_eq!(w1.titreHaut.as_deref(), Some("Au-dessus"), "dessus → titreHaut");
        assert!(w1.titreBas.is_none());
        let w2 = &scene.widgets[1];
        assert_eq!(w2.titreBas.as_deref(), Some("En dessous"), "dessous → titreBas");
        assert!(w2.titreHaut.is_none());
        let w3 = &scene.widgets[2];
        assert_eq!(w3.titreHaut.as_deref(), Some("Déjà pris"), "nouveau champ gagne");
        for w in &scene.widgets {
            assert!(w.titre.is_none(), "titre legacy vidé");
            assert!(w.titrePosition.is_none(), "titrePosition legacy vidé");
        }
        assert_eq!(scene.bgTitreBas.as_deref(), Some("Fond"), "bgTitre bas → bgTitreBas");
        assert!(scene.bgTitreHaut.is_none());
        assert!(scene.bgTitre.is_none(), "bgTitre legacy vidé");
        assert!(scene.bgTitrePosition.is_none(), "bgTitrePosition legacy vidé");
    }
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

    // 2. Charger la nouvelle scène (canvas = résolution OBS connue)
    let mut scene = load_scene_file(app, id)?;
    appliquer_canvas_obs(app, &mut scene);

    // 3. Bascule RAM + config + snapshot
    {
        let mut s = state.scene.lock().unwrap();
        *s = scene;
        let mut cid = state.current_id.lock().unwrap();
        *cid = id.to_string();
    }
    save_current_id_config(app, id)?;
    purge_cadres_chat(state);
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

/// Déplace une scène dans l'index (glisser-déposer de la barre « Vos scènes »).
/// `position` = index cible APRÈS retrait de l'entrée (borné à la fin).
pub fn deplacer(app: &AppHandle, id: &str, position: usize) -> Result<(), String> {
    let mut idx = load_index(app)?;
    let pos = idx.iter().position(|s| s.id == id)
        .ok_or_else(|| format!("Scène {} introuvable dans l'index", id))?;
    let entry = idx.remove(pos);
    let insert_at = position.min(idx.len());
    idx.insert(insert_at, entry);
    save_index(app, &idx)?;
    log::info!("Scène {} déplacée à la position {}", id, insert_at);
    Ok(())
}

/// Marque/démarque une scène « masquée » dans sa pastille de la barre
/// « Vos scènes » : titre orange + confirmation avant ouverture.
pub fn masquer_nom(app: &AppHandle, id: &str, masque: bool) -> Result<(), String> {
    let mut idx = load_index(app)?;
    let entry = idx.iter_mut().find(|s| s.id == id)
        .ok_or_else(|| format!("Scène {} introuvable dans l'index", id))?;
    entry.nomMasque = masque;
    save_index(app, &idx)?;
    log::info!("Scène {} : titre {} dans la barre", id, if masque { "masqué" } else { "affiché" });
    Ok(())
}

// ===== Résumé des scènes (tableau « Vos scènes ») =====

/// Résumé d'une scène pour le tableau « Vos scènes » : nb widgets, types
/// présents, modules HUD configurés, présence d'un fond, dims du canvas et
/// mini-rects des widgets — alimente la mini-map procédurale (aperçu de
/// layout façon OBS, pas de capture d'écran). Relu à chaque ouverture du
/// tableau : la vérité disque, zéro cache.
/// NB : champs camelCase VOLONTAIRES — contrat JSON direct avec le frontend
/// (même convention que SceneIndex/scene.rs). D'où le allow(non_snake_case).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize)]
pub struct SceneResume {
    pub id: String,
    /// false = <id>.json absent ou illisible → résumé vide (la pastille
    /// reste affichée via l'index).
    pub ok: bool,
    pub nbWidgets: usize,
    /// Types de widgets dédupliqués (ordre alphabétique).
    pub types: Vec<String>,
    pub nbTrous: usize,
    /// Modules HUD configurés : "horloge" | "musique" | "objectifs" | "equaliseur".
    pub modules: Vec<String>,
    /// Un fond est affiché : média, carrousel d'images ou source OBS liée.
    pub aFond: bool,
    pub canvasW: u32,
    pub canvasH: u32,
    pub widgets: Vec<WidgetResume>,
}

/// Rect d'un widget pour la mini-map : position/taille en pixels du canvas
/// de la scène (le frontend ramène en % via canvasW/canvasH), z pour
/// l'ordre d'empilement, trou = fenêtre de capture OBS (contour pointillé).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize)]
pub struct WidgetResume {
    #[serde(rename = "type")]
    pub widget_type: String,
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
    pub z: i32,
    pub trou: bool,
}

/// Lit chaque <id>.json de l'index et agrège son résumé. Fichiers petits
/// (~Ko) → coût négligeable même à chaque ouverture du tableau. Une scène
/// illisible → ok:false, le reste de l'index n'est pas affecté.
pub fn resumer(app: &AppHandle) -> Result<Vec<SceneResume>, String> {
    let idx = load_index(app)?;
    let mut out = Vec::with_capacity(idx.len());
    for entry in idx {
        let mut resume = SceneResume {
            id: entry.id.clone(),
            ok: false,
            nbWidgets: 0,
            types: Vec::new(),
            nbTrous: 0,
            modules: Vec::new(),
            aFond: false,
            canvasW: 1920,
            canvasH: 1080,
            widgets: Vec::new(),
        };
        if let Ok(scene) = load_scene_file(app, &entry.id) {
            resume.ok = true;
            resume.nbWidgets = scene.widgets.len();
            resume.nbTrous = scene.widgets.iter().filter(|w| w.trou).count();
            resume.aFond = !scene.bgMedia.is_empty()
                || scene.bgObsSource.is_some()
                || scene.bgSlides.as_ref().is_some_and(|s| !s.is_empty());
            resume.canvasW = scene.canvasW;
            resume.canvasH = scene.canvasH;
            let mut types: Vec<String> =
                scene.widgets.iter().map(|w| w.widget_type.clone()).collect();
            types.sort();
            types.dedup();
            resume.types = types;
            if let Some(m) = &scene.modules {
                if m.horloge.is_some() {
                    resume.modules.push("horloge".into());
                }
                if m.musique.is_some() {
                    resume.modules.push("musique".into());
                }
                if m.objectifs.is_some() {
                    resume.modules.push("objectifs".into());
                }
                if m.equaliseur.is_some() {
                    resume.modules.push("equaliseur".into());
                }
                if m.spots.is_some() {
                    resume.modules.push("spots".into());
                }
            }
            resume.widgets = scene
                .widgets
                .iter()
                .map(|w| WidgetResume {
                    widget_type: w.widget_type.clone(),
                    x: w.x,
                    y: w.y,
                    largeur: w.largeur,
                    hauteur: w.hauteur,
                    z: w.z,
                    trou: w.trou,
                })
                .collect();
        }
        out.push(resume);
    }
    Ok(out)
}

/// Supprime une scène : retire <id>.json + entrée index. Si la scène supprimée
/// est la courante, bascule sur la 1ère scène restante (ou scène vide si plus
/// aucune). Refuse si c'est la dernière scène (au moins 1 obligatoire).
pub fn supprimer(app: &AppHandle, state: &ScenesState, id: &str) -> Result<(), String> {
    let mut idx = load_index(app)?;
    if idx.len() <= 1 {
        return Err("Impossible de supprimer la dernière scène".into());
    }
    let pos = idx.iter().position(|s| s.id == id)
        .ok_or_else(|| format!("Scène {} introuvable dans l'index", id))?;

    // 1. Supprimer <id>.json du disque (non-fatal si absent)
    if let Ok(path) = scene_file_path(app, id) {
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }

    // 2. Retirer l'entrée de l'index + sauver
    let entry = idx.remove(pos);
    save_index(app, &idx)?;

    // 3. Si la scène supprimée est la courante → basculer sur la 1ère restante
    let current_id = state.current_id.lock().unwrap().clone();
    if current_id == id {
        let new_id = idx[0].id.clone();
        let mut scene = load_scene_file(app, &new_id).unwrap_or_else(|_| Scene::new());
        appliquer_canvas_obs(app, &mut scene);
        {
            let mut s = state.scene.lock().unwrap();
            *s = scene;
            let mut cid = state.current_id.lock().unwrap();
            *cid = new_id.clone();
        }
        save_current_id_config(app, &new_id)?;
        purge_cadres_chat(state);
        push_snapshot(state);
    }

    log::info!("Scène supprimée : « {} » (id={})", entry.nom, id);
    Ok(())
}

/// Retourne l'entrée courante ({ id, nom }).
pub fn courante(app: &AppHandle, state: &ScenesState) -> Result<SceneIndex, String> {
    let id = state.current_id.lock().unwrap().clone();
    let idx = load_index(app)?;
    let entry = trouver_entree(&idx, &id).cloned()
        .unwrap_or_else(|| SceneIndex { id: id.clone(), nom: nom_scene_defaut(app), nomMasque: false });
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
///
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
    // Migration cadre unifié (même règle que load_scene_file).
    scene.migrer_cadres();
    // Migration titres multi-positions (même règle que load_scene_file).
    scene.migrer_titres();
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

    // 5bis. Canvas = résolution OBS connue (le pack peut venir d'une machine
    //       avec une autre résolution — les widgets ne sont pas rescalés).
    appliquer_canvas_obs(app, &mut scene);

    // 6. Sauver la courante d'abord
    save_current(app, state)?;

    // 7. Nouvel id + écriture <id>.json + index += + config
    let id = nouvel_id();
    save_scene_file(app, &id, &scene)?;
    let entry = SceneIndex { id: id.clone(), nom: nom.clone(), nomMasque: false };
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
    purge_cadres_chat(state);
    push_snapshot(state);

    log::info!(
        "Scène importée : « {} » (id={}, {} médias copiés, {} manquants)",
        nom, id, copies, manquants
    );
    Ok(Some(entry))
}

// ===== Export « tout en .zip » (toutes les scènes + tous les médias) =====

/// Exporte TOUTES les scènes + TOUS les médias dans un seul .zip.
///
/// Structure de l'archive — UN dossier racine = UN nom de sauvegarde
/// (= le nom du fichier choisi, sans .zip) :
/// ```text
/// <sauvegarde>/
/// ├── scenes/index.json + scenes/<id>.json   (miroir brut — restauré tel quel
/// │                                           par l'import « Archive .zip »)
/// ├── medias/…                               (TOUS les médias, récursif —
/// │                                           musique/ incluse, référencés ou non)
/// └── packs/<nom-scène>/scene.json + medias/ (un pack par scène, format de
///                                             l'export unitaire → réimportable
///                                             via Importer après dézip)
/// ```
///
/// Dialog « Enregistrer sous » : dossier + nom de fichier choisis par
/// l'utilisateur ; un .zip existant est écrasé (« si les scènes existent
/// déjà dans le dossier, on les remplace »).
/// La scène courante est sauvée d'abord (le miroir reflète l'état RAM).
/// Un fichier illisible est sauté (log warn) ; une erreur en cours
/// d'écriture est fatale — le .zip partiel est supprimé (jamais d'archive
/// corrompue laissée sur disque).
/// Retourne Some(chemin du zip) si exporté, None si dialog annulé.
pub fn exporter_tout_zip(app: &AppHandle, state: &ScenesState) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    // 0. Sauver la courante d'abord : le miroir disque reflète l'état RAM.
    save_current(app, state)?;

    // 1. Dialog « Enregistrer sous » (filtre .zip + nom par défaut daté).
    let nom_defaut = format!(
        "streamos-scenes-{}.zip",
        chrono::Local::now().format("%Y%m%d-%H%M")
    );
    let file = app
        .dialog()
        .file()
        .add_filter("Archive ZIP", &["zip"])
        .set_file_name(&nom_defaut)
        .blocking_save_file();
    let Some(file) = file else {
        return Ok(None); // dialog annulé
    };
    let mut dest: PathBuf = file.into_path().map_err(|e| format!("Chemin invalide: {}", e))?;
    // .zip forcé si l'utilisateur a retiré l'extension dans le dialog.
    let ext_zip = dest
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !ext_zip {
        dest.set_extension("zip");
    }

    // 2. Nom de la sauvegarde = nom du fichier choisi (sans .zip) → il
    //    devient le dossier racine de l'archive (« un dossier = une
    //    sauvegarde »). Défaut si stem illisible : "streamos".
    let racine = dest
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("streamos")
        .to_string();

    // 3. Écriture de l'archive ; sur erreur le .zip partiel est supprimé.
    ecrire_zip_export(app, &dest, &racine).inspect_err(|_| {
        let _ = fs::remove_file(&dest);
    })?;

    Ok(Some(dest.display().to_string()))
}

/// Corps de `exporter_tout_zip` : écrit toute l'archive dans `dest` sous
/// le dossier racine `racine/` (File::create écrase un .zip existant —
/// écrasement voulu).
fn ecrire_zip_export(app: &AppHandle, dest: &std::path::Path, racine: &str) -> Result<(), String> {
    use crate::zip_store::ZipEcrivain;
    use std::io::BufWriter;

    let f = fs::File::create(dest)
        .map_err(|e| format!("Création {}: {}", dest.display(), e))?;
    let mut zip = ZipEcrivain::new(BufWriter::new(f));
    // Préfixe commun de toutes les entrées : « <sauvegarde>/ ».
    let r = format!("{}/", racine);

    // 1. Miroir scenes/ : index.json + <id>.json (l'index est la source de
    //    vérité — il est toujours écrit, même si le fichier disque manquait).
    let idx = load_index(app)?;
    let index_json = serde_json::to_string_pretty(&idx)
        .map_err(|e| format!("Sérialisation index: {}", e))?;
    zip.ajouter_octets(&format!("{}scenes/index.json", r), index_json.as_bytes())
        .map_err(|e| format!("ZIP scenes/index.json: {}", e))?;
    let dir_scenes = scenes_dir(app)?;
    let mut nb_scenes = 0u32;
    for entry in &idx {
        let p = dir_scenes.join(format!("{}.json", entry.id));
        match zip.ajouter_fichier(&format!("{}scenes/{}.json", r, entry.id), &p) {
            Ok(true) => nb_scenes += 1,
            Ok(false) => {
                log::warn!("Export ZIP : scène {} illisible, absente du miroir", entry.id)
            }
            Err(e) => return Err(format!("ZIP scenes/{}.json: {}", entry.id, e)),
        }
    }

    // 2. Miroir medias/ : TOUS les fichiers, récursif (musique/ incluse,
    //    médias référencés ou non).
    let medias_root = data_dir(app)?.join("medias");
    let mut nb_medias = 0u32;
    ajouter_dossier_zip(&mut zip, &medias_root, &format!("{}medias", r), &mut nb_medias)?;

    // 3. Packs par scène : packs/<nom>/scene.json + medias/ — même format
    //    que l'export unitaire (importer() le lit après dézip).
    let mut utilises = std::collections::HashSet::new();
    let mut nb_packs = 0u32;
    let mut medias_manquants = 0u32;
    for entry in &idx {
        let scene = match load_scene_file(app, &entry.id) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Export ZIP : scène {} illisible ({}), pack sauté", entry.id, e);
                continue;
            }
        };
        let nom_pack = nom_pack_unique(&sanitiser_nom_pack(&entry.nom), &mut utilises);
        let base = format!("{}packs/{}", r, nom_pack);
        let mut v = serde_json::to_value(&scene).map_err(|e| format!("Sérialisation: {}", e))?;
        if let Some(obj) = v.as_object_mut() {
            obj.insert("nom".to_string(), serde_json::Value::String(entry.nom.clone()));
        }
        let content =
            serde_json::to_string_pretty(&v).map_err(|e| format!("Sérialisation: {}", e))?;
        zip.ajouter_octets(&format!("{}/scene.json", base), content.as_bytes())
            .map_err(|e| format!("ZIP {}/scene.json: {}", base, e))?;
        let refs = collecter_medias(&scene);
        for rel in &refs {
            let file_name = rel.strip_prefix("medias/").unwrap_or(rel);
            let src = medias_root.join(file_name);
            if !src.exists() {
                medias_manquants += 1;
                log::warn!(
                    "Export ZIP : média manquant ({}), chemin gardé dans {}",
                    rel, base
                );
                continue;
            }
            match zip.ajouter_fichier(&format!("{}/medias/{}", base, file_name), &src) {
                Ok(true) => {}
                Ok(false) => {
                    log::warn!("Export ZIP : média illisible, ignoré : {}", src.display())
                }
                Err(e) => return Err(format!("ZIP {}/medias/{}: {}", base, file_name, e)),
            }
        }
        nb_packs += 1;
    }

    zip.terminer().map_err(|e| format!("Finalisation ZIP: {}", e))?;
    log::info!(
        "Export ZIP terminé : {} ({} scènes, {} médias, {} packs, {} médias pack manquants)",
        dest.display(),
        nb_scenes,
        nb_medias,
        nb_packs,
        medias_manquants
    );
    Ok(())
}

/// Ajoute récursivement tout un dossier au zip sous `prefixe` (séparateur
/// `/` exigé par le format ZIP). Un fichier illisible est sauté (log warn) ;
/// une erreur en cours d'écriture remonte — l'archive serait corrompue.
fn ajouter_dossier_zip<W: std::io::Write>(
    zip: &mut crate::zip_store::ZipEcrivain<W>,
    dir: &std::path::Path,
    prefixe: &str,
    nb: &mut u32,
) -> Result<(), String> {
    if !dir.exists() {
        return Ok(()); // pas de dossier medias/ → rien à ajouter
    }
    let rd = fs::read_dir(dir).map_err(|e| format!("Lecture {}: {}", dir.display(), e))?;
    for e in rd.flatten() {
        let p = e.path();
        let nom = e.file_name().to_string_lossy().into_owned();
        let chemin_zip = format!("{}/{}", prefixe, nom);
        if p.is_dir() {
            ajouter_dossier_zip(zip, &p, &chemin_zip, nb)?;
        } else if p.is_file() {
            match zip.ajouter_fichier(&chemin_zip, &p) {
                Ok(true) => *nb += 1,
                Ok(false) => {
                    log::warn!("Export ZIP : fichier illisible, ignoré : {}", p.display())
                }
                Err(e) => return Err(format!("ZIP {}: {}", chemin_zip, e)),
            }
        }
    }
    Ok(())
}

/// Sanitize un nom de scène en nom de dossier de pack : caractères interdits
/// Windows remplacés par '-', espaces/points de bord retirés (trailing
/// dot/space = nom invalide Windows), vide → "scene". UTF-8 conservé —
/// le flag d'entrée zip est posé dans zip_store.
fn sanitiser_nom_pack(nom: &str) -> String {
    let s: String = nom
        .chars()
        .map(|c| match c {
            '/' | '\\' | '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let s = s.trim().trim_end_matches(['.', ' ']);
    if s.is_empty() {
        "scene".to_string()
    } else {
        s.to_string()
    }
}

/// Déduplique un nom de pack (deux scènes peuvent porter le même nom) :
/// « nom », puis « nom-2 », « nom-3 »…
fn nom_pack_unique(base: &str, utilises: &mut std::collections::HashSet<String>) -> String {
    let mut candidat = base.to_string();
    let mut n = 2u32;
    while !utilises.insert(candidat.clone()) {
        candidat = format!("{}-{}", base, n);
        n += 1;
    }
    candidat
}

// ===== Import « tout en .zip » (restauration complète d'une sauvegarde) =====
//
// Protocole en 2 phases — même recette que l'import de commandes
// (commandes.rs) : la phase 1 ANALYSE sans rien écrire et remonte les
// conflits ; le frontend demande « remplacer ou passer ? » pour chacun,
// puis la phase 2 APPLIQUE avec la liste des ids acceptés.
//
// Compatibilité « pour tout le monde » : la racine de la sauvegarde est
// repérée par le suffixe « scenes/index.json » — un zip extrait puis
// re-zippé à la main dans un sous-dossier (ou notre format racine
// « <sauvegarde>/ », ou un vieux zip sans racine) est lu pareil.
// Scènes absentes localement → importées direct. Id déjà présent →
// tranché par l'UI. Scènes locales absentes du zip → conservées (fusion,
// jamais de réinitialisation). Médias <racine>medias/** restaurés en
// écrasant les fichiers du même nom (les noms étant des uuid, un même
// nom = le même fichier).

/// Conflit détecté par l'analyse : l'id existe déjà dans l'index local.
/// `nomLocal`/`nomArchive` permettent à l'UI de nommer la scène dans la
/// question « remplacer ou passer ? ».
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize)]
pub struct ConflitZip {
    pub id: String,
    pub nomArchive: String,
    pub nomLocal: String,
}

/// Résultat de la phase 1 d'import .zip (dialog + analyse — AUCUNE
/// écriture à ce stade). camelCase = contrat JSON direct frontend.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize)]
pub struct AnalyseZipImport {
    /// Chemin du zip choisi — repassé tel quel à l'étape « appliquer ».
    pub zipPath: String,
    /// Scènes déclarées dans l'archive (fichier <id>.json présent).
    pub scenes: Vec<SceneIndex>,
    /// Ids déjà présents localement → conflits à trancher par l'UI.
    pub conflits: Vec<ConflitZip>,
}

/// Bilan retourné par appliquer_zip_import (affiche dans l'alerte succès).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize)]
pub struct BilanImportZip {
    /// Scènes écrites (nouvelles + remplacées).
    pub scenesImportees: u32,
    /// Fichiers médias restaurés sous medias/.
    pub mediasRestaures: u32,
    /// Scènes du zip passées (conflit décliné ou fichier illisible).
    pub scenesSautees: u32,
}

/// Repère la racine de la sauvegarde dans l'archive : le préfixe commun
/// devant « scenes/index.json ». « <r>/scenes/index.json » → « <r>/ » ;
/// « scenes/index.json » (vieux zips sans racine) → « ». Le préfixe le
/// plus court gagne si plusieurs candidats (zip avec des fichiers en plus).
/// Erreur si aucun index — l'archive n'est pas une sauvegarde StreamOS.
fn detecter_racine_zip(lecteur: &crate::zip_store::ZipLecteur) -> Result<String, String> {
    let mut candidats: Vec<String> = lecteur
        .entrees()
        .filter_map(|e| e.strip_suffix("scenes/index.json").map(str::to_string))
        .collect();
    candidats.sort_by_key(|c| c.len());
    candidats.into_iter().next().ok_or_else(|| {
        "Archive invalide : aucune sauvegarde (scenes/index.json introuvable)".to_string()
    })
}

/// Ouvre le zip, repère la racine et lit l'index des scènes qu'il déclare.
/// Commun aux deux phases (l'archive est re-ouverte en phase 2 — coût
/// négligeable, le central directory fait quelques Ko).
fn lire_index_zip(
    zip_path: &std::path::Path,
) -> Result<(crate::zip_store::ZipLecteur, String, Vec<SceneIndex>), String> {
    let mut lecteur = crate::zip_store::ZipLecteur::ouvrir(zip_path)
        .map_err(|e| format!("Lecture du .zip : {}", e))?;
    let racine = detecter_racine_zip(&lecteur)?;
    let brut = lecteur
        .lire_bytes(&format!("{}scenes/index.json", racine))
        .map_err(|e| format!("Lecture scenes/index.json dans le .zip : {}", e))?;
    let idx: Vec<SceneIndex> = serde_json::from_slice(&brut)
        .map_err(|e| format!("scenes/index.json invalide dans le .zip : {}", e))?;
    Ok((lecteur, racine, idx))
}

/// Phase 1 — dialog « Ouvrir » .zip + analyse SANS écriture.
/// Retourne le chemin du zip (repassé tel quel à appliquer_zip_import) +
/// les scènes importables + les conflits à trancher par l'UI, ou None si
/// dialog annulé.
pub fn choisir_et_analyser_zip(
    app: &AppHandle,
) -> Result<Option<AnalyseZipImport>, String> {
    use tauri_plugin_dialog::DialogExt;

    let file = app
        .dialog()
        .file()
        .add_filter("Archive ZIP", &["zip"])
        .blocking_pick_file();
    let Some(file) = file else {
        return Ok(None); // dialog annulé
    };
    let zip_path = file.into_path().map_err(|e| format!("Chemin invalide: {}", e))?;

    let (lecteur, racine, idx_zip) = lire_index_zip(&zip_path)?;
    // Une scène déclarée sans fichier <id>.json dans l'archive est signalée
    // (index corrompu) — elle n'est pas proposée à l'import.
    let scenes: Vec<SceneIndex> = idx_zip
        .into_iter()
        .filter(|e| {
            let ok = lecteur
                .entrees()
                .any(|n| n == format!("{}scenes/{}.json", racine, e.id).as_str());
            if !ok {
                log::warn!("Import ZIP : scène {} déclarée mais absente de l'archive", e.id);
            }
            ok
        })
        .collect();

    let idx_local = load_index(app)?;
    let conflits: Vec<ConflitZip> = scenes
        .iter()
        .filter_map(|e| {
            idx_local
                .iter()
                .find(|l| l.id == e.id)
                .map(|l| ConflitZip {
                    id: e.id.clone(),
                    nomArchive: e.nom.clone(),
                    nomLocal: l.nom.clone(),
                })
        })
        .collect();

    Ok(Some(AnalyseZipImport {
        zipPath: zip_path.display().to_string(),
        scenes,
        conflits,
    }))
}

/// Phase 2 — applique l'import décidé par l'UI.
/// `remplacer` = ids de scènes locales que l'utilisateur accepte d'écraser
/// (les conflits non listés sont passés). Restaure <racine>medias/** en
/// écrasant les mêmes noms, écrit les <id>.json des scènes acceptées ou
/// nouvelles, met à jour index.json (noms + nomMasque repris de
/// l'archive), puis recharge la scène courante en RAM (elle a pu être
/// remplacée) + snapshot. Scènes locales absentes du zip : intactes.
pub fn appliquer_zip_import(
    app: &AppHandle,
    state: &ScenesState,
    zip_path: &std::path::Path,
    remplacer: &[String],
) -> Result<BilanImportZip, String> {
    // 0. Sauver la courante d'abord — les fichiers remplacés ensuite sont
    //    la vérité de l'archive ; la scène RAM est rechargée en fin.
    save_current(app, state)?;

    let (mut lecteur, racine, idx_zip) = lire_index_zip(zip_path)?;
    let remplacer: std::collections::HashSet<&str> =
        remplacer.iter().map(String::as_str).collect();
    let mut idx_local = load_index(app)?;
    let mut bilan = BilanImportZip {
        scenesImportees: 0,
        mediasRestaures: 0,
        scenesSautees: 0,
    };

    // 1. Médias : <racine>medias/** → data/medias/** (écrasement des mêmes
    //    noms — les noms sont des uuid, même nom = même fichier). Un nom
    //    malveillant a déjà été écarté par le lecteur (anti zip-slip) ;
    //    une entrée corrompue est sautée (log warn) sans bloquer le reste.
    let prefixe_medias = format!("{}medias/", racine);
    let medias_root = data_dir(app)?.join("medias");
    let noms: Vec<String> = lecteur.entrees().map(str::to_string).collect();
    for nom in noms.iter().filter(|n| n.starts_with(&prefixe_medias)) {
        let rel = &nom[prefixe_medias.len()..];
        let dest = medias_root.join(rel);
        match lecteur.lire_bytes(nom) {
            Ok(bytes) => {
                if let Some(parent) = dest.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        log::warn!("Import ZIP : création {}: {}", parent.display(), e);
                        continue;
                    }
                }
                match fs::write(&dest, &bytes) {
                    Ok(()) => bilan.mediasRestaures += 1,
                    Err(e) => log::warn!("Import ZIP : écriture {}: {}", dest.display(), e),
                }
            }
            Err(e) => log::warn!("Import ZIP : média illisible dans l'archive ({}): {}", nom, e),
        }
    }

    // 2. Scènes : nouvelle → fichier + index ; conflit accepté → fichier
    //    écrasé + nom/nomMasque repris de l'archive ; conflit décliné →
    //    passé. Un <id>.json dont le schéma ne parse pas n'est JAMAIS
    //    écrit (on ne casse pas une scène locale saine).
    for entry in &idx_zip {
        let existe = idx_local.iter().position(|s| s.id == entry.id);
        if existe.is_some() && !remplacer.contains(entry.id.as_str()) {
            bilan.scenesSautees += 1;
            continue; // conflit décliné par l'utilisateur
        }
        let chemin = format!("{}scenes/{}.json", racine, entry.id);
        let bytes = match lecteur.lire_bytes(&chemin) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("Import ZIP : scène {} illisible ({}), sautée", entry.id, e);
                bilan.scenesSautees += 1;
                continue;
            }
        };
        if let Err(e) = serde_json::from_slice::<Scene>(&bytes) {
            log::warn!("Import ZIP : scène {} schéma invalide ({}), sautée", entry.id, e);
            bilan.scenesSautees += 1;
            continue;
        }
        let dest = scene_file_path(app, &entry.id)?;
        fs::write(&dest, &bytes).map_err(|e| format!("Écriture {}: {}", dest.display(), e))?;
        match existe {
            Some(pos) => {
                idx_local[pos].nom = entry.nom.clone();
                idx_local[pos].nomMasque = entry.nomMasque;
            }
            None => idx_local.push(entry.clone()),
        }
        bilan.scenesImportees += 1;
    }
    save_index(app, &idx_local)?;

    // 3. Recharger la scène courante depuis le disque : si son id a été
    //    remplacé, la RAM doit refléter la version importée (sinon la
    //    prochaine sauvegarde réécrirait l'ancienne par-dessus).
    let current_id = state.current_id.lock().unwrap().clone();
    let mut scene = load_scene_file(app, &current_id).unwrap_or_else(|e| {
        log::warn!("Import ZIP : rechargement scène courante {}: {}", current_id, e);
        Scene::new()
    });
    appliquer_canvas_obs(app, &mut scene);
    {
        let mut s = state.scene.lock().unwrap();
        *s = scene;
    }
    purge_cadres_chat(state);
    push_snapshot(state);

    log::info!(
        "Import ZIP terminé : {} ({} scènes, {} médias, {} sautées)",
        zip_path.display(),
        bilan.scenesImportees,
        bilan.mediasRestaures,
        bilan.scenesSautees
    );
    Ok(bilan)
}
