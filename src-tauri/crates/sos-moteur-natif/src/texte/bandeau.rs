//! Overlay « bandeau premier message » natif — barre pleine largeur z 9998.
//!
//! Miroir de `playBandeauPM` / `stopBandeauPM` / `applyBandeauPMConfig` /
//! `bpmResoudreCouleurs` (diffusion.html 11189-11420) : barre pleine largeur
//! en bas (défaut) ou en haut, fond en dégradé horizontal noir → teinte du
//! cadre → noir, filet 2 px aux couleurs du cadre, ligne lumineuse 2 px,
//! pastille « ✦ PREMIER MESSAGE », avatar rond 44 px, pseudo (15 px, couleur
//! primaire) et message (20 px, blanc, une ligne tronquée).
//!
//! Animation : `opacity 0 → 1` (0,45 s `ease`) + glissière
//! `translateY(±100 %) → 0` (0,5 s `cubic-bezier(.16,1.08,.38,.98)`).
//! `bandeau-premier-message-stop` rejoue la transition en sens inverse puis
//! masque (le DOM attend 550 ms).
//!
//! Canal : frames `chat_tx` `bandeau-premier-message-play` / `-stop` /
//! `-config`. ⚠️ `bandeau:etat` (event Tauri) est l'état **dashboard** — ce
//! n'est pas le canal du rendu.
//!
//! Limites assumées :
//! - **mascotte Streamy non portée** : c'est un asset de marque du serveur
//!   (`/brand/streamy-welcome.png`), hors du crate moteur ; la réserve de
//!   300 px à droite est conservée pour que le texte se coupe au même endroit ;
//! - `box-shadow` (halo) et `text-shadow` non portés ;
//! - le dégradé `border-image` 135° est rendu en **teinte médiane** (2 px).

use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1Brush, ID2D1EllipseGeometry, ID2D1Geometry, ID2D1GradientStopCollection,
    ID2D1Layer, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

/// z du bandeau premier message (`.bandeau-pm-root`, z-index 9998).
pub const Z_BANDEAU: i32 = 9998;

/// `.bandeau-pm-contenu { padding: 16px 300px 16px 40px; gap: 18px }`.
const PAD_HAUT: f32 = 16.0;
const PAD_GAUCHE: f32 = 40.0;
/// Réserve de la mascotte Streamy (non portée) — le texte se coupe au même
/// endroit que dans le DOM.
const PAD_DROITE: f32 = 300.0;
const GAP: f32 = 18.0;
/// `.bandeau-pm-avatar { width/height: 44px; border: 2px }`.
const AVATAR: f32 = 44.0;
const AVATAR_BORD: f32 = 2.0;
/// `.bandeau-pm-badge { padding: 5px 14px; gap: 8px; border: 1px }`.
const BADGE_PAD_X: f32 = 14.0;
const BADGE_PAD_Y: f32 = 5.0;
const BADGE_GAP: f32 = 8.0;
const BADGE_BORD: f32 = 1.0;
/// Tailles de police fixes du DOM (px, indépendantes du widget).
const TAILLE_PSEUDO: f32 = 15.0;
const TAILLE_MESSAGE: f32 = 20.0;
const TAILLE_BADGE: f32 = 11.0;
const TAILLE_ICONE: f32 = 16.0;
/// `letter-spacing: 0.14em` du libellé de pastille.
const ESPACEMENT_BADGE: f32 = 0.14 * TAILLE_BADGE;
/// `.bandeau-pm-texte { gap: 2px }`.
const GAP_TEXTE: f32 = 2.0;

/// `transition: opacity 0.45s ease` + `transform 0.5s cubic-bezier(...)`.
const ENTRY_S: f32 = 0.45;
const GLISSE_S: f32 = 0.5;
/// Le DOM masque 550 ms après le stop (transition finie).
const SORTIE_S: f32 = 0.55;
const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);
const GLISSE_EASE: (f32, f32, f32, f32) = (0.16, 1.08, 0.38, 0.98);

