//! Harnais de la tranche audio — enregistrement avec son (loopback + micro).
//!
//! Usage :
//! `cargo run --release --example audio_scene -- <sortie.mp4> [secondes] [loop|mic|deux]`
//!
//! Le MP4 produit doit contenir **deux flux** (H.264 + AAC), et le log doit
//! annoncer une durée audio proche de la durée vidéo (dérive A/V).
//! Vérification : `ffprobe -show_streams`, puis `-af volumedetect` pour la
//! présence de signal (silence attendu si rien ne joue sur la machine).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sos_moteur_api::{AudioCfg, Codec, Conteneur, Moteur, SinkCfg, SortieCfg};
use sos_moteur_natif::moteur::MoteurNatif;

/// Journal minimal : le crate trace via `log`, les harnais n'installent pas
/// de logger. Sans ça, les durées audio (dérive A/V) restent invisibles.
struct Journal;
impl log::Log for Journal {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, enregistrement: &log::Record) {
        eprintln!("[{}] {}", enregistrement.level(), enregistrement.args());
    }
    fn flush(&self) {}
}
static JOURNAL: Journal = Journal;

fn main() {
    let _ = log::set_logger(&JOURNAL);
    log::set_max_level(log::LevelFilter::Info);

    let mut args = std::env::args().skip(1);
    let sortie = args
        .next()
        .unwrap_or_else(|| "audio-scene.mp4".to_string());
    let secondes: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(3);
    let sources = args.next().unwrap_or_else(|| "deux".to_string());
    let gain_loopback: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let gain_micro: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1000);

    let audio = AudioCfg {
        loopback: sources != "mic",
        micro: sources != "loop",
        gain_loopback,
        gain_micro,
    };

    // Scène minimale avec un titre : le MP4 doit aussi rester lisible à l'œil.
    let scene_json = r##"{
        "canvasW": 1360, "canvasH": 768,
        "bgTitreHaut": "audio",
        "bgTitreTaille": 96,
        "bgTitrePolice": "BebasNeue",
        "bgTitreCouleur": "#ffffff",
        "bgTitreCouleurFin": "#22d3ee",
        "bgTitreContourCouleur": "#000000",
        "bgTitreContourEpaisseur": 3,
        "widgets": []
    }"##;
    let scene: sos_scene::Scene = match serde_json::from_str(scene_json) {
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
        audio: Some(audio),
    };
    println!("audio demandé : {audio:?}");
    if let Err(e) = moteur.demarrer_sortie(cfg) {
        eprintln!("Démarrage refusé : {e}");
        return;
    }

    let debut = Instant::now();
    while debut.elapsed() < Duration::from_secs(secondes) {
        std::thread::sleep(Duration::from_secs(1));
        let i = moteur.instantane();
        println!(
            "t={:>2}s etat={:?} encodeur={:?} composites={} degradation={:?}",
            debut.elapsed().as_secs(),
            moteur.etat(),
            moteur.encodeur().map(|e| e.nom),
            i.frames_composites,
            i.raison_degradation
        );
    }
    moteur.arreter();
    println!("Fin — état {:?}", moteur.etat());
}
