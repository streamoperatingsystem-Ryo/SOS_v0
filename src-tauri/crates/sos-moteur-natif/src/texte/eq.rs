//! Module HUD « équaliseur » (`scene.modules.equaliseur`) — barres néon
//! dégradées + bulles montantes, piloté par le spectre `eq-spectrum`.
//!
//! Quatrième module porté en natif : contrairement aux autres, celui-ci
//! n'est PAS dans `#modules-root` (z 9000) — la couche `#eq-root` est un
//! canvas plein écran SOUS les widgets dans `#stage`, comme `#spots-canvas`
//! : `z = -1` dans le plan (fond `MIN` < spots -2 < eq -1 < widgets ≥ 0).
//! Aucun clip rect : les bulles débordent vers le haut du module (plafond
//! `y0 − 1,6·H`), comme le canvas plein écran du DOM.
//!
//! Le spectre arrive par `chat_tx` (`{"type":"eq-spectrum","bandes":[24]}`,
//! ~30 Hz — capture système/micro de `equaliseur.rs`) → [`EtatEq`] via
//! `MoteurNatif::pousser_eq`. Sans capture (module seul) les bandes restent
//! à 0 : barres au plancher, pas de balayage forcé — comme le DOM.
//!
//! Miroir de `eqDessiner` (diffusion.html) :
//! - lissage par bande : attaque 14/s, retombée 4,5/s (`dt` borné 1 ms–100 ms) ;
//! - barres : pad latéral 1,2 %, `bw = max(2, pas·0,62)`, hauteur
//!   `max(2, v·H·0,94)` depuis le bas, coins hauts arrondis `min(6, bw/2)` ;
//! - remplissage : dégradé vertical `col_bas → col_haut` — `col_bas` =
//!   `eqCouleur` user sinon l'accent du cadre de scène (`--mod-accent`) ;
//!   `col_haut` = `eqCouleurFin` posée par l'user, sinon `col_bas` à 23 %
//!   d'alpha (le dégradé « fond vers le haut » du placeholder dashboard) ;
//! - alpha par barre `0,45 + v·0,5` (pré-multiplié dans les stops du
//!   dégradé — `globalAlpha` du DOM appliqué à la brosse entière) ;
//! - bulles (`eqBulles !== false`) : spawn borné 80 quand `maxV > 0,12`,
//!   montée `vy` + dérive `vx` + oscillation sinusoïdale, cercle stroké
//!   `col_bas` alpha `a·fondu` vers le plafond.
//!
//! Limites assumées : `shadowBlur` néon non porté (coûteux — le dégradé +
//! l'alpha portent déjà l'effet), bulles strokées sans halo.

use std::time::Instant;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1GradientStopCollection, ID2D1LinearGradientBrush, ID2D1SolidColorBrush, ID2D1StrokeStyle,
    D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT,
};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte};

/// Bandes publiées par la capture — `NB_BANDES` de `equaliseur.rs`.
pub const NB_BANDES_EQ: usize = 24;

/// z de `#eq-root` dans le plan : sous les widgets, au-dessus de spots.
pub const Z_EQ: i32 = -1;
/// z de `#spots-canvas` : le plus bas des calques de scène (au-dessus du fond).
pub const Z_SPOTS: i32 = -2;

/// État live du spectre — cible poussée par l'app + valeurs lissées
/// (attaque/retombée du DOM appliquées à la frame, dans [`EtatEq::tick`]).
#[derive(Debug)]
pub struct EtatEq {
    /// Dernières bandes brutes poussées (~30 Hz).
    cible: [f32; NB_BANDES_EQ],
    /// Bandes lissées pour l'affichage.
    lissees: [f32; NB_BANDES_EQ],
    /// Base du `dt` du prochain tick.
    horodatage: Instant,
    version: u64,
}

impl Default for EtatEq {
    fn default() -> Self {
        Self {
            cible: [0.0; NB_BANDES_EQ],
            lissees: [0.0; NB_BANDES_EQ],
            horodatage: Instant::now(),
            version: 0,
        }
    }
}

impl EtatEq {
    pub fn nouveau() -> Self {
        Self::default()
    }

    /// Pousse une frame `eq-spectrum` ; `version` n'augmente que si les
    /// bandes ont changé (dedup — comme les autres états live).
    pub fn pousser(&mut self, bandes: [f32; NB_BANDES_EQ]) {
        if self.cible == bandes {
            return;
        }
        self.cible = bandes;
        self.version = self.version.wrapping_add(1);
    }

