//! Module HUD « spots lumineux » (`scene.modules.spots`) — deux faisceaux
//! qui balayent la scène depuis les coins supérieurs.
//!
//! Cinquième et dernier module porté en natif. Comme l'équaliseur il n'est
//! PAS dans `#modules-root` : `#spots-canvas` est la couche la plus basse
//! de `#stage` (`z = -2` dans le plan — sous les widgets ET sous
//! l'équaliseur -1, au-dessus du fond). Plein canvas : le rect du module
//! n'est pas utilisé par le DOM (config = opacité/vitesse/réactivité).
//!
//! Miroir de `spotsDessiner` (diffusion.html) :
//! - deux spots asymétriques (apex `5 %`/`95 %` du canvas, hauteurs
//!   `-2 %`/`-5,5 %`), cadences `×1`/`×1,17`, phases 0/2,1 — jamais en
//!   miroir ;
//! - angle : `dir·(base + sin(t·0,5·vit+phase)·0,30 + sin(t·0,83·vit+phase·1,7)·0,07)` —
//!   deux sinus non multiples → balayage organique, intérieur seulement ;
//! - largeur du cône qui respire (`±7 %`), intensité `×(0,85 + 0,5·énergie)`
//!   quand `spotsReagir` (énergie = max des bandes `eq-spectrum` brutes) ;
//! - **quatre passes additives** par spot : halo externe (teinte bord,
//!   `1,5·bw`, a·0,16), cône externe (teinte bord, `bw`, a·0,40), faisceau
//!   (teinte pure, `0,66·bw`, a·0,50), cœur blanc-chaud (`0,26·bw`, a·0,42) ;
//! - palette fluo cyclique 6 teintes (`spotsCouleur` : interpolation
//!   continue sur le cycle), teinte de bord = palette +0,07, cœur =
//!   mix(teinte, blanc, 0,55) ;
//! - halo radial à l'apex (`cw·0,045`, cœur 0,6 → teinte 0,28 → 0) et mare
//!   à `0,9·len` du faisceau (ellipse aplatie ×0,32, teinte bord 0,26 → 0).
//!
//! Limites assumées : le sprite DOM (cône plumé latéralement) est rendu
//! par des **trapèzes imbriqués** (les 4 passes sont déjà des cônes de
//! largeurs croissantes → tombée latérale approchée) ; la mare utilise
//! une brosse radiale ; l'évidage des trous (`peindreTrous`) est inutile
//! en natif — les calques widgets/captures passent au-dessus.

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D_POINT_2F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1GradientStopCollection, ID2D1LinearGradientBrush, ID2D1RadialGradientBrush,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PRIMITIVE_BLEND_ADD,
    D2D1_PRIMITIVE_BLEND_SOURCE_OVER, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
};

use super::{DessinateurTexte, ErreurTexte};

/// Palette fluo cyclique — `SPOTS_PALETTE` (diffusion.html), ordre roue
/// des teintes : vert → jaune → orange → rouge → violet → bleu.
const PALETTE: [[u8; 3]; 6] = [
    [16, 185, 60],
    [234, 255, 0],
    [255, 149, 0],
    [255, 23, 68],
    [191, 0, 255],
    [0, 191, 255],
];

/// Teinte cyclique : `t` balaie la palette en boucle (interpolation
/// continue entre deux entrées — `spotsCouleur` du DOM).
fn couleur_cycle(t: f32) -> [u8; 3] {
    let n = PALETTE.len() as f32;
    let u = ((t * n).rem_euclid(n)).min(n - 1e-6);
    let i = u.floor() as usize;
    let f = u - i as f32;
    let a = PALETTE[i];
    let b = PALETTE[(i + 1) % PALETTE.len()];
    [
        a[0] as f32 + (b[0] as f32 - a[0] as f32) * f,
        a[1] as f32 + (b[1] as f32 - a[1] as f32) * f,
        a[2] as f32 + (b[2] as f32 - a[2] as f32) * f,
    ]
    .map(|c| c.round().clamp(0.0, 255.0) as u8)
}

/// `spotsMix(coul, [255,255,255], f)` — poussée vers le blanc (cœur chaud).
fn mix_blanc(c: [u8; 3], f: f32) -> [u8; 3] {
    c.map(|v| (v as f32 + (255.0 - v as f32) * f).round() as u8)
}

