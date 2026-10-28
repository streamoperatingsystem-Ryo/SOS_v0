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
use std::path::Path;

use sos_scene::{CaptureCible, Scene};

use crate::media::{Fit, MediaCfg};
use crate::moteur::{CibleCapture, SourceCfg};

/// Traduit la cible persistée dans la scène en cible du moteur.
///
/// `Moniteur.index` est utilisé tel quel (l'ordre DXGI est stable entre
/// sessions sur une machine donnée) ; `name` est conservé pour une
/// re-résolution ultérieure. `Fenetre.hwnd` est valable pour la session.
fn cible_de(cible: &CaptureCible) -> CibleCapture {
    match cible {
        CaptureCible::Moniteur { index, .. } => CibleCapture::Moniteur { index: *index },
        CaptureCible::Fenetre { hwnd, .. } => CibleCapture::Fenetre {
            hwnd: *hwnd as isize,
        },
    }
}

/// Construit les sources du moteur depuis la scène et la table des cibles.
///
/// Priorité à `Widget.captureCible` (persisté dans la scène) ; `cibles` ne
/// sert que de repli pour les scènes anciennes ou les widgets sans cible
/// persistée. Un widget absent des deux est ignoré (pas de capture par
/// défaut).
pub fn sources_depuis_scene(
    scene: &Scene,
    cibles: &HashMap<String, CibleCapture>,
) -> Vec<SourceCfg> {
    scene
        .widgets
        .iter()
        .filter(|w| w.trou && w.largeur > 0.0 && w.hauteur > 0.0)
        .filter_map(|w| {
            let cible = w
                .captureCible
                .as_ref()
                .map(cible_de)
                .or_else(|| cibles.get(&w.id).copied())?;
            Some(SourceCfg {
                nom: w.id.clone(),
                cible,
                rect: (w.x, w.y, w.largeur, w.hauteur),
                z: w.z,
            })
        })
        .collect()
}

/// Construit les médias statiques de la scène (fond + widgets média).
///
/// `base` = dossier des données de l'app (`%APPDATA%\com.streamos.v0\`) ;
/// les chemins de la scène sont relatifs (`medias/<uuid>.<ext>`).
/// Le fond (`bgSlides` prioritaire sur `bgMedia`) est composé sous tout le
/// reste (`z = i32::MIN`). Une page transparente (`bgObsSource`) n'a pas de
/// média. Les trous et la caméra sont exclus (captures, pas des médias).
pub fn medias_depuis_scene(scene: &Scene, base: &Path) -> Vec<MediaCfg> {
    let mut medias = Vec::new();
    if scene.bgObsSource.is_none() {
        let fond_slides = scene
            .bgSlides
            .as_ref()
            .and_then(|slides| slides.first())
            .map(|slide| slide.rel.as_str());
        let fond = fond_slides.or(if scene.bgMedia.is_empty() {
            None
        } else {
            Some(scene.bgMedia.as_str())
        });
        if let Some(rel) = fond {
            medias.push(MediaCfg {
                chemin: base.join(rel),
                rect: (0.0, 0.0, scene.canvasW as f64, scene.canvasH as f64),
                z: i32::MIN,
                fit: Fit::depuis_nom(&scene.bgFit),
            });
        }
    }
    for w in &scene.widgets {
        if w.trou || w.widget_type == "camera" {
            continue;
        }
        if let Some(rel) = &w.media {
            medias.push(MediaCfg {
                chemin: base.join(rel),
                rect: (w.x, w.y, w.largeur, w.hauteur),
                z: w.z,
                fit: Fit::depuis_nom(&w.mediaFit),
            });
        }
    }
    medias
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

    #[test]
    fn medias_fond_et_widgets_avec_z() {
        let scene = scene(
            r#"{
                "canvasW": 1920, "canvasH": 1080,
                "bgMedia": "medias/fond.png",
                "widgets": [
                    {"id":"a","type":"media","x":10,"y":20,"largeur":300,"hauteur":200,"z":2,"media":"medias/a.png"},
                    {"id":"t","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":3,"trou":true,"media":"medias/t.png"},
                    {"id":"c","type":"camera","x":0,"y":0,"largeur":100,"hauteur":100,"z":1,"media":"medias/c.png"}
                ]
            }"#,
        );
        let medias = medias_depuis_scene(&scene, Path::new("C:/data"));
        assert_eq!(medias.len(), 2);
        assert_eq!(medias[0].z, i32::MIN);
        assert_eq!(medias[0].chemin, Path::new("C:/data/medias/fond.png"));
        assert_eq!(medias[0].rect, (0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(medias[1].z, 2);
        assert_eq!(medias[1].chemin, Path::new("C:/data/medias/a.png"));
        assert_eq!(medias[1].rect, (10.0, 20.0, 300.0, 200.0));
    }

    #[test]
    fn fond_obs_source_ne_charge_aucun_media() {
        let scene = scene(
            r#"{"bgMedia":"medias/fond.png","bgObsSource":"SOS-Fond-abc","widgets":[]}"#,
        );
        assert!(medias_depuis_scene(&scene, Path::new("C:/data")).is_empty());
    }

    #[test]
    fn cible_persistee_prime_sur_la_table() {
        let scene = scene(
            r#"{
                "widgets": [
                    {"id":"a","type":"media","x":0,"y":0,"largeur":640,"hauteur":360,"z":0,
                     "trou":true,
                     "captureCible":{"kind":"moniteur","name":"\\\\.\\DISPLAY1","index":1}}
                ]
            }"#,
        );
        let mut cibles = HashMap::new();
        cibles.insert("a".to_string(), CibleCapture::Fenetre { hwnd: 99 });
        let sources = sources_depuis_scene(&scene, &cibles);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].cible, CibleCapture::Moniteur { index: 1 });
    }

    #[test]
    fn cible_fenetre_persistee() {
        let scene = scene(
            r#"{
                "widgets": [
                    {"id":"w","type":"media","x":5,"y":6,"largeur":320,"hauteur":180,"z":0,
                     "trou":true,
                     "captureCible":{"kind":"fenetre","hwnd":42,"title":"Jeu","exe":"jeu.exe"}}
                ]
            }"#,
        );
        let sources = sources_depuis_scene(&scene, &HashMap::new());
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].cible, CibleCapture::Fenetre { hwnd: 42 });
        assert_eq!(sources[0].rect, (5.0, 6.0, 320.0, 180.0));
    }
}