    /// Compteur monotone (clé de cache du rendu).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Avance l'horloge et lisse les bandes — montée rapide (14/s),
    /// retombée douce (4,5/s), `dt` borné comme le DOM. Renvoie
    /// `(dt, max_lissee)` pour les bulles et les effets dépendants.
    pub fn tick(&mut self) -> (f32, f32) {
        let dt = self
            .horodatage
            .elapsed()
            .as_secs_f32()
            .clamp(0.001, 0.1);
        self.horodatage = Instant::now();
        let attaque = (dt * 14.0).min(1.0);
        let relache = (dt * 4.5).min(1.0);
        let mut max_v = 0.0f32;
        for i in 0..NB_BANDES_EQ {
            let v = self.cible[i].clamp(0.0, 1.0);
            let b = &mut self.lissees[i];
            *b += (v - *b) * if v > *b { attaque } else { relache };
            *b = b.max(0.0);
            if *b > max_v {
                max_v = *b;
            }
        }
        (dt, max_v)
    }

    /// Bandes lissées (après `tick`).
    pub fn bandes(&self) -> &[f32; NB_BANDES_EQ] {
        &self.lissees
    }

    /// Énergie brute du spectre (max des bandes **cibles** — le DOM lit
    /// `eqCible`, pas les bandes lissées, pour la réactivité des spots).
    pub fn energie_cible(&self) -> f32 {
        self.cible.iter().copied().fold(0.0, f32::max).min(1.0)
    }
}

/// Configuration du module équaliseur, extraite de
/// `scene.modules.equaliseur`.
#[derive(Debug, Clone, PartialEq)]
pub struct EqualiseurCfg {
    /// Rect du module en px canvas (barres — les bulles débordent).
    pub rect: (f64, f64, f64, f64),
    /// Ordre de composition (`Z_EQ` — couche sous les widgets).
    pub z: i32,
    /// `eqBulles !== false` (défaut DOM : bulles visibles).
    pub bulles: bool,
    /// Bas du dégradé : `eqCouleur` user ou accent du cadre de scène.
    pub col_bas: [u8; 4],
    /// Haut du dégradé : `eqCouleurFin` posée, sinon `col_bas` à 23 %.
    pub col_haut: [u8; 4],
}

/// Une bulle néon — état de rendu (spawn/mort dans `dessiner_equaliseur`).
#[derive(Debug, Clone)]
pub(super) struct BulleEq {
    x: f32,
    y: f32,
    r: f32,
    vy: f32,
    vx: f32,
    a: f32,
    ph: f32,
}

/// Pool de bulles du rendu — vit dans `DessinateurTexte` (état visuel,
/// pas scène).
pub(super) type BullesEq = Vec<BulleEq>;

/// PRNG minimal (xorshift64) pour les bulles — pas de crate rand dans le
/// moteur ; la séquence n'a pas besoin d'être crypto ni reproductible.
struct Graine(u64);

impl Graine {
    fn prochain(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        ((x >> 11) & 0x1F_FFFF) as f32 / 0x20_0000 as f32
    }
}

/// Graine globale — dérivée du temps au premier tirage.
fn graine() -> &'static std::sync::Mutex<Graine> {
    static GRAINE: std::sync::OnceLock<std::sync::Mutex<Graine>> = std::sync::OnceLock::new();
    GRAINE.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        std::sync::Mutex::new(Graine(nanos | 1))
    })
}

fn alea() -> f32 {
    graine()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .prochain()
}

