//! Vérification de l'encodage MFT — LOT 0.2c.
//!
//! Capture une frame d'un moniteur, la convertit en NV12 et encode une
//! séquence H.264 élémentaire (Annex B). Le fichier se vérifie avec ffmpeg :
//!
//! ```text
//! cargo run -p sos-moteur-natif --example encode -- [moniteur] [sortie.h264] [frames] [fps] [kbps]
//! ffmpeg -i sortie.h264 -f null -
//! ```

use std::io::Write;
use std::time::Instant;

use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::capture::wgc::{CaptureMoniteur, FrameBgra};
use sos_moteur_natif::color::bgra_vers_nv12;
use sos_moteur_natif::encode::detect::{lister_tous, EncodeurDispo};
use sos_moteur_natif::encode::mft::EncodeurMft;
use sos_moteur_natif::encode::{Codec, PaquetEncode};
use sos_moteur_natif::gpu::ContexteGpu;

fn main() {
    let mut args = std::env::args().skip(1);
    let index_moniteur: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let sortie = args
        .next()
        .unwrap_or_else(|| "capture-encodee.h264".to_string());
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(120);
    let fps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(30);
    let kbps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(6000);
    let mode = args.next().unwrap_or_default();

    // --- Capture ---
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

    let frame = dimensions_paires(frame);
    println!("Frame capturée : {}×{}", frame.largeur, frame.hauteur);

    // --- Conversion NV12 (CPU, spike) ---
    let debut_conv = Instant::now();
    let nv12 = match bgra_vers_nv12(&frame.pixels, frame.largeur, frame.hauteur) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Conversion impossible : {e}");
            std::process::exit(5);
        }
    };
    let duree_conv = debut_conv.elapsed();
    println!(
        "NV12 : {} octets en {} ms",
        nv12.len(),
        duree_conv.as_millis()
    );

    // --- Détection ---
    let encodeurs = match lister_tous(Codec::H264) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Détection impossible : {e}");
            std::process::exit(6);
        }
    };
    if encodeurs.is_empty() {
        eprintln!("Aucun encodeur H.264 disponible sur cette machine.");
        std::process::exit(6);
    }
    println!("\nEncodeurs H.264 détectés :");
    for e in &encodeurs {
        println!(
            "  - {} [{}]",
            e.nom,
            if e.materiel { "matériel" } else { "logiciel" }
        );
    }

    // Contexte GPU dédié pour tester QSV sur l'adaptateur Intel (index 1).
    let contexte_intel = if mode == "intel" {
        match ContexteGpu::creer(Some(1)) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("Contexte Intel impossible : {e}");
                std::process::exit(3);
            }
        }
    } else {
        None
    };

    let choisi = match mode.as_str() {
        "logiciel" => encodeurs.iter().find(|e| !e.materiel).unwrap_or(&encodeurs[0]),
        "intel" => encodeurs
            .iter()
            .find(|e| e.nom.contains("Quick Sync"))
            .unwrap_or(&encodeurs[0]),
        _ => encodeurs.iter().find(|e| e.materiel).unwrap_or(&encodeurs[0]),
    };
    println!(
        "\nEncodeur retenu : {} [{}]",
        choisi.nom,
        if choisi.materiel { "matériel" } else { "logiciel" }
    );

    // --- Encodage ---
    // Le matériel exige des entrées D3D11 (lot 0.2d). Si l'initialisation ou
    // la première frame échoue, on bascule sur le logiciel en le disant.
    let (mut encodeur, premiers) = match essayer(
        choisi,
        frame.largeur,
        frame.hauteur,
        fps,
        kbps,
        &nv12,
        contexte_intel.as_ref(),
    ) {
        Ok(v) => v,
        Err(e) if choisi.materiel => {
            eprintln!("⚠️  Encodeur matériel indisponible : {e}");
            let secours = encodeurs
                .iter()
                .find(|x| !x.materiel)
                .ok_or_else(|| "aucun encodeur logiciel de secours".to_string());
            match secours.and_then(|s| {
                println!("Bascule sur : {} [logiciel]", s.nom);
                essayer(s, frame.largeur, frame.hauteur, fps, kbps, &nv12, None)
            }) {
                Ok(v) => v,
                Err(e2) => {
                    eprintln!("Aucun encodeur exploitable : {e2}");
                    std::process::exit(7);
                }
            }
        }
        Err(e) => {
            eprintln!("Initialisation encodeur impossible : {e}");
            std::process::exit(7);
        }
    };

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

    for p in premiers {
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

    for i in 1..frames {
        let pts_us = (i as i64 * 1_000_000) / fps as i64;
        match encodeur.encoder(&nv12, pts_us) {
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
        "\n{} frames encodées en {} ms ({:.1} fps effectif)",
        frames,
        duree.as_millis(),
        frames as f64 / duree.as_secs_f64().max(0.001)
    );
    println!(
        "{} paquets, {} image(s)-clé, {} octets → {}",
        paquets, cles, octets, sortie
    );
    println!(
        "\nDébit moyen : {:.0} kbps (cible {})",
        (octets as f64 * 8.0) / (frames as f64 / fps as f64).max(0.001) / 1000.0,
        kbps
    );
}

/// Crée un encodeur et encode la première frame (validation immédiate).
fn essayer(
    dispo: &EncodeurDispo,
    largeur: u32,
    hauteur: u32,
    fps: u32,
    kbps: u32,
    nv12: &[u8],
    contexte: Option<&ContexteGpu>,
) -> Result<(EncodeurMft, Vec<PaquetEncode>), String> {
    let mut encodeur = EncodeurMft::nouveau_avec(
        dispo.activate(),
        Codec::H264,
        largeur,
        hauteur,
        fps,
        kbps,
        contexte,
    )
    .map_err(|e| e.to_string())?;
    let paquets = encodeur.encoder(nv12, 0).map_err(|e| e.to_string())?;
    Ok((encodeur, paquets))
}

/// Ramène une frame à des dimensions paires (exigence NV12) par recadrage.
fn dimensions_paires(frame: FrameBgra) -> FrameBgra {
    if frame.largeur % 2 == 0 && frame.hauteur % 2 == 0 {
        return frame;
    }
    let largeur = frame.largeur & !1;
    let hauteur = frame.hauteur & !1;
    let mut pixels = Vec::with_capacity(largeur as usize * hauteur as usize * 4);
    for y in 0..hauteur as usize {
        let debut = y * frame.largeur as usize * 4;
        let fin = debut + largeur as usize * 4;
        pixels.extend_from_slice(&frame.pixels[debut..fin]);
    }
    FrameBgra {
        largeur,
        hauteur,
        pixels,
    }
}
