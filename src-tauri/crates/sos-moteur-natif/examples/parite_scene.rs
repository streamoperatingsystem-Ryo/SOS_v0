//! Harnais de parité — une scène JSON réelle → MP4, sans passer par l'app.
//!
//! Usage :
//! `cargo run --release --example parite_scene -- <scene.json> <base> <sortie.mp4> [secondes] [id=moniteur:0 …]`
//!
//! `scene.json` = une scène de `%APPDATA%\com.streamos.v0\scenes\`, `base` =
//! le dossier de données (les chemins `medias/…` y sont relatifs). Les
//! arguments suivants affectent une cible de capture à un widget :
//! `id=moniteur:0` ou `id=fenetre:123456`.
//!
//! Le MP4 produit sert aux sondes `signalstats` (région par région) et au
//! SSIM contre la capture du projecteur OBS. Sortie **muette** : ce harnais
//! mesure le rendu, pas l'audio.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sos_moteur_api::{Codec, Conteneur, Moteur, SinkCfg, SortieCfg};
use sos_moteur_natif::input::{EtatInput, EtatManette, EtatSouris};
use sos_moteur_natif::moteur::{CibleCapture, MoteurNatif};

fn main() {
    let mut args = std::env::args().skip(1);
    let scene_path = args.next().unwrap_or_else(|| "scene.json".to_string());
    let base = args.next().unwrap_or_else(|| ".".to_string());
    let sortie = args
        .next()
        .unwrap_or_else(|| "parite-scene.mp4".to_string());
    let secondes: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(5);

    // `id=moniteur:0` / `id=fenetre:1234` : les scènes réelles portent leur
    // `captureCible`, ceci n'est qu'un moyen de la forcer pour un test.
    // `touches=KeyW,KeyA` : force l'état de l'input viewer (aucun clavier
    // dans un harnais, et c'est le seul moyen de tester l'état « pressé »).
    // `souris=gauche,droite,milieu,x=120,y=340` : idem pour la souris.
    // `manette=connectee,btn=0,btn12,axe0=0.8` : idem pour la manette
    // (`btn=N` ou `btnN` = index pressé, `axeN=V` = valeur -1..1).
    let mut cibles: HashMap<String, CibleCapture> = HashMap::new();
    let mut touches: Vec<String> = Vec::new();
    let mut souris = EtatSouris::default();
    let mut avec_souris = false;
    let mut manette = EtatManette::default();
    let mut avec_manette = false;
    for spec in args {
        match spec.split_once('=') {
            Some(("touches", valeur)) => {
                touches = valeur
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            }
            Some(("souris", valeur)) => {
                avec_souris = true;
                for token in valeur.split(',') {
                    match token.trim() {
                        "gauche" => souris.gauche = true,
                        "milieu" => souris.milieu = true,
                        "droite" => souris.droite = true,
                        autre => {
                            if let Some((axe, v)) = autre.split_once('=') {
                                match axe {
                                    "x" => souris.x = v.parse().unwrap_or(0),
                                    "y" => souris.y = v.parse().unwrap_or(0),
                                    _ => eprintln!("jeton souris ignoré : {autre}"),
                                }
                            }
                        }
                    }
                }
            }
            Some(("manette", valeur)) => {
                avec_manette = true;
                for token in valeur.split(',') {
                    let token = token.trim();
                    if token == "connectee" {
                        manette.connectee = true;
                    } else if let Some((cle, v)) = token.split_once('=') {
                        if let Some(i) = cle
                            .strip_prefix("axe")
                            .and_then(|s| s.parse::<usize>().ok())
                        {
                            if i < 6 {
                                manette.axes[i] = v.parse().unwrap_or(0.0);
                            }
                        } else if cle == "btn" {
                            if let Ok(i) = v.parse::<usize>() {
                                if i < 20 {
                                    manette.boutons[i] = true;
                                }
                            }
                        } else {
                            eprintln!("jeton manette ignoré : {token}");
                        }
                    } else if let Some(i) = token
                        .strip_prefix("btn")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if i < 20 {
                            manette.boutons[i] = true;
                        }
                    } else {
                        eprintln!("jeton manette ignoré : {token}");
                    }
                }
            }
            Some((id, c)) => match cible(c) {
                Some(cible) => {
                    cibles.insert(id.to_string(), cible);
                }
                None => eprintln!("argument ignoré (attendu id=moniteur:0) : {spec}"),
            },
            None => eprintln!("argument ignoré (attendu clé=valeur) : {spec}"),
        }
    }

    let json = match std::fs::read_to_string(&scene_path) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("scène illisible ({scene_path}) : {e}");
            return;
        }
    };
    let scene: sos_scene::Scene = match serde_json::from_str(&json) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("scène invalide : {e}");
            return;
        }
    };

    let moteur = MoteurNatif::nouveau();
    if !touches.is_empty() || avec_souris || avec_manette {
        moteur.pousser_input(EtatInput {
            touches: touches.clone(),
            souris,
            manette,
        });
        println!(
            "input viewer : {} touche(s) {touches:?}, souris {souris:?}, manette {manette:?}",
            touches.len()
        );
    }
    if let Err(e) = moteur.configurer_scene(&scene, &cibles, Path::new(&base)) {
        eprintln!("Scène refusée : {e}");
        return;
    }
    let nb_transformes = moteur
        .medias()
        .iter()
        .filter(|m| !m.transform.est_neutre())
        .count();
    println!(
        "scène : {} widgets, {} source(s) de capture, {} texte(s), {} cadre(s), {} chat(s), {} boîte(s), {} média(s) transformé(s), {} plan(s) perspective",
        scene.widgets.len(),
        moteur.sources().len(),
        moteur.textes().len(),
        moteur.cadres().len(),
        moteur.chats().len(),
        moteur.boites().len(),
        nb_transformes,
        moteur.persps().len()
    );
    if let Some(raison) = moteur.instantane().raison_degradation {
        println!("dégradation signalée : {raison}");
    }

    let cfg = SortieCfg {
        sinks: vec![SinkCfg::Fichier {
            chemin: PathBuf::from(&sortie),
            conteneur: Conteneur::Mp4,
        }],
        canvas: (scene.canvasW, scene.canvasH),
        fps: 60,
        codec: Codec::H264,
        debit_kbps: 6000,
        preset: "equilibre".to_string(),
        cle: None,
        // Harnais vidéo : sortie muette (aucun périphérique requis).
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
            "t={:>2}s etat={:?} encodeur={:?} composites={} drops={} kbps={} latence={}ms",
            debut.elapsed().as_secs(),
            moteur.etat(),
            moteur.encodeur().map(|e| e.nom),
            i.frames_composites,
            i.frames_perdues,
            i.kbps,
            i.latence_ms
        );
    }
    moteur.arreter();
    println!("Fin — état {:?} → {sortie}", moteur.etat());
    if let Some(raison) = moteur.instantane().raison_degradation {
        println!("dégradation : {raison}");
    }
}

/// `moniteur:0` / `fenetre:1234` → cible de capture.
fn cible(spec: &str) -> Option<CibleCapture> {
    let (genre, valeur) = spec.split_once(':')?;
    match genre {
        "moniteur" => Some(CibleCapture::Moniteur {
            index: valeur.parse().ok()?,
        }),
        "fenetre" => Some(CibleCapture::Fenetre {
            hwnd: valeur.parse().ok()?,
        }),
        _ => None,
    }
}
