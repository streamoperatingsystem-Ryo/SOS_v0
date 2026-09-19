/// Bandeau "premier message" — détection 1er message de n'importe quel viewer
/// (toutes plateformes) + émission WS vers diffusion.html.
///
/// Contrairement aux clips de bienvenue (welcome.rs) qui ne ciblent que les
/// streamers enregistrés (Twitch), le bandeau s'affiche pour TOUT le monde :
/// au premier message d'un viewer dans la session live, un bandeau s'affiche
/// en bas (ou en haut, configurable) de l'écran de diffusion.
///
/// Flux :
///   1. Module chat (twitch/youtube/kick/tiktok) appelle `on_message(...)`.
///   2. Si config active + viewer pas encore vu cette session → ajoute à `seen`
///      + émet `{"type":"bandeau-premier-message-play",...}` sur chat_tx
///        (forwardé vers diffusion.html :4321) + démarre un timer.
///   3. Timer (duree_ms) → émet `bandeau-premier-message-stop`.
///
/// Persistance : `data/bandeau_premier_message.json` (config uniquement).
/// Le seen set est en RAM (reset au reboot ou via reset_session).
///
/// Couleurs : la résolution des couleurs du cadre widget se fait côté
/// diffusion.html (qui reçoit le snapshot scène avec cadreWidget). Rust
/// n'émet que la config (position/duree) + le payload du message.
use crate::config::data_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

/// Jeton d'annulation du timer courant.
type TimerCancel = Arc<AtomicBool>;

/// Configuration du bandeau premier message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandeauConfig {
    pub actif: bool,
    /// Durée d'affichage en millisecondes.
    #[serde(default = "default_duree_ms")]
    pub duree_ms: u64,
    /// Position : "bas" (défaut) ou "haut".
    #[serde(default = "default_position")]
    pub position: String,
}

fn default_duree_ms() -> u64 {
    6000
}

fn default_position() -> String {
    "bas".to_string()
}

impl Default for BandeauConfig {
    fn default() -> Self {
        Self {
            actif: true,
            duree_ms: default_duree_ms(),
            position: default_position(),
        }
    }
}

/// État du bandeau (pour l'UI dashboard).
#[derive(Debug, Clone, Serialize)]
pub struct BandeauEtat {
    pub actif: bool,
    pub duree_ms: u64,
    pub position: String,
}

/// Format du fichier bandeau_premier_message.json sur disque.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BandeauFile {
    #[serde(default)]
    config: BandeauConfig,
}

/// État partagé bandeau. Clonable (Arc internes).
#[derive(Clone)]
pub struct BandeauState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<BandeauConfig>>,
    /// Viewers déjà vus cette session : clé = `plateforme_pseudo` (lowercase).
    seen: Arc<Mutex<HashSet<String>>>,
    /// Jeton d'annulation du timer courant.
    timer_cancel: Arc<Mutex<TimerCancel>>,
}

