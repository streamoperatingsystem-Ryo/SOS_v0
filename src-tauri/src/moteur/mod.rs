//! Registre du moteur — mode actif, persistance, commandes Tauri.
//!
//! Le registre ne pilote encore aucun moteur : il expose l'état du mode et
//! refuse proprement une bascule vers un natif non branché (lot 1.4).

pub mod mode;

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, State};

use mode::{verifier_bascule, Mode};

/// État partagé du registre moteur.
pub struct RegistreMoteur {
    mode: Mutex<Mode>,
    /// Le moteur natif est-il branché ? (lot 1.4 : non)
    natif_disponible: bool,
}

impl RegistreMoteur {
    pub fn nouveau(mode_initial: Mode) -> Self {
        Self {
            mode: Mutex::new(mode_initial),
            natif_disponible: false,
        }
    }

    pub fn mode(&self) -> Mode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// État exposé au frontend.
#[derive(Serialize, Clone)]
pub struct EtatMoteur {
    pub mode: String,
    pub natif_disponible: bool,
    pub obs_disponible: bool,
}

#[tauri::command]
pub fn moteur_etat(registre: State<'_, RegistreMoteur>) -> EtatMoteur {
    EtatMoteur {
        mode: registre.mode().nom().to_string(),
        natif_disponible: registre.natif_disponible,
        obs_disponible: true,
    }
}

/// Bascule de mode. Refusée proprement si le natif n'est pas branché ou si
/// une sortie OBS est active (jamais de coupure implicite, plan §11.2).
#[tauri::command]
pub fn moteur_basculer(
    app: AppHandle,
    registre: State<'_, RegistreMoteur>,
    cible: String,
    live_actif: bool,
) -> Result<String, String> {
    let cible = Mode::depuis_nom(&cible).ok_or_else(|| format!("mode inconnu : {cible}"))?;
    let actuel = registre.mode();
    let nouveau = verifier_bascule(actuel, cible, live_actif).map_err(|e| e.to_string())?;
    if nouveau != actuel {
        *registre.mode.lock().unwrap_or_else(|e| e.into_inner()) = nouveau;
        crate::config::sauver_mode_moteur(&app, nouveau.nom())?;
    }
    Ok(nouveau.nom().to_string())
}
