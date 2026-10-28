//! Gouverneur — dégradation **explicite**, jamais silencieuse (plan §9.2).
//!
//! Ordre de dégradation : FPS des sources → résolution de composition →
//! preset d'encodage → saturation signalée. Chaque décision porte sa raison ;
//! une décision au plus par `cooldown`, et jamais de remontée automatique
//! dans ce lot (dégradation monotone).

use std::fmt;
use std::time::{Duration, Instant};

/// Mesures fournies par le moteur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mesures {
    pub fps_cible: u32,
    pub fps_reel: f64,
    pub drops: u64,
    pub file_encode: u32,
    pub latence_ms: u32,
    pub resolution: (u32, u32),
}

/// Action décidée par le gouverneur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Rien,
    ReduireFps { de: u32, vers: u32 },
    ReduireResolution { de: (u32, u32), vers: (u32, u32) },
    ReduirePreset,
    Saturation,
}

/// Raison d'une décision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raison {
    Aucune,
    Drops,
    FileEncode,
    FpsBas,
    Latence,
}

impl Raison {
    pub fn nom(self) -> &'static str {
        match self {
            Raison::Aucune => "aucune",
            Raison::Drops => "drops",
            Raison::FileEncode => "file d'encodage",
            Raison::FpsBas => "fps bas",
            Raison::Latence => "latence",
        }
    }
}

/// Décision complète : action + raison lisible (UI, boîte noire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub action: Action,
    pub raison: Raison,
}

impl fmt::Display for Decision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.action {
            Action::Rien => write!(f, "rien"),
            Action::ReduireFps { de, vers } => {
                write!(f, "reduire fps {de}->{vers} ({})", self.raison.nom())
            }
            Action::ReduireResolution { de, vers } => write!(
                f,
                "reduire resolution {}x{}->{}x{} ({})",
                de.0,
                de.1,
                vers.0,
                vers.1,
                self.raison.nom()
            ),
            Action::ReduirePreset => write!(f, "reduire preset ({})", self.raison.nom()),
            Action::Saturation => write!(f, "saturation ({})", self.raison.nom()),
        }
    }
}

/// Étape courante de la dégradation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Etape {
    Rien,
    Fps,
    Resolution,
    Preset,
}

/// Gouverneur de charge.
pub struct Gouverneur {
    etape: Etape,
    cooldown: Duration,
    derniere: Option<Instant>,
}

impl Gouverneur {
    pub fn nouveau() -> Self {
        Self::avec_cooldown(Duration::from_secs(10))
    }

    pub fn avec_cooldown(cooldown: Duration) -> Self {
        Self {
            etape: Etape::Rien,
            cooldown,
            derniere: None,
        }
    }

    /// Évalue la charge et rend une décision (au plus une par cooldown).
    pub fn evaluer(&mut self, mesures: &Mesures) -> Decision {
        let raison = match Self::surcharge(mesures) {
            Some(r) => r,
            None => {
                return Decision {
                    action: Action::Rien,
                    raison: Raison::Aucune,
                }
            }
        };

        let maintenant = Instant::now();
        if let Some(derniere) = self.derniere {
            if maintenant.duration_since(derniere) < self.cooldown {
                return Decision {
                    action: Action::Rien,
                    raison: Raison::Aucune,
                };
            }
        }

        let action = match self.etape {
            Etape::Rien => {
                self.etape = Etape::Fps;
                Action::ReduireFps {
                    de: mesures.fps_cible,
                    vers: (mesures.fps_cible / 2).max(15),
                }
            }
            Etape::Fps => {
                self.etape = Etape::Resolution;
                Action::ReduireResolution {
                    de: mesures.resolution,
                    vers: Self::resolution_reduite(mesures.resolution),
                }
            }
            Etape::Resolution => {
                self.etape = Etape::Preset;
                Action::ReduirePreset
            }
            Etape::Preset => Action::Saturation,
        };
        self.derniere = Some(maintenant);

        Decision { action, raison }
    }

    /// Surcharge ? Ordre de priorité : drops, file d'encodage, fps, latence.
    fn surcharge(mesures: &Mesures) -> Option<Raison> {
        if mesures.drops > 0 {
            return Some(Raison::Drops);
        }
        if mesures.file_encode > 3 {
            return Some(Raison::FileEncode);
        }
        if mesures.fps_cible > 0
            && mesures.fps_reel < f64::from(mesures.fps_cible) * 0.9
        {
            return Some(Raison::FpsBas);
        }
        if mesures.latence_ms > 100 {
            return Some(Raison::Latence);
        }
        None
    }

    /// Résolution réduite : 2/3, arrondie paire, plancher 320×180.
    fn resolution_reduite((largeur, hauteur): (u32, u32)) -> (u32, u32) {
        let l = ((largeur * 2 / 3) & !1).max(320);
        let h = ((hauteur * 2 / 3) & !1).max(180);
        (l, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesure_surchargee() -> Mesures {
        Mesures {
            fps_cible: 60,
            fps_reel: 30.0,
            drops: 2,
            file_encode: 0,
            latence_ms: 10,
            resolution: (1360, 768),
        }
    }

    #[test]
    fn charge_normale_ne_degrade_pas() {
        let mut g = Gouverneur::avec_cooldown(Duration::ZERO);
        let mesures = Mesures {
            fps_cible: 60,
            fps_reel: 59.5,
            drops: 0,
            file_encode: 0,
            latence_ms: 10,
            resolution: (1360, 768),
        };
        let d = g.evaluer(&mesures);
        assert_eq!(d.action, Action::Rien);
        assert_eq!(d.raison, Raison::Aucune);
    }

    #[test]
    fn degrade_dans_l_ordre_du_plan() {
        let mut g = Gouverneur::avec_cooldown(Duration::ZERO);
        let mesures = mesure_surchargee();

        let d1 = g.evaluer(&mesures);
        assert_eq!(
            d1.action,
            Action::ReduireFps { de: 60, vers: 30 }
        );
        assert_eq!(d1.raison, Raison::Drops);

        let d2 = g.evaluer(&mesures);
        assert_eq!(
            d2.action,
            Action::ReduireResolution {
                de: (1360, 768),
                vers: (906, 512)
            }
        );

        let d3 = g.evaluer(&mesures);
        assert_eq!(d3.action, Action::ReduirePreset);

        let d4 = g.evaluer(&mesures);
        assert_eq!(d4.action, Action::Saturation);
    }

    #[test]
    fn cooldown_bloque_les_decisions() {
        let mut g = Gouverneur::avec_cooldown(Duration::from_secs(10));
        let mesures = mesure_surchargee();
        let d1 = g.evaluer(&mesures);
        assert_ne!(d1.action, Action::Rien);
        let d2 = g.evaluer(&mesures);
        assert_eq!(d2.action, Action::Rien);
    }

    #[test]
    fn priorite_des_raisons() {
        let mesures = Mesures {
            fps_cible: 60,
            fps_reel: 10.0,
            drops: 1,
            file_encode: 9,
            latence_ms: 500,
            resolution: (1920, 1080),
        };
        assert_eq!(Gouverneur::surcharge(&mesures), Some(Raison::Drops));
    }
}
