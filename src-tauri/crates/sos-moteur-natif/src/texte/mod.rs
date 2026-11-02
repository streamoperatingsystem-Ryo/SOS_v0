//! Rendu de texte natif — DirectWrite (mise en page) + Direct2D (dessin).
//!
//! Voie A validée le 2026-10-28 : les calques sont composés dans un canvas
//! **BGRA**, le texte est dessiné dessus par Direct2D, puis
//! `color_gpu::ConvertisseurNv12` convertit le canvas en NV12 pour l'encodeur
//! (passe mesurée à 0,145 ms/frame en 1360×768, 0,468 ms en 4K —
//! `examples/passe_nv12.rs`). Le texte a donc un **alpha réel**, ce que
//! `VideoProcessorBlt` ne sait pas faire.
//!
//! Contrat :
//! - le texte est **statique** : chaque titre est rendu **une fois** dans une
//!   texture BGRA transparente, mise en cache par (contenu, style). Aucun
//!   re-rendu par frame, aucune allocation par message ;
//! - les positions et les styles reproduisent `diffusion.html`
//!   (`applyTitreFond` / `applyTitreWidget`) : 8 px des bords, centrage,
//!   `vertical-rl` pour les latéraux, dégradé 135°, contour ;
//! - polices : les 20 `.woff2` embarqués (`src-tauri/resources/fonts/`) sont
//!   décodés en mémoire par `wuff` puis chargés par DirectWrite via
//!   `IDWriteInMemoryFontFileLoader` — aucune police téléchargée, aucun
//!   fichier écrit sur disque (CONTRAT FIGÉ vie privée respecté).
//!
//! Le **contour** est le tracé réel des glyphes (`GetGlyphRunOutline` +
//! `Widen`, lot H4) ; une police **simulée** (gras/oblique synthétique — les
//! familles embarquées n'ont qu'une graisse) n'a pas ce tracé et retombe sur
//! l'approximation par 16 passes décalées, plus proche du glyphe embelli.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use windows::core::{implement, Interface, IUnknown, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::{BOOL, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Brush, ID2D1Device, ID2D1DeviceContext, ID2D1Effect,
    ID2D1Factory1, ID2D1GradientStopCollection, ID2D1Geometry,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, ID2D1RenderTarget,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_ROUNDED_RECT, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1_UNIT_MODE_PIXELS,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteFontCollection1,
    IDWriteInlineObject, IDWritePixelSnapping_Impl, IDWriteTextFormat,
    IDWriteTextLayout1, IDWriteTextRenderer, IDWriteTextRenderer_Impl, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_GLYPH_RUN,
    DWRITE_GLYPH_RUN_DESCRIPTION, DWRITE_LINE_METRICS, DWRITE_LINE_SPACING_METHOD_UNIFORM,
    DWRITE_MATRIX, DWRITE_MEASURING_MODE, DWRITE_STRIKETHROUGH, DWRITE_TEXT_METRICS,
    DWRITE_TEXT_RANGE, DWRITE_UNDERLINE, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::cadres::{CadreCfg, ErreurCadre};
use crate::gpu::{ContexteGpu, DescTexture, Format};

mod alertes;
mod attente;
mod bandeau;
mod chat;
mod eq;
mod fight;
mod goal;
mod horloge;
mod input;
pub mod media;
mod musique;
mod musique_module;
mod objectifs;
mod pad;
mod persp;
mod presentation;
mod speedrun;
pub mod splitpop;
mod spots;
mod viewers;
mod welcome;

use alertes::BlocAlerte;
pub use alertes::{EtatAlertes, MajAlerte, Z_ALERTES};
use attente::{Asset, AssetMarque, BlocAttente};
pub use attente::{AttenteCfg, EtatAttente, Z_ATTENTE};
use bandeau::BlocBandeau;
pub use bandeau::{
    assombrir, resoudre_couleurs, EtatBandeau, MajBandeau, PositionBandeau, Z_BANDEAU,
};
/// Couleurs du bandeau quand aucun cadre n'est actif (`BPM_COULEURS_DEFAUT`).
pub const COULEURS_BANDEAU_DEFAUT: ([u8; 4], [u8; 4]) = bandeau::COULEURS_DEFAUT;
use chat::BlocChat;
pub use chat::{geometrie_chat, ChatGeometrie};
use eq::BullesEq;
pub use eq::{EqualiseurCfg, EtatEq, NB_BANDES_EQ, Z_EQ, Z_SPOTS};
use fight::BlocFight;
pub use fight::{
    CmdsFight, CoupFight, EtatFight, FightState, InfoFight, JoueurFight, PhaseFight, Z_FIGHT,
};
use goal::{BlocsGoal, LogosGoal};
pub use goal::{Communaute, EtatGoals, GoalCfg, GoalTwitch, MajGoal};
use horloge::BlocsHorloge;
pub use horloge::{accent_module, opacite_module, taille_module, HorlogeCfg, Z_MODULES};
pub use media::{TraceMedia, TraceMorphe, TracePixel};
use musique::BlocsMusique;
pub use musique::{EtatMusique, MajMusique, ModeMusique, MusiqueCfg, Piste};
use musique_module::BlocsMusiqueModule;
pub use musique_module::MusiqueModuleCfg;
use objectifs::BlocsObjectifs;
pub use objectifs::{ObjectifCfg, ObjectifLigneCfg};
use pad::BlocPad;
pub use pad::{
    media_affiche as media_affiche_pad, source_audio as source_audio_pad, EtatPad, MajPad,
    PadPlay, POSITION_DEFAUT as POSITION_PAD_DEFAUT, Z_PAD,
};
use persp::PlanPersp;
use splitpop::BlocSplitPop;
use presentation::{BlocCarte, BlocPresentation};
pub use presentation::{
    chemin_clip, formater_viewers, url_box_art, ClipEnCours, ClipPresentation, EtatPresentation,
    JeuRecent, MajPresentation, ModePresentation, PistePresentation, PresentationDonnees,
    COUCHE_CARTE, COUCHE_CLIP, COUCHE_PUBLIC, COUCHE_RIDEAUX, COUCHE_VOILE, Z_PRES_CARTE,
    Z_PRES_CLIP, Z_PRES_PUBLIC, Z_PRES_RIDEAUX, Z_PRES_VOILE,
};
use speedrun::BlocsSpeedrun;
pub use splitpop::{EtatSplitPop, MajSplitPop, PopSplit, Z_SPLITPOP};
pub use spots::SpotsCfg;
use welcome::BlocWelcome;
pub use welcome::{EtatWelcome, MajWelcome, Z_WELCOME};
pub use speedrun::{
    EtatSpeedrun, MajSpeedrun, MethodeTemps, ModeDelta, ModeTemps, PhaseSpeedrun,
    PrecisionTemps, SegmentLss, SpeedrunCfg,
};
use viewers::BlocsViewers;

/// Écart aux bords, en px canvas — valeur de `diffusion.html` (`.bg-titre`).
pub const MARGE_TITRE: f32 = 8.0;

/// Écart entre un titre de widget et son widget — `margin-top/bottom/left/
/// right: 2px` de `applyTitreWidget` (diffusion.html 9068-9101).
pub const MARGE_TITRE_WIDGET: f32 = 2.0;

/// `line-height` des titres — `.widget-titre` et `.bg-titre` (CSS).
pub const INTERLIGNE_TITRE: f32 = 1.1;

/// Une police embarquée : (id CSS/`fonts.ts`, famille OpenType, woff2).
///
/// Miroir de `src/lib/fonts.ts` (`POLICES_TITRE`) — le JSON des scènes stocke
/// l'`id`, DirectWrite exige le nom de **famille** contenu dans le fichier.
/// Ne jamais renommer un `id` sans migrer les scènes.
pub const POLICES: &[(&str, &str, &[u8])] = &[
    ("BebasNeue", "Bebas Neue", include_bytes!("../../../../resources/fonts/BebasNeue.woff2")),
    ("Anton", "Anton", include_bytes!("../../../../resources/fonts/Anton.woff2")),
    ("Oswald", "Oswald", include_bytes!("../../../../resources/fonts/Oswald.woff2")),
    ("Pacifico", "Pacifico", include_bytes!("../../../../resources/fonts/Pacifico.woff2")),
    ("PermanentMarker", "Permanent Marker", include_bytes!("../../../../resources/fonts/PermanentMarker.woff2")),
    ("Bungee", "Bungee", include_bytes!("../../../../resources/fonts/Bungee.woff2")),
    ("RussoOne", "Russo One", include_bytes!("../../../../resources/fonts/RussoOne.woff2")),
    ("PressStart2P", "Press Start 2P", include_bytes!("../../../../resources/fonts/PressStart2P.woff2")),
    ("Orbitron", "Orbitron", include_bytes!("../../../../resources/fonts/Orbitron.woff2")),
    ("Montserrat", "Montserrat", include_bytes!("../../../../resources/fonts/Montserrat.woff2")),
    ("Righteous", "Righteous", include_bytes!("../../../../resources/fonts/Righteous.woff2")),
    ("Audiowide", "Audiowide", include_bytes!("../../../../resources/fonts/Audiowide.woff2")),
    ("Lobster", "Lobster", include_bytes!("../../../../resources/fonts/Lobster.woff2")),
    ("Caveat", "Caveat", include_bytes!("../../../../resources/fonts/Caveat.woff2")),
    ("Satisfy", "Satisfy", include_bytes!("../../../../resources/fonts/Satisfy.woff2")),
    ("Teko", "Teko", include_bytes!("../../../../resources/fonts/Teko.woff2")),
    ("Rajdhani", "Rajdhani", include_bytes!("../../../../resources/fonts/Rajdhani.woff2")),
    ("BlackOpsOne", "Black Ops One", include_bytes!("../../../../resources/fonts/BlackOpsOne.woff2")),
    ("Creepster", "Creepster", include_bytes!("../../../../resources/fonts/Creepster.woff2")),
    ("Fredoka", "Fredoka", include_bytes!("../../../../resources/fonts/Fredoka.woff2")),
];

