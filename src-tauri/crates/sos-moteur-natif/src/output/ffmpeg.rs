//! Sink conteneurisé via le **sidecar FFmpeg** : bitstream → fMP4 / MKV.
//!
//! Le sidecar ne reçoit que le bitstream (`-c copy`), jamais de frame brute.
//! ⚠️ Un flux H.264 élémentaire ne transporte pas d'horodatage : le timing
//! vient de `-framerate` (valeur passée à la création). Les sinks RTMP
//! exigeront un muxeur qui porte les PTS (lot ultérieur).

use std::io::Write;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

use super::{Conteneur, ErreurSortie, Sink};
use crate::encode::ffmpeg::chemin_sidecar;
use crate::encode::PaquetEncode;

/// Muxeur piloté par un processus FFmpeg (stdin = bitstream H.264).
pub struct FfmpegSink {
    enfant: Child,
    entree: Option<ChildStdin>,
    octets: u64,
    paquets: u64,
    ferme: bool,
}

impl FfmpegSink {
    /// Démarre le sidecar. `ffmpeg` à `None` → résolution par `chemin_sidecar`.
    pub fn nouveau(
        ffmpeg: Option<&Path>,
        sortie: &Path,
        conteneur: Conteneur,
        fps: u32,
    ) -> Result<Self, ErreurSortie> {
        if fps == 0 {
            return Err(ErreurSortie::Processus("fps nul".into()));
        }
        let binaire = match ffmpeg {
            Some(p) => p.to_path_buf(),
            None => chemin_sidecar().map_err(|e| ErreurSortie::Processus(e.to_string()))?,
        };

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-f".into(),
            "h264".into(),
            "-framerate".into(),
            fps.to_string(),
            "-i".into(),
            "pipe:0".into(),
            "-c".into(),
            "copy".into(),
        ];
        match conteneur {
            Conteneur::Mp4 => args.extend([
                "-movflags".into(),
                "+frag_keyframe+empty_moov+default_base_moof".into(),
                "-f".into(),
                "mp4".into(),
            ]),
            Conteneur::Mkv => args.extend(["-f".into(), "matroska".into()]),
        }
        args.push(sortie.to_string_lossy().to_string());

        let mut enfant = Command::new(&binaire)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                ErreurSortie::Processus(format!("lancement de {} : {e}", binaire.display()))
            })?;
        let entree = enfant.stdin.take();

        Ok(Self {
            enfant,
            entree,
            octets: 0,
            paquets: 0,
            ferme: false,
        })
    }

    pub fn octets(&self) -> u64 {
        self.octets
    }

    pub fn paquets(&self) -> u64 {
        self.paquets
    }

    /// Arrêt **brutal** (annulation, test de robustesse) : tue le processus.
    ///
    /// Le conteneur reste lisible jusqu'au dernier fragment complet.
    pub fn couper(&mut self) -> Result<(), ErreurSortie> {
        self.ferme = true;
        drop(self.entree.take());
        self.enfant
            .kill()
            .map_err(|e| ErreurSortie::Processus(format!("kill : {e}")))?;
        let _ = self.enfant.wait();
        Ok(())
    }
}

impl Sink for FfmpegSink {
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurSortie> {
        let entree = self
            .entree
            .as_mut()
            .ok_or_else(|| ErreurSortie::Processus("stdin deja ferme".into()))?;
        entree
            .write_all(&paquet.data)
            .map_err(|e| ErreurSortie::Processus(format!("ecriture : {e}")))?;
        self.octets += paquet.data.len() as u64;
        self.paquets += 1;
        Ok(())
    }

    fn fermer(&mut self) -> Result<(), ErreurSortie> {
        self.ferme = true;
        drop(self.entree.take());
        let statut = self
            .enfant
            .wait()
            .map_err(|e| ErreurSortie::Processus(format!("attente : {e}")))?;
        if !statut.success() {
            return Err(ErreurSortie::Processus(format!(
                "ffmpeg : statut {:?}",
                statut.code()
            )));
        }
        Ok(())
    }
}

impl Drop for FfmpegSink {
    fn drop(&mut self) {
        if self.ferme {
            return;
        }
        // Best effort : EOF puis attente (finalise le conteneur).
        drop(self.entree.take());
        let _ = self.enfant.wait();
    }
}
