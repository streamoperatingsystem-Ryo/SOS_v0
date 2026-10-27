//! Vérification du chemin **zéro-copie** — LOT 0.2d.
//!
//! Capture WGC → texture BGRA → conversion NV12 par D3D11 Video Processor →
//! encodage MFT matériel sur la **même texture GPU**. Aucune frame brute ne
//! traverse la mémoire système.
//!
//! ```text
//! cargo run -p sos-moteur-natif --example encode_gpu -- [moniteur] [sortie.h264] [frames] [fps] [kbps]
//! ffmpeg -i sortie.h264 -f null -
//! ```

use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::capture::wgc::CaptureMoniteur;
use sos_moteur_natif::color_gpu::ConvertisseurNv12;
use sos_moteur_natif::encode::detect::lister_tous;
use sos_moteur_natif::encode::mft::EncodeurMft;
use sos_moteur_natif::encode::Codec;
use sos_moteur_natif::gpu::ContexteGpu;

fn main() {
    let mut args = std::env::args().skip(1);
    let index_moniteur: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let sortie = args
        .next()
        .unwrap_or_else(|| "capture-gpu.h264".to_string());
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(60);
    let fps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(30);
    let kbps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(4000);

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

    // Un seul contexte GPU pour la capture ET l'encodage : condition du
    // zéro-copie (plan §11).
    let contexte = match ContexteGpu::creer(Some(moniteur.adaptateur_index)) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Contexte GPU impossible : {e}");
            std::process::exit(3);
        }
    };
    println!(
        "Contexte GPU créé (adaptateur {})",
        moniteur.adaptateur_index
    );

    let capture = match CaptureMoniteur::demarrer_avec(moniteur, contexte.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Capture impossible : {e}");
            std::process::exit(3);
        }
    };

    let (largeur, hauteur) = capture.taille();
    let largeur = largeur & !1;
    let hauteur = hauteur & !1;
    println!("Capture : {}×{}", largeur, hauteur);

    // Texture BGRA de la capture (GPU).
    let texture_bgra = match capture.prochaine_texture(Duration::from_secs(5)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Aucune texture : {e}");
            std::process::exit(4);
        }
    };

    // Conversion GPU BGRA → NV12.
    let convertisseur = match ConvertisseurNv12::nouveau(&contexte, largeur, hauteur, fps) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Video processor impossible : {e}");
            std::process::exit(5);
        }
    };
    let debut_conv = Instant::now();
    let texture_nv12 = match convertisseur.convertir(&texture_bgra) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Conversion GPU impossible : {e}");
            std::process::exit(5);
        }
    };
    println!(
        "Conversion GPU BGRA→NV12 : {} ms",
        debut_conv.elapsed().as_millis()
    );

    // Encodeur matériel sur le MÊME device.
    let encodeurs = match lister_tous(Codec::H264) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Détection impossible : {e}");
            std::process::exit(6);
        }
    };
    let choisi = match encodeurs.iter().find(|e| e.materiel) {
        Some(e) => e,
        None => {
            eprintln!("Aucun encodeur matériel H.264 détecté.");
            std::process::exit(6);
        }
    };
    println!("Encodeur : {} [matériel]", choisi.nom);

    let mut encodeur = match EncodeurMft::nouveau_avec(
        choisi.activate(),
        Codec::H264,
        largeur,
        hauteur,
        fps,
        kbps,
        Some(&contexte),
    ) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Initialisation encodeur impossible : {e}");
            std::process::exit(7);
        }
    };
    println!("Modèle : {}", if encodeur.asynchrone() { "asynchrone" } else { "synchrone" });

    let mut fichier = match std::fs::File::create(&sortie) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Création {sortie} impossible : {e}");
            std::process::exit(8);
        }
    };

    let debut = Instant::now();
    let mut octets = 0usize;
    let mut paquets = 0usize;
    let mut cles = 0usize;

    for i in 0..frames {
        let pts_us = (i as i64 * 1_000_000) / fps as i64;
        match encodeur.encoder_texture(&texture_nv12, pts_us) {
            Ok(liste) => {
                for p in liste {
                    octets += p.data.len();
                    paquets += 1;
                    if p.cle {
                        cles += 1;
                    }
                    if let Err(e) = fichier.write_all(&p.data) {
                        eprintln!("Écriture impossible : {e}");
                        std::process::exit(9);
                    }
                }
            }
            Err(e) => {
                eprintln!("Encodage frame {i} impossible : {e}");
                std::process::exit(10);
            }
        }
    }

    match encodeur.terminer() {
        Ok(liste) => {
            for p in liste {
                octets += p.data.len();
                paquets += 1;
                if p.cle {
                    cles += 1;
                }
                let _ = fichier.write_all(&p.data);
            }
        }
        Err(e) => eprintln!("Finalisation impossible : {e}"),
    }

    let _ = fichier.flush();
    let duree = debut.elapsed();

    println!(
        "\n{} frames en {} ms ({:.1} fps effectif)",
        frames,
        duree.as_millis(),
        frames as f64 / duree.as_secs_f64().max(0.001)
    );
    println!(
        "{} paquets, {} image(s)-clé, {} octets → {}",
        paquets, cles, octets, sortie
    );
    println!(
        "Débit moyen : {:.0} kbps (cible {})",
        (octets as f64 * 8.0) / (frames as f64 / fps as f64).max(0.001) / 1000.0,
        kbps
    );
}
