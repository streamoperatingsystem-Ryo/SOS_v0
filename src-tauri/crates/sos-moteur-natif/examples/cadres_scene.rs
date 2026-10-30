//! Harnais de la tranche « cadres » — cadre `mgstyle` + média découpé.
//!
//! Usage :
//! `cargo run --release --example cadres_scene -- <base> <rel_image> <sortie.mp4> [secondes] [style]`
//!
//! `base` = dossier de données (`%APPDATA%\com.streamos.v0\`), `rel_image`
//! = chemin relatif (`medias/test.png`). Trois widgets partagent le même
//! média : deux avec cadre (bord du canvas inclus), un `sansCadre` témoin.
//! Le MP4 doit montrer les coins **coupés** (octogone) sur les deux premiers
//! et un rectangle plein sur le troisième.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sos_moteur_api::{ApercuCfg, Codec, Conteneur, Moteur, SinkCfg, SortieCfg};
use sos_moteur_natif::moteur::MoteurNatif;

fn main() {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| ".".to_string());
    let rel = args
        .next()
        .unwrap_or_else(|| "medias/test.png".to_string());
    let sortie = args
        .next()
        .unwrap_or_else(|| "cadres-scene.mp4".to_string());
    let secondes: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(3);
    let style = args.next().unwrap_or_else(|| "mgstyle".to_string());
    let rel_json = rel.replace('\\', "\\\\");

    let scene_json = format!(
        r##"{{
            "canvasW": 1360, "canvasH": 768,
            "cadreWidget": {{
                "style": "{style}", "strokeWidth": 20,
                "couleur": "#000000", "couleurFin": "#8c00ff",
                "gradientAngle": 135, "actif": true, "bordActif": true
            }},
            "widgets": [
                {{"id":"w1","type":"media","x":80,"y":90,"largeur":320,"hauteur":220,"z":1,
                  "media":"{rel_json}","mediaFit":"remplir"}},
                {{"id":"w2","type":"media","x":470,"y":140,"largeur":260,"hauteur":260,"z":2,
                  "media":"{rel_json}","mediaFit":"remplir"}},
                {{"id":"temoin","type":"media","x":820,"y":300,"largeur":340,"hauteur":190,"z":3,
                  "media":"{rel_json}","mediaFit":"remplir","sansCadre":true}}
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
    if let Err(e) = moteur.configurer_scene(&scene, &HashMap::new(), Path::new(&base)) {
        eprintln!("Scène refusée : {e}");
        return;
    }
    let cadres = moteur.cadres();
    println!(
        "cadres configurés : {} (bord + widgets, style {style})",
        cadres.len()
    );

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
