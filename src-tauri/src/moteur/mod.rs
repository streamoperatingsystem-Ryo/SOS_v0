//! Registre du moteur — mode actif, persistance, commandes Tauri.
//!
//! Le registre porte les deux moteurs : l'adaptateur OBS (lot 1.3) et le
//! moteur natif (lot 1.4). Il expose l'état du mode, les capacités du moteur
//! **réellement actif** et les commandes de sortie native.

pub mod mode;
pub mod obs;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use sos_moteur_api::{Capacites, Conteneur, Moteur, MoteurEtat, SortieCfg};
use sos_moteur_natif::moteur::MoteurNatif;
use tauri::{AppHandle, Manager, State};

use mode::{verifier_bascule, Mode};
use obs::MoteurObs;

/// Session d'enregistrement native en cours (chemin + fps figé).
#[derive(Debug, Clone)]
pub struct SessionNatif {
    pub chemin: PathBuf,
    pub fps: u32,
}

/// État partagé du registre moteur.
pub struct RegistreMoteur {
    mode: Mutex<Mode>,
    /// Le moteur natif est-il branché ? (lot 1.4 : oui)
    natif_disponible: bool,
    /// Adaptateur OBS du contrat `Moteur` (lot 1.3).
    obs: MoteurObs,
    /// Moteur natif — capture, composition, encodage, audio.
    natif: MoteurNatif,
    /// Dernière session native démarrée (effacée à l'arrêt).
    session: Mutex<Option<SessionNatif>>,
}

impl RegistreMoteur {
    pub fn nouveau(mode_initial: Mode) -> Self {
        Self {
            mode: Mutex::new(mode_initial),
            natif_disponible: true,
            obs: MoteurObs::nouveau(),
            natif: MoteurNatif::nouveau(),
            session: Mutex::new(None),
        }
    }

    pub fn mode(&self) -> Mode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn obs(&self) -> &MoteurObs {
        &self.obs
    }

    pub fn natif(&self) -> &MoteurNatif {
        &self.natif
    }

    /// Une sortie native tourne-t-elle ? (verrou court, jamais imbriqué)
    pub fn natif_actif(&self) -> bool {
        self.natif.en_cours()
    }

    /// Session native en cours (chemin affiché par l'UI).
    pub fn session(&self) -> Option<SessionNatif> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn definir_session(&self, session: SessionNatif) {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
    }

    /// Teardown : joint le fil moteur, libère NVENC, finalise le fichier.
    /// Idempotent — appelé à l'arrêt de l'app (`RunEvent::Exit`).
    pub fn arreter_natif(&self) {
        self.natif.arreter();
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// État exposé au frontend.
#[derive(Serialize, Clone)]
pub struct EtatMoteur {
    pub mode: String,
    pub natif_disponible: bool,
    pub obs_disponible: bool,
    /// Capacités du moteur actif — ids stables, libellés par l'i18n.
    pub capacites: Vec<&'static str>,
}

/// Traduit les drapeaux en identifiants stables (contrat avec `moteur.cap.*`).
fn capacites_actives(c: &Capacites) -> Vec<&'static str> {
    let drapeaux = [
        (c.capture_ecran, "capture_ecran"),
        (c.capture_fenetre, "capture_fenetre"),
        (c.capture_jeu, "capture_jeu"),
        (c.camera, "camera"),
        (c.fond_vert, "fond_vert"),
        (c.masque_shader, "masque_shader"),
        (c.cadres_animes, "cadres_animes"),
        (c.audio, "audio"),
        (c.audio_par_app, "audio_par_app"),
        (c.roi_encode, "roi_encode"),
        (c.multi_format, "multi_format"),
        (c.hdr, "hdr"),
        (c.srt, "srt"),
        (c.ndi, "ndi"),
    ];
    drapeaux
        .iter()
        .filter(|(actif, _)| *actif)
        .map(|(_, id)| *id)
        .collect()
}

#[tauri::command]
pub fn moteur_etat(registre: State<'_, RegistreMoteur>) -> EtatMoteur {
    let mode = registre.mode();
    let capacites = match mode {
        Mode::Obs => registre.obs().capacites(),
        Mode::Natif => registre.natif().capacites(),
    };
    EtatMoteur {
        mode: mode.nom().to_string(),
        natif_disponible: registre.natif_disponible,
        obs_disponible: true,
        capacites: capacites_actives(&capacites),
    }
}

/// Diagnostic du moteur — valeurs honnêtes : `None` (`N/A`) tant que rien
/// n'a démarré ; jamais de 0 inventé (plan §10.1).
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
    let mode = registre.mode();
    let actif = match mode {
        Mode::Obs => registre.obs().etat().est_actif(),
        Mode::Natif => registre.natif().etat().est_actif(),
    };
    let (encodeur, resolution, fps) = match mode {
        Mode::Natif => {
            let encodeur = registre.natif().encodeur().map(|e| e.nom);
            let instantane = registre.natif().instantane();
            let resolution = (instantane.resolution.0 > 0).then_some(instantane.resolution);
            (encodeur, resolution, registre.session().map(|s| s.fps))
        }
        Mode::Obs => (None, None, None),
    };
    DiagMoteur {
        actif,
        mode: mode.nom().to_string(),
        natif_disponible: registre.natif_disponible,
        adaptateur: Some(mode.nom().to_string()),
        encodeur,
        resolution,
        fps,
    }
}