/// Configuration du module spots, extraite de `scene.modules.spots`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotsCfg {
    /// Ordre de composition (`Z_SPOTS` — couche la plus basse).
    pub z: i32,
    /// Canvas de la scène en px (le module est plein écran).
    pub canvas: (f32, f32),
    /// `--mod-opacite` bornée 0–1 (défaut DOM 0,8 — pas 0,45).
    pub intensite: f32,
    /// `spotsVitesse` bornée 0,25–3 (défaut 1).
    pub vitesse: f32,
    /// `spotsReagir !== false` : intensité suit l'énergie du spectre.
    pub reagir: bool,
}

impl DessinateurTexte {
    /// Dessine les deux faisceaux — dans une passe ouverte, **sans clip**
    /// (couche plein écran, sous tous les widgets).
    ///
    /// `energie` = max des bandes `eq-spectrum` brutes (`EtatEq::
    /// energie_cible`) quand `reagir`, sinon 0 — la luminosité respire au
    /// rythme de la capture audio comme dans le DOM.
    pub fn dessiner_spots(
        &mut self,
        cfg: &SpotsCfg,
        energie: f32,
    ) -> Result<(), ErreurTexte> {
        let (cw, ch) = cfg.canvas;
        if cw <= 0.0 || ch <= 0.0 {
            return Ok(());
        }
        let t = self.debut_spots.elapsed().as_secs_f32();
        let intens = cfg.intensite * (0.85 + 0.5 * energie);
        let len = ch * 1.25;
        let bw_base = cw * 0.36;

        for i in 0..2 {
            let gauche = i == 0;
            let ax = if gauche { cw * 0.05 } else { cw * 0.95 };
            let apex_y = if gauche { -ch * 0.02 } else { -ch * 0.055 };
            let vit_spot = cfg.vitesse * if gauche { 1.0 } else { 1.17 };
            let phase = if gauche { 0.0 } else { 2.1 };
            let dir = if gauche { -1.0f32 } else { 1.0 };
            let ang = dir
                * ((if gauche { 0.34 } else { 0.44 })
                    + (t * 0.5 * vit_spot + phase).sin() * 0.30
                    + (t * 0.83 * vit_spot + phase * 1.7).sin() * 0.07);
            let bw = bw_base * (1.0 + 0.07 * (t * 0.9 * vit_spot + i as f32 * 2.7).sin());
            let intens2 = intens * (0.88 + 0.12 * (t * 0.8 * vit_spot + i as f32 * 2.7).sin());
            let coul = couleur_cycle(t * vit_spot * 0.035 + i as f32 * 0.5);
            let coul_e = couleur_cycle(t * vit_spot * 0.035 + i as f32 * 0.5 + 0.07);
            let coul_c = mix_blanc(coul, 0.55);

            // Repère du faisceau : +y = direction (`rotate(ang)` du DOM :
            // ang>0 bascule à gauche), +x = latéral.
            let dx = -ang.sin();
            let dy = ang.cos();
            let lx = ang.cos();
            let ly = ang.sin();
            let base = (ax + dx * len, apex_y + dy * len);

            self.spot_passe(ax, apex_y, (dx, dy), (lx, ly), len, bw * 1.5, coul_e, intens2 * 0.16)?;
            self.spot_passe(ax, apex_y, (dx, dy), (lx, ly), len, bw, coul_e, intens2 * 0.40)?;
            self.spot_passe(ax, apex_y, (dx, dy), (lx, ly), len, bw * 0.66, coul, intens2 * 0.50)?;
            self.spot_passe(ax, apex_y, (dx, dy), (lx, ly), len, bw * 0.26, coul_c, intens2 * 0.42)?;

            // Halo radial à l'apex (cœur chaud → teinte → transparent).
            self.halo_spot(ax, apex_y, cw * 0.045, coul, coul_c, intens2)?;
            // Mare de lumière à 90 % du faisceau (ellipse aplatie ×0,32).
            self.mare_spot(
                ax + dx * len * 0.9,
                apex_y + dy * len * 0.9,
                bw * 0.5,
                coul_e,
                intens2,
                base.0,
                base.1,
            )?;
        }
        Ok(())
    }

