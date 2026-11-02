//! Overlay « pad numérique » natif — carte média transitoire, z 9995.
//!
//! Miroir de `initPadOverlay` / `applyPadOverlayConfig` / `playPad` /
//! `stopPad` (`diffusion.html` 12203-12411) : une touche du pad déclenche
//! l'affichage d'une **image ou d'une vidéo** (ou seulement un son), avec
//! pop-in `scale(0.6) → 1` (0,3 s `cubic-bezier(.16,1.08,.38,.98)`),
//! fondu d'opacité (0,3 s `ease`), effets `bounce`/`zoom` optionnels, mode
//! **plein écran** (média `contain` plein canvas, sans cadre ni scale,
//! opacité de superposition) et **cadre SVG de la scène** sur la carte.
//!
//! Canal : frames `chat_tx` **`pad-play`** (payload `pad`) et **`pad-stop`**,
//! plus la frame **partagée** `position-overlay-config` (squelette de
//! position unifié : welcome + alertes + commandes + pad). ⚠️ Les events
//! Tauri `pad:touche` / `pad:touche-stop` sont l'état **dashboard** (audio
//! local + retour UI) — jamais le canal du rendu.
//!
//! Son (miroir de `playPad`) : vidéo → son porté par la vidéo ; image →
//! fichier `son` séparé ; touche `audio` → le média EST le son, aucun visuel.
//! Volume = `volume` de la touche (0..1).
//!
//! Limites assumées : `max-height: 80vh` mesuré sur la hauteur du **canvas**
//! (le viewport du DOM) ; `box-shadow`/`text-shadow` non portés (aucun texte
//! ici) ; le `radius: 8px` du média est omis quand un cadre de scène est actif
//! (le `clip-path` du polygone remplace le rayon, comme dans le DOM).

use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    ID2D1Geometry, ID2D1Layer, ID2D1PathGeometry, ID2D1RoundedRectangleGeometry,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_ROUNDED_RECT,
};

use crate::cadres::CadreCfg;
use crate::media::{EffetsCfg, Fit, LectureCfg, MediaCfg, Transform2D};

use super::{DessinateurTexte, ErreurTexte, TraceMedia};

/// z du pad numérique (`.pad-root`, z-index 9995) — sous les commandes 9996,
/// les alertes 9997, le bandeau 9998 et le welcome 9999.
pub const Z_PAD: i32 = 9995;

/// `transition: opacity 0.3s ease, transform 0.3s cubic-bezier(…)`.
const FONDU_S: f32 = 0.3;
const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);
const POP_EASE: (f32, f32, f32, f32) = (0.16, 1.08, 0.38, 0.98);
/// `transform: scale(0.6)` au repos → `scale(1)` visible.
const POP_DEPART: f32 = 0.6;
/// Le DOM masque (`display: none`) 500 ms après le stop.
const SORTIE_S: f32 = 0.5;
/// `.pad-media { border-radius: 8px }`.
const RAYON_MEDIA: f32 = 8.0;
/// `.pad-media { max-height: 80vh }` — le viewport du DOM est le canvas.
const HAUTEUR_MAX_RATIO: f32 = 0.8;
/// `pad-bounce` : `translateY(0 → -20px)` sur 0,6 s `ease-in-out` infini.
const BOUNCE_PX: f32 = 20.0;
const BOUNCE_S: f32 = 0.6;
/// `pad-zoom` : `scale(1 → 1.15)` sur 1,2 s `ease-in-out` infini.
const ZOOM_MAX: f32 = 1.15;
const ZOOM_S: f32 = 1.2;

/// Squelette par défaut du pad (`padOverlay` du DOM, avant la première frame
/// `position-overlay-config`).
pub const POSITION_DEFAUT: (f64, f64, f64, f64) = (660.0, 60.0, 600.0, 120.0);

// =============================================================================
// FRAMES — mutations décodées par le pont (`src/moteur/mod.rs`).
// =============================================================================

/// Contenu d'une frame `pad-play` (payload `pad`).
#[derive(Debug, Clone, PartialEq)]
pub struct PadPlay {
    /// `"image"` | `"video"` | `"audio"`.
    pub media_type: String,
    /// Chemin relatif à la base (`medias/…`) — l'image/vidéo du pad.
    pub media: Option<String>,
    /// `"image"` | `"video"` (le type réel du média).
    pub media_kind: Option<String>,
    /// Fichier audio séparé (mode image).
    pub son: Option<String>,
    /// Volume 0..1 (média **et** son séparé).
    pub volume: f32,
    /// Durée d'affichage (le stop vient de l'app).
    pub duree_ms: u64,
    pub bounce: bool,
    pub zoom: bool,
    pub plein_ecran: bool,
    /// Plein écran : opacité de superposition (laisse voir la scène).
    pub transparence: bool,
    pub transparence_opacite: f32,
}