/// Police par défaut des titres (`fonts.ts` → `POLICE_TITRE_DEFAUT`).
pub const POLICE_DEFAUT: &str = "BebasNeue";

/// Famille OpenType d'un `id` de police, `None` si l'id est inconnu.
pub fn famille_police(id: &str) -> Option<&'static str> {
    POLICES
        .iter()
        .find(|(identifiant, _, _)| *identifiant == id)
        .map(|(_, famille, _)| *famille)
}

/// Position d'un titre — vocabulaire unifié fond/widget (`titres.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositionTitre {
    Haut,
    Bas,
    Gauche,
    Droite,
}

impl PositionTitre {
    /// `true` pour les positions rendues verticalement (`vertical-rl`).
    pub fn verticale(self) -> bool {
        matches!(self, PositionTitre::Gauche | PositionTitre::Droite)
    }
}

/// Cible d'un titre : **dans** le rect de l'hôte (fond) ou **hors** (widget).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CibleTitre {
    Fond,
    Widget,
}

/// Style partagé par les 4 titres d'une cible (`bgTitre*` / `titre*`).
#[derive(Debug, Clone, PartialEq)]
pub struct StyleTexte {
    /// Id de police (`fonts.ts`) ; inconnu → famille système de repli.
    pub police: String,
    /// Taille en px canvas (12–72 côté UI).
    pub taille: f32,
    pub gras: bool,
    pub italique: bool,
    pub souligne: bool,
    /// `letter-spacing` en px (−2 à 20 côté UI).
    pub espacement: f32,
    /// Début du dégradé du texte (RGBA).
    pub couleur: [u8; 4],
    /// Fin du dégradé ; égale à `couleur` pour une couleur unie.
    pub couleur_fin: [u8; 4],
    /// Couleur du contour (RGBA).
    pub contour_couleur: [u8; 4],
    /// Épaisseur du contour en px (0 = aucun contour).
    pub contour_epaisseur: f32,
}

impl Default for StyleTexte {
    fn default() -> Self {
        Self {
            police: POLICE_DEFAUT.to_string(),
            taille: 24.0,
            gras: false,
            italique: false,
            souligne: false,
            espacement: 0.0,
            couleur: [255, 255, 255, 255],
            couleur_fin: [255, 255, 255, 255],
            contour_couleur: [255, 255, 255, 255],
            contour_epaisseur: 2.0,
        }
    }
}

/// Un titre à rendre : contenu + style + ancrage.
#[derive(Debug, Clone, PartialEq)]
pub struct TexteCfg {
    pub contenu: String,
    pub position: PositionTitre,
    pub cible: CibleTitre,
    /// Rect de l'hôte en px canvas (canvas entier pour le fond, widget sinon).
    pub cadre: (f64, f64, f64, f64),
    pub style: StyleTexte,
    /// Ordre de dessin (croissant = dessous).
    pub z: i32,
}

impl TexteCfg {
    /// Clé de cache : tout ce qui change le **pixels rendus**, rien d'autre
    /// (la position ne change pas la texture, seulement sa destination).
    pub fn cle(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.contenu.hash(&mut h);
        self.style.police.hash(&mut h);
        self.style.taille.to_bits().hash(&mut h);
        self.style.gras.hash(&mut h);
        self.style.italique.hash(&mut h);
        self.style.souligne.hash(&mut h);
        self.style.espacement.to_bits().hash(&mut h);
        self.style.couleur.hash(&mut h);
        self.style.couleur_fin.hash(&mut h);
        self.style.contour_couleur.hash(&mut h);
        self.style.contour_epaisseur.to_bits().hash(&mut h);
        h.finish()
    }
}

/// Destination d'un titre dans le canvas, rotation éventuelle comprise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Rect **non tourné** de la texture texte.
    pub rect: D2D_RECT_F,
    /// Centre de rotation (titre vertical) ; `None` = pas de rotation.
    pub rotation: Option<D2D_POINT_2F>,
}

/// Calcule la destination d'un titre de taille rendue `tw × th`.
///
/// Reproduit `applyTitreFond` / `applyTitreWidget` (diffusion.html) :
/// fond = 8 px **à l'intérieur** du rect de l'hôte, widget = **hors** du rect
/// avec l'écart `MARGE_TITRE_WIDGET` (2 px) de `applyTitreWidget`. `tw × th`
/// est la **boîte CSS** du titre (largeur du texte × `line-height` 1,1),
/// jamais la texture (voir [`CacheTexte::marge`]).
/// Les positions latérales tournent le texte de 90° (sens horaire, comme
/// `writing-mode: vertical-rl`).
pub fn placement(t: &TexteCfg, tw: f32, th: f32) -> Placement {
    let (x, y, w, h) = t.cadre;
    let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let centre_x = x + (w - tw) / 2.0;
    let centre_y = y + (h - th) / 2.0;
    let (gx, gy, vertical) = match (t.cible, t.position) {
        (CibleTitre::Fond, PositionTitre::Haut) => (centre_x, y + MARGE_TITRE, false),
        (CibleTitre::Fond, PositionTitre::Bas) => (centre_x, y + h - MARGE_TITRE - th, false),
        (CibleTitre::Fond, PositionTitre::Gauche) => (x + MARGE_TITRE, centre_y, true),
        (CibleTitre::Fond, PositionTitre::Droite) => (x + w - MARGE_TITRE - th, centre_y, true),
        (CibleTitre::Widget, PositionTitre::Haut) => {
            (centre_x, y - MARGE_TITRE_WIDGET - th, false)
        }
        (CibleTitre::Widget, PositionTitre::Bas) => (centre_x, y + h + MARGE_TITRE_WIDGET, false),
        (CibleTitre::Widget, PositionTitre::Gauche) => {
            (x - MARGE_TITRE_WIDGET - th, centre_y, true)
        }
        (CibleTitre::Widget, PositionTitre::Droite) => {
            (x + w + MARGE_TITRE_WIDGET, centre_y, true)
        }
    };
    if !vertical {
        return Placement {
            rect: D2D_RECT_F {
                left: gx,
                top: gy,
                right: gx + tw,
                bottom: gy + th,
            },
            rotation: None,
        };
    }
    // La boîte tournée occupe `th × tw` : on centre la texture non tournée
    // sur le centre de cette boîte, puis on tourne autour de ce centre.
    let cx = gx + th / 2.0;
    let cy = gy + tw / 2.0;
    Placement {
        rect: D2D_RECT_F {
            left: cx - tw / 2.0,
            top: cy - th / 2.0,
            right: cx + tw / 2.0,
            bottom: cy + th / 2.0,
        },
        rotation: Some(D2D_POINT_2F { x: cx, y: cy }),
    }
}

/// Style de fond et de bordure d'une boîte de widget.
///
/// Miroir des **surcharges CSS** de `.widget` par type (`diffusion.html`) :
/// le verre clair générique ne vaut que pour les widgets qui ne le
/// remplacent pas. Découvert le 2026-10-28 : le chat et l'input-viewer
/// utilisaient à tort le verre clair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleBoite {
    /// `.widget` : verre clair `rgba(224,224,224,0.15)` + bordure 1 px claire.
    Generique,
    /// `.widget.chat` : verre sombre 0,35, anneau inset 1 px, **pas de bordure**.
    Chat,
    /// `.widget.inputviewer` : verre sombre 0,55, bordure 0,12, rayon `1.2cqw`.
    InputViewer,
    /// `.widget.goal` : verre sombre 0,35 + bordure générique.
    Goal,
    /// `.widget.musique` : dégradé `#2e2e2e → #1c1c1c`, bordure 2 px, rayon 3 px.
    Musique,
}

impl StyleBoite {
    /// Épaisseur de la bordure CSS en px (0 = aucune bordure).
    pub fn bordure(self) -> f32 {
        match self {
            StyleBoite::Chat => 0.0,
            StyleBoite::Musique => 2.0,
            _ => 1.0,
        }
    }

    /// Rayon CSS en px pour une boîte de `largeur` px (`cqw` = 1 % de la largeur).
    pub fn rayon(self, largeur: f32) -> f32 {
        match self {
            StyleBoite::InputViewer => 0.012 * largeur,
            StyleBoite::Musique => 3.0,
            _ => 0.0,
        }
    }

    /// Couleur de fond ; `None` = dégradé (musique).
    fn couleur_fond(self) -> Option<D2D1_COLOR_F> {
        match self {
            StyleBoite::Generique => Some(D2D1_COLOR_F {
                r: 224.0 / 255.0,
                g: 224.0 / 255.0,
                b: 224.0 / 255.0,
                a: 0.15,
            }),
            StyleBoite::Chat | StyleBoite::Goal => Some(D2D1_COLOR_F {
                r: 10.0 / 255.0,
                g: 10.0 / 255.0,
                b: 10.0 / 255.0,
                a: 0.35,
            }),
            StyleBoite::InputViewer => Some(D2D1_COLOR_F {
                r: 12.0 / 255.0,
                g: 14.0 / 255.0,
                b: 22.0 / 255.0,
                a: 0.55,
            }),
            StyleBoite::Musique => None,
        }
    }
}

/// Erreurs du rendu de texte. Aucune variante ne fait paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurTexte {
    /// DirectWrite / Direct2D / COM.
    Systeme(String),
    /// Texture GPU ou surface D2D.
    Gpu(String),
    /// Police embarquée illisible.
    Police(String),
}

