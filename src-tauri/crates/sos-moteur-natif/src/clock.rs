//! Horloge du moteur — cadence à échéances **absolues**, sans dérive.
//!
//! Le tick `n` tombe à `origine_session + n × intervalle` : le sommeil
//! n'accumule jamais de dérive (jamais de `sleep(intervalle)` en boucle).
//! Les retards sont comptés, jamais masqués.
//!
//! ⚠️ **Un retard de plus d'une période recale l'échéance au lieu de
//! rattraper** (2026-11-02) : sans recalage, les échéances dépassées restent
//! dans le passé et la boucle part en rafale (N frames collées) — le mouvement
//! se fige puis saute. La durée de session, elle, reste ancrée à `origine`.

use std::fmt;
use std::time::{Duration, Instant};

/// Erreur d'horloge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurHorloge {
    FpsNul,
}

impl fmt::Display for ErreurHorloge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurHorloge::FpsNul => write!(f, "fps nul"),
        }
    }
}

impl std::error::Error for ErreurHorloge {}

/// Horloge cadencée à échéances absolues.
pub struct Horloge {
    /// Origine **fixe** de la session — durée écoulée et fps moyen. Ne bouge
    /// jamais : un recalage d'échéances ne doit pas raccourcir la session.
    origine: Instant,
    /// Origine **recalable** des échéances (voir [`Horloge::attendre`]).
    debut: Instant,
    intervalle: Duration,
    tick: u64,
    retards: u64,
    retard_max: Duration,
}

impl Horloge {
    pub fn nouvelle(fps: u32) -> Result<Self, ErreurHorloge> {
        if fps == 0 {
            return Err(ErreurHorloge::FpsNul);
        }
        let maintenant = Instant::now();
        Ok(Self {
            origine: maintenant,
            debut: maintenant,
            intervalle: Duration::from_secs_f64(1.0 / fps as f64),
            tick: 0,
            retards: 0,
            retard_max: Duration::ZERO,
        })
    }

    /// Attend l'échéance du tick courant, l'incrémente, rend le retard.
    ///
    /// `tick` est converti en `u32` pour `Duration × n` : à 60 fps, le
    /// dépassement n'arrive qu'après ~2 ans de marche continue.
    ///
    /// ⚠️ **Recalage, jamais de rattrapage** : au-delà d'une période de
    /// retard, l'échéance repart de *maintenant*. Sans ce recalage, toutes les
    /// échéances déjà dépassées restent dans le passé et la boucle part en
    /// **rafale** (N frames collées d'un seul tick) : le mouvement se fige
    /// puis saute, et la durée du fichier dérive (mesuré sur la scène réelle :
    /// 240 ms de retard → ~14 frames parties d'un coup, 48,6 fps moyens pour
    /// 60 demandés). Recaler transforme la rafale en **trou** — ce que fait
    /// OBS, et ce que l'œil lit comme une frame perdue, pas comme une
    /// accélération.
    pub fn attendre(&mut self) -> Duration {
        let echeance = self.debut + self.intervalle * self.tick as u32;
        let maintenant = Instant::now();
        let retard = if maintenant > echeance {
            maintenant - echeance
        } else {
            std::thread::sleep(echeance - maintenant);
            Duration::ZERO
        };
        if !retard.is_zero() {
            self.retards += 1;
            if retard > self.retard_max {
                self.retard_max = retard;
            }
            if retard > self.intervalle {
                // L'échéance du tick suivant vaut `maintenant + intervalle`.
                self.debut = maintenant - self.intervalle * self.tick as u32;
            }
        }
        self.tick += 1;
        retard
    }

    pub fn ticks(&self) -> u64 {
        self.tick
    }

    pub fn retards(&self) -> u64 {
        self.retards
    }

    pub fn retard_max(&self) -> Duration {
        self.retard_max
    }

    pub fn intervalle(&self) -> Duration {
        self.intervalle
    }

    /// Durée **réelle** depuis le début de la session — jamais affectée par un
    /// recalage d'échéances (`origine`, pas `debut`).
    pub fn ecoule(&self) -> Duration {
        self.origine.elapsed()
    }

    /// FPS moyen depuis le début (0 si rien n'est encore écoulé).
    pub fn fps_effectif(&self) -> f64 {
        let secondes = self.origine.elapsed().as_secs_f64();
        if secondes <= 0.0 {
            0.0
        } else {
            self.tick as f64 / secondes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_nul_refuse() {
        assert!(Horloge::nouvelle(0).is_err());
    }

    #[test]
    fn cinq_ticks_sans_derive() {
        let mut horloge = Horloge::nouvelle(50).expect("horloge"); // 20 ms
        for _ in 0..5 {
            horloge.attendre();
        }
        assert_eq!(horloge.ticks(), 5);
        assert!(horloge.ecoule() >= Duration::from_millis(80));
        assert!(horloge.fps_effectif() > 0.0);
        assert!(horloge.retard_max() < Duration::from_millis(200));
    }

    /// Un retard de plusieurs périodes **recale** l'horloge : le tick suivant
    /// attend de nouveau une période au lieu de partir en rafale, et la durée
    /// de session n'est pas raccourcie par le recalage.
    #[test]
    fn retard_recale_au_lieu_de_rattraper() {
        let mut horloge = Horloge::nouvelle(50).expect("horloge"); // 20 ms
        horloge.attendre();
        std::thread::sleep(Duration::from_millis(100));
        let retard = horloge.attendre();
        assert!(retard >= Duration::from_millis(60), "retard {retard:?}");

        let debut = Instant::now();
        let suivant = horloge.attendre();
        assert!(suivant.is_zero(), "rafale après recalage : {suivant:?}");
        assert!(
            debut.elapsed() >= Duration::from_millis(15),
            "le tick suivant doit attendre une période, mesuré {:?}",
            debut.elapsed()
        );
        // `ecoule` reste ancré à l'origine fixe (100 ms de sommeil + 3 ticks).
        assert!(horloge.ecoule() >= Duration::from_millis(110));
    }
}
