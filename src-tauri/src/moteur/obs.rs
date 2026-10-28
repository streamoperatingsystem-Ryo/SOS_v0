//! Adaptateur OBS du contrat `Moteur` (lot 1.3).
//!
//! Il **enveloppe** `obs.rs` — il ne réimplémente rien : les sorties passent
//! par `obs::set_stream` / `obs::set_record` (Start/Stop explicites,
//! idempotents, état relu). Le trait `Moteur` est synchrone et non bloquant :
//! les méthodes rendent immédiatement, le travail part sur le runtime Tauri,
//! et l'état exposé suit la fin de la tâche.
//!
//! ⚠️ Règles figées : `arreter()` ne coupe **jamais** un live (plan §11.2) ;
//! aucun `RemoveInput` n'est appelé (règle caméra) ; le mot de passe OBS n'est
//! ni journalisé ni persisté (mémoire seulement, masqué dans `Debug`).

use std::fmt;
use std::sync::{Arc, Mutex};

use sos_moteur_api::{
    ApercuFrame, Capacites, ErreurMoteur, Moteur, MoteurEtat, SortieCfg, SortieKind, SinkCfg,
};

/// Paramètres de connexion obs-websocket (frontend → registre).
#[derive(Clone)]
pub struct ConnexionObs {
    pub host: String,
    pub port: u16,
    pub password: String,
}

impl fmt::Debug for ConnexionObs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnexionObs")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("password", &"<masqué>")
            .finish()
    }
}

/// Cache des sorties OBS — alimenté par la fin des tâches, jamais deviné.
#[derive(Default)]
struct EtatObs {
    diffusion: bool,
    enregistrement: bool,
    erreur: Option<String>,
}

/// Adaptateur OBS : implémente le contrat `Moteur` en déléguant à `obs`.
pub struct MoteurObs {
    connexion: Mutex<Option<ConnexionObs>>,
    etat: Arc<Mutex<EtatObs>>,
}

impl Default for MoteurObs {
    fn default() -> Self {
        Self::nouveau()
    }
}

impl MoteurObs {
    pub fn nouveau() -> Self {
        Self {
            connexion: Mutex::new(None),
            etat: Arc::new(Mutex::new(EtatObs::default())),
        }
    }

    /// Reçoit la connexion obs-websocket (commande `moteur_obs_connexion`).
    /// Le mot de passe reste en mémoire : jamais sur disque, jamais loggé.
    pub fn configurer_connexion(&self, host: String, port: u16, password: String) {
        *self.connexion.lock().unwrap_or_else(|e| e.into_inner()) = Some(ConnexionObs {
            host,
            port,
            password,
        });
    }

