//! Overlay « écran d'attente » (BRB) natif — plein écran, z 9990.
//!
//! Miroir de `initEcranAttenteOverlay` / `majEcranAttente` / `eaCacher`
//! (`diffusion.html` 13009-13318) : fond aurore teinté par le cadre, média
//! local plein cadre (`cover`, vidéo en boucle), voile de lisibilité, titre en
//! dégradé de texte, carte minuteur (label + chrono, cadre SVG de la scène) et
//! — layout `signature` — les éléments de marque : stries néon à comète,
//! mascotte Streamy + anneaux orbitaux, wordmark électrique, ligne morse.
//!
//! Canal : frames `chat_tx` de type **`ecran-attente-etat`** (payload `etat`),
//! auto-réparatrices à chaque tick 1 Hz du chrono Rust. ⚠️ L'event Tauri
//! `ecran-attente:etat` est l'état **dashboard** (modale + pillule) : ce n'est
//! jamais le canal du rendu (même leçon que les lots E1/E2).
//!
//! Le chrono est détenu par l'app : ce module n'a **aucune horloge métier** —
//! il affiche `temps_restant` tel qu'il arrive, et ne mesure que ses propres
//! fondus (opacité 0,45 s à l'apparition, 0,45 s à la disparition).
//!
//! Limites assumées :
//! - `box-shadow`, `drop-shadow` et `text-shadow` non portés (halo des
//!   stries, glow des anneaux, ombre de la carte, lueur du titre et des
//!   chiffres, halo de la mascotte) ;
//! - les arcs orbitaux (`conic-gradient` + `mask` annulaire) sont rendus en
//!   **arcs de trait uni** : une brosse Direct2D n'a pas de dégradé angulaire ;
//! - le wordmark (`mix-blend-mode: screen` pour neutraliser son fond noir) est
//!   dessiné avec une **alpha dérivée de la luminance** — un noir devient
//!   transparent, un pic lumineux reste opaque : c'est l'approximation additive
//!   du `screen`, sans effet de bord sur le fond sombre ;
//! - `font-variant-numeric: tabular-nums` non appliqué (chiffres proportionnels
//!   de Segoe UI — l'écart est d'un demi-pixel par chiffre) ;
//! - `vmin` des stries = `min(canvas)` (le viewport du DOM est le canvas) ;
//! - le média vidéo est avancé **par la composition** (une fois par frame) :
//!   son premier décodage coûte ~100 ms sur la première frame du play.

use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::core::{Interface, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D_POINT_2F,
    D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1GradientStopCollection, ID2D1Layer, ID2D1LinearGradientBrush,
    ID2D1PathGeometry, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ELLIPSE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_WEIGHT_EXTRA_BOLD, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_LINE_SPACING_METHOD_UNIFORM,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS,
    DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};

use crate::cadres::CadreCfg;
use crate::gpu::{DescTexture, Format};
use crate::media::{EffetsCfg, Fit, LectureCfg, MediaCfg, Transform2D};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, TraceMedia};

/// z de l'écran d'attente (`.ecran-attente-root`, z-index 9990) — recouvre la
/// scène mais reste **sous** pad (9995), alertes (9997), bandeau (9998) et
/// bienvenue (9999), qui popent par-dessus pendant l'attente.
pub const Z_ATTENTE: i32 = 9990;

/// `transition: opacity 0.45s ease` de `.ecran-attente-root.visible`.
const FONDU_S: f32 = 0.45;
/// `ease` CSS = `cubic-bezier(0.25, 0.1, 0.25, 1)`.
const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);

/// `.ecran-attente-root { background: #05050a }` — derrière l'aurore.
const FOND_RACINE: [u8; 4] = [0x05, 0x05, 0x0a, 255];
/// `.ecran-attente-titre` : `top 7%`, `padding 0 5%`, 84 px, gras 800.
const TITRE_TAILLE: f32 = 84.0;
const TITRE_TOP: f32 = 0.07;
const TITRE_PAD: f32 = 0.05;
/// `letter-spacing: 0.04em`.
const TITRE_ESPACEMENT: f32 = 0.04 * TITRE_TAILLE;
/// `line-height: 1.1`.
const TITRE_INTERLIGNE: f32 = 1.1;

/// `.ecran-attente-minuteur` : `padding 24px 64px`, `gap 8px`, `border 2px`,
/// `border-radius 14px`, fond `rgba(8,8,14,0.88)`.
const CARTE_PAD_V: f32 = 24.0;
const CARTE_PAD_H: f32 = 64.0;
const CARTE_GAP: f32 = 8.0;
const CARTE_BORD: f32 = 2.0;
const CARTE_RAYON: f32 = 14.0;
const CARTE_FOND: [u8; 4] = [8, 8, 14, 224];
const CARTE_BORD_COULEUR: [u8; 4] = [224, 224, 224, 217];
/// `.ecran-attente-minuteur-label` : 20 px, 600, `letter-spacing .22em`.
const LABEL_TAILLE: f32 = 20.0;
const LABEL_ESPACEMENT: f32 = 0.22 * LABEL_TAILLE;
const LABEL_COULEUR: [u8; 4] = [0xbd, 0xbd, 0xbd, 255];
/// `.ecran-attente-temps` : 84 px, 800, `line-height: 1`.
const TEMPS_TAILLE: f32 = 84.0;
/// `.en-pause .ecran-attente-temps { opacity: 0.6 }`.
const TEMPS_PAUSE_ALPHA: f32 = 0.6;

/// Libellés du DOM (jamais i18n — constantes du rendu), déjà en capitales :
/// le CSS applique `text-transform: uppercase` sur « Début dans »/« En pause ».
const LABEL_ATTENTE: &str = "DÉBUT DANS";
const LABEL_PAUSE: &str = "EN PAUSE";

/// `.sig-strie` : 2 px de large, 34 vmin de haut, décalage de coin 5 vmin,
/// marge 9 vmin, comète 4 px × 18 % en cycle de 6 s.
const STRIE_LARGEUR: f32 = 2.0;
const STRIE_HAUTEUR_VMIN: f32 = 34.0;
const STRIE_COIN_VMIN: f32 = -5.0;
const STRIE_MARGE_VMIN: f32 = 9.0;
/// `::before` — ligne sœur à `left: 8px`, 1 px, `opacity: .5`.
const STRIE_SOEUR_X: f32 = 8.0;
const STRIE_SOEUR_LARGEUR: f32 = 1.0;
const STRIE_SOEUR_ALPHA: f32 = 0.5;
const COMETE_LARGEUR: f32 = 4.0;
const COMETE_HAUTEUR: f32 = 0.18;
const COMETE_CYCLE_S: f32 = 6.0;
/// `.sig-strie.sN .sig-comet { animation-delay }`.
const COMETE_DELAIS: [f32; 4] = [0.0, 1.5, 3.0, 4.5];

/// `.ecran-attente-sig-streamy { width: 300px; height: 430px }`.
const STREAMY_L: f32 = 300.0;
const STREAMY_H: f32 = 430.0;
/// `.sig-orbite { width: 118%; border: 2px }`.
const ORBITE_RATIO: f32 = 1.18;
const ORBITE_BORD: f32 = 2.0;
/// `.sig-orbite::before/::after` : épaisseur, part visible du tour, cycle.
const ARC_EXTERNE_EPAISSEUR: f32 = 4.0;
const ARC_EXTERNE_PART: f32 = 0.28;
const ARC_EXTERNE_CYCLE_S: f32 = 9.0;
const ARC_EXTERNE: [u8; 4] = [103, 232, 249, 217];
const ARC_INTERNE_EPAISSEUR: f32 = 3.0;
const ARC_INTERNE_PART: f32 = 0.22;
const ARC_INTERNE_CYCLE_S: f32 = 13.0;
const ARC_INTERNE: [u8; 4] = [236, 72, 153, 179];
/// `.sig-orbite` : `border: 2px solid rgba(103,232,249,.22)`.
const ORBITE_COULEUR: [u8; 4] = [103, 232, 249, 56];
/// `ea-streamy-flotte` : ±9 px sur 5,5 s `ease-in-out`.
const STREAMY_FLOTTE_PX: f32 = 9.0;
const STREAMY_FLOTTE_S: f32 = 5.5;

/// `.ecran-attente-sig-wordmark { width: 480px; opacity: 0.9 }`.
const WORDMARK_L: f32 = 480.0;
const WORDMARK_ALPHA: f32 = 0.9;

