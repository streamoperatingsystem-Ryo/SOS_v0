//! Cadres SVG natifs — géométrie et peinture Direct2D.
//!
//! Miroir **exact** de la source de vérité TS (`src/lib/cadres/`) :
//! `pointsClipPathCadre` / `pathCercle` (`points.ts`), `stopsLiseres` et
//! `angleVersCoords` (`couleurs.ts`), `getMGStyleClipPath` /
//! `getThinLineClipPath` (`cadre-mgstyle.ts` / `cadre-thin-line.ts`).
//! Toute divergence est un bug : les tests comparent les points produits ici
//! aux valeurs **mesurées** du TS (mêmes doubles, mêmes arrondis).
//!
//! **Neuf styles portés** — parité complète avec le registre TS
//! (`mgstyle`, `thin-line`, `carre`, `arrondis`, `tribal`, `cyberpunk`,
//! `story`, `hexagon`, `arena`), lots C1/C2a/C2b.
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
//! `animer` (lot C1) : segment comète sur le tracé du bord, miroir du SMIL
//! `stroke-dashoffset` des styles `mgstyle` / `thin-line` / `carre` /
//! `arrondis` — 7 % du périmètre, un tour toutes les 4 s, dans `couleurFin`.
//! `tribal` (lot C2b-1) a sa propre animation : respiration du motif et nœud
//! voyageur le long du bord.

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
    D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_LINE_JOIN_MITER, D2D1_LINE_JOIN_ROUND, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, ID2D1DeviceContext, ID2D1Factory, ID2D1Factory1,
    ID2D1GeometrySink, ID2D1GradientStopCollection, ID2D1LinearGradientBrush, ID2D1PathGeometry,
    ID2D1RadialGradientBrush, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};

/// Styles portés dans cette tranche (identifiants du registre TS).
pub const STYLE_MGSTYLE: &str = "mgstyle";
pub const STYLE_THIN_LINE: &str = "thin-line";
pub const STYLE_CARRE: &str = "carre";
pub const STYLE_ARRONDIS: &str = "arrondis";
pub const STYLE_TRIBAL: &str = "tribal";
pub const STYLE_CYBERPUNK: &str = "cyberpunk";
pub const STYLE_STORY: &str = "story";
pub const STYLE_HEXAGON: &str = "hexagon";
pub const STYLE_ARENA: &str = "arena";

/// Bornes d'épaisseur de trait (`clampStrokeWidth` des renderers TS).
pub const EPAISSEUR_MIN: f64 = 1.0;
pub const EPAISSEUR_MAX: f64 = 20.0;

/// Style de cadre porté par le moteur natif.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleCadre {
    MgStyle,
    ThinLine,
    Carre,
    Arrondis,
    Tribal,
    Cyberpunk,
    Story,
    Hexagon,
    Arena,
}

impl StyleCadre {
    /// Style depuis l'identifiant de scène ; `None` = non porté.
    pub fn depuis_nom(nom: &str) -> Option<Self> {
        match nom {
            STYLE_MGSTYLE => Some(Self::MgStyle),
            STYLE_THIN_LINE => Some(Self::ThinLine),
            STYLE_CARRE => Some(Self::Carre),
            STYLE_ARRONDIS => Some(Self::Arrondis),
            STYLE_TRIBAL => Some(Self::Tribal),
            STYLE_CYBERPUNK => Some(Self::Cyberpunk),
            STYLE_STORY => Some(Self::Story),
            STYLE_HEXAGON => Some(Self::Hexagon),
            STYLE_ARENA => Some(Self::Arena),
            _ => None,
        }
    }

    /// Identifiant de scène (registre TS).
    pub fn nom(self) -> &'static str {
        match self {
            Self::MgStyle => STYLE_MGSTYLE,
            Self::ThinLine => STYLE_THIN_LINE,
            Self::Carre => STYLE_CARRE,
            Self::Arrondis => STYLE_ARRONDIS,
            Self::Tribal => STYLE_TRIBAL,
            Self::Cyberpunk => STYLE_CYBERPUNK,
            Self::Story => STYLE_STORY,
            Self::Hexagon => STYLE_HEXAGON,
            Self::Arena => STYLE_ARENA,
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
    /// Segment comète (`CadreConfig.animer`) — 7 % du périmètre en rotation.
    pub animer: bool,
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

/// Traits partagés du rendu de cadre — créés **une fois** (coût nul au
/// rendu) : trait principal (miter, limite 4) + comète `animer` (bouts ronds).
pub struct StylesTrait {
    pub cadre: ID2D1StrokeStyle,
    pub comete: ID2D1StrokeStyle,
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
        // Styles à bord droit : aucune coupe.
        StyleCadre::Carre
        | StyleCadre::Arrondis
        | StyleCadre::Tribal
        | StyleCadre::Cyberpunk
        | StyleCadre::Story
        | StyleCadre::Hexagon
        | StyleCadre::Arena => 0.0,
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
        StyleCadre::ThinLine => polygone_arrondi(w, h, coupe),
        // `arrondis` : rectangle arrondi r = min(8, W/4, H/4) — le rayon est
        // en px absolus dans le DOM (`genererClipPathCadre`).
        StyleCadre::Arrondis => polygone_arrondi(w, h, 8.0_f64.min(w / 4.0).min(h / 4.0)),
        // `carre` : aucun clip dans le DOM — le rectangle est neutre.
        StyleCadre::Carre => vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, 0.0)],
        // `tribal` : octogone à coins adoucis (`tribalPath` du SVG).
        StyleCadre::Tribal => {
            let (cx, cy) = tribal_corner(stroke_width, w, h);
            let mut f = Aplatisseur::nouveau();
            f.ligne(cx, 0.0);
            f.ligne(w - cx, 0.0);
            f.quadratique(w - cx * 0.15, cy * 0.15, w, cy);
            f.ligne(w, h - cy);
            f.quadratique(w - cx * 0.15, h - cy * 0.15, w - cx, h);
            f.ligne(cx, h);
            f.quadratique(cx * 0.15, h - cy * 0.15, 0.0, h - cy);
            f.ligne(0.0, cy);
            f.quadratique(cx * 0.15, cy * 0.15, cx, 0.0);
            f.fermer();
            f.points
        }
        // `cyberpunk` : octogone à biseaux droits (`getCyberpunkClipPath`).
        StyleCadre::Cyberpunk => {
            let s = cyberpunk_bevel(stroke_width).min(w / 4.0).min(h / 4.0);
            vec![
                (s, 0.0),
                (w - s, 0.0),
                (w, s),
                (w, h - s),
                (w - s, h),
                (s, h),
                (0.0, h - s),
                (0.0, s),
                (s, 0.0),
            ]
        }
        // `story` : bord festonné en cubiques (`getStoryClipPath`).
        StyleCadre::Story => {
            let s = story_clip_size(stroke_width).min(w / 4.0).min(h / 4.0);
            let mut f = Aplatisseur::nouveau();
            f.ligne(s, 0.0);
            f.ligne(w - s, 0.0);
            f.cubique(
                w - s * 0.65,
                -s * 0.12,
                w - s * 0.12,
                -s * 0.12,
                w - s * 0.4,
                s * 0.08,
            );
            f.cubique(w - s * 0.68, s * 0.28, w + s * 0.12, s * 0.65, w, s);
            f.ligne(w, h - s);
            f.cubique(
                w + s * 0.12,
                h - s * 0.65,
                w - s * 0.68,
                h - s * 0.28,
                w - s * 0.4,
                h - s * 0.08,
            );
            f.cubique(
                w - s * 0.12,
                h + s * 0.12,
                w - s * 0.65,
                h + s * 0.12,
                w - s,
                h,
            );
            f.ligne(s, h);
            f.cubique(
                s * 0.65,
                h + s * 0.12,
                s * 0.12,
                h + s * 0.12,
                s * 0.4,
                h - s * 0.08,
            );
            f.cubique(
                s * 0.68,
                h - s * 0.28,
                -s * 0.12,
                h - s * 0.65,
                0.0,
                h - s,
            );
            f.ligne(0.0, s);
            f.cubique(-s * 0.12, s * 0.65, s * 0.68, s * 0.28, s * 0.4, s * 0.08);
            f.cubique(s * 0.12, -s * 0.12, s * 0.65, -s * 0.12, s, 0.0);
            f.fermer();
            f.points
        }
        // `hexagon` : octogone à pans coupés (`getHexagonClipPath`).
        StyleCadre::Hexagon => {
            let s = hexagon_clip_size(stroke_width).min(w / 4.0).min(h / 4.0);
            vec![
                (s, 0.0),
                (w - s, 0.0),
                (w, s * 0.5),
                (w, h - s * 0.5),
                (w - s, h),
                (s, h),
                (0.0, h - s * 0.5),
                (0.0, s * 0.5),
                (s, 0.0),
            ]
        }
        // `arena` : octogone allongé (`getArenaClipPath`).
        StyleCadre::Arena => {
            let (bx, by) = arena_bevels(stroke_width, w, h);
            vec![
                (bx, 0.0),
                (w - bx, 0.0),
                (w, by),
                (w, h - by),
                (w - bx, h),
                (bx, h),
                (0.0, h - by),
                (0.0, by),
                (bx, 0.0),
            ]
        }
    }
}

/// Biseau de `cyberpunk` (`getCyberpunkBevelSize`) : `max(8, round(3.5·ep))`.
fn cyberpunk_bevel(stroke_width: f64) -> f64 {
    8.0_f64.max(arrondi_js(epaisseur(stroke_width) * 3.5))
}

/// Pans de `hexagon` (`getHexagonClipSize`) : `max(12, round(4·ep))`.
fn hexagon_clip_size(stroke_width: f64) -> f64 {
    12.0_f64.max(arrondi_js(epaisseur(stroke_width) * 4.0))
}

