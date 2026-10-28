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
use sos_moteur_api::ErreurMoteur;

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
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurMoteur> {
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

    fn fermer(&mut self) -> Result<(), ErreurMoteur> {
        self.ferme = true;
        drop(self.entree.take());
        let statut = self
            .enfant
            .wait()
            .map_err(|e| ErreurSortie::Processus(format!("attente : {e}")))?;
        if !statut.success() {
            return Err(ErreurMoteur::from(ErreurSortie::Processus(format!(
                "ffmpeg : statut {:?}",
                statut.code()
            ))));
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

/// Ajoute les pistes audio à un fichier **déjà finalisé** (muxage en deux temps).
///
/// Le son est capturé en PCM brut pendant l'enregistrement (threads WASAPI,
/// `audio::piste`), puis FFmpeg rééchantillonne, applique les gains et mixe —
/// aucun code de resampling côté Rust.
///
/// Choix assumé (2026-10-28) : pas de muxage en direct. Une seconde entrée
/// FFmpeg exigerait un pipe nommé Windows ; le live (RTMP) est de toute façon
/// refusé par le moteur et demandera un muxeur portant les PTS.
///
/// Le fichier d'origine est remplacé **atomiquement** : mux vers un fichier
/// temporaire, bascule, suppression du temporaire. Si la bascule échoue, le
/// fichier d'origine est restauré.
pub fn muxer_audio(
    chemin: &Path,
    pistes: &[crate::audio::piste::Piste],
) -> Result<(), ErreurSortie> {
    if pistes.is_empty() {
        return Ok(());
    }
    let binaire = chemin_sidecar().map_err(|e| ErreurSortie::Processus(e.to_string()))?;
    let extension = chemin
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "mp4".to_string());
    let nom = chemin
        .file_stem()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "sortie".to_string());
    let dossier = chemin.parent().unwrap_or_else(|| Path::new("."));
    let temporaire = dossier.join(format!("{nom}.audio.{extension}"));
    let sauvegarde = dossier.join(format!("{nom}.sans-audio.{extension}"));

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        chemin.to_string_lossy().to_string(),
    ];
    for piste in pistes {
        args.extend([
            "-f".into(),
            "s16le".into(),
            "-ar".into(),
            piste.taux.to_string(),
            "-ac".into(),
            piste.canaux.to_string(),
            "-i".into(),
            piste.chemin.to_string_lossy().to_string(),
        ]);
    }

    // Gains et mixage : `volume` par source puis `amix`. Une seule source ne
    // passe pas par `amix` (inutile et source de variantes de comportement).
    let mut filtre = String::new();
    for (i, piste) in pistes.iter().enumerate() {
        if !filtre.is_empty() {
            filtre.push(';');
        }
        filtre.push_str(&format!(
            "[{}:a]volume={:.3}[a{i}]",
            i + 1,
            piste.gain_milliemes as f32 / 1000.0
        ));
    }
    // `apad` : une piste plus courte que la vidéo est complétée par du
    // silence. Sans lui, `-shortest` couperait le fichier à la longueur de
    // l'audio (mesuré : 2,83 s de vidéo pour 3 s demandées).
    if pistes.len() == 1 {
        filtre.push_str(";[a0]apad[a]");
    } else {
        filtre.push(';');
        for i in 0..pistes.len() {
            filtre.push_str(&format!("[a{i}]"));
        }
        filtre.push_str(&format!(
            "amix=inputs={}:duration=longest:normalize=0[am];[am]apad[a]",
            pistes.len()
        ));
    }

    args.extend([
        "-filter_complex".into(),
        filtre,
        "-map".into(),
        "0:v".into(),
        "-map".into(),
        "[a]".into(),
        "-c:v".into(),
        "copy".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "192k".into(),
        // La vidéo fait foi : si la capture audio déborde (elle démarre un
        // peu avant la première frame), le fichier garde la durée vidéo.
        "-shortest".into(),
        temporaire.to_string_lossy().to_string(),
    ]);

    let sortie = Command::new(&binaire)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| ErreurSortie::Processus(format!("muxage : {e}")))?;
    if !sortie.status.success() {
        let _ = std::fs::remove_file(&temporaire);
        let detail = String::from_utf8_lossy(&sortie.stderr);
        return Err(ErreurSortie::Processus(format!(
            "muxage audio : statut {:?} — {}",
            sortie.status.code(),
            detail.lines().last().unwrap_or("").trim()
        )));
    }

    std::fs::rename(chemin, &sauvegarde)
        .map_err(|e| ErreurSortie::Processus(format!("sauvegarde : {e}")))?;
    match std::fs::rename(&temporaire, chemin) {
        Ok(()) => {
            let _ = std::fs::remove_file(&sauvegarde);
            Ok(())
        }
        Err(e) => {
            // Restaure l'original plutôt que de laisser l'utilisateur sans
            // fichier.
            let _ = std::fs::rename(&sauvegarde, chemin);
            let _ = std::fs::remove_file(&temporaire);
            Err(ErreurSortie::Processus(format!("bascule : {e}")))
        }
    }
}
