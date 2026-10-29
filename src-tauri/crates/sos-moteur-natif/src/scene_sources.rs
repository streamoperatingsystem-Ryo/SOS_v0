//! Mapping scène → sources du moteur natif.
//!
//! La scène décrit **où** capturer (rect du widget porteur d'une cible),
//! pas **quoi** : la cible native (moniteur ou fenêtre) vit côté
//! application, choisie par l'utilisateur dans le sélecteur de capture. Ce
//! module fait le pont, sans modifier le contrat JSON des scènes (aucun
//! champ ajouté à `Widget`).
//!
//! Seuls les widgets portant une cible (`captureCible` persistée, ou à
//! défaut `cibles[id]` pour le harnais) et un rect non nul produisent une
//! source ; tout le reste (chat, médias, caméra tant que MF n'existe pas)
//! est ignoré — jamais d'invention. Le flag `trou` est un concept du mode
//! OBS : le moteur natif ne le lit pas (D8).

use std::collections::HashMap;
use std::path::Path;

use sos_scene::{CadreConfig, CaptureCible, Scene, Widget};

use crate::cadres::{couleur_hex, CadreCfg, StyleCadre};
use crate::chat::{ChatCfg, FILTRE_UNIFIE};
use crate::input::{Disposition, InputCfg, MappingManette, ModeInput};
use crate::media::{CarrouselCfg, EffetsCfg, Fit, LectureCfg, MediaCfg, PerspCfg, Transform2D};
use crate::moteur::{CibleCapture, SourceCfg};
use crate::texte::{CibleTitre, PositionTitre, StyleBoite, StyleTexte, TexteCfg, POLICE_DEFAUT};

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

/// Vrai si le widget est un emplacement de capture : une cible existe
/// (`captureCible` persistée, ou `cibles[id]` pour le harnais
/// `parite_scene`). Remplace l'ancien marqueur `trou`, concept du mode OBS
/// (D8). `medias_depuis_scene` / `boites_depuis_scene` ne reçoivent pas la
/// table `cibles` (signature fixée par `moteur.rs`, hors périmètre N1) :
/// leur exclusion équivalente est `w.captureCible.is_some()`.
fn est_capture(w: &Widget, cibles: &HashMap<String, CibleCapture>) -> bool {
    w.captureCible.is_some() || cibles.contains_key(&w.id)
}

/// État de lecture persisté d'un média — miroir `applyMediaState` /
/// `applyBgState` (diffusion.html). La boucle A–B n'existe que si les deux
/// bornes sont des nombres valides avec `b > a` (le DOM rejette sinon).
/// `mediaTime`/`bgTime` ne sont volontairement pas portés : le tracker DOM
/// est seedé à la création → aucun seek initial, cf. `LectureCfg`.
fn lecture(paused: bool, a: Option<f64>, b: Option<f64>) -> LectureCfg {
    LectureCfg {
        paused,
        boucle: match (a, b) {
            (Some(a), Some(b)) if b > a => Some((a, b)),
            _ => None,
        },
    }
}

