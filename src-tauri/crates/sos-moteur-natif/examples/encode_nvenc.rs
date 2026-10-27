//! Encodage **matériel via FFmpeg** (NVENC/AMF/QSV) — déblocage du P0.
//!
//! Capture WGC → NV12 (CPU, assumé) → sidecar FFmpeg → H.264.
//! Ce chemin remplace temporairement le MFT matériel (bloqué sur la machine
//! de test) pour permettre une mesure P0 honnête, avec encodage matériel.
//!
//! ```text
//! cargo run -p sos-moteur-natif --example encode_nvenc -- \
//!     [moniteur] [sortie.h264] [frames] [fps] [kbps] [encodeur]
//! ffmpeg -v error -i sortie.h264 -f null -
//! ```

use std::time::Instant;

use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::capture::wgc::CaptureMoniteur;
use sos_moteur_natif::color::bgra_vers_nv12;
use sos_moteur_natif::encode::ffmpeg::{chemin_sidecar, EncodeurFfmpeg};

fn main() {
    let mut args = std::env::args().skip(1);
    let index_moniteur: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let sortie = args
        .next()
        .unwrap_or_else(|| "capture-nvenc.h264".to_string());
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(120);
    let fps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(30);
    let kbps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(6000);
    let encodeur = args
        .next()
        .unwrap_or_else(|| "h264_nvenc".to_string());

    let ffmpeg = match chemin_sidecar() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    println!("FFmpeg : {}", ffmpeg.display());

    let moniteurs = match enumerer_moniteurs() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Énumération impossible : {e}");
            std::process::exit(2);
        }
    };
    let moniteur = match moniteurs.iter().find(|m| m.index == index_moniteur) {
        Some(m) => m,
        None => {
            eprintln!("Moniteur {index_moniteur} introuvable.");
            std::process::exit(2);
        }
    };

    let capture = match CaptureMoniteur::demarrer(moniteur) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Capture impossible : {e}");
            std::process::exit(3);
        }
    };

    let frame = match capture.prochaine_frame(std::time::Duration::from_secs(5)) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Aucune frame : {e}");
            std::process::exit(4);
        }
    };

    let largeur = frame.largeur & !1;
    let hauteur = frame.hauteur & !1;
    println!("Frame : {}×{}", largeur, hauteur);

    let debut_conv = Instant::now();
    let nv12 = match bgra_vers_nv12(&frame.pixels, largeur, hauteur) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Conversion impossible : {e}");
            std::process::exit(5);
        }
    };
    println!(
        "NV12 : {} octets en {} ms (copie CPU assumée pour le P0)",
        nv12.len(),
        debut_conv.elapsed().as_millis()
    );

    let chemin = std::path::Path::new(&sortie);
    let mut enc = match EncodeurFfmpeg::nouveau(&ffmpeg, &encodeur, largeur, hauteur, fps, kbps, chemin)
    {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Démarrage encodeur impossible : {e}");
            std::process::exit(6);
        }
    };
    println!("Encodeur : {encodeur}");

    let debut = Instant::now();
    for i in 0..frames {
        if let Err(e) = enc.pousser(&nv12) {
            eprintln!("Frame {i} refusée : {e}");
            std::process::exit(7);
        }
    }
    let duree = debut.elapsed();

    match enc.terminer() {
        Ok(resume) => {
            if !resume.stderr.is_empty() {
                eprintln!("FFmpeg stderr :\n{}", resume.stderr);
            }
            if !resume.succes() {
                eprintln!("FFmpeg a échoué (code {:?})", resume.code);
                std::process::exit(8);
            }
            println!(
                "\n{} frames en {} ms ({:.1} fps d'alimentation)",
                frames,
                duree.as_millis(),
                frames as f64 / duree.as_secs_f64().max(0.001)
            );
            println!(
                "{} octets NV12 poussés, débit cible {} kbps → {}",
                resume.octets_entree, kbps, sortie
            );
        }
        Err(e) => {
            eprintln!("Finalisation impossible : {e}");
            std::process::exit(8);
        }
    }
}