/// État de la sortie native — ce que l'UI affiche pendant l'enregistrement.
#[derive(Serialize, Clone)]
pub struct EtatSortieNatif {
    pub actif: bool,
    pub chemin: Option<String>,
    pub kbps: u32,
    pub frames_perdues: u64,
    pub encodeur: Option<String>,
    pub degradation: Option<String>,
    /// Erreur **terminale** du moteur natif (thread arrêté sur erreur).
    /// Distincte de `degradation` : une dégradation n'arrête pas la sortie.
    pub erreur: Option<String>,
}

#[tauri::command]
pub fn moteur_sortie_etat(registre: State<'_, RegistreMoteur>) -> EtatSortieNatif {
    let actif = registre.natif_actif();
    let etat = registre.natif().etat();
    let erreur = match &etat {
        MoteurEtat::Erreur(m) => Some(m.clone()),
        _ => None,
    };
    let instantane = registre.natif().instantane();
    EtatSortieNatif {
        actif,
        // Le chemin n'est annoncé que tant que le fil tourne : après une
        // erreur terminale, aucun fichier exploitable n'a été écrit.
        chemin: if actif {
            registre.session().map(|s| s.chemin.display().to_string())
        } else {
            None
        },
        kbps: if actif { instantane.kbps } else { 0 },
        frames_perdues: instantane.frames_perdues,
        encodeur: registre.natif().encodeur().map(|e| e.nom),
        degradation: instantane.raison_degradation,
        erreur,
    }
}

/// Normalise le chemin choisi par le dialogue : extension `.mp4` si absente,
/// dossier parent créé au besoin. Aucun chemin par défaut inventé.
fn normaliser_chemin(chemin: &str) -> Result<PathBuf, String> {
    let brut = chemin.trim();
    if brut.is_empty() {
        return Err("chemin vide".to_string());
    }
    let mut chemin = PathBuf::from(brut);
    if chemin.extension().is_none() {
        chemin.set_extension("mp4");
    }
    if let Some(parent) = chemin.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("dossier de sortie : {e}"))?;
    }
    Ok(chemin)
}

/// Démarre un enregistrement natif. Bloquant (GPU + encodeur) : exécuté hors
/// du thread UI pour ne jamais geler la fenêtre.
#[tauri::command]
pub async fn moteur_sortie_demarrer(app: AppHandle, chemin: String) -> Result<String, String> {
    let chemin = normaliser_chemin(&chemin)?;
    tauri::async_runtime::spawn_blocking(move || demarrer_bloquant(&app, &chemin))
        .await
        .map_err(|e| format!("tâche de démarrage : {e}"))?
}

