//! Encodage **matériel via FFmpeg** (NVENC/AMF/QSV) — chemin P0.
//!
//! Capture WGC → NV12 (CPU, conversion entière optimisée) → sidecar FFmpeg.
//! La conversion GPU existe (`color_gpu.rs`) mais la lecture du plan UV NV12
//! échoue sur le pilote de test ; le MFT zéro-copie reste le lot 0.2f.
//!
//! ```text
//! cargo run --release -p sos-moteur-natif --example encode_nvenc -- \
//!     [moniteur] [sortie.h264] [frames] [fps] [kbps] [encodeur] [duree_s]
//! ```

use std::time::{Duration, Instant};

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
    let encodeur = args.next().unwrap_or_else(|| "h264_nvenc".to_string());
    let duree_s: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);

    let ffmpeg = match chemin_sidecar() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

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

    let (largeur, hauteur) = capture.taille();
    let largeur = largeur & !1;
    let hauteur = hauteur & !1;
    println!("Capture : {largeur}×{hauteur}");

    let frame = match capture.prochaine_frame(Duration::from_secs(5)) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Aucune frame : {e}");
            std::process::exit(4);
        }
    };

    let debut_conv = Instant::now();
    let mut nv12 = match bgra_vers_nv12(&frame.pixels, largeur, hauteur) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Conversion impossible : {e}");
            std::process::exit(5);
        }
    };
    println!(
        "NV12 : {} octets en {} ms (conversion entière optimisée)",
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

    let frames = if duree_s > 0 {
        duree_s.saturating_mul(fps)
    } else {
        frames
    };
    println!(
        "Boucle : {} frames à {} fps{}",
        frames,
        fps,
        if duree_s > 0 {
            format!(" pendant {} s (temps réel)", duree_s)
        } else {
            " (sans cadencement)".to_string()
        }
    );

    let debut = Instant::now();
    let mut rafraichies = 0u32;
    let mut en_retard = 0u32;
    let mut retard_max_us = 0u64;
    let intervalle_us = 1_000_000 / fps as u64;

    for i in 0..frames {
        let cible = debut + Duration::from_micros(i as u64 * 1_000_000 / fps as u64);
        let maintenant = Instant::now();
        if cible > maintenant {
            std::thread::sleep(cible - maintenant);
        }

        // Retard = temps réel après la cible. Un retard > 1 intervalle est
        // une échéance manquée (le pipeline ne tient pas le fps).
        let apres = Instant::now();
        let retard = apres.saturating_duration_since(cible);
        if retard > Duration::from_micros(intervalle_us) {
            en_retard += 1;
        }
        if retard.as_micros() as u64 > retard_max_us {
            retard_max_us = retard.as_micros() as u64;
        }

        // Re-capture non bloquante : rafraîchit si l'écran change.
        if let Ok(f) = capture.prochaine_frame(Duration::from_millis(0)) {
            if f.largeur & !1 == largeur && f.hauteur & !1 == hauteur {
                if let Ok(v) = bgra_vers_nv12(&f.pixels, largeur, hauteur) {
                    nv12 = v;
                    rafraichies += 1;
                }
            }
        }

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
                "\n{} frames en {} ms ({:.1} fps d'alimentation, {} re-capturées)",
                frames,
                duree.as_millis(),
                frames as f64 / duree.as_secs_f64().max(0.001),
                rafraichies
            );
            println!(
                "{} octets NV12 poussés, débit cible {} kbps → {}",
                resume.octets_entree, kbps, sortie
            );
            let attendu = (duree.as_micros() as u64 * fps as u64 / 1_000_000) as u32;
            let drops = attendu.saturating_sub(frames);
            println!(
                "Frames poussées : {} | en retard : {} | retard max : {} ms | drops estimés : {}",
                resume.frames_poussees,
                en_retard,
                retard_max_us / 1000,
                drops
            );
        }
        Err(e) => {
            eprintln!("Finalisation impossible : {e}");
            std::process::exit(8);
        }
    }
}
