//! Harnais de validation du moteur natif (lot 1.6).
//!
//! Usage :
//! `cargo run --example moteur_natif -- sortie.mp4 [secondes] [fps] [kbps]`
//!
//! Critère go/no-go : le fichier produit doit se décoder sans erreur
//! (`ffmpeg -v error -i sortie.mp4 -f null -` → exit 0), à la durée demandée.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sos_moteur_api::{Codec, Conteneur, Moteur, SinkCfg, SortieCfg};
use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::moteur::{CibleCapture, MoteurNatif, SourceCfg};

fn main() {
    let mut args = std::env::args().skip(1);
    let sortie = args
        .next()
        .unwrap_or_else(|| "moteur-natif.mp4".to_string());
    let secondes: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(30);
    let fps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(60);
    let kbps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(6000);

    let moniteurs = match enumerer_moniteurs() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Énumération impossible : {e}");
            return;
        }
    };
    let principal = match moniteurs.iter().find(|m| m.primaire) {
        Some(m) => m,
        None => match moniteurs.first() {
            Some(m) => m,
            None => {
                eprintln!("Aucun moniteur.");
                return;
            }
        },
    };
    let (largeur, hauteur) = (principal.largeur & !1, principal.hauteur & !1);
    println!(
        "=== Moteur natif — canvas {largeur}x{hauteur} @ {fps} fps, {} kbps ===",
        kbps
    );

    let cfg = SortieCfg {
        sinks: vec![SinkCfg::Fichier {
            chemin: PathBuf::from(&sortie),
            conteneur: Conteneur::Mp4,
        }],
        canvas: (largeur, hauteur),
        fps,
        codec: Codec::H264,
        debit_kbps: kbps,
        preset: "equilibre".to_string(),
        cle: None,
    };

    let moteur = MoteurNatif::nouveau();
    if let Err(e) = moteur.configurer_sources(vec![SourceCfg {
        nom: format!("moniteur {}", principal.index),
        cible: CibleCapture::Moniteur {
            index: principal.index,
        },
        rect: (0.0, 0.0, largeur as f64, hauteur as f64),
        z: 0,
    }]) {
        eprintln!("Sources refusées : {e}");
        return;
    }
    if let Err(e) = moteur.demarrer_sortie(cfg) {
        eprintln!("Démarrage refusé : {e}");
        return;
    }

    let debut = Instant::now();
    while debut.elapsed() < Duration::from_secs(secondes) {
        std::thread::sleep(Duration::from_secs(1));
        let i = moteur.instantane();
        println!(
            "t={:>2}s etat={:?} encodeur={:?} capturees={} composites={} perdues={} kbps={} latence={}ms",
            debut.elapsed().as_secs(),
            moteur.etat(),
            moteur.encodeur(),
            i.frames_capturees,
            i.frames_composites,
            i.frames_perdues,
            i.kbps,
            i.latence_ms
        );
        if let sos_moteur_api::MoteurEtat::Erreur(m) = moteur.etat() {
            eprintln!("Erreur moteur : {m}");
            break;
        }
    }
    moteur.arreter();
    println!("Fin — état {:?}", moteur.etat());
}