    fn connexion(&self) -> Option<ConnexionObs> {
        self.connexion
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Lance une action OBS et met le cache à jour à la fin (une seule prise
    /// du verrou par statement — piège Mutex documenté dans AGENTS.md).
    fn lancer(&self, kind: SortieKind, actif: bool) {
        let Some(connexion) = self.connexion() else {
            return;
        };
        let etat = Arc::clone(&self.etat);
        tauri::async_runtime::spawn(async move {
            let resultat = match kind {
                SortieKind::Diffusion => {
                    crate::obs::set_stream(&connexion.host, connexion.port, &connexion.password, actif)
                        .await
                }
                SortieKind::Enregistrement => {
                    crate::obs::set_record(&connexion.host, connexion.port, &connexion.password, actif)
                        .await
                }
            };
            let mut e = etat.lock().unwrap_or_else(|p| p.into_inner());
            match resultat {
                Ok(actif_relu) => {
                    match kind {
                        SortieKind::Diffusion => e.diffusion = actif_relu,
                        SortieKind::Enregistrement => e.enregistrement = actif_relu,
                    }
                    e.erreur = None;
                }
                Err(m) => e.erreur = Some(m),
            }
        });
    }
}

impl Moteur for MoteurObs {
    /// Ce qu'OBS sait faire aujourd'hui (matrice produit, pas théorique).
    fn capacites(&self) -> Capacites {
        Capacites {
            capture_ecran: true,
            capture_fenetre: true,
            capture_jeu: true,
            camera: true,
            // Plugin occ-ai, inférence 100 % locale (contrat vie privée).
            fond_vert: true,
            masque_shader: false,
            // Browser source + SMIL : les cadres animés existent en OBS.
            cadres_animes: true,
            audio_par_app: true,
            roi_encode: false,
            multi_format: true,
            hdr: false,
            srt: true,
            ndi: false,
        }
    }

    fn demarrer_sortie(&self, cfg: SortieCfg) -> Result<(), ErreurMoteur> {
        cfg.valider()?;
        if self.connexion().is_none() {
            return Err(ErreurMoteur::Indisponible(
                "connexion OBS non configurée (commande moteur_obs_connexion)".to_string(),
            ));
        }
        for kind in sorties_depuis_cfg(&cfg) {
            self.lancer(kind, true);
        }
        Ok(())
    }

    fn arreter_sortie(&self, kind: SortieKind) -> Result<(), ErreurMoteur> {
        if self.connexion().is_none() {
            return Err(ErreurMoteur::Indisponible(
                "connexion OBS non configurée (commande moteur_obs_connexion)".to_string(),
            ));
        }
        self.lancer(kind, false);
        Ok(())
    }

    fn etat(&self) -> MoteurEtat {
        let e = self.etat.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(m) = &e.erreur {
            return MoteurEtat::Erreur(m.clone());
        }
        if e.diffusion || e.enregistrement {
            MoteurEtat::Actif
        } else {
            MoteurEtat::Inactif
        }
    }

    /// OBS n'expose pas d'aperçu au moteur : les projecteurs OBS s'en chargent.
    fn apercu(&self) -> Option<ApercuFrame> {
        None
    }

    /// Teardown : **jamais** de coupure implicite d'un live OBS.
    fn arreter(&self) {}
}

/// Traduit les sinks demandés en sorties OBS (sans doublon).
fn sorties_depuis_cfg(cfg: &SortieCfg) -> Vec<SortieKind> {
    let mut sorties = Vec::new();
    for sink in &cfg.sinks {
        let kind = match sink {
            SinkCfg::Fichier { .. } => Some(SortieKind::Enregistrement),
            SinkCfg::Rtmp { .. } | SinkCfg::Srt { .. } => Some(SortieKind::Diffusion),
            // L'aperçu ne démarre aucune sortie OBS.
            SinkCfg::Apercu => None,
        };
        if let Some(kind) = kind {
            if !sorties.contains(&kind) {
                sorties.push(kind);
            }
        }
    }
    sorties
}

#[cfg(test)]
mod tests {
    use super::*;
    use sos_moteur_api::Conteneur;

    fn cfg_fichier() -> SortieCfg {
        SortieCfg::fichier("C:/tmp/x.mp4", Conteneur::Mp4)
    }

    #[test]
    fn capacites_obs_coherentes() {
        let c = MoteurObs::nouveau().capacites();
        assert!(c.capture_ecran && c.camera && c.fond_vert && c.srt);
        assert!(!c.hdr && !c.ndi && !c.roi_encode);
    }

    #[test]
    fn etat_initial_inactif() {
        assert_eq!(MoteurObs::nouveau().etat(), MoteurEtat::Inactif);
    }

    #[test]
    fn demarrer_refuse_une_config_invalide() {
        let moteur = MoteurObs::nouveau();
        let mut cfg = cfg_fichier();
        cfg.fps = 0;
        assert!(matches!(
            moteur.demarrer_sortie(cfg),
            Err(ErreurMoteur::Configuration(_))
        ));
    }

    #[test]
    fn demarrer_sans_connexion_est_refuse() {
        let moteur = MoteurObs::nouveau();
        assert!(matches!(
            moteur.demarrer_sortie(cfg_fichier()),
            Err(ErreurMoteur::Indisponible(_))
        ));
    }

    #[test]
    fn arreter_sans_connexion_est_refuse() {
        let moteur = MoteurObs::nouveau();
        assert!(matches!(
            moteur.arreter_sortie(SortieKind::Diffusion),
            Err(ErreurMoteur::Indisponible(_))
        ));
    }

    #[test]
    fn arreter_ne_coupe_jamais_et_est_idempotent() {
        let moteur = MoteurObs::nouveau();
        moteur.arreter();
        moteur.arreter();
        assert_eq!(moteur.etat(), MoteurEtat::Inactif);
    }

    #[test]
    fn apercu_absent() {
        assert!(MoteurObs::nouveau().apercu().is_none());
    }

    #[test]
    fn sorties_deduites_des_sinks() {
        let mut cfg = cfg_fichier();
        cfg.sinks.push(SinkCfg::Fichier {
            chemin: "C:/tmp/y.mkv".into(),
            conteneur: Conteneur::Mkv,
        });
        cfg.sinks.push(SinkCfg::Rtmp {
            url: "rtmp://live.example/app".to_string(),
        });
        cfg.sinks.push(SinkCfg::Apercu);
        assert_eq!(
            sorties_depuis_cfg(&cfg),
            vec![SortieKind::Enregistrement, SortieKind::Diffusion]
        );
    }

    #[test]
    fn debug_masque_le_mot_de_passe() {
        let c = ConnexionObs {
            host: "127.0.0.1".to_string(),
            port: 4455,
            password: "secret-obs".to_string(),
        };
        let texte = format!("{c:?}");
        assert!(!texte.contains("secret-obs"));
        assert!(texte.contains("<masqué>"));
    }
}
