//! Encodage vidéo (Media Foundation).
//!
//! LOT 0.2c : détection des encodeurs + encodage H.264 d'une séquence NV12.
//! La conversion BGRA→NV12 de ce lot est **CPU** (spike) : elle sera
//! remplacée par `ID3D11VideoProcessor` (GPU, zéro-copie) au lot 0.2d, avant
//! toute mesure P0.

pub mod detect;
pub mod ffmpeg;
pub mod mft;
pub mod nvenc;

use std::fmt;

// Contrats partagés : définis une seule fois dans `sos-moteur-api` (§3.2).
pub use sos_moteur_api::{Codec, PaquetEncode};

/// Erreurs d'encodage. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurEncodage {
    /// Media Foundation n'a pas démarré.
    MediaFoundation(String),
    /// Aucun encodeur pour ce codec.
    AucunEncodeur(String),
    /// Échec d'un appel MFT.
    Mft(String),
    /// Conversion de format impossible.
    Conversion(String),
    /// Device / gestionnaire D3D11 indisponible.
    Gpu(String),
    /// Sortie (sidecar FFmpeg, fichier, processus).
    Sortie(String),
    /// Backend NVENC direct (SDK constructeur, zéro-copie).
    Nvenc(String),
    /// Plateforme non supportée.
    NonSupporte,
}

impl fmt::Display for ErreurEncodage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurEncodage::MediaFoundation(m) => write!(f, "Media Foundation : {m}"),
            ErreurEncodage::AucunEncodeur(m) => write!(f, "aucun encodeur : {m}"),
            ErreurEncodage::Mft(m) => write!(f, "MFT : {m}"),
            ErreurEncodage::Conversion(m) => write!(f, "conversion : {m}"),
            ErreurEncodage::Gpu(m) => write!(f, "GPU : {m}"),
            ErreurEncodage::Sortie(m) => write!(f, "sortie : {m}"),
            ErreurEncodage::Nvenc(m) => write!(f, "NVENC : {m}"),
            ErreurEncodage::NonSupporte => write!(f, "plateforme non supportée"),
        }
    }
}

impl std::error::Error for ErreurEncodage {}

/// Preset d'encodage (`SortieCfg.preset`) — compromis vitesse / qualité.
///
/// Mapping documenté :
///
/// | Preset      | NVENC (GUID réel) | MFT (`AVEncCommonQuality`, 0-100) |
/// |-------------|-------------------|-----------------------------------|
/// | `Rapide`    | P2                | 40                                |
/// | `Equilibre` | P4 (= historique) | 70 (= historique)                 |
/// | `Qualite`   | P7                | 100                               |
///
/// P1 est volontairement laissé en marge sous `Rapide` : son gain de
/// vitesse sur P2 est marginal et sa perte de qualité visible. P6 reste en
/// marge sous `Qualite`. Le défaut `Equilibre` reproduit exactement le
/// réglage historique — zéro régression quand le preset n'est pas fourni.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Vitesse privilégiée (`rapide`/`fast`/`performance`).
    Rapide,
    /// Défaut produit (`equilibre`/`balanced`/`balance`).
    Equilibre,
    /// Qualité privilégiée (`qualite`/`quality`/`hq`).
    Qualite,
}

impl Preset {
    /// Défaut produit : `equilibre` (P4 / qualité 70).
    pub const DEFAUT: Self = Self::Equilibre;

    /// Nom → preset. Les ids produit FR **et** les équivalents EN sont
    /// acceptés ; `None` si inconnu ("" compris — un preset absent se
    /// résout au défaut sans dégradation).
    pub fn depuis_nom(nom: &str) -> Option<Self> {
        match nom.trim() {
            "rapide" | "fast" | "performance" => Some(Self::Rapide),
            "equilibre" | "balanced" | "balance" => Some(Self::Equilibre),
            "qualite" | "quality" | "hq" => Some(Self::Qualite),
            _ => None,
        }
    }

    /// Valeur `CODECAPI_AVEncCommonQuality` du repli MFT (0-100, qualité
    /// croissante ; attribut best-effort, certains pilotes l'ignorent).
    pub fn qualite_mft(self) -> u32 {
        match self {
            Self::Rapide => 40,
            Self::Equilibre => 70,
            Self::Qualite => 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_depuis_nom_fr_en_et_defauts() {
        for nom in ["rapide", "fast", "performance"] {
            assert_eq!(Preset::depuis_nom(nom), Some(Preset::Rapide), "{nom}");
        }
        for nom in ["equilibre", "balanced", "balance"] {
            assert_eq!(Preset::depuis_nom(nom), Some(Preset::Equilibre), "{nom}");
        }
        for nom in ["qualite", "quality", "hq"] {
            assert_eq!(Preset::depuis_nom(nom), Some(Preset::Qualite), "{nom}");
        }
        // Inconnu et vide → None ; la résolution reste à l'appelant.
        assert_eq!(Preset::depuis_nom("p7"), None);
        assert_eq!(Preset::depuis_nom(""), None);
        assert_eq!(Preset::depuis_nom("equilibre"), Some(Preset::DEFAUT));
    }

    #[test]
    fn preset_qualite_mft_bornee_et_croissante() {
        assert_eq!(Preset::Rapide.qualite_mft(), 40);
        assert_eq!(Preset::Equilibre.qualite_mft(), 70);
        assert_eq!(Preset::Qualite.qualite_mft(), 100);
        assert!(Preset::Rapide.qualite_mft() < Preset::Equilibre.qualite_mft());
        assert!(Preset::Equilibre.qualite_mft() < Preset::Qualite.qualite_mft());
    }
}