/// `.ecran-attente-sig-morse` : segments `p` 6 px / `t` 22 px, 6 px de haut,
/// `gap 9px`, `bottom 1.7%`, cycle 9,5 s.
const MORSE_GAP: f32 = 9.0;
const MORSE_HAUTEUR: f32 = 6.0;
const MORSE_LARGEUR_P: f32 = 6.0;
const MORSE_LARGEUR_T: f32 = 22.0;
const MORSE_BAS: f32 = 0.017;
const MORSE_CYCLE_S: f32 = 9.5;
const MORSE_ALPHA_REPOS: f32 = 0.14;
const MORSE_COULEUR: [u8; 4] = [103, 232, 249, 230];
/// `···−−−···` : type (trait = `true`) et retard d'animation.
const MORSE_SEGMENTS: [(bool, f32); 9] = [
    (false, 0.0),
    (false, 0.7),
    (false, 1.4),
    (true, 2.4),
    (true, 4.0),
    (true, 5.6),
    (false, 7.2),
    (false, 7.9),
    (false, 8.6),
];

/// Assets de marque embarqués — mêmes fichiers que `GET /brand/{name}` de
/// l'app (`server.rs`, `include_bytes!`), donc zéro dérive d'asset.
const STREAMY_CHARGEMENT: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../src/assets/streamy/streamy-loading.png"));
const STREAMY_REPOS: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../src/assets/streamy/streamy-offline.png"));
const WORDMARK_PNG: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../src/assets/logo/LogoSOSDashboardSmallSize.png"));

// =============================================================================
// FRAME — état complet décodé par le pont (`src/moteur/mod.rs`).
// =============================================================================

/// État complet d'une frame `ecran-attente-etat` (auto-réparatrice : le DOM
/// réapplique tout à chaque tick, le moteur remplace donc son état).
#[derive(Debug, Clone, PartialEq)]
pub struct AttenteCfg {
    pub actif: bool,
    pub en_pause: bool,
    /// Secondes restantes — affichées telles quelles (le chrono vit dans l'app).
    pub temps_restant: u64,
    pub titre: String,
    pub afficher_titre: bool,
    /// Son du média (`muted = !son` du `<video>`).
    pub son: bool,
    /// Volume du média (0..1).
    pub volume: f32,
    /// Chemin relatif à la base de données (`medias/<uuid>.<ext>`).
    pub media: Option<String>,
    /// `"image"` | `"video"` (l'extension tranche au chargement).
    pub media_kind: Option<String>,
    /// Centre + échelle de la carte minuteur (px canvas).
    pub minuteur: (f64, f64, f64),
    /// `layout !== "minimal"` — bloc de marque StreamOS affiché.
    pub signature: bool,
    /// Centre + échelle de Streamy (px canvas).
    pub streamy: (f64, f64, f64),
    /// Centre + échelle du wordmark (px canvas).
    pub logo: (f64, f64, f64),
    /// Couper l'audio « bureau » pendant l'attente (`muter_bureau` de la
    /// config) — mute de la source loopback du mixeur natif.
    pub muter_bureau: bool,
}

impl Default for AttenteCfg {
    /// Défauts du domaine (`EcranAttenteConfig::default`), jamais des zéros :
    /// une frame partielle doit peindre le même écran que le DOM.
    fn default() -> Self {
        Self {
            actif: false,
            en_pause: false,
            temps_restant: 0,
            titre: String::new(),
            afficher_titre: true,
            son: true,
            volume: 0.5,
            media: None,
            media_kind: None,
            minuteur: (960.0, 540.0, 1.0),
            signature: true,
            streamy: (170.0, 180.0, 0.6),
            logo: (960.0, 140.0, 1.0),
            muter_bureau: true,
        }
    }
}

// =============================================================================
// ÉTAT LIVE
// =============================================================================

/// État vivant de l'écran d'attente : dernière config + phases de fondu.
#[derive(Default)]
pub struct EtatAttente {
    cfg: AttenteCfg,
    /// Lancé (et pas encore arrêté) — le fondu de sortie ne le remet pas à
    /// `true` : le mute bureau suit l'état de l'app, pas l'opacité.
    actif: bool,
    debut: Option<Instant>,
    /// `actif: false` reçu — base du fondu de sortie.
    fin: Option<Instant>,
    version: u64,
}

/// Ce que la composition peint ce tick-ci (opacité déjà résolue).
pub struct RenduAttente<'a> {
    pub cfg: &'a AttenteCfg,
    /// Opacité de la racine (fondu d'entrée / de sortie).
    pub opacite: f32,
}

impl EtatAttente {
    pub fn nouveau() -> Self {
        Self::default()
    }

    /// Applique une frame complète (idempotent : une frame identique ne
    /// change pas la version, donc ne re-met pas la carte en page).
    pub fn appliquer(&mut self, cfg: AttenteCfg) {
        self.appliquer_at(cfg, Instant::now());
    }

    fn appliquer_at(&mut self, cfg: AttenteCfg, maintenant: Instant) {
        if cfg.actif {
            if !self.actif {
                self.actif = true;
                self.debut = Some(maintenant);
                self.fin = None;
            }
        } else if self.actif {
            // Stop : fondu de sortie (le DOM retire le média après 500 ms).
            self.actif = false;
            self.fin = Some(maintenant);
        }
        if cfg != self.cfg {
            self.cfg = cfg;
            self.version = self.version.wrapping_add(1);
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// L'écran est lancé (mute bureau actif, son du média audible).
    pub fn actif(&self) -> bool {
        self.actif
    }

    /// Mute « bureau » demandé par la config — équivalent natif du
    /// `SetSceneItemEnabled false` sur les sources `desktop|bureau` d'OBS.
    pub fn mute_bureau(&self) -> bool {
        self.actif && self.cfg.muter_bureau
    }

    /// Gain du média BRB en millièmes (`muted = !son` + `volume` du DOM).
    pub fn gain_media_milliemes(&self) -> u32 {
        if !self.actif || !self.cfg.son {
            return 0;
        }
        (self.cfg.volume.clamp(0.0, 1.0) * 1000.0).round() as u32
    }

    /// Phases au temps courant — `None` quand l'écran est entièrement sorti.
    pub fn rendu(&self) -> Option<RenduAttente<'_>> {
        self.rendu_at(Instant::now())
    }

    fn rendu_at(&self, maintenant: Instant) -> Option<RenduAttente<'_>> {
        let opacite = match self.fin {
            Some(fin) => {
                let t = maintenant.saturating_duration_since(fin).as_secs_f32();
                if t >= FONDU_S {
                    return None;
                }
                1.0 - bezier_css(EASE, t / FONDU_S)
            }
            None if self.actif => {
                let debut = self.debut?;
                let t = maintenant.saturating_duration_since(debut).as_secs_f32();
                bezier_css(EASE, (t / FONDU_S).min(1.0))
            }
            None => return None,
        };
        Some(RenduAttente {
            cfg: &self.cfg,
            opacite,
        })
    }
}

/// `cubic-bezier(x1,y1,x2,y2)` CSS à `x` ∈ [0,1] (dichotomie) — miroir du
/// `ease` de `transition: opacity 0.45s ease`.
fn bezier_css((x1, y1, x2, y2): (f32, f32, f32, f32), x: f32) -> f32 {
    fn courbe(a1: f32, a2: f32, t: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * a1 + 3.0 * u * t * t * a2 + t * t * t
    }
    let x = x.clamp(0.0, 1.0);
    let (mut bas, mut haut) = (0.0_f32, 1.0_f32);
    let mut t = x;
    for _ in 0..32 {
        let v = courbe(x1, x2, t);
        if (v - x).abs() < 1e-5 {
            break;
        }
        if v < x {
            bas = t;
        } else {
            haut = t;
        }
        t = (bas + haut) / 2.0;
    }
    courbe(y1, y2, t)
}

/// `ease-in-out` CSS = `cubic-bezier(0.42, 0, 0.58, 1)`.
fn ease_in_out(t: f32) -> f32 {
    bezier_css((0.42, 0.0, 0.58, 1.0), t)
}