impl Default for PadPlay {
    fn default() -> Self {
        Self {
            media_type: String::new(),
            media: None,
            media_kind: None,
            son: None,
            volume: 1.0,
            duree_ms: 5000,
            bounce: false,
            zoom: false,
            plein_ecran: false,
            transparence: false,
            transparence_opacite: 0.5,
        }
    }
}

/// Mutation du pad — frames `chat_tx`.
#[derive(Debug, Clone, PartialEq)]
pub enum MajPad {
    /// `pad-play` : nouvelle carte (remplace la précédente).
    Play(PadPlay),
    /// `pad-stop` : fondu de sortie (0,3 s) puis masquage (500 ms).
    Stop,
    /// `position-overlay-config` : squelette partagé (x/y/largeur/hauteur).
    Config {
        x: f64,
        y: f64,
        largeur: f64,
        hauteur: f64,
    },
}

// =============================================================================
// ÉTAT LIVE
// =============================================================================

#[derive(Debug)]
struct PadEnCours {
    play: PadPlay,
    debut: Instant,
    /// `pad-stop` reçu — base de la sortie.
    arret: Option<Instant>,
}

/// État vivant du pad : squelette de position + carte en cours + version.
pub struct EtatPad {
    position: (f64, f64, f64, f64),
    en_cours: Option<PadEnCours>,
    version: u64,
}

impl Default for EtatPad {
    fn default() -> Self {
        Self {
            position: POSITION_DEFAUT,
            en_cours: None,
            version: 0,
        }
    }
}

/// Ce que la composition peint ce tick-ci (phases déjà résolues).
pub struct RenduPad<'a> {
    pub play: &'a PadPlay,
    /// Opacité de la racine (entrée / sortie).
    pub opacite: f32,
    /// Échelle de la racine (`scale(0.6 → 1)`) — 1,0 en plein écran.
    pub echelle: f32,
    /// Squelette de position (x, y, largeur, hauteur).
    pub position: (f64, f64, f64, f64),
    /// La carte est **sortie** (`pad-stop` reçu) : le son est coupé.
    pub arrete: bool,
}

impl EtatPad {
    pub fn nouveau() -> Self {
        Self::default()
    }

    pub fn appliquer(&mut self, maj: MajPad) {
        self.appliquer_at(maj, Instant::now());
    }

