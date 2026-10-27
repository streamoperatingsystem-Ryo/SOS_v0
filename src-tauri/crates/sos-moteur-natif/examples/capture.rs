//! Vérification de la capture WGC — LOT 0.2b.
//!
//! Énumère les moniteurs, capture **une** frame du moniteur choisi et
//! l'écrit en BMP (visualisable sans dépendance).
//!
//! ```text
//! cargo run -p sos-moteur-natif --example capture -- [index_moniteur] [sortie.bmp]
//! ```

use std::path::Path;
use std::time::{Duration, Instant};

use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::capture::wgc::{CaptureMoniteur, FrameBgra};
use sos_moteur_natif::capture::ErreurCapture;

fn main() {
    let mut args = std::env::args().skip(1);
    let index: u32 = args
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let sortie = args
        .next()
        .unwrap_or_else(|| "capture-moniteur.bmp".to_string());

    let moniteurs = match enumerer_moniteurs() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Énumération impossible : {e}");
            std::process::exit(2);
        }
    };

    println!("{} moniteur(s) :", moniteurs.len());
    for m in &moniteurs {
        println!(
            "  [{}] {} {}×{} @ ({},{}) adaptateur {} {}{}",
            m.index,
            m.nom,
            m.largeur,
            m.hauteur,
            m.x,
            m.y,
            m.adaptateur_index,
            if m.primaire { "primaire " } else { "" },
            format_args!("hmon={:#x}", m.hmon)
        );
    }

    let moniteur = match moniteurs.iter().find(|m| m.index == index) {
        Some(m) => m,
        None => {
            eprintln!("Moniteur {index} introuvable.");
            std::process::exit(2);
        }
    };

    let capture = match CaptureMoniteur::demarrer(moniteur) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Démarrage capture impossible : {e}");
            std::process::exit(3);
        }
    };

    println!(
        "\nCapture démarrée sur [{}] {}×{}",
        moniteur.index,
        capture.taille().0,
        capture.taille().1
    );

    let debut = Instant::now();
    let frame = match capture.prochaine_frame(Duration::from_secs(5)) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Aucune frame : {e}");
            std::process::exit(4);
        }
    };
    let duree = debut.elapsed();

    if frame.pixels.len() != frame.taille_attendue() {
        eprintln!(
            "Taille incohérente : {} octets reçus, {} attendus",
            frame.pixels.len(),
            frame.taille_attendue()
        );
        std::process::exit(5);
    }

    if let Err(e) = ecrire_bmp(Path::new(&sortie), &frame) {
        eprintln!("Écriture BMP impossible : {e}");
        std::process::exit(6);
    }

    println!(
        "Frame {}×{} reçue en {} ms → {} ({} octets)",
        frame.largeur,
        frame.hauteur,
        duree.as_millis(),
        sortie,
        frame.pixels.len()
    );
}

/// Écrit une frame BGRA en BMP 32 bits top-down (BI_RGB).
///
/// Aucune dépendance : suffisant pour vérifier visuellement la capture.
fn ecrire_bmp(chemin: &Path, frame: &FrameBgra) -> std::io::Result<()> {
    let largeur = frame.largeur;
    let hauteur = frame.hauteur;
    let taille_pixels = largeur
        .checked_mul(hauteur)
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "frame trop grande"))?;
    let offset = 54u32;
    let taille_fichier = offset + taille_pixels;

    let mut out = Vec::with_capacity(taille_fichier as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&taille_fichier.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());

    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(largeur as i32).to_le_bytes());
    out.extend_from_slice(&(-(hauteur as i32)).to_le_bytes()); // top-down
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&taille_pixels.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&frame.pixels);

    std::fs::write(chemin, out)
}

/// Garde-fou : le type d'erreur reste utilisé même si l'exemple sort tôt.
#[allow(dead_code)]
fn _type_verifie(_: ErreurCapture) {}