/// Pastille — constantes du DOM (jamais i18n).
const ICONE: &str = "✦";
const BADGE_TEXTE: &str = "Premier message";
/// `BPM_COULEURS_DEFAUT` (aucun cadre actif).
pub const COULEURS_DEFAUT: ([u8; 4], [u8; 4]) = ([0xe0, 0xe0, 0xe0, 255], [0xa0, 0xa0, 0xa0, 255]);

// =============================================================================
// FRAMES — mutations décodées par le pont (`src/moteur/mod.rs`).
// =============================================================================

/// Bord de la barre (`pos-bas` par défaut).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionBandeau {
    Bas,
    Haut,
}

/// Mutation du bandeau — frames `chat_tx`.
#[derive(Debug, Clone, PartialEq)]
pub enum MajBandeau {
    /// `bandeau-premier-message-play`.
    Play {
        display_name: String,
        /// URL de l'avatar (le moteur lit le fichier du cache disque).
        avatar: Option<String>,
        message: String,
    },
    /// `bandeau-premier-message-stop` : glissière + fondu de sortie.
    Stop,
    /// `bandeau-premier-message-config` : position / police.
    Config {
        position: Option<PositionBandeau>,
        /// Famille CSS (`--bpm-fonte`) — vide = police système.
        police: Option<String>,
    },
}

// =============================================================================
// ÉTAT LIVE
// =============================================================================

#[derive(Debug)]
struct BandeauEnCours {
    display_name: String,
    avatar: Option<String>,
    message: String,
    debut: Instant,
    /// `bandeau-premier-message-stop` reçu — base de la sortie.
    arret: Option<Instant>,
}

/// Ce que la composition peint ce tick-ci (phases déjà résolues).
pub struct RenduBandeau<'a> {
    pub display_name: &'a str,
    pub message: &'a str,
    pub avatar: Option<&'a str>,
    pub position: PositionBandeau,
    /// Famille de police configurée (vide = système).
    pub police: &'a str,
    /// Opacité de la barre (entrée / sortie).
    pub opacite: f32,
    /// Décalage vertical en fraction de la hauteur de barre (1 = hors écran).
    pub glisse: f32,
}

/// État vivant du bandeau : position + police + message en cours + version.
pub struct EtatBandeau {
    position: PositionBandeau,
    police: String,
    en_cours: Option<BandeauEnCours>,
    version: u64,
}

impl Default for EtatBandeau {
    fn default() -> Self {
        Self {
            position: PositionBandeau::Bas,
            police: String::new(),
            en_cours: None,
            version: 0,
        }
    }
}

impl EtatBandeau {
    pub fn nouveau() -> Self {
        Self::default()
    }

    pub fn appliquer(&mut self, maj: MajBandeau) {
        self.appliquer_at(maj, Instant::now());
    }

    fn appliquer_at(&mut self, maj: MajBandeau, maintenant: Instant) {
        match maj {
            MajBandeau::Play {
                display_name,
                avatar,
                message,
            } => {
                self.en_cours = Some(BandeauEnCours {
                    display_name,
                    avatar,
                    message,
                    debut: maintenant,
                    arret: None,
                });
                self.version = self.version.wrapping_add(1);
            }
            MajBandeau::Stop => {
                if let Some(clip) = self.en_cours.as_mut() {
                    if clip.arret.is_none() {
                        clip.arret = Some(maintenant);
                        self.version = self.version.wrapping_add(1);
                    }
                }
            }
            MajBandeau::Config { position, police } => {
                let mut change = false;
                if let Some(p) = position {
                    if p != self.position {
                        self.position = p;
                        change = true;
                    }
                }
                if let Some(p) = police {
                    let p = p.trim().to_string();
                    if p != self.police {
                        self.police = p;
                        change = true;
                    }
                }
                if change {
                    self.version = self.version.wrapping_add(1);
                }
            }
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Phases au temps courant — `None` quand la barre est sortie.
    pub fn rendu(&self) -> Option<RenduBandeau<'_>> {
        self.rendu_at(Instant::now())
    }

    fn rendu_at(&self, maintenant: Instant) -> Option<RenduBandeau<'_>> {
        let clip = self.en_cours.as_ref()?;
        let (opacite, glisse) = match clip.arret {
            None => {
                let t = maintenant.saturating_duration_since(clip.debut).as_secs_f32();
                let opacite = bezier_css(EASE, (t / ENTRY_S).min(1.0));
                let glisse = 1.0 - bezier_css(GLISSE_EASE, (t / GLISSE_S).min(1.0));
                (opacite, glisse)
            }
            Some(arret) => {
                let t = maintenant.saturating_duration_since(arret).as_secs_f32();
                if t >= SORTIE_S {
                    return None;
                }
                let opacite = 1.0 - bezier_css(EASE, (t / ENTRY_S).min(1.0));
                let glisse = bezier_css(GLISSE_EASE, (t / GLISSE_S).min(1.0));
                (opacite, glisse)
            }
        };
        Some(RenduBandeau {
            display_name: &clip.display_name,
            message: &clip.message,
            avatar: clip.avatar.as_deref(),
            position: self.position,
            police: &self.police,
            opacite,
            glisse,
        })
    }
}