    fn appliquer_at(&mut self, maj: MajPad, maintenant: Instant) {
        match maj {
            MajPad::Play(play) => {
                self.en_cours = Some(PadEnCours {
                    play,
                    debut: maintenant,
                    arret: None,
                });
                self.version = self.version.wrapping_add(1);
            }
            MajPad::Stop => {
                if let Some(pad) = self.en_cours.as_mut() {
                    if pad.arret.is_none() {
                        pad.arret = Some(maintenant);
                        self.version = self.version.wrapping_add(1);
                    }
                }
            }
            MajPad::Config {
                x,
                y,
                largeur,
                hauteur,
            } => {
                let nouvelle = (x, y, largeur, hauteur);
                if nouvelle != self.position {
                    self.position = nouvelle;
                    self.version = self.version.wrapping_add(1);
                }
            }
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn position(&self) -> (f64, f64, f64, f64) {
        self.position
    }

    /// Gain du son du pad en millièmes : `volume` de la touche, **0 dès le
    /// `pad-stop`** (le DOM met l'audio en pause) et 0 pour une touche sans son.
    pub fn gain_audio_milliemes(&self) -> u32 {
        let Some(pad) = self.en_cours.as_ref() else {
            return 0;
        };
        if pad.arret.is_some() {
            return 0;
        }
        (pad.play.volume.clamp(0.0, 1.0) * 1000.0).round() as u32
    }

    /// Phases au temps courant — `None` quand la carte est entièrement sortie.
    pub fn rendu(&self) -> Option<RenduPad<'_>> {
        self.rendu_at(Instant::now())
    }

    fn rendu_at(&self, maintenant: Instant) -> Option<RenduPad<'_>> {
        let pad = self.en_cours.as_ref()?;
        let plein = pad.play.plein_ecran;
        let (opacite, echelle, arrete) = match pad.arret {
            None => {
                let t = maintenant.saturating_duration_since(pad.debut).as_secs_f32();
                let k = (t / FONDU_S).min(1.0);
                let echelle = if plein {
                    1.0
                } else {
                    POP_DEPART + (1.0 - POP_DEPART) * bezier_css(POP_EASE, k)
                };
                (bezier_css(EASE, k), echelle, false)
            }
            Some(arret) => {
                let t = maintenant.saturating_duration_since(arret).as_secs_f32();
                if t >= SORTIE_S {
                    return None;
                }
                let k = (t / FONDU_S).min(1.0);
                let echelle = if plein {
                    1.0
                } else {
                    1.0 - (1.0 - POP_DEPART) * bezier_css(POP_EASE, k)
                };
                (1.0 - bezier_css(EASE, k), echelle, true)
            }
        };
        Some(RenduPad {
            play: &pad.play,
            opacite,
            echelle,
            position: self.position,
            arrete,
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

/// `ease-in-out` CSS (`pad-bounce` / `pad-zoom`).
fn ease_in_out(t: f32) -> f32 {
    bezier_css((0.42, 0.0, 0.58, 1.0), t)
}

/// Fichier audio réellement joué par une touche (miroir de `playPad`) :
/// vidéo → son porté par la vidéo (aucun fichier séparé) ; image → `son` ;
/// touche `audio` → le média lui-même.
pub fn source_audio(play: &PadPlay) -> Option<&str> {
    fn non_vide(s: &Option<String>) -> Option<&str> {
        s.as_deref().filter(|v| !v.trim().is_empty())
    }
    if play.media_type == "audio" {
        return non_vide(&play.media);
    }
    if play.media_kind.as_deref() == Some("video") {
        return None;
    }
    non_vide(&play.son)
}

/// Média affichable (image/vidéo) — `None` pour une touche audio pure.
pub fn media_affiche(play: &PadPlay) -> Option<&str> {
    if play.media_type == "audio" {
        return None;
    }
    play.media.as_deref().filter(|m| !m.trim().is_empty())
}

// =============================================================================
// BLOC DE RENDU
// =============================================================================

pub(super) struct BlocPad {
    cle: u64,
    /// Cadre concret posé sur la carte mesurée (jamais en plein écran).
    cadre: Option<(CadreCfg, ID2D1PathGeometry)>,
}

fn cle_pad(
    version: u64,
    position: (f64, f64, f64, f64),
    plein: bool,
    boite: (f32, f32, f32, f32),
    cadre: Option<&CadreCfg>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    for v in [position.0, position.1, position.2, position.3] {
        v.to_bits().hash(&mut h);
    }
    plein.hash(&mut h);
    for v in [boite.0, boite.1, boite.2, boite.3] {
        v.to_bits().hash(&mut h);
    }
    cadre.is_some().hash(&mut h);
    if let Some(c) = cadre {
        c.stroke_width.to_bits().hash(&mut h);
        c.gradient_angle.to_bits().hash(&mut h);
        c.couleur.hash(&mut h);
        c.couleur_fin.hash(&mut h);
        c.animer.hash(&mut h);
        (c.style as u8).hash(&mut h);
    }
    h.finish()
}

impl DessinateurTexte {
    /// Dessine le pad — dans une passe ouverte.
    ///
    /// `cadre` = gabarit du cadre de scène appliqué à la carte mesurée
    /// (`appliquerCadreOverlay`, jamais en plein écran) ; `temps_s` = horloge
    /// du flux (effets `bounce`/`zoom` + avance du média).
    pub fn dessiner_pad(
        &mut self,
        etat: &EtatPad,
        cadre: Option<&CadreCfg>,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let Some(rendu) = etat.rendu() else {
            // Sorti : média et son libérés (le DOM retire le `<video>`).
            self.pad_media = None;
            self.pad_media_rel = None;
            self.pad_media_kind = None;
            self.pad_audio = None;
            self.pad_audio_rel = None;
            self.bloc_pad = None;
            return Ok(());
        };
        self.synchroniser_media_pad(rendu.play, temps_s)?;
        self.synchroniser_audio_pad(rendu.play, temps_s);

        // Avance des flux (une frame d'avance, comme l'écran d'attente).
        let temps_us = (temps_s * 1e6) as i64;
        if let Some(media) = self.pad_media.as_mut() {
            if let Err(e) = media.avancer(temps_us) {
                log::debug!("pad : vidéo ({e})");
            }
            if let Err(e) = media.avancer_audio(temps_us) {
                log::debug!("pad : audio vidéo ({e})");
            }
        }
        if let Some(audio) = self.pad_audio.as_mut() {
            if let Err(e) = audio.avancer(temps_us) {
                log::debug!("pad : audio ({e})");
            }
        }

        let (w, h) = (canvas.0 as f32, canvas.1 as f32);

        // Couche : opacité du fondu (la carte + son cadre).
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
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer pad : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let resultat = self.peindre_pad(&rendu, etat.version(), cadre, canvas, temps_s);
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        resultat
    }

    /// Boîte de la carte (`plein écran` → tout le canvas) + le drapeau.
    ///
    /// Hors plein écran : largeur du squelette, hauteur déduite du média
    /// (`width: 100%`, hauteur naturelle) bornée à `max-height: 80vh`.
    fn boite_pad(
        &self,
        play: &PadPlay,
        position: (f64, f64, f64, f64),
        canvas: (u32, u32),
    ) -> (bool, (f32, f32, f32, f32)) {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        if play.plein_ecran {
            return (true, (0.0, 0.0, w, h));
        }
        let (x, y, largeur, _) = position;
        let largeur = (largeur as f32).max(1.0);
        let hauteur = match &self.pad_media {
            Some(media) => {
                let (mw, mh) = (media.taille.0.max(1) as f32, media.taille.1.max(1) as f32);
                (largeur * mh / mw).min(HAUTEUR_MAX_RATIO * h)
            }
            // Touche audio pure : rien n'est affiché (la carte n'existe pas).
            None => 0.0,
        };
        (false, (x as f32, y as f32, largeur, hauteur.max(1.0)))
    }

    /// Corps du rendu — dans la couche d'opacité.
    ///
    /// Ordre des transformations (calqué sur le CSS) : la racine porte le
    /// pop-in `scale(0.6 → 1)` autour du centre de la carte ; les effets
    /// `bounce`/`zoom` s'appliquent **au média**, à l'intérieur de cette
    /// échelle (donc aussi mis à l'échelle par elle). Le **masque** de la
    /// couche est posé en coordonnées canvas à la taille écran, transformation
    /// neutre : il ne subit jamais le pop ni le rebond.
    fn peindre_pad(
        &mut self,
        rendu: &RenduPad<'_>,
        version: u64,
        cadre: Option<&CadreCfg>,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let (plein, boite) = self.boite_pad(rendu.play, rendu.position, canvas);
        // Touche audio pure : aucun visuel (le son sort par le mixeur).
        let Some((texture, taille, alpha)) = self
            .pad_media
            .as_ref()
            .map(|m| (m.texture.clone(), m.taille, m.alpha))
        else {
            return Ok(());
        };
        let (x, y, bw, bh) = boite;
        let (cx, cy) = (x + bw / 2.0, y + bh / 2.0);

        // Pop-in : `scale(0.6 → 1)` autour du centre de la racine (jamais en
        // plein écran, `transform: none`).
        let echelle_root = if plein {
            1.0
        } else {
            rendu.echelle.max(0.01)
        };
        // Boîte **visible** (post-pop) : sert de masque et de cadre.
        let (vx, vy, vw, vh) = if plein {
            (x, y, bw, bh)
        } else {
            let (vw, vh) = (bw * echelle_root, bh * echelle_root);
            (cx - vw / 2.0, cy - vh / 2.0, vw, vh)
        };

        // Effets `bounce`/`zoom` (média seul, jamais en plein écran).
        let t = temps_s as f32;
        let (dy_media, echelle_media) = if plein {
            (0.0, 1.0)
        } else {
            let bounce = if rendu.play.bounce {
                // 0 % → 0, 50 % → −20 px, 100 % → 0 (`ease-in-out`).
                let p = (t / BOUNCE_S).rem_euclid(1.0);
                let k = if p < 0.5 {
                    ease_in_out(p * 2.0)
                } else {
                    1.0 - ease_in_out((p - 0.5) * 2.0)
                };
                -BOUNCE_PX * k
            } else {
                0.0
            };
            let zoom = if rendu.play.zoom {
                let p = (t / ZOOM_S).rem_euclid(1.0);
                let k = if p < 0.5 {
                    ease_in_out(p * 2.0)
                } else {
                    1.0 - ease_in_out((p - 0.5) * 2.0)
                };
                1.0 + (ZOOM_MAX - 1.0) * k
            } else {
                1.0
            };
            (bounce, zoom)
        };
        // Composition : racine ∘ média (même centre) → une seule matrice.
        let echelle = echelle_root * echelle_media;
        let mtx_media = Matrix3x2 {
            M11: echelle,
            M12: 0.0,
            M21: 0.0,
            M22: echelle,
            M31: cx * (1.0 - echelle),
            M32: cy * (1.0 - echelle) + echelle_root * dy_media,
        };

        // Cadre et masque à la boîte visible (le cadre est re-mesuré quand le
        // pop change de taille — clé `cle_pad`).
        self.synchroniser_bloc_pad(version, rendu.position, plein, (vx, vy, vw, vh), cadre)?;
        let cadre_geo = self
            .bloc_pad
            .as_ref()
            .and_then(|b| b.cadre.as_ref().map(|(c, g)| (c.clone(), g.clone())));

        // Masque : polygone du cadre, sinon `border-radius: 8px` du média.
        let arrondi: Option<ID2D1RoundedRectangleGeometry> = if cadre_geo.is_none() && !plein {
            let geo: ID2D1RoundedRectangleGeometry = unsafe {
                self.d2d.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: vx,
                        top: vy,
                        right: vx + vw,
                        bottom: vy + vh,
                    },
                    radiusX: RAYON_MEDIA * echelle_root,
                    radiusY: RAYON_MEDIA * echelle_root,
                })
            }
            .map_err(|e| ErreurTexte::Gpu(format!("rayon média pad : {}", e.message())))?;
            Some(geo)
        } else {
            None
        };
        let masque: Option<ID2D1Geometry> = match cadre_geo.as_ref() {
            Some((_, geo)) => Some(
                geo.cast()
                    .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry pad : {}", e.message())))?,
            ),
            None => match arrondi.as_ref() {
                Some(geo) => Some(geo.cast().map_err(|e| {
                    ErreurTexte::Gpu(format!("ID2D1Geometry rayon pad : {}", e.message()))
                })?),
                None => None,
            },
        };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: vx - BOUNCE_PX,
                top: vy - BOUNCE_PX,
                right: vx + vw + BOUNCE_PX,
                bottom: vy + vh + BOUNCE_PX,
            },
            geometricMask: ManuallyDrop::new(masque),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: if plein && rendu.play.transparence {
                rendu.play.transparence_opacite.clamp(0.0, 1.0)
            } else {
                1.0
            },
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer média pad : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };

