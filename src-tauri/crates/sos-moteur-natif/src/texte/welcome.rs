//! Overlay « clip de bienvenue » natif — carte transitoire z 9999.
//!
//! Miroir de `playWelcomeClip` / `stopWelcomeClip` /
//! `applyWelcomeOverlayConfig` (diffusion.html) : carte verticale
//! `.widget.welcome-clip` au squelette de position unifié
//! (`welcome-clip-config` / `position-overlay-config` — x/y/largeur,
//! hauteur auto), avatar rond + pseudo + CTA « Checkout and follow » + bio
//! (2 lignes), puis clip 16:9. Mode « spotlight »
//! (`utiliser_squelette:false`) : voile noir plein écran + carte centrée à
//! 85 % en split gauche/droite, intro avatar de 3 s puis transition 600 ms.
//!
//! Toute la logique métier reste au domaine (`welcome.rs`) : registre,
//! file FIFO, timer autoritaire, résolution MP4. Le natif ne peint que le
//! clip **en cours** — les frames `welcome-clip-play` / `welcome-clip-stop` /
//! `welcome-clip-config` du canal `chat_tx` alimentent [`EtatWelcome`] via
//! `MoteurNatif::pousser_welcome`, comme `EtatAlertes`. Jamais par la
//! scène : l'overlay est transitoire.
//!
//! Limites assumées du lot (parité partielle documentée) :
//! - **aucun média** : `clip.mp4_url`, `viewer.avatar`, `viewer.banniere` et
//!   les avatars des favoris sont des **URLs distantes** — le moteur natif ne
//!   charge que des fichiers locaux (`media::decoder_pbgra`,
//!   `MFCreateSourceReaderFromURL`) et n'a aucun cache HTTP (lot G) ;
//! - conséquences mesurables, identiques au DOM **sans** média : l'avatar
//!   est absent (`display:none`), la colonne droite (clip 16:9) n'apparaît
//!   jamais (elle attend `canplay`), le fond spotlight retombe sur le noir du
//!   voile, la rangée des favoris reste vide ;
//! - **aucun son** : `audio/` est une capture WASAPI, pas un lecteur ;
//! - `backdrop-filter` du panneau spotlight, `box-shadow` et
//!   `drop-shadow` du titre de plateforme non portés ;
//! - `line-height` CSS (1,2 / 1,1 / 1,3) non porté : interligne DirectWrite.

use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Geometry, ID2D1GradientStopCollection, ID2D1Layer,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};

use crate::cadres::CadreCfg;

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

/// z du calque bienvenue — `.welcome-clip-root` (z-index 9999).
pub const Z_WELCOME: i32 = 9999;

/// `cqw` — 1 % de la largeur de la carte (`container-type: inline-size` sur
/// `.widget.welcome-clip`).
const CQW: f32 = 0.01;

/// `.welcome-clip-root.spot { width: 85% }` — carte centrée du spotlight.
const SPOT_LARGEUR: f32 = 0.85;
/// `flex: 0 0 42%` de `.welcome-clip-entete` en spotlight.
const ENTETE_SPLIT: f32 = 0.42;

/// Voile noir du spotlight : `transition: opacity 350ms ease`.
const VOILE_S: f32 = 0.35;
/// Entrée de la carte spotlight : `550ms` avec `120ms` de délai.
const ENTREE_DELAI_S: f32 = 0.12;
const ENTREE_S: f32 = 0.55;
/// Intro spotlight : avatar seul 3 s (`INTRO_SPOTLIGHT_MS` de welcome.rs).
const INTRO_S: f32 = 3.0;
/// `transition: flex-basis 600ms ease` de l'en-tête au split.
const BASIS_S: f32 = 0.6;
/// `transform: translateY(26px) scale(0.94)` hors `.enter`.
const ENTREE_Y: f32 = 26.0;
const ENTREE_SCALE: f32 = 0.94;

/// `ease` CSS — voile, opacité de la carte, flex-basis.
const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);
/// `cubic-bezier(0.16, 1, 0.3, 1)` — transform de la carte.
const EASE_SORTIE: (f32, f32, f32, f32) = (0.16, 1.0, 0.3, 1.0);

/// Fond de la carte (`rgba(10,10,10,0.92)`).
const FOND: [u8; 4] = [10, 10, 10, 235];
/// Fond du panneau spotlight (`rgba(12,14,22,0.55)`).
const FOND_SPOT: [u8; 4] = [12, 14, 22, 140];
/// Bord du panneau spotlight (`rgba(255,255,255,0.14)`).
const BORD_SPOT: [u8; 4] = [255, 255, 255, 36];
/// Texte (`--texte: #e0e0e0`).
const TEXTE: [u8; 4] = [0xe0, 0xe0, 0xe0, 255];
/// `.welcome-clip-cta { opacity: 0.6 }` → alpha 153.
const ALPHA_CTA: u8 = 153;
/// `.welcome-clip-bio { opacity: 0.85 }` → alpha 217.
const ALPHA_BIO: u8 = 217;
/// `.welcome-clip-message { opacity: 0.9 }` → alpha 230.
const ALPHA_MESSAGE: u8 = 230;
/// Stops du titre « Streamer Twitch » (`#c9a2ff` 0 %, `#9146ff` 55 %,
/// `#e0bfff` 100 %).
const PLATEFORME_STOPS: [(f32, [u8; 4]); 3] = [
    (0.0, [0xc9, 0xa2, 0xff, 255]),
    (0.55, [0x91, 0x46, 0xff, 255]),
    (1.0, [0xe0, 0xbf, 0xff, 255]),
];
/// Titre de plateforme — constante du DOM (jamais i18n).
const PLATEFORME: &str = "Streamer Twitch";
/// CTA du DOM (`ctaEl.textContent`, jamais i18n).
const CTA: &str = "Checkout and follow";
/// Squelette initial (`PositionOverlayConfig::default` — bas-droite
/// 320×352 sur 1920×1080).
const POSITION_DEFAUT: (f64, f64, f64, f64) = (1480.0, 580.0, 320.0, 352.0);