impl std::fmt::Display for ErreurTexte {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErreurTexte::Systeme(m) => write!(f, "texte (système) : {m}"),
            ErreurTexte::Gpu(m) => write!(f, "texte (GPU) : {m}"),
            ErreurTexte::Police(m) => write!(f, "texte (police) : {m}"),
        }
    }
}

impl std::error::Error for ErreurTexte {}

/// Une texture de texte rendue + sa vue Direct2D (pour `DrawBitmap`).
struct CacheTexte {
    /// La texture doit vivre aussi longtemps que la vue Direct2D qui la
    /// référence (`ID2D1Bitmap1` ne la possède pas).
    _texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    bitmap: windows::Win32::Graphics::Direct2D::ID2D1Bitmap1,
    /// Boîte **CSS** du titre (largeur du texte × hauteur de ligne) — c'est
    /// elle que `placement` positionne, pas la texture.
    largeur: f32,
    hauteur: f32,
    /// Marge de la texture autour de la boîte CSS : (contour, contour +
    /// débord des glyphes hors de la boîte de ligne). Le dessin élargit le
    /// rect de destination d'autant pour que la boîte CSS tombe pile.
    marge: (f32, f32),
}

/// État partagé de la collecte — le renderer est consommé par `Draw`, ses
/// champs ne sont plus lisibles ensuite : ils vivent ici.
#[derive(Default)]
struct EtatContours {
    /// Un contour par run, déjà translaté à son origine.
    runs: Vec<ID2D1Geometry>,
    /// Au moins un run simulé : le tracé exact est faux, on abandonne.
    simule: bool,
}

#[implement(IDWriteTextRenderer)]
struct CollecteContours {
    d2d: ID2D1Factory1,
    etat: Rc<RefCell<EtatContours>>,
}

impl IDWritePixelSnapping_Impl for CollecteContours_Impl {
    fn IsPixelSnappingDisabled(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
    ) -> windows::core::Result<BOOL> {
        Ok(BOOL(1))
    }

    fn GetCurrentTransform(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        transform: *mut DWRITE_MATRIX,
    ) -> windows::core::Result<()> {
        unsafe {
            *transform = DWRITE_MATRIX {
                m11: 1.0,
                m12: 0.0,
                m21: 0.0,
                m22: 1.0,
                dx: 0.0,
                dy: 0.0,
            };
        }
        Ok(())
    }

    fn GetPixelsPerDip(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
    ) -> windows::core::Result<f32> {
        Ok(1.0)
    }
}

impl IDWriteTextRenderer_Impl for CollecteContours_Impl {
    fn DrawGlyphRun(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        baselineoriginx: f32,
        baselineoriginy: f32,
        _measuringmode: DWRITE_MEASURING_MODE,
        glyphrun: *const DWRITE_GLYPH_RUN,
        _glyphrundescription: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        let run = unsafe { &*glyphrun };
        let Some(face) = run.fontFace.as_ref() else {
            self.etat.borrow_mut().simule = true;
            return Ok(());
        };
        if unsafe { face.GetSimulations() }.0 != 0 {
            self.etat.borrow_mut().simule = true;
            return Ok(());
        }
        let geo = unsafe { self.d2d.CreatePathGeometry() }?;
        {
            let sink = unsafe { geo.Open() }?;
            let offsets = if run.glyphOffsets.is_null() {
                None
            } else {
                Some(run.glyphOffsets)
            };
            unsafe {
                face.GetGlyphRunOutline(
                    run.fontEmSize,
                    run.glyphIndices,
                    Some(run.glyphAdvances),
                    offsets,
                    run.glyphCount,
                    run.isSideways,
                    BOOL::from(run.bidiLevel & 1 == 1),
                    &sink,
                )?
            };
            unsafe { sink.Close()? };
        }
        let translation = Matrix3x2::translation(baselineoriginx, baselineoriginy);
        let deplacee = unsafe { self.d2d.CreateTransformedGeometry(&geo, &translation)? };
        self.etat.borrow_mut().runs.push(deplacee.cast()?);
        Ok(())
    }

