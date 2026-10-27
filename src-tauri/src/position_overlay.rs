/// Squelette de position unifié — source de vérité unique pour la position
/// et la taille des overlays "clip de bienvenue" et "alertes" côté diffusion.
///
/// Avant, welcome.rs et alertes.rs avaient chacun leur `OverlayConfig` (deux
/// fichiers JSON, deux réglages séparés dans la modale Interactions chat).
/// Désormais, un seul réglage dans l'onglet "Squelette de position" pilote les
/// deux overlays via ce state partagé.
///
/// Le chrono 2 (deuxième chronomètre déclenché par une touche du pad
/// numérique) a son PROPRE squelette dédié, indépendant du squelette partagé :
/// persisté dans `position_chrono.json`, poussé vers la diffusion via la
/// frame WS `chrono2-pos`.
///
/// Le pop de validation des splits (speedrun) a AUSSI son squelette dédié :
/// il se déclenche à chaque split pendant une run — fréquence incompatible
/// avec le cadre partagé des alertes (collisions visuelles quasi garanties).
/// Persisté `position_splitpop.json`, poussé via la frame WS `splitpop-pos`.
/// Resync `splitpop-resync` à la (re)connexion WS car les events speedrun
/// ne portent pas la position (contrairement aux frames `chrono2-etat`).
///
/// Flux :
///   1. Modale (onglet "Squelette de position") → `position_overlay_set` →
///      `set()` persiste `position_overlay.json` + émet WS `position-overlay-config`
///      (forwardé vers diffusion.html :4321) + émet event Tauri `position-overlay:etat`.
///   2. `diffusion.html` reçoit `position-overlay-config` → applique la position
///      au root welcome ET au root alerte (les deux overlays se repositionnent
///      en live).
///   3. Au moment d'un play welcome/alerte, `welcome.rs` et `alertes.rs`
///      ré-émettent leur propre config WS (`welcome-clip-config` / `alerte-config`)
///      en lisant la position depuis ce state — garantit que la position est
///      appliquée même si diffusion reconnecte après un changement.
use crate::config::data_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

/// Position/taille de l'overlay unifié (pixels canvas diffusion).
/// Partagée par les clips de bienvenue et les alertes. La hauteur est
/// "auto" côté diffusion (la carte s'ajuste à son contenu) — ce champ est
/// une référence visuelle pour le squelette de la modale uniquement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionOverlayConfig {
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
}

impl Default for PositionOverlayConfig {
    fn default() -> Self {
        // Reprend le défaut welcome actuel (bas-droite 320×352 sur 1920×1080).
        Self {
            x: 1480.0,
            y: 580.0,
            largeur: 320.0,
            hauteur: 352.0,
        }
    }
}

/// Squelette dédié du chrono 2 — défaut haut-centre (référence 1920×1080).
pub fn defaut_chrono() -> PositionOverlayConfig {
    PositionOverlayConfig {
        x: 760.0,
        y: 80.0,
        largeur: 400.0,
        hauteur: 120.0,
    }
}

/// Squelette dédié du pop de validation des splits — défaut haut-centre
/// (reprend l'ancien défaut JS `splitPopGeo`, référence 1920×1080).
/// Le pop se déclenche à CHAQUE split pendant une run : un squelette dédié
/// évite les collisions avec les alertes qui partagent l'autre cadre.
pub fn defaut_splitpop() -> PositionOverlayConfig {
    PositionOverlayConfig {
        x: 660.0,
        y: 60.0,
        largeur: 600.0,
        hauteur: 120.0,
    }
}

/// État partagé du squelette de position. Clonable (Arc internes) — passé aux
/// commandes Tauri et lu par welcome.rs / alertes.rs via `app.state::<Self>()`.
#[derive(Clone)]
pub struct PositionOverlayState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<PositionOverlayConfig>>,
    /// Squelette DÉDIÉ du chrono 2 — indépendant du squelette partagé
    /// (welcome/alertes/commandes/pad). Persisté `position_chrono.json`.
    config_chrono: Arc<Mutex<PositionOverlayConfig>>,
    /// Squelette DÉDIÉ du pop de validation des splits (speedrun) —
    /// indépendant du squelette partagé (le pop arrive à chaque split et
    /// pouvait chevaucher les alertes). Persisté `position_splitpop.json`,
    /// poussé vers la diffusion via la frame WS `splitpop-pos`.
    config_splitpop: Arc<Mutex<PositionOverlayConfig>>,
}

