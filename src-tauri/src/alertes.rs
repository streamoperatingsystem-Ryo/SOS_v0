/// Alertes Twitch — follow/raid/sub/resub/subgift/bits → overlay diffusion.
///
/// Sources d'événements :
///   - twitch_chat.rs : USERNOTICE (sub/resub/subgift/raid via tag `msg-id`) +
///     PRIVMSG avec tag `bits` → `alerte::on_event(...)` + event Tauri
///     "chat:event" (dashboard).
///   - Follows : pas d'event IRC pour les follows → diff des snapshots
///     followers côté frontend (stores/alertes.ts) → commande `alerte_declencher`.
///     C'est aussi le point d'entrée prévu pour les futures plateformes
///     (Kick/YouTube/TikTok) : elles appelleront la même commande.
///
/// Moteur (inspiré de l'ancienne app RUST_SOS_2026, réécrit pour l'architecture
/// v0 — même pattern que bandeau.rs) :
///   - Config par type persistée `data/alertes_config.json` : actif, duree_ms,
///     texte_template (variables {pseudo} {nbViewers} {nbBits} {niveauSub}
///     {nbMoisCumul} {destinataire}), couleur, taille_px, cooldowns, son.
///   - File FIFO (max 5 en attente) + UNE alerte affichée à la fois.
///   - Cooldown par viewer (anti-spam) + global (rythme), par type.
///   - Émission WS sur chat_tx : `alerte-config` (avant play) + `alerte-play`
///     + `alerte-stop` → diffusion.html (overlay autonome, z 9997).
///
/// ⚠️ Leçon de l'ancienne app : USERNOTICE doit être parsé explicitement —
/// tombé dans un `_ =>` ignoré, aucun raid/sub n'est jamais émis (bug "raid
/// sans alerte" en live).
use crate::config::data_dir;
use crate::position_overlay::PositionOverlayState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

/// Jeton d'annulation du timer courant.
type TimerCancel = Arc<AtomicBool>;

/// Types d'alerte supportés (ordre d'affichage UI).
pub const TYPES_ALERTE: &[&str] = &["follow", "raid", "sub", "resub", "subgift", "bits"];

/// Taille max de la file d'attente (au-delà → l'oldest est évincé).
const FILE_MAX: usize = 5;

// ===== Config =====

/// Configuration d'un type d'alerte.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlerteTypeConfig {
    #[serde(default = "default_true")]
    pub actif: bool,
    #[serde(default = "default_duree_ms")]
    pub duree_ms: u64,
    #[serde(default)]
    pub texte_template: String,
    #[serde(default = "default_couleur")]
    pub couleur: String,
    #[serde(default = "default_taille_px")]
    pub taille_px: u32,
    #[serde(default)]
    pub cooldown_viewer_s: u64,
    #[serde(default)]
    pub cooldown_global_s: u64,
    /// Chemin relatif du son importé ("medias/<uuid>.<ext>"). None = silence.
    /// Utilisé en mode image (son séparé) ; en mode vidéo le son est porté
    /// par la vidéo elle-même.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub son: Option<String>,
    /// Média de l'alerte ("medias/<uuid>.<ext>") : image (avec son séparé) ou
    /// vidéo (son inclus). None = icône emoji + texte (comportement historique).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    /// Kind du média : "image" | "video" (déduit à l'import, stocké pour
    /// éviter la re-déduction côté diffusion).
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

/// Config par défaut d'un type (template spécifique au type).
fn config_type_defaut(type_alerte: &str) -> AlerteTypeConfig {
    let template = match type_alerte {
        "follow" => "{pseudo} vient de suivre !",
        "raid" => "{pseudo} raide avec {nbViewers} viewers !",
        "sub" => "{pseudo} vient de s'abonner !",
        "resub" => "{pseudo} resub pour {nbMoisCumul} mois !",
        "subgift" => "{pseudo} offre un sub à {destinataire} !",
        "bits" => "{pseudo} a envoyé {nbBits} bits !",
        _ => "{pseudo}",
    };
    AlerteTypeConfig {
        actif: true,
        duree_ms: default_duree_ms(),
        texte_template: template.to_string(),
        couleur: default_couleur(),
        taille_px: default_taille_px(),
        cooldown_viewer_s: 0,
        cooldown_global_s: 0,
        son: None,
        media: None,
        media_kind: None,
    }
}

/// Config globale (persistée). Champs Option → absent sur disque = défaut.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AlertesConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow: Option<AlerteTypeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raid: Option<AlerteTypeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<AlerteTypeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resub: Option<AlerteTypeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgift: Option<AlerteTypeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<AlerteTypeConfig>,
    /// Position/taille de l'overlay diffusion — MÊME mécanisme que la carte
    /// des clips de bienvenue (welcome.rs OverlayConfig). None = défaut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<AlertesOverlayConfig>,
}

