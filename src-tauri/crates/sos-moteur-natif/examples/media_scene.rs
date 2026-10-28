//! Harnais de la tranche P3 n°1 — fond/média composés par le moteur.
//!
//! Usage :
//! `cargo run --example media_scene -- <base> <rel_image> <sortie.mp4> [secondes]`
//!
//! `base` = dossier de données (`%APPDATA%\com.streamos.v0\`), `rel_image`
//! = chemin relatif (`medias/test.png`). Le fichier produit doit décoder et
//! ne pas être noir (cf. `ffmpeg -vf signalstats`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sos_moteur_api::{Codec, Conteneur, Moteur, SinkCfg, SortieCfg};
use sos_moteur_natif::moteur::MoteurNatif;

fn main() {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| ".".to_string());
    let rel = args
        .next()
        .unwrap_or_else(|| "medias/test.png".to_string());
    let sortie = args
        .next()
        .unwrap_or_else(|| "media-scene.mp4".to_string());
    let secondes: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(3);

    let scene_json = format!(
        r#"{{"canvasW":1360,"canvasH":768,"bgMedia":"{}","widgets":[]}}"#,
        rel.replace('\\', "\\\\")
    );
    let scene: sos_scene::Scene = match serde_json::from_str(&scene_json) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Scène invalide : {e}");
            return;
        }
    };

    let moteur = MoteurNatif::nouveau();
    if let Err(e) = moteur.configurer_scene(&scene, &HashMap::new(), Path::new(&base)) {
        eprintln!("Scène refusée : {e}");
        return;
    }
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
