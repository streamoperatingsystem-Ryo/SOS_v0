//! Registre du moteur — mode actif, persistance, commandes Tauri.
//!
//! Le registre ne pilote encore aucun moteur : il expose l'état du mode et
//! refuse proprement une bascule vers un natif non branché (lot 1.4).

pub mod mode;
pub mod obs;

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, State};

use mode::{verifier_bascule, Mode};
use obs::MoteurObs;

/// État partagé du registre moteur.
pub struct RegistreMoteur {
    mode: Mutex<Mode>,
    /// Le moteur natif est-il branché ? (lot 1.4 : non)
    natif_disponible: bool,
    /// Adaptateur OBS du contrat `Moteur` (lot 1.3).
    obs: MoteurObs,
}

impl RegistreMoteur {
    pub fn nouveau(mode_initial: Mode) -> Self {
        Self {
            mode: Mutex::new(mode_initial),
            natif_disponible: false,
            obs: MoteurObs::nouveau(),
        }
    }

    pub fn mode(&self) -> Mode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn obs(&self) -> &MoteurObs {
        &self.obs
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

/// Diagnostic du moteur — valeurs honnêtes : `None` (`N/A`) tant que le natif
/// n'est pas branché ; jamais de 0 inventé (plan §10.1).
#[derive(Serialize, Clone)]
pub struct DiagMoteur {
    pub actif: bool,
    pub mode: String,
    pub natif_disponible: bool,
    pub adaptateur: Option<String>,
    pub encodeur: Option<String>,
    pub resolution: Option<(u32, u32)>,
    pub fps: Option<u32>,
}

#[tauri::command]
pub fn moteur_diag(registre: State<'_, RegistreMoteur>) -> DiagMoteur {
    DiagMoteur {
        actif: false,
        mode: registre.mode().nom().to_string(),
        natif_disponible: registre.natif_disponible,
        adaptateur: None,
        encodeur: None,
        resolution: None,
        fps: None,
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

/// Connexion obs-websocket pour l'adaptateur `MoteurObs` (lot 1.3).
/// Le mot de passe reste en mémoire du backend : jamais persisté ni loggé.
/// Appelée par le frontend avant toute bascule ou action moteur (lot 1.5).
#[tauri::command]
pub fn moteur_obs_connexion(
    registre: State<'_, RegistreMoteur>,
    host: String,
    port: u16,
    password: String,
) {
    registre.obs().configurer_connexion(host, port, password);
}