/// Position/taille de l'overlay alerte côté diffusion (pixels canvas).
/// Identique à welcome::OverlayConfig — squelette de réglage partagé.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertesOverlayConfig {
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
}

impl Default for AlertesOverlayConfig {
    fn default() -> Self {
        // Haut-centré sur canvas 1920×1080 (carte texte horizontale).
        Self {
            x: 660.0,
            y: 60.0,
            largeur: 600.0,
            hauteur: 120.0,
        }
    }
}

impl AlertesConfig {
    /// Config effective d'un type (champ absent sur disque → défaut du type).
    pub fn pour(&self, type_alerte: &str) -> AlerteTypeConfig {
        let defaut = config_type_defaut(type_alerte);
        match type_alerte {
            "follow" => self.follow.clone().unwrap_or(defaut),
            "raid" => self.raid.clone().unwrap_or(defaut),
            "sub" => self.sub.clone().unwrap_or(defaut),
            "resub" => self.resub.clone().unwrap_or(defaut),
            "subgift" => self.subgift.clone().unwrap_or(defaut),
            "bits" => self.bits.clone().unwrap_or(defaut),
            _ => defaut,
        }
    }

    /// Remplace la config d'un type (validation en amont).
    fn set(&mut self, type_alerte: &str, cfg: AlerteTypeConfig) {
        match type_alerte {
            "follow" => self.follow = Some(cfg),
            "raid" => self.raid = Some(cfg),
            "sub" => self.sub = Some(cfg),
            "resub" => self.resub = Some(cfg),
            "subgift" => self.subgift = Some(cfg),
            "bits" => self.bits = Some(cfg),
            _ => {}
        }
    }
}

// ===== Événement Twitch (depuis IRC) =====

/// Événement Twitch normalisé (USERNOTICE / bits / follow via commande).
/// Sérialisé vers le dashboard (event Tauri "chat:event") et consommé par le
/// moteur. `type_alerte` ∈ TYPES_ALERTE.
#[derive(Debug, Clone, Serialize)]
pub struct EventTwitch {
    pub type_alerte: String,
    pub login: String,
    /// Nom d'affichage (display-name tag, fallback login).
    pub pseudo: String,
    pub user_id: String,
    pub nb_viewers: i64,
    pub nb_bits: i64,
    /// "Prime" | "Tier 1" | "Tier 2" | "Tier 3" (sub/resub/subgift).
    pub niveau_sub: String,
    /// Mois cumulés (resub).
    pub nb_mois: i64,
    /// Destinataire du subgift.
    pub destinataire: String,
    /// Message système Twitch (tag system-msg, \s déjà décodé).
    pub system_msg: String,
}

// ===== Item de file =====

#[derive(Debug, Clone)]
struct AlerteItem {
    type_alerte: String,
    texte: String,
    son: Option<String>,
    media: Option<String>,
    media_kind: Option<String>,
    couleur: String,
    taille_px: u32,
    duree_ms: u64,
}

// ===== État partagé =====

/// État partagé alertes. Clonable (Arc internes).
#[derive(Clone)]
pub struct AlertesState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<AlertesConfig>>,
    /// File d'attente (alertes suivant celle affichée).
    file: Arc<Mutex<VecDeque<AlerteItem>>>,
    /// true = une alerte est affichée (le timer la libère).
    en_cours: Arc<Mutex<bool>>,
    timer_cancel: Arc<Mutex<TimerCancel>>,
    /// Cooldowns par viewer : clé "type:user_id" → dernier déclenchement.
    cooldowns: Arc<Mutex<HashMap<String, Instant>>>,
    /// Cooldown global par type : clé type → dernier déclenchement.
    dernier_global: Arc<Mutex<HashMap<String, Instant>>>,
}