/// Carrousel persisté d'un média — la liste **non vide** est prioritaire
/// sur le média unique (`carrousel.md`) ; `slidesSecOf` = `max(1, sec||5)`
/// (min 1 s du `Math.max(1, …)` DOM, défaut 5 s quand le champ est absent).
/// `ease` : fondu `ease-in-out` CSS des `.slide` (widget) vs alpha linéaire
/// `kFade` de `dessinerContenuFond` (fond).
fn carrousel(
    slides: Option<&[sos_scene::MediaSlide]>,
    sec: Option<f64>,
    base: &Path,
    ease: bool,
) -> Option<CarrouselCfg> {
    let slides = slides.filter(|s| !s.is_empty())?;
    Some(CarrouselCfg {
        slides: slides.iter().map(|s| base.join(&s.rel)).collect(),
        sec: sec.unwrap_or(5.0).max(1.0),
        ease,
    })
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
        .filter(|w| est_capture(w, cibles) && w.largeur > 0.0 && w.hauteur > 0.0)
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
                cadre: cadre_de_widget(scene, w),
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
/// média. Les widgets de capture (cible persistée) et la caméra sont
/// exclus (captures, pas des médias).
///
/// Transforms 2D (miroir `applyMediaStyle` / `dessinerContenuFond`) : le fond
/// porte `bgRot`/`bgOffset*`/`bgMiroir*` en chaîne interne et `bgZoom` en
/// `zoom_ecran` (le `scale(z)` posé sur `#bg`, espace écran) ; un widget
/// porte `mediaZoom`/`mediaRot`/`mediaOffset*`/`mediaMiroir*`. `bgFit` et
/// `mediaFit` n'ont PAS le même `centrer` (`depuis_nom_fond`, cf.
/// `fitRects`).
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
                fit: Fit::depuis_nom_fond(&scene.bgFit),
                transform: Transform2D {
                    // `bgZoom` n'est pas dans la chaîne du canvas (peint sur
                    // `#bg` entier, espace écran) → `zoom_ecran`, pas `zoom`.
                    zoom: 1.0,
                    rot: scene.bgRot,
                    offset_x: scene.bgOffsetX,
                    offset_y: scene.bgOffsetY,
                    miroir_h: scene.bgMiroirH,
                    miroir_v: scene.bgMiroirV,
                    zoom_ecran: scene.bgZoom,
                },
                // `chaineFilter` + `bgPixel` (chemin canvas du fond).
                effets: EffetsCfg {
                    lum: scene.bgLum,
                    contraste: scene.bgContraste,
                    teinte: scene.bgTeinte,
                    flou: scene.bgFlou,
                    pixel: scene.bgPixel,
                },
                // Le fond n'est pas un widget : jamais de cadre.
                cadre: None,
                morphs: scene.bgMorphs.clone().unwrap_or_default(),
                // `bgPixel` est déjà dans `effets.pixel` — chemin morph
                // inclus (le DOM pixellise aussi dans l'offscreen).
                pixel_morph: scene.bgPixel,
                lecture: lecture(scene.bgPaused, scene.bgLoopA, scene.bgLoopB),
                // `bgSlides` gagne sur `bgMedia` (priorité du DOM). Le fondu
                // du fond est linéaire (`globalAlpha`), pas eased.
                carrousel: carrousel(
                    scene.bgSlides.as_deref(),
                    scene.bgSlidesSec,
                    base,
                    false,
                ),
            });
        }
    }
    for w in &scene.widgets {
        // Même exclusion que `boites_depuis_scene` : la table `cibles`
        // n'atteint pas cette fonction (signature fixée par `moteur.rs`,
        // hors périmètre N1) → seule la cible persistée exclut le média.
        if w.captureCible.is_some() || w.widget_type == "camera" {
            continue;
        }
        if let Some(rel) = &w.media {
            medias.push(MediaCfg {
                chemin: base.join(rel),
                rect: (w.x, w.y, w.largeur, w.hauteur),
                z: w.z,
                fit: Fit::depuis_nom(&w.mediaFit),
                transform: Transform2D {
                    zoom: w.mediaZoom,
                    rot: w.mediaRot,
                    offset_x: w.mediaOffsetX,
                    offset_y: w.mediaOffsetY,
                    miroir_h: w.mediaMiroirH,
                    miroir_v: w.mediaMiroirV,
                    zoom_ecran: 1.0,
                },
                // `mediaPixel` n'est rendu qu'en chemin morph (T4) — parité
                // DOM : aucun `filter` pixel n'existe côté widget.
                effets: EffetsCfg {
                    lum: w.mediaLum,
                    contraste: w.mediaContraste,
                    teinte: w.mediaTeinte,
                    flou: w.mediaFlou,
                    pixel: 0.0,
                },
                cadre: cadre_de_widget(scene, w),
                morphs: w.morphs.clone().unwrap_or_default(),
                // `mediaPixel` n'agit qu'en chemin morph (parité DOM :
                // aucun `filter` pixel côté widget).
                pixel_morph: w.mediaPixel,
                lecture: lecture(w.mediaPaused, w.mediaLoopA, w.mediaLoopB),
                // `mediaSlides` gagne sur `media` ; le fondu widget est
                // `ease-in-out` (transition CSS `.slide`).
                carrousel: carrousel(
                    w.mediaSlides.as_deref(),
                    w.mediaSlidesSec,
                    base,
                    true,
                ),
            });
        }
    }
    medias
}