/// Biseaux de `arena` (`getArenaBevels`) : `bx = max(16, round(4.2·ep))`,
/// `by = max(10, round(2.4·ep))`, bornés à `W/4`/`H/4`.
fn arena_bevels(stroke_width: f64, w: f64, h: f64) -> (f64, f64) {
    let ep = epaisseur(stroke_width);
    (
        16.0_f64.max(arrondi_js(ep * 4.2)).min(w / 4.0),
        10.0_f64.max(arrondi_js(ep * 2.4)).min(h / 4.0),
    )
}

/// Feston de `story` (`getStoryClipSize`) : `max(12, round(4·ep))`.
fn story_clip_size(stroke_width: f64) -> f64 {
    12.0_f64.max(arrondi_js(epaisseur(stroke_width) * 4.0))
}

/// Coins de `tribal` (`getTribalCorner`) : `cx`/`cy` bruts, puis bornés à
/// W/4 et H/4 par l'appelant.
fn tribal_corner(stroke_width: f64, w: f64, h: f64) -> (f64, f64) {
    let ep = epaisseur(stroke_width);
    let cx = 12.0_f64.max(arrondi_js(ep * 3.0));
    let cy = 9.0_f64.max(arrondi_js(ep * 2.4));
    (cx.min(w / 4.0), cy.min(h / 4.0))
}

/// Rectangle arrondi aplati — même ordre de commandes que le path SVG
/// (`M r 0 … Q … Z`), donc mêmes points que `svgPathEnPoints`.
fn polygone_arrondi(w: f64, h: f64, r: f64) -> Vec<(f64, f64)> {
    let mut f = Aplatisseur::nouveau();
    f.ligne(r, 0.0);
    f.ligne(w - r, 0.0);
    f.quadratique(w, 0.0, w, r);
    f.ligne(w, h - r);
    f.quadratique(w, h, w - r, h);
    f.ligne(r, h);
    f.quadratique(0.0, h, 0.0, h - r);
    f.ligne(0.0, r);
    f.quadratique(0.0, 0.0, r, 0.0);
    f.fermer();
    f.points
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

/// Style de trait de la comète (`animer`) : `stroke-linecap="round"` du SVG.
pub fn style_comete(factory: &ID2D1Factory1) -> Result<ID2D1StrokeStyle, ErreurCadre> {
    let props = D2D1_STROKE_STYLE_PROPERTIES {
        startCap: D2D1_CAP_STYLE_ROUND,
        endCap: D2D1_CAP_STYLE_ROUND,
        dashCap: D2D1_CAP_STYLE_FLAT,
        lineJoin: D2D1_LINE_JOIN_ROUND,
        miterLimit: 4.0,
        dashStyle: D2D1_DASH_STYLE_SOLID,
        dashOffset: 0.0,
    };
    let base: ID2D1Factory = factory
        .cast()
        .map_err(|e| ErreurCadre::Systeme(format!("ID2D1Factory : {}", e.message())))?;
    unsafe { base.CreateStrokeStyle(&props, None) }.map_err(|e| {
        ErreurCadre::Systeme(format!("CreateStrokeStyle (comète) : {}", e.message()))
    })
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

/// Géométrie ouverte (comète) à partir d'une liste de points (au moins 2).
pub fn geometrie_ouverte(
    factory: &ID2D1Factory1,
    points: &[(f64, f64)],
) -> Result<ID2D1PathGeometry, ErreurCadre> {
    if points.len() < 2 {
        return Err(ErreurCadre::Systeme(format!(
            "tracé dégénéré : {} point(s)",
            points.len()
        )));
    }
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry() }
        .and_then(|g| g.cast())
        .map_err(|e| ErreurCadre::Systeme(format!("CreatePathGeometry : {}", e.message())))?;
    let sink: ID2D1GeometrySink = unsafe { geo.Open() }
        .map_err(|e| ErreurCadre::Systeme(format!("Open : {}", e.message())))?;
    unsafe {
        sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
        sink.BeginFigure(point(points[0]), D2D1_FIGURE_BEGIN_HOLLOW);
        for p in &points[1..] {
            sink.AddLine(point(*p));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
    }
    unsafe { sink.Close() }
        .map_err(|e| ErreurCadre::Systeme(format!("Close : {}", e.message())))?;
    Ok(geo)
}

/// Durée d'un tour de comète (`<animate … dur="4s">`).
const DUREE_COMETE_S: f64 = 4.0;
/// `stroke-dasharray="7 93"` sur `pathLength="100"`.
const FRACTION_COMETE: f64 = 0.07;

/// Tracé de la comète : le **chemin du SVG du style**, pas le polygone de
/// découpe.
///
/// - `mgstyle` : rectangle plein cadre (`rectPath` rx=0 du SVG) ;
/// - `thin-line` : rectangle arrondi, identique au polygone de découpe
///   (`getThinLineClipPath`).
///
/// En coordonnées canvas, fermé (dernier point = premier).
pub fn trace_comete(cfg: &CadreCfg) -> Vec<(f64, f64)> {
    let (x, y, w, h) = cfg.rect;
    match cfg.style {
        StyleCadre::MgStyle => vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)],
        StyleCadre::ThinLine => cfg.polygone(),
        // `carre` : `rectPath` rx=0 du SVG.
        StyleCadre::Carre => vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)],
        // `arrondis` : `rectPath` avec rx=8 (non borné dans le DOM) — la
        // rognure vient du clip `min(8, W/4, H/4)`.
        StyleCadre::Arrondis => polygone_arrondi(w, h, 8.0)
            .into_iter()
            .map(|(px, py)| (px + x, py + y))
            .collect(),
        // `tribal` : le nœud voyageur suit le bord (`tribalPath`), pas de
        // comète (la respiration du motif est gérée dans le rendu).
        StyleCadre::Tribal => cfg.polygone(),
        // `cyberpunk` : le runner suit le bord biseauté, pas de comète.
        StyleCadre::Cyberpunk => cfg.polygone(),
        // `story` : l'étincelle voyageuse suit le bord festonné.
        StyleCadre::Story => cfg.polygone(),
        // `hexagon` : le nœud voyageur suit le bord octogonal.
        StyleCadre::Hexagon => cfg.polygone(),
        // `arena` : le nœud voyageur suit le bord octogonal allongé.
        StyleCadre::Arena => cfg.polygone(),
    }
}

/// Segment de comète sur un tracé fermé.
///
/// `phase` ∈ [0,1) = départ en fraction de périmètre (`stroke-dashoffset`
/// 0→−100 sur 4 s), `fraction` = longueur du segment (`7 93` → 0,07).
/// Le segment **boucle** : il traverse le point de départ si nécessaire.
pub fn comete_points(trace: &[(f64, f64)], phase: f64, fraction: f64) -> Vec<(f64, f64)> {
    if trace.len() < 2 {
        return Vec::new();
    }
    let mut cumul = Vec::with_capacity(trace.len());
    cumul.push(0.0);
    let mut total = 0.0;
    for i in 1..trace.len() {
        let (x0, y0) = trace[i - 1];
        let (x1, y1) = trace[i];
        total += ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        cumul.push(total);
    }
    if total <= f64::EPSILON {
        return Vec::new();
    }
    let mut reste = fraction.clamp(0.0, 1.0) * total;
    let mut s = phase.rem_euclid(1.0) * total;
    let mut points = vec![point_sur(trace, &cumul, s)];
    let mut garde = 0;
    while reste > 1e-9 && garde < trace.len() * 2 {
        garde += 1;
        let i = cumul
            .iter()
            .position(|c| *c > s + 1e-9)
            .unwrap_or(cumul.len() - 1);
        let d = (cumul[i] - s).min(reste);
        s += d;
        reste -= d;
        if s >= total - 1e-9 {
            s = 0.0;
        }
        points.push(point_sur(trace, &cumul, s));
    }
    points
}

/// Point à la distance curviligne `s` sur le tracé.
fn point_sur(trace: &[(f64, f64)], cumul: &[f64], s: f64) -> (f64, f64) {
    let total = *cumul.last().unwrap_or(&0.0);
    let s = s.clamp(0.0, total);
    let mut i = 0;
    while i + 2 < cumul.len() && cumul[i + 1] < s {
        i += 1;
    }
    let (x0, y0) = trace[i];
    let (x1, y1) = trace[i + 1];
    let longueur = cumul[i + 1] - cumul[i];
    let t = if longueur > f64::EPSILON {
        ((s - cumul[i]) / longueur).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t)
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
    styles: &StylesTrait,
    temps_s: f64,
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
        StyleCadre::MgStyle => dessiner_mg(factory, rt, geo, cfg, ep, &styles.cadre),
        StyleCadre::ThinLine => dessiner_thin(rt, geo, cfg, ep, &styles.cadre),
        StyleCadre::Carre | StyleCadre::Arrondis => {
            dessiner_simple(rt, cfg, ep, &styles.cadre)
        }
        StyleCadre::Tribal => dessiner_tribal(factory, rt, geo, cfg, ep, styles, temps_s),
        StyleCadre::Cyberpunk => {
            dessiner_cyberpunk(rt, geo, cfg, ep, &styles.cadre, temps_s)
        }
        StyleCadre::Story => dessiner_story(factory, rt, geo, cfg, ep, styles, temps_s),
        StyleCadre::Hexagon => dessiner_hexagon(factory, rt, geo, cfg, ep, styles, temps_s),
        StyleCadre::Arena => dessiner_arena(factory, rt, geo, cfg, ep, styles, temps_s),
    }
    .and_then(|()| {
        // Seuls les styles « comète » l'animent ici ; tribal/cyberpunk/story/
        // hexagon dessinent leur propre animation dans leur renderer (même
        // couche, donc même rognure que le `clip-path` du parent dans le DOM).
        if cfg.animer
            && matches!(
                cfg.style,
                StyleCadre::MgStyle
                    | StyleCadre::ThinLine
                    | StyleCadre::Carre
                    | StyleCadre::Arrondis
            )
        {
            dessiner_comete(factory, rt, cfg, ep, &styles.comete, temps_s)
        } else {
            Ok(())
        }
    });

    unsafe { ctx.PopLayer() };
    // `geometricMask` est en `ManuallyDrop` : la référence créée plus haut ne
    // serait jamais relâchée (une fuite par frame).
    unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
    resultat
}