impl AlertesState {
    /// Crée l'état + charge la config depuis le disque.
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let config = load_file(&app).unwrap_or_default();
        Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(config)),
            file: Arc::new(Mutex::new(VecDeque::new())),
            en_cours: Arc::new(Mutex::new(false)),
            timer_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            dernier_global: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // ===== Déclenchement =====

    /// Point d'entrée unique : un événement Twitch arrive (USERNOTICE, bits,
    /// follow via diff frontend, futur : autres plateformes via commande).
    /// Vérifie actif + cooldowns → rend le template → file ou affichage direct.
    pub fn on_event(&self, evt: EventTwitch) {
        if !TYPES_ALERTE.contains(&evt.type_alerte.as_str()) {
            return;
        }
        let cfg = self.config.lock().unwrap().pour(&evt.type_alerte);
        if !cfg.actif {
            return;
        }

        // Cooldown par viewer (anti-spam d'un même viewer).
        let now = Instant::now();
        if cfg.cooldown_viewer_s > 0 && !evt.user_id.is_empty() {
            let key = format!("{}:{}", evt.type_alerte, evt.user_id);
            let mut cds = self.cooldowns.lock().unwrap();
            if let Some(last) = cds.get(&key) {
                if now.duration_since(*last).as_secs() < cfg.cooldown_viewer_s {
                    return;
                }
            }
            cds.insert(key, now);
        }

        // Cooldown global (rythme maximum du type).
        if cfg.cooldown_global_s > 0 {
            let mut g = self.dernier_global.lock().unwrap();
            if let Some(last) = g.get(&evt.type_alerte) {
                if now.duration_since(*last).as_secs() < cfg.cooldown_global_s {
                    return;
                }
            }
            g.insert(evt.type_alerte.clone(), now);
        }

        let item = AlerteItem {
            type_alerte: evt.type_alerte.clone(),
            texte: rendre_template(&cfg.texte_template, &evt),
            son: cfg.son.clone(),
            media: cfg.media.clone(),
            media_kind: cfg.media_kind.clone(),
            couleur: cfg.couleur.clone(),
            taille_px: cfg.taille_px,
            duree_ms: cfg.duree_ms.max(1000),
        };

        // Une seule alerte à la fois : direct si libre, sinon file (max 5).
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
    fn afficher(&self, item: AlerteItem) {
        // Pousser la config avant le play (cohérence diffusion).
        self.emit_config_ws();

        let msg = serde_json::json!({
            "type": "alerte-play",
            "alerte": {
                "type": item.type_alerte,
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

        // Timer → alerte-stop + libération + alerte suivante de la file.
        let state = self.clone();
        let new_cancel = Arc::new(AtomicBool::new(false));
        *self.timer_cancel.lock().unwrap() = new_cancel.clone();

        tauri::async_runtime::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(item.duree_ms)) => {
                    if !new_cancel.load(Ordering::SeqCst) {
                        let stop = serde_json::json!({ "type": "alerte-stop" });
                        let _ = state.chat_tx.send(stop.to_string());
                        *state.en_cours.lock().unwrap() = false;
                        state.avancer();
                    }
                }
                _ = await_timer_cancel(&new_cancel) => {}
            }
        });
    }

    /// Affiche l'alerte suivante de la file (si libre + file non vide).
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

    /// Déclenche une alerte de test (données factices, cooldowns contournés).
    pub fn tester(&self, type_alerte: &str) -> Result<(), String> {
        if !TYPES_ALERTE.contains(&type_alerte) {
            return Err(format!("Type d'alerte inconnu : {}", type_alerte));
        }
        let evt = match type_alerte {
            "follow" => EventTwitch {
                type_alerte: "follow".into(),
                login: "streamy".into(),
                pseudo: "Streamy".into(),
                user_id: "test".into(),
                nb_viewers: 0,
                nb_bits: 0,
                niveau_sub: String::new(),
                nb_mois: 0,
                destinataire: String::new(),
                system_msg: String::new(),
            },
            "raid" => EventTwitch {
                nb_viewers: 25,
                pseudo: "Streamy".into(),
                ..evt_test_base("raid")
            },
            "sub" => EventTwitch {
                niveau_sub: "Tier 1".into(),
                pseudo: "Streamy".into(),
                ..evt_test_base("sub")
            },
            "resub" => EventTwitch {
                niveau_sub: "Tier 2".into(),
                nb_mois: 6,
                pseudo: "Streamy".into(),
                ..evt_test_base("resub")
            },
            "subgift" => EventTwitch {
                niveau_sub: "Tier 1".into(),
                destinataire: "Viewer".into(),
                pseudo: "Streamy".into(),
                ..evt_test_base("subgift")
            },
            "bits" => EventTwitch {
                nb_bits: 500,
                pseudo: "Streamy".into(),
                ..evt_test_base("bits")
            },
            _ => unreachable!(),
        };
        self.on_event(evt);
        Ok(())
    }

    // ===== Config =====

    /// Remplace la config d'un type + persiste + émet config WS + état.
    pub fn set_config_type(&self, type_alerte: &str, cfg: AlerteTypeConfig) -> Result<(), String> {
        if !TYPES_ALERTE.contains(&type_alerte) {
            return Err(format!("Type d'alerte inconnu : {}", type_alerte));
        }
        {
            let mut c = self.config.lock().unwrap();
            let mut cfg = cfg;
            cfg.duree_ms = cfg.duree_ms.max(1000);
            c.set(type_alerte, cfg);
        }
        self.save_to_disk()?;
        self.emit_config_ws();
        self.emit_etat();
        Ok(())
    }

    /// Retourne la config complète (pour l'UI dashboard).
    pub fn etat(&self) -> AlertesConfig {
        self.config.lock().unwrap().clone()
    }

    /// Remplace la position/taille de l'overlay (même mécanique que
    /// welcome_set_overlay_config) + persiste + push WS diffusion.
    ///
    /// Désormais, la source de vérité est le squelette de position unifié
    /// (position_overlay.rs). Cette méthode délègue à `PositionOverlayState::set`
    /// qui persiste `position_overlay.json` et émet `position-overlay-config`
    /// sur le WS (applique aux deux overlays en live). On garde l'émission
    /// `alerte-config` pour rétro-compat.
    pub fn set_overlay_config(
        &self,
        x: f64,
        y: f64,
        largeur: f64,
        hauteur: f64,
    ) -> Result<(), String> {
        if let Some(state) = PositionOverlayState::from_app(&self.app) {
            state.set(crate::position_overlay::PositionOverlayConfig {
                x,
                y,
                largeur,
                hauteur,
            })?;
        }
        // Aussi émettre alerte-config (rétro-compat diffusion.html).
        self.emit_config_ws();
        Ok(())
    }

    /// Émet l'état vers le dashboard (event Tauri alertes:etat).
    fn emit_etat(&self) {
        let cfg = self.etat();
        let _ = self.app.emit("alertes:etat", &cfg);
    }

    /// Émet alerte-config sur le WS :4321 (config complète par type + overlay).
    /// Lit l'overlay depuis le squelette unifié (PositionOverlayState).
    fn emit_config_ws(&self) {
        let cfg = self.config.lock().unwrap();
        let ov = PositionOverlayState::from_app(&self.app)
            .map(|s| {
                let p = s.etat();
                AlertesOverlayConfig {
                    x: p.x,
                    y: p.y,
                    largeur: p.largeur,
                    hauteur: p.hauteur,
                }
            })
            .unwrap_or_else(|| overlay_effectif(&cfg));
        let msg = serde_json::json!({
            "type": "alerte-config",
            "config": {
                "follow": cfg.follow,
                "raid": cfg.raid,
                "sub": cfg.sub,
                "resub": cfg.resub,
                "subgift": cfg.subgift,
                "bits": cfg.bits,
                "overlay": {
                    "x": ov.x,
                    "y": ov.y,
                    "largeur": ov.largeur,
                    "hauteur": ov.hauteur,
                },
            },
        });
        let _ = self.chat_tx.send(msg.to_string());
    }

    // ===== Persistance =====

    fn save_to_disk(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let path = dir.join("alertes_config.json");
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize alertes_config.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire alertes_config.json: {}", e))?;
        Ok(())
    }
}