/// `eaFormatTemps` du DOM : `M:SS`, ou `H:MM:SS` au-delà d'une heure.
pub fn formater_temps(total: u64) -> String {
    let t = total;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `color-mix(in srgb, couleur part%, blanc)` — mélange sRGB droit, comme le
/// CSS (`ea-prim 82%` / `ea-sec 60%` des chiffres).
pub fn meler_blanc(c: [u8; 4], part: f32) -> [u8; 4] {
    let p = part.clamp(0.0, 1.0);
    let f = |v: u8| (v as f32 * p + 255.0 * (1.0 - p)).round().clamp(0.0, 255.0) as u8;
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

// =============================================================================
// BLOC DE RENDU
// =============================================================================

struct LigneA {
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    h: f32,
    w: f32,
}

pub(super) struct BlocAttente {
    cle: u64,
    titre: Option<LigneA>,
    label: Option<LigneA>,
    temps: Option<LigneA>,
    /// Boîte de la carte **non mise à l'échelle** (w, h).
    carte: (f32, f32),
    /// Cadre concret (style de la scène posé sur la carte mesurée).
    cadre: Option<(CadreCfg, ID2D1PathGeometry)>,
    /// Même polygone à la taille **écran** (masque de la couche).
    masque: Option<ID2D1PathGeometry>,
}

/// Assets de marque décodés une fois (`BitmapChat` = texture + vue D2D + px).
pub(super) type Asset = (
    windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    windows::Win32::Graphics::Direct2D::ID2D1Bitmap1,
    (f32, f32),
);

fn cle_attente(
    version: u64,
    canvas: (u32, u32),
    couleurs: ([u8; 4], [u8; 4]),
    cadre: Option<&CadreCfg>,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    canvas.hash(&mut h);
    couleurs.hash(&mut h);
    cadre.is_some().hash(&mut h);
    if let Some(c) = cadre {
        c.stroke_width.to_bits().hash(&mut h);
        c.gradient_angle.to_bits().hash(&mut h);
        c.couleur.hash(&mut h);
        c.couleur_fin.hash(&mut h);
        c.animer.hash(&mut h);
        // `StyleCadre` n'implémente pas `Hash` : son discriminant suffit
        // (une scène change de style ⇒ nouveau gabarit ⇒ nouvelle géométrie).
        (c.style as u8).hash(&mut h);
    }
    h.finish()
}

impl DessinateurTexte {
    /// Dessine l'écran d'attente — dans une passe ouverte.
    ///
    /// `couleurs` = `(primaire, secondaire)` du cadre de scène (même
    /// résolution `bpmResoudreCouleurs` que le bandeau) ; `cadre` = gabarit du
    /// cadre de scène appliqué à la **carte minuteur** mesurée ; `temps_s` =
    /// horloge du flux (animations de la signature + avance du média).
    pub fn dessiner_attente(
        &mut self,
        etat: &EtatAttente,
        couleurs: ([u8; 4], [u8; 4]),
        cadre: Option<&CadreCfg>,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let Some(rendu) = etat.rendu() else {
            // Entièrement sorti : média et bloc libérés (le DOM retire le
            // `<video>` 500 ms après le stop).
            self.attente_media = None;
            self.attente_media_rel = None;
            self.attente_media_kind = None;
            self.bloc_attente = None;
            return Ok(());
        };
        self.synchroniser_media_attente(rendu.cfg, temps_s)?;
        // Vidéo : une frame d'avance par composition (le média BRB n'est pas
        // dans le plan des médias de scène, qui sont avancés par la boucle).
        let temps_us = (temps_s * 1e6) as i64;
        if let Some(media) = self.attente_media.as_mut() {
            if let Err(e) = media.avancer(temps_us) {
                log::debug!("attente : vidéo ({e})");
            }
            // Anneau du son : drainé par le mixeur (gain = `son` × `volume`).
            if let Err(e) = media.avancer_audio(temps_us) {
                log::debug!("attente : audio vidéo ({e})");
            }
        }
        self.synchroniser_bloc_attente(etat.version(), rendu.cfg, couleurs, cadre, canvas)?;

        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        // Couche plein canvas : opacité du fondu (la racine a `overflow:
        // hidden`, la cible Direct2D borne déjà au canvas).
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: h,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: rendu.opacite.clamp(0.0, 1.0),
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer attente : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let resultat = self.peindre_attente(rendu.cfg, couleurs, canvas, temps_s);
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        resultat
    }

    /// Corps du rendu — dans la couche d'opacité.
    fn peindre_attente(
        &mut self,
        cfg: &AttenteCfg,
        couleurs: ([u8; 4], [u8; 4]),
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let plein = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };
        // Racine opaque : sous l'aurore comme sous le média.
        let fond_racine =
            unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(FOND_RACINE), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse racine : {}", e.message())))?;
        unsafe { self.cible_rt.FillRectangle(&plein, &fond_racine) };

        // Média plein cadre `object-fit: cover` — sinon l'aurore teintée.
        // Handles clonés : garder `attente_media` emprunté empêcherait
        // `bitmap_media` (`&mut self`).
        let infos = self
            .attente_media
            .as_ref()
            .map(|m| (m.texture.clone(), m.taille, m.alpha));
        match infos {
            Some((texture, taille, alpha)) => {
                self.dessiner_media_attente(&texture, taille, alpha, canvas)?
            }
            None => self.peindre_aurore(couleurs, canvas, temps_s)?,
        }

        // Voile de lisibilité (au-dessus du média, sous la marque).
        let voile = [
            (0.0, [0, 0, 0, 115]),
            (0.30, [0, 0, 0, 0]),
            (0.68, [0, 0, 0, 0]),
            (1.0, [0, 0, 0, 128]),
        ];
        let brosse_voile = self.brosse_lineaire_css(&voile, plein, 180.0)?;
        unsafe { self.cible_rt.FillRectangle(&plein, &brosse_voile) };

        if cfg.signature {
            self.peindre_signature(cfg, canvas, temps_s)?;
        }
        self.peindre_titre(couleurs, canvas)?;
        self.peindre_minuteur(cfg, couleurs, temps_s)?;
        Ok(())
    }

    /// Aurore du fond (`.ecran-attente-fond` + `::before` dérivant).
    fn peindre_aurore(
        &mut self,
        couleurs: ([u8; 4], [u8; 4]),
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let plein = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };
        // `linear-gradient(160deg, #06060c, #0b0b16 55%, #06060c)`.
        let base = [
            (0.0, [0x06, 0x06, 0x0c, 255]),
            (0.55, [0x0b, 0x0b, 0x16, 255]),
            (1.0, [0x06, 0x06, 0x0c, 255]),
        ];
        let brosse_base = self.brosse_lineaire_css(&base, plein, 160.0)?;
        unsafe { self.cible_rt.FillRectangle(&plein, &brosse_base) };

        let (prim, sec) = couleurs;
        // `radial-gradient(ellipse 80% 60% at 20% 18%, prim .20, transparent 62%)`.
        let halo = |centre: (f32, f32),
                    rayons: (f32, f32),
                    couleur: [u8; 4],
                    arret: f32|
         -> Result<(), ErreurTexte> {
            let transparent = [couleur[0], couleur[1], couleur[2], 0];
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: couleur_d2d(couleur),
                },
                D2D1_GRADIENT_STOP {
                    position: arret,
                    color: couleur_d2d(transparent),
                },
            ];
            let collection: ID2D1GradientStopCollection = unsafe {
                self.cible_rt
                    .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("dégradé halo attente : {}", e.message()))
            })?;
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: D2D_POINT_2F {
                    x: centre.0,
                    y: centre.1,
                },
                gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
                radiusX: rayons.0,
                radiusY: rayons.1,
            };
            let brosse = unsafe {
                self.cible_rt
                    .CreateRadialGradientBrush(&props, None, &collection)
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("CreateRadialGradientBrush : {}", e.message()))
            })?;
            unsafe { self.cible_rt.FillRectangle(&plein, &brosse) };
            Ok(())
        };
        halo(
            (0.20 * w, 0.18 * h),
            (0.80 * w, 0.60 * h),
            [prim[0], prim[1], prim[2], 51],
            0.62,
        )?;
        halo(
            (0.80 * w, 0.78 * h),
            (0.70 * w, 0.55 * h),
            [sec[0], sec[1], sec[2], 41],
            0.65,
        )?;

        // `::before` — halo dérivant (`ea-derive` 22 s alternate), inset -18 %.
        let cycle = 22.0_f32;
        let phase = (temps_s as f32 / cycle).rem_euclid(2.0);
        let k = ease_in_out(if phase > 1.0 { 2.0 - phase } else { phase });
        let (bx, by) = (-0.18 * w, -0.18 * h);
        let (bw, bh) = (1.36 * w, 1.36 * h);
        let dx = (-0.04 + 0.08 * k) * bw;
        let dy = (-0.03 + 0.06 * k) * bh;
        let echelle = 1.0 + 0.08 * k;
        let (cx, cy) = (bx + bw / 2.0, by + bh / 2.0);
        let mtx = Matrix3x2 {
            M11: echelle,
            M12: 0.0,
            M21: 0.0,
            M22: echelle,
            M31: cx * (1.0 - echelle) + dx,
            M32: cy * (1.0 - echelle) + dy,
        };
        unsafe { self.cible_rt.SetTransform(&mtx) };
        halo(
            (bx + 0.32 * bw, by + 0.32 * bh),
            (0.46 * bw, 0.46 * bh),
            [prim[0], prim[1], prim[2], 41],
            1.0,
        )?;
        halo(
            (bx + 0.68 * bw, by + 0.62 * bh),
            (0.52 * bw, 0.52 * bh),
            [sec[0], sec[1], sec[2], 33],
            1.0,
        )?;
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        Ok(())
    }

    /// Média plein cadre (`object-fit: cover`) — image ou frame vidéo.
    fn dessiner_media_attente(
        &mut self,
        texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        taille: (u32, u32),
        alpha: bool,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f64, canvas.1 as f64);
        let (source, destination) =
            crate::media::fit_rects(taille, (0.0, 0.0, w, h), Fit::Remplir);
        let bitmap = self.bitmap_media(texture, alpha)?;
        let trace = TraceMedia {
            destination,
            source,
            boite: windows::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: canvas.0 as i32,
                bottom: canvas.1 as i32,
            },
            transforme: None,
            clip: None,
            effets: &EffetsCfg::NEUTRE,
            opacite: 1.0,
        };
        self.dessiner_media(&bitmap, &trace)
    }

    /// Bloc « signature » : stries, Streamy + orbites, wordmark, morse.
    fn peindre_signature(
        &mut self,
        cfg: &AttenteCfg,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        self.peindre_stries(canvas, temps_s)?;
        self.peindre_streamy(cfg, temps_s)?;
        self.peindre_wordmark(cfg)?;
        self.peindre_morse(canvas, temps_s)?;
        Ok(())
    }

    /// 4 stries néon diagonales aux coins, chacune avec sa comète.
    fn peindre_stries(&mut self, canvas: (u32, u32), temps_s: f64) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let vmin = w.min(h) / 100.0;
        let hauteur = STRIE_HAUTEUR_VMIN * vmin;
        let marge = STRIE_MARGE_VMIN * vmin;
        let coin = STRIE_COIN_VMIN * vmin;
        let t = temps_s as f32;
        // (x, y, angle) — `s1` haut-gauche, `s2` haut-droit, `s3` bas-gauche,
        // `s4` bas-droit (miroir du CSS `.sig-strie.sN`).
        let stries = [
            (marge, coin, 45.0_f32),
            (w - marge - STRIE_LARGEUR, coin, -45.0),
            (marge, h + coin - hauteur, -45.0),
            (w - marge - STRIE_LARGEUR, h + coin - hauteur, 45.0),
        ];
        for (i, (x, y, angle)) in stries.iter().enumerate() {
            let (cx, cy) = (x + STRIE_LARGEUR / 2.0, y + hauteur / 2.0);
            let mtx = Matrix3x2::rotation(*angle, cx, cy);
            unsafe { self.cible_rt.SetTransform(&mtx) };
            let barre = D2D_RECT_F {
                left: *x,
                top: *y,
                right: x + STRIE_LARGEUR,
                bottom: y + hauteur,
            };
            // `linear-gradient(to bottom, transparent, cyan .45 30%,
            // violet .45 70%, transparent)`.
            let stops = [
                (0.0, [103, 232, 249, 0]),
                (0.30, [103, 232, 249, 115]),
                (0.70, [168, 85, 247, 115]),
                (1.0, [168, 85, 247, 0]),
            ];
            let brosse = self.brosse_lineaire_css(&stops, barre, 180.0)?;
            unsafe { self.cible_rt.FillRectangle(&barre, &brosse) };

            // `::before` — ligne sœur 1 px à `left: 8px`, moitié d'opacité.
            let soeur = D2D_RECT_F {
                left: x + STRIE_SOEUR_X,
                top: *y,
                right: x + STRIE_SOEUR_X + STRIE_SOEUR_LARGEUR,
                bottom: y + hauteur,
            };
            let stops_soeur: Vec<(f32, [u8; 4])> = stops
                .iter()
                .map(|(p, c)| (*p, [c[0], c[1], c[2], (c[3] as f32 * STRIE_SOEUR_ALPHA) as u8]))
                .collect();
            let brosse_soeur = self.brosse_lineaire_css(&stops_soeur, soeur, 180.0)?;
            unsafe { self.cible_rt.FillRectangle(&soeur, &brosse_soeur) };

            // Comète : `translateY(-120% → 620%)` sur 6 s, retard par strie.
            let hc = COMETE_HAUTEUR * hauteur;
            let phase = ((t - COMETE_DELAIS[i]) / COMETE_CYCLE_S).rem_euclid(1.0);
            let decalage = (-1.2 + 7.4 * phase) * hc;
            let comete = D2D_RECT_F {
                left: x - 1.0,
                top: y + decalage,
                right: x - 1.0 + COMETE_LARGEUR,
                bottom: y + decalage + hc,
            };
            let stops_comete = [
                (0.0, [224, 255, 255, 0]),
                (0.5, [224, 255, 255, 242]),
                (1.0, [224, 255, 255, 0]),
            ];
            let brosse_comete = self.brosse_lineaire_css(&stops_comete, comete, 180.0)?;
            unsafe { self.cible_rt.FillRectangle(&comete, &brosse_comete) };
            unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        }
        Ok(())
    }

    /// Mascotte Streamy (flottante) + anneaux orbitaux rotatifs.
    fn peindre_streamy(&mut self, cfg: &AttenteCfg, temps_s: f64) -> Result<(), ErreurTexte> {
        let (sx, sy, echelle) = cfg.streamy;
        // Boîte locale : `left/top = streamy_x/y`, `translate(-50%,-50%)` →
        // le CENTRE de la boîte atterrit sur (streamy_x, streamy_y).
        let (bx, by) = (sx as f32, sy as f32);
        let mtx = matrice_centre(bx, by, STREAMY_L, STREAMY_H, echelle as f32);
        unsafe { self.cible_rt.SetTransform(&mtx) };
        let (cx, cy) = (bx + STREAMY_L / 2.0, by + STREAMY_H / 2.0);
        let t = temps_s as f32;

        // Anneau (`.sig-orbite` 118 % de la largeur du bloc, centré).
        let rayon = STREAMY_L * ORBITE_RATIO / 2.0;
        let brosse_anneau =
            unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(ORBITE_COULEUR), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse orbite : {}", e.message())))?;
        let ellipse = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: cx, y: cy },
            radiusX: rayon,
            radiusY: rayon,
        };
        unsafe {
            self.cible_rt
                .DrawEllipse(&ellipse, &brosse_anneau, ORBITE_BORD, None)
        };

        // Arcs brillants (conic + masque annulaire → arc de trait uni).
        self.arc_orbital(&ArcOrbite {
            cx,
            cy,
            rayon: rayon + ORBITE_BORD,
            epaisseur: ARC_EXTERNE_EPAISSEUR,
            part: ARC_EXTERNE_PART,
            couleur: ARC_EXTERNE,
            depart_deg: (t / ARC_EXTERNE_CYCLE_S * 360.0).rem_euclid(360.0),
        })?;
        self.arc_orbital(&ArcOrbite {
            cx,
            cy,
            rayon: rayon * 0.86,
            epaisseur: ARC_INTERNE_EPAISSEUR,
            part: ARC_INTERNE_PART,
            couleur: ARC_INTERNE,
            depart_deg: -(t / ARC_INTERNE_CYCLE_S * 360.0).rem_euclid(360.0) + 100.8,
        })?;

        // Mascotte : `object-fit: contain`, flottement ±9 px sur 5,5 s.
        let p = (t / STREAMY_FLOTTE_S).rem_euclid(1.0);
        let f = if p < 0.5 {
            ease_in_out(p * 2.0)
        } else {
            1.0 - ease_in_out((p - 0.5) * 2.0)
        };
        let flotte = -STREAMY_FLOTTE_PX + 2.0 * STREAMY_FLOTTE_PX * f;
        let asset = if cfg.en_pause {
            self.asset_marque(AssetMarque::StreamyRepos)
        } else {
            self.asset_marque(AssetMarque::StreamyChargement)
        };
        if let Some((_, bitmap, (iw, ih))) = asset {
            // `contain` dans la boîte 300×430, centré, décalé du flottement.
            let (iw, ih) = (iw as f64, ih as f64);
            let e = (STREAMY_L as f64 / iw).min(STREAMY_H as f64 / ih);
            let (dw, dh) = (iw * e, ih * e);
            let dx = bx as f64 + (STREAMY_L as f64 - dw) / 2.0;
            let dy = by as f64 + (STREAMY_H as f64 - dh) / 2.0 + flotte as f64;
            let dst = D2D_RECT_F {
                left: dx as f32,
                top: dy as f32,
                right: (dx + dw) as f32,
                bottom: (dy + dh) as f32,
            };
            unsafe {
                self.cible_rt.DrawBitmap(
                    &bitmap,
                    Some(&dst),
                    1.0,
                    windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                )
            };
        }
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        Ok(())
    }

    /// Wordmark électrique (480 px de large, centré, échelle configurable).
    fn peindre_wordmark(&mut self, cfg: &AttenteCfg) -> Result<(), ErreurTexte> {
        let (lx, ly, echelle) = cfg.logo;
        let Some((_, bitmap, (iw, ih))) = self.asset_marque(AssetMarque::Wordmark) else {
            return Ok(());
        };
        let hauteur = WORDMARK_L * ih / iw.max(1.0);
        // Boîte locale `left/top = logo_x/y` : le CENTRE atterrit sur
        // (logo_x, logo_y) après `translate(-50%,-50%) scale`.
        let (bx, by) = (lx as f32, ly as f32);
        let mtx = matrice_centre(bx, by, WORDMARK_L, hauteur, echelle as f32);
        unsafe { self.cible_rt.SetTransform(&mtx) };
        let dst = D2D_RECT_F {
            left: bx,
            top: by,
            right: bx + WORDMARK_L,
            bottom: by + hauteur,
        };
        unsafe {
            self.cible_rt.DrawBitmap(
                &bitmap,
                Some(&dst),
                WORDMARK_ALPHA,
                windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            )
        };
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        Ok(())
    }

    /// Ligne morse `···−−−···` — 9 segments flashés en séquence (9,5 s).
    fn peindre_morse(&mut self, canvas: (u32, u32), temps_s: f64) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let largeur_totale = MORSE_SEGMENTS
            .iter()
            .map(|(est_trait, _)| if *est_trait { MORSE_LARGEUR_T } else { MORSE_LARGEUR_P })
            .sum::<f32>()
            + MORSE_GAP * (MORSE_SEGMENTS.len() - 1) as f32;
        let mut x = w / 2.0 - largeur_totale / 2.0;
        let y = h - MORSE_BAS * h - MORSE_HAUTEUR;
        let t = temps_s as f32;
        for (est_trait, delai) in MORSE_SEGMENTS {
            let largeur = if est_trait { MORSE_LARGEUR_T } else { MORSE_LARGEUR_P };
            // `animation-delay` positif : l'élément reste au repos avant.
            let ecoule = t - delai;
            let alpha = if ecoule < 0.0 {
                MORSE_ALPHA_REPOS
            } else {
                let phase = (ecoule / MORSE_CYCLE_S).rem_euclid(1.0);
                // Flash : 1 à 0 %, puis retombée à 14 % (5 % point / 12 % trait).
                let fin = if est_trait { 0.12 } else { 0.05 };
                if phase >= fin {
                    MORSE_ALPHA_REPOS
                } else {
                    let k = phase / fin;
                    1.0 + (MORSE_ALPHA_REPOS - 1.0) * k
                }
            };
            let couleur = [
                MORSE_COULEUR[0],
                MORSE_COULEUR[1],
                MORSE_COULEUR[2],
                (MORSE_COULEUR[3] as f32 * alpha) as u8,
            ];
            let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse morse : {}", e.message())))?;
            let rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + largeur,
                bottom: y + MORSE_HAUTEUR,
            };
            // `border-radius: 4px` — arrondi borné à la moitié de la hauteur.
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect,
                        radiusX: 4.0_f32.min(MORSE_HAUTEUR / 2.0),
                        radiusY: 4.0_f32.min(MORSE_HAUTEUR / 2.0),
                    },
                    &brosse,
                )
            };
            x += largeur + MORSE_GAP;
        }
        Ok(())
    }

    /// Arc d'orbite — `conic-gradient` masqué en anneau, approché par un
    /// trait uni sur la part visible du tour (voir les limites du module).
    fn arc_orbital(&mut self, arc: &ArcOrbite) -> Result<(), ErreurTexte> {
        let ArcOrbite {
            cx,
            cy,
            rayon,
            epaisseur,
            part,
            couleur,
            depart_deg,
        } = *arc;
        let geo: ID2D1PathGeometry = unsafe { self.d2d.CreatePathGeometry() }
            .and_then(|g| g.cast())
            .map_err(|e| ErreurTexte::Gpu(format!("CreatePathGeometry orbite : {}", e.message())))?;
        let sink = unsafe { geo.Open() }
            .map_err(|e| ErreurTexte::Gpu(format!("Open orbite : {}", e.message())))?;
        // Angles CSS : 0° = haut, sens horaire.
        let point = |deg: f32| {
            let rad = (deg - 90.0) * std::f32::consts::PI / 180.0;
            D2D_POINT_2F {
                x: cx + rayon * rad.cos(),
                y: cy + rayon * rad.sin(),
            }
        };
        let debut = point(depart_deg);
        let fin = point(depart_deg + part * 360.0);
        unsafe {
            sink.BeginFigure(debut, D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: fin,
                size: D2D_SIZE_F {
                    width: rayon,
                    height: rayon,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()
        }
        .map_err(|e| ErreurTexte::Gpu(format!("Close orbite : {}", e.message())))?;
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse arc : {}", e.message())))?;
        let brosse: ID2D1Brush = brosse
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush arc : {}", e.message())))?;
        unsafe { self.cible_rt.DrawGeometry(&geo, &brosse, epaisseur, &self.styles.cadre) };
        Ok(())
    }

    /// Titre en dégradé de texte (`background-clip: text` du DOM).
    fn peindre_titre(
        &mut self,
        couleurs: ([u8; 4], [u8; 4]),
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let Some(ligne) = self.bloc_attente.as_ref().and_then(|b| b.titre.as_ref()) else {
            return Ok(());
        };
        let (layout, lh) = (ligne.layout.clone(), ligne.h);
        let x = TITRE_PAD * w;
        let y = TITRE_TOP * h;
        // Boîte du dégradé = boîte de l'ÉLÉMENT (padding box : 90 % de la
        // largeur, hauteur du texte), comme `background-clip: text`.
        let boite = D2D_RECT_F {
            left: x,
            top: y,
            right: x + (1.0 - 2.0 * TITRE_PAD) * w,
            bottom: y + lh,
        };
        let (prim, sec) = couleurs;
        let brosse = self.brosse_lineaire_css(&[(0.0, prim), (1.0, sec)], boite, 135.0)?;
        unsafe {
            self.cible_rt
                .DrawTextLayout(D2D_POINT_2F { x, y }, &layout, &brosse, D2D1_DRAW_TEXT_OPTIONS_NONE)
        };
        Ok(())
    }

    /// Carte minuteur : label + chrono, cadre SVG de la scène par-dessus.
    ///
    /// La carte est peinte dans son repère local puis mise à l'échelle autour
    /// de son centre (`left/top = centre` + `translate(-50%,-50%) scale`).
    /// Le **masque** du cadre, lui, est posé en coordonnées canvas **avant**
    /// la transformation : le polygone est construit à la taille écran (sinon
    /// la couche le transformerait une seconde fois).
    fn peindre_minuteur(
        &mut self,
        cfg: &AttenteCfg,
        couleurs: ([u8; 4], [u8; 4]),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let Some(bloc) = self.bloc_attente.as_ref() else {
            return Ok(());
        };
        let (cw, ch) = bloc.carte;
        let label = bloc.label.as_ref().map(|l| (l.layout.clone(), l.h, l.w));
        let temps = bloc.temps.as_ref().map(|l| (l.layout.clone(), l.h, l.w));
        let cadre = bloc.cadre.clone();
        let masque = bloc.masque.clone();

        let (mx, my, echelle) = cfg.minuteur;
        let (mx, my) = (mx as f32, my as f32);
        let echelle = (echelle as f32).clamp(0.4, 3.0);

        // Couche masquée par le polygone (à la taille écran).
        let masque_geo: Option<windows::Win32::Graphics::Direct2D::ID2D1Geometry> =
            match masque.as_ref() {
                Some(geo) => Some(geo.cast().map_err(|e| {
                    ErreurTexte::Gpu(format!("ID2D1Geometry attente : {}", e.message()))
                })?),
                None => None,
            };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: mx + cw * echelle + 64.0,
                bottom: my + ch * echelle + 64.0,
            },
            geometricMask: ManuallyDrop::new(masque_geo),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer minuteur : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };

        let resultat = (|| -> Result<(), ErreurTexte> {
            unsafe { self.cible_rt.SetTransform(&matrice_centre(mx, my, cw, ch, echelle)) };
            let boite = D2D_RECT_F {
                left: mx,
                top: my,
                right: mx + cw,
                bottom: my + ch,
            };
            let brosse_fond =
                unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(CARTE_FOND), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse carte attente : {}", e.message()))
                    })?;
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: boite,
                        radiusX: CARTE_RAYON,
                        radiusY: CARTE_RAYON,
                    },
                    &brosse_fond,
                )
            };
            // `border: 2px solid rgba(224,224,224,.85)` — recouvert par le
            // cadre SVG quand il est actif (le DOM ne le retire pas).
            let brosse_bord = unsafe {
                self.cible_rt
                    .CreateSolidColorBrush(&couleur_d2d(CARTE_BORD_COULEUR), None)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse bord attente : {}", e.message())))?;
            unsafe {
                self.cible_rt.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: boite.left + CARTE_BORD / 2.0,
                            top: boite.top + CARTE_BORD / 2.0,
                            right: boite.right - CARTE_BORD / 2.0,
                            bottom: boite.bottom - CARTE_BORD / 2.0,
                        },
                        radiusX: (CARTE_RAYON - CARTE_BORD / 2.0).max(0.0),
                        radiusY: (CARTE_RAYON - CARTE_BORD / 2.0).max(0.0),
                    },
                    &brosse_bord,
                    CARTE_BORD,
                    None,
                )
            };

            // Colonne `align-items: center` : label puis chrono, `gap 8px`.
            let mut y = my + CARTE_BORD + CARTE_PAD_V;
            if let Some((layout, lh, lw)) = &label {
                let x = mx + cw / 2.0 - lw / 2.0;
                let brosse = unsafe {
                    self.cible_rt
                        .CreateSolidColorBrush(&couleur_d2d(LABEL_COULEUR), None)
                }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse label attente : {}", e.message()))
                })?;
                unsafe {
                    self.cible_rt.DrawTextLayout(
                        D2D_POINT_2F { x, y },
                        layout,
                        &brosse,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
                y += lh + CARTE_GAP;
            }
            if let Some((layout, lh, lw)) = &temps {
                let x = mx + cw / 2.0 - lw / 2.0;
                // `linear-gradient(135deg, prim 82%→blanc, sec 60%→blanc)`.
                let pause = if cfg.en_pause { TEMPS_PAUSE_ALPHA } else { 1.0 };
                let att = |c: [u8; 4]| {
                    [
                        c[0],
                        c[1],
                        c[2],
                        (c[3] as f32 * pause).round().clamp(0.0, 255.0) as u8,
                    ]
                };
                let (prim, sec) = couleurs;
                let boite_temps = D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + lw,
                    bottom: y + lh,
                };
                let stops = [
                    (0.0, att(meler_blanc(prim, 0.82))),
                    (1.0, att(meler_blanc(sec, 0.60))),
                ];
                let brosse = self.brosse_lineaire_css(&stops, boite_temps, 135.0)?;
                unsafe {
                    self.cible_rt.DrawTextLayout(
                        D2D_POINT_2F { x, y },
                        layout,
                        &brosse,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
            }

            // `.cadre-overlay` — trait du cadre de scène sur la carte mesurée.
            if let Some((cfg_cadre, geo)) = &cadre {
                self.dessiner_cadre(cfg_cadre, geo, temps_s)?;
            }
            Ok(())
        })();
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        resultat
    }

    /// (Re)charge le média du fond quand `media|kind` change — comme la
    /// signature `eaMediaSig` du DOM (sinon la vidéo se rechargerait à chaque
    /// tick 1 Hz du chrono).
    fn synchroniser_media_attente(
        &mut self,
        cfg: &AttenteCfg,
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let rel = cfg.media.as_deref().filter(|m| !m.is_empty());
        let meme = self.attente_media_rel.as_deref() == rel
            && self.attente_media_kind.as_deref() == cfg.media_kind.as_deref();
        if meme {
            return Ok(());
        }
        self.attente_media = None;
        self.attente_media_rel = rel.map(|s| s.to_string());
        self.attente_media_kind = cfg.media_kind.clone();
        let Some(rel) = rel else {
            return Ok(());
        };
        let chemin = self.base.join(rel);
        let media_cfg = MediaCfg {
            chemin: chemin.clone(),
            // Rect/effets neutres : le média BRB est peint en plein cadre par
            // `dessiner_media_attente` (aucun widget, aucun cadre).
            rect: (0.0, 0.0, 0.0, 0.0),
            z: Z_ATTENTE,
            fit: Fit::Remplir,
            transform: Transform2D::NEUTRE,
            effets: EffetsCfg::NEUTRE,
            cadre: None,
            morphs: Vec::new(),
            pixel_morph: 0.0,
            // `v.loop = true` + `autoplay` : jamais en pause.
            lecture: LectureCfg {
                paused: false,
                boucle: None,
                boucle_infinie: true,
            },
            carrousel: None,
        };
        match crate::media::charger(&self.contexte, &media_cfg) {
            Ok(mut media) => {
                // Chargé en cours de session : la première frame est due
                // maintenant (sinon la vidéo sauterait à `temps_s`).
                if let Err(e) = media.recaler((temps_s * 1e6) as i64) {
                    log::debug!("attente : recalage média ({e})");
                }
                self.attente_media = Some(media);
            }
            Err(e) => log::debug!("attente : média {} ignoré ({e})", chemin.display()),
        }
        Ok(())
    }

    /// Anneau audio du média BRB (le mixeur y puise le son de l'attente).
    pub(crate) fn anneau_audio_attente(
        &self,
    ) -> Option<std::sync::Arc<std::sync::Mutex<crate::audio::live::AnneauAudio>>> {
        self.attente_media.as_ref()?.anneau_audio()
    }

    /// (Re)met en page le bloc quand la clé change — carte re-mesurée à
    /// chaque tick (le chrono change) et à chaque changement de scène.
    fn synchroniser_bloc_attente(
        &mut self,
        version: u64,
        cfg: &AttenteCfg,
        couleurs: ([u8; 4], [u8; 4]),
        cadre: Option<&CadreCfg>,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let cle = cle_attente(version, canvas, couleurs, cadre);
        if self.bloc_attente.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        let w = canvas.0 as f32;

        let titre = if cfg.afficher_titre && !cfg.titre.trim().is_empty() {
            Some(self.ligne_attente(LigneSpec {
                texte: &cfg.titre,
                taille: TITRE_TAILLE,
                poids: DWRITE_FONT_WEIGHT_EXTRA_BOLD,
                espacement: TITRE_ESPACEMENT,
                largeur: (1.0 - 2.0 * TITRE_PAD) * w,
                wrap: true,
                interligne: TITRE_INTERLIGNE,
                centre: true,
            })?)
        } else {
            None
        };
        let label = Some(self.ligne_attente(LigneSpec {
            texte: if cfg.en_pause { LABEL_PAUSE } else { LABEL_ATTENTE },
            taille: LABEL_TAILLE,
            poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
            espacement: LABEL_ESPACEMENT,
            largeur: 4096.0,
            wrap: false,
            interligne: 1.0,
            centre: false,
        })?);
        let texte_temps = formater_temps(cfg.temps_restant);
        let temps = Some(self.ligne_attente(LigneSpec {
            texte: &texte_temps,
            taille: TEMPS_TAILLE,
            poids: DWRITE_FONT_WEIGHT_EXTRA_BOLD,
            espacement: 0.0,
            largeur: 4096.0,
            wrap: false,
            interligne: 1.0,
            centre: false,
        })?);

        // Carte : `padding 24px 64px`, `gap 8px`, `border 2px` (border-box).
        let cw = label.as_ref().map(|l| l.w).unwrap_or(0.0).max(
            temps.as_ref().map(|l| l.w).unwrap_or(0.0),
        ) + 2.0 * CARTE_PAD_H
            + 2.0 * CARTE_BORD;
        let ch = label.as_ref().map(|l| l.h).unwrap_or(0.0)
            + CARTE_GAP
            + temps.as_ref().map(|l| l.h).unwrap_or(0.0)
            + 2.0 * CARTE_PAD_V
            + 2.0 * CARTE_BORD;
        let carte = (cw.max(1.0), ch.max(1.0));

        // Cadre : trait à la taille **non mise à l'échelle** (le DOM met à
        // l'échelle le SVG entier), masque à la taille écran.
        let (mx, my, echelle) = cfg.minuteur;
        let (mx, my) = (mx as f32, my as f32);
        let echelle = (echelle as f32).clamp(0.4, 3.0);
        let (cadre_geo, masque) = match cadre {
            Some(gabarit) => {
                let mut cfg_cadre = gabarit.clone();
                cfg_cadre.rect = (mx as f64, my as f64, carte.0 as f64, carte.1 as f64);
                cfg_cadre.z = Z_ATTENTE;
                let geo = match self.geometrie_cadre(&cfg_cadre) {
                    Ok(geo) => Some((cfg_cadre, geo)),
                    Err(e) => {
                        log::warn!("attente : polygone du cadre ignoré ({e})");
                        None
                    }
                };
                let mut cfg_masque = gabarit.clone();
                let (sw, sh) = (carte.0 * echelle, carte.1 * echelle);
                cfg_masque.rect = (
                    (mx - sw / 2.0) as f64,
                    (my - sh / 2.0) as f64,
                    sw as f64,
                    sh as f64,
                );
                cfg_masque.z = Z_ATTENTE;
                let masque = match self.geometrie_cadre(&cfg_masque) {
                    Ok(geo) => Some(geo),
                    Err(e) => {
                        log::warn!("attente : masque du cadre ignoré ({e})");
                        None
                    }
                };
                (geo, masque)
            }
            None => (None, None),
        };

        self.bloc_attente = Some(BlocAttente {
            cle,
            titre,
            label,
            temps,
            carte,
            cadre: cadre_geo,
            masque,
        });
        Ok(())
    }

    /// Met en page une ligne de l'écran d'attente — `system-ui` du body, donc
    /// famille système (`Segoe UI`), poids et interligne explicites (le CSS
    /// utilise 800/600 et `line-height: 1`/`1.1`, hors du vocabulaire
    /// `StyleTexte`).
    fn ligne_attente(&self, spec: LigneSpec<'_>) -> Result<LigneA, ErreurTexte> {
        let famille: Vec<u16> = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        let format = unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(famille.as_ptr()),
                None,
                spec.poids,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                spec.taille,
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat attente : {}", e.message())))?;
        unsafe {
            format.SetTextAlignment(if spec.centre {
                DWRITE_TEXT_ALIGNMENT_CENTER
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            })
        }
        .map_err(|e| ErreurTexte::Systeme(format!("alignement attente : {}", e.message())))?;
        // `line-height` CSS : hauteur de ligne exacte (sinon DirectWrite
        // prend ~1,2 em et la carte minuteur grandit de ~15 px).
        if (spec.interligne - 1.0).abs() > f32::EPSILON || spec.taille > 40.0 {
            unsafe {
                format.SetLineSpacing(
                    DWRITE_LINE_SPACING_METHOD_UNIFORM,
                    spec.taille * spec.interligne,
                    spec.taille * 0.8,
                )
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("interligne attente : {}", e.message()))
            })?;
        }
        let contenu: Vec<u16> = spec.texte.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, spec.largeur.max(1.0), 4096.0)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("layout attente : {}", e.message())))?;
        unsafe {
            layout.SetWordWrapping(if spec.wrap {
                DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            })
        }
        .map_err(|e| ErreurTexte::Systeme(format!("wrap attente : {}", e.message())))?;
        if spec.espacement != 0.0 {
            let layout1: windows::Win32::Graphics::DirectWrite::IDWriteTextLayout1 =
                layout.cast().map_err(|e| {
                    ErreurTexte::Systeme(format!("IDWriteTextLayout1 attente : {}", e.message()))
                })?;
            unsafe {
                layout1.SetCharacterSpacing(
                    0.0,
                    spec.espacement,
                    0.0,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: contenu.len() as u32,
                    },
                )
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("espacement attente : {}", e.message()))
            })?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques attente : {}", e.message())))?;
        Ok(LigneA {
            layout,
            _format: format,
            h: metriques.height,
            w: metriques.widthIncludingTrailingWhitespace,
        })
    }

    /// Brosse linéaire d'un `linear-gradient(<angle>deg, …)` CSS : la ligne de
    /// dégradé traverse la boîte (`|w·sin θ| + |h·cos θ|`) et l'angle est
    /// mesuré depuis le **haut**, sens horaire.
    fn brosse_lineaire_css(
        &self,
        stops: &[(f32, [u8; 4])],
        boite: D2D_RECT_F,
        angle_deg: f32,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let (w, h) = (boite.right - boite.left, boite.bottom - boite.top);
        let rad = angle_deg.to_radians();
        let (dx, dy) = (rad.sin(), -rad.cos());
        let longueur = (w * rad.sin()).abs() + (h * rad.cos()).abs();
        let longueur = longueur.max(1.0);
        let (cx, cy) = (
            (boite.left + boite.right) / 2.0,
            (boite.top + boite.bottom) / 2.0,
        );
        let arrets: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, couleur)| D2D1_GRADIENT_STOP {
                position: *position,
                color: couleur_d2d(*couleur),
            })
            .collect();
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&arrets, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé attente : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: D2D_POINT_2F {
                x: cx - dx * longueur / 2.0,
                y: cy - dy * longueur / 2.0,
            },
            endPoint: D2D_POINT_2F {
                x: cx + dx * longueur / 2.0,
                y: cy + dy * longueur / 2.0,
            },
        };
        let brosse: ID2D1LinearGradientBrush =
            unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse dégradé attente : {}", e.message()))
                })?;
        Ok(brosse.into())
    }

    /// Asset de marque décodé une fois (texture + vue D2D + taille source).
    ///
    /// Rend une **copie** des handles : le renderer garde `&mut self` pour
    /// dessiner (aucun emprunt du cache ne survit à l'appel).
    fn asset_marque(&mut self, lequel: AssetMarque) -> Option<Asset> {
        if !self.assets_attente.contains_key(&lequel) {
            let octets = match lequel {
                AssetMarque::StreamyChargement => STREAMY_CHARGEMENT,
                AssetMarque::StreamyRepos => STREAMY_REPOS,
                AssetMarque::Wordmark => WORDMARK_PNG,
            };
            let (largeur, hauteur, pixels, alpha) =
                crate::media::decoder_pbgra_memoire(octets).ok()?;
            if largeur == 0 || hauteur == 0 {
                return None;
            }
            // `mix-blend-mode: screen` : un fond noir devient transparent
            // (alpha dérivée de la luminance, voir les limites du module).
            let (pixels, alpha) = if lequel == AssetMarque::Wordmark {
                (ecreter_fond_noir(&pixels), true)
            } else {
                (pixels, alpha)
            };
            let desc = DescTexture::new(Format::Bgra8, largeur, hauteur).ok()?;
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
                    largeur * 4,
                    0,
                )
            };
            let bitmap = self.bitmap_media(&texture, alpha).ok()?;
            self.assets_attente.insert(
                lequel,
                (texture, bitmap, (largeur as f32, hauteur as f32)),
            );
        }
        self.assets_attente.get(&lequel).cloned()
    }
}

