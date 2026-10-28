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
use crate::texte::{CibleTitre, PositionTitre, StyleTexte, TexteCfg, POLICE_DEFAUT};

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

/// Couleurs du texte d'un titre : `couleur` explicite sinon dégradé du cadre.
///
/// Miroir de `gradientCadre` (diffusion.html) : cadre actif → ses couleurs,
/// sinon le repli gris clair → gris. Jamais de couleur en dur hors repli.
fn degrade_defaut(scene: &Scene) -> ([u8; 4], [u8; 4]) {
    match scene.cadreWidget.as_ref().filter(|c| c.actif) {
        Some(cadre) => (
            couleur_hex(&cadre.couleur).unwrap_or([255, 255, 255, 255]),
            couleur_hex(&cadre.couleurFin).unwrap_or([255, 255, 255, 255]),
        ),
        None => ([224, 224, 224, 255], [154, 154, 154, 255]),
    }
}

/// `#rgb`, `#rrggbb` ou `#rrggbbaa` → RGBA. `None` si la chaîne est invalide.
fn couleur_hex(hex: &str) -> Option<[u8; 4]> {
    let h = hex.trim().strip_prefix('#')?;
    let octet = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        3 => {
            let mut c = [255u8; 4];
            for (i, ch) in h.chars().enumerate() {
                let v = octet(&ch.to_string().repeat(2))?;
                c[i] = v;
            }
            Some(c)
        }
        6 => Some([
            octet(&h[0..2])?,
            octet(&h[2..4])?,
            octet(&h[4..6])?,
            255,
        ]),
        8 => Some([
            octet(&h[0..2])?,
            octet(&h[2..4])?,
            octet(&h[4..6])?,
            octet(&h[6..8])?,
        ]),
        _ => None,
    }
}

/// Couleur de début/fin d'un titre : explicite, sinon dégradé du cadre.
fn couleurs_titre(
    debut: Option<&str>,
    fin: Option<&str>,
    defaut: ([u8; 4], [u8; 4]),
) -> ([u8; 4], [u8; 4]) {
    match debut.and_then(couleur_hex) {
        Some(c) => (c, fin.and_then(couleur_hex).unwrap_or(c)),
        None => defaut,
    }
}