impl PositionOverlayState {
    /// Crée l'état + charge `position_overlay.json` (absent → défaut).
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let config = load_file(&app).unwrap_or_default();
        let config_chrono = load_file_chrono(&app).unwrap_or_else(|_| defaut_chrono());
        let config_splitpop =
            load_file_splitpop(&app).unwrap_or_else(|_| defaut_splitpop());
        let state = Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(config)),
            config_chrono: Arc::new(Mutex::new(config_chrono)),
            config_splitpop: Arc::new(Mutex::new(config_splitpop)),
        };
        // NB : une SEULE lecture lock() → clone. Plusieurs .lock() du même
        // Mutex dans une même expression (ex. args d'un eprintln!) gardent le
        // 1er MutexGuard vivant jusqu'à la fin du statement → auto-deadlock
        // du thread principal (setup Tauri) = écran noir figé au boot.
        state
    }

    /// Snapshot courant (pour l'UI dashboard).
    pub fn etat(&self) -> PositionOverlayConfig {
        self.config.lock().unwrap().clone()
    }

    /// Remplace la config + persiste + push WS diffusion + émet event Tauri.
    pub fn set(&self, cfg: PositionOverlayConfig) -> Result<(), String> {
        {
            let mut c = self.config.lock().unwrap();
            *c = cfg.clone();
        }
        self.save_to_disk()?;
        self.emit_ws();
        self.emit_etat();
        Ok(())
    }

    /// Émet `position-overlay-config` sur le WS :4321 (forwardé vers
    /// diffusion.html, qui applique la position aux deux overlays en live).
    pub fn emit_ws(&self) {
        let cfg = self.config.lock().unwrap().clone();
        let msg = serde_json::json!({
            "type": "position-overlay-config",
            "config": {
                "x": cfg.x,
                "y": cfg.y,
                "largeur": cfg.largeur,
                "hauteur": cfg.hauteur,
            },
        });
        let _ = self.chat_tx.send(msg.to_string());
    }

    /// Émet l'état vers le dashboard (event Tauri `position-overlay:etat`).
    pub fn emit_etat(&self) {
        let cfg = self.etat();
        let _ = self.app.emit("position-overlay:etat", &cfg);
    }

    /// Récupère le state depuis l'app (utilisé par welcome.rs / alertes.rs).
    pub fn from_app(app: &AppHandle) -> Option<Self> {
        app.try_state::<Self>().map(|s| s.inner().clone())
    }

    // ===== Chrono 2 — squelette dédié =====

    /// Snapshot du squelette chrono 2 (pour l'UI dashboard).
    pub fn etat_chrono(&self) -> PositionOverlayConfig {
        self.config_chrono.lock().unwrap().clone()
    }

    /// Remplace le squelette chrono 2 + persiste (`position_chrono.json`)
    /// + push WS `chrono2-pos` (repositionnement live de la diffusion)
    /// + émet event Tauri `position-chrono:etat`.
    pub fn set_chrono(&self, cfg: PositionOverlayConfig) -> Result<(), String> {
        {
            let mut c = self.config_chrono.lock().unwrap();
            *c = cfg.clone();
        }
        self.save_chrono_to_disk()?;
        self.emit_ws_chrono();
        self.emit_etat_chrono();
        Ok(())
    }

    /// Émet `chrono2-pos` sur le WS :4321 (position du squelette chrono 2 —
    /// la diffusion l'applique à la racine overlay chrono, visible ou non).
    pub fn emit_ws_chrono(&self) {
        let cfg = self.config_chrono.lock().unwrap().clone();
        let msg = serde_json::json!({
            "type": "chrono2-pos",
            "config": {
                "x": cfg.x,
                "y": cfg.y,
                "largeur": cfg.largeur,
                "hauteur": cfg.hauteur,
            },
        });
        let _ = self.chat_tx.send(msg.to_string());
    }

    /// Émet l'état chrono vers le dashboard (event Tauri `position-chrono:etat`).
    pub fn emit_etat_chrono(&self) {
        let cfg = self.etat_chrono();
        let _ = self.app.emit("position-chrono:etat", &cfg);
    }

    // ===== Pop de validation des splits — squelette dédié =====

    /// Snapshot du squelette splitpop (pour l'UI dashboard).
    pub fn etat_splitpop(&self) -> PositionOverlayConfig {
        self.config_splitpop.lock().unwrap().clone()
    }

    /// Remplace le squelette splitpop + persiste (`position_splitpop.json`)
    /// + push WS `splitpop-pos` (repositionnement live de la diffusion)
    /// + émet event Tauri `position-splitpop:etat`.
    pub fn set_splitpop(&self, cfg: PositionOverlayConfig) -> Result<(), String> {
        {
            let mut c = self.config_splitpop.lock().unwrap();
            *c = cfg.clone();
        }
        self.save_splitpop_to_disk()?;
        self.emit_ws_splitpop();
        self.emit_etat_splitpop();
        Ok(())
    }

    /// Émet `splitpop-pos` sur le WS :4321 (position du squelette splitpop —
    /// la diffusion l'applique au root du pop, visible ou non).
    pub fn emit_ws_splitpop(&self) {
        let cfg = self.config_splitpop.lock().unwrap().clone();
        let msg = serde_json::json!({
            "type": "splitpop-pos",
            "config": {
                "x": cfg.x,
                "y": cfg.y,
                "largeur": cfg.largeur,
                "hauteur": cfg.hauteur,
            },
        });
        let _ = self.chat_tx.send(msg.to_string());
    }

    /// Émet l'état splitpop vers le dashboard (event Tauri
    /// `position-splitpop:etat`).
    pub fn emit_etat_splitpop(&self) {
        let cfg = self.etat_splitpop();
        let _ = self.app.emit("position-splitpop:etat", &cfg);
    }

    // ===== Persistance =====

    fn save_to_disk(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let path = dir.join("position_overlay.json");
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize position_overlay.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire position_overlay.json: {}", e))?;
        Ok(())
    }

    fn save_chrono_to_disk(&self) -> Result<(), String> {
        let cfg = self.config_chrono.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let path = dir.join("position_chrono.json");
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize position_chrono.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire position_chrono.json: {}", e))?;
        Ok(())
    }

    fn save_splitpop_to_disk(&self) -> Result<(), String> {
        let cfg = self.config_splitpop.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let path = dir.join("position_splitpop.json");
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize position_splitpop.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire position_splitpop.json: {}", e))?;
        Ok(())
    }
}

