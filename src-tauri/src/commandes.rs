/// Commandes chat — "!commande" tapée par un viewer → overlay diffusion.
///
/// Même moteur que alertes.rs (file FIFO max 5, UNE commande à la fois,
/// cooldowns viewer + global, timer Rust autoritaire + fallback diffusion),
/// mais déclenché par un MESSAGE chat : le 1er mot (ex: "!hype") est comparé
/// aux commandes enregistrées. Le message continue de circuler normalement
/// dans le chat (comme un cheer — l'overlay est EN PLUS).
///
/// Flux :
///   1. Module chat (twitch/kick/tiktok/youtube) appelle `on_message(...)`
///      à côté du hook bandeau (non-fatal si état absent).
///   2. 1er mot du message (sans `!`, lowercase) match une commande
///      enregistrée + active + cooldowns OK → template rendu → file/direct.
///   3. WS chat_tx : `commande-config` (liste complète, avant play) +
///      `commande-play` + `commande-stop` → diffusion.html (overlay z 9996).
///
/// Position/taille : squelette de position unifié (position_overlay.rs) —
/// même squelette que les clips de bienvenue et les alertes.
///
/// Persistance : `data/commandes_config.json` (liste de commandes).
/// Cooldowns en RAM (clé viewer = pseudo — ChatMessage n'a pas de user_id).
use crate::config::data_dir;
use crate::position_overlay::PositionOverlayState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::broadcast;

/// Jeton d'annulation du timer courant.
type TimerCancel = Arc<AtomicBool>;

/// Taille max de la file d'attente (au-delà → l'oldest est évincé).
const FILE_MAX: usize = 5;

/// Nom fixe de la commande générique qui liste les commandes viewer actives.
/// Réservé dans tous les modules (aucune commande perso/module ne peut le prendre).
pub const CMD_LISTE: &str = "cmd";
/// Cooldown global de `!cmd` (s) — évite le spam de la liste.
const CMD_LISTE_COOLDOWN_S: u64 = 10;
/// Longueur max d'un message chat Twitch pour la liste (marge sous 500).
const CMD_CHAT_MAX: usize = 440;

// ===== Config =====

/// Configuration d'une commande chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandeConfig {
    /// ID unique (uuid) — upsert par id, généré si vide.
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_true")]
    pub actif: bool,
    /// Mot de la commande SANS le "!" (ex: "hype"). Normalisé lowercase.
    #[serde(default)]
    pub commande: String,
    #[serde(default = "default_duree_ms")]
    pub duree_ms: u64,
    /// Variables : {pseudo} {commande} {message} (reste de la ligne).
    #[serde(default)]
    pub texte_template: String,
    #[serde(default = "default_taille_px")]
    pub taille_px: u32,
    #[serde(default)]
    pub cooldown_viewer_s: u64,
    #[serde(default)]
    pub cooldown_global_s: u64,
    /// Chemin relatif du son importé ("medias/<uuid>.<ext>"). Joué
    /// par-dessus dans TOUS les modes (texte seul, image, vidéo — l'audio
    /// de la vidéo n'est pas coupé).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub son: Option<String>,
    /// Média ("medias/<uuid>.<ext>"). None = icône + texte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    /// "image" | "video".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_kind: Option<String>,
    /// Audio de la vidéo (mode video uniquement) : false = vidéo muette
    /// (utile quand un son importé joue par-dessus). Défaut true.
    #[serde(default = "default_true")]
    pub video_son: bool,
    /// Police Google embarquée (id fichier woff2, ex: "BebasNeue").
    /// "" = police système (défaut — même règle que le bandeau).
    #[serde(default)]
    pub police: String,
    #[serde(default)]
    pub gras: bool,
    #[serde(default)]
    pub italique: bool,
    #[serde(default)]
    pub souligne: bool,
    /// Couleur personnalisée du texte (hex). None = auto (couleur du cadre
    /// widget si actif, sinon blanc — rendu historique).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub couleur: Option<String>,
    /// false = aucun accusé chat (même sur Twitch qui peut répondre —
    /// même règle que musique/fight).
    #[serde(default = "default_true")]
    pub reponses_chat: bool,
}

fn default_true() -> bool {
    true
}
fn default_duree_ms() -> u64 {
    5000
}
fn default_taille_px() -> u32 {
    48
}