    /// Un cône de lumière : trapèze (apex 12 % de `bw`, base `bw`) rempli
    /// d'un dégradé axial couleur→transparent — brosse additive si dispo.
    #[allow(clippy::too_many_arguments)]
    fn spot_passe(
        &mut self,
        ax: f32,
        ay: f32,
        d: (f32, f32),
        l: (f32, f32),
        len: f32,
        bw: f32,
        coul: [u8; 3],
        alpha: f32,
    ) -> Result<(), ErreurTexte> {
        if alpha <= 0.001 || bw <= 0.0 {
            return Ok(());
        }
        let ah = bw * 0.06;
        let bh = bw / 2.0;
        let pts = [
            (ax as f64 - l.0 as f64 * ah as f64, ay as f64 - l.1 as f64 * ah as f64),
            (ax as f64 + l.0 as f64 * ah as f64, ay as f64 + l.1 as f64 * ah as f64),
            (
                ax as f64 + d.0 as f64 * len as f64 + l.0 as f64 * bh as f64,
                ay as f64 + d.1 as f64 * len as f64 + l.1 as f64 * bh as f64,
            ),
            (
                ax as f64 + d.0 as f64 * len as f64 - l.0 as f64 * bh as f64,
                ay as f64 + d.1 as f64 * len as f64 - l.1 as f64 * bh as f64,
            ),
        ];
        let geo = self.geometrie_points(&pts)?;
        let c = D2D1_COLOR_F {
            r: coul[0] as f32 / 255.0,
            g: coul[1] as f32 / 255.0,
            b: coul[2] as f32 / 255.0,
            a: alpha.min(1.0),
        };
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: c },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: D2D1_COLOR_F { a: 0.0, ..c },
            },
        ];
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("stops spots : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: D2D_POINT_2F { x: ax, y: ay },
            endPoint: D2D_POINT_2F {
                x: ax + d.0 * len,
                y: ay + d.1 * len,
            },
        };
        let brosse: ID2D1LinearGradientBrush = unsafe {
            self.cible_rt.CreateLinearGradientBrush(&props, None, &collection)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé spots : {}", e.message())))?;
        let brosse: ID2D1Brush = brosse.into();
        unsafe {
            self.ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
            self.ctx.FillGeometry(&geo, &brosse, None);
            self.ctx
                .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        }
        Ok(())
    }

    /// Halo radial à l'apex — `createRadialGradient(0..cw·0,045)` :
    /// cœur chaud 0,6 → teinte 0,28 (à 40 %) → transparent.
    fn halo_spot(
        &mut self,
        ax: f32,
        ay: f32,
        r: f32,
        coul: [u8; 3],
        coul_c: [u8; 3],
        alpha: f32,
    ) -> Result<(), ErreurTexte> {
        let f = |c: [u8; 3], a: f32| D2D1_COLOR_F {
            r: c[0] as f32 / 255.0,
            g: c[1] as f32 / 255.0,
            b: c[2] as f32 / 255.0,
            a: a * alpha.min(1.0),
        };
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: f(coul_c, 0.6) },
            D2D1_GRADIENT_STOP { position: 0.4, color: f(coul, 0.28) },
            D2D1_GRADIENT_STOP { position: 1.0, color: f(coul, 0.0) },
        ];
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("stops halo : {}", e.message())))?;
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: D2D_POINT_2F { x: ax, y: ay },
            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
            radiusX: r,
            radiusY: r,
        };
        let brosse: ID2D1RadialGradientBrush = unsafe {
            self.cible_rt.CreateRadialGradientBrush(&props, None, &collection)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("halo spots : {}", e.message())))?;
        let brosse: ID2D1Brush = brosse.into();
        unsafe {
            self.ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
            self.cible_rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: ax, y: ay },
                    radiusX: r,
                    radiusY: r,
                },
                &brosse,
            );
            self.ctx
                .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        }
        Ok(())
    }

    /// Mare de lumière à l'impact — ellipse aplatie (×0,32), teinte de
    /// bord 0,26 → transparent (radial).
    #[allow(clippy::too_many_arguments)]
    fn mare_spot(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        coul_e: [u8; 3],
        alpha: f32,
        _bx: f32,
        _by: f32,
    ) -> Result<(), ErreurTexte> {
        let f = |a: f32| D2D1_COLOR_F {
            r: coul_e[0] as f32 / 255.0,
            g: coul_e[1] as f32 / 255.0,
            b: coul_e[2] as f32 / 255.0,
            a: a * alpha.min(1.0),
        };
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: f(0.26) },
            D2D1_GRADIENT_STOP { position: 1.0, color: f(0.0) },
        ];
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("stops mare : {}", e.message())))?;
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: D2D_POINT_2F { x: cx, y: cy },
            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
            radiusX: r,
            radiusY: r * 0.32,
        };
        let brosse: ID2D1RadialGradientBrush = unsafe {
            self.cible_rt.CreateRadialGradientBrush(&props, None, &collection)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("mare spots : {}", e.message())))?;
        let brosse: ID2D1Brush = brosse.into();
        unsafe {
            self.ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
            self.cible_rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: cx, y: cy },
                    radiusX: r,
                    radiusY: r * 0.32,
                },
                &brosse,
            );
            self.ctx
                .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        }
        Ok(())
    }
}
