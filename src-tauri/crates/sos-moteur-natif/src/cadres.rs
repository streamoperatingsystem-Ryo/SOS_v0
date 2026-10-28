//! Cadres SVG natifs — géométrie et peinture Direct2D.
//!
//! Miroir **exact** de la source de vérité TS (`src/lib/cadres/`) :
//! `pointsClipPathCadre` / `pathCercle` (`points.ts`), `stopsLiseres` et
//! `angleVersCoords` (`couleurs.ts`), `getMGStyleClipPath` /
//! `getThinLineClipPath` (`cadre-mgstyle.ts` / `cadre-thin-line.ts`).
//! Toute divergence est un bug : les tests comparent les points produits ici
//! aux valeurs **mesurées** du TS (mêmes doubles, mêmes arrondis).
//!
//! **Deux styles seulement** (décision D3 du plan) : `mgstyle` et
//! `thin-line`. Les sept autres du registre ne sont pas portés — la refonte
//! du système de cadres les remplacera.
//!
//! **Rendu fidèle au parent** (vérifié 2026-10-28) : dans `diffusion.html`
//! l'overlay du cadre est enfant du widget, donc rogné par
//! `overflow:hidden` (rect) **et** par le `clip-path` (polygone). En natif,
//! tout le dessin est clipé au **polygone** (inscrit dans le rect). La moitié
//! externe du trait et les ornements de marge (triangles de coin, brackets,
//! circuits, nœuds, diode, « ! », ticks, filet externe thin-line) sont donc
//! **invisibles dans la référence** et ne sont pas portés. Ce qui reste
//! visible est dessiné à l'identique : les trois couches de trait `mgstyle`
//! et ses quatre arcs d'angle, le trait principal `thin-line` et son fermoir
//! (moitié basse).
//!
//! `animer` est hors tranche : le cadre est rendu **statique** (documenté
//! §35 — jamais un rendu faux silencieux).

use std::fmt;
use std::mem::ManuallyDrop;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_ALTERNATE, D2D1_GRADIENT_STOP, D2D_POINT_2F,
    D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL,
    D2D1_CAP_STYLE_FLAT, D2D1_DASH_STYLE_SOLID, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_LINE_JOIN_MITER,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, ID2D1DeviceContext, ID2D1Factory, ID2D1Factory1,
    ID2D1GeometrySink, ID2D1GradientStopCollection, ID2D1LinearGradientBrush, ID2D1PathGeometry,
    ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};

/// Styles portés dans cette tranche (identifiants du registre TS).
pub const STYLE_MGSTYLE: &str = "mgstyle";
pub const STYLE_THIN_LINE: &str = "thin-line";

/// Bornes d'épaisseur de trait (`clampStrokeWidth` des renderers TS).
pub const EPAISSEUR_MIN: f64 = 1.0;
pub const EPAISSEUR_MAX: f64 = 20.0;

/// Style de cadre porté par le moteur natif.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleCadre {
    MgStyle,
    ThinLine,
}

impl StyleCadre {
    /// Style depuis l'identifiant de scène ; `None` = non porté.
    pub fn depuis_nom(nom: &str) -> Option<Self> {
        match nom {
            STYLE_MGSTYLE => Some(Self::MgStyle),
            STYLE_THIN_LINE => Some(Self::ThinLine),
            _ => None,
        }
    }

    /// Identifiant de scène (registre TS).
    pub fn nom(self) -> &'static str {
        match self {
            Self::MgStyle => STYLE_MGSTYLE,
            Self::ThinLine => STYLE_THIN_LINE,
        }
    }
}

/// Un cadre à peindre : rect canvas + style + couleurs.
///
/// `rect` est en **coordonnées canvas** ; le polygone est construit dans le
/// repère local puis translaté, si bien que la même géométrie sert au trait
/// **et** à la découpe du média (un seul polygone, zéro dérive).
#[derive(Debug, Clone, PartialEq)]
pub struct CadreCfg {
    pub rect: (f64, f64, f64, f64),
    pub style: StyleCadre,
    pub stroke_width: f64,
    pub couleur: [u8; 4],
    pub couleur_fin: [u8; 4],
    pub gradient_angle: f64,
    pub z: i32,
}

impl CadreCfg {
    /// Polygone de découpe du cadre, en coordonnées canvas.
    pub fn polygone(&self) -> Vec<(f64, f64)> {
        let (x, y, w, h) = self.rect;
        points_polygone(self.style, w, h, self.stroke_width)
            .into_iter()
            .map(|(px, py)| (px + x, py + y))
            .collect()
    }
}