/// Comète `animer` : un segment de 7 % du périmètre, en rotation (4 s/tour),
/// dans `couleurFin`, épaisseur `max(1.2, 0.7·ep)` — miroir du SMIL
/// `stroke-dashoffset` sur `pathLength="100"`.
///
/// Le tracé est celui du SVG du style (rectangle / rectangle arrondi), pas le
/// polygone de découpe : aux coins coupés, la comète sort du polygone et y
/// est rognée par la couche — comme le `clip-path` du parent dans le DOM.
///
/// Une géométrie est créée par frame et par cadre animé (à mesurer si
/// l'encodeur cesse d'être le facteur limitant).
fn dessiner_comete(
    factory: &ID2D1Factory1,
    rt: &ID2D1RenderTarget,
    cfg: &CadreCfg,
    ep: f32,
    style_comete: &ID2D1StrokeStyle,
    temps_s: f64,
) -> Result<(), ErreurCadre> {
    let trace = trace_comete(cfg);
    let points = comete_points(&trace, temps_s / DUREE_COMETE_S, FRACTION_COMETE);
    if points.len() < 2 {
        return Ok(());
    }
    let geo = geometrie_ouverte(factory, &points)?;
    let brosse = brosse_unie(rt, cfg.couleur_fin, 1.0)?;
    trait_geo(
        rt,
        &geo,
        &en_brosse(&brosse)?,
        (ep * 0.7).max(1.2),
        style_comete,
    )
}

/// `carre` / `arrondis` : un seul trait au dégradé (`renderCadreInline` du
/// DOM — rect `rx = 8` pour `arrondis`, 0 sinon), puis la comète éventuelle.
///
/// Le trait est centré sur la boîte (0,0 → w,h) : sa moitié extérieure est
/// rognée par la couche, comme le `clip-path` du parent dans le DOM.
fn dessiner_simple(
    rt: &ID2D1RenderTarget,
    cfg: &CadreCfg,
    ep: f32,
    style_trait: &ID2D1StrokeStyle,
) -> Result<(), ErreurCadre> {
    let (x, y, w, h) = cfg.rect;
    let brosse = brosse_degrade(rt, cfg)?;
    let b = en_brosse(&brosse)?;
    let rect = D2D_RECT_F {
        left: x as f32,
        top: y as f32,
        right: (x + w) as f32,
        bottom: (y + h) as f32,
    };
    match cfg.style {
        StyleCadre::Arrondis => {
            // SVG borne `rx` à W/2 (H/2) ; le DOM écrit 8 en dur.
            let r = 8.0_f32.min(w as f32 / 2.0).min(h as f32 / 2.0);
            let rr = D2D1_ROUNDED_RECT {
                rect,
                radiusX: r,
                radiusY: r,
            };
            unsafe { rt.DrawRoundedRectangle(&rr, &b, ep, style_trait) };
        }
        _ => unsafe { rt.DrawRectangle(&rect, &b, ep, style_trait) },
    }
    Ok(())
}

/// Un sous-tracé aplati (points canvas).
type Trace = Vec<(f64, f64)>;
/// Motif d'un bord : corps pleins + balayures.
type MotifBord = (Vec<Trace>, Vec<Trace>);