impl BandeauState {
    /// Crée l'état + charge la config depuis le disque.
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let file = load_file(&app).unwrap_or_default();
        Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(file.config)),
            seen: Arc::new(Mutex::new(HashSet::new())),
            timer_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
        }
    }

    // ===== Détection 1er message =====

    /// Appelé par les modules chat à chaque message.
    /// Si config active + viewer pas vu cette session → émet play + timer.
    pub fn on_message(
        &self,
        plateforme: &str,
        pseudo: &str,
        display_name: &str,
        avatar: Option<String>,
        texte: &str,
    ) {
        // 1. Config active ?
        if !self.config.lock().unwrap().actif {
            return;
        }

        // 2. Pas déjà vu cette session ?
        let seen_key = format!("{}_{}", plateforme, pseudo.to_lowercase());
        {
            let mut seen = self.seen.lock().unwrap();
            if seen.contains(&seen_key) {
                return;
            }
            seen.insert(seen_key);
        }

        self.jouer(display_name, texte, avatar, plateforme);
    }

    /// Émet bandeau-premier-message-play + démarre le timer d'auto-hide.
    fn jouer(&self, display_name: &str, texte: &str, avatar: Option<String>, plateforme: &str) {
        // Annuler le timer précédent (si un bandeau tournait).
        {
            let cancel = self.timer_cancel.lock().unwrap();
            cancel.store(true, Ordering::SeqCst);
        }

        // Pousser la config avant le play (position/duree courante).
        self.emit_config_ws();

        // Émettre bandeau-premier-message-play sur le WS :4321.
        let msg = serde_json::json!({
            "type": "bandeau-premier-message-play",
            "viewer": {
                "display_name": display_name,
                "avatar": avatar,
                "plateforme": plateforme,
            },
            "message": texte,
        });
        let _ = self.chat_tx.send(msg.to_string());

        // Démarrer le timer → bandeau-premier-message-stop.
        let duree_ms = self.config.lock().unwrap().duree_ms.max(1000);
        let state = self.clone();
        let new_cancel = Arc::new(AtomicBool::new(false));
        *self.timer_cancel.lock().unwrap() = new_cancel.clone();

        tauri::async_runtime::spawn(async move {
            let cancel = new_cancel;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(duree_ms)) => {
                    if !cancel.load(Ordering::SeqCst) {
                        let msg = serde_json::json!({ "type": "bandeau-premier-message-stop" });
                        let _ = state.chat_tx.send(msg.to_string());
                    }
                }
                _ = await_timer_cancel(&cancel) => {}
            }
        });

        self.emit_etat();
    }

    // ===== Test manuel (modale Interaction viewer) =====

    /// Lance un bandeau côté diffusion SANS passer par la détection (test manuel).
    pub fn tester(&self, display_name: &str, message: &str) -> Result<(), String> {
        self.jouer(display_name, message, None, "twitch");
        Ok(())
    }

    // ===== Contrôles =====

    /// Met à jour la config (merge partiel) + persiste + émet config WS + état.
    pub fn set_config(&self, actif: Option<bool>, duree_ms: Option<u64>, position: Option<String>) -> Result<(), String> {
        {
            let mut cfg = self.config.lock().unwrap();
            if let Some(a) = actif { cfg.actif = a; }
            if let Some(d) = duree_ms { cfg.duree_ms = d.max(500); }
            if let Some(p) = position {
                if p == "bas" || p == "haut" {
                    cfg.position = p;
                }
            }
        }
        self.save_to_disk()?;
        self.emit_config_ws();
        self.emit_etat();
        // Une SEULE lecture lock() → clone (plusieurs .lock() du même Mutex
        // dans une même expression = auto-deadlock, cf. position_overlay.rs).
        Ok(())
    }

    /// Stop immédiat du bandeau courant (annule timer + émet stop).
    pub fn stop(&self) {
        self.timer_cancel.lock().unwrap().store(true, Ordering::SeqCst);
        let msg = serde_json::json!({ "type": "bandeau-premier-message-stop" });
        let _ = self.chat_tx.send(msg.to_string());
    }

    /// Reset le seen set (nouveau stream → tous les viewers redeviennent éligibles).
    pub fn reset_session(&self) {
        self.seen.lock().unwrap().clear();
    }

    // ===== État (pour l'UI dashboard) =====

    /// Retourne l'état courant (snapshot config).
    pub fn etat(&self) -> BandeauEtat {
        let cfg = self.config.lock().unwrap();
        BandeauEtat {
            actif: cfg.actif,
            duree_ms: cfg.duree_ms,
            position: cfg.position.clone(),
        }
    }

    /// Émet l'état vers le dashboard (event Tauri bandeau:etat).
    fn emit_etat(&self) {
        let etat = self.etat();
        let _ = self.app.emit("bandeau:etat", &etat);
    }

    /// Émet bandeau-premier-message-config sur le WS :4321 (mise à jour live
    /// côté diffusion : position + durée).
    fn emit_config_ws(&self) {
        let cfg = self.config.lock().unwrap();
        let msg = serde_json::json!({
            "type": "bandeau-premier-message-config",
            "config": {
                "duree_ms": cfg.duree_ms,
                "position": cfg.position,
            },
        });
        let _ = self.chat_tx.send(msg.to_string());
    }

    // ===== Persistance =====

    /// Sauve la config sur disque (bandeau_premier_message.json).
    fn save_to_disk(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let file = BandeauFile { config: cfg };
        let dir = data_dir(&self.app)?;
        let path = dir.join("bandeau_premier_message.json");
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("Serialize bandeau_premier_message.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire bandeau_premier_message.json: {}", e))?;
        Ok(())
    }
}

// ===== Fonctions libres =====

/// Charge le fichier bandeau_premier_message.json depuis le disque.
fn load_file(app: &AppHandle) -> Result<BandeauFile, String> {
    let dir = data_dir(app)?;
    let path = dir.join("bandeau_premier_message.json");
    if !path.exists() {
        return Ok(BandeauFile::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire bandeau_premier_message.json: {}", e))?;
    let file: BandeauFile = serde_json::from_str(&content)
        .map_err(|e| format!("Parse bandeau_premier_message.json: {}", e))?;
    Ok(file)
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
