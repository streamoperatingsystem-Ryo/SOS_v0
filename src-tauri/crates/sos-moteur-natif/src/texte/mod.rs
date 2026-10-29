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
//! Limite assumée de la tranche : le **contour** est une approximation par
//! passes décalées (16 directions) et non un vrai trait de géométrie
//! (`IDWriteFontFace::GetGlyphRunOutline`). Le rendu est correct à la taille
//! canvas mais pas identique au pixel près à `-webkit-text-stroke`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;

use windows::core::{Interface, IUnknown, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FILL_MODE_ALTERNATE,
    D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Brush, ID2D1Device, ID2D1DeviceContext, ID2D1Effect,
    ID2D1Factory1, ID2D1Geometry, ID2D1GeometrySink, ID2D1GradientStopCollection,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush,
    ID2D1StrokeStyle, CLSID_D2D13DPerspectiveTransform,
    D2D1_3DPERSPECTIVETRANSFORM_INTERPOLATION_MODE_LINEAR,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_BORDER_MODE, D2D1_3DPERSPECTIVETRANSFORM_PROP_DEPTH,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_INTERPOLATION_MODE,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_PERSPECTIVE_ORIGIN,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_ROTATION,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_ROTATION_ORIGIN,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_PROPERTY_TYPE_UNKNOWN, D2D1_ROUNDED_RECT, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1_UNIT_MODE_PIXELS,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BORDER_MODE_SOFT, D2D1_COMPOSITE_MODE_SOURCE_OVER,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteFontCollection1,
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS,
    DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::cadres::{CadreCfg, ErreurCadre};
use crate::chat::{ChatCfg, FileChat};
use crate::media::PerspCfg;
use crate::gpu::{ContexteGpu, DescTexture, Format};
use crate::input::{
    bouton_physique, resoudre_dpad, FileInput, Forme, InputCfg, LienManette, ModeInput,
    Primitive, Segment, SkinManette, StyleLabel, StyleManette,
};

pub mod media;

pub use media::{TraceMedia, TraceMorphe, TracePixel};

/// Écart aux bords, en px canvas — valeur de `diffusion.html` (`.bg-titre`).
pub const MARGE_TITRE: f32 = 8.0;

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
/// fond = 8 px **à l'intérieur** du rect de l'hôte, widget = **hors** du rect.
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
        (CibleTitre::Widget, PositionTitre::Haut) => (centre_x, y - th, false),
        (CibleTitre::Widget, PositionTitre::Bas) => (centre_x, y + h, false),
        (CibleTitre::Widget, PositionTitre::Gauche) => (x - th, centre_y, true),
        (CibleTitre::Widget, PositionTitre::Droite) => (x + w, centre_y, true),
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
    largeur: f32,
    hauteur: f32,
}

/// Dessinateur du canvas BGRA — nom historique : il porte désormais les
/// **titres**, les **cadres** (`cadres::dessiner`), les **médias découpés au
/// polygone** et le **chat** multi-lignes.
///
/// Un seul `ID2D1DeviceContext` et une seule cible : chaque appel de dessin
/// est encadré par `debut`/`fin` **par le moteur**, qui peut ainsi insérer des
/// `VideoProcessorBlt` bornés entre deux segments (ordre z exact).
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
    /// Trait des cadres (miter, limite 4) — créé une fois.
    style_trait: ID2D1StrokeStyle,
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

/// Un plan à perspective (`perspective(d) rotateX rotateY`, diffusion.html) :
/// les calques du plan (voile/média/capture/bordure/cadre d'un widget, ou le
/// média de fond) sont d'abord rasterisés dans `bitmap` — taille canvas,
/// coords canvas comme le DOM — puis l'effet `3DPerspectiveTransform`
/// projette le tout sur le canvas. `DrawBitmap` ne sait pas faire de
/// perspective ; la texture intermédiaire est incontournable.
struct PlanPersp {
    cfg: PerspCfg,
    bitmap: ID2D1Bitmap1,
    effet: ID2D1Effect,
    /// Texture porteuse de `bitmap` (la bitmap ne la possède pas).
    _texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    /// Contenu peint cette frame : un plan vierge n'est pas projeté.
    dirty: bool,
}

/// Un message mis en page : layout DirectWrite + hauteur rendue.
struct MessageRendu {
    layout: IDWriteTextLayout,
    hauteur: f32,
    /// `nth-child` impair (index pair) — zébrage de `diffusion.html`.
    pair: bool,
    /// Brosse de la couleur du pseudo : le layout la référence.
    _brosse: Option<ID2D1SolidColorBrush>,
}

