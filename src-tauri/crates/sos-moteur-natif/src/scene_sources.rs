//! Mapping scène → sources du moteur natif.
//!
//! La scène décrit **où** capturer (rect du widget `trou`), pas **quoi** :
//! la cible native (moniteur ou fenêtre) vit côté application, choisie par
//! l'utilisateur dans le sélecteur de capture. Ce module fait le pont, sans
//! modifier le contrat JSON des scènes (aucun champ ajouté à `Widget`).
//!
//! Seuls les widgets `trou` avec une cible connue et un rect non nul
//! produisent une source ; tout le reste (chat, médias, caméra tant que MF
//! n'existe pas) est ignoré — jamais d'invention.

use std::collections::HashMap;

use sos_scene::Scene;

use crate::moteur::{CibleCapture, SourceCfg};

/// Construit les sources du moteur depuis la scène et la table des cibles.
///
/// `cibles` : identifiant de widget → cible native. Un widget absent de la
/// table est ignoré (pas de capture par défaut).
pub fn sources_depuis_scene(
    scene: &Scene,
    cibles: &HashMap<String, CibleCapture>,
) -> Vec<SourceCfg> {
    scene
        .widgets
        .iter()
        .filter(|w| w.trou && w.largeur > 0.0 && w.hauteur > 0.0)
        .filter_map(|w| {
            cibles.get(&w.id).map(|cible| SourceCfg {
                nom: w.id.clone(),
                cible: *cible,
                rect: (w.x, w.y, w.largeur, w.hauteur),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(json: &str) -> Scene {
        serde_json::from_str(json).expect("scène JSON")
    }

    #[test]
    fn seuls_les_trous_avec_cible_produisent_une_source() {
        let scene = scene(
            r#"{
                "widgets": [
                    {"id":"a","type":"media","x":10,"y":20,"largeur":300,"hauteur":200,"z":0,"trou":true},
                    {"id":"b","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":0,"trou":true},
                    {"id":"c","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":0}
                ]
            }"#,
        );
        let mut cibles = HashMap::new();
        cibles.insert("a".to_string(), CibleCapture::Moniteur { index: 0 });
        cibles.insert("b".to_string(), CibleCapture::Fenetre { hwnd: 42 });
        cibles.insert("c".to_string(), CibleCapture::Moniteur { index: 1 });

        let sources = sources_depuis_scene(&scene, &cibles);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].nom, "a");
        assert_eq!(sources[0].rect, (10.0, 20.0, 300.0, 200.0));
        assert_eq!(sources[0].cible, CibleCapture::Moniteur { index: 0 });
        assert_eq!(sources[1].cible, CibleCapture::Fenetre { hwnd: 42 });
    }

    #[test]
    fn trou_sans_cible_ou_de_taille_nulle_est_ignore() {
        let scene = scene(
            r#"{
                "widgets": [
                    {"id":"sans-cible","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":0,"trou":true},
                    {"id":"nul","type":"media","x":0,"y":0,"largeur":0,"hauteur":100,"z":0,"trou":true}
                ]
            }"#,
        );
        let mut cibles = HashMap::new();
        cibles.insert("nul".to_string(), CibleCapture::Moniteur { index: 0 });
        assert!(sources_depuis_scene(&scene, &cibles).is_empty());
    }
}