// =============================================================================
// FRAMES — mutations décodées par le pont (`src/moteur/mod.rs`).
// =============================================================================

/// Mutation de l'overlay bienvenue — frames `chat_tx`.
#[derive(Debug, Clone, PartialEq)]
pub enum MajWelcome {
    /// `welcome-clip-play` : le domaine a déjà résolu le clip et la file.
    Play {
        /// `viewer.display_name` (repli `login` fait par le pont).
        display_name: String,
        /// `viewer.bio` — masquée si vide (2 lignes en squelette).
        bio: Option<String>,
        /// Message de bienvenue — rendu en spotlight seulement.
        message: String,
        /// `true` = voile plein écran + carte 85 % (`!utiliser_squelette`).
        spotlight: bool,
    },
    /// `welcome-clip-stop` : l'overlay disparaît — pas de fondu de sortie
    /// (`stopWelcomeClip` passe le root en `display:none`).
    Stop,
    /// Squelette de position : `welcome-clip-config` (mode inclus) ou
    /// `position-overlay-config` (mode conservé — `spotlight` absent).
    Config {
        x: f64,
        y: f64,
        largeur: f64,
        /// Référence visuelle du squelette — la carte reste en hauteur auto.
        hauteur: f64,
        spotlight: Option<bool>,
    },
}

// =============================================================================
// ÉTAT LIVE — alimenté pendant la session, jamais par la scène.
// =============================================================================

/// Clip affiché (l'état interne de `playWelcomeClip`).
#[derive(Debug)]
struct ClipEnCours {
    display_name: String,
    bio: Option<String>,
    message: String,
    /// Début d'affichage — base des phases (voile, entrée, intro, split).
    debut: Instant,
}

/// Ce que la composition peint ce tick-ci (phases déjà résolues).
pub struct RenduWelcome<'a> {
    /// `viewer.display_name` (pseudo du viewer).
    pub display_name: &'a str,
    /// Bio Twitch (2 lignes en squelette, 4 en spotlight).
    pub bio: Option<&'a str>,
    /// Message de bienvenue (spotlight uniquement).
    pub message: &'a str,
    /// Mode spotlight (`!utiliser_squelette`).
    pub spotlight: bool,
    /// Squelette de position courant (x, y, largeur, hauteur).
    pub position: (f64, f64, f64, f64),
    /// Opacité du voile noir (spotlight, fondu 350 ms).
    pub voile: f32,
    /// Opacité de la carte (entrée spotlight, 550 ms + 120 ms).
    pub opacite: f32,
    /// Avancement de l'entrée spotlight (transform, même minuterie).
    pub entree: f32,
    /// Phase intro : textes masqués, colonne droite cachée.
    pub intro: bool,
    /// Fraction de la largeur utile occupée par l'en-tête (1 → 0,42).
    pub entete: f32,
}

/// État vivant de l'overlay : squelette + mode + clip en cours + version
/// de contenu (clé de cache du bloc de rendu).
pub struct EtatWelcome {
    position: (f64, f64, f64, f64),
    spotlight: bool,
    en_cours: Option<ClipEnCours>,
    version: u64,
}

impl Default for EtatWelcome {
    fn default() -> Self {
        Self {
            position: POSITION_DEFAUT,
            spotlight: false,
            en_cours: None,
            version: 0,
        }
    }
}

impl EtatWelcome {
    pub fn nouveau() -> Self {
        Self::default()
    }

    /// Applique une mutation ; `version` n'augmente que sur changement réel
    /// (dedup — `position-overlay-config` est ré-émis à chaque connexion WS).
    pub fn appliquer(&mut self, maj: MajWelcome) {
        self.appliquer_at(maj, Instant::now());
    }

    fn appliquer_at(&mut self, maj: MajWelcome, maintenant: Instant) {
        match maj {
            MajWelcome::Play {
                display_name,
                bio,
                message,
                spotlight,
            } => {
                self.spotlight = spotlight;
                self.en_cours = Some(ClipEnCours {
                    display_name,
                    bio,
                    message,
                    debut: maintenant,
                });
                self.version = self.version.wrapping_add(1);
            }
            MajWelcome::Stop => {
                if self.en_cours.take().is_some() {
                    self.version = self.version.wrapping_add(1);
                }
            }
            MajWelcome::Config {
                x,
                y,
                largeur,
                hauteur,
                spotlight,
            } => {
                // La hauteur du squelette est une référence visuelle : la
                // carte reste en hauteur auto (`height:auto` du DOM). Elle
                // est conservée telle quelle (diagnostic), jamais rendue.
                let position = (x, y, largeur, hauteur);
                let mode = spotlight.unwrap_or(self.spotlight);
                if self.position == position && self.spotlight == mode {
                    return;
                }
                self.position = position;
                self.spotlight = mode;
                self.version = self.version.wrapping_add(1);
            }
        }
    }

    /// Compteur monotone (clé de cache du rendu).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Phases au temps courant — `None` sans clip (aucun dessin).
    pub fn rendu(&self) -> Option<RenduWelcome<'_>> {
        self.rendu_at(Instant::now())
    }

    fn rendu_at(&self, maintenant: Instant) -> Option<RenduWelcome<'_>> {
        let clip = self.en_cours.as_ref()?;
        let t = maintenant.saturating_duration_since(clip.debut).as_secs_f32();
        let (voile, opacite, entree, intro, entete) = if self.spotlight {
            // Voile : `opacity 0 → 1` sur 350 ms.
            let voile = bezier_css(EASE, (t / VOILE_S).min(1.0));
            // Carte : opacité 550 ms (`ease`) + transform 550 ms
            // (`cubic-bezier(0.16,1,0.3,1)`), retardées de 120 ms.
            let p = ((t - ENTREE_DELAI_S) / ENTREE_S).clamp(0.0, 1.0);
            let opacite = bezier_css(EASE, p);
            let entree = bezier_css(EASE_SORTIE, p);
            // Intro 3 s : avatar seul ; puis split (`flex-basis` 600 ms).
            let intro = t < INTRO_S;
            let q = ((t - INTRO_S) / BASIS_S).clamp(0.0, 1.0);
            let entete = if intro {
                1.0
            } else {
                1.0 - (1.0 - ENTETE_SPLIT) * bezier_css(EASE, q)
            };
            (voile, opacite, entree, intro, entete)
        } else {
            // Squelette : aucune transition (le root passe en `display:block`).
            (0.0, 1.0, 1.0, false, 1.0)
        };
        Some(RenduWelcome {
            display_name: &clip.display_name,
            bio: clip.bio.as_deref(),
            message: &clip.message,
            spotlight: self.spotlight,
            position: self.position,
            voile,
            opacite,
            entree,
            intro,
            entete,
        })
    }
}

