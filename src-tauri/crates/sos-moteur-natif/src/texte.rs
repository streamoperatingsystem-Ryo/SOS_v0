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
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Brush, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1Geometry, ID2D1GradientStopCollection, ID2D1Layer, ID2D1LinearGradientBrush,
    ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1_UNIT_MODE_PIXELS,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteFontCollection1,
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::cadres::{CadreCfg, ErreurCadre};
use crate::chat::{ChatCfg, FileChat};
use crate::gpu::{ContexteGpu, DescTexture, Format};

/// Écart aux bords, en px canvas — valeur de `diffusion.html` (`.bg-titre`).
pub const MARGE_TITRE: f32 = 8.0;

/// Une police embarquée : (id CSS/`fonts.ts`, famille OpenType, woff2).
///
/// Miroir de `src/lib/fonts.ts` (`POLICES_TITRE`) — le JSON des scènes stocke
/// l'`id`, DirectWrite exige le nom de **famille** contenu dans le fichier.
/// Ne jamais renommer un `id` sans migrer les scènes.
pub const POLICES: &[(&str, &str, &[u8])] = &[
    ("BebasNeue", "Bebas Neue", include_bytes!("../../../resources/fonts/BebasNeue.woff2")),
    ("Anton", "Anton", include_bytes!("../../../resources/fonts/Anton.woff2")),
    ("Oswald", "Oswald", include_bytes!("../../../resources/fonts/Oswald.woff2")),
    ("Pacifico", "Pacifico", include_bytes!("../../../resources/fonts/Pacifico.woff2")),
    ("PermanentMarker", "Permanent Marker", include_bytes!("../../../resources/fonts/PermanentMarker.woff2")),
    ("Bungee", "Bungee", include_bytes!("../../../resources/fonts/Bungee.woff2")),
    ("RussoOne", "Russo One", include_bytes!("../../../resources/fonts/RussoOne.woff2")),
    ("PressStart2P", "Press Start 2P", include_bytes!("../../../resources/fonts/PressStart2P.woff2")),
    ("Orbitron", "Orbitron", include_bytes!("../../../resources/fonts/Orbitron.woff2")),
    ("Montserrat", "Montserrat", include_bytes!("../../../resources/fonts/Montserrat.woff2")),
    ("Righteous", "Righteous", include_bytes!("../../../resources/fonts/Righteous.woff2")),
    ("Audiowide", "Audiowide", include_bytes!("../../../resources/fonts/Audiowide.woff2")),
    ("Lobster", "Lobster", include_bytes!("../../../resources/fonts/Lobster.woff2")),
    ("Caveat", "Caveat", include_bytes!("../../../resources/fonts/Caveat.woff2")),
    ("Satisfy", "Satisfy", include_bytes!("../../../resources/fonts/Satisfy.woff2")),
    ("Teko", "Teko", include_bytes!("../../../resources/fonts/Teko.woff2")),
    ("Rajdhani", "Rajdhani", include_bytes!("../../../resources/fonts/Rajdhani.woff2")),
    ("BlackOpsOne", "Black Ops One", include_bytes!("../../../resources/fonts/BlackOpsOne.woff2")),
    ("Creepster", "Creepster", include_bytes!("../../../resources/fonts/Creepster.woff2")),
    ("Fredoka", "Fredoka", include_bytes!("../../../resources/fonts/Fredoka.woff2")),
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
    _cible_bitmap: ID2D1Bitmap1,
    collection: IDWriteFontCollection,
    /// TTF décodés : les références de police pointent dedans, ils doivent
    /// vivre aussi longtemps que la collection.
    _donnees_polices: Vec<Vec<u8>>,
    device_d3d: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    /// Trait des cadres (miter, limite 4) — créé une fois.
    style_trait: ID2D1StrokeStyle,
    cache: HashMap<u64, CacheTexte>,
    /// Bitmaps Direct2D des médias, clé = pointeur de la texture. L'entrée
    /// conserve la texture : `ID2D1Bitmap1` ne la possède pas.
    bitmaps: HashMap<
        usize,
        (
            windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
            ID2D1Bitmap1,
        ),
    >,
    /// Blocs de chat mis en page, clé = (version du buffer, filtre, métriques).
    blocs: HashMap<u64, BlocChat>,
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
            _cible_bitmap: cible_bitmap,
            collection,
            _donnees_polices: donnees,
            device_d3d: contexte.device().clone(),
            style_trait,
            cache: HashMap::new(),
            bitmaps: HashMap::new(),
            blocs: HashMap::new(),
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

    /// Vue Direct2D d'une texture média (créée une fois, réutilisée).
    ///
    /// Clé = pointeur de la texture : les médias vivent toute la session.
    /// La texture doit porter `D3D11_BIND_SHADER_RESOURCE` (voir
    /// `ContexteGpu::creer_texture_echantillonnable`).
    pub fn bitmap_media(
        &mut self,
        texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    ) -> Result<ID2D1Bitmap1, ErreurTexte> {
        let cle = texture.as_raw() as usize;
        if let Some((_, bitmap)) = self.bitmaps.get(&cle) {
            return Ok(bitmap.clone());
        }
        let surface: IDXGISurface = texture
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("IDXGISurface média : {}", e.message())))?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: ManuallyDrop::new(None),
        };
        let bitmap = unsafe { self.ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
            .map_err(|e| ErreurTexte::Gpu(format!("bitmap média : {}", e.message())))?;
        self.bitmaps.insert(cle, (texture.clone(), bitmap.clone()));
        Ok(bitmap)
    }

    /// Dessine un média **découpé au polygone** du cadre — dans une passe.
    ///
    /// `source`/`destination` sont ceux du video processor
    /// (`media::geometrie`) : le rendu est identique, seule la découpe change.
    pub fn dessiner_media(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        destination: RECT,
        source: RECT,
        clip: &ID2D1PathGeometry,
    ) -> Result<(), ErreurTexte> {
        let masque: ID2D1Geometry = clip
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))?;
        let dst = D2D_RECT_F {
            left: destination.left as f32,
            top: destination.top as f32,
            right: destination.right as f32,
            bottom: destination.bottom as f32,
        };
        let src = D2D_RECT_F {
            left: source.left as f32,
            top: source.top as f32,
            right: source.right as f32,
            bottom: source.bottom as f32,
        };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: dst,
            geometricMask: ManuallyDrop::new(Some(masque)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer média : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        unsafe {
            self.cible_rt.DrawBitmap(
                bitmap,
                Some(&dst),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                Some(&src),
            )
        };
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        Ok(())
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