/// Géométrie à plusieurs figures (motifs `tribal`) — un `BeginFigure` par
/// sous-tracé, rempli en `ALTERNATE` comme le `fill` SVG. `ferme = false`
/// pour les équerres (traits ouverts).
fn geometrie_figures(
    factory: &ID2D1Factory1,
    figures: &[Trace],
    ferme: bool,
) -> Result<ID2D1PathGeometry, ErreurCadre> {
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry() }
        .and_then(|g| g.cast())
        .map_err(|e| ErreurCadre::Systeme(format!("CreatePathGeometry : {}", e.message())))?;
    let sink: ID2D1GeometrySink = unsafe { geo.Open() }
        .map_err(|e| ErreurCadre::Systeme(format!("Open : {}", e.message())))?;
    unsafe {
        sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
        for pts in figures {
            if pts.len() < 2 {
                continue;
            }
            sink.BeginFigure(
                point(pts[0]),
                if ferme {
                    D2D1_FIGURE_BEGIN_FILLED
                } else {
                    D2D1_FIGURE_BEGIN_HOLLOW
                },
            );
            for p in &pts[1..] {
                sink.AddLine(point(*p));
            }
            sink.EndFigure(if ferme {
                D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
        }
    }
    unsafe { sink.Close() }
        .map_err(|e| ErreurCadre::Systeme(format!("Close : {}", e.message())))?;
    Ok(geo)
}

/// Repère d'un bord du motif `tribal` (`motifBord`) : point le long du bord
/// + normale (profil vers l'intérieur).
struct Bord {
    x1: f64,
    y1: f64,
    dx: f64,
    dy: f64,
    nx: f64,
    ny: f64,
}

impl Bord {
    fn p(&self, t: f64, prof: f64) -> (f64, f64) {
        (
            self.x1 + self.dx * t + self.nx * prof,
            self.y1 + self.dy * t + self.ny * prof,
        )
    }
}

/// Pointe centrale du motif (`pointeCentrale`) — 4 quadratiques fermées.
fn pointe_centrale(b: &Bord, c: f64, w: f64, h: f64) -> Trace {
    let mut f = Aplatisseur::nouveau();
    let (x, y) = b.p(c, -h * 0.2);
    f.ligne(x, y);
    let (qx, qy) = b.p(c + w * 0.22, h * 0.3);
    let (ex, ey) = b.p(c + w, h * 0.4);
    f.quadratique(qx, qy, ex, ey);
    let (qx, qy) = b.p(c + w * 0.26, h * 0.5);
    let (ex, ey) = b.p(c, h);
    f.quadratique(qx, qy, ex, ey);
    let (qx, qy) = b.p(c - w * 0.26, h * 0.5);
    let (ex, ey) = b.p(c - w, h * 0.4);
    f.quadratique(qx, qy, ex, ey);
    let (qx, qy) = b.p(c - w * 0.22, h * 0.3);
    let (ex, ey) = b.p(c, -h * 0.2);
    f.quadratique(qx, qy, ex, ey);
    f.fermer();
    f.points
}

/// Griffe du motif (`griffe`) — 2 quadratiques fermées.
fn griffe(b: &Bord, t0: f64, s: f64, w: f64, h: f64, l: f64) -> Trace {
    let mut f = Aplatisseur::nouveau();
    let (x, y) = b.p(t0, h * 0.06);
    f.ligne(x, y);
    let (qx, qy) = b.p(t0 + s * l * 0.3, h * 1.05);
    let (ex, ey) = b.p(t0 + s * l, h * 0.12);
    f.quadratique(qx, qy, ex, ey);
    let (qx, qy) = b.p(t0 + s * l * 0.55, h * 0.4);
    let (ex, ey) = b.p(t0 + s * w, h * 0.02);
    f.quadratique(qx, qy, ex, ey);
    f.fermer();
    f.points
}

/// Balayure fine du motif (`balayure`) — 2 quadratiques fermées.
fn balayure(b: &Bord, t0: f64, t1: f64, peak: f64, ep: f64) -> Trace {
    let mid = (t0 + t1) / 2.0;
    let th = (ep * 0.3).max(0.9);
    let mut f = Aplatisseur::nouveau();
    let (x, y) = b.p(t0, ep * 0.35);
    f.ligne(x, y);
    let (qx, qy) = b.p(mid, peak);
    let (ex, ey) = b.p(t1, ep * 0.3);
    f.quadratique(qx, qy, ex, ey);
    let (qx, qy) = b.p(mid, peak + th);
    let (ex, ey) = b.p(t0, ep * 0.35 + th * 0.6);
    f.quadratique(qx, qy, ex, ey);
    f.fermer();
    f.points
}

/// Motif d'un bord (`motifBord`) : corps pleins + balayures.
fn motif_bord(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    nx: f64,
    ny: f64,
    ep: f64,
) -> MotifBord {
    let mut len = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    if len == 0.0 {
        len = 1.0;
    }
    let b = Bord {
        x1,
        y1,
        dx: (x2 - x1) / len,
        dy: (y2 - y1) / len,
        nx,
        ny,
    };
    let h_max = (ep * 2.4).max(9.0);
    let pad = h_max * 1.9;
    let demi = len / 2.0 - pad;
    if demi < h_max * 1.5 {
        return (Vec::new(), Vec::new());
    }
    let c = len / 2.0;
    let mut corps = Vec::new();
    let mut balayures = Vec::new();
    let sw = h_max * 0.38;
    corps.push(pointe_centrale(&b, c, sw, h_max * 1.1));
    let n_griffe = if demi > h_max * 3.6 { 2 } else { 1 };
    for si in 0..2 {
        let s = if si == 0 { 1.0 } else { -1.0 };
        let mut cur = c + s * (sw + h_max * 0.55);
        for i in 0..n_griffe {
            let k = 1.0 - i as f64 * 0.3;
            corps.push(griffe(
                &b,
                cur,
                s,
                h_max * 0.42 * k,
                h_max * k,
                h_max * 1.45 * k + h_max * 0.4,
            ));
            cur += s * (h_max * 1.85 * k + h_max * 0.45);
        }
        let b0 = cur + s * h_max * 0.2;
        let b1 = if s > 0.0 { len - pad * 0.25 } else { pad * 0.25 };
        if (b1 - b0).abs() > h_max * 1.6 {
            balayures.push(balayure(&b, b0, b1, -ep * 0.15, ep));
            balayures.push(balayure(&b, b0 + s * h_max * 0.15, b1, h_max * 0.55, ep));
        }
    }
    (corps, balayures)
}

/// Équerre de visée d'un coin (`equerre`) — trait ouvert.
struct Equerre {
    cx0: f64,
    cy0: f64,
    sx: f64,
    sy: f64,
    ix: f64,
    iy: f64,
    ax: f64,
    ay: f64,
}

impl Equerre {
    fn points(&self, r: f64) -> Vec<(f64, f64)> {
        let x = |v: f64| self.cx0 + v * self.sx;
        let y = |v: f64| self.cy0 + v * self.sy;
        let mut f = Aplatisseur::nouveau();
        f.ligne(x(self.ix + self.ax), y(self.iy));
        f.ligne(x(self.ix + r), y(self.iy));
        f.quadratique(x(self.ix), y(self.iy), x(self.ix), y(self.iy + r));
        f.ligne(x(self.ix), y(self.iy + self.ay));
        f.points
    }
}

/// `tribal` : 4 traits de bord, motif (corps + balayures), équerres — le
/// tout clippé au bord. Le nœud voyageur est dessiné **hors** couche
/// (`dessiner_voyageur`), comme dans le DOM.
fn dessiner_tribal(
    factory: &ID2D1Factory1,
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    cfg: &CadreCfg,
    ep: f32,
    styles: &StylesTrait,
    temps_s: f64,
) -> Result<(), ErreurCadre> {
    let (x, y, w, h) = cfg.rect;
    let principal = cfg.couleur_fin;
    let clair = eclaircir(principal, 0.4);
    let ep64 = ep as f64;

    // 4 traits de bord : halo sombre, principal 22 %, dégradé, filet clair.
    let halo = brosse_unie(rt, [6, 6, 6, 255], 0.5)?;
    trait_geo(rt, geo, &en_brosse(&halo)?, ep * 3.2, &styles.cadre)?;
    let voile = brosse_unie(rt, principal, 0.22)?;
    trait_geo(rt, geo, &en_brosse(&voile)?, ep * 2.4, &styles.cadre)?;
    let degrade = brosse_degrade(rt, cfg)?;
    trait_geo(rt, geo, &en_brosse(&degrade)?, ep, &styles.cadre)?;
    let filet = brosse_unie(rt, clair, 0.45)?;
    trait_geo(
        rt,
        geo,
        &en_brosse(&filet)?,
        (ep * 0.2).max(0.8),
        &styles.cadre,
    )?;

    // Motif des 4 bords (profils vers l'intérieur).
    let (cx, cy) = tribal_corner(ep64, w, h);
    let mut corps: Vec<Trace> = Vec::new();
    let mut balayures: Vec<Trace> = Vec::new();
    for (c, b) in [
        motif_bord(x + cx * 1.15, y, x + w - cx * 1.15, y, 0.0, 1.0, ep64),
        motif_bord(
            x + w,
            y + cy * 1.15,
            x + w,
            y + h - cy * 1.15,
            -1.0,
            0.0,
            ep64,
        ),
        motif_bord(
            x + w - cx * 1.15,
            y + h,
            x + cx * 1.15,
            y + h,
            0.0,
            -1.0,
            ep64,
        ),
        motif_bord(x, y + h - cy * 1.15, x, y + cy * 1.15, 1.0, 0.0, ep64),
    ] {
        corps.extend(c);
        balayures.extend(b);
    }
    // Respiration du groupe (`animate opacity 0.85;1;0.85 dur=4s`).
    let alpha = if cfg.animer {
        let phase = temps_s / 4.0;
        let triangle = 1.0 - (2.0 * phase.rem_euclid(1.0) - 1.0).abs();
        (0.85 + 0.15 * triangle) as f32
    } else {
        1.0
    };
    if !corps.is_empty() {
        let g = geometrie_figures(factory, &corps, true)?;
        let b = en_brosse(&degrade)?;
        unsafe { b.SetOpacity(alpha) };
        unsafe { rt.FillGeometry(&g, &b, None) };
    }
    if !balayures.is_empty() {
        let g = geometrie_figures(factory, &balayures, true)?;
        let b = en_brosse(&degrade)?;
        unsafe { b.SetOpacity(0.85 * alpha) };
        unsafe { rt.FillGeometry(&g, &b, None) };
    }
    if let Ok(b) = en_brosse(&degrade) {
        unsafe { b.SetOpacity(1.0) };
    }

    // Équerres de visée (4 coins), seulement sur une boîte assez grande.
    if w > 130.0 && h > 90.0 {
        let ix = (w * 0.09).min((cx * 1.6).max(ep64 * 2.6));
        let iy = (h * 0.11).min((cy * 1.6).max(ep64 * 2.6));
        let ax = (w * 0.045).max(12.0);
        let ay = (h * 0.05).max(9.0);
        let figures = vec![
            Equerre { cx0: x, cy0: y, sx: 1.0, sy: 1.0, ix, iy, ax, ay }.points(2.5),
            Equerre { cx0: x + w, cy0: y, sx: -1.0, sy: 1.0, ix, iy, ax, ay }.points(2.5),
            Equerre { cx0: x + w, cy0: y + h, sx: -1.0, sy: -1.0, ix, iy, ax, ay }.points(2.5),
            Equerre { cx0: x, cy0: y + h, sx: 1.0, sy: -1.0, ix, iy, ax, ay }.points(2.5),
        ];
        let g = geometrie_figures(factory, &figures, false)?;
        let b = brosse_unie(rt, clair, 0.85)?;
        trait_geo(
            rt,
            &g,
            &en_brosse(&b)?,
            (ep * 0.45).max(1.3),
            &styles.comete,
        )?;
    }
    if cfg.animer {
        dessiner_voyageur(rt, cfg, ep, temps_s)?;
    }
    Ok(())
}

/// Nœud voyageur `tribal` (`animateMotion dur="5.5s"` sur le bord) : halo
/// dans la couleur claire + point blanc, **hors** du clip du cadre.
fn dessiner_voyageur(
    rt: &ID2D1RenderTarget,
    cfg: &CadreCfg,
    ep: f32,
    temps_s: f64,
) -> Result<(), ErreurCadre> {
    let trace = cfg.polygone();
    let points = comete_points(&trace, temps_s / 5.5, 0.0);
    let Some(&(px, py)) = points.first() else {
        return Ok(());
    };
    let dot = (ep * 0.5).max(1.8);
    let clair = eclaircir(cfg.couleur_fin, 0.4);
    let halo = brosse_unie(rt, clair, 0.35)?;
    let centre = D2D_POINT_2F {
        x: px as f32,
        y: py as f32,
    };
    let h = D2D1_ELLIPSE {
        point: centre,
        radiusX: dot * 2.4,
        radiusY: dot * 2.4,
    };
    unsafe { rt.FillEllipse(&h, &en_brosse(&halo)?) };
    let blanc = brosse_unie(rt, [255, 255, 255, 255], 1.0)?;
    let c = D2D1_ELLIPSE {
        point: centre,
        radiusX: dot,
        radiusY: dot,
    };
    unsafe { rt.FillEllipse(&c, &en_brosse(&blanc)?) };
    Ok(())
}

/// `cyberpunk` : 4 traits de bord (néon 22 %, dégradé, liseré intérieur
/// blanc/neon, filet blanc). Les ornements de marge (équerres, circuits,
/// code-barres) sont **invisibles** dans la référence — rognés par le
/// `clip-path` du parent — donc non portés (même leçon que mgstyle).
fn dessiner_cyberpunk(
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    cfg: &CadreCfg,
    ep: f32,
    style_trait: &ID2D1StrokeStyle,
    temps_s: f64,
) -> Result<(), ErreurCadre> {
    let c1 = cfg.couleur_fin;
    let c3 = [255, 255, 255, 255];
    let neon = brosse_unie(rt, c1, 0.22)?;
    trait_geo(rt, geo, &en_brosse(&neon)?, ep * 2.6, style_trait)?;
    let degrade = brosse_degrade(rt, cfg)?;
    trait_geo(rt, geo, &en_brosse(&degrade)?, ep, style_trait)?;
    // Liseré intérieur : blanc 0.9 → néon 0.6 → blanc 0.9, opacité 0.85.
    let inner = brosse_degrade_stops(
        rt,
        cfg,
        &[(0.0, c3, 0.9), (0.5, c1, 0.6), (1.0, c3, 0.9)],
    )?;
    let bi = en_brosse(&inner)?;
    unsafe { bi.SetOpacity(0.85) };
    trait_geo(rt, geo, &bi, (ep * 0.35).max(1.0), style_trait)?;
    let filet = brosse_unie(rt, c3, 0.4)?;
    trait_geo(
        rt,
        geo,
        &en_brosse(&filet)?,
        (ep * 0.2).max(0.8),
        style_trait,
    )?;
    if cfg.animer {
        dessiner_runner_cyberpunk(rt, cfg, ep, temps_s)?;
    }
    Ok(())
}

/// Runner `cyberpunk` (`animateMotion dur="5s"` sur le bord biseauté) :
/// halo néon + point blanc, **hors** du clip (groupe `opacity 0.95`).
fn dessiner_runner_cyberpunk(
    rt: &ID2D1RenderTarget,
    cfg: &CadreCfg,
    ep: f32,
    temps_s: f64,
) -> Result<(), ErreurCadre> {
    let trace = cfg.polygone();
    let points = comete_points(&trace, temps_s / 5.0, 0.0);
    let Some(&(px, py)) = points.first() else {
        return Ok(());
    };
    let dot = (ep * 0.55).max(1.8);
    let c1 = cfg.couleur_fin;
    let centre = D2D_POINT_2F {
        x: px as f32,
        y: py as f32,
    };
    let halo = brosse_unie(rt, c1, 0.35 * 0.95)?;
    let h = D2D1_ELLIPSE {
        point: centre,
        radiusX: dot * 2.6,
        radiusY: dot * 2.6,
    };
    unsafe { rt.FillEllipse(&h, &en_brosse(&halo)?) };
    let blanc = brosse_unie(rt, [255, 255, 255, 255], 0.95)?;
    let c = D2D1_ELLIPSE {
        point: centre,
        radiusX: dot,
        radiusY: dot,
    };
    unsafe { rt.FillEllipse(&c, &en_brosse(&blanc)?) };
    Ok(())
}

/// Cœur `story` (`heartShape`) — 2 cubiques fermées.
fn heart_points(cx: f64, cy: f64, s: f64) -> Trace {
    let mut f = Aplatisseur::nouveau();
    f.ligne(cx, cy + s * 0.25);
    f.cubique(
        cx - s * 0.42,
        cy - s * 0.08,
        cx - s * 0.42,
        cy - s * 0.42,
        cx,
        cy - s * 0.18,
    );
    f.cubique(
        cx + s * 0.42,
        cy - s * 0.42,
        cx + s * 0.42,
        cy - s * 0.08,
        cx,
        cy + s * 0.25,
    );
    f.fermer();
    f.points
}

/// Étincelle 4-pointes `story` (`sparkleShape`) — 4 quadratiques fermées.
fn sparkle_points(cx: f64, cy: f64, r: f64) -> Trace {
    let k = r * 0.16;
    let mut f = Aplatisseur::nouveau();
    f.ligne(cx, cy - r);
    f.quadratique(cx + k, cy - k, cx + r, cy);
    f.quadratique(cx + k, cy + k, cx, cy + r);
    f.quadratique(cx - k, cy + k, cx - r, cy);
    f.quadratique(cx - k, cy - k, cx, cy - r);
    f.fermer();
    f.points
}

/// Volute de coin `story` (`voluteCoin`) — trait ouvert + point final.
fn volute_points(cx: f64, cy: f64, sx: f64, sy: f64, m: f64) -> (Trace, (f64, f64)) {
    let x = |v: f64| cx + v * sx;
    let y = |v: f64| cy + v * sy;
    let mut f = Aplatisseur::nouveau();
    f.ligne(x(-m * 0.05), y(-m * 0.1));
    f.cubique(
        x(-m * 0.6),
        y(-m * 0.15),
        x(-m * 0.85),
        y(-m * 0.7),
        x(-m * 0.45),
        y(-m * 0.9),
    );
    f.cubique(
        x(-m * 0.15),
        y(-m * 1.05),
        x(0.0),
        y(-m * 0.75),
        x(-m * 0.12),
        y(-m * 0.5),
    );
    (f.points, (x(-m * 0.12), y(-m * 0.5)))
}

/// `story` : 3 traits festonnés, cœurs de coin et de bord, volutes de coin
/// (rognées au bord comme le parent), étincelle voyageuse (7 s). Les
/// étincelles de marge sont **hors polygone** → invisibles dans la
/// référence, donc non portées.
fn dessiner_story(
    factory: &ID2D1Factory1,
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    cfg: &CadreCfg,
    ep: f32,
    styles: &StylesTrait,
    temps_s: f64,
) -> Result<(), ErreurCadre> {
    let (x, y, w, h) = cfg.rect;
    let vif = cfg.couleur_fin;
    let clair = eclaircir(vif, 0.35);
    let pale = eclaircir(vif, 0.75);
    let blanc = [255, 255, 255, 255];
    let ep64 = ep as f64;
    let s = story_clip_size(ep64).min(w / 4.0).min(h / 4.0);
    let marge = (ep64 * 3.0).max(12.0);

    let halo = brosse_unie(rt, vif, 0.2)?;
    trait_geo(rt, geo, &en_brosse(&halo)?, ep * 2.0, &styles.comete)?;
    let degrade = brosse_degrade(rt, cfg)?;
    trait_geo(rt, geo, &en_brosse(&degrade)?, ep, &styles.comete)?;
    let inner = brosse_degrade_stops(
        rt,
        cfg,
        &[(0.0, blanc, 0.7), (0.5, pale, 0.5), (1.0, blanc, 0.7)],
    )?;
    let bi = en_brosse(&inner)?;
    unsafe { bi.SetOpacity(0.7) };
    trait_geo(rt, geo, &bi, (ep * 0.3).max(1.0), &styles.comete)?;

    // Cœurs de coin (4), blanc 0.92.
    let taille_coin = (ep64 * 1.5).max(6.0);
    let figures: Vec<Trace> = [
        (x + s * 0.25, y + s * 0.25),
        (x + w - s * 0.25, y + s * 0.25),
        (x + s * 0.25, y + h - s * 0.25),
        (x + w - s * 0.25, y + h - s * 0.25),
    ]
    .iter()
    .map(|(cx, cy)| heart_points(*cx, *cy, taille_coin))
    .collect();
    let g = geometrie_figures(factory, &figures, true)?;
    let b = brosse_unie(rt, blanc, 0.92)?;
    unsafe { rt.FillGeometry(&g, &en_brosse(&b)?, None) };

    // Petits cœurs des bords, groupe blanc 0.75.
    let taille_petit = (ep64 * 0.7).max(3.0);
    let safe_start = s + (ep64 * 3.0).max(15.0);
    let safe_end_top = w - s - (ep64 * 3.0).max(15.0);
    let safe_end_side = h - s - (ep64 * 3.0).max(15.0);
    let n_top = ((safe_end_top - safe_start) / 70.0).floor().max(0.0) as usize;
    let n_side = ((safe_end_side - safe_start) / 70.0).floor().max(0.0) as usize;
    let mut petits: Vec<Trace> = Vec::new();
    for i in 0..n_top {
        let t = safe_start + (i + 1) as f64 * (safe_end_top - safe_start) / (n_top + 1) as f64;
        petits.push(heart_points(x + t, y, taille_petit));
        petits.push(heart_points(x + t, y + h, taille_petit));
    }
    for j in 0..n_side {
        let t = safe_start + (j + 1) as f64 * (safe_end_side - safe_start) / (n_side + 1) as f64;
        petits.push(heart_points(x, y + t, taille_petit));
        petits.push(heart_points(x + w, y + t, taille_petit));
    }
    if !petits.is_empty() {
        let g = geometrie_figures(factory, &petits, true)?;
        let b = brosse_unie(rt, blanc, 0.75)?;
        unsafe { rt.FillGeometry(&g, &en_brosse(&b)?, None) };
    }

    // Volutes de coin (traits clairs + point final), rognées au bord.
    let trait_fin = (ep * 0.3).max(0.9);
    let b_volute = brosse_unie(rt, clair, 0.9)?;
    let b_point = brosse_unie(rt, blanc, 0.9)?;
    let r_point = (ep64 * 0.28).max(1.0) as f32;
    for (cx, cy, sx, sy) in [
        (x, y, 1.0, 1.0),
        (x + w, y, -1.0, 1.0),
        (x + w, y + h, -1.0, -1.0),
        (x, y + h, 1.0, -1.0),
    ] {
        let (points, (px, py)) = volute_points(cx, cy, sx, sy, marge);
        let g = geometrie_ouverte(factory, &points)?;
        trait_geo(rt, &g, &en_brosse(&b_volute)?, trait_fin, &styles.comete)?;
        let e = D2D1_ELLIPSE {
            point: D2D_POINT_2F {
                x: px as f32,
                y: py as f32,
            },
            radiusX: r_point,
            radiusY: r_point,
        };
        unsafe { rt.FillEllipse(&e, &en_brosse(&b_point)?) };
    }

    // Étincelle voyageuse (`animateMotion dur="7s"`), groupe 0.95.
    if cfg.animer {
        let trace = cfg.polygone();
        let pts = comete_points(&trace, temps_s / 7.0, 0.0);
        if let Some(&(px, py)) = pts.first() {
            let r = (ep64 * 0.9).max(3.5);
            let g = geometrie_figures(factory, &[sparkle_points(px, py, r)], true)?;
            let b = brosse_unie(rt, blanc, 0.95)?;
            unsafe { rt.FillGeometry(&g, &en_brosse(&b)?, None) };
        }
    }
    Ok(())
}

/// `hexagon` : 3 traits de bord, nœuds aux 8 sommets, cadre intérieur inséré
/// et nœud voyageur (6 s). Les grappes d'alvéoles et glyphes de marge sont
/// **hors polygone** → invisibles dans la référence, non portés.
fn dessiner_hexagon(
    factory: &ID2D1Factory1,
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    cfg: &CadreCfg,
    ep: f32,
    styles: &StylesTrait,
    temps_s: f64,
) -> Result<(), ErreurCadre> {
    let (x, y, w, h) = cfg.rect;
    let c1 = cfg.couleur_fin;
    let clair = eclaircir(c1, 0.45);
    let ep64 = ep as f64;

    let principal = brosse_unie(rt, c1, 0.2)?;
    trait_geo(rt, geo, &en_brosse(&principal)?, ep * 2.2, &styles.cadre)?;
    let degrade = brosse_degrade(rt, cfg)?;
    trait_geo(rt, geo, &en_brosse(&degrade)?, ep, &styles.cadre)?;
    let filet = brosse_unie(rt, clair, 0.6)?;
    trait_geo(
        rt,
        geo,
        &en_brosse(&filet)?,
        (ep * 0.3).max(1.0),
        &styles.cadre,
    )?;

    // Nœuds aux 8 sommets (groupe 0.85) — sur le bord, rognés comme le parent.
    let local = points_polygone(StyleCadre::Hexagon, w, h, cfg.stroke_width);
    let noeud = brosse_unie(rt, clair, 0.85)?;
    let r = (ep64 * 0.3).max(1.2) as f32;
    for (px, py) in local.iter().take(local.len().saturating_sub(1)) {
        let e = D2D1_ELLIPSE {
            point: D2D_POINT_2F {
                x: (x + px) as f32,
                y: (y + py) as f32,
            },
            radiusX: r,
            radiusY: r,
        };
        unsafe { rt.FillEllipse(&e, &en_brosse(&noeud)?) };
    }

    // Cadre intérieur inséré (`translate(0.6ep) scale(1 - 1.2ep/W, …)`).
    let t = ep64 * 0.6;
    let sx = (1.0 - (ep64 * 1.2) / w).max(0.01);
    let sy = (1.0 - (ep64 * 1.2) / h).max(0.01);
    let inner: Vec<(f64, f64)> = local
        .iter()
        .map(|(px, py)| (x + t + sx * px, y + t + sy * py))
        .collect();
    let g = geometrie_fermee(factory, &inner)?;
    let b = brosse_unie(rt, clair, 0.4)?;
    trait_geo(rt, &g, &en_brosse(&b)?, (ep * 0.2).max(1.0), &styles.cadre)?;

    // Nœud voyageur (`animateMotion dur="6s"`).
    if cfg.animer {
        let trace = cfg.polygone();
        let pts = comete_points(&trace, temps_s / 6.0, 0.0);
        if let Some(&(px, py)) = pts.first() {
            let centre = D2D_POINT_2F {
                x: px as f32,
                y: py as f32,
            };
            let halo = brosse_unie(rt, clair, 0.4)?;
            let rh = (ep64 * 0.5).max(2.2) as f32;
            let h1 = D2D1_ELLIPSE {
                point: centre,
                radiusX: rh,
                radiusY: rh,
            };
            unsafe { rt.FillEllipse(&h1, &en_brosse(&halo)?) };
            let blanc = brosse_unie(rt, [255, 255, 255, 255], 1.0)?;
            let rc = (ep64 * 0.32).max(1.4) as f32;
            let c = D2D1_ELLIPSE {
                point: centre,
                radiusX: rc,
                radiusY: rc,
            };
            unsafe { rt.FillEllipse(&c, &en_brosse(&blanc)?) };
        }
    }
    Ok(())
}

/// Dégradé radial (orbs `arena`) : centre + rayon explicites.
fn brosse_radiale(
    rt: &ID2D1RenderTarget,
    centre: (f32, f32),
    rayon: f32,
    arrets: &[(f32, [u8; 4], f32)],
) -> Result<ID2D1RadialGradientBrush, ErreurCadre> {
    let stops: Vec<D2D1_GRADIENT_STOP> = arrets
        .iter()
        .map(|(position, couleur, alpha)| D2D1_GRADIENT_STOP {
            position: *position,
            color: couleur_d2d(*couleur, *alpha),
        })
        .collect();
    let collection: ID2D1GradientStopCollection = unsafe {
        rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }
    .map_err(|e| ErreurCadre::Systeme(format!("CreateGradientStopCollection : {}", e.message())))?;
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
        center: D2D_POINT_2F {
            x: centre.0,
            y: centre.1,
        },
        gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
        radiusX: rayon,
        radiusY: rayon,
    };
    unsafe { rt.CreateRadialGradientBrush(&props, None, &collection) }
        .map_err(|e| ErreurCadre::Systeme(format!("CreateRadialGradientBrush : {}", e.message())))
}

