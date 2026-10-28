//! Horloge du moteur — cadence à échéances **absolues**, sans dérive.
//!
//! Le tick `n` tombe à `début + n × intervalle` : le sommeil n'accumule
//! jamais de dérive (jamais de `sleep(intervalle)` en boucle). Les retards
//! sont comptés, jamais masqués.

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
        Ok(Self {
            debut: Instant::now(),
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

    pub fn ecoule(&self) -> Duration {
        self.debut.elapsed()
    }

    /// FPS moyen depuis le début (0 si rien n'est encore écoulé).
    pub fn fps_effectif(&self) -> f64 {
        let secondes = self.debut.elapsed().as_secs_f64();
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
}
