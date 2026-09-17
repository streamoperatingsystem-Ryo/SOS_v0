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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tokio::sync::broadcast;

/// Jeton d'annulation du timer courant.
type TimerCancel = Arc<AtomicBool>;

/// Taille max de la file d'attente (au-delà → l'oldest est évincé).
const FILE_MAX: usize = 5;

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
    /// Couleur de fallback du texte (si aucun cadre SVG actif — sinon la
    /// couleur du cadre widget est utilisée côté diffusion).
    #[serde(default = "default_couleur")]
    pub couleur: String,
    #[serde(default = "default_taille_px")]
    pub taille_px: u32,
    #[serde(default)]
    pub cooldown_viewer_s: u64,
    #[serde(default)]
    pub cooldown_global_s: u64,
    /// Chemin relatif du son importé ("medias/<uuid>.<ext>"). Mode image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub son: Option<String>,
    /// Média ("medias/<uuid>.<ext>"). None = icône + texte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    /// "image" | "video".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_kind: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_duree_ms() -> u64 {
    5000
}
fn default_couleur() -> String {
    "#ffffff".to_string()
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
            couleur: default_couleur(),
            taille_px: default_taille_px(),
            cooldown_viewer_s: 0,
            cooldown_global_s: 0,
            son: None,
            media: None,
            media_kind: None,
        }
    }
}

// ===== Item de file =====

#[derive(Debug, Clone)]
struct CommandeItem {
    id: String,
    texte: String,
    son: Option<String>,
    media: Option<String>,
    media_kind: Option<String>,
    couleur: String,
    taille_px: u32,
    duree_ms: u64,
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
        let file = load_file(&app).unwrap_or_default();
        let n = file.commandes.len();
        let state = Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(file.commandes)),
            file: Arc::new(Mutex::new(VecDeque::new())),
            en_cours: Arc::new(Mutex::new(false)),
            timer_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            dernier_global: Arc::new(Mutex::new(HashMap::new())),
        };
        eprintln!("[Commandes] init : {} commande(s)", n);
        state
    }

    // ===== Déclenchement (hook chat) =====

    /// Appelé par les modules chat à chaque message (à côté du hook bandeau).
    /// Le 1er mot du message (ex "!hype") est comparé aux commandes
    /// enregistrées. Match + actif + cooldowns OK → file ou affichage direct.
    pub fn on_message(&self, pseudo: &str, display_name: &str, texte: &str) {
        let trimmed = texte.trim();
        let Some(rest) = trimmed.strip_prefix('!') else {
            return;
        };
        let mut mots = rest.split_whitespace();
        let Some(premier) = mots.next() else {
            return;
        };
        let nom = premier.to_lowercase();
        let reste = rest[premier.len()..].trim().to_string();

        let cfg = {
            let c = self.config.lock().unwrap();
            c.iter().find(|c| c.actif && c.commande == nom).cloned()
        };
        let Some(cfg) = cfg else { return };

        // Cooldown par viewer (anti-spam d'un même viewer — clé par pseudo).
        let now = Instant::now();
        if cfg.cooldown_viewer_s > 0 {
            let key = format!("{}:{}", cfg.id, pseudo.to_lowercase());
            let mut cds = self.cooldowns.lock().unwrap();
            if let Some(last) = cds.get(&key) {
                if now.duration_since(*last).as_secs() < cfg.cooldown_viewer_s {
                    return;
                }
            }
            cds.insert(key, now);
        }

        // Cooldown global (rythme maximum de la commande).
        if cfg.cooldown_global_s > 0 {
            let mut g = self.dernier_global.lock().unwrap();
            if let Some(last) = g.get(&cfg.id) {
                if now.duration_since(*last).as_secs() < cfg.cooldown_global_s {
                    return;
                }
            }
            g.insert(cfg.id.clone(), now);
        }

        let texte_rendu = cfg
            .texte_template
            .replace("{pseudo}", display_name)
            .replace("{commande}", &cfg.commande)
            .replace("{message}", &reste);

        let item = CommandeItem {
            id: cfg.id.clone(),
            texte: texte_rendu,
            son: cfg.son.clone(),
            media: cfg.media.clone(),
            media_kind: cfg.media_kind.clone(),
            couleur: cfg.couleur.clone(),
            taille_px: cfg.taille_px,
            duree_ms: cfg.duree_ms.max(1000),
        };

        eprintln!(
            "[Commandes] !{} pseudo={} → file/affichage",
            cfg.commande, display_name
        );

        // Une seule commande à la fois : direct si libre, sinon file (max 5).
        {
            let mut en_cours = self.en_cours.lock().unwrap();
            if *en_cours {
                let mut file = self.file.lock().unwrap();
                if file.len() >= FILE_MAX {
                    file.pop_front();
                }
                file.push_back(item);
                return;
            }
            *en_cours = true;
        }
        self.afficher(item);
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
                "couleur": item.couleur,
                "taille_px": item.taille_px,
                "duree_ms": item.duree_ms,
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
                _ = await_timer_cancel(&new_cancel) => {
                    eprintln!("[Commandes] timer annulé");
                }
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
        let item = CommandeItem {
            id: cfg.id.clone(),
            texte: cfg
                .texte_template
                .replace("{pseudo}", "Streamy")
                .replace("{commande}", &cfg.commande)
                .replace("{message}", "test"),
            son: cfg.son.clone(),
            media: cfg.media.clone(),
            media_kind: cfg.media_kind.clone(),
            couleur: cfg.couleur.clone(),
            taille_px: cfg.taille_px,
            duree_ms: cfg.duree_ms.max(1000),
        };
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
}

/// Format du fichier commandes_config.json sur disque.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CommandesFile {
    #[serde(default)]
    commandes: Vec<CommandeConfig>,
}

/// Charge commandes_config.json (absent → liste vide).
fn load_file(app: &AppHandle) -> Result<CommandesFile, String> {
    let dir = data_dir(app)?;
    let path = dir.join("commandes_config.json");
    if !path.exists() {
        return Ok(CommandesFile::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire commandes_config.json: {}", e))?;
    let cfg: CommandesFile = serde_json::from_str(&content)
        .map_err(|e| format!("Parse commandes_config.json: {}", e))?;
    Ok(cfg)
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
