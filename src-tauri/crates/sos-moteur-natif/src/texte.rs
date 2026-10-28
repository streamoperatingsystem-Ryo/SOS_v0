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

use windows::core::{Interface, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Brush, ID2D1Device, ID2D1Factory1,
    ID2D1GradientStopCollection, ID2D1LinearGradientBrush, ID2D1RenderTarget,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_GAMMA_2_2, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1_UNIT_MODE_PIXELS,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteFontCollection1,
    IDWriteTextFormat, IDWriteTextLayout1, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

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

/// Dessinateur de textes sur un canvas BGRA donné.
pub struct DessinateurTexte {
    dwrite: IDWriteFactory5,
    device: ID2D1Device,
    /// La fabrique doit vivre aussi longtemps que ses objets (device, ctx).
    _d2d: ID2D1Factory1,
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
    cache: HashMap<u64, CacheTexte>,
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

        Ok(Self {
            dwrite,
            device,
            _d2d: d2d,
            cible_rt,
            _cible_bitmap: cible_bitmap,
            collection,
            _donnees_polices: donnees,
            device_d3d: contexte.device().clone(),
            cache: HashMap::new(),
        })
    }

    /// Nombre de titres en cache (diagnostic / tests).
    pub fn taille_cache(&self) -> usize {
        self.cache.len()
    }

    /// Dessine les titres sur le canvas, dans l'ordre de `z`.
    ///
    /// Le canvas n'est **pas** effacé : il porte déjà les calques composés.
    /// Les textures de texte absentes du cache sont rendues à la volée, une
    /// seule fois, puis réutilisées à chaque frame.
    pub fn dessiner(&mut self, textes: &[TexteCfg]) -> Result<(), ErreurTexte> {
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

        unsafe { self.cible_rt.BeginDraw() };
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
        unsafe { self.cible_rt.EndDraw(None, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("EndDraw : {}", e.message())))?;
        Ok(())
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
}