/// Bloc de chat complet (tous les messages d'un widget), mis en page une fois
/// par version du buffer.
///
/// La clé de cache ne contient **pas** la version : le bloc est réutilisé
/// tant que la configuration du widget ne change pas, et reconstruit quand
/// `version` diffère. La table reste donc bornée par le nombre de widgets.
struct BlocChat {
    /// Version du buffer au moment de la mise en page.
    version: u64,
    /// Le layout DirectWrite ne possède pas le format : il doit vivre ici.
    _format: IDWriteTextFormat,
    messages: Vec<MessageRendu>,
    /// Hauteur totale, interlignes de bulles comprises.
    total: f32,
}

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
        let style_trait = crate::cadres::style_trait(&d2d)
            .map_err(|e| ErreurTexte::Systeme(e.to_string()))?;
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
            style_trait,
            cache: HashMap::new(),
            bitmaps: HashMap::new(),
            blocs: HashMap::new(),
            formats_input: HashMap::new(),
            persps: Vec::new(),
            effets: HashMap::new(),
            petits: HashMap::new(),
            warp: None,
            morphes: HashMap::new(),
            sorties_morph: HashMap::new(),
        })
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
                    Some(&place.rect),
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

    /// Peint un cadre (clip au polygone, 2 styles) — dans une passe ouverte.
    pub fn dessiner_cadre(
        &mut self,
        cfg: &CadreCfg,
        geo: &ID2D1PathGeometry,
    ) -> Result<(), ErreurTexte> {
        crate::cadres::dessiner(&self.d2d, &self.ctx, &self.cible_rt, geo, cfg, &self.style_trait)
            .map_err(|e: ErreurCadre| ErreurTexte::Systeme(e.to_string()))
    }

    /// Crée l'offscreen + l'effet de projection de chaque plan à
    /// perspective (taille du canvas : les calques y dessinent en coords
    /// canvas, comme le DOM — l'effet projette ensuite le plan entier).
    pub fn configurer_persps(
        &mut self,
        contexte: &ContexteGpu,
        largeur: u32,
        hauteur: u32,
        persps: &[PerspCfg],
    ) -> Result<(), ErreurTexte> {
        self.persps.clear();
        for cfg in persps {
            let desc = DescTexture::new(Format::Bgra8, largeur, hauteur)
                .map_err(|e| ErreurTexte::Gpu(e.to_string()))?;
            let texture = contexte
                .creer_texture_echantillonnable(&desc)
                .map_err(|e| ErreurTexte::Gpu(format!("offscreen persp : {e}")))?;
            let surface: IDXGISurface = texture
                .cast()
                .map_err(|e| ErreurTexte::Gpu(format!("IDXGISurface persp : {}", e.message())))?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    // Le plan garde son alpha : les zones vides de
                    // l'offscreen ne recouvrent pas le canvas.
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                colorContext: ManuallyDrop::new(None),
            };
            let bitmap = unsafe { self.ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
                .map_err(|e| ErreurTexte::Gpu(format!("bitmap persp : {}", e.message())))?;
            let effet = unsafe { self.ctx.CreateEffect(&CLSID_D2D13DPerspectiveTransform) }
                .map_err(|e| ErreurTexte::Gpu(format!("effet persp : {}", e.message())))?;
            unsafe { effet.SetInput(0, &bitmap, true) };
            let (cx, cy) = (
                (cfg.rect.0 + cfg.rect.2 / 2.0) as f32,
                (cfg.rect.1 + cfg.rect.3 / 2.0) as f32,
            );
            // Même sémantique que `perspective(d) rotateX() rotateY()` avec
            // `transform-origin: center` : rotation en degrés, origines au
            // centre de la boîte, profondeur = la constante CSS.
            // `D2D1_PROPERTY_TYPE_UNKNOWN` = « type déjà déclaré par l'effet » :
            // les enums s'enregistrent en `ENUM`, pas `UINT32` (E_INVALIDARG).
            let interpolation = D2D1_3DPERSPECTIVETRANSFORM_INTERPOLATION_MODE_LINEAR.0 as u32;
            let bordure = D2D1_BORDER_MODE_SOFT.0 as u32;
            let profondeur = cfg.profondeur as f32;
            let origine_p = [cx, cy];
            let origine_r = [cx, cy, 0.0];
            let rotation = [cfg.rot_x as f32, cfg.rot_y as f32, 0.0];
            let props: [(u32, &[u8]); 6] = [
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_INTERPOLATION_MODE.0 as u32,
                    octets(&interpolation),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_BORDER_MODE.0 as u32,
                    octets(&bordure),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_DEPTH.0 as u32,
                    octets(&profondeur),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_PERSPECTIVE_ORIGIN.0 as u32,
                    octets(&origine_p),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_ROTATION_ORIGIN.0 as u32,
                    octets(&origine_r),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_ROTATION.0 as u32,
                    octets(&rotation),
                ),
            ];
            for (idx, data) in props {
                unsafe { effet.SetValue(idx, D2D1_PROPERTY_TYPE_UNKNOWN, data) }.map_err(|e| {
                    ErreurTexte::Gpu(format!("propriété persp {idx} : {}", e.message()))
                })?;
            }
            self.persps.push(PlanPersp {
                cfg: cfg.clone(),
                bitmap,
                effet,
                _texture: texture,
                dirty: false,
            });
        }
        Ok(())
    }

    /// Efface chaque offscreen de perspective en transparent — début de
    /// frame, puisque chaque plan est re-rasterisé entièrement.
    pub fn effacer_persps(&mut self) {
        if self.persps.is_empty() {
            return;
        }
        for p in &mut self.persps {
            unsafe { self.ctx.SetTarget(&p.bitmap) };
            unsafe { self.cible_rt.Clear(Some(&couleur_d2d([0, 0, 0, 0]))) };
            p.dirty = false;
        }
        unsafe { self.ctx.SetTarget(&self.cible_bitmap) };
    }

    /// Plan à perspective couvrant (`rect`, `z`) — clé de regroupement des
    /// calques d'un widget (tous partagent son rect et son z, voir
    /// `persps_depuis_scene`). `None` = dessiner sur le canvas.
    pub fn groupe_de(&self, rect: (f64, f64, f64, f64), z: i32) -> Option<usize> {
        self.persps
            .iter()
            .position(|p| p.cfg.z == z && p.cfg.rect == rect)
    }

    /// Redirige la cible de dessin : offscreen du plan, ou canvas.
    ///
    /// `SetTarget` en cours de passe est légal sur `ID2D1DeviceContext` ;
    /// les clips/transforms des `dessiner_*` sont équilibrés avant retour,
    /// le basculement entre deux calques est donc sûr.
    pub fn cible_pour(&mut self, groupe: Option<usize>) {
        match groupe {
            Some(g) => {
                let p = &mut self.persps[g];
                p.dirty = true;
                unsafe { self.ctx.SetTarget(&p.bitmap) };
            }
            None => unsafe { self.ctx.SetTarget(&self.cible_bitmap) },
        }
    }

    /// Projette sur le canvas les plans dépassés (`cfg.z < z`) qui ont du
    /// contenu — le plan transformé se pose à son niveau de z, comme le
    /// `.widget` DOM dans son stacking context.
    pub fn flusher_persps(&mut self, z: i32) -> Result<(), ErreurTexte> {
        // `persps` est trié par z à la configuration — l'ordre de balayage
        // est déjà l'ordre de projection.
        for g in 0..self.persps.len() {
            if self.persps[g].dirty && self.persps[g].cfg.z < z {
                self.peindre_persp(g)?;
            }
        }
        Ok(())
    }

    /// Projette tous les plans restants — fin du plan de composition.
    pub fn flusher_persps_fin(&mut self) -> Result<(), ErreurTexte> {
        self.flusher_persps(i32::MAX)
    }

    /// Projette un plan sur le canvas : sortie de l'effet
    /// `3DPerspectiveTransform`, puis `scale(bgZ)` éventuel appliqué en
    /// espace écran autour du centre (`scale` en premier sur `#bg`).
    fn peindre_persp(&mut self, g: usize) -> Result<(), ErreurTexte> {
        unsafe { self.ctx.SetTarget(&self.cible_bitmap) };
        let p = &self.persps[g];
        let ze = p.cfg.zoom_ecran as f32;
        let (cx, cy) = (
            (p.cfg.rect.0 + p.cfg.rect.2 / 2.0) as f32,
            (p.cfg.rect.1 + p.cfg.rect.3 / 2.0) as f32,
        );
        let image = unsafe { p.effet.GetOutput() }
            .map_err(|e| ErreurTexte::Gpu(format!("sortie effet persp : {}", e.message())))?;
        unsafe {
            let mut avant = Matrix3x2::identity();
            if ze != 1.0 {
                self.cible_rt.GetTransform(&mut avant);
                let t_mc = Matrix3x2::translation(-cx, -cy);
                let t_c = Matrix3x2::translation(cx, cy);
                let z = Matrix3x2 {
                    M11: ze,
                    M12: 0.0,
                    M21: 0.0,
                    M22: ze,
                    M31: 0.0,
                    M32: 0.0,
                };
                self.cible_rt.SetTransform(&(t_mc * z * t_c * avant));
            }
            self.ctx.DrawImage(
                &image,
                None,
                None,
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            if ze != 1.0 {
                self.cible_rt.SetTransform(&avant);
            }
        }
        self.persps[g].dirty = false;
        Ok(())
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

    /// Dessine les widgets chat — dans une passe ouverte.
    ///
    /// Chaque widget a son bloc mis en page (cache par version du buffer) :
    /// un nouveau message ne reconfigure **rien**, il invalide le bloc.
    /// Le plus récent est en bas, on pousse vers le haut ; ce qui dépasse est
    /// rogné au rect du widget (défilement automatique, pas de scroll manuel).
    pub fn dessiner_chat(&mut self, cfgs: &[ChatCfg], file: &FileChat) -> Result<(), ErreurTexte> {
        if cfgs.is_empty() || file.version() == 0 {
            return Ok(());
        }
        let blanc = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d([255, 255, 255, 255]), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse chat : {}", e.message())))?;
        let clair = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.07,
        };
        let fond = unsafe { self.cible_rt.CreateSolidColorBrush(&clair, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse fond chat : {}", e.message())))?;
        let noir = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d([0, 0, 0, 255]), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse fond chat : {}", e.message())))?;

        for cfg in cfgs {
            let g = geometrie_chat(cfg);
            let cle = cle_bloc(cfg, &g);
            // `is_none_or` est 1.82 : MSRV du dépôt = 1.77.
            let perime = match self.blocs.get(&cle) {
                Some(bloc) => bloc.version != file.version(),
                None => true,
            };
            if perime {
                self.construire_bloc(cfg, file, &g, cle)?;
            }
            let Some(bloc) = self.blocs.get(&cle) else {
                continue;
            };
            let (x, y, w, h) = cfg.rect;
            let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
            let clip = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            unsafe {
                self.cible_rt
                    .PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };

            let mut curseur = y + h - g.pad_bas - bloc.total;
            for message in &bloc.messages {
                let rect_fond = D2D_RECT_F {
                    left: x + g.pad_x,
                    top: curseur,
                    right: x + w - g.pad_x,
                    bottom: curseur + message.hauteur,
                };
                // Bulles : fond uniforme clair (zébrage off, cf. CSS) ;
                // compact : une ligne sur deux en noir plein.
                let brosse = if cfg.bulles || message.pair {
                    &fond
                } else {
                    &noir
                };
                unsafe { self.cible_rt.FillRectangle(&rect_fond, brosse) };
                let origine = D2D_POINT_2F {
                    x: x + g.pad_x + g.marge_gauche,
                    y: curseur + g.marge_haut,
                };
                unsafe {
                    self.cible_rt.DrawTextLayout(
                        origine,
                        &message.layout,
                        &blanc,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
                curseur += message.hauteur + g.gap;
            }
            unsafe { self.cible_rt.PopAxisAlignedClip() };
        }
        Ok(())
    }

    /// Dessine un widget input-viewer (clavier / numpad / souris / manette) —
    /// dans une passe.
    ///
    /// Le contenu est rogné au rect du widget (`overflow: hidden`) **et** au
    /// polygone de son cadre (`clip-path`), comme le CSS.
    pub fn dessiner_input(
        &mut self,
        cfg: &InputCfg,
        etat: &FileInput,
        clip: Option<&ID2D1PathGeometry>,
    ) -> Result<(), ErreurTexte> {
        let geometrie = crate::input::disposition(cfg);
        if geometrie.chassis.is_none()
            && geometrie.touches.is_empty()
            && geometrie.souris.is_none()
            && geometrie.manette.is_none()
        {
            return Ok(());
        }
        let (x, y, w, h) = cfg.rect;
        let zone = D2D_RECT_F {
            left: x as f32,
            top: y as f32,
            right: (x + w) as f32,
            bottom: (y + h) as f32,
        };
        unsafe {
            self.cible_rt
                .PushAxisAlignedClip(&zone, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
        };
        let masque: Option<ID2D1Geometry> = match clip {
            Some(geo) => Some(
                geo.cast()
                    .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))?,
            ),
            None => None,
        };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: zone,
            geometricMask: ManuallyDrop::new(masque),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let avec_couche = params.geometricMask.is_some();
        if avec_couche {
            let layer = unsafe { self.cible_rt.CreateLayer(None) }
                .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer input : {}", e.message())))?;
            unsafe { self.ctx.PushLayer(&params, &layer) };
        }
        let resultat = self.dessiner_input_interne(cfg, etat, &geometrie);
        if avec_couche {
            unsafe { self.ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        resultat
    }

    fn dessiner_input_interne(
        &mut self,
        cfg: &InputCfg,
        etat: &FileInput,
        geometrie: &crate::input::GeometrieInput,
    ) -> Result<(), ErreurTexte> {
        let largeur = cfg.rect.2 as f32;
        let cqw = largeur / 100.0;
        if let Some(chassis) = &geometrie.chassis {
            self.dessiner_chassis(cfg, chassis, cqw, etat)?;
        }
        for touche in &geometrie.touches {
            self.dessiner_touche(cfg, cqw, touche, etat)?;
        }
        if let Some(souris) = &geometrie.souris {
            self.dessiner_souris(cfg, etat, souris)?;
        }
        if let Some(manette) = &geometrie.manette {
            self.dessiner_manette(cfg, etat, manette)?;
        }
        Ok(())
    }

    /// Souris (`ivGenererSourisHTML`) : corps, zones de clic, molette crantée,
    /// bouton DPI, reflet, coordonnées.
    ///
    /// `fill: var(--inputviewer-couleur-presse)` du CSS remplace le dégradé
    /// des zones/molette par la couleur de presse **unie** ; le
    /// `drop-shadow(0 0 12px)` est approché par un liseré large translucide
    /// sous la forme (même convention que le halo des LEDs).
    fn dessiner_souris(
        &mut self,
        cfg: &InputCfg,
        etat: &FileInput,
        s: &crate::input::SourisGeometrie,
    ) -> Result<(), ErreurTexte> {
        let e = s.echelle;
        let etat_s = etat.souris();
        let presse = couleur_d2d(cfg.couleur);
        let halo_presse = D2D1_COLOR_F {
            a: 0.3,
            ..presse
        };

        // Corps translucide (fill-opacity 0.62) — le média transparaît.
        let corps = self.chemin_d2d(&s.corps)?;
        let (cx, y0, _, y1) = bornes_chemin(&s.corps);
        let brosse_corps = self.brosse_degrade_stops(
            &[
                (0.0, couleur_d2d_alpha([0x3a, 0x3f, 0x4d, 255], 0.62)),
                (0.5, couleur_d2d_alpha([0x26, 0x2a, 0x33, 255], 0.62)),
                (1.0, couleur_d2d_alpha([0x14, 0x16, 0x1c, 255], 0.62)),
            ],
            (cx, y0),
            (cx, y1),
        )?;
        unsafe { self.cible_rt.FillGeometry(&corps, &brosse_corps, None) };
        let filet = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.12,
        })?;
        unsafe {
            self.cible_rt
                .DrawGeometry(&corps, &filet, (2.0 * e).max(0.5), None)
        };

        // Zones de clic gauche/droite (opaques, dégradé `ivs-btn`).
        for (chemin, enfoncee) in [
            (&s.zone_gauche, etat_s.gauche),
            (&s.zone_droite, etat_s.droite),
        ] {
            let geo = self.chemin_d2d(chemin)?;
            let (zx, z0, _, z1) = bornes_chemin(chemin);
            if enfoncee {
                let halo = self.brosse_unie(halo_presse)?;
                unsafe {
                    self.cible_rt
                        .DrawGeometry(&geo, &halo, (6.0 * e).max(1.0), None)
                };
            }
            let remplissage = if enfoncee {
                self.brosse_unie(presse)?
            } else {
                self.brosse_degrade_stops(
                    &[
                        (0.0, couleur_d2d([0x48, 0x4e, 0x5c, 255])),
                        (1.0, couleur_d2d([0x22, 0x25, 0x2d, 255])),
                    ],
                    (zx, z0),
                    (zx, z1),
                )?
            };
            unsafe { self.cible_rt.FillGeometry(&geo, &remplissage, None) };
            let contour = self.brosse_unie(if enfoncee {
                presse
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.10,
                }
            })?;
            unsafe {
                self.cible_rt
                    .DrawGeometry(&geo, &contour, (1.5 * e).max(0.4), None)
            };
        }

        // Coutures (centrale + séparation sous les clics).
        let couture = self.brosse_unie(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.4,
        })?;
        unsafe {
            self.cible_rt.DrawLine(
                pt(s.couture_verticale.0),
                pt(s.couture_verticale.1),
                &couture,
                (2.0 * e).max(0.5),
                None,
            );
            self.cible_rt.DrawLine(
                pt(s.couture_horizontale.0),
                pt(s.couture_horizontale.1),
                &couture,
                (2.0 * e).max(0.5),
                None,
            );
        }

        // Logement de la molette.
        let loge = rounded(s.loge, 22.0 * e);
        let noir = self.brosse_unie(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.25,
        })?;
        unsafe { self.cible_rt.FillRoundedRectangle(&loge, &noir) };

        // Molette (clic milieu) — crantée.
        if etat_s.milieu {
            let halo = self.brosse_unie(halo_presse)?;
            unsafe {
                self.cible_rt
                    .DrawRoundedRectangle(&rounded(s.molette, 14.0 * e), &halo, 6.0 * e, None)
            };
        }
        let molette = rounded(s.molette, 14.0 * e);
        let brosse_molette = if etat_s.milieu {
            self.brosse_unie(presse)?
        } else {
            self.brosse_degrade_stops(
                &[
                    (0.0, couleur_d2d([0x56, 0x5e, 0x6e, 255])),
                    (1.0, couleur_d2d([0x2a, 0x2e, 0x37, 255])),
                ],
                (s.molette.x, s.molette.y),
                (s.molette.x, s.molette.y + s.molette.hauteur),
            )?
        };
        unsafe { self.cible_rt.FillRoundedRectangle(&molette, &brosse_molette) };
        let bord_molette = self.brosse_unie(if etat_s.milieu {
            presse
        } else {
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.18,
            }
        })?;
        unsafe {
            self.cible_rt
                .DrawRoundedRectangle(&molette, &bord_molette, (1.5 * e).max(0.4), None)
        };
        // Les crans ne sont PAS dans le sélecteur `.presse` du CSS : toujours
        // noirs translucides.
        let cran = self.brosse_unie(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.35,
        })?;
        for (a, b) in &s.crans {
            unsafe {
                self.cible_rt
                    .DrawLine(pt(*a), pt(*b), &cran, (2.0 * e).max(0.5), None)
            };
        }

        // Bouton DPI.
        let dpi = D2D1_ELLIPSE {
            point: pt(s.dpi.0),
            radiusX: s.dpi.1,
            radiusY: s.dpi.1,
        };
        let remplissage_dpi = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.08,
        })?;
        unsafe { self.cible_rt.FillEllipse(&dpi, &remplissage_dpi) };
        let bord_dpi = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.10,
        })?;
        unsafe {
            self.cible_rt
                .DrawEllipse(&dpi, &bord_dpi, (1.0 * e).max(0.3), None)
        };

        // Reflet sur la coque (ellipse tournée de -16° autour de son centre).
        let mut avant = Matrix3x2::identity();
        unsafe { self.cible_rt.GetTransform(&mut avant) };
        let rotation = Matrix3x2::rotation(
            s.reflet.angle.to_radians(),
            s.reflet.centre.x,
            s.reflet.centre.y,
        );
        unsafe { self.cible_rt.SetTransform(&(rotation * avant)) };
        let reflet = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.12,
        })?;
        unsafe {
            self.cible_rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: pt(s.reflet.centre),
                    radiusX: s.reflet.rayon_x,
                    radiusY: s.reflet.rayon_y,
                },
                &reflet,
            )
        };
        unsafe { self.cible_rt.SetTransform(&avant) };

        // Coordonnées curseur (`X: … · Y: …`, ancre médiane, taille 16 viewBox).
        let (ancre, taille) = s.coordonnees;
        let format = self.format_input(taille, true)?;
        let texte: Vec<u16> = format!("X: {} · Y: {}", etat_s.x, etat_s.y)
            .encode_utf16()
            .collect();
        let rect = D2D_RECT_F {
            left: ancre.x - 80.0 * e,
            top: ancre.y - 0.95 * taille,
            right: ancre.x + 80.0 * e,
            bottom: ancre.y + 0.25 * taille,
        };
        let encre = self.brosse_unie(couleur_d2d([0xae, 0xb4, 0xc2, 255]))?;
        unsafe {
            self.cible_rt.DrawText(
                &texte,
                &format,
                &rect,
                &encre,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
        Ok(())
    }

    /// Manette (`ivGenererManette*SVG`) : peint les `groupes` dans l'ordre du
    /// document SVG et résout leur état via le `lien`.
    ///
    /// Presse : couleur unie `cfg.couleur` + liseré-halo translucide dessiné
    /// **sous** la forme (approche du `drop-shadow` CSS, même convention que
    /// les zones de la souris). `connectee` n'a pas de rendu dédié :
    /// `majInputViewerEtat` ne le lit pas non plus — une manette débranchée
    /// n'est que « tout relâché ».
    fn dessiner_manette(
        &mut self,
        cfg: &InputCfg,
        etat: &FileInput,
        m: &crate::input::ManetteGeometrie,
    ) -> Result<(), ErreurTexte> {
        let e = m.echelle;
        let etat_m = etat.manette();
        let mapping = cfg.mapping.as_ref();
        let dpad = resoudre_dpad(&etat_m, m.skin, mapping);
        let presse = PresseManette {
            allume: false,
            couleur: couleur_d2d(cfg.couleur),
            halo: D2D1_COLOR_F {
                a: 0.3,
                ..couleur_d2d(cfg.couleur)
            },
        };

        for groupe in &m.groupes {
            let (allume, deplacement) = match groupe.lien {
                LienManette::Statique => (false, (0.0, 0.0)),
                LienManette::Bouton(idx) => {
                    let phys = bouton_physique(idx, mapping);
                    (etat_m.boutons.get(phys).copied().unwrap_or(false), (0.0, 0.0))
                }
                LienManette::Dpad(dir) => (dir.activee(dpad), (0.0, 0.0)),
                LienManette::Stick { gauche, bouton } => {
                    let (ax, ay) = if gauche {
                        (etat_m.axes[0], etat_m.axes[1])
                    } else {
                        (etat_m.axes[2], etat_m.axes[3])
                    };
                    let phys = bouton_physique(bouton, mapping);
                    let enfonce = etat_m.boutons.get(phys).copied().unwrap_or(false);
                    // `.actif` : le cap s'éclaire aussi quand le stick bouge
                    // (`translate(ax*26, ay*26)` px viewBox, seuil 0,15).
                    let actif = ax.abs() > 0.15 || ay.abs() > 0.15;
                    (enfonce || actif, (ax * 26.0 * e, ay * 26.0 * e))
                }
            };

            let deplace = deplacement != (0.0, 0.0);
            let mut avant = Matrix3x2::identity();
            if deplace {
                unsafe { self.cible_rt.GetTransform(&mut avant) };
                let t = Matrix3x2::translation(deplacement.0, deplacement.1);
                unsafe { self.cible_rt.SetTransform(&(t * avant)) };
            }
            let resultat = self.dessiner_primitives_manette(
                groupe,
                m.skin,
                PresseManette { allume, ..presse },
                e,
            );
            if deplace {
                unsafe { self.cible_rt.SetTransform(&avant) };
            }
            resultat?;
        }
        Ok(())
    }

    /// Primitives d'un `<g>` manette — erreurs propagées, transform déjà posé.
    fn dessiner_primitives_manette(
        &mut self,
        groupe: &crate::input::GroupeManette,
        skin: SkinManette,
        presse: PresseManette,
        e: f32,
    ) -> Result<(), ErreurTexte> {
        for primitive in &groupe.primitives {
            match primitive {
                Primitive::Forme { forme, style } => {
                    self.dessiner_forme_manette(skin, *style, forme, presse, e)?;
                }
                Primitive::Texte {
                    ancre,
                    taille,
                    contenu,
                    style,
                } => {
                    self.dessiner_label_manette(*ancre, *taille, contenu, *style, presse.allume)?;
                }
            }
        }
        Ok(())
    }

    /// Une forme de la figure : `style` → fill/stroke du CSS `iv-fig-*` ;
    /// `presse.allume` applique la convention presse (liseré-halo + couleur
    /// unie). Le liseré-halo ≈ moitié du rayon du `drop-shadow` : 12 px → 6e,
    /// 10 px → 5e, 8 px → 4e.
    fn dessiner_forme_manette(
        &mut self,
        skin: SkinManette,
        style: StyleManette,
        forme: &Forme,
        presse: PresseManette,
        e: f32,
    ) -> Result<(), ErreurTexte> {
        let lumiere = |a: f32| D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a,
        };
        // `?` sur le liseré-halo : dessiné sous la forme, largeur en e.
        let allumer = |largeur: f32| -> Result<(), ErreurTexte> {
            let brosse = self.brosse_unie(presse.halo)?;
            self.tracer_forme(forme, &brosse, (largeur * e).max(0.8))
        };
        let allume = presse.allume;
        let couleur = presse.couleur;
        match style {
            StyleManette::Corps => {
                let (x0, y0, x1, y1) = bornes_forme(forme);
                let stops = stops_corps(skin)
                    .map(|(p, rgb)| (p, couleur_d2d_alpha(rgb, 0.62)));
                let brosse =
                    self.brosse_degrade_stops(&stops, ((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1))?;
                self.remplir_forme(forme, &brosse)?;
                let filet = self.brosse_unie(lumiere(0.12))?;
                self.tracer_forme(forme, &filet, (2.0 * e).max(0.5))
            }
            StyleManette::CorpsInner => {
                let brosse = self.brosse_unie(lumiere(0.03))?;
                self.remplir_forme(forme, &brosse)?;
                let filet = self.brosse_unie(lumiere(0.05))?;
                self.tracer_forme(forme, &filet, (1.5 * e).max(0.4))
            }
            StyleManette::Bouton | StyleManette::Home | StyleManette::StickCap => {
                if allume {
                    allumer(if style == StyleManette::Home { 4.0 } else { 6.0 })?;
                }
                let brosse = if allume {
                    self.brosse_unie(couleur)?
                } else {
                    let (x0, y0, x1, y1) = bornes_forme(forme);
                    let stops = stops_btn(skin).map(|(p, rgb)| (p, couleur_d2d(rgb)));
                    self.brosse_degrade_stops(&stops, ((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1))?
                };
                self.remplir_forme(forme, &brosse)?;
                // `.iv-fig-home` garde son filet blanc quand pressé ; face/
                // shoulder/pill/cap passent le filet à la couleur de presse.
                let teinte = if allume && style != StyleManette::Home {
                    couleur
                } else {
                    lumiere(if style == StyleManette::StickCap { 0.16 } else { 0.15 })
                };
                let filet = self.brosse_unie(teinte)?;
                self.tracer_forme(forme, &filet, (1.5 * e).max(0.4))
            }
            StyleManette::Zone => {
                if !allume {
                    return Ok(()); // `fill: transparent` au repos.
                }
                allumer(5.0)?;
                let brosse = self.brosse_unie(D2D1_COLOR_F {
                    a: 0.85,
                    ..couleur
                })?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::Diag => {
                if allume {
                    allumer(4.0)?;
                }
                let brosse = self.brosse_unie(if allume { couleur } else { lumiere(0.12) })?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::Pad => {
                if allume {
                    allumer(6.0)?;
                    let brosse = self.brosse_unie(couleur)?;
                    self.remplir_forme(forme, &brosse)?;
                } else {
                    let brosse = self.brosse_dish(skin, forme)?;
                    self.remplir_forme(forme, &brosse)?;
                }
                // Le filet 0,10 du touchpad est identique pressé ou non.
                let filet = self.brosse_unie(lumiere(0.10))?;
                self.tracer_forme(forme, &filet, (1.5 * e).max(0.4))
            }
            StyleManette::Dish | StyleManette::StickBase => {
                let brosse = self.brosse_dish(skin, forme)?;
                self.remplir_forme(forme, &brosse)?;
                let filet = self.brosse_unie(lumiere(0.08))?;
                self.tracer_forme(forme, &filet, (2.0 * e).max(0.5))
            }
            StyleManette::Cross | StyleManette::Hub => {
                let (x0, y0, x1, y1) = bornes_forme(forme);
                // Hub : dégradé « cross » (8bitdo/xbox) ou « btn » (ps).
                let stops = if skin == SkinManette::Ps && style == StyleManette::Hub {
                    stops_btn(skin)
                } else {
                    stops_cross(skin)
                };
                let stops = stops.map(|(p, rgb)| (p, couleur_d2d(rgb)));
                let brosse =
                    self.brosse_degrade_stops(&stops, ((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1))?;
                self.remplir_forme(forme, &brosse)?;
                let filet = self.brosse_unie(lumiere(0.10))?;
                self.tracer_forme(forme, &filet, (1.5 * e).max(0.4))
            }
            StyleManette::HubDot => {
                let brosse = self.brosse_unie(lumiere(0.08))?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::StickRing => {
                let filet = self.brosse_unie(lumiere(0.08))?;
                self.tracer_forme(forme, &filet, (2.0 * e).max(0.5))
            }
            StyleManette::Ombre => {
                let brosse = self.brosse_unie(D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.35,
                })?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::Sheen => {
                let brosse = self.brosse_unie(lumiere(0.12))?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::SheenLine => {
                let filet = self.brosse_unie(lumiere(0.07))?;
                self.tracer_forme(forme, &filet, (3.0 * e).max(0.7))
            }
            StyleManette::Ico => {
                let brosse = self.brosse_unie(couleur_d2d([0xae, 0xb4, 0xc2, 255]))?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::Deco => {
                let brosse = self.brosse_unie(lumiere(0.10))?;
                self.remplir_forme(forme, &brosse)
            }
        }
    }

    /// `radialGradient` des `iv*-dish` : centre (50 %, 45 %) de la boîte,
    /// rayons 0,62 × boîte — alpha 0,75 comme le `fill-opacity` du CSS.
    fn brosse_dish(&self, skin: SkinManette, forme: &Forme) -> Result<ID2D1Brush, ErreurTexte> {
        let (x0, y0, x1, y1) = bornes_forme(forme);
        let stops = stops_dish(skin).map(|(p, rgb)| (p, couleur_d2d_alpha(rgb, 0.75)));
        self.brosse_radiale(
            &stops,
            ((x0 + x1) / 2.0, y0 + 0.45 * (y1 - y0)),
            0.62 * (x1 - x0),
            0.62 * (y1 - y0),
        )
    }

    /// Brosse dégradée radial (`CreateRadialGradientBrush`, comme le linéaire).
    fn brosse_radiale(
        &self,
        stops: &[(f32, D2D1_COLOR_F)],
        centre: (f32, f32),
        rayon_x: f32,
        rayon_y: f32,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let arrets: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, color)| D2D1_GRADIENT_STOP {
                position: *position,
                color: *color,
            })
            .collect();
        let collection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&arrets, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateGradientStopCollection : {}", e.message())))?;
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: D2D_POINT_2F {
                x: centre.0,
                y: centre.1,
            },
            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
            radiusX: rayon_x,
            radiusY: rayon_y,
        };
        let brosse = unsafe {
            self.cible_rt
                .CreateRadialGradientBrush(&props, None, &collection)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateRadialGradientBrush : {}", e.message())))?;
        brosse
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush : {}", e.message())))
    }

    /// `<text>` de la figure : `text-anchor: middle` + `dominant-baseline:
    /// central` → boîte centrée sur l'ancre ; blanc si le groupe est allumé.
    fn dessiner_label_manette(
        &mut self,
        ancre: crate::input::Point,
        taille: f32,
        contenu: &str,
        style: StyleLabel,
        allume: bool,
    ) -> Result<(), ErreurTexte> {
        let format = self.format_input(taille, true)?;
        let texte: Vec<u16> = contenu.encode_utf16().collect();
        let rect = D2D_RECT_F {
            left: ancre.x - 4.0 * taille,
            top: ancre.y - 0.7 * taille,
            right: ancre.x + 4.0 * taille,
            bottom: ancre.y + 0.6 * taille,
        };
        let encre = self.brosse_unie(if allume {
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            }
        } else {
            match style {
                StyleLabel::Lbl => couleur_d2d([0xdf, 0xe3, 0xec, 255]),
                StyleLabel::LblSm => couleur_d2d([0xae, 0xb4, 0xc2, 255]),
                StyleLabel::Arrow => D2D1_COLOR_F {
                    r: 223.0 / 255.0,
                    g: 227.0 / 255.0,
                    b: 236.0 / 255.0,
                    a: 0.40,
                },
                StyleLabel::Couleur(rgb) => couleur_d2d([rgb[0], rgb[1], rgb[2], 255]),
            }
        })?;
        unsafe {
            self.cible_rt.DrawText(
                &texte,
                &format,
                &rect,
                &encre,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
        Ok(())
    }

    /// Remplit une `Forme` manette (ellipse / rect arrondi / chemin).
    fn remplir_forme(&self, forme: &Forme, brosse: &ID2D1Brush) -> Result<(), ErreurTexte> {
        match forme {
            Forme::Cercle { centre, rayon } => unsafe {
                self.cible_rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: pt(*centre),
                        radiusX: *rayon,
                        radiusY: *rayon,
                    },
                    brosse,
                )
            },
            Forme::Ellipse {
                centre,
                rayon_x,
                rayon_y,
            } => unsafe {
                self.cible_rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: pt(*centre),
                        radiusX: *rayon_x,
                        radiusY: *rayon_y,
                    },
                    brosse,
                )
            },
            Forme::Rect { rect, rayon } => unsafe {
                self.cible_rt
                    .FillRoundedRectangle(&rounded(*rect, *rayon), brosse)
            },
            Forme::Chemin(chemin) => unsafe {
                self.cible_rt
                    .FillGeometry(&self.chemin_d2d(chemin)?, brosse, None)
            },
        }
        Ok(())
    }

    /// Trace le contour d'une `Forme` manette (filet ou liseré-halo).
    fn tracer_forme(&self, forme: &Forme, brosse: &ID2D1Brush, largeur: f32) -> Result<(), ErreurTexte> {
        match forme {
            Forme::Cercle { centre, rayon } => unsafe {
                self.cible_rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: pt(*centre),
                        radiusX: *rayon,
                        radiusY: *rayon,
                    },
                    brosse,
                    largeur,
                    None,
                )
            },
            Forme::Ellipse {
                centre,
                rayon_x,
                rayon_y,
            } => unsafe {
                self.cible_rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: pt(*centre),
                        radiusX: *rayon_x,
                        radiusY: *rayon_y,
                    },
                    brosse,
                    largeur,
                    None,
                )
            },
            Forme::Rect { rect, rayon } => unsafe {
                self.cible_rt.DrawRoundedRectangle(
                    &rounded(*rect, *rayon),
                    brosse,
                    largeur,
                    None,
                )
            },
            Forme::Chemin(chemin) => unsafe {
                self.cible_rt
                    .DrawGeometry(&self.chemin_d2d(chemin)?, brosse, largeur, None)
            },
        }
        Ok(())
    }

    /// `Chemin` (données pures) → `ID2D1Geometry` Direct2D.
    ///
    /// La géométrie de chemin est transformée en interface de base : les
    /// `Fill`/`Draw` n'utilisent que `ID2D1Geometry`.
    fn chemin_d2d(&self, chemin: &crate::input::Chemin) -> Result<ID2D1Geometry, ErreurTexte> {
        let geo: ID2D1PathGeometry = unsafe { self.d2d.CreatePathGeometry() }
            .and_then(|g| g.cast())
            .map_err(|e| ErreurTexte::Gpu(format!("CreatePathGeometry : {}", e.message())))?;
        let sink: ID2D1GeometrySink = unsafe { geo.Open() }
            .map_err(|e| ErreurTexte::Gpu(format!("GeometrySink : {}", e.message())))?;
        unsafe {
            sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
            for segment in &chemin.segments {
                match segment {
                    Segment::Debut(p) => {
                        sink.BeginFigure(pt(*p), D2D1_FIGURE_BEGIN_FILLED)
                    }
                    Segment::Ligne(p) => sink.AddLine(pt(*p)),
                    Segment::Cubique(a, b, d) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: pt(*a),
                        point2: pt(*b),
                        point3: pt(*d),
                    }),
                    Segment::Fermer => sink.EndFigure(D2D1_FIGURE_END_CLOSED),
                }
            }
        }
        unsafe { sink.Close() }
            .map_err(|e| ErreurTexte::Gpu(format!("Close : {}", e.message())))?;
        geo.cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))
    }

    /// Châssis du clavier/numpad : plaque translucide, plateau encaissé, LEDs.
    fn dessiner_chassis(
        &mut self,
        cfg: &InputCfg,
        chassis: &crate::input::Chassis,
        _cqw: f32,
        etat: &FileInput,
    ) -> Result<(), ErreurTexte> {
        let largeur = cfg.rect.2 as f32;
        let plaque = rounded(chassis.plaque, chassis.rayon);
        let brosse = self.brosse_degrade_stops(
            &[
                (0.0, couleur_d2d_alpha([0x33, 0x38, 0x44, 255], 0.55)),
                (0.5, couleur_d2d_alpha([0x23, 0x26, 0x2e, 255], 0.55)),
                (1.0, couleur_d2d_alpha([0x14, 0x16, 0x1b, 255], 0.55)),
            ],
            (chassis.plaque.x, chassis.plaque.y),
            (chassis.plaque.x, chassis.plaque.y + chassis.plaque.hauteur),
        )?;
        unsafe { self.cible_rt.FillRoundedRectangle(&plaque, &brosse) };
        let filet = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.10,
        })?;
        unsafe {
            self.cible_rt.DrawRoundedRectangle(
                &plaque,
                &filet,
                (0.004 * largeur).max(0.4),
                None,
            )
        };

        let cable = rounded(chassis.cable, chassis.rayon * 0.27);
        let noir = self.brosse_unie(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.35,
        })?;
        unsafe { self.cible_rt.FillRoundedRectangle(&cable, &noir) };

        let plateau = rounded(chassis.plateau, chassis.rayon * 0.67);
        unsafe { self.cible_rt.FillRoundedRectangle(&plateau, &noir) };
        let filet_plateau = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.06,
        })?;
        unsafe {
            self.cible_rt.DrawRoundedRectangle(
                &plateau,
                &filet_plateau,
                (0.003 * largeur).max(0.3),
                None,
            )
        };

        for led in &chassis.leds {
            let pressee = etat.est_pressee(led.code);
            let [cr, cg, cb, _] = cfg.couleur;
            let (r, g, b, a) = if pressee {
                (cr as f32 / 255.0, cg as f32 / 255.0, cb as f32 / 255.0, 1.0)
            } else {
                (1.0, 1.0, 1.0, 0.14)
            };
            let centre = D2D_POINT_2F {
                x: led.centre.0,
                y: led.centre.1,
            };
            if pressee {
                // `filter: drop-shadow(0 0 8px couleur)` : halo approximé par
                // un disque translucide plus grand, dessiné **sous** la LED.
                let halo = self.brosse_unie(D2D1_COLOR_F {
                    r,
                    g,
                    b,
                    a: 0.35,
                })?;
                unsafe {
                    self.cible_rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: centre,
                            radiusX: led.rayon * 2.2,
                            radiusY: led.rayon * 2.2,
                        },
                        &halo,
                    )
                };
            }
            let disque = self.brosse_unie(D2D1_COLOR_F { r, g, b, a })?;
            unsafe {
                self.cible_rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: centre,
                        radiusX: led.rayon,
                        radiusY: led.rayon,
                    },
                    &disque,
                )
            };
            let format = self.format_input(led.taille, true)?;
            let texte: Vec<u16> = led.label.encode_utf16().collect();
            let demi = 3.0 * led.taille;
            let rect = D2D_RECT_F {
                left: led.centre.0 - demi,
                top: led.centre.1 + led.rayon,
                right: led.centre.0 + demi,
                bottom: led.centre.1 + led.rayon + 2.0 * led.taille,
            };
            let encre = self.brosse_unie(if pressee {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.35,
                }
            })?;
            unsafe {
                self.cible_rt.DrawText(
                    &texte,
                    &format,
                    &rect,
                    &encre,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                )
            };
        }
        Ok(())
    }

    /// Une keycap : dégradé vertical, bordure, presse (couleur + contour + scale).
    fn dessiner_touche(
        &mut self,
        cfg: &InputCfg,
        cqw: f32,
        touche: &crate::input::TouchePlacee,
        etat: &FileInput,
    ) -> Result<(), ErreurTexte> {
        let pressee = etat.est_pressee_parmi(touche.codes);
        let numpad = cfg.mode == ModeInput::Numpad;
        let rayon = if numpad { 1.6 * cqw } else { 0.9 * cqw };
        let mut r = touche.rect;
        if pressee {
            // `transform: scale(0.92) translateY(0.1cqw)` autour du centre.
            let (cx, cy) = (
                r.x + r.largeur / 2.0,
                r.y + r.hauteur / 2.0 + 0.1 * cqw,
            );
            r = crate::input::RectF {
                x: cx - r.largeur * 0.46,
                y: cy - r.hauteur * 0.46,
                largeur: r.largeur * 0.92,
                hauteur: r.hauteur * 0.92,
            };
        }
        let cap = rounded(r, rayon);
        let brosse = if pressee {
            let [cr, cg, cb, _] = cfg.couleur;
            self.brosse_degrade_stops(
                &[
                    (0.0, couleur_d2d([cr, cg, cb, 255])),
                    (1.0, couleur_d2d(melange_noir([cr, cg, cb, 255], 0.7))),
                ],
                (r.x, r.y),
                (r.x, r.y + r.hauteur),
            )?
        } else {
            self.brosse_degrade_stops(
                &[
                    (0.0, couleur_d2d([0x3d, 0x42, 0x4e, 255])),
                    (0.55, couleur_d2d([0x27, 0x2b, 0x33, 255])),
                    (1.0, couleur_d2d([0x1d, 0x20, 0x27, 255])),
                ],
                (r.x, r.y),
                (r.x, r.y + r.hauteur),
            )?
        };
        unsafe { self.cible_rt.FillRoundedRectangle(&cap, &brosse) };
        let bord = if pressee {
            couleur_d2d(cfg.couleur)
        } else {
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.16,
            }
        };
        let brosse_bord = self.brosse_unie(bord)?;
        unsafe { self.cible_rt.DrawRoundedRectangle(&cap, &brosse_bord, 1.0, None) };

        if pressee {
            // `outline: 3px; outline-offset: 3px` → bande [3, 6] px.
            let dehors = rounded(
                crate::input::RectF {
                    x: r.x - 4.5,
                    y: r.y - 4.5,
                    largeur: r.largeur + 9.0,
                    hauteur: r.hauteur + 9.0,
                },
                rayon + 4.5,
            );
            let brosse_outline = self.brosse_unie(couleur_d2d(cfg.couleur))?;
            unsafe { self.cible_rt.DrawRoundedRectangle(&dehors, &brosse_outline, 3.0, None) };
        }

        if !touche.label.is_empty() {
            let taille = if numpad { 2.0 * cqw } else { 1.9 * cqw };
            let format = self.format_input(taille, numpad)?;
            let texte: Vec<u16> = touche.label.encode_utf16().collect();
            let rect = D2D_RECT_F {
                left: r.x,
                top: r.y,
                right: r.x + r.largeur,
                bottom: r.y + r.hauteur,
            };
            let encre = self.brosse_unie(if pressee {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }
            } else {
                couleur_d2d([0xf0, 0xf0, 0xf0, 255])
            })?;
            unsafe {
                self.cible_rt.DrawText(
                    &texte,
                    &format,
                    &rect,
                    &encre,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                )
            };
        }
        Ok(())
    }

    /// `IDWriteTextFormat` centré (deux axes) pour les libellés input-viewer.
    fn format_input(&mut self, taille: f32, gras: bool) -> Result<IDWriteTextFormat, ErreurTexte> {
        let cle = ((taille * 10.0).round().max(1.0) as u32, gras);
        if let Some(format) = self.formats_input.get(&cle) {
            return Ok(format.clone());
        }
        let nom: Vec<u16> = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        let format = unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(nom.as_ptr()),
                None,
                if gras {
                    DWRITE_FONT_WEIGHT_BOLD
                } else {
                    DWRITE_FONT_WEIGHT_NORMAL
                },
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                taille.max(1.0),
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat input : {}", e.message())))?;
        unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER) }
            .map_err(|e| ErreurTexte::Systeme(format!("alignement : {}", e.message())))?;
        unsafe { format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER) }
            .map_err(|e| ErreurTexte::Systeme(format!("paragraphe : {}", e.message())))?;
        unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("nowrap : {}", e.message())))?;
        self.formats_input.insert(cle, format.clone());
        Ok(format)
    }

    /// Brosse unie (couleur déjà en flottants).
    fn brosse_unie(&self, couleur: D2D1_COLOR_F) -> Result<ID2D1Brush, ErreurTexte> {
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse : {}", e.message())))?;
        brosse
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush : {}", e.message())))
    }

    /// Brosse dégradée linéaire entre deux points, avec arrêts explicites.
    fn brosse_degrade_stops(
        &self,
        stops: &[(f32, D2D1_COLOR_F)],
        debut: (f32, f32),
        fin: (f32, f32),
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let arrets: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, color)| D2D1_GRADIENT_STOP {
                position: *position,
                color: *color,
            })
            .collect();
        let collection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&arrets, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateGradientStopCollection : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: D2D_POINT_2F {
                x: debut.0,
                y: debut.1,
            },
            endPoint: D2D_POINT_2F { x: fin.0, y: fin.1 },
        };
        let brosse = unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("CreateLinearGradientBrush : {}", e.message()))
            })?;
        brosse
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush : {}", e.message())))
    }

    /// Met en page tous les messages d'un widget (une fois par version).
    fn construire_bloc(
        &mut self,
        cfg: &ChatCfg,
        file: &FileChat,
        g: &ChatGeometrie,
        cle: u64,
    ) -> Result<(), ErreurTexte> {
        let format = self.format_chat(g.police)?;
        let mut messages = Vec::new();
        let mut total = 0.0f32;
        for (index, message) in file.pour_filtre(&cfg.filtre).enumerate() {
            let contenu = if message.pseudo.is_empty() {
                message.texte.clone()
            } else {
                format!("{}: {}", message.pseudo, message.texte)
            };
            let utf16: Vec<u16> = contenu.encode_utf16().collect();
            let layout = unsafe {
                self.dwrite
                    .CreateTextLayout(&utf16, &format, g.largeur_texte, 100_000.0)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout chat : {}", e.message())))?;
            unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP) }
                .map_err(|e| ErreurTexte::Systeme(format!("wrap chat : {}", e.message())))?;

            let longueur = message.pseudo.encode_utf16().count() as u32;
            let mut brosse = None;
            if longueur > 0 {
                let plage = DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length: longueur,
                };
                unsafe { layout.SetFontWeight(DWRITE_FONT_WEIGHT_BOLD, plage) }
                    .map_err(|e| ErreurTexte::Systeme(format!("gras chat : {}", e.message())))?;
                let couleur = unsafe {
                    self.cible_rt
                        .CreateSolidColorBrush(&couleur_d2d(message.couleur_pseudo()), None)
                }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse pseudo : {}", e.message())))?;
                let inconnu: IUnknown = couleur
                    .cast()
                    .map_err(|e| ErreurTexte::Systeme(format!("IUnknown pseudo : {}", e.message())))?;
                unsafe { layout.SetDrawingEffect(&inconnu, plage) }
                    .map_err(|e| ErreurTexte::Systeme(format!("couleur pseudo : {}", e.message())))?;
                brosse = Some(couleur);
            }

            let mut metriques = DWRITE_TEXT_METRICS::default();
            unsafe { layout.GetMetrics(&mut metriques) }
                .map_err(|e| ErreurTexte::Systeme(format!("métriques chat : {}", e.message())))?;
            let hauteur = metriques.height.ceil() + g.marge_haut + g.marge_bas;
            total += hauteur;
            messages.push(MessageRendu {
                layout,
                hauteur,
                pair: index % 2 == 0,
                _brosse: brosse,
            });
        }
        total += g.gap * messages.len().saturating_sub(1) as f32;
        self.blocs.insert(
            cle,
            BlocChat {
                version: file.version(),
                _format: format,
                messages,
                total,
            },
        );
        Ok(())
    }

    /// `IDWriteTextFormat` du chat : police système, interligne natif.
    fn format_chat(&self, police: f32) -> Result<IDWriteTextFormat, ErreurTexte> {
        let nom: Vec<u16> = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(nom.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                police.max(1.0),
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat chat : {}", e.message())))
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

        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("GetMetrics : {}", e.message())))?;

        // Marge interne : le contour déborde des glyphes.
        let debord = (texte.style.contour_epaisseur * 0.5).ceil() + 2.0;
        let largeur = (metriques.width.ceil() + 2.0 * debord).max(1.0);
        let hauteur = (metriques.height.ceil() + 2.0 * debord).max(1.0);

        let desc = DescTexture::new(Format::Bgra8, largeur as u32, hauteur as u32)
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

        // Contour : passes décalées (approximation documentée).
        if texte.style.contour_epaisseur > 0.0 {
            let couleur = couleur_d2d(texte.style.contour_couleur);
            let brosse = unsafe { rt.CreateSolidColorBrush(&couleur, None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse contour : {}", e.message())))?;
            let rayon = (texte.style.contour_epaisseur * 0.5).max(0.5);
            for (dx, dy) in couronne(rayon) {
                let origine = D2D_POINT_2F {
                    x: debord + dx,
                    y: debord + dy,
                };
                unsafe { rt.DrawTextLayout(origine, &layout, &brosse, D2D1_DRAW_TEXT_OPTIONS_NONE) };
            }
        }

        // Remplissage : dégradé 135° (haut-gauche → bas-droite), comme le CSS.
        let brosse = self.brosse_degrade(&rt, &texte.style, largeur, hauteur)?;
        let origine = D2D_POINT_2F {
            x: debord,
            y: debord,
        };
        unsafe { rt.DrawTextLayout(origine, &layout, &brosse, D2D1_DRAW_TEXT_OPTIONS_NONE) };

        unsafe { rt.EndDraw(None, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("EndDraw texte : {}", e.message())))?;

        Ok(CacheTexte {
            _texture: texture,
            bitmap,
            largeur,
            hauteur,
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

/// Métriques de mise en page d'un widget chat (px canvas).
///
/// Miroir des règles CSS de `diffusion.html` :
/// - `compact` : `padding: 4px`, `padding-top: 5%` (pourcentage de la
///   **largeur** en CSS), pas d'interligne, message `4px 6px 4px 10px` ;
/// - `bulles` : `padding: 1.4em`, `gap: 0.6em`, message en `1.3em` avec
///   `padding: 6px 10px 6px 12px`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChatGeometrie {
    pub police: f32,
    pub pad_x: f32,
    pub pad_haut: f32,
    pub pad_bas: f32,
    pub gap: f32,
    pub largeur_texte: f32,
    pub marge_gauche: f32,
    pub marge_droite: f32,
    pub marge_haut: f32,
    pub marge_bas: f32,
}

/// Calcule les métriques d'un widget chat (pure, testable sans GPU).
pub fn geometrie_chat(cfg: &ChatCfg) -> ChatGeometrie {
    let (_, _, largeur, _) = cfg.rect;
    let w = largeur as f32;
    let (police, pad_x, pad_haut, pad_bas, gap, mg, md, mh, mb) = if cfg.bulles {
        let pad = 1.4 * cfg.taille_police;
        (
            1.3 * cfg.taille_police,
            pad,
            pad,
            pad,
            0.6 * cfg.taille_police,
            12.0,
            10.0,
            6.0,
            6.0,
        )
    } else {
        (
            cfg.taille_police,
            4.0,
            0.05 * w,
            4.0,
            0.0,
            10.0,
            6.0,
            4.0,
            4.0,
        )
    };
    ChatGeometrie {
        police,
        pad_x,
        pad_haut,
        pad_bas,
        gap,
        largeur_texte: (w - 2.0 * pad_x - mg - md).max(1.0),
        marge_gauche: mg,
        marge_droite: md,
        marge_haut: mh,
        marge_bas: mb,
    }
}

/// Clé de cache d'un bloc de chat : tout ce qui change la **mise en page**
/// (la version du buffer est comparée séparément, pour borner la table).
fn cle_bloc(cfg: &ChatCfg, g: &ChatGeometrie) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cfg.filtre.hash(&mut h);
    cfg.bulles.hash(&mut h);
    g.police.to_bits().hash(&mut h);
    g.largeur_texte.to_bits().hash(&mut h);
    h.finish()
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

/// Couleur RGBA avec une opacité explicite (`fill-opacity` du SVG).
fn couleur_d2d_alpha(rgba: [u8; 4], alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: rgba[0] as f32 / 255.0,
        g: rgba[1] as f32 / 255.0,
        b: rgba[2] as f32 / 255.0,
        a: alpha,
    }
}

/// `color-mix(in srgb, couleur 70%, #000)` : assombrit sans changer l'alpha.
fn melange_noir(rgba: [u8; 4], ratio: f32) -> [u8; 4] {
    [
        (rgba[0] as f32 * ratio).round() as u8,
        (rgba[1] as f32 * ratio).round() as u8,
        (rgba[2] as f32 * ratio).round() as u8,
        rgba[3],
    ]
}

// =============================================================================
// Manette — état allumé d'un groupe + dégradés des 3 skins (`ivg*`/`ivgx*`/`ivgp*`)
// =============================================================================

/// Couleurs de l'état d'un groupe manette : `allume` + couleur de presse et
/// son halo translucide (regroupement pour `too_many_arguments`).
#[derive(Clone, Copy)]
struct PresseManette {
    allume: bool,
    couleur: D2D1_COLOR_F,
    halo: D2D1_COLOR_F,
}

/// Dégradé de coque (`ivg-corps` / `ivgx-corps` / `ivgp-corps`).
fn stops_corps(skin: SkinManette) -> [(f32, [u8; 4]); 3] {
    match skin {
        SkinManette::Bitdo => [
            (0.0, [0x3a, 0x3f, 0x4d, 255]),
            (0.45, [0x26, 0x2a, 0x33, 255]),
            (1.0, [0x14, 0x16, 0x1c, 255]),
        ],
        SkinManette::Xbox => [
            (0.0, [0x33, 0x37, 0x3f, 255]),
            (0.45, [0x21, 0x24, 0x2b, 255]),
            (1.0, [0x12, 0x14, 0x18, 255]),
        ],
        SkinManette::Ps => [
            (0.0, [0x34, 0x3a, 0x46, 255]),
            (0.45, [0x22, 0x26, 0x2e, 255]),
            (1.0, [0x13, 0x15, 0x19, 255]),
        ],
    }
}

/// Dégradé des boutons (`ivg-btn` / `ivgx-btn` / `ivgp-btn`).
fn stops_btn(skin: SkinManette) -> [(f32, [u8; 4]); 2] {
    match skin {
        SkinManette::Bitdo => [(0.0, [0x4a, 0x50, 0x60, 255]), (1.0, [0x22, 0x25, 0x2d, 255])],
        SkinManette::Xbox => [(0.0, [0x45, 0x4b, 0x57, 255]), (1.0, [0x1f, 0x22, 0x29, 255])],
        SkinManette::Ps => [(0.0, [0x47, 0x4e, 0x5c, 255]), (1.0, [0x21, 0x24, 0x2c, 255])],
    }
}

/// Dégradé de la croix D-pad (`ivg-cross` / `ivgx-cross`) — pas de `ivgp-cross`
/// (le moyeu PS prend le dégradé bouton).
fn stops_cross(skin: SkinManette) -> [(f32, [u8; 4]); 2] {
    match skin {
        SkinManette::Bitdo => [(0.0, [0x2f, 0x33, 0x3e, 255]), (1.0, [0x1c, 0x1f, 0x26, 255])],
        SkinManette::Xbox => [(0.0, [0x2c, 0x30, 0x38, 255]), (1.0, [0x19, 0x1c, 0x21, 255])],
        SkinManette::Ps => stops_btn(skin),
    }
}

/// Dégradé radial des creux (`ivg-dish` / `ivgx-dish` / `ivgp-dish`).
fn stops_dish(skin: SkinManette) -> [(f32, [u8; 4]); 2] {
    match skin {
        SkinManette::Bitdo => [(0.0, [0x20, 0x24, 0x2c, 255]), (1.0, [0x10, 0x13, 0x18, 255])],
        _ => [(0.0, [0x1e, 0x21, 0x28, 255]), (1.0, [0x0e, 0x11, 0x14, 255])],
    }
}

/// Boîte englobante (x0, y0, x1, y1) d'une `Forme` manette — base des
/// dégradés `objectBoundingBox`.
fn bornes_forme(forme: &Forme) -> (f32, f32, f32, f32) {
    match forme {
        Forme::Cercle { centre, rayon } => (
            centre.x - rayon,
            centre.y - rayon,
            centre.x + rayon,
            centre.y + rayon,
        ),
        Forme::Ellipse {
            centre,
            rayon_x,
            rayon_y,
        } => (
            centre.x - rayon_x,
            centre.y - rayon_y,
            centre.x + rayon_x,
            centre.y + rayon_y,
        ),
        Forme::Rect { rect, .. } => (
            rect.x,
            rect.y,
            rect.x + rect.largeur,
            rect.y + rect.hauteur,
        ),
        Forme::Chemin(c) => {
            // `bornes_chemin` rend (x milieu, y0, x1, y1) → x0 = 2·xm − x1.
            let (xm, y0, x1, y1) = bornes_chemin(c);
            (2.0 * xm - x1, y0, x1, y1)
        }
    }
}

/// Rect arrondi D2D d'un rect flottant.
fn rounded(rect: crate::input::RectF, rayon: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.largeur,
            bottom: rect.y + rect.hauteur,
        },
        radiusX: rayon.max(0.0),
        radiusY: rayon.max(0.0),
    }
}