    fn DrawUnderline(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        _baselineoriginx: f32,
        _baselineoriginy: f32,
        _underline: *const DWRITE_UNDERLINE,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        // Le remplissage reste `DrawTextLayout` : le soulignement y est déjà.
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        _baselineoriginx: f32,
        _baselineoriginy: f32,
        _strikethrough: *const DWRITE_STRIKETHROUGH,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawInlineObject(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        _originx: f32,
        _originy: f32,
        _inlineobject: Option<&IDWriteInlineObject>,
        _issideways: BOOL,
        _isrighttoleft: BOOL,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}


///
/// Un seul `ID2D1DeviceContext` et une seule cible : chaque appel de dessin
/// est encadré par `debut`/`fin` **par le moteur**, qui peut ainsi insérer des
/// `VideoProcessorBlt` bornés entre deux segments (ordre z exact).
/// Image décodée prête à dessiner : texture (elle possède la bitmap),
/// vue Direct2D et taille source en px.
pub(super) type BitmapChat = (
    windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    ID2D1Bitmap1,
    (f32, f32),
);

pub struct DessinateurTexte {
    dwrite: IDWriteFactory5,
    device: ID2D1Device,
    /// La fabrique doit vivre aussi longtemps que ses objets (device, ctx).
    d2d: ID2D1Factory1,
    /// Contexte de périphérique : `PushLayer` avec masque géométrique et
    /// `CreateBitmapFromDxgiSurface` ne vivent que là.
    ctx: ID2D1DeviceContext,
    /// Vue « render target » du canvas : c'est elle qui porte `BeginDraw`,
    /// `DrawBitmap`, `SetTransform` (l'héritage COM ne donne pas de `Deref`
    /// vers `ID2D1RenderTarget` dans `windows` 0.58 — cast explicite).
    cible_rt: ID2D1RenderTarget,
    /// Bitmap cible du canvas — référence conservée pour `SetTarget`
    /// (retour au canvas après un offscreen de perspective).
    cible_bitmap: ID2D1Bitmap1,
    collection: IDWriteFontCollection,
    /// TTF décodés : les références de police pointent dedans, ils doivent
    /// vivre aussi longtemps que la collection.
    _donnees_polices: Vec<Vec<u8>>,
    device_d3d: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    /// Traits des cadres (cadre + comète `animer`) — créés une fois.
    styles: crate::cadres::StylesTrait,
    cache: HashMap<u64, CacheTexte>,
    /// Bitmaps Direct2D des médias, clé = (pointeur de la texture, alpha).
    /// L'entrée conserve la texture : `ID2D1Bitmap1` ne la possède pas.
    bitmaps: HashMap<
        (usize, bool),
        (
            windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
            ID2D1Bitmap1,
        ),
    >,
    /// Blocs de chat mis en page, clé = (version du buffer, filtre, métriques).
    blocs: HashMap<u64, BlocChat>,
    /// Blocs de la barre viewers en tête des widgets chat, clé = (filtre,
    /// taille) — le contenu est suivi par `EtatGoals::version`.
    blocs_viewers: BlocsViewers,
    /// Lignes mises en page des modules horloge, clé = options de mise en
    /// page (`cle_bloc`) — le texte change au tick seconde, formats et
    /// layouts vivent dans le bloc.
    blocs_horloge: BlocsHorloge,
    blocs_objectifs: BlocsObjectifs,
    blocs_musique_module: BlocsMusiqueModule,
    /// Pool de bulles du module équaliseur (spawn/mort à la frame).
    bulles_eq: BullesEq,
    /// Base de temps du balayage des spots (création de la voie).
    debut_spots: std::time::Instant,
    /// Lignes mises en page des widgets goal, clé = (id, hauteur,
    /// plateforme) — le contenu vit dans `EtatGoals`, poussé par l'app.
    blocs_goal: BlocsGoal,
    /// Glyphes vectoriels des logos plateforme (Twitch/Kick, en viewBox) —
    /// créés à la demande, le scale est posé au dessin.
    logos_goal: LogosGoal,
    /// Lignes mises en page des widgets speedrun, clé = (id, métriques) —
    /// l'état vit dans `EtatSpeedrun`, poussé par l'app (frames
    /// `speedrun-etat`, ~15 Hz en run : les slots inchangés ne sont pas
    /// re-mis en page).
    blocs_speedrun: BlocsSpeedrun,
    /// Lignes mises en page + état lissé EQ des widgets musique, clé =
    /// (id, hauteur, mode) — le contenu vit dans `EtatMusique`, poussé par
    /// l'app (events Tauri `musique:update`).
    blocs_musique: BlocsMusique,
    /// Dossier de données (`medias/` y est relatif) — posé par la voie de
    /// rendu, base des chemins de cache des images du chat (avatars +
    /// emotes, `%APPDATA%\com.streamos.v0\medias\cache\`).
    base: PathBuf,
    /// Images du chat décodées, clé = chemin de cache. Seuls les **succès**
    /// sont mémorisés : un fichier encore absent est re-testé à chaque
    /// reconstruction de bloc (téléchargement à la volée, lot G).
    images_chat: HashMap<PathBuf, BitmapChat>,
    /// Cache de la carte alerte en cours (une seule, contrat domaine) —
    /// re-mesurée à chaque play / repositionnement (clé `cle_alerte`), le
    /// contenu vit dans `EtatAlertes`, poussé par l'app (frames
    /// `alerte-*` / `position-overlay-config` du canal `chat_tx`).
    bloc_alerte: Option<BlocAlerte>,
    /// Cache de la carte bienvenue en cours (une seule, contrat domaine) —
    /// re-mesurée à chaque play / repositionnement (clé `cle_welcome`), le
    /// contenu vit dans `EtatWelcome`, poussé par l'app (frames
    /// `welcome-clip-*` / `position-overlay-config` du canal `chat_tx`).
    bloc_welcome: Option<BlocWelcome>,
    /// Cache de la barre « premier message » (une seule) — re-mesurée à
    /// chaque play / config (clé `cle_bandeau`).
    bloc_bandeau: Option<BlocBandeau>,
    /// Cache de la carte minuteur de l'écran d'attente (une seule) —
    /// re-mesurée à chaque tick du chrono (clé `cle_attente`), le contenu
    /// vit dans `EtatAttente`, poussé par l'app (frames `ecran-attente-etat`
    /// du canal `chat_tx`).
    bloc_attente: Option<BlocAttente>,
    /// Média du fond de l'écran d'attente (image ou vidéo en boucle) —
    /// rechargé seulement si `media|kind` change (`eaMediaSig` du DOM).
    attente_media: Option<crate::media::MediaChargee>,
    attente_media_rel: Option<String>,
    attente_media_kind: Option<String>,
    /// Assets de marque du layout « signature » (Streamy ×2, wordmark),
    /// décodés une fois depuis les PNG embarqués.
    assets_attente: HashMap<AssetMarque, Asset>,
    /// Contexte GPU **partagé** avec la scène : les overlays qui portent un
    /// média (écran d'attente) décodent sur le même device.
    contexte: ContexteGpu,
    /// Cache du cadre de la carte pad (une seule) — re-mesurée au play et au
    /// repositionnement (clé `cle_pad`).
    bloc_pad: Option<BlocPad>,
    /// Image/vidéo du pad (jouée une fois) — rechargée si `media|kind` change.
    pad_media: Option<crate::media::MediaChargee>,
    pad_media_rel: Option<String>,
    pad_media_kind: Option<String>,
    /// Son séparé du pad (image, ou touche « audio » sans visuel).
    pad_audio: Option<crate::media::AudioFichier>,
    pad_audio_rel: Option<String>,
    /// Géométries et tirage déterministe du décor de la présentation
    /// streamer (trapèzes des spots, fauteuils, places, poussière) —
    /// reconstruits quand la version de l'overlay ou le canvas change.
    bloc_presentation: Option<BlocPresentation>,
    /// Carte de la présentation (tranche E6b) : colonnes mises en page et
    /// liste d'éléments à peindre, reconstruites au changement de version.
    bloc_carte: Option<BlocCarte>,
    /// Dernière tentative de lecture d'une image distante de la présentation
    /// (bannière, avatar, boîtes de jeux, vignettes de clips) — re-tentative
    /// 500 ms tant que le téléchargement de l'app n'a pas abouti.
    pres_images_tentative: HashMap<String, std::time::Instant>,
    /// Vidéo du lecteur clip (fichier **local** préchargé par l'app,
    /// `medias/clips/<fnv16(url)>.mp4`) — jamais de réseau (§13.8).
    pres_clip_media: Option<crate::media::MediaChargee>,
    pres_clip_url: Option<String>,
    /// Dernière tentative de lecture du fichier du clip (re-tentative 500 ms
    /// tant que l'app n'a pas fini de télécharger).
    pres_clip_tentative: Option<std::time::Instant>,
    /// Titre du lecteur clip mis en page : (clé, ligne, x, y).
    bloc_clip: Option<(u64, presentation::LigneC, f32, f32)>,
    /// Lignes mises en page du pop de validation des splits (badge, hero, mot).
    bloc_splitpop: Option<BlocSplitPop>,
    /// Son du pop de validation (`srPointsSon`) — rouvert à chaque pop,
    /// avancé à chaque frame, jamais de réseau (fichier `medias/…`).
    splitpop_audio: Option<crate::media::AudioFichier>,
    /// Chemin relatif du son chargé (diagnostic).
    splitpop_audio_rel: Option<String>,
    /// Dernière version de son déjà jouée (`EtatSplitPop::son_version`).
    splitpop_son_vu: u64,
    /// Les 2 pistes audio de la présentation (`voix`, `musique`) — fichiers
    /// **locaux** `medias/…`, indexés par `PistePresentation::index`.
    pres_audios: [Option<crate::media::AudioFichier>; 2],
    /// Signature du fichier chargé par piste (`medias/…`) — évite de rouvrir
    /// le Source Reader à chaque frame.
    pres_audio_rel: [Option<String>; 2],
    /// Cache de l'arène `!fight` — re-mise en page quand la version change ou
    /// que la seconde affichée change (échéances décomptées en local).
    bloc_fight: Option<BlocFight>,
    /// Formats de libellés input-viewer, clé = (taille en dixièmes, gras).
    formats_input: HashMap<(u32, bool), IDWriteTextFormat>,
    /// Plans à perspective : offscreen intermédiaire + effet de projection,
    /// triés par `z` (`persps_depuis_scene`).
    persps: Vec<PlanPersp>,
    /// Chaînes d'effets média (`ColorMatrix` [→ `GaussianBlur`]) par
    /// (bitmap, matrice, σ) — l'effet final garde son graphe en vie.
    effets: HashMap<u64, Vec<ID2D1Effect>>,
    /// Buffers de pixelisation (`boite / pf`) : effets + transform y sont
    /// dessinés avant l'upscale sans lissage — miroir des petits canvas de
    /// `dessinerContenuFond` / du chemin morph.
    petits: HashMap<(u32, u32), (windows::Win32::Graphics::Direct3D11::ID3D11Texture2D, ID2D1Bitmap1)>,
    /// Pipeline du warp morph (bulge/pinch), initialisé à la demande.
    warp: Option<crate::morph::WarpMorph>,
    /// Offscreen pré-warp par boîte (`morphOffscreen` du DOM).
    morphes: HashMap<CleBoite, EntreeMorphIn>,
    /// Sortie du warp par boîte : texture RT+SRV + bitmap D2D de lecture + RTV.
    sorties_morph: HashMap<CleBoite, EntreeMorphOut>,
}

/// Clé d'une boîte morphée : bits du rect canvas `(x, y, w, h)`.
type CleBoite = (u64, u64, u64, u64);
/// Offscreen pré-warp : texture RT+SRV + bitmap D2D TARGET + vue SRV.
type EntreeMorphIn = (
    windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    ID2D1Bitmap1,
    windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
);
/// Sortie du warp : texture RT+SRV + bitmap D2D de lecture + vue RTV.
type EntreeMorphOut = (
    windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    ID2D1Bitmap1,
    windows::Win32::Graphics::Direct3D11::ID3D11RenderTargetView,
);

impl DessinateurTexte {
    /// Prépare Direct2D sur le canvas BGRA (cible des `DrawBitmap`).
    pub fn nouveau(
        contexte: &ContexteGpu,
        canvas: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    ) -> Result<Self, ErreurTexte> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

        let d2d: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
                .map_err(|e| ErreurTexte::Systeme(format!("D2D1CreateFactory : {}", e.message())))?;
        let dwrite: IDWriteFactory5 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
            .map_err(|e| ErreurTexte::Systeme(format!("DWriteCreateFactory : {}", e.message())))?;

        let dxgi: windows::Win32::Graphics::Dxgi::IDXGIDevice = contexte
            .device()
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("IDXGIDevice : {}", e.message())))?;
        let device = unsafe { d2d.CreateDevice(&dxgi) }
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Device : {}", e.message())))?;
        let cible = unsafe { device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1DeviceContext : {}", e.message())))?;
        unsafe { cible.SetUnitMode(D2D1_UNIT_MODE_PIXELS) };
        let cible_rt: ID2D1RenderTarget = cible
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1RenderTarget : {}", e.message())))?;

        let surface: IDXGISurface = canvas
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("IDXGISurface : {}", e.message())))?;
        // Canvas opaque (les calques sont déjà composés) : alpha ignoré.
        // `CANNOT_DRAW` : le canvas n'est jamais échantillonné par D2D (il est
        // lu par le video processor), et c'est **obligatoire** pour une
        // texture sans `D3D11_BIND_SHADER_RESOURCE` — sinon
        // `CreateBitmapFromDxgiSurface` rend E_INVALIDARG (vérifié 2026-10-28,
        // `examples/d2d_diag.rs`).
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let cible_bitmap = unsafe { cible.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
            .map_err(|e| ErreurTexte::Gpu(format!("bitmap canvas : {}", e.message())))?;
        unsafe { cible.SetTarget(&cible_bitmap) };

        let (collection, donnees) = charger_polices(&dwrite)?;
        let collection: IDWriteFontCollection = collection
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("IDWriteFontCollection : {}", e.message())))?;
        let styles = crate::cadres::StylesTrait {
            cadre: crate::cadres::style_trait(&d2d)
                .map_err(|e| ErreurTexte::Systeme(e.to_string()))?,
            comete: crate::cadres::style_comete(&d2d)
                .map_err(|e| ErreurTexte::Systeme(e.to_string()))?,
        };
        Ok(Self {
            dwrite,
            device,
            d2d,
            ctx: cible,
            cible_rt,
            cible_bitmap,
            collection,
            _donnees_polices: donnees,
            device_d3d: contexte.device().clone(),
            styles,
            cache: HashMap::new(),
            bitmaps: HashMap::new(),
            blocs: HashMap::new(),
            blocs_viewers: HashMap::new(),
            blocs_horloge: HashMap::new(),
            blocs_objectifs: HashMap::new(),
            blocs_musique_module: HashMap::new(),
            bulles_eq: Vec::new(),
            debut_spots: std::time::Instant::now(),
            blocs_goal: HashMap::new(),
            logos_goal: HashMap::new(),
            blocs_speedrun: HashMap::new(),
            blocs_musique: HashMap::new(),
            bloc_alerte: None,
            bloc_welcome: None,
            bloc_bandeau: None,
            bloc_attente: None,
            attente_media: None,
            attente_media_rel: None,
            attente_media_kind: None,
            assets_attente: HashMap::new(),
            contexte: ContexteGpu::depuis_device(contexte.device().clone())
                .map_err(|e| ErreurTexte::Gpu(format!("contexte partagé : {e}")))?,
            bloc_pad: None,
            pad_media: None,
            pad_media_rel: None,
            pad_media_kind: None,
            pad_audio: None,
            pad_audio_rel: None,
            bloc_presentation: None,
            bloc_carte: None,
            pres_images_tentative: HashMap::new(),
            pres_clip_media: None,
            pres_clip_url: None,
            pres_clip_tentative: None,
            bloc_clip: None,
            bloc_splitpop: None,
            splitpop_audio: None,
            splitpop_audio_rel: None,
            splitpop_son_vu: 0,
            pres_audios: [None, None],
            pres_audio_rel: [None, None],
            bloc_fight: None,
            base: PathBuf::new(),
            images_chat: HashMap::new(),
            formats_input: HashMap::new(),
            persps: Vec::new(),
            effets: HashMap::new(),
            petits: HashMap::new(),
            warp: None,
            morphes: HashMap::new(),
            sorties_morph: HashMap::new(),
        })
    }

    /// Dossier de données — base des chemins de cache des images du chat.
    pub fn definir_base(&mut self, base: &Path) {
        self.base = base.to_path_buf();
    }

    /// Décode un fichier image (WIC) en bitmap Direct2D.
    ///
    /// `None` si le fichier manque ou est illisible — jamais fatal : le
    /// rendu retombe sur le texte (emote) ou sur l'espace vide (avatar).
    pub(super) fn bitmap_fichier(&mut self, chemin: &Path) -> Option<BitmapChat> {
        let (w, h, pixels, alpha) = match crate::media::decoder_pbgra(chemin) {
            Ok(d) if d.0 > 0 && d.1 > 0 => d,
            Ok(_) => return None,
            Err(e) => {
                log::debug!("chat : image {} ignorée ({e})", chemin.display());
                return None;
            }
        };
        let desc = DescTexture::new(Format::Bgra8, w, h).ok()?;
        let texture = self.contexte_texture(&desc).ok()?;
        let ressource: windows::Win32::Graphics::Direct3D11::ID3D11Resource =
            texture.cast().ok()?;
        let contexte = unsafe { self.device_d3d.GetImmediateContext() }.ok()?;
        unsafe {
            contexte.UpdateSubresource(
                Some(&ressource),
                0,
                None,
                pixels.as_ptr() as *const std::ffi::c_void,
                w * 4,
                0,
            );
        }
        let bitmap = self.bitmap_media(&texture, alpha).ok()?;
        Some((texture, bitmap, (w as f32, h as f32)))
    }

    /// Image du chat (avatar ou emote) — cache par chemin de cache.
    ///
    /// Le fichier est écrit par l'app (`cache_images.rs`, téléchargement à
    /// la volée) : tant qu'il manque, `None` et le rendu garde le texte.
    pub(super) fn image_cache(&mut self, url: &str) -> Option<BitmapChat> {
        let chemin = crate::chat::chemin_cache(&self.base, url);
        if let Some(deja) = self.images_chat.get(&chemin) {
            return Some(deja.clone());
        }
        // Fichier pas encore téléchargé : ne rien mémoriser (le prochain
        // rebuild du bloc retentera — le chat se répare tout seul).
        if !chemin.is_file() {
            return None;
        }
        let charge = self.bitmap_fichier(&chemin)?;
        self.images_chat.insert(chemin, charge.clone());
        Some(charge)
    }

    /// Nombre de titres en cache (diagnostic / tests).
    pub fn taille_cache(&self) -> usize {
        self.cache.len()
    }

    /// Ouvre une passe de dessin (le moteur peut insérer des blts entre deux
    /// passes : la cible Direct2D ne doit jamais rester ouverte pendant un
    /// `VideoProcessorBlt`).
    pub fn debut(&mut self) {
        unsafe { self.cible_rt.BeginDraw() };
    }

    /// Ferme la passe de dessin (flush Direct2D).
    pub fn fin(&mut self) -> Result<(), ErreurTexte> {
        unsafe { self.cible_rt.EndDraw(None, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("EndDraw : {}", e.message())))
    }

    /// Efface le canvas avec une couleur **opaque** (dans une passe ouverte).
    ///
    /// Nécessaire au chemin segmenté : le premier lot du compositeur ne peint
    /// plus toute la surface (il est borné au calque), donc plus personne ne
    /// remet le fond à zéro entre deux frames.
    pub fn effacer(&mut self, couleur: [u8; 4]) {
        unsafe { self.cible_rt.Clear(Some(&couleur_d2d(couleur))) };
    }

    /// Dessine les titres sur le canvas, dans l'ordre de `z`.
    ///
    /// Le canvas n'est **pas** effacé : il porte déjà les calques composés.
    /// Les textures de texte absentes du cache sont rendues à la volée, une
    /// seule fois, puis réutilisées à chaque frame.
    pub fn dessiner(&mut self, textes: &[TexteCfg]) -> Result<(), ErreurTexte> {
        self.debut();
        self.dessiner_textes(textes)?;
        self.fin()
    }

    /// Titres seuls, **sans** `BeginDraw`/`EndDraw` (appelé dans une passe).
    pub fn dessiner_textes(&mut self, textes: &[TexteCfg]) -> Result<(), ErreurTexte> {
        if textes.is_empty() {
            return Ok(());
        }
        let mut ordre: Vec<&TexteCfg> = textes.iter().collect();
        ordre.sort_by_key(|t| t.z);

        let mut plan: Vec<(u64, Placement)> = Vec::with_capacity(ordre.len());
        for texte in ordre {
            let cle = texte.cle();
            if !self.cache.contains_key(&cle) {
                let entree = self.rendre(texte)?;
                self.cache.insert(cle, entree);
            }
            let entree = &self.cache[&cle];
            plan.push((cle, placement(texte, entree.largeur, entree.hauteur)));
        }

        for (cle, place) in &plan {
            let Some(entree) = self.cache.get(cle) else {
                continue;
            };
            // La texture déborde la boîte CSS (contour + glyphes hors de la
            // ligne) : on élargit la destination d'autant pour que la boîte
            // CSS tombe exactement sur `place.rect`.
            let dst = D2D_RECT_F {
                left: place.rect.left - entree.marge.0,
                top: place.rect.top - entree.marge.1,
                right: place.rect.right + entree.marge.0,
                bottom: place.rect.bottom + entree.marge.1,
            };
            unsafe {
                if let Some(centre) = place.rotation {
                    // ⚠️ `Matrix3x2::rotation` passe par `D2D1MakeRotateMatrix`,
                    // qui attend des **degrés** (pas des radians) : π/2 donnait
                    // 1,57° et le titre restait horizontal.
                    let rotation = Matrix3x2::rotation(90.0, centre.x, centre.y);
                    self.cible_rt.SetTransform(&rotation);
                }
                self.cible_rt.DrawBitmap(
                    &entree.bitmap,
                    Some(&dst),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );
                if place.rotation.is_some() {
                    self.cible_rt.SetTransform(&Matrix3x2::identity());
                }
            }
        }
        Ok(())
    }

    /// Géométrie (coordonnées canvas) du polygone d'un cadre.
    pub fn geometrie_cadre(&self, cfg: &CadreCfg) -> Result<ID2D1PathGeometry, ErreurTexte> {
        crate::cadres::geometrie(&self.d2d, cfg)
            .map_err(|e| ErreurTexte::Systeme(e.to_string()))
    }

    /// Géométrie fermée d'une liste de points (coordonnées canvas) — forme
    /// dédiée caméra (`cameraForme`, contrat 9) : le clip doit vivre dans la
    /// même factory que la cible de rendu, d'où cet accès — une géométrie
    /// créée par une autre `ID2D1Factory1` serait refusée au dessin.
    pub fn geometrie_points(&self, points: &[(f64, f64)]) -> Result<ID2D1PathGeometry, ErreurTexte> {
        crate::cadres::geometrie_fermee(&self.d2d, points)
            .map_err(|e| ErreurTexte::Systeme(e.to_string()))
    }

    /// Peint un cadre (clip au polygone, 2 styles) — dans une passe ouverte.
    /// `temps_s` = horloge du flux (comète `animer`).
    pub fn dessiner_cadre(
        &mut self,
        cfg: &CadreCfg,
        geo: &ID2D1PathGeometry,
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        crate::cadres::dessiner(
            &self.d2d,
            &self.ctx,
            &self.cible_rt,
            geo,
            cfg,
            &self.styles,
            temps_s,
        )
        .map_err(|e: ErreurCadre| ErreurTexte::Systeme(e.to_string()))
    }

    /// Anneau `cameraBord` d'une forme caméra dédiée — dans une passe ouverte.
    /// `clip` = polygone du cercle (`None` = rect `carre`, clip axe-aligné).
    pub fn dessiner_anneau_camera(
        &mut self,
        cfg: &crate::moteur::SourceCfg,
        clip: Option<&ID2D1PathGeometry>,
    ) -> Result<(), ErreurTexte> {
        let Some(anneau) = cfg.anneau.as_ref() else {
            return Ok(());
        };
        let forme = crate::cadres::AnneauRendu {
            rect: cfg.rect,
            cercle: matches!(cfg.forme_camera, Some(crate::moteur::FormeCamera::Cercle)),
            clip,
        };
        crate::cadres::dessiner_anneau_camera(
            &self.d2d,
            &self.ctx,
            &self.cible_rt,
            &forme,
            anneau,
            &self.styles.cadre,
        )
        .map_err(|e| ErreurTexte::Systeme(e.to_string()))
    }

    /// Voile d'un widget, selon son type (surcharges CSS de `.widget`).
    ///
    /// `rect` = **boîte de contenu**. `clip` = polygone du cadre quand le
    /// widget en a un : le CSS applique le `clip-path` à l'élément entier,
    /// donc au fond comme au contenu — et retire alors bordure et rayon.
    /// Sans cadre, le fond couvre la boîte de bordure (`background-clip:
    /// border-box`) : il est élargi de l'épaisseur de la bordure.
    pub fn dessiner_fond(
        &mut self,
        style: StyleBoite,
        rect: RECT,
        clip: Option<&ID2D1PathGeometry>,
    ) -> Result<(), ErreurTexte> {
        let marge = if clip.is_some() { 0.0 } else { style.bordure() };
        let boite = RECT {
            left: rect.left - marge as i32,
            top: rect.top - marge as i32,
            right: rect.right + marge as i32,
            bottom: rect.bottom + marge as i32,
        };
        let Some(dst) = rect_visible(boite) else {
            return Ok(());
        };
        let rayon = if clip.is_some() {
            0.0
        } else {
            style.rayon(boite.right as f32 - boite.left as f32)
        };
        let brosse = self.brosse_fond(style, &dst)?;
        match clip {
            Some(geo) => unsafe { self.cible_rt.FillGeometry(geo, &brosse, None) },
            None if rayon > 0.0 => unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: dst,
                        radiusX: rayon,
                        radiusY: rayon,
                    },
                    &brosse,
                )
            },
            None => unsafe { self.cible_rt.FillRectangle(&dst, &brosse) },
        };
        if style == StyleBoite::Chat {
            // `box-shadow: inset 0 0 0 1px` — ce n'est **pas** une bordure :
            // l'anneau survit à un cadre actif (seul `border` est retiré).
            let anneau = D2D_RECT_F {
                left: dst.left + 0.5,
                top: dst.top + 0.5,
                right: dst.right - 0.5,
                bottom: dst.bottom - 0.5,
            };
            let brosse = unsafe {
                self.cible_rt.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.18,
                    },
                    None,
                )
            }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse anneau : {}", e.message())))?;
            unsafe { self.cible_rt.DrawRectangle(&anneau, &brosse, 1.0, None) };
        }
        Ok(())
    }

    /// Bordure d'un widget — **uniquement sans cadre actif** (`applyCadreWidget`
    /// met `border: none`). Trait centré sur le bord extérieur de la boîte de
    /// contenu, comme une bordure CSS en `content-box`.
    pub fn dessiner_bordure(&mut self, style: StyleBoite, rect: RECT) -> Result<(), ErreurTexte> {
        let epaisseur = style.bordure();
        if epaisseur <= 0.0 {
            return Ok(());
        }
        let Some(_) = rect_visible(rect) else {
            return Ok(());
        };
        let couleur = match style {
            StyleBoite::InputViewer => D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.12,
            },
            StyleBoite::Musique => couleur_d2d([0x55, 0x55, 0x55, 255]),
            _ => D2D1_COLOR_F {
                r: 224.0 / 255.0,
                g: 224.0 / 255.0,
                b: 224.0 / 255.0,
                a: 0.6,
            },
        };
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse bordure : {}", e.message())))?;
        let demi = epaisseur / 2.0;
        let trait_rect = D2D_RECT_F {
            left: rect.left as f32 - demi,
            top: rect.top as f32 - demi,
            right: rect.right as f32 + demi,
            bottom: rect.bottom as f32 + demi,
        };
        let rayon = style.rayon(rect.right as f32 - rect.left as f32);
        // `ID2D1RenderTarget::DrawRectangle` rend `()` (piège §35.7.3).
        if rayon > 0.0 {
            unsafe {
                self.cible_rt.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: trait_rect,
                        radiusX: rayon,
                        radiusY: rayon,
                    },
                    &brosse,
                    epaisseur,
                    None,
                )
            };
        } else {
            unsafe { self.cible_rt.DrawRectangle(&trait_rect, &brosse, epaisseur, None) };
        }
        Ok(())
    }

    /// Brosse du fond d'une boîte : unie, ou dégradé vertical pour la musique.
    fn brosse_fond(
        &self,
        style: StyleBoite,
        dst: &D2D_RECT_F,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let Some(couleur) = style.couleur_fond() else {
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: couleur_d2d([0x2e, 0x2e, 0x2e, 255]),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: couleur_d2d([0x1c, 0x1c, 0x1c, 255]),
                },
            ];
            let collection = unsafe {
                self.cible_rt
                    .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("CreateGradientStopCollection : {}", e.message()))
            })?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: D2D_POINT_2F {
                    x: dst.left,
                    y: dst.top,
                },
                endPoint: D2D_POINT_2F {
                    x: dst.left,
                    y: dst.bottom,
                },
            };
            let brosse = unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("CreateLinearGradientBrush : {}", e.message()))
                })?;
            return brosse
                .cast()
                .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush : {}", e.message())));
        };
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse fond : {}", e.message())))?;
        brosse
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush : {}", e.message())))
    }

    /// Contours des glyphes d'un layout — `None` si une police est
    /// **simulée** (gras/oblique synthétique d'une famille à une seule
    /// graisse) : `GetGlyphRunOutline` rend alors le contour non embelli et
    /// le tracé serait faux. L'appelant retombe sur l'approximation.
    fn contours_glyphes(
        &self,
        layout: &windows::Win32::Graphics::DirectWrite::IDWriteTextLayout,
    ) -> Result<Option<Vec<ID2D1Geometry>>, ErreurTexte> {
        let etat = Rc::new(RefCell::new(EtatContours::default()));
        let renderer: IDWriteTextRenderer = CollecteContours {
            d2d: self.d2d.clone(),
            etat: etat.clone(),
        }
        .into();
        unsafe { layout.Draw(None, &renderer, 0.0, 0.0) }
            .map_err(|e| ErreurTexte::Systeme(format!("contours du titre : {}", e.message())))?;
        let e = etat.borrow();
        Ok(if e.simule || e.runs.is_empty() {
            None
        } else {
            Some(e.runs.clone())
        })
    }

    /// Rend un titre dans une texture BGRA transparente (une seule fois).
    fn rendre(&mut self, texte: &TexteCfg) -> Result<CacheTexte, ErreurTexte> {
        let format = self.format(&texte.style)?;
        let contenu: Vec<u16> = texte.contenu.encode_utf16().collect();
        if contenu.is_empty() {
            return Err(ErreurTexte::Systeme("titre vide".to_string()));
        }
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, 8192.0, 4096.0)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout : {}", e.message())))?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("SetWordWrapping : {}", e.message())))?;

        let plage = DWRITE_TEXT_RANGE {
            startPosition: 0,
            length: contenu.len() as u32,
        };
        if texte.style.espacement != 0.0 {
            let layout1: IDWriteTextLayout1 = layout
                .cast()
                .map_err(|e| ErreurTexte::Systeme(format!("IDWriteTextLayout1 : {}", e.message())))?;
            unsafe {
                layout1.SetCharacterSpacing(0.0, texte.style.espacement, 0.0, plage)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("SetCharacterSpacing : {}", e.message())))?;
        }
        if texte.style.souligne {
            unsafe { layout.SetUnderline(true, plage) }
                .map_err(|e| ErreurTexte::Systeme(format!("SetUnderline : {}", e.message())))?;
        }

        // `line-height: 1.1` du DOM (`.widget-titre` / `.bg-titre`) : sans
        // cela DirectWrite mesure la hauteur **naturelle** de la police
        // (~1,2-1,35 em) — la boîte du titre, donc son placement, dérive avec
        // `titreTaille`. `UNIFORM` + un baseline « demi-interligne » reproduit
        // la répartition CSS : le glyphe est centré dans la boîte de ligne
        // (half-leading au-dessus et au-dessous).
        let interligne = INTERLIGNE_TITRE * texte.style.taille;
        let mut ligne = [DWRITE_LINE_METRICS::default()];
        let mut nb_lignes = 0u32;
        unsafe { layout.GetLineMetrics(Some(&mut ligne), &mut nb_lignes) }
            .map_err(|e| ErreurTexte::Systeme(format!("GetLineMetrics : {}", e.message())))?;
        let naturelle = ligne[0].height;
        let demi_interligne = (interligne - naturelle) / 2.0;
        unsafe {
            layout.SetLineSpacing(
                DWRITE_LINE_SPACING_METHOD_UNIFORM,
                interligne,
                demi_interligne + ligne[0].baseline,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("SetLineSpacing : {}", e.message())))?;

        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("GetMetrics : {}", e.message())))?;

        // Boîte **CSS** du titre : largeur du texte × `line-height` (1,1 em).
        // C'est elle que le DOM positionne (`.widget-titre`/`.bg-titre`) —
        // la texture est plus grande pour le contour et, quand l'interligne
        // est plus court que la hauteur naturelle de la police, pour le
        // débord des glyphes (le CSS les laisse sortir de la boîte de ligne).
        let largeur = metriques.width.max(1.0);
        let hauteur = metriques.height.max(1.0);
        let debord = (texte.style.contour_epaisseur * 0.5).ceil() + 2.0;
        let marge = (debord, debord + (-demi_interligne).max(0.0));
        let (l_tex, h_tex) = (
            (largeur + 2.0 * marge.0).ceil().max(1.0),
            (hauteur + 2.0 * marge.1).ceil().max(1.0),
        );

        let desc = DescTexture::new(Format::Bgra8, l_tex as u32, h_tex as u32)
            .map_err(|e| ErreurTexte::Gpu(e.to_string()))?;
        let texture = self
            .contexte_texture(&desc)
            .map_err(ErreurTexte::Gpu)?;

        let surface: IDXGISurface = texture
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("IDXGISurface texte : {}", e.message())))?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let ctx = unsafe { self.device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1DeviceContext texte : {}", e.message())))?;
        unsafe { ctx.SetUnitMode(D2D1_UNIT_MODE_PIXELS) };
        let rt: ID2D1RenderTarget = ctx
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1RenderTarget texte : {}", e.message())))?;
        let bitmap = unsafe { ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
            .map_err(|e| ErreurTexte::Gpu(format!("bitmap texte : {}", e.message())))?;
        unsafe { ctx.SetTarget(&bitmap) };
        unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };

        unsafe { rt.BeginDraw() };
        let transparent = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        unsafe { rt.Clear(Some(&transparent)) };

        // Contour (`-webkit-text-stroke`, `paint-order: stroke fill`) : tracé
        // **exact** des glyphes — `GetGlyphRunOutline` + `Widen` (trait centré,
        // comme le CSS) — quand la police n'est pas simulée. Une police
        // synthétique (gras/oblique d'une famille à une seule graisse) n'a pas
        // ce tracé : on garde alors l'approximation par 16 passes décalées.
        if texte.style.contour_epaisseur > 0.0 {
            let couleur = couleur_d2d(texte.style.contour_couleur);
            let brosse = unsafe { rt.CreateSolidColorBrush(&couleur, None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse contour : {}", e.message())))?;
            let contours = self.contours_glyphes(&layout)?;
            if let Some(runs) = contours {
                // La géométrie est en coordonnées du layout : `Widen` la
                // translate à l'origine de la texture (`marge`).
                let translation = Matrix3x2::translation(marge.0, marge.1);
                for geo in &runs {
                    let trace = unsafe { self.d2d.CreatePathGeometry() }.map_err(|e| {
                        ErreurTexte::Systeme(format!("géométrie contour : {}", e.message()))
                    })?;
                    {
                        let sink = unsafe { trace.Open() }.map_err(|e| {
                            ErreurTexte::Systeme(format!("sink contour : {}", e.message()))
                        })?;
                        unsafe {
                            geo.Widen(
                                texte.style.contour_epaisseur,
                                None,
                                Some(&translation),
                                0.25,
                                &sink,
                            )
                        }
                        .map_err(|e| {
                            ErreurTexte::Systeme(format!("widen contour : {}", e.message()))
                        })?;
                        unsafe { sink.Close() }.map_err(|e| {
                            ErreurTexte::Systeme(format!("sink contour (close) : {}", e.message()))
                        })?;
                    }
                    unsafe { rt.FillGeometry(&trace, &brosse, None) };
                }
            } else {
                let rayon = (texte.style.contour_epaisseur * 0.5).max(0.5);
                for (dx, dy) in couronne(rayon) {
                    let origine = D2D_POINT_2F {
                        x: marge.0 + dx,
                        y: marge.1 + dy,
                    };
                    unsafe {
                        rt.DrawTextLayout(origine, &layout, &brosse, D2D1_DRAW_TEXT_OPTIONS_NONE)
                    };
                }
            }
        }

        // Remplissage : dégradé 135° (haut-gauche → bas-droite), comme le CSS.
        let brosse = self.brosse_degrade(&rt, &texte.style, l_tex, h_tex)?;
        let origine = D2D_POINT_2F {
            x: marge.0,
            y: marge.1,
        };
        unsafe { rt.DrawTextLayout(origine, &layout, &brosse, D2D1_DRAW_TEXT_OPTIONS_NONE) };

        unsafe { rt.EndDraw(None, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("EndDraw texte : {}", e.message())))?;

        Ok(CacheTexte {
            _texture: texture,
            bitmap,
            largeur,
            hauteur,
            marge,
        })
    }

    fn contexte_texture(
        &self,
        desc: &DescTexture,
    ) -> Result<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D, String> {
        use windows::Win32::Graphics::Direct3D11::{
            ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        };
        use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
        let desc_d3d = D3D11_TEXTURE2D_DESC {
            Width: desc.largeur,
            Height: desc.hauteur,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.format.dxgi(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            // SHADER_RESOURCE : sans lui, D2D refuse la texture comme cible
            // dessinable (`TARGET` + RT seul = E_INVALIDARG).
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe { self.device_d3d.CreateTexture2D(&desc_d3d, None, Some(&mut texture)) }
            .map_err(|e| format!("CreateTexture2D texte : {}", e.message()))?;
        texture.ok_or_else(|| "texture texte nulle".to_string())
    }

    /// `IDWriteTextFormat` du style — collection embarquée si la police existe.
    fn format(&self, style: &StyleTexte) -> Result<IDWriteTextFormat, ErreurTexte> {
        let poids = if style.gras {
            DWRITE_FONT_WEIGHT_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        let pente = if style.italique {
            DWRITE_FONT_STYLE_ITALIC
        } else {
            DWRITE_FONT_STYLE_NORMAL
        };
        let famille = famille_police(&style.police).unwrap_or("Segoe UI");
        let nom: Vec<u16> = famille.encode_utf16().chain(std::iter::once(0)).collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        let collection = famille_police(&style.police).map(|_| &self.collection);
        unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(nom.as_ptr()),
                collection,
                poids,
                pente,
                DWRITE_FONT_STRETCH_NORMAL,
                style.taille.max(1.0),
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat({famille}) : {}", e.message())))
    }

    /// Brosse dégradée (135°) sur la boîte du texte.
    fn brosse_degrade(
        &self,
        rt: &ID2D1RenderTarget,
        style: &StyleTexte,
        largeur: f32,
        hauteur: f32,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        if style.couleur == style.couleur_fin {
            let brosse = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(style.couleur), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse unie : {}", e.message())))?;
            return Ok(brosse.into());
        }
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: couleur_d2d(style.couleur),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: couleur_d2d(style.couleur_fin),
            },
        ];
        let collection: ID2D1GradientStopCollection = unsafe {
            rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateGradientStopCollection : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: D2D_POINT_2F { x: 0.0, y: 0.0 },
            endPoint: D2D_POINT_2F {
                x: largeur,
                y: hauteur,
            },
        };
        let brosse: ID2D1LinearGradientBrush =
            unsafe { rt.CreateLinearGradientBrush(&props, None, &collection) }
                .map_err(|e| ErreurTexte::Systeme(format!("CreateLinearGradientBrush : {}", e.message())))?;
        Ok(brosse.into())
    }
}

/// Décode les 20 `.woff2` embarqués et construit la collection DirectWrite.
fn charger_polices(
    dwrite: &IDWriteFactory5,
) -> Result<(IDWriteFontCollection1, Vec<Vec<u8>>), ErreurTexte> {
    let loader = unsafe { dwrite.CreateInMemoryFontFileLoader() }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateInMemoryFontFileLoader : {}", e.message())))?;
    // ⚠️ Obligatoire : sans enregistrement, `CreateInMemoryFontFileReference`
    // rend E_INVALIDARG (vérifié 2026-10-28 — pas d'erreur explicite côté
    // DirectWrite, le loader est simplement inconnu de la factory).
    unsafe { dwrite.RegisterFontFileLoader(&loader) }.map_err(|e| {
        ErreurTexte::Systeme(format!("RegisterFontFileLoader : {}", e.message()))
    })?;
    let builder = unsafe { dwrite.CreateFontSetBuilder() }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateFontSetBuilder : {}", e.message())))?;
    let mut donnees: Vec<Vec<u8>> = Vec::with_capacity(POLICES.len());
    for (id, _, woff2) in POLICES {
        let ttf = wuff::decompress_woff2(woff2)
            .map_err(|e| ErreurTexte::Police(format!("{id} : {e}")))?;
        // `CreateInMemoryFontFileReference` ne copie pas les octets : la
        // référence pointe sur le tampon du Vec, conservé dans `donnees`.
        let reference = unsafe {
            loader.CreateInMemoryFontFileReference(
                dwrite,
                ttf.as_ptr() as *const std::ffi::c_void,
                ttf.len() as u32,
                None,
            )
        }
        .map_err(|e| ErreurTexte::Police(format!("{id} : {}", e.message())))?;
        donnees.push(ttf);
        unsafe { builder.AddFontFile(&reference) }
            .map_err(|e| ErreurTexte::Police(format!("{id} (ajout) : {}", e.message())))?;
    }
    let set = unsafe { builder.CreateFontSet() }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateFontSet : {}", e.message())))?;
    let collection = unsafe { dwrite.CreateFontCollectionFromFontSet(&set) }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateFontCollectionFromFontSet : {}", e.message())))?;
    Ok((collection, donnees))
}

/// 16 directions sur un cercle de rayon `r` (contour par passes décalées).
fn couronne(r: f32) -> Vec<(f32, f32)> {
    (0..16)
        .map(|i| {
            let angle = std::f32::consts::TAU * i as f32 / 16.0;
            (angle.cos() * r, angle.sin() * r)
        })
        .collect()
}

fn couleur_d2d(rgba: [u8; 4]) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: rgba[0] as f32 / 255.0,
        g: rgba[1] as f32 / 255.0,
        b: rgba[2] as f32 / 255.0,
        a: rgba[3] as f32 / 255.0,
    }
}

