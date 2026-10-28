//! Sorties du moteur — un `Sink` reçoit les paquets encodés.
//!
//! Le moteur n'écrit jamais directement un fichier : il pousse dans un
//! `Sink` (`FileSink` brut, `FfmpegSink` conteneurisé). Les sinks RTMP/SRT
//! arriveront sur la même interface (plan §4.4).

pub mod ffmpeg;
pub mod file;
pub mod tee;

// Contrats partagés : définis une seule fois dans `sos-moteur-api` (§3.2).
pub use sos_moteur_api::{Conteneur, ErreurSortie, Sink};