/// Événement de test de base (pseudo/user_id communs).
fn evt_test_base(type_alerte: &str) -> EventTwitch {
    EventTwitch {
        type_alerte: type_alerte.to_string(),
        login: "streamy".to_string(),
        pseudo: String::new(), // rempli par l'appelant (..evt_test_base)
        user_id: "test".to_string(),
        nb_viewers: 0,
        nb_bits: 0,
        niveau_sub: String::new(),
        nb_mois: 0,
        destinataire: String::new(),
        system_msg: String::new(),
    }
}

/// Remplace les variables {pseudo}, {nbViewers}, {nbBits}, {niveauSub},
/// {nbMoisCumul}, {destinataire} dans le template.
fn rendre_template(template: &str, evt: &EventTwitch) -> String {
    template
        .replace("{pseudo}", &evt.pseudo)
        .replace("{nbViewers}", &evt.nb_viewers.to_string())
        .replace("{nbBits}", &evt.nb_bits.to_string())
        .replace("{niveauSub}", &evt.niveau_sub)
        .replace("{nbMoisCumul}", &evt.nb_mois.to_string())
        .replace("{destinataire}", &evt.destinataire)
}

/// Overlay effectif (config absente → défaut haut-centré).
/// Fonction libre (pas de re-lock du Mutex config depuis emit_config_ws).
fn overlay_effectif(cfg: &AlertesConfig) -> AlertesOverlayConfig {
    cfg.overlay.clone().unwrap_or_default()
}

// ===== Persistance =====

/// Charge alertes_config.json (absent → défauts via Option::None).
fn load_file(app: &AppHandle) -> Result<AlertesConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("alertes_config.json");
    if !path.exists() {
        return Ok(AlertesConfig::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire alertes_config.json: {}", e))?;
    let cfg: AlertesConfig =
        serde_json::from_str(&content).map_err(|e| format!("Parse alertes_config.json: {}", e))?;
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
