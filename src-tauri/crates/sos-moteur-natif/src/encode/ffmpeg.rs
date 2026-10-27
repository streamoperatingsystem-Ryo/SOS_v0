//! Encodage H.264/HEVC/AV1 via le **sidecar FFmpeg** (NVENC / AMF / QSV).
//!
//! Pourquoi ce chemin : les MFT matériels refusent nos entrées sur la machine
//! de test (E_UNEXPECTED, cf. commit a17f215). FFmpeg offre une voie matérielle
//! robuste et multi-constructeur **sans bloquer le P0**. Le coût assumé est une
//! copie CPU du NV12 dans le pipe — à remplacer plus tard par le zéro-copie.
//!
//! Contrat : aucune frame brute ne doit être conservée, aucune clé de flux ne
//! doit apparaître dans les logs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use super::ErreurEncodage;

/// Résultat d'un encodage FFmpeg.
#[derive(Debug, Clone)]
pub struct ResumeFfmpeg {
    /// Octets NV12 poussés dans le pipe.
    pub octets_entree: usize,
    /// Nombre de frames effectivement poussées.
    pub frames_poussees: u64,
    /// Sortie d'erreur (vide si succès).
    pub stderr: String,
    /// Code de sortie du processus.
    pub code: Option<i32>,
}

impl ResumeFfmpeg {
    pub fn succes(&self) -> bool {
        self.code == Some(0)
    }
}

/// Encodeur piloté par un processus FFmpeg (stdin = rawvideo NV12).
pub struct EncodeurFfmpeg {
    enfant: Child,
    stdin: Option<ChildStdin>,
    largeur: u32,
    hauteur: u32,
    taille_frame: usize,
    octets_entree: usize,
    frames_poussees: u64,
}

impl EncodeurFfmpeg {
    /// Démarre FFmpeg avec l'encodeur demandé (ex. `h264_nvenc`, `h264_qsv`,
    /// `h264_amf`, `libx264`).
    ///
    /// `preset`/`tune` sont propres à l'encodeur ; les valeurs par défaut
    /// visent NVENC en faible latence.
    pub fn nouveau(
        ffmpeg: &Path,
        encodeur: &str,
        largeur: u32,
        hauteur: u32,
        fps: u32,
        debit_kbps: u32,
        sortie: &Path,
    ) -> Result<Self, ErreurEncodage> {
        let largeur = largeur & !1;
        let hauteur = hauteur & !1;
        if largeur == 0 || hauteur == 0 || fps == 0 {
            return Err(ErreurEncodage::Conversion(
                "dimensions ou fps invalides".into(),
            ));
        }

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "nv12".into(),
            "-s".into(),
            format!("{largeur}x{hauteur}"),
            "-r".into(),
            fps.to_string(),
            "-i".into(),
            "pipe:0".into(),
            "-c:v".into(),
            encodeur.to_string(),
            "-b:v".into(),
            format!("{debit_kbps}k"),
            "-g".into(),
            fps.saturating_mul(2).to_string(),
        ];

        // Réglages NVENC (faible latence). Ignorés/refusés par d'autres
        // encodeurs : à mapper proprement dans le moteur (lot ultérieur).
        if encodeur.ends_with("_nvenc") {
            args.push("-preset".into());
            args.push("p4".into());
            args.push("-tune".into());
            args.push("ll".into());
        }

        args.push("-f".into());
        args.push("h264".into());
        args.push(sortie.to_string_lossy().to_string());

        let mut enfant = Command::new(ffmpeg)
            .args(&args)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                ErreurEncodage::Sortie(format!(
                    "lancement de {} : {e}",
                    ffmpeg.display()
                ))
            })?;

        let stdin = enfant.stdin.take();

        Ok(Self {
            enfant,
            stdin,
            largeur,
            hauteur,
            taille_frame: (largeur as usize * hauteur as usize * 3) / 2,
            octets_entree: 0,
            frames_poussees: 0,
        })
    }

    pub fn largeur(&self) -> u32 {
        self.largeur
    }

    pub fn hauteur(&self) -> u32 {
        self.hauteur
    }

    pub fn taille_frame(&self) -> usize {
        self.taille_frame
    }

    /// Pousse une frame NV12. `write_all` fournit la backpressure : si le pipe
    /// est plein, l'appelant bloque (jamais de buffer illimité en mémoire).
    pub fn pousser(&mut self, nv12: &[u8]) -> Result<(), ErreurEncodage> {
        if nv12.len() != self.taille_frame {
            return Err(ErreurEncodage::Conversion(format!(
                "frame {} octets, {} attendus",
                nv12.len(),
                self.taille_frame
            )));
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| ErreurEncodage::Sortie("stdin FFmpeg déjà fermé".into()))?;

        stdin.write_all(nv12).map_err(|e| {
            ErreurEncodage::Sortie(format!("écriture dans FFmpeg : {e}"))
        })?;
        self.octets_entree += nv12.len();
        self.frames_poussees += 1;
        Ok(())
    }

    /// Ferme stdin et attend la fin du processus.
    pub fn terminer(mut self) -> Result<ResumeFfmpeg, ErreurEncodage> {
        drop(self.stdin.take());

        let sortie = self
            .enfant
            .wait_with_output()
            .map_err(|e| ErreurEncodage::Sortie(format!("attente FFmpeg : {e}")))?;

        Ok(ResumeFfmpeg {
            octets_entree: self.octets_entree,
            frames_poussees: self.frames_poussees,
            stderr: String::from_utf8_lossy(&sortie.stderr).trim().to_string(),
            code: sortie.status.code(),
        })
    }
}

/// Localise le sidecar FFmpeg embarqué.
///
/// Priorité à `SOS_FFMPEG`, puis aux emplacements connus du dépôt.
pub fn chemin_sidecar() -> Result<PathBuf, ErreurEncodage> {
    if let Some(p) = std::env::var_os("SOS_FFMPEG") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }

    let nom = "ffmpeg-x86_64-pc-windows-msvc.exe";
    let candidats = [
        PathBuf::from("binaries").join(nom),
        PathBuf::from("src-tauri").join("binaries").join(nom),
        PathBuf::from("..").join("binaries").join(nom),
        PathBuf::from("..").join("src-tauri").join("binaries").join(nom),
    ];

    for c in candidats {
        if c.is_file() {
            return Ok(c);
        }
    }

    Err(ErreurEncodage::Sortie(format!(
        "sidecar FFmpeg introuvable (cherché {nom}) ; définir SOS_FFMPEG"
    )))
}