impl DessinateurTexte {
    /// Dessine le module équaliseur — dans une passe ouverte, **sans clip**
    /// (les bulles débordent du rect, couche plein écran sous les widgets).
    pub fn dessiner_equaliseur(
        &mut self,
        cfg: &EqualiseurCfg,
        etat: &mut EtatEq,
    ) -> Result<(), ErreurTexte> {
        let (x0, y0, w, h) = cfg.rect;
        let (x0, y0, w, h) = (x0 as f32, y0 as f32, w as f32, h as f32);
        if w <= 0.0 || h <= 0.0 {
            return Ok(());
        }
        let (dt, max_v) = etat.tick();
        let bandes = *etat.bandes();

        // Barres — dégradé vertical par barre (l'alpha 0,45+v·0,5 du DOM
        // est pré-multiplié dans les stops, `globalAlpha` étant par
        // élément côté canvas 2d).
        let pad_x = w * 0.012;
        let pas = (w - pad_x * 2.0) / NB_BANDES_EQ as f32;
        let bw = (pas * 0.62).max(2.0);
        let r_coin = (bw / 2.0).min(6.0);
        for (i, &v) in bandes.iter().enumerate() {
            if v <= 0.001 {
                continue;
            }
            let bh = (v * h * 0.94).max(2.0);
            let bx = x0 + pad_x + i as f32 * pas + (pas - bw) / 2.0;
            let alpha = 0.45 + v * 0.5;
            let brosse = self.degrade_eq(
                D2D_POINT_2F { x: 0.0, y: y0 + h },
                D2D_POINT_2F { x: 0.0, y: y0 },
                cfg.col_bas,
                cfg.col_haut,
                alpha,
            )?;
            let rect = D2D_RECT_F {
                left: bx,
                top: y0 + h - bh,
                right: bx + bw,
                bottom: y0 + h,
            };
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect, radiusX: r_coin, radiusY: r_coin },
                    &brosse,
                )
            };
            // Base carrée : le DOM n'arrondit que les coins HAUTS — la
            // partie inférieure est re-remplie en plein.
            if bh > r_coin {
                let bas = D2D_RECT_F { top: y0 + h - r_coin, ..rect };
                unsafe { self.cible_rt.FillRectangle(&bas, &brosse) };
            }
        }

        if cfg.bulles {
            self.dessiner_bulles_eq(cfg, x0, y0, w, h, dt, max_v)?;
        }
        Ok(())
    }

    /// Spawn + dérive + tracé des bulles néon (miroir de la boucle
    /// `eqBulles` du DOM — plafond `y0 − 1,6·H`, fondu de sortie).
    #[allow(clippy::too_many_arguments)]
    fn dessiner_bulles_eq(
        &mut self,
        cfg: &EqualiseurCfg,
        x0: f32,
        y0: f32,
        w: f32,
        h: f32,
        dt: f32,
        max_v: f32,
    ) -> Result<(), ErreurTexte> {
        let pad_x = w * 0.012;
        if max_v > 0.12 && self.bulles_eq.len() < 80 && alea() < dt * (2.0 + max_v * 20.0) {
            self.bulles_eq.push(BulleEq {
                x: x0 + pad_x + alea() * (w - pad_x * 2.0),
                y: y0 + h - alea() * 12.0,
                r: 1.5 + alea() * 4.5,
                vy: 26.0 + alea() * 55.0 + max_v * 45.0,
                vx: (alea() - 0.5) * 16.0,
                a: 0.3 + alea() * 0.45,
                ph: alea() * std::f32::consts::TAU,
            });
        }
        let plafond = y0 - h * 1.6;
        let col = couleur_d2d(cfg.col_bas);
        let mut i = self.bulles_eq.len();
        while i > 0 {
            i -= 1;
            let bu = &mut self.bulles_eq[i];
            bu.y -= bu.vy * dt;
            bu.x += bu.vx * dt;
            bu.ph += dt * 2.2;
            if bu.y + bu.r < plafond || bu.y - bu.r > y0 + h {
                self.bulles_eq.swap_remove(i);
                continue;
            }
            let fondu = ((bu.y - plafond) / (h * 0.5)).clamp(0.0, 1.0);
            let a = bu.a * fondu;
            let (cx, cy, r) = (bu.x + bu.ph.sin() * 7.0, bu.y, bu.r);
            let b = self.brosse_eq(D2D1_COLOR_F { r: col.r, g: col.g, b: col.b, a })?;
            unsafe {
                self.cible_rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cx, y: cy },
                        radiusX: r,
                        radiusY: r,
                    },
                    &b,
                    1.2,
                    None::<&ID2D1StrokeStyle>,
                )
            };
        }
        Ok(())
    }

    /// Brosse unie à la demande.
    fn brosse_eq(&self, c: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush, ErreurTexte> {
        unsafe { self.cible_rt.CreateSolidColorBrush(&c, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse eq : {}", e.message())))
    }

    /// Brosse dégradé vertical `debut → fin` avec `alpha` pré-multiplié.
    fn degrade_eq(
        &self,
        debut: D2D_POINT_2F,
        fin: D2D_POINT_2F,
        col_bas: [u8; 4],
        col_haut: [u8; 4],
        alpha: f32,
    ) -> Result<ID2D1LinearGradientBrush, ErreurTexte> {
        let c0 = couleur_d2d(col_bas);
        let c1 = couleur_d2d(col_haut);
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: D2D1_COLOR_F { r: c0.r, g: c0.g, b: c0.b, a: c0.a * alpha },
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: D2D1_COLOR_F { r: c1.r, g: c1.g, b: c1.b, a: c1.a * alpha },
            },
        ];
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("stops eq : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: debut,
            endPoint: fin,
        };
        unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
            .map_err(|e| ErreurTexte::Systeme(format!("dégradé eq : {}", e.message())))
    }
}