        let resultat = (|| -> Result<(), ErreurTexte> {
            // Média `object-fit: contain` dans la boîte (non mise à l'échelle :
            // c'est la matrice qui l'applique).
            let (source, destination) = crate::media::fit_rects(
                taille,
                (x as f64, y as f64, bw as f64, bh as f64),
                Fit::Ajuster,
            );
            unsafe { self.cible_rt.SetTransform(&mtx_media) };
            let bitmap = self.bitmap_media(&texture, alpha)?;
            let trace = TraceMedia {
                destination,
                source,
                boite: windows::Win32::Foundation::RECT {
                    left: x as i32,
                    top: y as i32,
                    right: (x + bw) as i32,
                    bottom: (y + bh) as i32,
                },
                transforme: None,
                clip: None,
                effets: &EffetsCfg::NEUTRE,
                opacite: 1.0,
            };
            let dessin = self.dessiner_media(&bitmap, &trace);
            // Trait du cadre à la taille visible, hors pop/rebond, dans la
            // même couche (donc rogné au polygone comme le `clip-path` du DOM).
            unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
            dessin?;
            if let Some((cfg_cadre, geo)) = &cadre_geo {
                self.dessiner_cadre(cfg_cadre, geo, temps_s)?;
            }
            Ok(())
        })();
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        resultat
    }

    /// (Re)charge l'image/vidéo du pad quand `media|kind` change.
    fn synchroniser_media_pad(
        &mut self,
        play: &PadPlay,
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let rel = media_affiche(play);
        let meme = self.pad_media_rel.as_deref() == rel
            && self.pad_media_kind.as_deref() == play.media_kind.as_deref();
        if meme {
            return Ok(());
        }
        self.pad_media = None;
        self.pad_media_rel = rel.map(|s| s.to_string());
        self.pad_media_kind = play.media_kind.clone();
        let Some(rel) = rel else {
            return Ok(());
        };
        let chemin = self.base.join(rel);
        let media_cfg = MediaCfg {
            chemin: chemin.clone(),
            rect: (0.0, 0.0, 0.0, 0.0),
            z: Z_PAD,
            fit: Fit::Ajuster,
            transform: Transform2D::NEUTRE,
            effets: EffetsCfg::NEUTRE,
            cadre: None,
            morphs: Vec::new(),
            pixel_morph: 0.0,
            // Le pad joue **une fois** (pas de `loop` dans le DOM).
            lecture: LectureCfg {
                paused: false,
                boucle: None,
                boucle_infinie: false,
            },
            carrousel: None,
        };
        match crate::media::charger(&self.contexte, &media_cfg) {
            Ok(mut media) => {
                if let Err(e) = media.recaler((temps_s * 1e6) as i64) {
                    log::debug!("pad : recalage média ({e})");
                }
                self.pad_media = Some(media);
            }
            Err(e) => log::debug!("pad : média {} ignoré ({e})", chemin.display()),
        }
        Ok(())
    }

    /// (Re)charge le fichier audio séparé (image / touche audio).
    fn synchroniser_audio_pad(&mut self, play: &PadPlay, temps_s: f64) {
        let rel = source_audio(play);
        if self.pad_audio_rel.as_deref() == rel {
            return;
        }
        self.pad_audio = None;
        self.pad_audio_rel = rel.map(|s| s.to_string());
        let Some(rel) = rel else {
            return;
        };
        let chemin = self.base.join(rel);
        match crate::media::AudioFichier::ouvrir(&chemin, (temps_s * 1e6) as i64) {
            Ok(audio) => self.pad_audio = Some(audio),
            Err(e) => log::debug!("pad : audio {} ignoré ({e})", chemin.display()),
        }
    }

    /// Anneau audio du pad (média vidéo **ou** fichier séparé).
    pub(crate) fn anneau_audio_pad(
        &self,
    ) -> Option<std::sync::Arc<std::sync::Mutex<crate::audio::live::AnneauAudio>>> {
        if let Some(audio) = self.pad_audio.as_ref() {
            return Some(audio.anneau_audio());
        }
        self.pad_media.as_ref()?.anneau_audio()
    }

    /// (Re)construit le cadre de la carte quand la clé change.
    fn synchroniser_bloc_pad(
        &mut self,
        version: u64,
        position: (f64, f64, f64, f64),
        plein: bool,
        boite: (f32, f32, f32, f32),
        cadre: Option<&CadreCfg>,
    ) -> Result<(), ErreurTexte> {
        let cle = cle_pad(version, position, plein, boite, cadre);
        if self.bloc_pad.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        // Plein écran = pas de cadre (le DOM le retire explicitement).
        let cadre_geo = match cadre.filter(|_| !plein) {
            Some(gabarit) => {
                let mut cfg = gabarit.clone();
                cfg.rect = (
                    boite.0 as f64,
                    boite.1 as f64,
                    boite.2 as f64,
                    boite.3 as f64,
                );
                cfg.z = Z_PAD;
                match self.geometrie_cadre(&cfg) {
                    Ok(geo) => Some((cfg, geo)),
                    Err(e) => {
                        log::warn!("pad : polygone du cadre ignoré ({e})");
                        None
                    }
                }
            }
            None => None,
        };
        self.bloc_pad = Some(BlocPad { cle, cadre: cadre_geo });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn play(plein: bool) -> MajPad {
        MajPad::Play(PadPlay {
            media_type: "video".to_string(),
            media: Some("medias/a.mp4".to_string()),
            media_kind: Some("video".to_string()),
            volume: 0.8,
            duree_ms: 5000,
            bounce: true,
            plein_ecran: plein,
            ..PadPlay::default()
        })
    }

    #[test]
    fn etat_initial_et_play_puis_stop() {
        let mut etat = EtatPad::nouveau();
        assert_eq!(etat.position(), POSITION_DEFAUT);
        assert!(etat.rendu().is_none());

        let t0 = Instant::now();
        etat.appliquer_at(play(false), t0);
        assert_eq!(etat.version(), 1);
        let depart = etat.rendu_at(t0).expect("carte visible");
        assert!(depart.opacite.abs() < 1e-3);
        assert!((depart.echelle - POP_DEPART).abs() < 1e-3, "scale(0.6) au départ");
        assert!(!depart.arrete);
        let stable = etat
            .rendu_at(t0 + Duration::from_millis(400))
            .expect("stable");
        assert!((stable.opacite - 1.0).abs() < 1e-3);
        assert!((stable.echelle - 1.0).abs() < 1e-3);

        // Stop : fondu 0,3 s puis masquage à 500 ms.
        etat.appliquer_at(MajPad::Stop, t0 + Duration::from_millis(1000));
        assert_eq!(etat.version(), 2);
        let sortie = etat
            .rendu_at(t0 + Duration::from_millis(1150))
            .expect("sortie en cours");
        assert!(sortie.opacite < 1.0 && sortie.echelle > POP_DEPART);
        assert!(sortie.arrete, "le son est coupé dès le stop");
        assert!(
            etat.rendu_at(t0 + Duration::from_millis(1600)).is_none(),
            "masqué après 500 ms"
        );
        // Second stop : idempotent.
        etat.appliquer_at(MajPad::Stop, t0 + Duration::from_millis(1700));
        assert_eq!(etat.version(), 2);
    }

    #[test]
    fn plein_ecran_sans_scale() {
        let mut etat = EtatPad::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(play(true), t0);
        let depart = etat.rendu_at(t0).expect("visible");
        assert!((depart.echelle - 1.0).abs() < 1e-6, "transform: none");
        assert!(depart.play.plein_ecran);
    }

    #[test]
    fn config_position_partagee() {
        let mut etat = EtatPad::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(
            MajPad::Config {
                x: 100.0,
                y: 200.0,
                largeur: 500.0,
                hauteur: 120.0,
            },
            t0,
        );
        assert_eq!(etat.version(), 1);
        assert_eq!(etat.position(), (100.0, 200.0, 500.0, 120.0));
        // Identique → aucune version (la carte n'est pas re-mesurée).
        etat.appliquer_at(
            MajPad::Config {
                x: 100.0,
                y: 200.0,
                largeur: 500.0,
                hauteur: 120.0,
            },
            t0,
        );
        assert_eq!(etat.version(), 1);
    }

    #[test]
    fn source_audio_suit_le_mode_du_dom() {
        // Vidéo : son porté par la vidéo, jamais de fichier séparé.
        let mut p = PadPlay {
            media_type: "video".to_string(),
            media: Some("medias/a.mp4".to_string()),
            media_kind: Some("video".to_string()),
            son: Some("medias/son.mp3".to_string()),
            ..PadPlay::default()
        };
        assert_eq!(source_audio(&p), None);
        assert_eq!(media_affiche(&p), Some("medias/a.mp4"));

        // Image + son séparé.
        p.media_type = "image".to_string();
        p.media_kind = Some("image".to_string());
        assert_eq!(source_audio(&p), Some("medias/son.mp3"));
        assert_eq!(media_affiche(&p), Some("medias/a.mp4"));

        // Audio pur : le média EST le son, aucun visuel.
        let a = PadPlay {
            media_type: "audio".to_string(),
            media: Some("medias/bruit.mp3".to_string()),
            son: Some("medias/bruit.mp3".to_string()),
            ..PadPlay::default()
        };
        assert_eq!(source_audio(&a), Some("medias/bruit.mp3"));
        assert_eq!(media_affiche(&a), None);

        // Image sans son : rien à jouer.
        p.son = None;
        assert_eq!(source_audio(&p), None);
        // Chemin vide = pas de source (jamais `base.join("")`).
        p.son = Some(String::new());
        assert_eq!(source_audio(&p), None);
    }

    #[test]
    fn bezier_css_borne_et_monotone() {
        assert_eq!(bezier_css(EASE, 0.0), 0.0);
        assert!((bezier_css(POP_EASE, 1.0) - 1.0).abs() < 1e-6);
        let mut precedent = -1.0;
        for i in 0..=20 {
            let v = bezier_css(POP_EASE, i as f32 / 20.0);
            assert!(v >= precedent, "bezier non monotone à {i}");
            precedent = v;
        }
        // Le `cubic-bezier(.16,1.08,.38,.98)` dépasse 1 (rebond).
        assert!(bezier_css(POP_EASE, 0.5) > 0.9);
    }
}