/// Plans à perspective de la scène — widgets `rotateX`/`rotateY` ≠ 0 et
/// fond `bgRotX`/`bgRotY` ≠ 0 (`applyWidgetStyle` / `renderBg`).
///
/// `rect`+`z` servent de clé : tous les calques d'un widget (voile, média,
/// capture, bordure, cadre) partagent son rect et son z — miroir du
/// `.widget` DOM qui porte la perspective. Le bord du canvas (`z = MAX`)
/// ne matche jamais le fond (`z = MIN`) : `#cadre-app-overlay` est hors
/// `#bg`, il reste plat. Deux widgets au même (rect, z) fusionneraient en
/// un seul plan — cas dégénéré accepté.
///
/// `bgObsSource` exclut le plan du fond : page transparente, pas de média à
/// projeter. `bgZoom` est porté par le plan (post-projection), PAS par le
/// transform 2D du média — il est retiré de la matrice 2D pour les entrées
/// groupées.
pub fn persps_depuis_scene(scene: &Scene) -> Vec<PerspCfg> {
    let mut groupes = Vec::new();
    if scene.bgObsSource.is_none() && (scene.bgRotX != 0.0 || scene.bgRotY != 0.0) {
        groupes.push(PerspCfg {
            rect: (0.0, 0.0, scene.canvasW as f64, scene.canvasH as f64),
            z: i32::MIN,
            rot_x: scene.bgRotX,
            rot_y: scene.bgRotY,
            profondeur: 2.0 * scene.canvasW as f64,
            zoom_ecran: scene.bgZoom,
        });
    }
    for w in &scene.widgets {
        if w.rotateX != 0.0 || w.rotateY != 0.0 {
            groupes.push(PerspCfg {
                rect: (w.x, w.y, w.largeur, w.hauteur),
                z: w.z,
                rot_x: w.rotateX,
                rot_y: w.rotateY,
                profondeur: 800.0,
                zoom_ecran: 1.0,
            });
        }
    }
    groupes.sort_by_key(|g| g.z);
    groupes
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

/// Cadre de scène **actif** (même prédicat que `applyCadreWidget`).
fn cadre_actif(scene: &Scene) -> Option<&CadreConfig> {
    scene
        .cadreWidget
        .as_ref()
        .filter(|c| c.actif && c.strokeWidth > 0.0)
}

/// Couleur de cadre validée comme `genererSVG` : `#rrggbb` exact, sinon repli.
fn couleur_cadre(hex: &str, defaut: [u8; 4]) -> [u8; 4] {
    if hex.len() == 7 && hex.starts_with('#') {
        couleur_hex(hex).unwrap_or(defaut)
    } else {
        defaut
    }
}

/// Traduit une `CadreConfig` en cadre natif ; `None` = style non porté.
fn cadre_cfg(cadre: &CadreConfig, rect: (f64, f64, f64, f64), z: i32) -> Option<CadreCfg> {
    let style = StyleCadre::depuis_nom(&cadre.style)?;
    let couleur = couleur_cadre(&cadre.couleur, [255, 255, 255, 255]);
    let couleur_fin = couleur_cadre(&cadre.couleurFin, couleur);
    Some(CadreCfg {
        rect,
        style,
        stroke_width: cadre.strokeWidth,
        couleur,
        couleur_fin,
        gradient_angle: cadre.gradientAngle,
        z,
    })
}

/// Cadre qui s'applique à CE widget, ou `None`.
///
/// Miroir de `applyCadreWidget` + les exceptions du dépôt :
/// - `sansCadre` (opt-out par instance — 8 widgets sur la scène de référence) ;
/// - caméra (forme dédiée `cameraForme`, contrat 9 — et hors périmètre ici) ;
/// - chat `bulles` : `diffusion.html` masque le cadre du widget et décore
///   chaque bulle (hors tranche) → aucun cadre de widget.
pub fn cadre_de_widget(scene: &Scene, w: &Widget) -> Option<CadreCfg> {
    let cadre = cadre_actif(scene)?;
    if w.sansCadre || w.widget_type == "camera" {
        return None;
    }
    if w.widget_type == "chat" && w.chatStyle.as_deref() == Some("bulles") {
        return None;
    }
    if w.largeur <= 0.0 || w.hauteur <= 0.0 {
        return None;
    }
    cadre_cfg(cadre, (w.x, w.y, w.largeur, w.hauteur), w.z)
}

/// Boîte d'un widget à voile (`.widget`) : rect de contenu, ordre z, cadre.
#[derive(Debug, Clone, PartialEq)]
pub struct BoiteCfg {
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
    /// Style de fond/bordure — chaque type de widget surcharge `.widget`.
    pub style: StyleBoite,
    /// Cadre du widget : `Some` = le voile est **clippé au polygone** et il
    /// n'y a **aucune bordure** (`applyCadreWidget` met `border: none` sous un
    /// cadre actif). `None` = voile rectangulaire élargi + bordure.
    pub cadre: Option<CadreCfg>,
}

/// Style de boîte d'un widget — miroir des surcharges CSS de `.widget`.
fn style_boite(w: &Widget) -> StyleBoite {
    match w.widget_type.as_str() {
        "chat" => StyleBoite::Chat,
        "input-viewer" => StyleBoite::InputViewer,
        "goal" => StyleBoite::Goal,
        "musique" => StyleBoite::Musique,
        _ => StyleBoite::Generique,
    }
}

/// Boîtes à voile de la scène — miroir de `.widget` et de ses exceptions.
///
/// `renderScene` crée un `.widget` pour tout widget sauf trois cas :
/// - `sansCadre` → `.widget.sans-cadre` : ni voile ni bordure ;
/// - `trou` et caméra → `.widget.trou` : transparent côté DOM. **Divergence
///   voulue** : en natif l'exclusion porte sur la cible (`captureCible`),
///   pas le flag `trou` (D8) — un widget `trou: true` SANS cible reçoit sa
///   boîte et son voile comme tout widget ordinaire ;
/// - `cameraCachee` → le widget n'existe pas du tout.
///
/// Le cadre éventuel est celui de [`cadre_de_widget`] : le chat `bulles` n'en
/// a pas (il garde donc sa bordure), les autres widgets à cadre en ont un.
pub fn boites_depuis_scene(scene: &Scene) -> Vec<BoiteCfg> {
    scene
        .widgets
        .iter()
        .filter(|w| !w.sansCadre && w.captureCible.is_none() && w.widget_type != "camera")
        .filter(|w| w.largeur > 0.0 && w.hauteur > 0.0)
        .map(|w| BoiteCfg {
            rect: (w.x, w.y, w.largeur, w.hauteur),
            z: w.z,
            style: style_boite(w),
            cadre: cadre_de_widget(scene, w),
        })
        .collect()
}

/// Widgets input-viewer de la scène (mode, disposition, skin, couleur, cadre).
///
/// Miroir de la branche `input-viewer` de `renderScene` : le widget garde son
/// média de fond (composé comme média) et reçoit en plus son rendu de touches.
/// Couleur de presse : `--inputviewer-couleur-presse`, défaut `#8b5cf6`.
pub fn input_depuis_scene(scene: &Scene) -> Vec<InputCfg> {
    scene
        .widgets
        .iter()
        .filter(|w| w.widget_type == "input-viewer" && w.largeur > 0.0 && w.hauteur > 0.0)
        .map(|w| InputCfg {
            rect: (w.x, w.y, w.largeur, w.hauteur),
            z: w.z,
            mode: ModeInput::depuis_nom(w.inputMode.as_deref().unwrap_or("clavier")),
            disposition: Disposition::depuis_nom(w.inputLayout.as_deref().unwrap_or("azerty")),
            skin: w.inputSkin.clone().unwrap_or_else(|| "xbox".to_string()),
            couleur: w
                .inputCouleur
                .as_deref()
                .and_then(couleur_hex)
                .unwrap_or([0x8b, 0x5c, 0xf6, 255]),
            mapping: w.inputMapping.as_ref().map(|m| MappingManette {
                boutons: m.buttons.clone(),
                dpad_invert_x: m.dpadInvertX,
                dpad_invert_y: m.dpadInvertY,
            }),
            cadre: cadre_de_widget(scene, w),
        })
        .collect()
}

/// Cadres de la scène : bord du canvas (si `bordActif`) puis un par widget.
///
/// Le bord porte `z = i32::MAX` : il est dessiné **après** tous les calques,
/// comme `#cadre-app-overlay` (`z-index: 100`). Un style non porté ne produit
/// aucun cadre (jamais un tracé faux) — le moteur le signale au démarrage.
pub fn cadres_depuis_scene(scene: &Scene) -> Vec<CadreCfg> {
    let Some(cadre) = cadre_actif(scene) else {
        return Vec::new();
    };
    let mut cadres = Vec::new();
    if cadre.bordActif {
        let rect = (0.0, 0.0, scene.canvasW as f64, scene.canvasH as f64);
        if let Some(bord) = cadre_cfg(cadre, rect, i32::MAX) {
            cadres.push(bord);
        }
    }
    cadres.extend(scene.widgets.iter().filter_map(|w| cadre_de_widget(scene, w)));
    cadres
}

/// Widgets chat de la scène (rect + filtre + taille + style + z).
///
/// Aucun message ici : le buffer vit dans `FileChat`, alimenté par l'app
/// pendant l'enregistrement (le rendu se met à jour sans reconfigurer).
pub fn chat_depuis_scene(scene: &Scene) -> Vec<ChatCfg> {
    scene
        .widgets
        .iter()
        .filter(|w| w.widget_type == "chat" && w.largeur > 0.0 && w.hauteur > 0.0)
        .map(|w| ChatCfg {
            rect: (w.x, w.y, w.largeur, w.hauteur),
            filtre: w
                .chatFiltre
                .clone()
                .unwrap_or_else(|| FILTRE_UNIFIE.to_string()),
            taille_police: w.taillePolice.unwrap_or(16) as f32,
            bulles: w.chatStyle.as_deref() == Some("bulles"),
            z: w.z,
        })
        .collect()
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
    fn seuls_les_widgets_avec_cible_produisent_une_source() {
        let scene = scene(
            r#"{
                "widgets": [
                    {"id":"a","type":"media","x":10,"y":20,"largeur":300,"hauteur":200,"z":0,"trou":true},
                    {"id":"b","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":0},
                    {"id":"c","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":0,"trou":true}
                ]
            }"#,
        );
        let mut cibles = HashMap::new();
        cibles.insert("a".to_string(), CibleCapture::Moniteur { index: 0 });
        cibles.insert("b".to_string(), CibleCapture::Fenetre { hwnd: 42 });

        let sources = sources_depuis_scene(&scene, &cibles);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].nom, "a");
        assert_eq!(sources[0].rect, (10.0, 20.0, 300.0, 200.0));
        assert_eq!(sources[0].cible, CibleCapture::Moniteur { index: 0 });
        assert_eq!(sources[1].cible, CibleCapture::Fenetre { hwnd: 42 });
    }

    #[test]
    fn sans_cible_ou_taille_nulle_ne_produit_aucune_source() {
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
                    {"id":"t","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":3,"captureCible":{"kind":"moniteur","name":"\\\\.\\DISPLAY1","index":0},"media":"medias/t.png"},
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
        // Cible + média → capture seule : pas de média dessiné, mais la
        // source est bien produite.
        assert_eq!(sources_depuis_scene(&scene, &HashMap::new()).len(), 1);
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
    fn cadres_bord_plus_widgets_avec_exclusions() {
        let scene = scene(
            r##"{
                "canvasW": 1360, "canvasH": 768,
                "cadreWidget": {"style":"mgstyle","strokeWidth":20,"couleur":"#000000",
                                "couleurFin":"#8c00ff","gradientAngle":135,
                                "actif":true,"bordActif":true},
                "widgets": [
                    {"id":"a","type":"media","x":10,"y":20,"largeur":300,"hauteur":200,"z":2},
                    {"id":"b","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":3,"sansCadre":true},
                    {"id":"c","type":"camera","x":0,"y":0,"largeur":100,"hauteur":100,"z":4},
                    {"id":"d","type":"chat","x":0,"y":0,"largeur":100,"hauteur":100,"z":5,"chatStyle":"bulles"},
                    {"id":"e","type":"chat","x":0,"y":0,"largeur":100,"hauteur":100,"z":6,"chatStyle":"compact"}
                ]
            }"##,
        );
        let cadres = cadres_depuis_scene(&scene);
        assert_eq!(cadres.len(), 3, "bord + a + e");
        assert_eq!(cadres[0].rect, (0.0, 0.0, 1360.0, 768.0));
        assert_eq!(cadres[0].z, i32::MAX, "le bord passe au-dessus de tout");
        assert_eq!(cadres[1].rect, (10.0, 20.0, 300.0, 200.0));
        assert_eq!(cadres[1].z, 2);
        assert_eq!(cadres[2].z, 6);
        assert_eq!(cadres[1].style, StyleCadre::MgStyle);
        assert_eq!(cadres[1].couleur, [0, 0, 0, 255]);
        assert_eq!(cadres[1].couleur_fin, [0x8c, 0x00, 0xff, 255]);
    }

    #[test]
    fn cadre_inactif_ou_style_non_porte_ne_produit_rien() {
        let inactif = scene(
            r##"{"cadreWidget":{"style":"mgstyle","strokeWidth":20,"actif":false},"widgets":[]}"##,
        );
        assert!(cadres_depuis_scene(&inactif).is_empty());
        let epaisseur_nulle = scene(
            r##"{"cadreWidget":{"style":"mgstyle","strokeWidth":0,"actif":true},"widgets":[]}"##,
        );
        assert!(cadres_depuis_scene(&epaisseur_nulle).is_empty());
        let non_porte = scene(
            r##"{"cadreWidget":{"style":"cyberpunk","strokeWidth":20,"actif":true},
                 "widgets":[{"id":"a","type":"media","x":0,"y":0,"largeur":10,"hauteur":10,"z":0}]}"##,
        );
        assert!(cadres_depuis_scene(&non_porte).is_empty());
    }

    #[test]
    fn bord_desactive_garde_les_cadres_de_widgets() {
        let scene = scene(
            r##"{"canvasW":800,"canvasH":600,
                 "cadreWidget":{"style":"thin-line","strokeWidth":4,"actif":true,"bordActif":false},
                 "widgets":[{"id":"a","type":"media","x":5,"y":6,"largeur":70,"hauteur":80,"z":1}]}"##,
        );
        let cadres = cadres_depuis_scene(&scene);
        assert_eq!(cadres.len(), 1);
        assert_eq!(cadres[0].z, 1);
    }

    #[test]
    fn media_porte_le_cadre_du_widget() {
        let scene = scene(
            r##"{"cadreWidget":{"style":"mgstyle","strokeWidth":20,"couleur":"#000000",
                                "couleurFin":"#8c00ff","actif":true,"bordActif":true},
                 "widgets":[
                    {"id":"a","type":"media","x":10,"y":20,"largeur":300,"hauteur":200,"z":2,
                     "media":"medias/a.png"},
                    {"id":"b","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":3,
                     "media":"medias/b.png","sansCadre":true}
                 ]}"##,
        );
        let medias = medias_depuis_scene(&scene, Path::new("C:/data"));
        assert!(medias[0].cadre.is_some(), "média à cadre : découpé au polygone");
        assert_eq!(
            medias[0].cadre.as_ref().map(|c| c.rect),
            Some((10.0, 20.0, 300.0, 200.0))
        );
        assert!(medias[1].cadre.is_none(), "sansCadre : aucun cadre");
    }

    #[test]
    fn boites_excluent_sans_cadre_capture_et_camera() {
        let scene = scene(
            r##"{"widgets":[
                {"id":"media","type":"media","x":10,"y":20,"largeur":300,"hauteur":200,"z":2},
                {"id":"libre","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":3,"sansCadre":true},
                {"id":"cap","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":4,"captureCible":{"kind":"moniteur","name":"\\\\.\\DISPLAY1","index":0}},
                {"id":"cam","type":"camera","x":0,"y":0,"largeur":100,"hauteur":100,"z":5},
                {"id":"nul","type":"media","x":0,"y":0,"largeur":0,"hauteur":100,"z":6}
            ]}"##,
        );
        let boites = boites_depuis_scene(&scene);
        assert_eq!(boites.len(), 1, "seul le widget média à voile reste");
        assert_eq!(boites[0].rect, (10.0, 20.0, 300.0, 200.0));
        assert_eq!(boites[0].z, 2);
        assert_eq!(boites[0].style, StyleBoite::Generique);
        assert!(boites[0].cadre.is_none(), "aucun cadre actif dans cette scène");
    }

    #[test]
    fn trou_sans_cible_est_un_widget_ordinaire() {
        // D8 : `trou` sans cible n'est plus spécial en natif — média et
        // voile sont dessinés comme pour tout widget, sans source créée.
        let scene = scene(
            r#"{"widgets":[
                {"id":"t","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":1,"trou":true,"media":"medias/t.png"}
            ]}"#,
        );
        assert!(sources_depuis_scene(&scene, &HashMap::new()).is_empty());
        let medias = medias_depuis_scene(&scene, Path::new("C:/data"));
        assert_eq!(medias.len(), 1, "trou sans cible : média dessiné");
        assert_eq!(boites_depuis_scene(&scene).len(), 1, "voile dessinée");
    }

    #[test]
    fn voile_de_chat_bulles_sans_cadre_et_voile_clippe_ailleurs() {
        let scene = scene(
            r##"{"canvasW":1360,"canvasH":768,
                "cadreWidget":{"style":"mgstyle","strokeWidth":20,"couleur":"#000000",
                               "couleurFin":"#8c00ff","actif":true,"bordActif":true},
                "widgets":[
                    {"id":"sr","type":"speedrun","x":22,"y":27,"largeur":215,"hauteur":704,"z":2},
                    {"id":"chat","type":"chat","x":334,"y":669,"largeur":692,"hauteur":88,"z":14,
                     "chatStyle":"bulles"},
                    {"id":"iv","type":"input-viewer","x":1124,"y":328,"largeur":213,"hauteur":197,"z":9}
                ]}"##,
        );
        let boites = boites_depuis_scene(&scene);
        assert_eq!(boites.len(), 3);
        // Le chat `bulles` n'a pas de cadre → voile rectangulaire + bordure.
        assert!(boites[1].cadre.is_none(), "chat bulles : bordure, pas de clip");
        assert_eq!(boites[1].style, StyleBoite::Chat);
        // Speedrun et input-viewer portent le cadre → voile clippé, pas de bordure.
        assert_eq!(
            boites[0].cadre.as_ref().map(|c| c.style),
            Some(StyleCadre::MgStyle)
        );
        assert_eq!(boites[0].style, StyleBoite::Generique);
        assert!(boites[2].cadre.is_some());
        assert_eq!(boites[2].style, StyleBoite::InputViewer);
    }

    #[test]
    fn input_viewer_lit_mode_disposition_skin_et_couleur() {
        let scene = scene(
            r##"{"canvasW":1360,"canvasH":768,
                "cadreWidget":{"style":"mgstyle","strokeWidth":20,"actif":true,"bordActif":false},
                "widgets":[
                    {"id":"iv","type":"input-viewer","x":1124,"y":328,"largeur":213,"hauteur":197,
                     "z":9,"inputMode":"numpad","inputLayout":"qwerty","inputSkin":"ps",
                     "inputCouleur":"#ff0000"},
                    {"id":"iv2","type":"input-viewer","x":0,"y":0,"largeur":100,"hauteur":100,"z":3},
                    {"id":"m","type":"media","x":0,"y":0,"largeur":10,"hauteur":10,"z":4}
                ]}"##,
        );
        let inputs = input_depuis_scene(&scene);
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].mode, ModeInput::Numpad);
        assert_eq!(inputs[0].disposition, Disposition::Qwerty);
        assert_eq!(inputs[0].skin, "ps");
        assert_eq!(inputs[0].couleur, [0xff, 0x00, 0x00, 255]);
        assert!(inputs[0].cadre.is_some(), "cadre de scène actif");
        // Défauts du dashboard : clavier AZERTY, skin xbox, violet.
        assert_eq!(inputs[1].mode, ModeInput::Clavier);
        assert_eq!(inputs[1].disposition, Disposition::Azerty);
        assert_eq!(inputs[1].skin, "xbox");
        assert_eq!(inputs[1].couleur, [0x8b, 0x5c, 0xf6, 255]);
    }

    #[test]
    fn chat_depuis_scene_filtre_taille_et_style() {
        let scene = scene(
            r##"{"widgets":[
                {"id":"c","type":"chat","x":334,"y":669,"largeur":692,"hauteur":88,"z":14,
                 "chatFiltre":"twitch","taillePolice":11,"chatStyle":"bulles"},
                {"id":"d","type":"chat","x":0,"y":0,"largeur":0,"hauteur":100,"z":15},
                {"id":"m","type":"media","x":0,"y":0,"largeur":10,"hauteur":10,"z":16}
            ]}"##,
        );
        let chats = chat_depuis_scene(&scene);
        assert_eq!(chats.len(), 1, "rect nul ignoré, seul le type chat compte");
        assert_eq!(chats[0].rect, (334.0, 669.0, 692.0, 88.0));
        assert_eq!(chats[0].filtre, "twitch");
        assert_eq!(chats[0].taille_police, 11.0);
        assert!(chats[0].bulles);
        assert_eq!(chats[0].z, 14);
    }

    #[test]
    fn chat_sans_filtre_ni_taille_prend_les_defauts() {
        let scene = scene(
            r##"{"widgets":[{"id":"c","type":"chat","x":0,"y":0,"largeur":10,"hauteur":10,"z":0}]}"##,
        );
        let chats = chat_depuis_scene(&scene);
        assert_eq!(chats[0].filtre, "unifie");
        assert_eq!(chats[0].taille_police, 16.0);
        assert!(!chats[0].bulles);
    }

    #[test]
    fn couleurs_hex_trois_six_et_huit_chiffres() {
        assert_eq!(couleur_hex("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(couleur_hex("#8b5cf6"), Some([0x8b, 0x5c, 0xf6, 255]));
        assert_eq!(couleur_hex("#8b5cf680"), Some([0x8b, 0x5c, 0xf6, 0x80]));
        assert_eq!(couleur_hex("8b5cf6"), None);
        assert_eq!(couleur_hex("#zzzzzz"), None);
    }

    #[test]
    fn transforms_media_et_fond_depuis_scene() {
        // Miroir `applyMediaStyle` (widget) / `dessinerContenuFond` +
        // `renderBg` (fond) : zoom interne vs `zoom_ecran`, fits distincts.
        let scene = scene(
            r##"{
                "canvasW": 1920, "canvasH": 1080,
                "bgMedia": "medias/fond.png",
                "bgFit": "centrer", "bgZoom": 1.5, "bgRot": 12.0,
                "bgOffsetX": 4.0, "bgOffsetY": -2.0, "bgMiroirH": true,
                "widgets": [
                    {"id":"m","type":"media","x":10,"y":20,"largeur":300,"hauteur":200,"z":2,
                     "media":"medias/a.png","mediaFit":"vignette","mediaZoom":1.1,
                     "mediaRot":30.0,"mediaOffsetX":7.0,"mediaOffsetY":-3.0,
                     "mediaMiroirV":true},
                    {"id":"n","type":"media","x":0,"y":0,"largeur":10,"hauteur":10,"z":3,
                     "media":"medias/b.png"}
                ]
            }"##,
        );
        let medias = medias_depuis_scene(&scene, Path::new("C:/data"));
        assert_eq!(medias.len(), 3);
        let fond = &medias[0];
        assert_eq!(fond.fit, Fit::Vignette, "centrer du fond = scale-down");
        assert_eq!(fond.transform.zoom, 1.0);
        assert_eq!(fond.transform.zoom_ecran, 1.5);
        assert_eq!(fond.transform.rot, 12.0);
        assert_eq!(fond.transform.offset_x, 4.0);
        assert_eq!(fond.transform.offset_y, -2.0);
        assert!(fond.transform.miroir_h);
        assert!(!fond.transform.miroir_v);
        let widget = &medias[1];
        assert_eq!(widget.fit, Fit::Vignette);
        assert_eq!(widget.transform.zoom, 1.1);
        assert_eq!(widget.transform.rot, 30.0);
        assert_eq!(widget.transform.offset_x, 7.0);
        assert_eq!(widget.transform.offset_y, -3.0);
        assert!(widget.transform.miroir_v);
        assert_eq!(widget.transform.zoom_ecran, 1.0);
        assert!(!widget.transform.est_neutre());
        assert_eq!(medias[2].transform, Transform2D::NEUTRE);
        assert_eq!(medias[2].fit, Fit::Ajuster);
    }

    #[test]
    fn persps_depuis_scene_widget_fond_et_tri() {
        // `applyWidgetStyle` → profondeur 800 ; `renderBg` → 2·canvasW et le
        // `scale(bgZoom)` reste hors de la projection (`zoom_ecran`).
        let scene = scene(
            r#"{
                "canvasW": 1920, "canvasH": 1080,
                "bgMedia": "medias/fond.png", "bgRotY": -10.0, "bgZoom": 1.5,
                "widgets": [
                    {"id":"p","type":"media","x":10,"y":20,"largeur":300,"hauteur":200,"z":5,
                     "rotateY":38.25},
                    {"id":"q","type":"media","x":0,"y":0,"largeur":10,"hauteur":10,"z":1,
                     "rotateX":15.0,"rotateY":0.0},
                    {"id":"n","type":"media","x":0,"y":0,"largeur":10,"hauteur":10,"z":2}
                ]
            }"#,
        );
        let persps = persps_depuis_scene(&scene);
        assert_eq!(persps.len(), 3);
        // Tri par z : fond (MIN) → widget z 1 → widget z 5.
        assert_eq!(persps[0].z, i32::MIN);
        assert_eq!(persps[0].rect, (0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(persps[0].rot_y, -10.0);
        assert_eq!(persps[0].profondeur, 3840.0);
        assert_eq!(persps[0].zoom_ecran, 1.5);
        assert_eq!(persps[1].z, 1);
        assert_eq!(persps[1].rot_x, 15.0);
        assert_eq!(persps[1].profondeur, 800.0);
        assert_eq!(persps[1].zoom_ecran, 1.0);
        assert_eq!(persps[2].z, 5);
        assert_eq!(persps[2].rot_y, 38.25);
        assert_eq!(persps[2].rect, (10.0, 20.0, 300.0, 200.0));
    }

    #[test]
    fn persps_exclues_fond_obs_et_widget_plat() {
        let scene = scene(
            r#"{
                "canvasW": 1920, "canvasH": 1080,
                "bgMedia": "medias/fond.png", "bgRotX": 20.0,
                "bgObsSource": "SOS-Fond-abc",
                "widgets": []
            }"#,
        );
        assert!(persps_depuis_scene(&scene).is_empty());
    }
}