/// Évalue une `cubic-bezier(x1,y1,x2,y2)` CSS à `x` ∈ [0,1] — même
/// résolution par dichotomie que `alertes::bezier_css` (dupliquée : les deux
/// renderers d'overlay sont indépendants, comme `LigneAlerte`/`LigneSr`).
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

// =============================================================================
// GÉOMÉTRIE — miroir des `cqw` de diffusion.html.
// =============================================================================

/// `clamp(min, val, max)` CSS.
fn clamp(min: f32, val: f32, max: f32) -> f32 {
    val.max(min).min(max)
}

/// Métriques du bloc, dérivées de la largeur de carte (`cqw` = 1 % de `w`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Metriques {
    /// Largeur de la carte.
    w: f32,
    /// `padding: 1.5cqw` (squelette) / `3cqw` (spotlight).
    pad: f32,
    /// `gap: 0.5cqw` de `.welcome-clip-entete` / `1.2cqw` en spotlight.
    gap_entete: f32,
    /// `gap: 2.5cqw` de la rangée spotlight.
    gap_row: f32,
    /// `border-radius` (12 px / 18 px).
    rayon: f32,
    /// `border` du panneau spotlight (1 px) — 0 en squelette.
    bord: f32,
    fond: [u8; 4],
    taille_pseudo: f32,
    taille_cta: f32,
    taille_bio: f32,
    taille_message: f32,
    taille_plateforme: f32,
    /// `-webkit-line-clamp` de la bio (2 / 4).
    lignes_bio: u32,
}

impl Metriques {
    fn new(w: f32, spotlight: bool) -> Self {
        if spotlight {
            Self {
                w,
                pad: 3.0 * CQW * w,
                gap_entete: 1.2 * CQW * w,
                gap_row: 2.5 * CQW * w,
                rayon: 18.0,
                bord: 1.0,
                fond: FOND_SPOT,
                // `clamp(1rem, 3cqw, 2.2rem)`
                taille_pseudo: clamp(16.0, 3.0 * CQW * w, 35.2),
                // `clamp(0.7rem, 1.6cqw, 1rem)`
                taille_cta: clamp(11.2, 1.6 * CQW * w, 16.0),
                // `clamp(0.8rem, 1.8cqw, 1.2rem)`
                taille_bio: clamp(12.8, 1.8 * CQW * w, 19.2),
                // `clamp(0.8rem, 2.2cqw, 1.4rem)`
                taille_message: clamp(12.8, 2.2 * CQW * w, 22.4),
                // `clamp(0.9rem, 2.6cqw, 1.7rem)`
                taille_plateforme: clamp(14.4, 2.6 * CQW * w, 27.2),
                lignes_bio: 4,
            }
        } else {
            Self {
                w,
                pad: 1.5 * CQW * w,
                gap_entete: 0.5 * CQW * w,
                gap_row: 0.8 * CQW * w,
                rayon: 12.0,
                bord: 0.0,
                fond: FOND,
                // `clamp(0.65rem, 3.5cqw, 1.1rem)`
                taille_pseudo: clamp(10.4, 3.5 * CQW * w, 17.6),
                // `clamp(0.5rem, 2.2cqw, 0.72rem)`
                taille_cta: clamp(8.0, 2.2 * CQW * w, 11.52),
                // `clamp(0.55rem, 2.5cqw, 0.82rem)`
                taille_bio: clamp(8.8, 2.5 * CQW * w, 13.12),
                taille_message: 0.0,
                taille_plateforme: 0.0,
                lignes_bio: 2,
            }
        }
    }

    /// Largeur utile (carte moins le padding horizontal).
    fn utile(&self) -> f32 {
        (self.w - 2.0 * self.pad).max(1.0)
    }
}

// =============================================================================
// BLOC DE RENDU — layouts mis en page une fois par (version, mode, taille).
// =============================================================================

/// Une ligne mise en page du bloc.
struct LigneW {
    layout: IDWriteTextLayout,
    /// Le format doit vivre aussi longtemps que le layout.
    _format: IDWriteTextFormat,
    /// Hauteur des métriques (interligne DirectWrite).
    h: f32,
    /// Largeur d'avance (`widthIncludingTrailingWhitespace`) — barre du
    /// titre de plateforme.
    w: f32,
}

/// Cache de la carte : layouts + polygone du cadre (squelette).
pub(super) struct BlocWelcome {
    /// Clé de validité (`cle_welcome`).
    cle: u64,
    /// Rect de la carte en mode squelette `(x, y, w, h)` — hauteur auto
    /// mesurée ; en spotlight la carte est centrée et re-mesurée par frame.
    carte: (f32, f32, f32, f32),
    /// Métriques du mode (largeur de référence des `cqw`).
    metriques: Metriques,
    pseudo: Option<LigneW>,
    message: Option<LigneW>,
    cta: Option<LigneW>,
    bio: Option<LigneW>,
    plateforme: Option<LigneW>,
    /// Cadre concret de la carte (rect appliqué) + son polygone — squelette
    /// uniquement (`nettoyerCadreOverlay` du spotlight).
    cadre: Option<(CadreCfg, ID2D1PathGeometry)>,
}

