/// Squelette de position unifié — source de vérité unique pour la position
/// et la taille des overlays "clip de bienvenue" et "alertes" côté diffusion.
///
/// Avant, welcome.rs et alertes.rs avaient chacun leur `OverlayConfig` (deux
/// fichiers JSON, deux réglages séparés dans la modale Interactions chat).
/// Désormais, un seul réglage dans l'onglet "Squelette de position" pilote les
/// deux overlays via ce state partagé.
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

/// État partagé du squelette de position. Clonable (Arc internes) — passé aux
/// commandes Tauri et lu par welcome.rs / alertes.rs via `app.state::<Self>()`.
#[derive(Clone)]
pub struct PositionOverlayState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<PositionOverlayConfig>>,
}

impl PositionOverlayState {
    /// Crée l'état + charge `position_overlay.json` (absent → défaut).
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let config = load_file(&app).unwrap_or_default();
        let state = Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(config)),
        };
        // NB : une SEULE lecture lock() → clone. Plusieurs .lock() du même
        // Mutex dans une même expression (ex. args d'un eprintln!) gardent le
        // 1er MutexGuard vivant jusqu'à la fin du statement → auto-deadlock
        // du thread principal (setup Tauri) = écran noir figé au boot.
        let c = state.config.lock().unwrap().clone();
        eprintln!(
            "[PositionOverlay] init : x={} y={} {}x{}",
            c.x, c.y, c.largeur, c.hauteur
        );
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
        eprintln!(
            "[PositionOverlay] set x={} y={} {}x{}",
            cfg.x, cfg.y, cfg.largeur, cfg.hauteur
        );
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
