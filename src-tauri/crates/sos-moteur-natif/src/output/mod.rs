//! Sorties du moteur — un `Sink` reçoit les paquets encodés.
//!
//! Le moteur n'écrit jamais directement un fichier : il pousse dans un
//! `Sink` (`FileSink` brut, `FfmpegSink` conteneurisé). Les flux live
//! RTMP/SRT sont des `FfmpegSink` réseau (FLV / MPEG-TS, mêmes stdin et
//! pipe audio que l'enregistrement fichier — plan §4.4 / §12.2).

pub mod ffmpeg;
pub mod file;

// Contrats partagés : définis une seule fois dans `sos-moteur-api` (§3.2).
pub use sos_moteur_api::{Conteneur, ErreurSortie, Sink};