/// Clé de cache du bloc : version de l'état + mode + squelette + canvas +
/// gabarit de cadre.
fn cle_welcome(
    version: u64,
    spotlight: bool,
    position: (f64, f64, f64, f64),
    canvas: (u32, u32),
    cadre: Option<&CadreCfg>,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    spotlight.hash(&mut h);
    position.0.to_bits().hash(&mut h);
    position.1.to_bits().hash(&mut h);
    position.2.to_bits().hash(&mut h);
    canvas.hash(&mut h);
    if let Some(c) = cadre {
        c.style.hash(&mut h);
        c.stroke_width.to_bits().hash(&mut h);
        c.couleur.hash(&mut h);
        c.couleur_fin.hash(&mut h);
        c.gradient_angle.to_bits().hash(&mut h);
    }
    h.finish()
}

/// Somme des hauteurs visibles + les `gap` qui les séparent (une ligne vide
/// n'occupe aucune place dans une colonne flex).
fn hauteur_colonne(hauteurs: &[Option<f32>], gap: f32) -> f32 {
    let mut h = 0.0;
    let mut n = 0;
    for hauteur in hauteurs.iter().flatten() {
        h += *hauteur;
        n += 1;
    }
    if n > 1 {
        h += gap * (n - 1) as f32;
    }
    h
}