/// Octets bruts d'une valeur POD — pour `ID2D1Properties::SetValue`
/// (propriétés d'effets : float, u32, Vector2/3 de f32, matrice 5×4).
fn octets<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((v as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}

/// Rect D2D d'un `RECT`, `None` s'il est vide (rien à peindre).
fn rect_visible(rect: RECT) -> Option<D2D_RECT_F> {
    (rect.right > rect.left && rect.bottom > rect.top).then_some(D2D_RECT_F {
        left: rect.left as f32,
        top: rect.top as f32,
        right: rect.right as f32,
        bottom: rect.bottom as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texte(position: PositionTitre, cible: CibleTitre) -> TexteCfg {
        TexteCfg {
            contenu: "Bienvenue".to_string(),
            position,
            cible,
            cadre: (0.0, 0.0, 1920.0, 1080.0),
            style: StyleTexte::default(),
            z: 0,
        }
    }

    #[test]
    fn toutes_les_polices_du_frontend_sont_embarquees() {
        assert_eq!(POLICES.len(), 20);
        for id in [
            "BebasNeue", "Anton", "Oswald", "Pacifico", "PermanentMarker", "Bungee",
            "RussoOne", "PressStart2P", "Orbitron", "Montserrat", "Righteous", "Audiowide",
            "Lobster", "Caveat", "Satisfy", "Teko", "Rajdhani", "BlackOpsOne", "Creepster",
            "Fredoka",
        ] {
            assert!(famille_police(id).is_some(), "police absente : {id}");
        }
        assert!(famille_police("Inconnue").is_none());
        assert_eq!(famille_police(POLICE_DEFAUT), Some("Bebas Neue"));
    }

    #[test]
    fn les_woff2_embarques_se_decodent() {
        for (id, _, woff2) in POLICES {
            let ttf = wuff::decompress_woff2(woff2)
                .unwrap_or_else(|e| panic!("{id} : {e}"));
            assert!(ttf.len() > woff2.len(), "{id} : TTF plus petit que le woff2");
            assert_eq!(&ttf[..4], &[0x00, 0x01, 0x00, 0x00], "{id} : signature sfnt");
        }
    }

    #[test]
    fn titre_fond_haut_centre_et_decale_de_8_px() {
        let p = placement(&texte(PositionTitre::Haut, CibleTitre::Fond), 200.0, 30.0);
        assert!(p.rotation.is_none());
        assert_eq!(p.rect.left, (1920.0 - 200.0) / 2.0);
        assert_eq!(p.rect.top, MARGE_TITRE);
        assert_eq!(p.rect.right - p.rect.left, 200.0);
    }

    #[test]
    fn titre_fond_bas_colle_au_bord_interieur() {
        let p = placement(&texte(PositionTitre::Bas, CibleTitre::Fond), 200.0, 30.0);
        assert_eq!(p.rect.bottom, 1080.0 - MARGE_TITRE);
    }

    #[test]
    fn titre_fond_gauche_est_vertical_et_dans_le_canvas() {
        let p = placement(&texte(PositionTitre::Gauche, CibleTitre::Fond), 200.0, 30.0);
        let centre = p.rotation.expect("titre latéral tourné");
        // La boîte tournée fait `th × tw` : bord gauche à 8 px du canvas,
        // hauteur = longueur du texte, centrée verticalement.
        assert_eq!(centre.x - 15.0, MARGE_TITRE);
        assert_eq!(centre.y, (1080.0 - 30.0) / 2.0 + 100.0);
        // La texture non tournée reste `tw × th`, centrée sur ce point.
        assert_eq!(p.rect.right - p.rect.left, 200.0);
        assert_eq!(p.rect.bottom - p.rect.top, 30.0);
    }

    #[test]
    fn titre_widget_haut_est_au_dessus_du_widget() {
        let mut t = texte(PositionTitre::Haut, CibleTitre::Widget);
        t.cadre = (100.0, 200.0, 400.0, 300.0);
        let p = placement(&t, 150.0, 24.0);
        // `margin-top: -2px` du DOM : le bas de la boîte est 2 px au-dessus.
        assert_eq!(p.rect.bottom, 200.0 - MARGE_TITRE_WIDGET);
        assert_eq!(p.rect.left, 100.0 + (400.0 - 150.0) / 2.0);
    }

    #[test]
    fn titre_widget_droite_est_a_droite_du_widget() {
        let mut t = texte(PositionTitre::Droite, CibleTitre::Widget);
        t.cadre = (100.0, 200.0, 400.0, 300.0);
        let p = placement(&t, 150.0, 24.0);
        let centre = p.rotation.expect("titre latéral tourné");
        // Boîte tournée de 24 px de large, à 2 px du bord droit du widget
        // (`margin-left: 2px`).
        assert_eq!(centre.x, 500.0 + MARGE_TITRE_WIDGET + 12.0);
        assert_eq!(centre.y, 200.0 + (300.0 - 24.0) / 2.0 + 75.0);
    }

    #[test]
    fn titre_widget_bas_et_gauche_portent_la_marge_de_2_px() {
        let mut bas = texte(PositionTitre::Bas, CibleTitre::Widget);
        bas.cadre = (100.0, 200.0, 400.0, 300.0);
        let p = placement(&bas, 150.0, 24.0);
        assert_eq!(p.rect.top, 500.0 + MARGE_TITRE_WIDGET);

        let mut gauche = texte(PositionTitre::Gauche, CibleTitre::Widget);
        gauche.cadre = (100.0, 200.0, 400.0, 300.0);
        let p = placement(&gauche, 150.0, 24.0);
        let centre = p.rotation.expect("titre latéral tourné");
        // Bord droit de la boîte tournée = `x - 2`.
        assert_eq!(centre.x + 12.0, 100.0 - MARGE_TITRE_WIDGET);
    }

    #[test]
    fn interligne_des_titres_est_le_line_height_du_dom() {
        // `.widget-titre` / `.bg-titre` : `line-height: 1.1`.
        assert_eq!(INTERLIGNE_TITRE, 1.1);
        assert!((INTERLIGNE_TITRE * 24.0 - 26.4).abs() < 1e-4);
    }

    /// Le contour exact n'est possible que sans police simulée : une famille
    /// embarquée n'a qu'une graisse, donc `titreGras` retombe sur
    /// l'approximation par passes décalées. Test GPU : ignoré sans device.
    #[test]
    fn contour_exact_sauf_police_simulee() {
        let Ok(contexte) = ContexteGpu::creer(None) else {
            eprintln!("texte : pas de device D3D11, test ignoré");
            return;
        };
        let canvas = contexte
            .creer_texture(&DescTexture::new(Format::Bgra8, 64, 64).expect("desc"))
            .expect("canvas");
        let mut dessinateur = DessinateurTexte::nouveau(&contexte, &canvas).expect("dessinateur");
        let contenu: Vec<u16> = "Aa".encode_utf16().collect();
        let layout_pour = |d: &mut DessinateurTexte, gras: bool| {
            let style = StyleTexte {
                taille: 32.0,
                gras,
                ..StyleTexte::default()
            };
            let format = d.format(&style).expect("format");
            unsafe { d.dwrite.CreateTextLayout(&contenu, &format, 512.0, 256.0) }.expect("layout")
        };

        let normal = layout_pour(&mut dessinateur, false);
        let contours = dessinateur.contours_glyphes(&normal).expect("contours");
        let contours = contours.expect("une police réelle doit donner un tracé exact");
        assert!(!contours.is_empty(), "au moins un run");

        let gras = layout_pour(&mut dessinateur, true);
        assert!(
            dessinateur.contours_glyphes(&gras).expect("contours").is_none(),
            "une police simulée (gras synthétique) doit retomber sur l'approximation"
        );
    }

    /// La boîte mesurée d'un titre est celle du CSS (largeur du texte ×
    /// `line-height: 1.1`), pas la hauteur naturelle de la police — sinon le
    /// placement dérive avec `titreTaille`. Test GPU : ignoré sans device.
    #[test]
    fn titre_mesure_la_boite_de_ligne_css() {
        let Ok(contexte) = ContexteGpu::creer(None) else {
            eprintln!("texte : pas de device D3D11, test ignoré");
            return;
        };
        let canvas = contexte
            .creer_texture(&DescTexture::new(Format::Bgra8, 64, 64).expect("desc"))
            .expect("canvas");
        let mut dessinateur = DessinateurTexte::nouveau(&contexte, &canvas).expect("dessinateur");
        for taille in [24.0_f32, 40.0, 96.0] {
            let mut t = texte(PositionTitre::Haut, CibleTitre::Widget);
            t.style.taille = taille;
            let cache = dessinateur.rendre(&t).expect("rendu du titre");
            assert!(
                (cache.hauteur - INTERLIGNE_TITRE * taille).abs() < 0.01,
                "taille {taille} : hauteur {} au lieu de {}",
                cache.hauteur,
                INTERLIGNE_TITRE * taille
            );
            // La texture garde une marge pour le contour (boîte CSS intacte).
            assert!(cache.marge.0 >= 1.0 && cache.marge.1 >= cache.marge.0);
        }
    }

    #[test]
    fn cle_de_cache_ignore_la_position() {
        let a = texte(PositionTitre::Haut, CibleTitre::Fond);
        let mut b = texte(PositionTitre::Bas, CibleTitre::Widget);
        b.cadre = (10.0, 20.0, 30.0, 40.0);
        assert_eq!(a.cle(), b.cle());
        b.style.taille = 48.0;
        assert_ne!(a.cle(), b.cle());
    }

    #[test]
    fn couronne_est_bornee_au_rayon() {
        let points = couronne(2.0);
        assert_eq!(points.len(), 16);
        for (dx, dy) in points {
            let d = (dx * dx + dy * dy).sqrt();
            assert!((d - 2.0).abs() < 0.01, "rayon {d}");
        }
    }

}