/// Cartouche `arena` : plaque hexagonale + barre + deux points.
fn arena_cartouche(
    cx: f64,
    cy: f64,
    tx: f64,
    ty: f64,
    cw: f64,
    ch: f64,
    cd: f64,
) -> (Trace, Trace, (f64, f64), (f64, f64)) {
    let nx = -ty;
    let ny = tx;
    let p = |t: f64, n: f64| (cx + tx * t + nx * n, cy + ty * t + ny * n);
    let hexa = vec![
        p(-cw + cd, -ch),
        p(cw - cd, -ch),
        p(cw, 0.0),
        p(cw - cd, ch),
        p(-cw + cd, ch),
        p(-cw, 0.0),
        p(-cw + cd, -ch),
    ];
    let barre = vec![p(-cw * 0.5, ch * 0.35), p(cw * 0.5, ch * 0.35)];
    (
        hexa,
        barre,
        p(-cw + cd * 1.2, ch * 0.35),
        p(cw - cd * 1.2, ch * 0.35),
    )
}

/// Aileron `arena` : cerf-volant plein orienté par sa normale.
fn arena_aileron(cx: f64, cy: f64, nx: f64, ny: f64, wing_l: f64, wing_t: f64) -> Trace {
    let tx = -ny;
    let ty = nx;
    let p = |t: f64, n: f64| (cx + tx * t + nx * n, cy + ty * t + ny * n);
    vec![
        p(-wing_t, -wing_l * 0.35),
        p(0.0, wing_l),
        p(wing_t, -wing_l * 0.35),
        p(0.0, -wing_l),
        p(-wing_t, -wing_l * 0.35),
    ]
}