/// Charge `position_overlay.json` (absent → défaut).
fn load_file(app: &AppHandle) -> Result<PositionOverlayConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("position_overlay.json");
    if !path.exists() {
        return Ok(PositionOverlayConfig::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire position_overlay.json: {}", e))?;
    let cfg: PositionOverlayConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse position_overlay.json: {}", e))?;
    Ok(cfg)
}

/// Charge `position_chrono.json` (absent → défaut haut-centre).
fn load_file_chrono(app: &AppHandle) -> Result<PositionOverlayConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("position_chrono.json");
    if !path.exists() {
        return Ok(defaut_chrono());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire position_chrono.json: {}", e))?;
    let cfg: PositionOverlayConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse position_chrono.json: {}", e))?;
    Ok(cfg)
}

/// Charge `position_splitpop.json` (absent → défaut haut-centre 600×120).
fn load_file_splitpop(app: &AppHandle) -> Result<PositionOverlayConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("position_splitpop.json");
    if !path.exists() {
        return Ok(defaut_splitpop());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire position_splitpop.json: {}", e))?;
    let cfg: PositionOverlayConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Parse position_splitpop.json: {}", e))?;
    Ok(cfg)
}

/// Re-émet la frame `splitpop-pos` vers la diffusion (resync à la
/// (re)connexion WS — les events speedrun ne portent PAS la position du
/// pop, contrairement aux frames `chrono2-etat` ; la diffusion envoie
/// `splitpop-resync` à l'ouverture). Appelée par server.rs. No-op si le
/// state n'est pas encore enregistré.
pub fn reemit_splitpop(app: &AppHandle) {
    let Some(state) = app.try_state::<PositionOverlayState>() else {
        return;
    };
    state.emit_ws_splitpop();
}

/// Test manuel du pop de validation des splits (bouton « Tester le rendu »
/// de l'onglet Squelette de position) : émet `splitpop-test` → la diffusion
/// affiche un pop factice au squelette dédié, via le même code path visuel
/// qu'un vrai split. No-op si le state n'est pas enregistré.
pub fn tester_splitpop(app: &AppHandle) {
    let Some(state) = app.try_state::<PositionOverlayState>() else {
        return;
    };
    let msg = serde_json::json!({ "type": "splitpop-test" });
    let _ = state.chat_tx.send(msg.to_string());
}
