/// Just Chatting — overlay non-destructif sur la diffusion :4321.
///
/// Pattern `ecran_attente.rs`, sans timer et sans JSON :
///   - `actif` est un état runtime, défaut false, NON persisté. OFF au boot.
///   - La scène persistée (`currentScene`, JSON disque) n'est jamais lue ni
///     écrite ici. Pas de `commitScene` / `update_scene` / `save_current`.
///   - Frame WS auto-réparatrice `just-chatting-etat` sur chat_tx.
///   - Resync : la diffusion envoie `just-chatting-resync` à la (re)connexion
///     WS → server.rs appelle `reemit()`. Différence volontaire avec
///     `ecran_attente::reemit` : si aucune frame n'a encore été émise, on
///     reconstruit et on envoie `actif: false` (pas un no-op). Une Browser
///     Source connectée avant le premier toggle doit recevoir l'état.
///   - Event Tauri `just-chatting:etat` → dashboard (pastille + store).
///
/// Formule de layout unique (fractions 0–1). La diffusion choisit paysage ou
/// portrait selon `scene.canvasW/H` et multiplie par le canvas réel au rendu.
/// Les fractions du preset embarqué `just-chatting.json` ne sont pas utilisées.
///
/// Piège Mutex non réentrant (AGENTS.md, ecran_attente.rs, position_overlay.rs) :
/// un seul `.lock()` par statement. Jamais deux locks imbriqués, jamais un
/// lock tenu pendant un `.send()` / `.emit()`.
use serde::Serialize;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

/// Rectangle en fractions du canvas (0–1). Converti en pixels côté diffusion.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct JcRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Paire caméra / chat pour une orientation.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct JcLayout {
    pub camera: JcRect,
    pub chat: JcRect,
}

/// Snapshot dashboard + charge utile de la frame WS.
///
/// Les deux orientations sont calculées ici (Rust est la seule implémentation
/// de la formule). La diffusion et le dashboard sélectionnent selon W/H :
/// paysage si `W >= H`, portrait si `H > W`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct JustChattingEtat {
    pub actif: bool,
    pub paysage: JcLayout,
    pub portrait: JcLayout,
}

/// État partagé. Clonable (Arc internes). Pas de fichier, pas de timer.
#[derive(Clone)]
pub struct JustChattingState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    actif: Arc<Mutex<bool>>,
    /// Dernière frame WS émise. `None` tant que personne n'a émis — `reemit`
    /// reconstruit alors une frame `actif: false`.
    derniere_frame: Arc<Mutex<Option<String>>>,
}

impl JustChattingState {
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        Self {
            app,
            chat_tx,
            actif: Arc::new(Mutex::new(false)),
            derniere_frame: Arc::new(Mutex::new(None)),
        }
    }

    /// Snapshot. Layouts constants — seul `actif` est lu sous lock.
    pub fn etat(&self) -> JustChattingEtat {
        let actif = *self.actif.lock().unwrap();
        JustChattingEtat {
            actif,
            paysage: layout_paysage(),
            portrait: layout_portrait(),
        }
    }

    /// Active ou coupe le mode. N'écrit rien sur disque, ne touche aucune scène.
    pub fn set_actif(&self, actif: bool) {
        *self.actif.lock().unwrap() = actif;
        self.emit_all();
    }

    /// Frame WS + event dashboard. Locks relâchés avant send/emit.
    fn emit_all(&self) {
        let frame = self.frame_json();
        *self.derniere_frame.lock().unwrap() = Some(frame.clone());
        let _ = self.chat_tx.send(frame);
        let etat = self.etat();
        let _ = self.app.emit("just-chatting:etat", etat);
    }

    /// Sérialise la frame. Un seul lock (lecture de `actif`).
    fn frame_json(&self) -> String {
        let etat = self.etat();
        json!({
            "type": "just-chatting-etat",
            "etat": etat,
        })
        .to_string()
    }
}

/// Resync diffusion. Envoie toujours une frame : la dernière connue, ou une
/// frame reconstruite `actif: false` si aucune n'a encore été émise.
pub fn reemit(app: &AppHandle) {
    let Some(state) = app.try_state::<JustChattingState>() else {
        return;
    };
    let frame = state.derniere_frame.lock().unwrap().clone();
    let frame = match frame {
        Some(f) => f,
        None => state.frame_json(),
    };
    // Mémoriser la frame reconstruite pour les resync suivants (un lock).
    *state.derniere_frame.lock().unwrap() = Some(frame.clone());
    let _ = state.chat_tx.send(frame);
}

// =============================================================================
// Formule unique
// =============================================================================

/// Paysage (`W >= H`).
///
/// `m = 0.04`, `g = 0.02`, `h = 0.78`, `y = (1 - h) / 2`
/// `inner = 1 - 2*m - g`
/// caméra : `x = m`, `w = 0.62 * inner`
/// chat   : `x = m + wCaméra + g`, `w = 0.38 * inner`
fn layout_paysage() -> JcLayout {
    let m = 0.04;
    let g = 0.02;
    let h = 0.78;
    let y = (1.0 - h) / 2.0;
    let inner = 1.0 - 2.0 * m - g;
    let w_camera = 0.62 * inner;
    let w_chat = 0.38 * inner;
    JcLayout {
        camera: JcRect {
            x: m,
            y,
            w: w_camera,
            h,
        },
        chat: JcRect {
            x: m + w_camera + g,
            y,
            w: w_chat,
            h,
        },
    }
}

/// Portrait (`H > W`). Même largeur, empilés.
///
/// Marges horizontales `m = 0.04`, gouttière verticale `g = 0.02`.
/// Hauteur utile `inner = 1 - 2*m - g` (mêmes marges verticales, pour que
/// haut / bas / centre soient partagés — le preset ne l'est pas).
/// Caméra 62 % de la hauteur utile au-dessus, chat 38 % en dessous.
fn layout_portrait() -> JcLayout {
    let m = 0.04;
    let g = 0.02;
    let inner = 1.0 - 2.0 * m - g;
    let w = 1.0 - 2.0 * m;
    let h_camera = 0.62 * inner;
    let h_chat = 0.38 * inner;
    JcLayout {
        camera: JcRect {
            x: m,
            y: m,
            w,
            h: h_camera,
        },
        chat: JcRect {
            x: m,
            y: m + h_camera + g,
            w,
            h: h_chat,
        },
    }
}
