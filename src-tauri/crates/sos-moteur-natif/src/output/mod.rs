//! Sorties du moteur — un `Sink` reçoit les paquets encodés.
//!
//! Le moteur n'écrit jamais directement un fichier : il pousse dans un
//! `Sink` (`FileSink` brut, `FfmpegSink` conteneurisé). Les sinks RTMP/SRT
//! arriveront sur la même interface (plan §4.4).

pub mod ffmpeg;
pub mod file;
pub mod tee;

use std::fmt;

use crate::encode::PaquetEncode;

/// Destinataire des paquets encodés.
pub trait Sink: Send {
    /// Pousse un paquet (backpressure par `write_all`).
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurSortie>;
    /// Termine proprement (fermeture du conteneur, attente du processus).
    fn fermer(&mut self) -> Result<(), ErreurSortie>;
}

/// Conteneur de sortie fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conteneur {
    /// MP4 **fragmenté** (`frag_keyframe+empty_moov`) : lisible même si le
    /// processus meurt en cours d'enregistrement.
    Mp4,
    /// Matroska : clusters, robuste à la coupure.
    Mkv,
}

impl Conteneur {
    pub fn extension(self) -> &'static str {
        match self {
            Conteneur::Mp4 => "mp4",
            Conteneur::Mkv => "mkv",
        }
    }
}

/// Erreurs de sortie. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurSortie {
    /// Fichier local (création, écriture, flush).
    Fichier(String),
    /// Processus sidecar (lancement, statut, kill).
    Processus(String),
}

impl fmt::Display for ErreurSortie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurSortie::Fichier(m) => write!(f, "fichier : {m}"),
            ErreurSortie::Processus(m) => write!(f, "processus : {m}"),
        }
    }
}

impl std::error::Error for ErreurSortie {}