/// `arena` : canal sombre + tube double filet, écho intérieur, nœuds aux
/// sommets, cartouches/ailerons, 4 orbs (anneau + croix X + radial) et nœud
/// voyageur. Tout est clippé au bord comme le `clip-path` du parent.
fn dessiner_arena(
    factory: &ID2D1Factory1,
    rt: &ID2D1RenderTarget,
    geo: &ID2D1PathGeometry,
    cfg: &CadreCfg,
    ep: f32,
    styles: &StylesTrait,
    temps_s: f64,
) -> Result<(), ErreurCadre> {
    let (x, y, w, h) = cfg.rect;
    let principal = cfg.couleur_fin;
    let profond = cfg.couleur;
    let clair = eclaircir(principal, 0.5);
    let ep64 = ep as f64;
    let (bx, by) = arena_bevels(cfg.stroke_width, w, h);
    let sombre = [6u8, 10, 4, 255];

    let canal = brosse_unie(rt, sombre, 0.55)?;
    trait_geo(rt, geo, &en_brosse(&canal)?, ep * 3.4, &styles.cadre)?;
    let voile = brosse_unie(rt, principal, 0.28)?;
    trait_geo(rt, geo, &en_brosse(&voile)?, ep * 2.6, &styles.cadre)?;
    let degrade = brosse_degrade(rt, cfg)?;
    trait_geo(rt, geo, &en_brosse(&degrade)?, ep, &styles.cadre)?;
    let filet = brosse_unie(rt, clair, 0.5)?;
    trait_geo(
        rt,
        geo,
        &en_brosse(&filet)?,
        (ep * 0.2).max(0.8),
        &styles.cadre,
    )?;

    // Écho intérieur (octogone inséré de d = max(2.5, 1.1·ep)).
    let d = (ep64 * 1.1).max(2.5);
    let di = d * 1.4;
    let echo = vec![
        (x + bx + di, y + d),
        (x + w - bx - di, y + d),
        (x + w - d, y + by + di),
        (x + w - d, y + h - by - di),
        (x + w - bx - di, y + h - d),
        (x + bx + di, y + h - d),
        (x + d, y + h - by - di),
        (x + d, y + by + di),
        (x + bx + di, y + d),
    ];
    let g_echo = geometrie_fermee(factory, &echo)?;
    let b_echo = brosse_unie(rt, clair, 0.75)?;
    trait_geo(
        rt,
        &g_echo,
        &en_brosse(&b_echo)?,
        (ep * 0.28).max(1.0),
        &styles.cadre,
    )?;

    // Nœuds aux 8 sommets.
    let local = points_polygone(StyleCadre::Arena, w, h, cfg.stroke_width);
    let noeud = brosse_unie(rt, clair, 1.0)?;
    let r_noeud = (ep64 * 0.35).max(1.3) as f32;
    for (px, py) in local.iter().take(local.len().saturating_sub(1)) {
        let e = D2D1_ELLIPSE {
            point: D2D_POINT_2F {
                x: (x + px) as f32,
                y: (y + py) as f32,
            },
            radiusX: r_noeud,
            radiusY: r_noeud,
        };
        unsafe { rt.FillEllipse(&e, &en_brosse(&noeud)?) };
    }

    // Cartouches + ailerons (seulement sur une boîte assez grande).
    let trait_fin = (ep64 * 0.35).max(1.2);
    let cw = (bx * 1.15).max(18.0);
    let ch = (ep64 * 1.7).max(4.5);
    let cd = cw * 0.3;
    let wing_l = (ep64 * 2.2).max(5.0);
    let wing_t = (ep64 * 3.0).max(7.0);
    if w > cw * 4.0 && h > wing_t * 4.0 {
        let b_plaque = brosse_unie(rt, sombre, 0.75)?;
        let b_aileron = brosse_unie(rt, sombre, 0.7)?;
        let b_trait = brosse_unie(rt, clair, 1.0)?;
        let b_barre = brosse_unie(rt, clair, 0.6)?;
        let b_point = brosse_unie(rt, clair, 1.0)?;
        let r_point = (ep64 * 0.45).max(1.4) as f32;
        for (cx, cy, tx, ty) in [
            (x + w * 0.72, y, 1.0, 0.0),
            (x + w / 2.0, y + h, 1.0, 0.0),
        ] {
            let (hexa, barre, pg, pd) = arena_cartouche(cx, cy, tx, ty, cw, ch, cd);
            let g = geometrie_fermee(factory, &hexa)?;
            unsafe { rt.FillGeometry(&g, &en_brosse(&b_plaque)?, None) };
            trait_geo(rt, &g, &en_brosse(&b_trait)?, trait_fin as f32, &styles.cadre)?;
            let gb = geometrie_ouverte(factory, &barre)?;
            trait_geo(
                rt,
                &gb,
                &en_brosse(&b_barre)?,
                (trait_fin * 0.7) as f32,
                &styles.cadre,
            )?;
            for (px, py) in [pg, pd] {
                let e = D2D1_ELLIPSE {
                    point: D2D_POINT_2F {
                        x: px as f32,
                        y: py as f32,
                    },
                    radiusX: r_point,
                    radiusY: r_point,
                };
                unsafe { rt.FillEllipse(&e, &en_brosse(&b_point)?) };
            }
        }
        for (cx, cy, nx, ny) in [
            (x, y + h / 2.0, 1.0, 0.0),
            (x + w, y + h / 2.0, -1.0, 0.0),
        ] {
            let kite = arena_aileron(cx, cy, nx, ny, wing_l, wing_t);
            let g = geometrie_fermee(factory, &kite)?;
            unsafe { rt.FillGeometry(&g, &en_brosse(&b_aileron)?, None) };
            trait_geo(rt, &g, &en_brosse(&b_trait)?, trait_fin as f32, &styles.cadre)?;
        }
    }

    // 4 orbs : anneau sombre + disque radial + croix X blanche.
    let orb_r = (ep64 * 2.0).max(6.0).max(bx.min(by) * 1.25);
    let orb_w = (ep64 * 0.55).max(1.6);
    let bevel_len = ((bx * bx + by * by).sqrt()).max(f64::EPSILON);
    let dec = (by / bevel_len) * orb_r * 0.22;
    let decy = (bx / bevel_len) * orb_r * 0.22;
    let centres = [
        (x + bx / 2.0 + dec, y + by / 2.0 + decy),
        (x + w - bx / 2.0 - dec, y + by / 2.0 + decy),
        (x + w - bx / 2.0 - dec, y + h - by / 2.0 - decy),
        (x + bx / 2.0 + dec, y + h - by / 2.0 - decy),
    ];
    let alpha_groupe = if cfg.animer {
        let phase = (temps_s / 3.5).rem_euclid(1.0);
        (0.55 + 0.45 * (2.0 * phase - 1.0).abs()) as f32
    } else {
        1.0
    };
    let b_anneau = brosse_unie(rt, sombre, 0.65 * alpha_groupe)?;
    let b_contour = brosse_unie(rt, clair, alpha_groupe)?;
    let b_croix = brosse_unie(rt, [255, 255, 255, 255], 0.95 * alpha_groupe)?;
    let xr = orb_r * 0.6;
    for (cx, cy) in centres {
        let centre = D2D_POINT_2F {
            x: cx as f32,
            y: cy as f32,
        };
        let ext = D2D1_ELLIPSE {
            point: centre,
            radiusX: orb_r as f32,
            radiusY: orb_r as f32,
        };
        unsafe { rt.FillEllipse(&ext, &en_brosse(&b_anneau)?) };
        unsafe { rt.DrawEllipse(&ext, &en_brosse(&b_contour)?, orb_w as f32, None) };
        let inter = D2D1_ELLIPSE {
            point: centre,
            radiusX: (orb_r - orb_w * 0.9) as f32,
            radiusY: (orb_r - orb_w * 0.9) as f32,
        };
        let radial = brosse_radiale(
            rt,
            (
                (cx - orb_r * 0.3) as f32,
                (cy - orb_r * 0.4) as f32,
            ),
            (orb_r * 1.5) as f32,
            &[(0.0, clair, 0.9 * alpha_groupe), (1.0, profond, 0.9 * alpha_groupe)],
        )?;
        unsafe { rt.FillEllipse(&inter, &en_brosse(&radial)?) };
        let mut f = Aplatisseur::nouveau();
        f.ligne(cx - xr, cy - xr);
        f.quadratique(cx, cy, cx + xr, cy + xr);
        let mut f2 = Aplatisseur::nouveau();
        f2.ligne(cx + xr, cy - xr);
        f2.quadratique(cx, cy, cx - xr, cy + xr);
        let g = geometrie_figures(factory, &[f.points, f2.points], false)?;
        trait_geo(
            rt,
            &g,
            &en_brosse(&b_croix)?,
            (ep64 * 0.5).max(1.4) as f32,
            &styles.comete,
        )?;
    }

    // Nœud voyageur (`animateMotion dur="5s"`).
    if cfg.animer {
        let trace = cfg.polygone();
        let pts = comete_points(&trace, temps_s / 5.0, 0.0);
        if let Some(&(px, py)) = pts.first() {
            let centre = D2D_POINT_2F {
                x: px as f32,
                y: py as f32,
            };
            let dot = (ep64 * 0.5).max(1.8);
            let halo = brosse_unie(rt, clair, 0.35)?;
            let h1 = D2D1_ELLIPSE {
                point: centre,
                radiusX: (dot * 2.4) as f32,
                radiusY: (dot * 2.4) as f32,
            };
            unsafe { rt.FillEllipse(&h1, &en_brosse(&halo)?) };
            let blanc = brosse_unie(rt, [255, 255, 255, 255], 1.0)?;
            let c = D2D1_ELLIPSE {
                point: centre,
                radiusX: dot as f32,
                radiusY: dot as f32,
            };
            unsafe { rt.FillEllipse(&c, &en_brosse(&blanc)?) };
        }
    }
    Ok(())
}