/// Point D2D d'un `Point` de la géométrie input.
fn pt(p: crate::input::Point) -> D2D_POINT_2F {
    D2D_POINT_2F { x: p.x, y: p.y }
}

/// Bornes (x milieu, y haut, x droit, y bas) d'un `Chemin` — pour les
/// dégradés `objectBoundingBox` du SVG (direction verticale sur la boîte
/// englobante).
fn bornes_chemin(chemin: &crate::input::Chemin) -> (f32, f32, f32, f32) {
    let mut min = (f32::MAX, f32::MAX);
    let mut max = (f32::MIN, f32::MIN);
    let mut acc = |p: &crate::input::Point| {
        min = (min.0.min(p.x), min.1.min(p.y));
        max = (max.0.max(p.x), max.1.max(p.y));
    };
    for segment in &chemin.segments {
        match segment {
            Segment::Debut(p) | Segment::Ligne(p) => acc(p),
            Segment::Cubique(a, b, d) => {
                acc(a);
                acc(b);
                acc(d);
            }
            Segment::Fermer => {}
        }
    }
    ((min.0 + max.0) / 2.0, min.1, max.0, max.1)
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
        assert_eq!(p.rect.bottom, 200.0);
        assert_eq!(p.rect.left, 100.0 + (400.0 - 150.0) / 2.0);
    }

    #[test]
    fn titre_widget_droite_est_a_droite_du_widget() {
        let mut t = texte(PositionTitre::Droite, CibleTitre::Widget);
        t.cadre = (100.0, 200.0, 400.0, 300.0);
        let p = placement(&t, 150.0, 24.0);
        let centre = p.rotation.expect("titre latéral tourné");
        // Boîte tournée de 24 px de large, collée au bord droit du widget.
        assert_eq!(centre.x, 500.0 + 12.0);
        assert_eq!(centre.y, 200.0 + (300.0 - 24.0) / 2.0 + 75.0);
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

    fn chat(bulles: bool) -> ChatCfg {
        let mut cfg = ChatCfg::depuis_widget((0.0, 0.0, 692.0, 88.0), 14);
        cfg.taille_police = 11.0;
        cfg.bulles = bulles;
        cfg
    }

    #[test]
    fn chat_compact_reprend_le_padding_css() {
        let g = geometrie_chat(&chat(false));
        assert_eq!(g.police, 11.0);
        assert_eq!(g.pad_x, 4.0);
        // `padding-top: 5%` en CSS = 5 % de la LARGEUR du bloc.
        assert!((g.pad_haut - 34.6).abs() < 1e-3);
        assert_eq!(g.gap, 0.0);
        // 692 − 2×4 − 10 (gauche) − 6 (droite)
        assert!((g.largeur_texte - 668.0).abs() < 1e-3);
    }

    #[test]
    fn chat_bulles_grandit_police_padding_et_interligne() {
        let g = geometrie_chat(&chat(true));
        assert!((g.police - 14.3).abs() < 1e-4, "1.3em");
        assert!((g.pad_x - 15.4).abs() < 1e-4, "1.4em");
        assert!((g.pad_haut - 15.4).abs() < 1e-4);
        assert!((g.gap - 6.6).abs() < 1e-4, "0.6em");
        // 692 − 2×15,4 − 12 − 10
        assert!((g.largeur_texte - 639.2).abs() < 1e-3);
    }

    #[test]
    fn cle_de_bloc_ne_depend_que_de_la_mise_en_page() {
        // La version du buffer est comparée à part : deux appels identiques
        // doivent produire la même clé, sinon la table grossirait à chaque
        // message.
        let cfg = chat(false);
        let g = geometrie_chat(&cfg);
        assert_eq!(cle_bloc(&cfg, &g), cle_bloc(&cfg, &g));
        let mut autre = chat(true);
        autre.filtre = "kick".to_string();
        assert_ne!(cle_bloc(&autre, &geometrie_chat(&autre)), cle_bloc(&cfg, &g));
    }
}
