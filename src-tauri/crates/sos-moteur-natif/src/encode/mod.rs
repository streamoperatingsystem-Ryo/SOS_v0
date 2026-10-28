//! Encodage vidéo (Media Foundation).
//!
//! LOT 0.2c : détection des encodeurs + encodage H.264 d'une séquence NV12.
//! La conversion BGRA→NV12 de ce lot est **CPU** (spike) : elle sera
//! remplacée par `ID3D11VideoProcessor` (GPU, zéro-copie) au lot 0.2d, avant
//! toute mesure P0.

pub mod detect;
pub mod ffmpeg;
pub mod mft;

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
            ErreurEncodage::NonSupporte => write!(f, "plateforme non supportée"),
        }
    }
}

impl std::error::Error for ErreurEncodage {}