impl Default for CommandeConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            actif: true,
            commande: String::new(),
            duree_ms: default_duree_ms(),
            texte_template: "{pseudo} lance {commande} !".to_string(),
            taille_px: default_taille_px(),
            cooldown_viewer_s: 0,
            cooldown_global_s: 0,
            son: None,
            media: None,
            media_kind: None,
            video_son: true,
            police: String::new(),
            gras: false,
            italique: false,
            souligne: false,
            couleur: None,
            reponses_chat: true,
        }
    }
}

/// Résultat de l'import (sérialisé vers le frontend).
/// Les `conflits` gardent leurs chemins médias d'origine (relatifs au pack) :
/// ils ne sont copiés vers data/medias/ que si l'utilisateur choisit
/// « écraser » → `importer_ecraser`.
#[derive(Debug, Serialize)]
pub struct ImportCommandesResultat {
    /// true = dialog dossier annulé (rien lu).
    pub annule: bool,
    /// Dossier pack choisi — repassé tel quel à `commande_importer_ecraser`.
    pub pack_dir: Option<String>,
    /// Nb de commandes importées directement (sans conflit de nom).
    pub importees: u32,
    /// Commandes du pack dont le nom existe déjà → l'UI tranche chacune
    /// (écraser / ignorer). Chemins médias pas encore copiés.
    pub conflits: Vec<CommandeConfig>,
    /// Noms réservés à la musique (!rs, !skip…) ignorés à l'import.
    pub reserves: Vec<String>,
}

// ===== Item de file =====

#[derive(Debug, Clone)]
struct CommandeItem {
    id: String,
    texte: String,
    son: Option<String>,
    media: Option<String>,
    media_kind: Option<String>,
    taille_px: u32,
    duree_ms: u64,
    video_son: bool,
    police: String,
    gras: bool,
    italique: bool,
    souligne: bool,
    couleur: Option<String>,
}

impl CommandeItem {
    /// Construit l'item depuis une config + texte déjà rendu (variables
    /// remplacées par l'appelant).
    fn depuis_config(cfg: &CommandeConfig, texte: String) -> Self {
        Self {
            id: cfg.id.clone(),
            texte,
            son: cfg.son.clone(),
            media: cfg.media.clone(),
            media_kind: cfg.media_kind.clone(),
            taille_px: cfg.taille_px,
            duree_ms: cfg.duree_ms.max(1000),
            video_son: cfg.video_son,
            police: cfg.police.clone(),
            gras: cfg.gras,
            italique: cfg.italique,
            souligne: cfg.souligne,
            couleur: cfg.couleur.clone(),
        }
    }
}

// ===== État partagé =====

/// État partagé commandes. Clonable (Arc internes).
#[derive(Clone)]
pub struct CommandesState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<Vec<CommandeConfig>>>,
    /// File d'attente (commandes suivant celle affichée).
    file: Arc<Mutex<VecDeque<CommandeItem>>>,
    /// true = une commande est affichée (le timer la libère).
    en_cours: Arc<Mutex<bool>>,
    timer_cancel: Arc<Mutex<TimerCancel>>,
    /// Cooldowns par viewer : clé "id:pseudo" → dernier déclenchement.
    cooldowns: Arc<Mutex<HashMap<String, Instant>>>,
    /// Cooldown global par commande : clé id → dernier déclenchement.
    dernier_global: Arc<Mutex<HashMap<String, Instant>>>,
}

