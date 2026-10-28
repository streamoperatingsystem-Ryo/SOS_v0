//! Machine à états du mode moteur (OBS ↔ natif) — plan §11.
//!
//! Règles figées : ne jamais couper un live implicitement ; le natif n'est
//! proposé que lorsqu'il est réellement branché (lot 1.4 : non).

use std::fmt;

/// Mode de diffusion actif.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Obs,
    Natif,
}

impl Mode {
    pub fn nom(self) -> &'static str {
        match self {
            Mode::Obs => "obs",
            Mode::Natif => "natif",
        }
    }

    pub fn depuis_nom(nom: &str) -> Option<Self> {
        match nom {
            "obs" => Some(Mode::Obs),
            "natif" => Some(Mode::Natif),
            _ => None,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.nom())
    }
}

/// Erreurs de bascule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurMode {
    /// Moteur natif non branché dans l'app (lot 1.4).
    NatifIndisponible,
    /// Une sortie OBS est active : jamais de coupure implicite.
    LiveEnCours,
}

impl fmt::Display for ErreurMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurMode::NatifIndisponible => write!(f, "moteur natif non disponible"),
            ErreurMode::LiveEnCours => write!(f, "une sortie OBS est active"),
        }
    }
}

impl std::error::Error for ErreurMode {}

/// Décide d'une bascule. `live_actif` = OBS diffuse ou enregistre.
pub fn verifier_bascule(actuel: Mode, cible: Mode, live_actif: bool) -> Result<Mode, ErreurMode> {
    if cible == actuel {
        return Ok(actuel);
    }
    match cible {
        // Lot 1.4 : le moteur natif n'est pas encore branché dans l'app.
        Mode::Natif => Err(ErreurMode::NatifIndisponible),
        Mode::Obs => {
            if live_actif {
                Err(ErreurMode::LiveEnCours)
            } else {
                Ok(Mode::Obs)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meme_mode_ne_fait_rien() {
        assert_eq!(verifier_bascule(Mode::Obs, Mode::Obs, true), Ok(Mode::Obs));
    }

    #[test]
    fn natif_indisponible() {
        assert_eq!(
            verifier_bascule(Mode::Obs, Mode::Natif, false),
            Err(ErreurMode::NatifIndisponible)
        );
    }

    #[test]
    fn jamais_de_coupure_implicite() {
        assert_eq!(
            verifier_bascule(Mode::Natif, Mode::Obs, true),
            Err(ErreurMode::LiveEnCours)
        );
        assert_eq!(verifier_bascule(Mode::Natif, Mode::Obs, false), Ok(Mode::Obs));
    }

    #[test]
    fn noms_stables() {
        assert_eq!(Mode::Obs.nom(), "obs");
        assert_eq!(Mode::Natif.nom(), "natif");
        assert_eq!(Mode::depuis_nom("obs"), Some(Mode::Obs));
        assert_eq!(Mode::depuis_nom("x"), None);
    }
}