/// Erreurs du rendu de cadre. Aucune variante ne fait paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurCadre {
    /// Direct2D (géométrie, brosse, couche).
    Systeme(String),
}

impl fmt::Display for ErreurCadre {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurCadre::Systeme(m) => write!(f, "cadre (système) : {m}"),
        }
    }
}

impl std::error::Error for ErreurCadre {}

/// Épaisseur bornée 1..20 (`clampStrokeWidth`), 4 si non finie.
pub fn epaisseur(sw: f64) -> f64 {
    if sw.is_finite() {
        sw.clamp(EPAISSEUR_MIN, EPAISSEUR_MAX)
    } else {
        4.0
    }
}

/// `Math.round` de JavaScript : arrondi à l'entier le plus proche, les
/// demis vers +∞ (`f64::round` arrondit les demis hors de zéro → écart
/// possible sur les valeurs négatives de `angleVersCoords`).
fn arrondi_js(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// Taille de coupe du style (côté biseau mgstyle, rayon thin-line).
pub fn taille_coupe(style: StyleCadre, stroke_width: f64) -> f64 {
    let ep = epaisseur(stroke_width);
    match style {
        StyleCadre::MgStyle => arrondi_js(ep * 3.0).max(10.0),
        StyleCadre::ThinLine => arrondi_js(ep * 1.5).max(6.0),
    }
}

/// Polygone du clip-path d'un cadre — miroir de `pointsClipPathCadre`.
///
/// `w`/`h` = dimensions du widget (ou du canvas pour le bord). Les points
/// sont en repère **local** (0,0 → w,h). Le dernier point est le point de
/// départ rejoué par `Z` : le TS le produit aussi, on le reproduit tel quel
/// (le supprimer serait une divergence).
pub fn points_polygone(style: StyleCadre, w: f64, h: f64, stroke_width: f64) -> Vec<(f64, f64)> {
    let coupe = taille_coupe(style, stroke_width).min(w / 4.0).min(h / 4.0);
    match style {
        StyleCadre::MgStyle => {
            let (sx, sy) = (coupe, coupe * 0.7);
            vec![
                (sx, 0.0),
                (w - sx, 0.0),
                (w, sy),
                (w, h - sy),
                (w - sx, h),
                (sx, h),
                (0.0, h - sy),
                (0.0, sy),
                (sx, 0.0),
            ]
        }
        StyleCadre::ThinLine => {
            let mut f = Aplatisseur::nouveau();
            f.ligne(coupe, 0.0);
            f.ligne(w - coupe, 0.0);
            f.quadratique(w, 0.0, w, coupe);
            f.ligne(w, h - coupe);
            f.quadratique(w, h, w - coupe, h);
            f.ligne(coupe, h);
            f.quadratique(0.0, h, 0.0, h - coupe);
            f.ligne(0.0, coupe);
            f.quadratique(0.0, 0.0, coupe, 0.0);
            f.fermer();
            f.points
        }
    }
}

/// Aplatisseur de segments — miroir de `svgPathEnPoints` (12 pas par Q).
///
/// Les valeurs sont émises directement en `f64` : le TS passe par une chaîne
/// puis `parseFloat`, qui retrouve le même double (repr. la plus courte,
/// aller-retour exact) — l'arithmétique est donc identique.
struct Aplatisseur {
    points: Vec<(f64, f64)>,
    courant: (f64, f64),
    depart: (f64, f64),
}

impl Aplatisseur {
    /// 12 segments par courbe quadratique (`svgPathEnPoints`).
    const PAS: u32 = 12;

    fn nouveau() -> Self {
        Self {
            points: Vec::new(),
            courant: (0.0, 0.0),
            depart: (0.0, 0.0),
        }
    }

    fn ligne(&mut self, x: f64, y: f64) {
        // `depart` n'est posé qu'au premier point (commande M du TS) : `Z`
        // rejoue CE point, pas le dernier segment.
        if self.points.is_empty() {
            self.depart = (x, y);
        }
        self.courant = (x, y);
        self.points.push((x, y));
    }

    fn quadratique(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        let (x0, y0) = self.courant;
        for s in 1..=Self::PAS {
            let t = s as f64 / Self::PAS as f64;
            let t1 = 1.0 - t;
            let bx = t1 * t1 * x0 + 2.0 * t1 * t * cx + t * t * x;
            let by = t1 * t1 * y0 + 2.0 * t1 * t * cy + t * t * y;
            self.points.push((bx, by));
        }
        self.courant = (x, y);
    }

    fn fermer(&mut self) {
        self.points.push(self.depart);
    }
}

/// Path SVG du cercle inscrit — miroir de `pathCercle` (`points.ts`).
///
/// Non rendu dans cette tranche (la caméra est hors périmètre) : la fonction
/// existe pour que le polygone de la forme dédiée caméra n'ait qu'une seule
/// source côté Rust le jour où elle sera branchée.
pub fn path_cercle(w: f64, h: f64) -> String {
    let r = w.min(h) / 2.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let k = 0.552_284_749_830_793_6 * r;
    format!(
        "M {a} {b} C {a} {c} {d} {e} {f} {e}\
         C {i} {e} {i} {c} {i} {b}\
         C {i} {j} {g} {k2} {f} {k2}\
         C {d} {k2} {a} {j} {a} {b} Z",
        a = cx + r,
        b = cy,
        c = cy + k,
        d = cx + k,
        e = cy + r,
        f = cx,
        g = cx - k,
        i = cx - r,
        j = cy - k,
        k2 = cy - r,
    )
}

/// Points du cercle inscrit (aplati par le même aplatisseur, 12 pas).
pub fn points_cercle(w: f64, h: f64) -> Vec<(f64, f64)> {
    let r = w.min(h) / 2.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let k = 0.552_284_749_830_793_6 * r;
    let mut f = Aplatisseur::nouveau();
    f.ligne(cx + r, cy);
    f.cubique(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
    f.cubique(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
    f.cubique(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
    f.cubique(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    f.fermer();
    f.points
}

impl Aplatisseur {
    fn cubique(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        let (x0, y0) = self.courant;
        for s in 1..=Self::PAS {
            let t = s as f64 / Self::PAS as f64;
            let t1 = 1.0 - t;
            let bx = t1 * t1 * t1 * x0
                + 3.0 * t1 * t1 * t * c1x
                + 3.0 * t1 * t * t * c2x
                + t * t * t * x;
            let by = t1 * t1 * t1 * y0
                + 3.0 * t1 * t1 * t * c1y
                + 3.0 * t1 * t * t * c2y
                + t * t * t * y;
            self.points.push((bx, by));
        }
        self.courant = (x, y);
    }
}

/// `#rgb`, `#rrggbb` ou `#rrggbbaa` → RGBA. `None` si la chaîne est invalide.
pub fn couleur_hex(hex: &str) -> Option<[u8; 4]> {
    let h = hex.trim().strip_prefix('#')?;
    let octet = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        3 => {
            let mut c = [255u8; 4];
            for (i, ch) in h.chars().enumerate() {
                c[i] = octet(&ch.to_string().repeat(2))?;
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

fn canal(v: f64) -> u8 {
    arrondi_js(v.clamp(0.0, 255.0)) as u8
}

/// Éclaircit une couleur d'un ratio (0 = inchangé, 1 = blanc) — miroir de
/// `eclaircir` (`couleurs.ts`). L'alpha est conservé.
pub fn eclaircir(c: [u8; 4], ratio: f64) -> [u8; 4] {
    let k = ratio.clamp(0.0, 1.0);
    [
        canal(c[0] as f64 + (255.0 - c[0] as f64) * k),
        canal(c[1] as f64 + (255.0 - c[1] as f64) * k),
        canal(c[2] as f64 + (255.0 - c[2] as f64) * k),
        c[3],
    ]
}

/// Assombrit une couleur d'un ratio (0 = inchangé, 1 = noir) — miroir de
/// `assombrir` (`couleurs.ts`). L'alpha est conservé.
pub fn assombrir(c: [u8; 4], ratio: f64) -> [u8; 4] {
    let k = 1.0 - ratio.clamp(0.0, 1.0);
    [
        canal(c[0] as f64 * k),
        canal(c[1] as f64 * k),
        canal(c[2] as f64 * k),
        c[3],
    ]
}

/// Rampe « liserés » du dégradé principal — miroir de `stopsLiseres`.
///
/// Dominante `sombre`, « éclat » (`vive` éclaircie 45 %) à 33 %, `vive` pure
/// à 68 %. Offsets en fractions 0..1 (le TS travaille en pourcentages).
pub fn stops_liseres(sombre: [u8; 4], vive: [u8; 4]) -> [(f32, [u8; 4]); 8] {
    let eclat = eclaircir(vive, 0.45);
    [
        (0.00, sombre),
        (0.30, sombre),
        (0.33, eclat),
        (0.36, sombre),
        (0.64, sombre),
        (0.68, vive),
        (0.72, sombre),
        (1.00, sombre),
    ]
}

/// Angle (degrés) → extrémités du dégradé en **pourcentages** — miroir de
/// `angleVersCoords` (`couleurs.ts`).
pub fn angle_vers_coords(angle: f64) -> (f64, f64, f64, f64) {
    let angle = if angle.is_finite() { angle } else { 135.0 };
    let rad = angle * std::f64::consts::PI / 180.0;
    let (dx, dy) = (rad.cos(), rad.sin());
    let scale = 50.0 / dx.abs().max(dy.abs()).max(0.001);
    (
        arrondi_js(50.0 - dx * scale),
        arrondi_js(50.0 - dy * scale),
        arrondi_js(50.0 + dx * scale),
        arrondi_js(50.0 + dy * scale),
    )
}

/// Style de trait commun aux deux styles : SVG par défaut (`linecap: butt`,
/// `linejoin: miter`, `stroke-miterlimit: 4`).
pub fn style_trait(factory: &ID2D1Factory1) -> Result<ID2D1StrokeStyle, ErreurCadre> {
    let props = D2D1_STROKE_STYLE_PROPERTIES {
        startCap: D2D1_CAP_STYLE_FLAT,
        endCap: D2D1_CAP_STYLE_FLAT,
        dashCap: D2D1_CAP_STYLE_FLAT,
        lineJoin: D2D1_LINE_JOIN_MITER,
        miterLimit: 4.0,
        dashStyle: D2D1_DASH_STYLE_SOLID,
        dashOffset: 0.0,
    };
    // `ID2D1Factory1::CreateStrokeStyle` attend `…PROPERTIES1` et rend un
    // `ID2D1StrokeStyle1` : on repasse par l'interface de base.
    let base: ID2D1Factory = factory
        .cast()
        .map_err(|e| ErreurCadre::Systeme(format!("ID2D1Factory : {}", e.message())))?;
    unsafe { base.CreateStrokeStyle(&props, None) }
        .map_err(|e| ErreurCadre::Systeme(format!("CreateStrokeStyle : {}", e.message())))
}

/// Géométrie fermée du polygone du cadre, en **coordonnées canvas**.
///
/// C'est l'unique polygone de la tranche : trait du cadre **et** découpe du
/// média passent par cette géométrie.
pub fn geometrie(factory: &ID2D1Factory1, cfg: &CadreCfg) -> Result<ID2D1PathGeometry, ErreurCadre> {
    geometrie_fermee(factory, &cfg.polygone())
}

/// Géométrie fermée à partir d'une liste de points (au moins 3).
pub fn geometrie_fermee(
    factory: &ID2D1Factory1,
    points: &[(f64, f64)],
) -> Result<ID2D1PathGeometry, ErreurCadre> {
    if points.len() < 3 {
        return Err(ErreurCadre::Systeme(format!(
            "polygone dégénéré : {} point(s)",
            points.len()
        )));
    }
    // `ID2D1Factory1::CreatePathGeometry` rend un `ID2D1PathGeometry1` :
    // cast explicite vers l'interface de base, seule utilisée ici.
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry() }
        .and_then(|g| g.cast())
        .map_err(|e| ErreurCadre::Systeme(format!("CreatePathGeometry : {}", e.message())))?;
    let sink: ID2D1GeometrySink = unsafe { geo.Open() }
        .map_err(|e| ErreurCadre::Systeme(format!("Open : {}", e.message())))?;
    unsafe {
        sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
        sink.BeginFigure(point(points[0]), D2D1_FIGURE_BEGIN_FILLED);
        for p in &points[1..] {
            sink.AddLine(point(*p));
        }
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    }
    unsafe { sink.Close() }
        .map_err(|e| ErreurCadre::Systeme(format!("Close : {}", e.message())))?;
    Ok(geo)
}

fn point(p: (f64, f64)) -> D2D_POINT_2F {
    D2D_POINT_2F {
        x: p.0 as f32,
        y: p.1 as f32,
    }
}

fn couleur_d2d(c: [u8; 4], alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c[0] as f32 / 255.0,
        g: c[1] as f32 / 255.0,
        b: c[2] as f32 / 255.0,
        a: (c[3] as f32 / 255.0) * alpha,
    }
}

/// Peint un cadre sur le canvas.
///
/// Le dessin est **clippé au polygone** (miroir du `clip-path` du widget) :
/// la moitié externe du trait et les ornements de marge disparaissent, comme
/// dans `diffusion.html`.
pub fn dessiner(
    factory: &ID2D1Factory1,
    ctx: &ID2D1DeviceContext,
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    cfg: &CadreCfg,
    style_trait: &ID2D1StrokeStyle,
) -> Result<(), ErreurCadre> {
    let (x, y, w, h) = cfg.rect;
    let ep = epaisseur(cfg.stroke_width) as f32;
    let marge = (3.0 * ep).max(16.0) as f64;
    let masque: windows::Win32::Graphics::Direct2D::ID2D1Geometry = geo
        .cast()
        .map_err(|e| ErreurCadre::Systeme(format!("ID2D1Geometry : {}", e.message())))?;
    let mut params = D2D1_LAYER_PARAMETERS1 {
        contentBounds: D2D_RECT_F {
            left: (x - marge) as f32,
            top: (y - marge) as f32,
            right: (x + w + marge) as f32,
            bottom: (y + h + marge) as f32,
        },
        geometricMask: ManuallyDrop::new(Some(masque)),
        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        maskTransform: Matrix3x2::identity(),
        opacity: 1.0,
        opacityBrush: ManuallyDrop::new(None),
        layerOptions: D2D1_LAYER_OPTIONS1_NONE,
    };
    let layer = unsafe { rt.CreateLayer(None) }
        .map_err(|e| ErreurCadre::Systeme(format!("CreateLayer : {}", e.message())))?;
    unsafe { ctx.PushLayer(&params, &layer) };

    let resultat = match cfg.style {
        StyleCadre::MgStyle => dessiner_mg(factory, rt, geo, cfg, ep, style_trait),
        StyleCadre::ThinLine => dessiner_thin(rt, geo, cfg, ep, style_trait),
    };

    unsafe { ctx.PopLayer() };
    // `geometricMask` est en `ManuallyDrop` : la référence créée plus haut ne
    // serait jamais relâchée (une fuite par frame).
    unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
    resultat
}

/// MGStyle visible sous le clip : trois couches de trait + quatre arcs.
fn dessiner_mg(
    factory: &ID2D1Factory1,
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    cfg: &CadreCfg,
    ep: f32,
    style_trait: &ID2D1StrokeStyle,
) -> Result<(), ErreurCadre> {
    // 1) sous-couche `ep*2` à 18 % (halo intérieur), 2) dégradé `ep`,
    // 3) filet accent `0.25ep` à 55 % — même ordre que le SVG.
    let halo = brosse_unie(rt, cfg.couleur, 0.18)?;
    trait_geo(rt, geo, &en_brosse(&halo)?, ep * 2.0, style_trait)?;
    let degrade = brosse_degrade(rt, cfg)?;
    trait_geo(rt, geo, &en_brosse(&degrade)?, ep, style_trait)?;
    let filet = brosse_unie(rt, cfg.couleur_fin, 0.55)?;
    trait_geo(rt, geo, &en_brosse(&filet)?, (ep * 0.25).max(0.8), style_trait)?;

    // Arcs d'angle (seuls ornements mgstyle visibles : ils sont à
    // l'intérieur du polygone).
    let (x, y, w, h) = cfg.rect;
    let coupe = taille_coupe(cfg.style, cfg.stroke_width)
        .min(w / 4.0)
        .min(h / 4.0);
    let r = (ep * 1.2).max(4.0);
    let coins = [
        (x + coupe, y, 0.0_f64),
        (x + w - coupe, y, 90.0),
        (x + w - coupe, y + h, 180.0),
        (x + coupe, y + h, 270.0),
    ];
    let arcs = geometrie_arcs(factory, &coins, r)?;
    let accent = brosse_unie(rt, cfg.couleur_fin, 0.6)?;
    trait_geo(rt, &arcs, &en_brosse(&accent)?, (ep * 0.45).max(1.0), style_trait)?;
    Ok(())
}

/// Thin-line visible sous le clip : trait principal + moitié basse du fermoir.
fn dessiner_thin(
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    cfg: &CadreCfg,
    ep: f32,
    style_trait: &ID2D1StrokeStyle,
) -> Result<(), ErreurCadre> {
    let principal = brosse_unie(rt, cfg.couleur, 1.0)?;
    trait_geo(rt, geo, &en_brosse(&principal)?, ep, style_trait)?;

    // Fermoir : cercle au milieu du bord haut — le clip n'en garde que la
    // moitié basse, exactement comme le parent rogne le SVG.
    let (x, y, w, _) = cfg.rect;
    let r = (ep * 0.45).max(1.6);
    let ellipse = D2D1_ELLIPSE {
        point: D2D_POINT_2F {
            x: (x + w / 2.0) as f32,
            y: y as f32,
        },
        radiusX: r,
        radiusY: r,
    };
    let accent = brosse_unie(rt, cfg.couleur_fin, 1.0)?;
    unsafe { rt.FillEllipse(&ellipse, &en_brosse(&accent)?) };
    let contour = brosse_unie(rt, cfg.couleur, 1.0)?;
    unsafe {
        rt.DrawEllipse(
            &ellipse,
            &en_brosse(&contour)?,
            (ep * 0.15).max(0.5),
            style_trait,
        )
    };
    Ok(())
}

/// Quatre arcs d'angle, une figure par coin (`M cx+r cy A r r 0 0 1 …`).
fn geometrie_arcs(
    factory: &ID2D1Factory1,
    coins: &[(f64, f64, f64)],
    r: f32,
) -> Result<ID2D1PathGeometry, ErreurCadre> {
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry() }
        .and_then(|g| g.cast())
        .map_err(|e| ErreurCadre::Systeme(format!("CreatePathGeometry (arcs) : {}", e.message())))?;
    let sink: ID2D1GeometrySink = unsafe { geo.Open() }
        .map_err(|e| ErreurCadre::Systeme(format!("Open (arcs) : {}", e.message())))?;
    for (i, (cx, cy, rot)) in coins.iter().enumerate() {
        if i == 0 {
            unsafe {
                sink.BeginFigure(
                    D2D_POINT_2F {
                        x: (*cx + r as f64) as f32,
                        y: *cy as f32,
                    },
                    D2D1_FIGURE_BEGIN_HOLLOW,
                )
            };
        }
        let angle = (rot + 90.0) * std::f64::consts::PI / 180.0;
        let fin = D2D_POINT_2F {
            x: (*cx + r as f64 * angle.cos()) as f32,
            y: (*cy + r as f64 * angle.sin()) as f32,
        };
        unsafe {
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: fin,
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
        }
    }
    unsafe {
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()
    }
    .map_err(|e| ErreurCadre::Systeme(format!("Close (arcs) : {}", e.message())))?;
    Ok(geo)
}

fn en_brosse<T: Interface>(
    brosse: &T,
) -> Result<windows::Win32::Graphics::Direct2D::ID2D1Brush, ErreurCadre> {
    brosse
        .cast()
        .map_err(|e| ErreurCadre::Systeme(format!("ID2D1Brush : {}", e.message())))
}

fn trait_geo(
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    brosse: &windows::Win32::Graphics::Direct2D::ID2D1Brush,
    largeur: f32,
    style_trait: &ID2D1StrokeStyle,
) -> Result<(), ErreurCadre> {
    unsafe { rt.DrawGeometry(geo, brosse, largeur, style_trait) };
    Ok(())
}

fn brosse_unie(
    rt: &ID2D1RenderTarget,
    couleur: [u8; 4],
    alpha: f32,
) -> Result<ID2D1SolidColorBrush, ErreurCadre> {
    unsafe { rt.CreateSolidColorBrush(&couleur_d2d(couleur, alpha), None) }
        .map_err(|e| ErreurCadre::Systeme(format!("CreateSolidColorBrush : {}", e.message())))
}

/// Dégradé `couleur → couleurFin` selon `gradientAngle` (rampe liserés).
///
/// Extrémités en coordonnées canvas : le SVG exprime le dégradé en
/// pourcentages de la boîte du tracé (0,0 → w,h), transposée ici par `rect`.
fn brosse_degrade(
    rt: &ID2D1RenderTarget,
    cfg: &CadreCfg,
) -> Result<ID2D1LinearGradientBrush, ErreurCadre> {
    let (x, y, w, h) = cfg.rect;
    let (x1, y1, x2, y2) = angle_vers_coords(cfg.gradient_angle);
    let stops: Vec<D2D1_GRADIENT_STOP> = stops_liseres(cfg.couleur, cfg.couleur_fin)
        .iter()
        .map(|(position, couleur)| D2D1_GRADIENT_STOP {
            position: *position,
            color: couleur_d2d(*couleur, 1.0),
        })
        .collect();
    let collection: ID2D1GradientStopCollection = unsafe {
        rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }
    .map_err(|e| {
        ErreurCadre::Systeme(format!("CreateGradientStopCollection : {}", e.message()))
    })?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: D2D_POINT_2F {
            x: (x + x1 / 100.0 * w) as f32,
            y: (y + y1 / 100.0 * h) as f32,
        },
        endPoint: D2D_POINT_2F {
            x: (x + x2 / 100.0 * w) as f32,
            y: (y + y2 / 100.0 * h) as f32,
        },
    };
    unsafe { rt.CreateLinearGradientBrush(&props, None, &collection) }
        .map_err(|e| ErreurCadre::Systeme(format!("CreateLinearGradientBrush : {}", e.message())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Référence mesurée : `pointsClipPathCadre("mgstyle", …)` du TS.
    const CHAT_MG: [(f64, f64); 9] = [
        (22.156_268_578_620_278, 0.0),
        (670.392_125_835_857_8, 0.0),
        (692.548_394_414_478, 15.509_388_005_034_193),
        (692.548_394_414_478, 73.115_686_309_446_91),
        (670.392_125_835_857_8, 88.625_074_314_481_11),
        (22.156_268_578_620_278, 88.625_074_314_481_11),
        (0.0, 73.115_686_309_446_91),
        (0.0, 15.509_388_005_034_193),
        (22.156_268_578_620_278, 0.0),
    ];

    /// Référence mesurée : `pointsClipPathCadre("thin-line", 100, 40, 4)`.
    const PETIT_THIN: [(f64, f64); 54] = [
        (6.0, 0.0),
        (94.0, 0.0),
        (94.958_333_333_333_31, 0.041_666_666_666_666_664),
        (95.833_333_333_333_34, 0.166_666_666_666_666_66),
        (96.625, 0.375),
        (97.333_333_333_333_34, 0.666_666_666_666_666_6),
        (97.958_333_333_333_31, 1.041_666_666_666_666_7),
        (98.5, 1.5),
        (98.958_333_333_333_33, 2.041_666_666_666_667),
        (99.333_333_333_333_34, 2.666_666_666_666_666_5),
        (99.625, 3.375),
        (99.833_333_333_333_34, 4.166_666_666_666_667),
        (99.958_333_333_333_33, 5.041_666_666_666_666),
        (100.0, 6.0),
        (100.0, 34.0),
        (99.958_333_333_333_31, 34.958_333_333_333_336),
        (99.833_333_333_333_34, 35.833_333_333_333_34),
        (99.625, 36.625),
        (99.333_333_333_333_34, 37.333_333_333_333_336),
        (98.958_333_333_333_33, 37.958_333_333_333_33),
        (98.5, 38.5),
        (97.958_333_333_333_33, 38.958_333_333_333_336),
        (97.333_333_333_333_34, 39.333_333_333_333_336),
        (96.625, 39.625),
        (95.833_333_333_333_34, 39.833_333_333_333_336),
        (94.958_333_333_333_33, 39.958_333_333_333_336),
        (94.0, 40.0),
        (6.0, 40.0),
        (5.041_666_666_666_666, 39.958_333_333_333_33),
        (4.166_666_666_666_667, 39.833_333_333_333_336),
        (3.375, 39.625),
        (2.666_666_666_666_667, 39.333_333_333_333_336),
        (2.041_666_666_666_666, 38.958_333_333_333_33),
        (1.5, 38.5),
        (1.041_666_666_666_666_5, 37.958_333_333_333_33),
        (0.666_666_666_666_666_7, 37.333_333_333_333_336),
        (0.375, 36.625),
        (0.166_666_666_666_666_6, 35.833_333_333_333_336),
        (0.041_666_666_666_666_706, 34.958_333_333_333_336),
        (0.0, 34.0),
        (0.0, 6.0),
        (0.041_666_666_666_666_664, 5.041_666_666_666_666),
        (0.166_666_666_666_666_66, 4.166_666_666_666_667),
        (0.375, 3.375),
        (0.666_666_666_666_666_6, 2.666_666_666_666_667),
        (1.041_666_666_666_666_7, 2.041_666_666_666_666),
        (1.5, 1.5),
        (2.041_666_666_666_667, 1.041_666_666_666_666_5),
        (2.666_666_666_666_666_5, 0.666_666_666_666_666_7),
        (3.375, 0.375),
        (4.166_666_666_666_667, 0.166_666_666_666_666_6),
        (5.041_666_666_666_666, 0.041_666_666_666_666_706),
        (6.0, 0.0),
        (6.0, 0.0),
    ];

    fn proche(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn mgstyle_du_chat_est_identique_au_ts() {
        let p = points_polygone(StyleCadre::MgStyle, 692.548_394_414_478, 88.625_074_314_481_11, 20.0);
        assert_eq!(p.len(), CHAT_MG.len());
        for (i, (a, b)) in p.iter().zip(CHAT_MG.iter()).enumerate() {
            assert!(proche(*a, *b), "point {i} : {a:?} ≠ {b:?}");
        }
    }

    #[test]
    fn thin_line_du_petit_widget_est_identique_au_ts() {
        let p = points_polygone(StyleCadre::ThinLine, 100.0, 40.0, 4.0);
        assert_eq!(p.len(), PETIT_THIN.len(), "54 points attendus (Z rejoue le départ)");
        for (i, (a, b)) in p.iter().zip(PETIT_THIN.iter()).enumerate() {
            assert!(proche(*a, *b), "point {i} : {a:?} ≠ {b:?}");
        }
    }

    #[test]
    fn coupe_mgstyle_est_bornee_par_le_quart_du_widget() {
        // Chat : coupe = max(10, 20*3) = 60, bornée à H/4 = 22,156…
        let p = points_polygone(StyleCadre::MgStyle, 692.548_394_414_478, 88.625_074_314_481_11, 20.0);
        assert!((p[0].0 - 88.625_074_314_481_11 / 4.0).abs() < 1e-9);
        // Speedrun : coupe bornée à W/4 = 53,751…
        let p = points_polygone(StyleCadre::MgStyle, 215.007_939_760_270_5, 704.572_260_121_205_3, 20.0);
        assert!((p[0].0 - 215.007_939_760_270_5 / 4.0).abs() < 1e-9);
        assert!((p[2].1 - 215.007_939_760_270_5 / 4.0 * 0.7).abs() < 1e-9);
    }

    #[test]
    fn thin_line_rejoue_le_point_de_depart() {
        let p = points_polygone(StyleCadre::ThinLine, 300.0, 200.0, 4.0);
        assert_eq!(p.len(), 54);
        assert_eq!(p[0], p[52]);
        assert_eq!(p[52], p[53]);
    }

    #[test]
    fn epaisseur_bornee_et_non_finie() {
        assert_eq!(epaisseur(0.0), 1.0);
        assert_eq!(epaisseur(20.0), 20.0);
        assert_eq!(epaisseur(50.0), 20.0);
        assert_eq!(epaisseur(f64::NAN), 4.0);
        assert_eq!(taille_coupe(StyleCadre::MgStyle, 4.0), 12.0);
        assert_eq!(taille_coupe(StyleCadre::ThinLine, 4.0), 6.0);
    }

    #[test]
    fn angle_135_donne_les_extremites_du_ts() {
        assert_eq!(angle_vers_coords(135.0), (100.0, 0.0, 0.0, 100.0));
        assert_eq!(angle_vers_coords(0.0), (0.0, 50.0, 100.0, 50.0));
        assert_eq!(angle_vers_coords(f64::NAN), (100.0, 0.0, 0.0, 100.0));
    }

    #[test]
    fn rampe_liseres_et_eclaircissement() {
        let sombre = [0, 0, 0, 255];
        let vive = [140, 0, 255, 255];
        let stops = stops_liseres(sombre, vive);
        assert_eq!(stops[0], (0.0, sombre));
        assert_eq!(stops[3].0, 0.36);
        assert_eq!(stops[5], (0.68, vive));
        // éclat = eclaircir(#8c00ff, 0.45) : 140 + 115*0.45 = 191,75 → 192
        assert_eq!(stops[2].1, [192, 115, 255, 255]);
        assert_eq!(eclaircir([10, 20, 30, 128], 0.0), [10, 20, 30, 128]);
        assert_eq!(assombrir([10, 20, 30, 128], 1.0), [0, 0, 0, 128]);
    }

    #[test]
    fn couleur_hex_trois_six_et_huit_chiffres() {
        assert_eq!(couleur_hex("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(couleur_hex("#8b5cf6"), Some([0x8b, 0x5c, 0xf6, 255]));
        assert_eq!(couleur_hex("#8b5cf680"), Some([0x8b, 0x5c, 0xf6, 0x80]));
        assert_eq!(couleur_hex("8b5cf6"), None);
        assert_eq!(couleur_hex("#zzzzzz"), None);
    }

    #[test]
    fn cercle_inscrit_et_polygone() {
        let p = points_cercle(129.176_118_700_744_38, 88.552_200_433_533_29);
        assert_eq!(p.len(), 50);
        assert!(proche(p[0], (108.864_159_567_138_84, 44.276_100_216_766_64)));
        assert!(path_cercle(100.0, 100.0).starts_with("M 100 50 C 100 77.61"));
    }

    #[test]
    fn polygone_decale_au_rect_canvas() {
        let cfg = CadreCfg {
            rect: (100.0, 50.0, 200.0, 100.0),
            style: StyleCadre::MgStyle,
            stroke_width: 20.0,
            couleur: [0, 0, 0, 255],
            couleur_fin: [140, 0, 255, 255],
            gradient_angle: 135.0,
            z: 3,
        };
        let p = cfg.polygone();
        let local = points_polygone(StyleCadre::MgStyle, 200.0, 100.0, 20.0);
        assert_eq!(p.len(), local.len());
        for (a, b) in p.iter().zip(local.iter()) {
            assert!((a.0 - (b.0 + 100.0)).abs() < 1e-9);
            assert!((a.1 - (b.1 + 50.0)).abs() < 1e-9);
        }
    }
}