/// Paramètres d'un arc d'orbite — regroupement clippy (`too_many_arguments`,
/// convention §37.4.5).
struct ArcOrbite {
    cx: f32,
    cy: f32,
    rayon: f32,
    epaisseur: f32,
    /// Part visible du tour (0..1).
    part: f32,
    couleur: [u8; 4],
    /// Angle de départ en degrés CSS (0° = haut, sens horaire).
    depart_deg: f32,
}

/// Spécification d'une ligne de l'écran d'attente.
struct LigneSpec<'a> {
    texte: &'a str,
    taille: f32,
    poids: windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT,
    espacement: f32,
    largeur: f32,
    wrap: bool,
    interligne: f32,
    /// `true` = `text-align: center` dans la boîte de mise en page (titre) ;
    /// `false` = leading, la ligne est centrée à la main (label/chrono, dont
    /// la boîte est celle du texte).
    centre: bool,
}

/// Matrice CSS `translate(-50%,-50%) scale(s)` avec `left/top = centre` et
/// `transform-origin: center` : le centre de la boîte `(w, h)` atterrit en
/// `(cx, cy)` et tout est mis à l'échelle autour de lui.
fn matrice_centre(cx: f32, cy: f32, w: f32, h: f32, echelle: f32) -> Matrix3x2 {
    let s = echelle.max(0.01);
    // Origine = centre naturel de l'élément (left/top = cx, cy).
    let (ox, oy) = (cx + w / 2.0, cy + h / 2.0);
    Matrix3x2 {
        M11: s,
        M12: 0.0,
        M21: 0.0,
        M22: s,
        M31: ox - s * ox - w / 2.0,
        M32: oy - s * oy - h / 2.0,
    }
}