fn demarrer_bloquant(app: &AppHandle, chemin: &Path) -> Result<String, String> {
    let registre = app
        .try_state::<RegistreMoteur>()
        .ok_or("registre moteur absent")?;
    if registre.mode() != Mode::Natif {
        return Err("la sortie native exige le mode Rust".to_string());
    }
    if registre.natif_actif() {
        return Err("un enregistrement natif est déjà en cours".to_string());
    }
    let scene = app
        .try_state::<crate::scenes::ScenesState>()
        .ok_or("état des scènes absent")?
        .scene
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let base = crate::config::data_dir(app)?;
    registre
        .natif()
        .configurer_scene(&scene, &HashMap::new(), &base)
        .map_err(|e| e.to_string())?;

    let canvas = (scene.canvasW, scene.canvasH);
    let mut cfg = SortieCfg::fichier(chemin, Conteneur::Mp4);
    cfg.canvas = canvas;
    registre
        .natif()
        .demarrer_sortie(cfg)
        .map_err(|e| e.to_string())?;
    registre.definir_session(SessionNatif {
        chemin: chemin.to_path_buf(),
        fps: 60,
    });
    Ok(chemin.display().to_string())
}

/// Arrête l'enregistrement natif et retourne le chemin finalisé (`None` si
/// rien ne tournait). Le muxage audio se fait pendant cet appel (~1 s) :
/// exécuté hors du thread UI.
#[tauri::command]
pub async fn moteur_sortie_arreter(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let registre = app
            .try_state::<RegistreMoteur>()
            .ok_or("registre moteur absent")?;
        let chemin = registre.session().map(|s| s.chemin.display().to_string());
        registre.arreter_natif();
        Ok::<Option<String>, String>(chemin)
    })
    .await
    .map_err(|e| format!("tâche d'arrêt : {e}"))?
}

/// Bascule de mode. Refusée proprement si une sortie OBS **ou** native est
/// active (jamais de coupure implicite, plan §11.2).
#[tauri::command]
pub fn moteur_basculer(
    app: AppHandle,
    registre: State<'_, RegistreMoteur>,
    cible: String,
    live_actif: bool,
) -> Result<String, String> {
    let cible = Mode::depuis_nom(&cible).ok_or_else(|| format!("mode inconnu : {cible}"))?;
    let actuel = registre.mode();
    let natif_actif = registre.natif_actif();
    let nouveau =
        verifier_bascule(actuel, cible, live_actif, natif_actif).map_err(|e| e.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacites_aucune_ne_produit_aucun_id() {
        assert!(capacites_actives(&Capacites::aucune()).is_empty());
    }

    #[test]
    fn capacites_ids_stables_pour_l_i18n() {
        let c = Capacites {
            camera: true,
            audio: true,
            srt: true,
            ..Capacites::aucune()
        };
        assert_eq!(capacites_actives(&c), vec!["camera", "audio", "srt"]);
    }

    #[test]
    fn registre_natif_disponible_et_inactif_au_depart() {
        let registre = RegistreMoteur::nouveau(Mode::Obs);
        assert!(registre.natif_disponible);
        assert!(!registre.natif_actif());
        assert!(registre.session().is_none());
        assert_eq!(registre.natif().encodeur(), None);
    }

    #[test]
    fn capacites_du_moteur_actif() {
        let registre = RegistreMoteur::nouveau(Mode::Obs);
        // OBS : capture de jeu ; natif : pas de capture de jeu.
        assert!(registre.obs().capacites().capture_jeu);
        assert!(!registre.natif().capacites().capture_jeu);
        assert!(registre.obs().capacites().audio);
        assert!(registre.natif().capacites().audio);
    }

    #[test]
    fn chemin_normalise_ajoute_mp4() {
        let dossier = std::env::temp_dir().join("sos-moteur-chemin");
        let p = normaliser_chemin(&dossier.join("sortie").display().to_string())
            .expect("chemin");
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("mp4"));
        assert!(normaliser_chemin("   ").is_err());
    }

    #[test]
    fn arret_idempotent_sans_session() {
        let registre = RegistreMoteur::nouveau(Mode::Natif);
        registre.arreter_natif();
        registre.arreter_natif();
        assert!(!registre.natif_actif());
        assert!(registre.session().is_none());
    }
}
