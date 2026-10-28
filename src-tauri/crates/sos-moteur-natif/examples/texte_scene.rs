//! Harnais de la tranche P3 n°2 — titres rendus par le moteur natif.
//!
//! Usage :
//! `cargo run --release --example texte_scene -- <sortie.mp4> [secondes] [contenu]`
//!
//! Scène minimale : fond noir + 4 titres du fond (haut, bas, gauche, droite)
//! + un widget portant un titre.
//!
//! Le MP4 produit doit décoder et **contenir du texte** : contrôle par
//! extraction de frame (`ffmpeg -vf signalstats`, bande du titre nettement
//! plus lumineuse que le reste), pas seulement « ça compile ».

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sos_moteur_api::{Codec, Conteneur, Moteur, SinkCfg, SortieCfg};
use sos_moteur_natif::moteur::MoteurNatif;

fn main() {
    let mut args = std::env::args().skip(1);
    let sortie = args
        .next()
        .unwrap_or_else(|| "texte-scene.mp4".to_string());
    let secondes: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(3);
    let contenu = args.next().unwrap_or_else(|| "StreamOS".to_string());

    let scene_json = format!(
        r##"{{
            "canvasW": 1360, "canvasH": 768,
            "bgTitreHaut": "{contenu}",
            "bgTitreBas": "moteur natif",
            "bgTitreGauche": "P3",
            "bgTitreTaille": 96,
            "bgTitrePolice": "BebasNeue",
            "bgTitreGras": true,
            "bgTitreEspacement": 4,
            "bgTitreCouleur": "#ffffff",
            "bgTitreCouleurFin": "#8b5cf6",
            "bgTitreContourCouleur": "#000000",
            "bgTitreContourEpaisseur": 3,
            "widgets": [
                {{"id":"w1","type":"media","x":480,"y":300,"largeur":400,"hauteur":200,"z":1,
                  "titreHaut":"titre de widget","titreTaille":32,"titrePolice":"Anton",
                  "titreCouleur":"#ffffff","titreCouleurFin":"#22d3ee",
                  "titreContourCouleur":"#000000","titreContourEpaisseur":2}}
            ]
        }}"##
    );
    let scene: sos_scene::Scene = match serde_json::from_str(&scene_json) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Scène invalide : {e}");
            return;
        }
    };

    let moteur = MoteurNatif::nouveau();
    if let Err(e) = moteur.configurer_scene(&scene, &HashMap::new(), Path::new(".")) {
        eprintln!("Scène refusée : {e}");
        return;
    }
    println!("textes configurés : {}", moteur.textes().len());

    let cfg = SortieCfg {
        sinks: vec![SinkCfg::Fichier {
            chemin: PathBuf::from(&sortie),
            conteneur: Conteneur::Mp4,
        }],
        canvas: (1360, 768),
        fps: 60,
        codec: Codec::H264,
        debit_kbps: 6000,
        preset: "equilibre".to_string(),
        cle: None,
        // Harnais vidéo : sortie muette (pas de périphérique requis).
        audio: None,
    };
    if let Err(e) = moteur.demarrer_sortie(cfg) {
        eprintln!("Démarrage refusé : {e}");
        return;
    }

    let debut = Instant::now();
    while debut.elapsed() < Duration::from_secs(secondes) {
        std::thread::sleep(Duration::from_secs(1));
        let i = moteur.instantane();
        println!(
            "t={:>2}s etat={:?} encodeur={:?} composites={} kbps={}",
            debut.elapsed().as_secs(),
            moteur.etat(),
            moteur.encodeur(),
            i.frames_composites,
            i.kbps
        );
    }
    moteur.arreter();
    println!("Fin — état {:?}", moteur.etat());
}