impl DessinateurTexte {
    /// Dessine l'overlay bienvenue — dans une passe ouverte.
    ///
    /// `cadre` = gabarit du cadre de scène (`cadre_overlay_welcome`, rect
    /// nul) : la carte prend son style à SA taille, comme
    /// `appliquerCadreOverlay` régénère le SVG sur la carte mesurée.
    /// `canvas` sert au mode spotlight (85 % de la largeur, centrage) et au
    /// voile plein écran.
    pub fn dessiner_welcome(
        &mut self,
        etat: &EtatWelcome,
        cadre: Option<&CadreCfg>,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let Some(rendu) = etat.rendu() else {
            return Ok(());
        };
        self.synchroniser_bloc_welcome(etat.version(), &rendu, cadre, canvas)?;

        // Les handles COM du bloc sont clonés : garder `self.bloc_welcome`
        // emprunté empêcherait les appels `&mut self` (`dessiner_cadre`).
        let (carte, metriques, pseudo, message, cta, bio, plateforme, cadre_geo) = {
            let Some(bloc) = &self.bloc_welcome else {
                return Ok(());
            };
            (
                bloc.carte,
                bloc.metriques,
                bloc.pseudo.as_ref().map(|l| (l.layout.clone(), l.h)),
                bloc.message.as_ref().map(|l| (l.layout.clone(), l.h)),
                bloc.cta.as_ref().map(|l| (l.layout.clone(), l.h)),
                bloc.bio.as_ref().map(|l| (l.layout.clone(), l.h)),
                bloc.plateforme
                    .as_ref()
                    .map(|l| (l.layout.clone(), l.h, l.w)),
                bloc.cadre.clone(),
            )
        };
        let m = metriques;

        // Voile noir plein écran (spotlight) — sous la carte, jamais clippé.
        if rendu.voile > 0.001 {
            let alpha = (255.0 * rendu.voile).round().clamp(0.0, 255.0) as u8;
            let brosse =
                unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d([0, 0, 0, alpha]), None) }
                    .map_err(|e| ErreurTexte::Systeme(format!("brosse voile : {}", e.message())))?;
            let plein = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: canvas.0 as f32,
                bottom: canvas.1 as f32,
            };
            unsafe { self.cible_rt.FillRectangle(&plein, &brosse) };
        }

        // Rect de la carte : statique en squelette, re-mesuré par frame en
        // spotlight (l'intro masque des lignes → la hauteur change).
        let (x, y, w, h, entete_w, entete_y, droite_y, droite_w) = if rendu.spotlight {
            let w = (SPOT_LARGEUR * canvas.0 as f32).max(1.0);
            let utile = m.utile();
            let entete_w = if rendu.intro {
                utile
            } else {
                (utile * rendu.entete).max(1.0)
            };
            let droite_w = if rendu.intro {
                0.0
            } else {
                (utile - entete_w - m.gap_row).max(1.0)
            };
            // Intro : textes et colonne droite masqués (avatar seul, absent
            // faute de média → carte réduite à son padding, comme le DOM).
            let h_entete = if rendu.intro {
                0.0
            } else {
                hauteur_colonne(
                    &[
                        pseudo.as_ref().map(|p| p.1),
                        message.as_ref().map(|p| p.1),
                        cta.as_ref().map(|p| p.1),
                        bio.as_ref().map(|p| p.1),
                    ],
                    m.gap_entete,
                )
            };
            // `::after` du titre : barre 2 px + `margin-top: 0.6cqw`.
            let h_droite = if rendu.intro {
                0.0
            } else {
                plateforme
                    .as_ref()
                    .map(|(_, lh, _)| lh + 0.6 * CQW * w + 2.0)
                    .unwrap_or(0.0)
            };
            let interieur = h_entete.max(h_droite);
            let h = 2.0 * m.pad + interieur;
            let x = (canvas.0 as f32 - w) / 2.0;
            let y = (canvas.1 as f32 - h) / 2.0;
            (
                x,
                y,
                w,
                h,
                entete_w,
                y + m.pad + (interieur - h_entete) / 2.0,
                y + m.pad + (interieur - h_droite) / 2.0,
                droite_w,
            )
        } else {
            let (x, y, w, h) = carte;
            (x, y, w, h, m.utile(), y + m.pad, 0.0, 0.0)
        };

        // Entrée spotlight : `translateY(26px) scale(0.94)` autour du centre.
        let zoome = rendu.spotlight && (rendu.entree < 0.999 || rendu.opacite < 0.999);
        if zoome {
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            let s = ENTREE_SCALE + (1.0 - ENTREE_SCALE) * rendu.entree;
            let dy = ENTREE_Y * (1.0 - rendu.entree);
            let mtx = Matrix3x2 {
                M11: s,
                M12: 0.0,
                M21: 0.0,
                M22: s,
                M31: cx * (1.0 - s),
                M32: cy * (1.0 - s) + dy,
            };
            unsafe { self.cible_rt.SetTransform(&mtx) };
        }

        // Couche : opacité de l'entrée + clip éventuel au polygone du cadre
        // (`clip-path` de la carte — tout le contenu, bordure comprise).
        let masque: Option<ID2D1Geometry> = match &cadre_geo {
            Some((_, geo)) => Some(geo.cast().map_err(|e| {
                ErreurTexte::Gpu(format!("ID2D1Geometry welcome : {}", e.message()))
            })?),
            None => None,
        };
        let marge = cadre_geo
            .as_ref()
            .map(|(c, _)| 3.0 * crate::cadres::epaisseur(c.stroke_width) as f32 + 16.0)
            .unwrap_or(0.0);
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: x - marge,
                top: y - marge,
                right: x + w + marge,
                bottom: y + h + marge,
            },
            geometricMask: ManuallyDrop::new(masque),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: rendu.opacite,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer welcome : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let resultat = (|| -> Result<(), ErreurTexte> {
            let rt = &self.cible_rt;
            let boite = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            let brosse_fond = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(m.fond), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse welcome : {}", e.message())))?;
            if cadre_geo.is_some() {
                // `clip-path` polygonal : le fond est rogné (le radius
                // disparaît dans la forme, comme le CSS).
                unsafe { rt.FillRectangle(&boite, &brosse_fond) };
            } else {
                unsafe {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: boite,
                            radiusX: m.rayon,
                            radiusY: m.rayon,
                        },
                        &brosse_fond,
                    )
                };
            }
            // `border: 1px solid rgba(255,255,255,0.14)` du panneau spotlight.
            if m.bord > 0.0 {
                let b = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(BORD_SPOT), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse bord welcome : {}", e.message()))
                    })?;
                unsafe {
                    rt.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: x + m.bord / 2.0,
                                top: y + m.bord / 2.0,
                                right: x + w - m.bord / 2.0,
                                bottom: y + h - m.bord / 2.0,
                            },
                            radiusX: (m.rayon - m.bord / 2.0).max(0.0),
                            radiusY: (m.rayon - m.bord / 2.0).max(0.0),
                        },
                        &b,
                        m.bord,
                        None,
                    )
                };
            }

            // Colonne de l'en-tête : avatar (absent), pseudo, message (spot)
            // CTA, bio — masquée pendant l'intro.
            if !rendu.intro {
                // Les layouts sont mis en page à la largeur du split ; pendant
                // la transition `flex-basis`, ils sont recentrés dans la boîte
                // courante (`entete_w`).
                let w0 = if rendu.spotlight {
                    (m.utile() * ENTETE_SPLIT).max(1.0)
                } else {
                    entete_w
                };
                let gauche = x + m.pad + (entete_w - w0) / 2.0;
                let w_bio0 = (0.92 * w0).max(1.0);
                let gauche_bio = x + m.pad + (entete_w - w_bio0) / 2.0;
                let mut cy = entete_y;
                let mut premier = true;
                if let Some((layout, lh)) = &pseudo {
                    self.dessiner_ligne_welcome(layout, gauche, cy, TEXTE)?;
                    cy += *lh;
                    premier = false;
                }
                if rendu.spotlight {
                    if let Some((layout, lh)) = &message {
                        if !premier {
                            cy += m.gap_entete;
                        }
                        self.dessiner_ligne_welcome(
                            layout,
                            gauche,
                            cy,
                            [TEXTE[0], TEXTE[1], TEXTE[2], ALPHA_MESSAGE],
                        )?;
                        cy += *lh;
                        premier = false;
                    }
                }
                if let Some((layout, lh)) = &cta {
                    if !premier {
                        cy += m.gap_entete;
                    }
                    self.dessiner_ligne_welcome(
                        layout,
                        gauche,
                        cy,
                        [TEXTE[0], TEXTE[1], TEXTE[2], ALPHA_CTA],
                    )?;
                    cy += *lh;
                    premier = false;
                }
                if let Some((layout, _)) = &bio {
                    // `max-width: 92%` : boîte plus étroite, centrée.
                    if !premier {
                        cy += m.gap_entete;
                    }
                    self.dessiner_ligne_welcome(
                        layout,
                        gauche_bio,
                        cy,
                        [TEXTE[0], TEXTE[1], TEXTE[2], ALPHA_BIO],
                    )?;
                }

                // Titre « Streamer Twitch » (colonne droite, spotlight) :
                // dégradé 3 stops sur la boîte du texte + barre `::after`.
                if let Some((layout, lh, lw)) = &plateforme {
                    let w_droite0 = (m.utile() - w0 - m.gap_row).max(1.0);
                    let gauche_droite = x + m.pad + entete_w + m.gap_row;
                    // Origine du layout (boîte `w_droite0`, texte centré) et
                    // début visuel du texte (dégradé + barre).
                    let origine = gauche_droite + (droite_w - w_droite0) / 2.0;
                    let texte_w = (*lw).min(w_droite0).max(1.0);
                    // `text-indent: 0.38em` compense le `letter-spacing`.
                    let decalage = 0.19 * m.taille_plateforme;
                    let bx = origine + (w_droite0 - texte_w) / 2.0 + decalage;
                    let brosse = self.brosse_plateforme(bx, droite_y, texte_w)?;
                    unsafe {
                        rt.DrawTextLayout(
                            D2D_POINT_2F {
                                x: origine + decalage,
                                y: droite_y,
                            },
                            layout,
                            &brosse,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        )
                    };
                    // `::after` : 2 px, `margin-top: 0.6cqw`, transparent →
                    // #9146ff → transparent.
                    let y_barre = droite_y + lh + 0.6 * CQW * w;
                    let stops = [
                        D2D1_GRADIENT_STOP {
                            position: 0.0,
                            color: couleur_d2d([0x91, 0x46, 0xff, 0]),
                        },
                        D2D1_GRADIENT_STOP {
                            position: 0.5,
                            color: couleur_d2d([0x91, 0x46, 0xff, 255]),
                        },
                        D2D1_GRADIENT_STOP {
                            position: 1.0,
                            color: couleur_d2d([0x91, 0x46, 0xff, 0]),
                        },
                    ];
                    let collection: ID2D1GradientStopCollection = unsafe {
                        rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                    }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("dégradé barre : {}", e.message()))
                    })?;
                    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: D2D_POINT_2F { x: bx, y: y_barre },
                        endPoint: D2D_POINT_2F {
                            x: bx + texte_w,
                            y: y_barre,
                        },
                    };
                    let brosse_barre: ID2D1LinearGradientBrush = unsafe {
                        rt.CreateLinearGradientBrush(&props, None, &collection)
                    }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse barre : {}", e.message()))
                    })?;
                    let rect_barre = D2D_RECT_F {
                        left: bx,
                        top: y_barre,
                        right: bx + texte_w,
                        bottom: y_barre + 2.0,
                    };
                    unsafe { rt.FillRectangle(&rect_barre, &brosse_barre) };
                }
            }

            // Trait du cadre au-dessus (`.cadre-overlay`, z 5 dans la carte).
            if let Some((cfg, geo)) = &cadre_geo {
                self.dessiner_cadre(cfg, geo, temps_s)?;
            }
            Ok(())
        })();
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        if zoome {
            unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        }
        resultat
    }

    /// Dessine une ligne du bloc (pinceau uni, couleur avec alpha).
    fn dessiner_ligne_welcome(
        &self,
        layout: &IDWriteTextLayout,
        x: f32,
        y: f32,
        couleur: [u8; 4],
    ) -> Result<(), ErreurTexte> {
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse texte welcome : {}", e.message())))?;
        unsafe {
            self.cible_rt.DrawTextLayout(
                D2D_POINT_2F { x, y },
                layout,
                &brosse,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            )
        };
        Ok(())
    }

    /// Brosse dégradée horizontale du titre de plateforme (3 stops CSS).
    fn brosse_plateforme(&self, x: f32, y: f32, largeur: f32) -> Result<ID2D1Brush, ErreurTexte> {
        let stops: Vec<D2D1_GRADIENT_STOP> = PLATEFORME_STOPS
            .iter()
            .map(|(position, couleur)| D2D1_GRADIENT_STOP {
                position: *position,
                color: couleur_d2d(*couleur),
            })
            .collect();
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé plateforme : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: D2D_POINT_2F { x, y },
            endPoint: D2D_POINT_2F {
                x: x + largeur,
                y,
            },
        };
        let brosse: ID2D1LinearGradientBrush =
            unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse plateforme : {}", e.message()))
                })?;
        Ok(brosse.into())
    }

    /// (Re)construit le bloc quand la clé a changé — la carte est re-mesurée
    /// à chaque play / repositionnement, comme le DOM
    /// (`requestAnimationFrame` de `playWelcomeClip`).
    fn synchroniser_bloc_welcome(
        &mut self,
        version: u64,
        rendu: &RenduWelcome,
        cadre: Option<&CadreCfg>,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let cle = cle_welcome(version, rendu.spotlight, rendu.position, canvas, cadre);
        if self.bloc_welcome.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        let spotlight = rendu.spotlight;
        let w = if spotlight {
            (SPOT_LARGEUR * canvas.0 as f32).max(1.0)
        } else {
            (rendu.position.2 as f32).max(1.0)
        };
        let m = Metriques::new(w, spotlight);
        let utile = m.utile();
        // Largeur de mise en page de l'en-tête : celle du split (la boîte
        // réelle est recentrée pendant la transition `flex-basis`).
        let w_entete = if spotlight {
            (utile * ENTETE_SPLIT).max(1.0)
        } else {
            utile
        };
        let w_droite = if spotlight {
            (utile - w_entete - m.gap_row).max(1.0)
        } else {
            0.0
        };
        // `max-width: 92%` de `.welcome-clip-bio`.
        let w_bio = (0.92 * w_entete).max(1.0);

        let style = |taille: f32, gras: bool, italique: bool| StyleTexte {
            // `system-ui` du body → famille système (police vide = repli
            // `Segoe UI` de `format`), comme la carte alerte.
            police: String::new(),
            taille,
            gras,
            italique,
            contour_epaisseur: 0.0,
            ..StyleTexte::default()
        };

        let pseudo = if rendu.display_name.trim().is_empty() {
            None
        } else {
            Some(self.ligne_welcome(
                &style(m.taille_pseudo, true, false),
                rendu.display_name,
                w_entete,
                false,
                1,
            )?)
        };
        let message = if spotlight && !rendu.message.trim().is_empty() {
            Some(self.ligne_welcome(
                &style(m.taille_message, false, true),
                rendu.message,
                w_entete,
                true,
                0,
            )?)
        } else {
            None
        };
        let cta = Some(self.ligne_welcome(
            &style(m.taille_cta, false, true),
            CTA,
            w_entete,
            false,
            1,
        )?);
        let bio = match rendu.bio.filter(|b| !b.trim().is_empty()) {
            Some(b) => Some(self.ligne_welcome(
                &style(m.taille_bio, false, false),
                b,
                w_bio,
                true,
                m.lignes_bio,
            )?),
            None => None,
        };
        let plateforme = if spotlight {
            let mut st = style(m.taille_plateforme, true, false);
            // `letter-spacing: 0.38em`.
            st.espacement = 0.38 * m.taille_plateforme;
            Some(self.ligne_welcome(&st, &PLATEFORME.to_uppercase(), w_droite, false, 1)?)
        } else {
            None
        };

        // Carte du squelette : largeur du squelette, hauteur auto (contenu +
        // padding). En spotlight, le rect est calculé à la frame.
        let carte = if spotlight {
            (0.0, 0.0, w, 0.0)
        } else {
            let h = 2.0 * m.pad
                + hauteur_colonne(
                    &[
                        pseudo.as_ref().map(|p| p.h),
                        cta.as_ref().map(|p| p.h),
                        bio.as_ref().map(|p| p.h),
                    ],
                    m.gap_entete,
                );
            (
                rendu.position.0 as f32,
                rendu.position.1 as f32,
                w,
                h.max(1.0),
            )
        };

        // Cadre concret : le gabarit appliqué au rect de la carte mesurée
        // (`genererCadreSVG` à la taille réelle) — squelette uniquement.
        let cadre_geo = match cadre.filter(|_| !spotlight) {
            Some(gabarit) => {
                let mut cfg = gabarit.clone();
                cfg.rect = (
                    carte.0 as f64,
                    carte.1 as f64,
                    carte.2 as f64,
                    carte.3 as f64,
                );
                cfg.z = Z_WELCOME;
                match self.geometrie_cadre(&cfg) {
                    Ok(geo) => Some((cfg, geo)),
                    Err(e) => {
                        log::warn!("welcome : polygone du cadre ignoré ({e})");
                        None
                    }
                }
            }
            None => None,
        };

        self.bloc_welcome = Some(BlocWelcome {
            cle,
            carte,
            metriques: m,
            pseudo,
            message,
            cta,
            bio,
            plateforme,
            cadre: cadre_geo,
        });
        Ok(())
    }

    /// Met en page une ligne de la carte — centrée, tronquée « … »
    /// (`text-overflow: ellipsis`) et bornée à `lignes_max` interlignes
    /// (`-webkit-line-clamp`).
    fn ligne_welcome(
        &self,
        style: &StyleTexte,
        texte: &str,
        largeur: f32,
        wrap: bool,
        lignes_max: u32,
    ) -> Result<LigneW, ErreurTexte> {
        let format = self.format(style)?;
        unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER) }.map_err(|e| {
            ErreurTexte::Systeme(format!("alignement welcome : {}", e.message()))
        })?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, largeur.max(1.0), 4096.0)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("layout welcome : {}", e.message())))?;
        unsafe {
            layout.SetWordWrapping(if wrap {
                DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            })
        }
        .map_err(|e| ErreurTexte::Systeme(format!("wrap welcome : {}", e.message())))?;
        // Troncature caractère + signe « … » (ellipsis / line-clamp).
        let signe = unsafe { self.dwrite.CreateEllipsisTrimmingSign(&format) }.map_err(|e| {
            ErreurTexte::Systeme(format!("EllipsisTrimmingSign : {}", e.message()))
        })?;
        unsafe {
            layout.SetTrimming(
                &DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                },
                &signe,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("SetTrimming welcome : {}", e.message())))?;
        if style.espacement != 0.0 {
            let layout1: IDWriteTextLayout1 = layout.cast().map_err(|e| {
                ErreurTexte::Systeme(format!("IDWriteTextLayout1 welcome : {}", e.message()))
            })?;
            unsafe {
                layout1.SetCharacterSpacing(
                    0.0,
                    style.espacement,
                    0.0,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: contenu.len() as u32,
                    },
                )
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("espacement welcome : {}", e.message()))
            })?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques welcome : {}", e.message())))?;
        let mut h = metriques.height;
        if lignes_max > 0 && metriques.lineCount > lignes_max {
            // `-webkit-line-clamp` : borner la hauteur à N interlignes — le
            // signe de troncature s'applique alors à la dernière ligne.
            let interligne = metriques.height / metriques.lineCount as f32;
            unsafe { layout.SetMaxHeight(interligne * lignes_max as f32) }.map_err(|e| {
                ErreurTexte::Systeme(format!("line-clamp welcome : {}", e.message()))
            })?;
            let mut borne = DWRITE_TEXT_METRICS::default();
            unsafe { layout.GetMetrics(&mut borne) }.map_err(|e| {
                ErreurTexte::Systeme(format!("métriques clamp welcome : {}", e.message()))
            })?;
            h = borne.height;
        }
        Ok(LigneW {
            layout,
            _format: format,
            h,
            w: metriques.widthIncludingTrailingWhitespace,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn play(spotlight: bool) -> MajWelcome {
        MajWelcome::Play {
            display_name: "Manu".to_string(),
            bio: Some("Streamer retro".to_string()),
            message: "Bienvenue !".to_string(),
            spotlight,
        }
    }

    #[test]
    fn position_par_defaut_suit_le_squelette_dom() {
        let etat = EtatWelcome::nouveau();
        assert_eq!(etat.position, POSITION_DEFAUT);
        assert!(!etat.spotlight);
        assert!(etat.rendu().is_none());
    }

    #[test]
    fn play_puis_stop_puis_play_incremente_la_version() {
        let mut etat = EtatWelcome::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(play(false), t0);
        assert_eq!(etat.version(), 1);
        assert!(etat.rendu_at(t0).is_some());
        etat.appliquer_at(MajWelcome::Stop, t0);
        assert_eq!(etat.version(), 2);
        assert!(etat.rendu_at(t0).is_none());
        // Stop sans clip en cours : aucune version (idempotent).
        etat.appliquer_at(MajWelcome::Stop, t0);
        assert_eq!(etat.version(), 2);
    }

    #[test]
    fn config_sans_mode_conserve_le_mode_et_dedup() {
        let mut etat = EtatWelcome::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(
            MajWelcome::Config {
                x: 1480.0,
                y: 580.0,
                largeur: 320.0,
                hauteur: 352.0,
                spotlight: None,
            },
            t0,
        );
        // Position identique au défaut → aucune version.
        assert_eq!(etat.version(), 0);
        etat.appliquer_at(play(true), t0);
        assert!(etat.spotlight);
        // `position-overlay-config` : position seule, mode conservé.
        etat.appliquer_at(
            MajWelcome::Config {
                x: 100.0,
                y: 200.0,
                largeur: 400.0,
                hauteur: 300.0,
                spotlight: None,
            },
            t0,
        );
        assert_eq!(etat.position.0, 100.0);
        assert_eq!(etat.position.2, 400.0);
        assert!(etat.spotlight);
        // `welcome-clip-config` : le mode est porté par la frame.
        etat.appliquer_at(
            MajWelcome::Config {
                x: 100.0,
                y: 200.0,
                largeur: 400.0,
                hauteur: 300.0,
                spotlight: Some(false),
            },
            t0,
        );
        assert!(!etat.spotlight);
    }

    #[test]
    fn phases_spotlight_voile_entree_intro_split() {
        let mut etat = EtatWelcome::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(play(true), t0);

        let debut = etat.rendu_at(t0).expect("clip en cours");
        assert_eq!(debut.voile, 0.0);
        assert_eq!(debut.opacite, 0.0);
        assert_eq!(debut.entree, 0.0);
        assert!(debut.intro);
        assert_eq!(debut.entete, 1.0);

        // Voile stabilisé à 350 ms, entrée en cours (120 ms de délai).
        let voile = etat.rendu_at(t0 + Duration::from_millis(400)).expect("clip");
        assert!((voile.voile - 1.0).abs() < 1e-3);
        assert!(voile.entree > 0.0 && voile.entree < 1.0);

        // Fin d'intro (3 s) : split en cours.
        let split = etat.rendu_at(t0 + Duration::from_millis(3300)).expect("clip");
        assert!(!split.intro);
        assert!(split.entete > ENTETE_SPLIT && split.entete < 1.0);
        assert!((split.opacite - 1.0).abs() < 1e-3);

        // Transition finie (3,6 s) : en-tête à 42 %.
        let stable = etat.rendu_at(t0 + Duration::from_millis(3700)).expect("clip");
        assert!((stable.entete - ENTETE_SPLIT).abs() < 1e-3);
        assert_eq!(stable.entree, 1.0);
    }

    #[test]
    fn squelette_sans_animation() {
        let mut etat = EtatWelcome::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(play(false), t0);
        let r = etat.rendu_at(t0).expect("clip");
        assert_eq!(r.voile, 0.0);
        assert_eq!(r.opacite, 1.0);
        assert_eq!(r.entree, 1.0);
        assert!(!r.intro);
    }

    #[test]
    fn metriques_squelette_320_mirroir_des_cqw() {
        // Carte par défaut 320 px : padding 1,5 % = 4,8 px ; gap 0,5 % = 1,6.
        let m = Metriques::new(320.0, false);
        assert!((m.pad - 4.8).abs() < 1e-4);
        assert!((m.gap_entete - 1.6).abs() < 1e-4);
        assert!((m.utile() - (320.0 - 9.6)).abs() < 1e-4);
        // `clamp(0.65rem, 3.5cqw, 1.1rem)` : 3,5 % = 11,2 px (dans les bornes).
        assert!((m.taille_pseudo - 11.2).abs() < 1e-4);
        // `clamp(0.5rem, 2.2cqw, 0.72rem)` : 2,2 % = 7,04 → borné à 8 px.
        assert!((m.taille_cta - 8.0).abs() < 1e-4);
        // `clamp(0.55rem, 2.5cqw, 0.82rem)` : 2,5 % = 8 px → borné à 8,8 px.
        assert!((m.taille_bio - 8.8).abs() < 1e-4);
        assert_eq!(m.lignes_bio, 2);
        assert_eq!(m.rayon, 12.0);
        assert_eq!(m.bord, 0.0);
    }

    #[test]
    fn metriques_spotlight_1080p_mirroir_des_cqw() {
        // Carte 85 % de 1920 = 1632 px : padding 3 % = 48,96 px.
        let m = Metriques::new(0.85 * 1920.0, true);
        assert!((m.pad - 48.96).abs() < 1e-3);
        assert!((m.gap_row - 40.8).abs() < 1e-3);
        // `clamp(1rem, 3cqw, 2.2rem)` : 48,96 → borné à 35,2 px.
        assert!((m.taille_pseudo - 35.2).abs() < 1e-4);
        // `clamp(0.7rem, 1.6cqw, 1rem)` : 26,1 → borné à 16 px.
        assert!((m.taille_cta - 16.0).abs() < 1e-4);
        // `clamp(0.8rem, 2.2cqw, 1.4rem)` : 35,9 → borné à 22,4 px.
        assert!((m.taille_message - 22.4).abs() < 1e-4);
        // `clamp(0.9rem, 2.6cqw, 1.7rem)` : 42,4 → borné à 27,2 px.
        assert!((m.taille_plateforme - 27.2).abs() < 1e-4);
        assert_eq!(m.lignes_bio, 4);
        assert_eq!(m.rayon, 18.0);
        assert_eq!(m.bord, 1.0);
    }

    #[test]
    fn hauteur_colonne_compte_les_gaps_entre_lignes_visibles() {
        // Les lignes absentes ne comptent ni hauteur ni gap (colonne flex).
        assert!((hauteur_colonne(&[Some(10.0), Some(20.0)], 4.0) - 34.0).abs() < 1e-4);
        assert!((hauteur_colonne(&[Some(10.0), None, Some(20.0)], 4.0) - 34.0).abs() < 1e-4);
        assert!((hauteur_colonne(&[None, Some(10.0)], 4.0) - 10.0).abs() < 1e-4);
        assert_eq!(hauteur_colonne(&[None, None], 4.0), 0.0);
    }

    #[test]
    fn bezier_css_borne_et_monotone() {
        assert_eq!(bezier_css(EASE, 0.0), 0.0);
        assert!((bezier_css(EASE, 1.0) - 1.0).abs() < 1e-6);
        assert!((bezier_css(EASE_SORTIE, 1.0) - 1.0).abs() < 1e-6);
        let mut precedent = -1.0;
        for i in 0..=20 {
            let v = bezier_css(EASE_SORTIE, i as f32 / 20.0);
            assert!(v >= precedent, "bezier non monotone à {i}");
            precedent = v;
        }
    }
}