/// Alpha d'un PNG à fond noir : luminance perçue → alpha, canaux re-pré-
/// multipliés (PBGRA). Approximation additive de `mix-blend-mode: screen`
/// sur un fond sombre (noir → transparent, pic lumineux → opaque).
fn ecreter_fond_noir(pixels: &[u8]) -> Vec<u8> {
    let mut sortie = Vec::with_capacity(pixels.len());
    for p in pixels.chunks_exact(4) {
        let (b, g, r, a) = (p[0], p[1], p[2], p[3]);
        let luminance = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
        let alpha = (luminance * a as u32 / 255) as u8;
        let premultiplie = |v: u8| ((v as u32 * alpha as u32) / 255) as u8;
        sortie.extend_from_slice(&[premultiplie(b), premultiplie(g), premultiplie(r), alpha]);
    }
    sortie
}

/// Assets de marque du layout « signature ».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum AssetMarque {
    StreamyChargement,
    StreamyRepos,
    Wordmark,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn actif() -> AttenteCfg {
        AttenteCfg {
            actif: true,
            temps_restant: 600,
            titre: "On revient".to_string(),
            media: Some("medias/a.mp4".to_string()),
            media_kind: Some("video".to_string()),
            ..AttenteCfg::default()
        }
    }

    #[test]
    fn etat_initial_puis_play_et_fondu_entree() {
        let mut etat = EtatAttente::nouveau();
        assert!(etat.rendu().is_none(), "inactif au boot");
        assert!(!etat.actif());
        assert!(!etat.mute_bureau(), "aucun mute hors attente");

        let t0 = Instant::now();
        etat.appliquer_at(actif(), t0);
        assert_eq!(etat.version(), 1);
        assert!(etat.actif());
        assert!(etat.mute_bureau(), "muter_bureau du domaine = true");
        // `opacity 0 → 1` sur 0,45 s `ease`.
        let depart = etat.rendu_at(t0).expect("écran visible");
        assert!(depart.opacite.abs() < 1e-3, "opacité 0 au départ");
        let milieu = etat.rendu_at(t0 + Duration::from_millis(225)).expect("fondu");
        assert!(milieu.opacite > 0.3 && milieu.opacite < 0.9, "{}", milieu.opacite);
        let stable = etat.rendu_at(t0 + Duration::from_millis(600)).expect("stable");
        assert!((stable.opacite - 1.0).abs() < 1e-3);
        assert_eq!(stable.cfg.temps_restant, 600);
    }

    #[test]
    fn stop_avec_fondu_de_sortie_puis_masque() {
        let mut etat = EtatAttente::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(actif(), t0);
        let mut stop = actif();
        stop.actif = false;
        etat.appliquer_at(stop, t0 + Duration::from_millis(1000));
        assert_eq!(etat.version(), 2);
        assert!(!etat.actif());
        assert!(!etat.mute_bureau(), "le stop rend l'audio bureau");
        let sortie = etat
            .rendu_at(t0 + Duration::from_millis(1200))
            .expect("fondu en cours");
        assert!(sortie.opacite > 0.0 && sortie.opacite < 1.0);
        assert!(
            etat.rendu_at(t0 + Duration::from_millis(1500)).is_none(),
            "masqué après 0,45 s"
        );
        // Un second stop est sans effet (idempotent).
        let mut encore = actif();
        encore.actif = false;
        etat.appliquer_at(encore, t0 + Duration::from_millis(1600));
        assert_eq!(etat.version(), 2);
    }

    #[test]
    fn relance_pendant_la_sortie_rejoue_l_entree() {
        let mut etat = EtatAttente::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(actif(), t0);
        let mut stop = actif();
        stop.actif = false;
        etat.appliquer_at(stop, t0 + Duration::from_millis(500));
        etat.appliquer_at(actif(), t0 + Duration::from_millis(700));
        assert!(etat.actif());
        let reprise = etat.rendu_at(t0 + Duration::from_millis(700)).expect("visible");
        assert!(reprise.opacite.abs() < 1e-3, "fondu d'entrée rejoué");
    }

    #[test]
    fn frame_identique_ne_change_pas_la_version() {
        let mut etat = EtatAttente::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(actif(), t0);
        etat.appliquer_at(actif(), t0 + Duration::from_millis(1000));
        assert_eq!(etat.version(), 1, "tick sans changement = pas de re-mise en page");
        let mut autre = actif();
        autre.temps_restant = 599;
        etat.appliquer_at(autre, t0 + Duration::from_millis(2000));
        assert_eq!(etat.version(), 2);
    }

    #[test]
    fn temps_formate_comme_ea_format_temps() {
        assert_eq!(formater_temps(0), "0:00");
        assert_eq!(formater_temps(59), "0:59");
        assert_eq!(formater_temps(600), "10:00");
        assert_eq!(formater_temps(3599), "59:59");
        assert_eq!(formater_temps(3600), "1:00:00");
        assert_eq!(formater_temps(3725), "1:02:05");
    }

    #[test]
    fn gain_media_suit_son_et_volume() {
        let mut etat = EtatAttente::nouveau();
        assert_eq!(etat.gain_media_milliemes(), 0, "aucun son hors attente");
        let t0 = Instant::now();
        let mut cfg = actif();
        cfg.volume = 0.5;
        etat.appliquer_at(cfg.clone(), t0);
        assert_eq!(etat.gain_media_milliemes(), 500);
        cfg.volume = 0.8;
        etat.appliquer_at(cfg.clone(), t0 + Duration::from_millis(10));
        assert_eq!(etat.gain_media_milliemes(), 800);
        cfg.son = false;
        etat.appliquer_at(cfg, t0 + Duration::from_millis(20));
        assert_eq!(etat.gain_media_milliemes(), 0, "`muted = !son`");
    }

    #[test]
    fn melange_blanc_miroir_de_color_mix() {
        // `color-mix(in srgb, #e0e0e0 82%, #ffffff)` → 224 × 0,82 + 255 × 0,18.
        assert_eq!(meler_blanc([0xe0, 0xe0, 0xe0, 255], 0.82), [230, 230, 230, 255]);
        // 0 % de la couleur = blanc pur ; 100 % = la couleur.
        assert_eq!(meler_blanc([10, 20, 30, 255], 0.0), [255, 255, 255, 255]);
        assert_eq!(meler_blanc([10, 20, 30, 255], 1.0), [10, 20, 30, 255]);
    }

    #[test]
    fn matrice_centre_place_le_centre_sur_la_config() {
        // Boîte 200×100 à `left/top = (960, 540)`, échelle 2.
        let m = matrice_centre(960.0, 540.0, 200.0, 100.0, 2.0);
        // Le centre de la boîte (1060, 590) doit atterrir en (960, 540).
        let x = m.M11 * 1060.0 + m.M31;
        let y = m.M22 * 590.0 + m.M32;
        assert!((x - 960.0).abs() < 1e-3, "x = {x}");
        assert!((y - 540.0).abs() < 1e-3, "y = {y}");
        // Le bord gauche (960) est à 100 px du centre → 200 px après ×2.
        let gauche = m.M11 * 960.0 + m.M31;
        assert!((gauche - 760.0).abs() < 1e-3, "gauche = {gauche}");
    }

    #[test]
    fn ecreter_fond_noir_rend_le_noir_transparent() {
        // PBGRA opaque : noir, gris, blanc.
        let pixels = [
            0u8, 0, 0, 255, // noir
            128, 128, 128, 255, // gris
            255, 255, 255, 255, // blanc
        ];
        let sortie = ecreter_fond_noir(&pixels);
        assert_eq!(&sortie[0..4], &[0, 0, 0, 0], "noir → alpha 0");
        assert_eq!(sortie[11], 255, "blanc → opaque");
        let gris = sortie[7];
        assert!(gris > 100 && gris < 150, "luminance du gris : {gris}");
        // Prémultiplié : les canaux ne dépassent jamais l'alpha.
        for p in sortie.chunks_exact(4) {
            assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]);
        }
    }

    #[test]
    fn bezier_css_borne_et_monotone() {
        assert_eq!(bezier_css(EASE, 0.0), 0.0);
        assert!((bezier_css(EASE, 1.0) - 1.0).abs() < 1e-6);
        let mut precedent = -1.0;
        for i in 0..=20 {
            let v = bezier_css(EASE, i as f32 / 20.0);
            assert!(v >= precedent, "bezier non monotone à {i}");
            precedent = v;
        }
        // `ease-in-out` : symétrique.
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-3);
    }
}