/// Anneau `cameraBord` d'une forme caméra dédiée (contrat 9) : trait
/// `clamp(ep, 1.5, 12)`, dégradé `stopsLiseres` du cadre (fallback
/// blanc→gris quand aucun cadre actif).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnneauCamera {
    pub epaisseur: f64,
    pub couleur: [u8; 4],
    pub couleur_fin: [u8; 4],
}

/// Forme porteuse de l'anneau : rect canvas + cercle inscrit + clip éventuel
/// (polygone du cercle ; `None` = rect `carre`).
pub struct AnneauRendu<'a> {
    pub rect: (f64, f64, f64, f64),
    pub cercle: bool,
    pub clip: Option<&'a ID2D1PathGeometry>,
}

/// Peint l'anneau `cameraBord`, **rogné à la forme** (la moitié extérieure
/// du trait est invisible, comme le `clip-path` du parent dans le DOM).
pub fn dessiner_anneau_camera(
    factory: &ID2D1Factory1,
    ctx: &ID2D1DeviceContext,
    rt: &ID2D1RenderTarget,
    forme: &AnneauRendu,
    anneau: &AnneauCamera,
    style_trait: &ID2D1StrokeStyle,
) -> Result<(), ErreurCadre> {
    let _ = factory;
    let (x, y, w, h) = forme.rect;
    if w <= 0.0 || h <= 0.0 {
        return Ok(());
    }
    let ep = anneau.epaisseur.clamp(1.5, 12.0) as f32;
    // Dégradé diagonal TL→BR (x1 0% y1 0% x2 100% y2 100% du SVG).
    let stops: Vec<D2D1_GRADIENT_STOP> = stops_liseres(anneau.couleur, anneau.couleur_fin)
        .iter()
        .map(|(position, couleur)| D2D1_GRADIENT_STOP {
            position: *position,
            color: couleur_d2d(*couleur, 1.0),
        })
        .collect();
    let collection: ID2D1GradientStopCollection = unsafe {
        rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }
    .map_err(|e| ErreurCadre::Systeme(format!("CreateGradientStopCollection : {}", e.message())))?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: D2D_POINT_2F {
            x: x as f32,
            y: y as f32,
        },
        endPoint: D2D_POINT_2F {
            x: (x + w) as f32,
            y: (y + h) as f32,
        },
    };
    let brosse: ID2D1LinearGradientBrush =
        unsafe { rt.CreateLinearGradientBrush(&props, None, &collection) }
            .map_err(|e| ErreurCadre::Systeme(format!("CreateLinearGradientBrush : {}", e.message())))?;
    let b = en_brosse(&brosse)?;

    match forme.clip {
        Some(geo) => {
            let masque: windows::Win32::Graphics::Direct2D::ID2D1Geometry = geo
                .cast()
                .map_err(|e| ErreurCadre::Systeme(format!("ID2D1Geometry : {}", e.message())))?;
            let marge = ep as f64 + 1.0;
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
            let r = trait_anneau(rt, forme, ep, &b, style_trait);
            unsafe { ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
            r
        }
        None => {
            let clip = D2D_RECT_F {
                left: x as f32,
                top: y as f32,
                right: (x + w) as f32,
                bottom: (y + h) as f32,
            };
            unsafe { rt.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
            let r = trait_anneau(rt, forme, ep, &b, style_trait);
            unsafe { rt.PopAxisAlignedClip() };
            r
        }
    }
}

fn trait_anneau(
    rt: &ID2D1RenderTarget,
    forme: &AnneauRendu,
    ep: f32,
    brosse: &windows::Win32::Graphics::Direct2D::ID2D1Brush,
    style_trait: &ID2D1StrokeStyle,
) -> Result<(), ErreurCadre> {
    let (x, y, w, h) = forme.rect;
    if forme.cercle {
        let r = (w.min(h) / 2.0) as f32;
        let e = D2D1_ELLIPSE {
            point: D2D_POINT_2F {
                x: (x + w / 2.0) as f32,
                y: (y + h / 2.0) as f32,
            },
            radiusX: r,
            radiusY: r,
        };
        unsafe { rt.DrawEllipse(&e, brosse, ep, style_trait) };
    } else {
        let rect = D2D_RECT_F {
            left: x as f32,
            top: y as f32,
            right: (x + w) as f32,
            bottom: (y + h) as f32,
        };
        unsafe { rt.DrawRectangle(&rect, brosse, ep, style_trait) };
    }
    Ok(())
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
    let stops: Vec<(f32, [u8; 4], f32)> = stops_liseres(cfg.couleur, cfg.couleur_fin)
        .iter()
        .map(|(position, couleur)| (*position, *couleur, 1.0))
        .collect();
    brosse_degrade_stops(rt, cfg, &stops)
}

/// Dégradé linéaire à stops explicites `(position, couleur, alpha)` — même
/// géométrie que `brosse_degrade` (`angle_vers_coords`).
fn brosse_degrade_stops(
    rt: &ID2D1RenderTarget,
    cfg: &CadreCfg,
    arrets: &[(f32, [u8; 4], f32)],
) -> Result<ID2D1LinearGradientBrush, ErreurCadre> {
    let (x, y, w, h) = cfg.rect;
    let (x1, y1, x2, y2) = angle_vers_coords(cfg.gradient_angle);
    let stops: Vec<D2D1_GRADIENT_STOP> = arrets
        .iter()
        .map(|(position, couleur, alpha)| D2D1_GRADIENT_STOP {
            position: *position,
            color: couleur_d2d(*couleur, *alpha),
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
    fn carre_et_arrondis_sont_portes() {
        assert_eq!(StyleCadre::depuis_nom("carre"), Some(StyleCadre::Carre));
        assert_eq!(StyleCadre::depuis_nom("arrondis"), Some(StyleCadre::Arrondis));
        assert_eq!(StyleCadre::Carre.nom(), "carre");
        assert_eq!(StyleCadre::Arrondis.nom(), "arrondis");

        // `carre` : aucun clip dans le DOM → rectangle neutre (5 points).
        assert_eq!(
            points_polygone(StyleCadre::Carre, 100.0, 40.0, 4.0),
            vec![
                (0.0, 0.0),
                (100.0, 0.0),
                (100.0, 40.0),
                (0.0, 40.0),
                (0.0, 0.0)
            ]
        );

        // `arrondis` : rectangle arrondi r = min(8, W/4, H/4) — mêmes points
        // que le path SVG (`genererClipPathCadre`), 54 points comme thin-line.
        let arr = points_polygone(StyleCadre::Arrondis, 100.0, 40.0, 4.0);
        assert_eq!(arr.len(), 54);
        assert!(proche(arr[0], (8.0, 0.0)));
        // Petite boîte : le rayon est borné à W/4 (5 px pour 20×20).
        assert!(proche(
            points_polygone(StyleCadre::Arrondis, 20.0, 20.0, 4.0)[0],
            (5.0, 0.0)
        ));
    }

    #[test]
    fn trace_comete_carre_et_arrondis() {
        let carre = CadreCfg {
            rect: (10.0, 20.0, 100.0, 40.0),
            style: StyleCadre::Carre,
            stroke_width: 4.0,
            couleur: [0, 0, 0, 255],
            couleur_fin: [1, 2, 3, 255],
            gradient_angle: 135.0,
            z: 0,
            animer: true,
        };
        assert_eq!(
            trace_comete(&carre),
            vec![
                (10.0, 20.0),
                (110.0, 20.0),
                (110.0, 60.0),
                (10.0, 60.0),
                (10.0, 20.0)
            ]
        );
        // `arrondis` : la comète suit `rectPath` avec rx=8 (non borné au W/4
        // du clip) — premier point décalé de 8 px.
        let arr = CadreCfg {
            style: StyleCadre::Arrondis,
            ..carre.clone()
        };
        let t = trace_comete(&arr);
        assert!(proche(t[0], (18.0, 20.0)), "{:?}", t[0]);
        assert_eq!(t.len(), 54);
    }

    #[test]
    fn tribal_porte_et_suit_le_path() {
        assert_eq!(StyleCadre::depuis_nom("tribal"), Some(StyleCadre::Tribal));
        assert_eq!(StyleCadre::Tribal.nom(), "tribal");
        // `tribalPath` 200×100, trait 13 : coins (39, 31) bornés à W/4·H/4.
        let p = points_polygone(StyleCadre::Tribal, 200.0, 100.0, 13.0);
        assert_eq!(p.len(), 54);
        assert!(proche(p[0], (39.0, 0.0)), "{:?}", p[0]);
        assert!(proche(*p.last().expect("fermé"), (39.0, 0.0)));
        // Petite boîte : coins bornés à W/4 (25) et H/4 (10).
        let petit = points_polygone(StyleCadre::Tribal, 100.0, 40.0, 13.0);
        assert!(proche(petit[0], (25.0, 0.0)), "{:?}", petit[0]);
    }

    #[test]
    fn motif_tribal_suit_la_longueur_du_bord() {
        // Bord large : pointe + griffes + balayures (il faut ~700 px à
        // trait 13 pour que la condition de balayure du DOM soit vraie).
        let (corps, balayures) = motif_bord(0.0, 0.0, 700.0, 0.0, 0.0, 1.0, 13.0);
        assert!(!corps.is_empty());
        assert!(!balayures.is_empty());
        // Bord trop court (`demi < hMax * 1.5`) : rien, comme le DOM.
        let (corps, balayures) = motif_bord(0.0, 0.0, 100.0, 0.0, 0.0, 1.0, 13.0);
        assert!(corps.is_empty());
        assert!(balayures.is_empty());
    }

    #[test]
    fn equerre_tribal_est_un_trait_ouvert() {
        let e = Equerre {
            cx0: 0.0,
            cy0: 0.0,
            sx: 1.0,
            sy: 1.0,
            ix: 20.0,
            iy: 15.0,
            ax: 30.0,
            ay: 20.0,
        };
        // M + L + Q(12) + L = 15 points, ouvert (pas de fermeture).
        assert_eq!(e.points(2.5).len(), 15);
    }

    #[test]
    fn cyberpunk_porte_et_suit_le_biseau() {
        assert_eq!(
            StyleCadre::depuis_nom("cyberpunk"),
            Some(StyleCadre::Cyberpunk)
        );
        assert_eq!(StyleCadre::Cyberpunk.nom(), "cyberpunk");
        // bevel = max(8, round(3.5·20)) = 70, borné à W/4·H/4 (75 ici).
        let p = points_polygone(StyleCadre::Cyberpunk, 400.0, 300.0, 20.0);
        assert_eq!(p.len(), 9);
        assert_eq!(p[0], (70.0, 0.0));
        assert_eq!(p[2], (400.0, 70.0));
        assert_eq!(*p.last().expect("fermé"), (70.0, 0.0));
        // Petite boîte : biseau borné à H/4 (10 pour 100×40).
        let petit = points_polygone(StyleCadre::Cyberpunk, 100.0, 40.0, 20.0);
        assert_eq!(petit[0], (10.0, 0.0));
    }

    #[test]
    fn story_porte_et_suit_le_feston() {
        assert_eq!(StyleCadre::depuis_nom("story"), Some(StyleCadre::Story));
        assert_eq!(StyleCadre::Story.nom(), "story");
        // s = max(12, round(4·20)) = 80, borné à W/4·H/4 (75 en 400×300).
        let p = points_polygone(StyleCadre::Story, 400.0, 300.0, 20.0);
        assert_eq!(p.len(), 102);
        assert!(proche(p[0], (75.0, 0.0)), "{:?}", p[0]);
        assert!(proche(*p.last().expect("fermé"), (75.0, 0.0)));
        // Petite boîte : s borné à H/4 (10 pour 100×40).
        let petit = points_polygone(StyleCadre::Story, 100.0, 40.0, 20.0);
        assert!(proche(petit[0], (10.0, 0.0)), "{:?}", petit[0]);
    }

    #[test]
    fn formes_story_ont_le_bon_nombre_de_points() {
        assert_eq!(heart_points(0.0, 0.0, 10.0).len(), 26);
        assert_eq!(sparkle_points(0.0, 0.0, 10.0).len(), 50);
        assert_eq!(volute_points(0.0, 0.0, 1.0, 1.0, 12.0).0.len(), 25);
    }

    #[test]
    fn hexagon_porte_et_suit_les_pans() {
        assert_eq!(StyleCadre::depuis_nom("hexagon"), Some(StyleCadre::Hexagon));
        assert_eq!(StyleCadre::Hexagon.nom(), "hexagon");
        // s = max(12, round(4·20)) = 80, borné à W/4·H/4 (75 en 400×300).
        let p = points_polygone(StyleCadre::Hexagon, 400.0, 300.0, 20.0);
        assert_eq!(p.len(), 9);
        assert_eq!(p[0], (75.0, 0.0));
        assert_eq!(p[2], (400.0, 37.5));
        assert_eq!(*p.last().expect("fermé"), (75.0, 0.0));
        // Petite boîte : s borné à H/4 (10 pour 100×40).
        let petit = points_polygone(StyleCadre::Hexagon, 100.0, 40.0, 20.0);
        assert_eq!(petit[0], (10.0, 0.0));
    }

    #[test]
    fn arena_porte_et_suit_les_biseaux() {
        assert_eq!(StyleCadre::depuis_nom("arena"), Some(StyleCadre::Arena));
        assert_eq!(StyleCadre::Arena.nom(), "arena");
        // bx = max(16, round(4.2·20)) = 84 borné à W/4 (75) ;
        // by = max(10, round(2.4·20)) = 48 borné à H/4 (75).
        let p = points_polygone(StyleCadre::Arena, 300.0, 300.0, 20.0);
        assert_eq!(p.len(), 9);
        assert_eq!(p[0], (75.0, 0.0));
        assert_eq!(p[2], (300.0, 48.0));
        assert_eq!(*p.last().expect("fermé"), (75.0, 0.0));
    }

    #[test]
    fn comete_parcourt_le_perimetre_et_boucle() {
        // Carré 100×100 : périmètre 400, segment 7 % = 28 px.
        let trace = vec![
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (0.0, 100.0),
            (0.0, 0.0),
        ];
        let pts = comete_points(&trace, 0.0, 0.07);
        assert!(proche(pts[0], (0.0, 0.0)));
        let dernier = *pts.last().expect("segment");
        assert!(proche(dernier, (28.0, 0.0)), "{dernier:?}");
        // Phase 0,99 : départ à 396 px (arête gauche) → traverse le coin.
        let pts = comete_points(&trace, 0.99, 0.07);
        assert!(proche(pts[0], (0.0, 4.0)), "{:?}", pts[0]);
        let dernier = *pts.last().expect("segment");
        assert!(proche(dernier, (24.0, 0.0)), "{dernier:?}");
        // Période d'un tour : la phase est cyclique.
        assert_eq!(
            comete_points(&trace, 0.25, 0.07),
            comete_points(&trace, 1.25, 0.07)
        );
    }

    #[test]
    fn trace_comete_suit_le_chemin_du_style() {
        let mg = CadreCfg {
            rect: (10.0, 20.0, 200.0, 100.0),
            style: StyleCadre::MgStyle,
            stroke_width: 20.0,
            couleur: [0, 0, 0, 255],
            couleur_fin: [1, 2, 3, 255],
            gradient_angle: 135.0,
            z: 0,
            animer: true,
        };
        assert_eq!(
            trace_comete(&mg),
            vec![
                (10.0, 20.0),
                (210.0, 20.0),
                (210.0, 120.0),
                (10.0, 120.0),
                (10.0, 20.0)
            ]
        );
        // thin-line : le tracé est le polygone de découpe (rectangle arrondi).
        let thin = CadreCfg {
            style: StyleCadre::ThinLine,
            ..mg.clone()
        };
        assert_eq!(trace_comete(&thin), thin.polygone());
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
            animer: false,
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