impl CommandesState {
    /// Crée l'état + charge la config depuis le disque.
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let commandes = load_file(&app).unwrap_or_default();
        Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(commandes)),
            file: Arc::new(Mutex::new(VecDeque::new())),
            en_cours: Arc::new(Mutex::new(false)),
            timer_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            dernier_global: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // ===== Déclenchement (hook chat) =====

    /// Appelé par les modules chat à chaque message (à côté du hook bandeau).
    /// Le 1er mot du message (ex "!hype") est comparé aux commandes
    /// enregistrées. Match + actif + cooldowns OK → file ou affichage direct.
    /// `plateforme` sert uniquement à router la réponse de `!cmd` (Twitch a
    /// un canal d'envoi ; les autres plateformes n'ont que l'overlay).
    /// Retourne un accusé à poster en chat (Twitch PRIVMSG) — None si la
    /// commande est refusée (inconnue, cooldown) ou si reponses_chat = false.
    pub fn on_message(
        &self,
        plateforme: &str,
        pseudo: &str,
        display_name: &str,
        texte: &str,
    ) -> Option<String> {
        let trimmed = texte.trim();
        let rest = trimmed.strip_prefix('!')?;
        let mut mots = rest.split_whitespace();
        let premier = mots.next()?;
        let nom = premier.to_lowercase();
        let reste = rest[premier.len()..].trim().to_string();

        // Liste générique : gérée AVANT toute commande perso (nom réservé).
        if nom == CMD_LISTE {
            self.lister_commandes(plateforme);
            return None;
        }

        let cfg = {
            let c = self.config.lock().unwrap();
            c.iter().find(|c| c.actif && c.commande == nom).cloned()
        }?;

        // Cooldown par viewer (anti-spam d'un même viewer — clé par pseudo).
        let now = Instant::now();
        if cfg.cooldown_viewer_s > 0 {
            let key = format!("{}:{}", cfg.id, pseudo.to_lowercase());
            let mut cds = self.cooldowns.lock().unwrap();
            if let Some(last) = cds.get(&key) {
                if now.duration_since(*last).as_secs() < cfg.cooldown_viewer_s {
                    return None;
                }
            }
            cds.insert(key, now);
        }

        // Cooldown global (rythme maximum de la commande).
        if cfg.cooldown_global_s > 0 {
            let mut g = self.dernier_global.lock().unwrap();
            if let Some(last) = g.get(&cfg.id) {
                if now.duration_since(*last).as_secs() < cfg.cooldown_global_s {
                    return None;
                }
            }
            g.insert(cfg.id.clone(), now);
        }

        let texte_rendu = cfg
            .texte_template
            .replace("{pseudo}", display_name)
            .replace("{commande}", &cfg.commande)
            .replace("{message}", &reste);

        let item = CommandeItem::depuis_config(&cfg, texte_rendu);

        // Accusé d'acceptation (déclenché aussi quand la commande part en
        // file — le déclenchement est validé, l'affichage suivra).
        let accuse = cfg
            .reponses_chat
            .then(|| format!("✓ !{}", cfg.commande));

        // Une seule commande à la fois : direct si libre, sinon file (max 5).
        {
            let mut en_cours = self.en_cours.lock().unwrap();
            if *en_cours {
                let mut file = self.file.lock().unwrap();
                if file.len() >= FILE_MAX {
                    file.pop_front();
                }
                file.push_back(item);
                return accuse;
            }
            *en_cours = true;
        }
        self.afficher(item);
        accuse
    }

    /// `!cmd` — liste les commandes viewer actives (perso + modules).
    /// Overlay pour TOUTES les plateformes (carte `cmd-liste`) ; chat Twitch
    /// en chunks d'une ligne (les autres plateformes n'ont pas d'envoi).
    fn lister_commandes(&self, plateforme: &str) {
        // Anti-spam global (clé interne distincte des ids uuid).
        {
            let now = Instant::now();
            let mut g = self.dernier_global.lock().unwrap();
            if let Some(last) = g.get("__cmd") {
                if now.duration_since(*last).as_secs() < CMD_LISTE_COOLDOWN_S {
                    return;
                }
            }
            g.insert("__cmd".to_string(), now);
        }

        let mut groupes: Vec<(&str, Vec<String>)> = Vec::new();

        // Commandes perso actives (le nom réservé `cmd` est exclu).
        let perso: Vec<String> = self
            .config
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.actif && !c.commande.is_empty() && c.commande != CMD_LISTE)
            .map(|c| format!("!{}", c.commande))
            .collect();
        if !perso.is_empty() {
            groupes.push(("Perso", perso));
        }

        // Musique : !skip est réservé aux mods → exclu de la liste viewer.
        if let Some(m) = self.app.try_state::<crate::musique::MusiqueState>() {
            let cfg = m.config();
            if cfg.actif {
                groupes.push((
                    "Musique",
                    vec![
                        format!("!{}", cfg.commande),
                        format!("!{}", cfg.commande_retirer),
                        format!("!{}", cfg.commande_file),
                    ],
                ));
            }
        }

        // Arène d'emojis : !fightstop est réservé aux mods → exclu.
        // Même inactive, `!fight aide` reste disponible → listé.
        if let Some(f) = self.app.try_state::<crate::fight::FightState>() {
            let cfg = f.config();
            if cfg.actif {
                groupes.push((
                    "Arène",
                    vec![
                        format!("!{}", cfg.commande_fight),
                        format!("!{}", cfg.commande_accept),
                        format!("!{}", cfg.commande_refuse),
                        format!("!{}", cfg.commande_a),
                        format!("!{}", cfg.commande_b),
                        format!("!{}", cfg.commande_c),
                        format!("!{}", cfg.commande_ff),
                        format!("!{} aide", cfg.commande_fight),
                    ],
                ));
            } else {
                groupes.push(("Aide", vec![format!("!{} aide", cfg.commande_fight)]));
            }
        }

        // Cadres.
        if let Some(c) = self.app.try_state::<crate::cadres_cmd::CadresChatState>() {
            let cfg = c.etat();
            if cfg.actif {
                groupes.push(("Cadres", vec![format!("!{}", cfg.commande)]));
            }
        }

        // Toujours : la commande de liste elle-même.
        groupes.push(("Liste", vec![format!("!{}", CMD_LISTE)]));

        // Dédoublonne (config legacy avec un nom réservé, etc.).
        let mut vus: Vec<String> = Vec::new();
        for (_, cmds) in &mut groupes {
            cmds.retain(|c| {
                if vus.contains(c) {
                    false
                } else {
                    vus.push(c.clone());
                    true
                }
            });
        }

        let (overlay, chunks) = construire_liste(&groupes);
        let frame = serde_json::json!({
            "type": "cmd-liste",
            "texte": overlay,
            "duree_ms": 12000,
        });
        let _ = self.chat_tx.send(frame.to_string());

        if plateforme == "twitch" && !chunks.is_empty() {
            let n = chunks.len();
            if let Some(twitch) = self.app.try_state::<crate::TwitchState>() {
                for (i, chunk) in chunks.into_iter().enumerate() {
                    let texte = if n > 1 {
                        format!("({}/{}) {}", i + 1, n, chunk)
                    } else {
                        chunk
                    };
                    let _ = twitch.envoyer_message_chat(&texte);
                }
            }
        }
    }

    /// Affiche l'item (émission WS + timer de libération → suivant).
    fn afficher(&self, item: CommandeItem) {
        // Pousser la config avant le play (cohérence diffusion).
        self.emit_config_ws();

        let msg = serde_json::json!({
            "type": "commande-play",
            "commande": {
                "id": item.id,
                "texte": item.texte,
                "son": item.son,
                "media": item.media,
                "media_kind": item.media_kind,
                "taille_px": item.taille_px,
                "duree_ms": item.duree_ms,
                "video_son": item.video_son,
                "police": item.police,
                "gras": item.gras,
                "italique": item.italique,
                "souligne": item.souligne,
                "couleur": item.couleur,
            },
        });
        let _ = self.chat_tx.send(msg.to_string());

        // Timer → commande-stop + libération + suivante de la file.
        let state = self.clone();
        let new_cancel = Arc::new(AtomicBool::new(false));
        *self.timer_cancel.lock().unwrap() = new_cancel.clone();

        tauri::async_runtime::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(item.duree_ms)) => {
                    if !new_cancel.load(Ordering::SeqCst) {
                        let stop = serde_json::json!({ "type": "commande-stop" });
                        let _ = state.chat_tx.send(stop.to_string());
                        *state.en_cours.lock().unwrap() = false;
                        state.avancer();
                    }
                }
                _ = await_timer_cancel(&new_cancel) => {}
            }
        });
    }

    /// Affiche la commande suivante de la file (si libre + file non vide).
    fn avancer(&self) {
        let next = {
            let mut en_cours = self.en_cours.lock().unwrap();
            if *en_cours {
                return;
            }
            let mut file = self.file.lock().unwrap();
            match file.pop_front() {
                Some(it) => {
                    *en_cours = true;
                    Some(it)
                }
                None => None,
            }
        };
        if let Some(it) = next {
            self.afficher(it);
        }
    }

    // ===== Test manuel =====

    /// Déclenche une commande de test (données factices, cooldowns contournés).
    pub fn tester(&self, id: &str) -> Result<(), String> {
        let cfg = {
            let c = self.config.lock().unwrap();
            c.iter().find(|c| c.id == id).cloned()
        };
        let Some(cfg) = cfg else {
            return Err(format!("Commande inconnue : {}", id));
        };
        let item = CommandeItem::depuis_config(
            &cfg,
            cfg.texte_template
                .replace("{pseudo}", "Streamy")
                .replace("{commande}", &cfg.commande)
                .replace("{message}", "test"),
        );
        self.afficher(item);
        Ok(())
    }

    // ===== Config =====

    /// Ajoute/remplace une commande (upsert par id) + persiste + push WS.
    /// Normalise le nom (trim, sans "!", lowercase) et la durée (≥ 1s).
    pub fn set_config(&self, mut cfg: CommandeConfig) -> Result<CommandeConfig, String> {
        if cfg.id.is_empty() {
            cfg.id = uuid::Uuid::new_v4().simple().to_string();
        }
        cfg.commande = cfg.commande.trim().trim_start_matches('!').to_lowercase();
        if cfg.commande.is_empty() {
            return Err("Nom de commande vide".into());
        }
        if cfg.commande == CMD_LISTE {
            return Err("« !cmd » est réservé à la liste des commandes".into());
        }
        // Collision avec les commandes musique (!rs, !skip, !retirer, !file) :
        // les deux hooks on_message déclencheraient en double → refusé.
        if let Some(m) = self.app.try_state::<crate::musique::MusiqueState>() {
            let reserves = m.noms_reserves();
            if reserves.contains(&cfg.commande) {
                return Err(format!(
                    "« !{} » est réservé à la musique — choisissez un autre nom",
                    cfg.commande
                ));
            }
        }
        // Même règle avec les commandes de l'arène d'emojis (!fight, !a…).
        if let Some(f) = self.app.try_state::<crate::fight::FightState>() {
            let reserves = f.noms_reserves();
            if reserves.contains(&cfg.commande) {
                return Err(format!(
                    "« !{} » est réservé à l'arène d'emojis — choisissez un autre nom",
                    cfg.commande
                ));
            }
        }
        // Même règle avec la commande cadres (!cadres — couleur du cadre).
        if let Some(c) = self.app.try_state::<crate::cadres_cmd::CadresChatState>() {
            let reserves = c.noms_reserves();
            if reserves.contains(&cfg.commande) {
                return Err(format!(
                    "« !{} » est réservé aux cadres — choisissez un autre nom",
                    cfg.commande
                ));
            }
        }
        cfg.duree_ms = cfg.duree_ms.max(1000);
        {
            let mut c = self.config.lock().unwrap();
            match c.iter_mut().find(|x| x.id == cfg.id) {
                Some(slot) => *slot = cfg.clone(),
                None => c.push(cfg.clone()),
            }
        }
        self.save_to_disk()?;
        self.emit_config_ws();
        Ok(cfg)
    }

    /// Supprime une commande + persiste + push WS.
    pub fn supprimer(&self, id: &str) -> Result<(), String> {
        {
            let mut c = self.config.lock().unwrap();
            c.retain(|x| x.id != id);
        }
        self.save_to_disk()?;
        self.emit_config_ws();
        Ok(())
    }

    /// Retourne la liste complète (pour l'UI dashboard).
    pub fn etat(&self) -> Vec<CommandeConfig> {
        self.config.lock().unwrap().clone()
    }

    /// Émet commande-config sur le WS :4321 (liste complète + overlay).
    /// L'overlay (position/taille) est lu depuis le squelette de position
    /// unifié (PositionOverlayState) — même mécanique que alertes.rs.
    fn emit_config_ws(&self) {
        let cfg = self.config.lock().unwrap().clone();
        let overlay = PositionOverlayState::from_app(&self.app)
            .map(|s| {
                let p = s.etat();
                serde_json::json!({
                    "x": p.x,
                    "y": p.y,
                    "largeur": p.largeur,
                    "hauteur": p.hauteur,
                })
            })
            .unwrap_or_else(|| serde_json::json!({ "x": 660.0, "y": 60.0, "largeur": 600.0, "hauteur": 120.0 }));
        let msg = serde_json::json!({
            "type": "commande-config",
            "commandes": cfg,
            "overlay": overlay,
        });
        let _ = self.chat_tx.send(msg.to_string());
    }

    // ===== Persistance =====

    fn save_to_disk(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let path = dir.join("commandes_config.json");
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize commandes_config.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire commandes_config.json: {}", e))?;
        Ok(())
    }

    // ===== Export / import (pack dossier — même mécanique que scenes.rs) =====

    /// Exporte les commandes comme pack dossier portable :
    /// `<parent>/<nom_pack>/commandes.json` (la liste telle quelle, chemins
    /// médias relatifs au pack) + `medias/` (son + media réellement
    /// référencés). Le dialog choisit le dossier PARENT ; nom vide → erreur ;
    /// dossier existant → erreur (pas d'écrasement silencieux).
    /// Retourne true si exporté, false si dialog annulé.
    pub fn exporter(&self, nom_pack: &str) -> Result<bool, String> {
        use tauri_plugin_dialog::DialogExt;

        let nom_pack = nom_pack.trim();
        if nom_pack.is_empty() {
            return Err("Nom du pack vide".into());
        }
        let folder = self.app.dialog().file().blocking_pick_folder();
        let Some(folder) = folder else {
            return Ok(false);
        };
        let parent: PathBuf = folder
            .into_path()
            .map_err(|e| format!("Chemin invalide: {}", e))?;

        let pack_dir = parent.join(nom_pack);
        if pack_dir.exists() {
            return Err(format!(
                "Le dossier « {} » existe déjà dans {}",
                nom_pack,
                parent.display()
            ));
        }
        let medias_dir = pack_dir.join("medias");
        fs::create_dir_all(&medias_dir).map_err(|e| format!("Création dossier pack: {}", e))?;

        // Copier les médias référencés (son + media de chaque commande,
        // dédupliqués — même règle que scenes.rs : manquant → warn + chemin gardé).
        let cfgs = self.config.lock().unwrap().clone();
        let data_medias = data_dir(&self.app)?.join("medias");
        let mut vus = std::collections::BTreeSet::new();
        let mut copies = 0u32;
        let mut manquants = 0u32;
        for rel in cfgs
            .iter()
            .flat_map(|c| [c.son.as_deref(), c.media.as_deref()].into_iter().flatten())
        {
            if rel.is_empty() || !vus.insert(rel) {
                continue;
            }
            let file_name = rel.strip_prefix("medias/").unwrap_or(rel);
            let src = data_medias.join(file_name);
            if src.exists() {
                fs::copy(&src, medias_dir.join(file_name))
                    .map_err(|e| format!("Copie média {}: {}", file_name, e))?;
                copies += 1;
            } else {
                manquants += 1;
                log::warn!(
                    "Export commandes : média manquant sur disque ({}), chemin gardé",
                    rel
                );
            }
        }

        let content = serde_json::to_string_pretty(&cfgs)
            .map_err(|e| format!("Sérialisation commandes.json: {}", e))?;
        fs::write(pack_dir.join("commandes.json"), content)
            .map_err(|e| format!("Écriture commandes.json: {}", e))?;
        log::info!(
            "Pack commandes exporté : {:?} ({} commandes, {} médias copiés, {} manquants)",
            pack_dir,
            cfgs.len(),
            copies,
            manquants
        );
        Ok(true)
    }

    /// Importe les commandes depuis un dossier pack (celui qui contient
    /// commandes.json — même fallback « .json unique à la racine » que
    /// scenes::importer). Les commandes sans conflit de nom sont importées
    /// directement (nouvel id, médias copiés vers data/medias/<uuid>.<ext>,
    /// chemins réécrits). Les noms déjà pris sont retournés dans `conflits`
    /// pour décision UI → `importer_ecraser` ; les noms réservés à la musique
    /// sont ignorés (même garde que set_config).
    pub fn importer(&self) -> Result<ImportCommandesResultat, String> {
        use tauri_plugin_dialog::DialogExt;

        let mut res = ImportCommandesResultat {
            annule: false,
            pack_dir: None,
            importees: 0,
            conflits: Vec::new(),
            reserves: Vec::new(),
        };

        let folder = self.app.dialog().file().blocking_pick_folder();
        let Some(folder) = folder else {
            res.annule = true;
            return Ok(res);
        };
        let dossier: PathBuf = folder
            .into_path()
            .map_err(|e| format!("Chemin invalide: {}", e))?;

        // commandes.json, sinon un seul .json à la racine (fallback legacy).
        let json_path = {
            let direct = dossier.join("commandes.json");
            if direct.exists() {
                direct
            } else {
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
                    return Err("Aucun commandes.json (ni .json) trouvé dans le dossier".into());
                } else {
                    return Err(
                        "Plusieurs .json trouvés : renommer le fichier en commandes.json".into(),
                    );
                }
            }
        };

        let content = fs::read_to_string(&json_path)
            .map_err(|e| format!("Lecture commandes.json: {}", e))?;
        let pack_cmds: Vec<CommandeConfig> =
            serde_json::from_str(&content).map_err(|e| format!("JSON invalide: {}", e))?;

        let mut reserves = self
            .app
            .try_state::<crate::musique::MusiqueState>()
            .map(|m| m.noms_reserves())
            .unwrap_or_default();
        // Noms réservés arène + cadres exclus aussi à l'import (même garde).
        if let Some(f) = self.app.try_state::<crate::fight::FightState>() {
            reserves.extend(f.noms_reserves());
        }
        if let Some(c) = self.app.try_state::<crate::cadres_cmd::CadresChatState>() {
            reserves.extend(c.noms_reserves());
        }
        // `!cmd` est réservé à la liste générique — jamais importé.
        reserves.push(CMD_LISTE.to_string());
        let pack_medias = dossier.join("medias");
        let data_medias = data_dir(&self.app)?.join("medias");

        let mut nouvelles: Vec<CommandeConfig> = Vec::new();
        {
            let existants = self.config.lock().unwrap().clone();
            // Dédup interne au pack : la 1ère occurrence du nom gagne.
            let mut vus = std::collections::HashSet::new();
            for mut c in pack_cmds {
                c.commande = c.commande.trim().trim_start_matches('!').to_lowercase();
                if c.commande.is_empty() || !vus.insert(c.commande.clone()) {
                    continue;
                }
                if reserves.contains(&c.commande) {
                    res.reserves.push(c.commande.clone());
                    continue;
                }
                if existants.iter().any(|x| x.commande == c.commande) {
                    // Médias NON copiés — seulement si « écraser » est choisi.
                    res.conflits.push(c);
                    continue;
                }
                normaliser_import(&mut c);
                importer_medias(&pack_medias, &data_medias, &mut c);
                nouvelles.push(c);
            }
        }
        res.importees = nouvelles.len() as u32;
        if !nouvelles.is_empty() {
            self.config.lock().unwrap().extend(nouvelles);
            self.save_to_disk()?;
            self.emit_config_ws();
        }
        res.pack_dir = Some(dossier.to_string_lossy().to_string());
        log::info!(
            "Pack commandes importé : {:?} ({} importées, {} conflits, {} réservés)",
            dossier,
            res.importees,
            res.conflits.len(),
            res.reserves.len()
        );
        Ok(res)
    }

    /// Phase conflits de l'import : les commandes acceptées par l'UI
    /// remplacent les existantes DE MÊME NOM (médias copiés depuis le pack,
    /// nouvel id). Retourne le nb de commandes remplacées.
    pub fn importer_ecraser(
        &self,
        pack_dir: &str,
        cmds: Vec<CommandeConfig>,
    ) -> Result<u32, String> {
        let pack_medias = PathBuf::from(pack_dir).join("medias");
        let data_medias = data_dir(&self.app)?.join("medias");
        let mut n = 0u32;
        {
            let mut cfg = self.config.lock().unwrap();
            for mut c in cmds {
                c.commande = c.commande.trim().trim_start_matches('!').to_lowercase();
                if c.commande.is_empty() || c.commande == CMD_LISTE {
                    continue;
                }
                normaliser_import(&mut c);
                importer_medias(&pack_medias, &data_medias, &mut c);
                cfg.retain(|x| x.commande != c.commande);
                cfg.push(c);
                n += 1;
            }
        }
        if n > 0 {
            self.save_to_disk()?;
            self.emit_config_ws();
        }
        Ok(n)
    }
}