/// Construit les titres de la scène : 4 positions du fond, puis les titres
/// de chaque widget (`titreHaut/Bas/Gauche/Droite`, **hors** du widget).
///
/// `z` : les titres du fond passent juste au-dessus du fond
/// (`i32::MIN + 1`) ; ceux d'un widget reprennent son `z`. Tous les titres
/// sont peints **après** la composition des calques (limite assumée de la
/// tranche, cf. `docs/features/moteur-natif.md` §29.7).
pub fn textes_depuis_scene(scene: &Scene) -> Vec<TexteCfg> {
    let defaut = degrade_defaut(scene);
    let cadre_fond = (
        0.0,
        0.0,
        scene.canvasW as f64,
        scene.canvasH as f64,
    );
    let couleurs_fond = couleurs_titre(
        scene.bgTitreCouleur.as_deref(),
        scene.bgTitreCouleurFin.as_deref(),
        defaut,
    );
    let style_fond = StyleTexte {
        police: scene
            .bgTitrePolice
            .clone()
            .unwrap_or_else(|| POLICE_DEFAUT.to_string()),
        taille: scene.bgTitreTaille.map(|v| v as f32).unwrap_or(24.0),
        gras: scene.bgTitreGras.unwrap_or(false),
        italique: scene.bgTitreItalique.unwrap_or(false),
        souligne: scene.bgTitreSouligne.unwrap_or(false),
        espacement: scene.bgTitreEspacement.unwrap_or(0.0) as f32,
        couleur: couleurs_fond.0,
        couleur_fin: couleurs_fond.1,
        contour_couleur: scene
            .bgTitreContourCouleur
            .as_deref()
            .and_then(couleur_hex)
            .unwrap_or([255, 255, 255, 255]),
        contour_epaisseur: scene.bgTitreContourEpaisseur.map(|v| v as f32).unwrap_or(2.0),
    };

    let mut textes = Vec::new();
    let positions = [
        (PositionTitre::Haut, scene.bgTitreHaut.as_deref()),
        (PositionTitre::Bas, scene.bgTitreBas.as_deref()),
        (PositionTitre::Gauche, scene.bgTitreGauche.as_deref()),
        (PositionTitre::Droite, scene.bgTitreDroite.as_deref()),
    ];
    for (position, contenu) in positions {
        if let Some(contenu) = contenu.filter(|c| !c.trim().is_empty()) {
            textes.push(TexteCfg {
                contenu: contenu.to_string(),
                position,
                cible: CibleTitre::Fond,
                cadre: cadre_fond,
                style: style_fond.clone(),
                z: i32::MIN + 1,
            });
        }
    }

    for w in &scene.widgets {
        let couleurs = couleurs_titre(
            w.titreCouleur.as_deref(),
            w.titreCouleurFin.as_deref(),
            defaut,
        );
        let style = StyleTexte {
            police: w
                .titrePolice
                .clone()
                .unwrap_or_else(|| POLICE_DEFAUT.to_string()),
            taille: w.titreTaille.map(|v| v as f32).unwrap_or(24.0),
            gras: w.titreGras.unwrap_or(false),
            italique: w.titreItalique.unwrap_or(false),
            souligne: w.titreSouligne.unwrap_or(false),
            espacement: w.titreEspacement.unwrap_or(0.0) as f32,
            couleur: couleurs.0,
            couleur_fin: couleurs.1,
            contour_couleur: w
                .titreContourCouleur
                .as_deref()
                .and_then(couleur_hex)
                .unwrap_or([255, 255, 255, 255]),
            contour_epaisseur: w.titreContourEpaisseur.map(|v| v as f32).unwrap_or(2.0),
        };
        let positions = [
            (PositionTitre::Haut, w.titreHaut.as_deref()),
            (PositionTitre::Bas, w.titreBas.as_deref()),
            (PositionTitre::Gauche, w.titreGauche.as_deref()),
            (PositionTitre::Droite, w.titreDroite.as_deref()),
        ];
        for (position, contenu) in positions {
            if let Some(contenu) = contenu.filter(|c| !c.trim().is_empty()) {
                textes.push(TexteCfg {
                    contenu: contenu.to_string(),
                    position,
                    cible: CibleTitre::Widget,
                    cadre: (w.x, w.y, w.largeur, w.hauteur),
                    style: style.clone(),
                    z: w.z,
                });
            }
        }
    }

    textes
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

    #[test]
    fn titres_du_fond_avec_style_et_z() {
        let scene = scene(
            r##"{
                "canvasW": 1920, "canvasH": 1080,
                "bgTitreHaut": "Bienvenue",
                "bgTitreBas": "  ",
                "bgTitreTaille": 48,
                "bgTitrePolice": "Anton",
                "bgTitreGras": true,
                "bgTitreContourEpaisseur": 0,
                "bgTitreCouleur": "#8b5cf6",
                "widgets": []
            }"##,
        );
        let textes = textes_depuis_scene(&scene);
        assert_eq!(textes.len(), 1, "le titre vide ne produit rien");
        let t = &textes[0];
        assert_eq!(t.contenu, "Bienvenue");
        assert_eq!(t.position, PositionTitre::Haut);
        assert_eq!(t.cible, CibleTitre::Fond);
        assert_eq!(t.cadre, (0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(t.z, i32::MIN + 1);
        assert_eq!(t.style.police, "Anton");
        assert_eq!(t.style.taille, 48.0);
        assert!(t.style.gras);
        assert_eq!(t.style.contour_epaisseur, 0.0);
        assert_eq!(t.style.couleur, [0x8b, 0x5c, 0xf6, 255]);
        assert_eq!(t.style.couleur_fin, [0x8b, 0x5c, 0xf6, 255]);
    }

    #[test]
    fn titre_sans_couleur_reprend_le_degrade_du_cadre() {
        let scene = scene(
            r##"{
                "bgTitreHaut": "Live",
                "cadreWidget": {"actif": true, "couleur": "#112233", "couleurFin": "#445566"},
                "widgets": []
            }"##,
        );
        let textes = textes_depuis_scene(&scene);
        assert_eq!(textes[0].style.couleur, [0x11, 0x22, 0x33, 255]);
        assert_eq!(textes[0].style.couleur_fin, [0x44, 0x55, 0x66, 255]);
    }

    #[test]
    fn cadre_inactif_donne_le_repli_gris() {
        let scene = scene(
            r##"{
                "bgTitreHaut": "Live",
                "cadreWidget": {"actif": false, "couleur": "#112233", "couleurFin": "#445566"},
                "widgets": []
            }"##,
        );
        let textes = textes_depuis_scene(&scene);
        assert_eq!(textes[0].style.couleur, [224, 224, 224, 255]);
        assert_eq!(textes[0].style.couleur_fin, [154, 154, 154, 255]);
    }

    #[test]
    fn titres_de_widget_hors_du_rect_avec_le_z_du_widget() {
        let scene = scene(
            r#"{
                "widgets": [
                    {"id":"a","type":"media","x":100,"y":200,"largeur":400,"hauteur":300,"z":7,
                     "titreHaut":"Titre A","titreDroite":"Cote"}
                ]
            }"#,
        );
        let textes = textes_depuis_scene(&scene);
        assert_eq!(textes.len(), 2);
        assert_eq!(textes[0].cible, CibleTitre::Widget);
        assert_eq!(textes[0].cadre, (100.0, 200.0, 400.0, 300.0));
        assert_eq!(textes[0].z, 7);
        assert_eq!(textes[0].position, PositionTitre::Haut);
        assert_eq!(textes[1].position, PositionTitre::Droite);
        assert!(textes[1].position.verticale());
    }

    #[test]
    fn couleurs_hex_trois_six_et_huit_chiffres() {
        assert_eq!(couleur_hex("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(couleur_hex("#8b5cf6"), Some([0x8b, 0x5c, 0xf6, 255]));
        assert_eq!(couleur_hex("#8b5cf680"), Some([0x8b, 0x5c, 0xf6, 0x80]));
        assert_eq!(couleur_hex("8b5cf6"), None);
        assert_eq!(couleur_hex("#zzzzzz"), None);
    }
}