/// `cubic-bezier(x1,y1,x2,y2)` CSS à `x` ∈ [0,1] (dichotomie).
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

/// Assombrit une couleur d'un ratio (0 = inchangé, 1 = noir) — miroir
/// `bpmAssombrir`.
pub fn assombrir(c: [u8; 4], ratio: f32) -> [u8; 4] {
    let r = ratio.clamp(0.0, 1.0);
    let f = |v: u8| (v as f32 * (1.0 - r)).round().clamp(0.0, 255.0) as u8;
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

/// Luminance perceptuelle (rec.601) — miroir `bpmLuminance`.
pub fn luminance(c: [u8; 4]) -> f32 {
    c[0] as f32 * 0.299 + c[1] as f32 * 0.587 + c[2] as f32 * 0.114
}

/// Couleurs `{ primaire, secondaire }` — miroir `bpmResoudreCouleurs` :
/// le cadre actif donne la teinte visible (l'extrémité la plus lumineuse),
/// l'autre devient la secondaire ; `couleur` seule → secondaire assombrie
/// de 25 % ; aucun cadre → noir & blanc.
pub fn resoudre_couleurs(cadre: Option<([u8; 4], [u8; 4])>) -> ([u8; 4], [u8; 4]) {
    let Some((c1, c2)) = cadre else {
        return COULEURS_DEFAUT;
    };
    if luminance(c1) < luminance(c2) {
        (c2, c1)
    } else {
        (c1, c2)
    }
}

/// Teinte médiane d'un dégradé (filet 2 px).
fn teinte_mediane(a: [u8; 4], b: [u8; 4]) -> [u8; 4] {
    let moy = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
    [
        moy(a[0], b[0]),
        moy(a[1], b[1]),
        moy(a[2], b[2]),
        moy(a[3], b[3]),
    ]
}

// =============================================================================
// BLOC DE RENDU
// =============================================================================

struct LigneB {
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    h: f32,
    w: f32,
}

pub(super) struct BlocBandeau {
    cle: u64,
    icone: Option<LigneB>,
    badge: Option<LigneB>,
    pseudo: Option<LigneB>,
    message: Option<LigneB>,
    /// Hauteur de la barre (contenu + padding).
    hauteur: f32,
    /// Largeur de la pastille (icône + gap + texte + padding + bord).
    badge_largeur: f32,
    /// Avatar décodé (le fichier peut manquer : barre sans avatar).
    avatar: Option<(ID3D11Texture2D, ID2D1Bitmap1)>,
    avatar_geo: Option<ID2D1EllipseGeometry>,
}

fn cle_bandeau(
    version: u64,
    police: &str,
    couleurs: ([u8; 4], [u8; 4]),
    canvas: (u32, u32),
    avatar: bool,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    police.hash(&mut h);
    couleurs.hash(&mut h);
    canvas.hash(&mut h);
    avatar.hash(&mut h);
    h.finish()
}

impl DessinateurTexte {
    /// Dessine le bandeau premier message — dans une passe ouverte.
    ///
    /// `couleurs` = `(primaire, secondaire)` résolues depuis le cadre de la
    /// scène (`resoudre_couleurs`) ; `canvas` = dimensions du canvas.
    pub fn dessiner_bandeau(
        &mut self,
        etat: &EtatBandeau,
        couleurs: ([u8; 4], [u8; 4]),
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let Some(rendu) = etat.rendu() else {
            return Ok(());
        };
        self.synchroniser_bloc_bandeau(etat.version(), &rendu, couleurs, canvas)?;
        let (bloc_lignes, hauteur, badge_largeur, avatar, avatar_geo) = {
            let Some(bloc) = &self.bloc_bandeau else {
                return Ok(());
            };
            (
                (
                    bloc.icone.as_ref().map(|l| (l.layout.clone(), l.h, l.w)),
                    bloc.badge.as_ref().map(|l| (l.layout.clone(), l.h, l.w)),
                    bloc.pseudo.as_ref().map(|l| (l.layout.clone(), l.h, l.w)),
                    bloc.message.as_ref().map(|l| (l.layout.clone(), l.h, l.w)),
                ),
                bloc.hauteur,
                bloc.badge_largeur,
                bloc.avatar.clone(),
                bloc.avatar_geo.clone(),
            )
        };
        let (icone, badge, pseudo, message) = bloc_lignes;
        let (prim, sec) = couleurs;
        let w = canvas.0 as f32;
        let h = hauteur;
        // Position : la barre est collée au bord, décalée par la glissière.
        let base_y = match rendu.position {
            PositionBandeau::Bas => canvas.1 as f32 - h,
            PositionBandeau::Haut => 0.0,
        };
        let dy = match rendu.position {
            PositionBandeau::Bas => rendu.glisse * h,
            PositionBandeau::Haut => -rendu.glisse * h,
        };
        let y = base_y + dy;

        // Couche : opacité de l'entrée/sortie.
        let params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: y,
                right: w,
                bottom: y + h,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: rendu.opacite,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer bandeau : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let resultat = (|| -> Result<(), ErreurTexte> {
            // Fond : noir → teinte assombrie → noir (90°, opaque).
            let fond = assombrir(prim, 0.55);
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: couleur_d2d([10, 10, 10, 247]),
                },
                D2D1_GRADIENT_STOP {
                    position: 0.5,
                    color: couleur_d2d([fond[0], fond[1], fond[2], 255]),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: couleur_d2d([10, 10, 10, 247]),
                },
            ];
            let brosse_fond = self.brosse_stops(
                &stops,
                D2D_POINT_2F { x: 0.0, y },
                D2D_POINT_2F { x: w, y },
            )?;
            let barre = D2D_RECT_F {
                left: 0.0,
                top: y,
                right: w,
                bottom: y + h,
            };
            unsafe { self.cible_rt.FillRectangle(&barre, &brosse_fond) };

            // Filet 2 px du bord intérieur (teinte médiane du dégradé cadre).
            let filet = teinte_mediane(prim, sec);
            let brosse_filet =
                unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(filet), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse filet bandeau : {}", e.message()))
                    })?;
            let (fy, fbas) = match rendu.position {
                PositionBandeau::Bas => (y, y + 2.0),
                PositionBandeau::Haut => (y + h - 2.0, y + h),
            };
            unsafe {
                self.cible_rt.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top: fy,
                        right: w,
                        bottom: fbas,
                    },
                    &brosse_filet,
                )
            };

            // Ligne lumineuse 2 px : transparent → prim 85 % → sec 95 % →
            // prim 85 % → transparent (`.bandeau-pm-ligne`).
            let ligne_y = fy;
            let avec = |c: [u8; 4], a: u8| [c[0], c[1], c[2], a];
            let stops_ligne = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: couleur_d2d([0, 0, 0, 0]),
                },
                D2D1_GRADIENT_STOP {
                    position: 0.25,
                    color: couleur_d2d(avec(prim, 217)),
                },
                D2D1_GRADIENT_STOP {
                    position: 0.5,
                    color: couleur_d2d(avec(sec, 242)),
                },
                D2D1_GRADIENT_STOP {
                    position: 0.75,
                    color: couleur_d2d(avec(prim, 217)),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: couleur_d2d([0, 0, 0, 0]),
                },
            ];
            let brosse_ligne = self.brosse_stops(
                &stops_ligne,
                D2D_POINT_2F { x: 0.0, y: ligne_y },
                D2D_POINT_2F { x: w, y: ligne_y },
            )?;
            unsafe {
                self.cible_rt.FillRectangle(
                    &D2D_RECT_F {
                        left: 0.0,
                        top: ligne_y,
                        right: w,
                        bottom: ligne_y + 2.0,
                    },
                    &brosse_ligne,
                )
            };

            // Contenu : pastille, avatar, pseudo + message.
            let content_h = h - 2.0 * PAD_HAUT;
            let milieu = y + PAD_HAUT + content_h / 2.0;
            let mut cx = PAD_GAUCHE;

            // Pastille « ✦ PREMIER MESSAGE ».
            let badge_h = icone
                .as_ref()
                .map(|i| i.1)
                .unwrap_or(TAILLE_ICONE)
                .max(badge.as_ref().map(|b| b.1).unwrap_or(TAILLE_BADGE))
                + 2.0 * BADGE_PAD_Y
                + 2.0 * BADGE_BORD;
            let badge_rect = D2D_RECT_F {
                left: cx,
                top: milieu - badge_h / 2.0,
                right: cx + badge_largeur,
                bottom: milieu + badge_h / 2.0,
            };
            let fond_badge = avec(prim, 31); // rgba(prim, 0.12)
            let b_fond_badge =
                unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(fond_badge), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse badge bandeau : {}", e.message()))
                    })?;
            unsafe {
                self.cible_rt.FillRectangle(&badge_rect, &b_fond_badge);
                self.cible_rt.DrawRectangle(&badge_rect, &brosse_filet, BADGE_BORD, None);
            }
            if let Some((layout, lh, _)) = &icone {
                let b = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(prim), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse icône bandeau : {}", e.message()))
                    })?;
                unsafe {
                    self.cible_rt.DrawTextLayout(
                        D2D_POINT_2F {
                            x: cx + BADGE_PAD_X,
                            y: milieu - lh / 2.0,
                        },
                        layout,
                        &b,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
            }
            if let Some((layout, lh, _)) = &badge {
                let b = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(prim), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse badge bandeau : {}", e.message()))
                    })?;
                unsafe {
                    self.cible_rt.DrawTextLayout(
                        D2D_POINT_2F {
                            x: cx + BADGE_PAD_X
                                + icone.as_ref().map(|i| i.2).unwrap_or(0.0)
                                + BADGE_GAP,
                            y: milieu - lh / 2.0,
                        },
                        layout,
                        &b,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
            }
            cx += badge_largeur + GAP;

            // Avatar rond (2 px de bordure à 60 % de la primaire).
            if let (Some((_, bitmap)), Some(geo)) = (avatar.as_ref(), avatar_geo.as_ref()) {
                let ax = cx;
                let ay = milieu - AVATAR / 2.0;
                let masque: ID2D1Geometry = geo.cast().map_err(|e| {
                    ErreurTexte::Gpu(format!("ellipse avatar bandeau : {}", e.message()))
                })?;
                let mut p_avatar = D2D1_LAYER_PARAMETERS1 {
                    contentBounds: D2D_RECT_F {
                        left: ax,
                        top: ay,
                        right: ax + AVATAR,
                        bottom: ay + AVATAR,
                    },
                    geometricMask: ManuallyDrop::new(Some(masque)),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2 {
                        M31: ax + AVATAR / 2.0,
                        M32: ay + AVATAR / 2.0,
                        ..Matrix3x2::identity()
                    },
                    opacity: 1.0,
                    opacityBrush: ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                };
                let l: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }.map_err(|e| {
                    ErreurTexte::Gpu(format!("layer avatar bandeau : {}", e.message()))
                })?;
                unsafe { self.ctx.PushLayer(&p_avatar, &l) };
                unsafe {
                    self.cible_rt.DrawBitmap(
                        bitmap,
                        Some(&D2D_RECT_F {
                            left: ax,
                            top: ay,
                            right: ax + AVATAR,
                            bottom: ay + AVATAR,
                        }),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    )
                };
                unsafe { self.ctx.PopLayer() };
                unsafe { ManuallyDrop::drop(&mut p_avatar.geometricMask) };
                let anneau = unsafe {
                    self.cible_rt
                        .CreateSolidColorBrush(&couleur_d2d(avec(prim, 153)), None)
                }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse anneau bandeau : {}", e.message()))
                })?;
                unsafe {
                    self.cible_rt.DrawEllipse(
                        &D2D1_ELLIPSE {
                            point: D2D_POINT_2F {
                                x: ax + AVATAR / 2.0,
                                y: ay + AVATAR / 2.0,
                            },
                            radiusX: AVATAR / 2.0,
                            radiusY: AVATAR / 2.0,
                        },
                        &anneau,
                        AVATAR_BORD,
                        None,
                    )
                };
                cx += AVATAR + GAP;
            }

            // Pseudo + message (colonne, largeur restante).
            let h_texte = pseudo.as_ref().map(|p| p.1).unwrap_or(0.0)
                + message.as_ref().map(|m| m.1).unwrap_or(0.0)
                + if pseudo.is_some() && message.is_some() {
                    GAP_TEXTE
                } else {
                    0.0
                };
            let mut ty = milieu - h_texte / 2.0;
            if let Some((layout, lh, _)) = &pseudo {
                let b = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(prim), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse pseudo bandeau : {}", e.message()))
                    })?;
                unsafe {
                    self.cible_rt.DrawTextLayout(
                        D2D_POINT_2F { x: cx, y: ty },
                        layout,
                        &b,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
                ty += lh + GAP_TEXTE;
            }
            if let Some((layout, _, _)) = &message {
                let b = unsafe {
                    self.cible_rt
                        .CreateSolidColorBrush(&couleur_d2d([255, 255, 255, 255]), None)
                }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse message bandeau : {}", e.message()))
                })?;
                unsafe {
                    self.cible_rt.DrawTextLayout(
                        D2D_POINT_2F { x: cx, y: ty },
                        layout,
                        &b,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
            }
            Ok(())
        })();
        unsafe { self.ctx.PopLayer() };
        resultat
    }

    /// Brosse dégradée linéaire (stops déjà triés).
    fn brosse_stops(
        &self,
        stops: &[D2D1_GRADIENT_STOP],
        debut: D2D_POINT_2F,
        fin: D2D_POINT_2F,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé bandeau : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: debut,
            endPoint: fin,
        };
        let brosse = unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("brosse bandeau : {}", e.message()))
            })?;
        Ok(brosse.into())
    }

    fn synchroniser_bloc_bandeau(
        &mut self,
        version: u64,
        rendu: &RenduBandeau,
        couleurs: ([u8; 4], [u8; 4]),
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let avatar_present = rendu.avatar.is_some();
        let cle = cle_bandeau(version, rendu.police, couleurs, canvas, avatar_present);
        if self.bloc_bandeau.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        let w = canvas.0 as f32;
        let style = |taille: f32, gras: bool, police: &str| StyleTexte {
            police: police.to_string(),
            taille,
            gras,
            contour_epaisseur: 0.0,
            ..StyleTexte::default()
        };
        let icone = Some(self.ligne_bandeau(
            &style(TAILLE_ICONE, false, rendu.police),
            ICONE,
            TAILLE_ICONE * 2.0,
            false,
        )?);
        // `letter-spacing: 0.14em` + `text-transform: uppercase` du libellé.
        let mut style_badge = style(TAILLE_BADGE, true, rendu.police);
        style_badge.espacement = ESPACEMENT_BADGE;
        let badge = Some(self.ligne_bandeau(
            &style_badge,
            &BADGE_TEXTE.to_uppercase(),
            w,
            false,
        )?);
        let pseudo = if rendu.display_name.trim().is_empty() {
            None
        } else {
            Some(self.ligne_bandeau(
                &style(TAILLE_PSEUDO, true, rendu.police),
                rendu.display_name,
                (w - PAD_GAUCHE - PAD_DROITE).max(1.0),
                false,
            )?)
        };
        let message = if rendu.message.trim().is_empty() {
            None
        } else {
            Some(self.ligne_bandeau(
                &style(TAILLE_MESSAGE, false, rendu.police),
                rendu.message,
                (w - PAD_GAUCHE - PAD_DROITE).max(1.0),
                false,
            )?)
        };
        // Largeur de pastille : icône + gap + texte + padding + bordure.
        let badge_largeur = icone.as_ref().map(|i| i.w).unwrap_or(0.0)
            + BADGE_GAP
            + badge.as_ref().map(|b| b.w).unwrap_or(0.0)
            + 2.0 * BADGE_PAD_X
            + 2.0 * BADGE_BORD;
        // Hauteur de barre : le plus haut des contenus + padding vertical.
        let texte_h = pseudo.as_ref().map(|p| p.h).unwrap_or(0.0)
            + message.as_ref().map(|m| m.h).unwrap_or(0.0)
            + if pseudo.is_some() && message.is_some() {
                GAP_TEXTE
            } else {
                0.0
            };
        let badge_h = icone
            .as_ref()
            .map(|i| i.h)
            .unwrap_or(TAILLE_ICONE)
            .max(badge.as_ref().map(|b| b.h).unwrap_or(TAILLE_BADGE))
            + 2.0 * BADGE_PAD_Y
            + 2.0 * BADGE_BORD;
        let hauteur = texte_h.max(AVATAR).max(badge_h) + 2.0 * PAD_HAUT;

        // Avatar : fichier du cache disque (l'app le télécharge à la volée).
        let avatar = rendu.avatar.and_then(|url| self.image_cache(url));
        let (avatar, avatar_geo) = match avatar {
            Some((texture, bitmap, _)) => {
                let geo = unsafe {
                    self.d2d.CreateEllipseGeometry(&D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: 0.0, y: 0.0 },
                        radiusX: AVATAR / 2.0,
                        radiusY: AVATAR / 2.0,
                    })
                }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("ellipse bandeau : {}", e.message()))
                })?;
                (Some((texture, bitmap)), Some(geo))
            }
            None => (None, None),
        };

        self.bloc_bandeau = Some(BlocBandeau {
            cle,
            icone,
            badge,
            pseudo,
            message,
            hauteur,
            badge_largeur,
            avatar,
            avatar_geo,
        });
        Ok(())
    }

    /// Met en page une ligne du bandeau — une ligne, troncature « … ».
    fn ligne_bandeau(
        &self,
        style: &StyleTexte,
        texte: &str,
        largeur: f32,
        wrap: bool,
    ) -> Result<LigneB, ErreurTexte> {
        let format = self.format(style)?;
        unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING) }.map_err(|e| {
            ErreurTexte::Systeme(format!("alignement bandeau : {}", e.message()))
        })?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, largeur.max(1.0), 4096.0)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("layout bandeau : {}", e.message())))?;
        unsafe {
            layout.SetWordWrapping(if wrap {
                windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            })
        }
        .map_err(|e| ErreurTexte::Systeme(format!("wrap bandeau : {}", e.message())))?;
        if style.espacement != 0.0 {
            let layout1: windows::Win32::Graphics::DirectWrite::IDWriteTextLayout1 = layout
                .cast()
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("IDWriteTextLayout1 bandeau : {}", e.message()))
                })?;
            unsafe {
                layout1.SetCharacterSpacing(
                    0.0,
                    style.espacement,
                    0.0,
                    windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: contenu.len() as u32,
                    },
                )
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("espacement bandeau : {}", e.message()))
            })?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques bandeau : {}", e.message())))?;
        Ok(LigneB {
            layout,
            _format: format,
            h: metriques.height,
            w: metriques.widthIncludingTrailingWhitespace,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn play() -> MajBandeau {
        MajBandeau::Play {
            display_name: "Manu".to_string(),
            avatar: Some("https://static-cdn.jtvnw.net/a.png".to_string()),
            message: "Salut !".to_string(),
        }
    }

    #[test]
    fn etat_initial_et_play_puis_stop() {
        let mut etat = EtatBandeau::nouveau();
        assert_eq!(etat.position, PositionBandeau::Bas);
        assert!(etat.police.is_empty());
        assert!(etat.rendu().is_none());

        let t0 = Instant::now();
        etat.appliquer_at(play(), t0);
        assert_eq!(etat.version(), 1);
        let debut = etat.rendu_at(t0).expect("bandeau en cours");
        assert_eq!(debut.opacite, 0.0);
        assert_eq!(debut.glisse, 1.0, "hors écran au départ");

        // Stop : la sortie dure 550 ms puis plus rien.
        etat.appliquer_at(MajBandeau::Stop, t0 + Duration::from_millis(1000));
        assert_eq!(etat.version(), 2);
        let sortie = etat
            .rendu_at(t0 + Duration::from_millis(1200))
            .expect("sortie en cours");
        assert!(sortie.opacite < 1.0 && sortie.glisse > 0.0);
        assert!(
            etat.rendu_at(t0 + Duration::from_millis(1600)).is_none(),
            "masqué après 550 ms"
        );
        // Un second stop ne change rien (idempotent).
        etat.appliquer_at(MajBandeau::Stop, t0 + Duration::from_millis(1700));
        assert_eq!(etat.version(), 2);
    }

    #[test]
    fn phases_entree_glissiere_et_fondu() {
        let mut etat = EtatBandeau::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(play(), t0);
        // 0,5 s : glissière finie (0,5 s) et fondu fini (0,45 s).
        let stable = etat.rendu_at(t0 + Duration::from_millis(520)).expect("rendu");
        assert!((stable.opacite - 1.0).abs() < 1e-3);
        assert!(stable.glisse.abs() < 1e-3);
    }

    #[test]
    fn config_position_et_police() {
        let mut etat = EtatBandeau::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(
            MajBandeau::Config {
                position: Some(PositionBandeau::Haut),
                police: Some("Anton".to_string()),
            },
            t0,
        );
        assert_eq!(etat.position, PositionBandeau::Haut);
        assert_eq!(etat.police, "Anton");
        assert_eq!(etat.version(), 1);
        // Identique → aucune version.
        etat.appliquer_at(
            MajBandeau::Config {
                position: Some(PositionBandeau::Haut),
                police: Some("Anton".to_string()),
            },
            t0,
        );
        assert_eq!(etat.version(), 1);
        // Police vide = système (retour au défaut).
        etat.appliquer_at(
            MajBandeau::Config {
                position: None,
                police: Some(String::new()),
            },
            t0,
        );
        assert!(etat.police.is_empty());
        assert_eq!(etat.position, PositionBandeau::Haut, "position conservée");
    }

    #[test]
    fn couleurs_miroir_de_bpm_resoudre_couleurs() {
        // Aucun cadre → noir & blanc du DOM.
        assert_eq!(resoudre_couleurs(None), COULEURS_DEFAUT);
        // Cadre noir → couleur : la teinte visible est l'extrémité lumineuse.
        let noir = [0, 0, 0, 255];
        let violet = [0x91, 0x46, 0xff, 255];
        assert_eq!(resoudre_couleurs(Some((noir, violet))), (violet, noir));
        assert_eq!(resoudre_couleurs(Some((violet, noir))), (violet, noir));
        // Assombrissement 25 % (miroir `bpmAssombrir`).
        assert_eq!(assombrir([200, 100, 0, 255], 0.25), [150, 75, 0, 255]);
        assert_eq!(assombrir([200, 100, 0, 255], 0.0), [200, 100, 0, 255]);
        assert_eq!(assombrir([200, 100, 0, 255], 1.0), [0, 0, 0, 255]);
        // Luminance rec.601.
        assert!((luminance([255, 255, 255, 255]) - 255.0).abs() < 1e-3);
        assert_eq!(luminance([0, 0, 0, 255]), 0.0);
    }

    #[test]
    fn bezier_css_borne_et_monotone() {
        assert_eq!(bezier_css(EASE, 0.0), 0.0);
        assert!((bezier_css(GLISSE_EASE, 1.0) - 1.0).abs() < 1e-6);
        let mut precedent = -1.0;
        for i in 0..=20 {
            let v = bezier_css(GLISSE_EASE, i as f32 / 20.0);
            assert!(v >= precedent, "bezier non monotone à {i}");
            precedent = v;
        }
    }
}
