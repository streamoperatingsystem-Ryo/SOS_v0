//! Machine à états du mode moteur (OBS ↔ natif) — plan §11.
//!
//! Règles figées : ne jamais couper un live implicitement ; une sortie
//! native active bloque aussi la bascule (lot 1.4).

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
    /// Une sortie OBS est active : jamais de coupure implicite.
    LiveEnCours,
    /// Une sortie native est active : jamais de coupure implicite.
    SortieNatifEnCours,
}

impl fmt::Display for ErreurMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurMode::LiveEnCours => write!(f, "une sortie OBS est active"),
            ErreurMode::SortieNatifEnCours => write!(f, "une sortie native est active"),
        }
    }
}

impl std::error::Error for ErreurMode {}

/// Décide d'une bascule. `live_actif` = OBS diffuse ou enregistre ;
/// `natif_actif` = une sortie native tourne (même règle, symétrique).
pub fn verifier_bascule(
    actuel: Mode,
    cible: Mode,
    live_actif: bool,
    natif_actif: bool,
) -> Result<Mode, ErreurMode> {
    if cible == actuel {
        return Ok(actuel);
    }
    match cible {
        Mode::Natif => {
            if live_actif {
                Err(ErreurMode::LiveEnCours)
            } else {
                Ok(Mode::Natif)
            }
        }
        Mode::Obs => {
            if live_actif {
                Err(ErreurMode::LiveEnCours)
            } else if natif_actif {
                Err(ErreurMode::SortieNatifEnCours)
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
        assert_eq!(
            verifier_bascule(Mode::Obs, Mode::Obs, true, true),
            Ok(Mode::Obs)
        );
        assert_eq!(
            verifier_bascule(Mode::Natif, Mode::Natif, true, true),
            Ok(Mode::Natif)
        );
    }

    #[test]
    fn bascule_vers_natif_acceptee_hors_live() {
        assert_eq!(
            verifier_bascule(Mode::Obs, Mode::Natif, false, false),
            Ok(Mode::Natif)
        );
        assert_eq!(
            verifier_bascule(Mode::Obs, Mode::Natif, true, false),
            Err(ErreurMode::LiveEnCours)
        );
    }

    #[test]
    fn jamais_de_coupure_implicite() {
        assert_eq!(
            verifier_bascule(Mode::Natif, Mode::Obs, true, false),
            Err(ErreurMode::LiveEnCours)
        );
        assert_eq!(
            verifier_bascule(Mode::Natif, Mode::Obs, false, true),
            Err(ErreurMode::SortieNatifEnCours)
        );
        assert_eq!(
            verifier_bascule(Mode::Natif, Mode::Obs, false, false),
            Ok(Mode::Obs)
        );
    }

    #[test]
    fn noms_stables() {
        assert_eq!(Mode::Obs.nom(), "obs");
        assert_eq!(Mode::Natif.nom(), "natif");
        assert_eq!(Mode::depuis_nom("obs"), Some(Mode::Obs));
        assert_eq!(Mode::depuis_nom("x"), None);
    }
}