/// Charge commandes_config.json (absent → liste vide).
/// Le fichier contient le tableau CommandeConfig sérialisé tel quel par
/// save_to_disk (pas d'enveloppe).
fn load_file(app: &AppHandle) -> Result<Vec<CommandeConfig>, String> {
    let dir = data_dir(app)?;
    let path = dir.join("commandes_config.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire commandes_config.json: {}", e))?;
    let cfg: Vec<CommandeConfig> = serde_json::from_str(&content)
        .map_err(|e| format!("Parse commandes_config.json: {}", e))?;
    Ok(cfg)
}

/// Prépare une commande importée : nouvel id anti-collision + durée ≥ 1 s
/// (mêmes normalisations que set_config).
fn normaliser_import(c: &mut CommandeConfig) {
    c.id = uuid::Uuid::new_v4().simple().to_string();
    c.duree_ms = c.duree_ms.max(1000);
}

/// Copie les médias (son + media) d'une commande importée depuis
/// `<pack>/medias/` vers `data/medias/<uuid>.<ext>` et réécrit les chemins —
/// même mécanique que l'import de scène. Média absent du pack ou copie
/// échouée → chemin gardé (case vide au rendu).
fn importer_medias(pack_medias: &Path, data_medias: &Path, c: &mut CommandeConfig) {
    for champ in [&mut c.son, &mut c.media] {
        let Some(rel) = champ.clone() else {
            continue;
        };
        let file_name = rel.strip_prefix("medias/").unwrap_or(&rel);
        let src = pack_medias.join(file_name);
        if !src.exists() {
            log::warn!(
                "Import commandes : média absent du pack ({}), chemin gardé",
                rel
            );
            continue;
        }
        let ext = Path::new(file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("dat");
        let new_name = format!("{}.{}", uuid::Uuid::new_v4().simple(), ext);
        match fs::copy(&src, data_medias.join(&new_name)) {
            Ok(_) => *champ = Some(format!("medias/{}", new_name)),
            Err(e) => log::warn!("Import commandes : copie média {} échouée ({})", file_name, e),
        }
    }
}

/// Attend que le jeton d'annulation passe à true (polling 100ms).
async fn await_timer_cancel(cancel: &TimerCancel) {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Construit la carte overlay (multi-ligne, groupée) + les chunks chat
/// Twitch (une ligne, ≤ CMD_CHAT_MAX) à partir des groupes de commandes.
fn construire_liste(groupes: &[(&str, Vec<String>)]) -> (String, Vec<String>) {
    let mut overlay = String::from("📋 COMMANDES ACTIVES");
    for (groupe, cmds) in groupes {
        if cmds.is_empty() {
            continue;
        }
        overlay.push('\n');
        overlay.push_str(groupe);
        overlay.push_str(" : ");
        overlay.push_str(&cmds.join(" · "));
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut courant = String::new();
    for (_, cmds) in groupes {
        for cmd in cmds {
            let ajout = if courant.is_empty() {
                format!("📋 Commandes : {}", cmd)
            } else {
                format!("{} · {}", courant, cmd)
            };
            if ajout.chars().count() > CMD_CHAT_MAX && !courant.is_empty() {
                chunks.push(courant);
                courant = format!("📋 Commandes : {}", cmd);
            } else {
                courant = ajout;
            }
        }
    }
    if !courant.is_empty() {
        chunks.push(courant);
    }
    (overlay, chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liste_overlay_et_chunks() {
        let groupes = vec![
            ("Perso", vec!["!hype".to_string(), "!socials".to_string()]),
            ("Musique", vec!["!rs".to_string(), "!file".to_string()]),
            ("Liste", vec!["!cmd".to_string()]),
        ];
        let (overlay, chunks) = construire_liste(&groupes);
        assert!(overlay.contains("Perso : !hype · !socials"));
        assert!(overlay.contains("Musique : !rs · !file"));
        assert!(overlay.contains("Liste : !cmd"));
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("!hype"));
        assert!(chunks[0].contains("!cmd"));
        assert!(chunks[0].chars().count() <= CMD_CHAT_MAX);
    }

    #[test]
    fn liste_chunks_respecte_la_limite() {
        let cmds: Vec<String> = (0..80).map(|i| format!("!commande{}", i)).collect();
        let groupes = vec![("Perso", cmds)];
        let (_, chunks) = construire_liste(&groupes);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.chars().count() <= CMD_CHAT_MAX));
        let total: String = chunks.join(" ");
        assert!(total.contains("!commande0"));
        assert!(total.contains("!commande79"));
    }
}
