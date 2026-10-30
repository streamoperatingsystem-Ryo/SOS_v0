//! Harnais de la tranche « chat » — widget chat alimenté **pendant** la sortie.
//!
//! Usage :
//! `cargo run --release --example chat_scene -- <sortie.mp4> [secondes]`
//!
//! Le harnais pousse un message **chaque seconde** pendant l'enregistrement :
//! la preuve attendue est que les messages apparaissent sans redémarrer la
//! sortie ni reconfigurer la scène (`pousser_chat` n'est jamais refusé).
//! Le widget est en `bulles` (fond uniforme clair), le dernier message est en
//! bas et le plus ancien sort par le haut.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sos_moteur_api::{ApercuCfg, Codec, Conteneur, Moteur, SinkCfg, SortieCfg};
use sos_moteur_natif::chat::MessageChat;
use sos_moteur_natif::moteur::MoteurNatif;

fn main() {
    let mut args = std::env::args().skip(1);
    let sortie = args
        .next()
        .unwrap_or_else(|| "chat-scene.mp4".to_string());
    let secondes: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(6);

    let scene_json = r##"{
        "canvasW": 1360, "canvasH": 768,
        "widgets": [
            {"id":"chat","type":"chat","x":334,"y":180,"largeur":692,"hauteur":420,"z":1,
             "chatFiltre":"unifie","taillePolice":18,"chatStyle":"bulles"}
        ]
    }"##;
    let scene: sos_scene::Scene = match serde_json::from_str(scene_json) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Scène invalide : {e}");
            return;
        }
    };

    let moteur = MoteurNatif::nouveau();
    if let Err(e) = moteur.configurer_scene(&scene, &std::collections::HashMap::new(), std::path::Path::new(".")) {
        eprintln!("Scène refusée : {e}");
        return;
    }
    println!("widgets chat : {}", moteur.chats().len());

    // Messages présents AVANT le démarrage (l'historique de la session).
    for (pseudo, texte, couleur) in [
        ("Ryo", "bienvenue sur le stream", Some("#ff7f50")),
        ("modo_42", "!cmd", Some("#00ff7f")),
        ("Zoe", "ce cadre est découpé au polygone exact du SVG", None),
        ("viewer_42", "les accents éàü passent aussi", None),
    ] {
        moteur.pousser_chat(MessageChat::nouveau("twitch", pseudo, texte, couleur));
    }

    let cfg = SortieCfg {
        sinks: vec![SinkCfg::Fichier {
            chemin: PathBuf::from(&sortie),
            conteneur: Conteneur::Mp4,
        }],
        canvas: (1360, 768),
        resolution: None,
        fps: 60,
        apercu: ApercuCfg::default(),
        codec: Codec::H264,
        debit_kbps: 6000,
        preset: "equilibre".to_string(),
        cle: None,
        audio: None,
    };
    if let Err(e) = moteur.demarrer_sortie(cfg) {
        eprintln!("Démarrage refusé : {e}");
        return;
    }

    let debut = Instant::now();
    let mut n = 0;
    while debut.elapsed() < Duration::from_secs(secondes) {
        std::thread::sleep(Duration::from_secs(1));
        n += 1;
        // Message poussé PENDANT la sortie : aucun `DejaActif`.
        moteur.pousser_chat(MessageChat::nouveau(
            if n % 2 == 0 { "kick" } else { "twitch" },
            format!("viewer_{n}"),
            format!("message en direct n°{n}"),
            None,
        ));
        let i = moteur.instantane();
        println!(
            "t={:>2}s chat={} etat={:?} composites={} kbps={}",
            debut.elapsed().as_secs(),
            moteur.chat_len(),
            moteur.etat(),
            i.frames_composites,
            i.kbps
        );
    }
    moteur.arreter();
    println!("Fin — état {:?}", moteur.etat());
}
