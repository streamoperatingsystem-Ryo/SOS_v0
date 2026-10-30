//! Overlay « alerte » natif — carte transitoire z 9997 (`.alerte-root`).
//!
//! Miroir de `playAlerte` / `stopAlerte` / `applyAlerteOverlayConfig`
//! (diffusion.html) : carte sombre au squelette de position unifié
//! (`position-overlay-config` / `alerte-config.overlay` — x/y/largeur,
//! hauteur auto = contenu), texte gras centré au-dessus, image éventuelle
//! dessous (`width:100%`, `max-height:60vh`, `object-fit:contain`), pop-in
//! `opacity 0→1` (0,35 s) + `scale(0,6→1)` (0,4 s,
//! `cubic-bezier(0.16,1.08,0.38,0.98)`), transition symétrique au
//! `alerte-stop` (`display:none` une fois la sortie finie).
//!
//! Toute la logique métier reste au domaine (`alertes.rs`, D4) : file
//! FIFO max 5, cooldowns, templates, timer autoritaire. Le natif ne peint
//! que la carte **en cours** — les frames `alerte-play` / `alerte-stop` /
//! `alerte-config` / `position-overlay-config` du canal `chat_tx`
//! alimentent [`EtatAlertes`] via `MoteurNatif::pousser_alerte`, comme
//! `EtatMusique`. Jamais par la scène : l'overlay est transitoire.
//!
//! Limites assumées du lot (parité partielle documentée) :
//! - **aucun son** : `alerte.son` et l'audio porté par les vidéos ne sont
//!   pas joués — le moteur n'a pas de lecture de fichier audio ;
//! - **vidéo non portée** : `media_kind:"video"` → carte texte seule (le
//!   pont ne transmet que les images) ;
//! - `text-shadow` et `border-radius:8px` du média non portés ;
//! - le cadre de scène suit les 2 styles du lot cadres (décision D3).

use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1Geometry, ID2D1Layer, ID2D1PathGeometry,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Resource, ID3D11Texture2D};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS,
};

use crate::cadres::CadreCfg;
use crate::gpu::{DescTexture, Format};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

/// z du calque alerte — `.alerte-root` (z-index 9997).
pub const Z_ALERTES: i32 = 9997;

/// `font-size` hérité de la carte — base des `em` du padding/gap
/// (body de diffusion.html = 16 px, comme le dashboard).
const EM: f32 = 16.0;
/// `.alerte-card` : `padding: 0.5em 0.9em`, `gap: 0.45em`.
const PAD_Y: f32 = 0.5 * EM;
const PAD_X: f32 = 0.9 * EM;
const GAP: f32 = 0.45 * EM;
/// `border-radius: 14px`.
const RAYON: f32 = 14.0;
/// `border: 2px solid rgba(224,224,224,0.85)`.
const BORD: f32 = 2.0;
/// `max-height: 60vh` de `.alerte-media` (fraction du canvas).
const MEDIA_MAX_H: f32 = 0.6;
/// Pop-in : `opacity` 0,35 s, `scale` 0,4 s — sortie symétrique.
const FONDU_S: f32 = 0.35;
const ZOOM_S: f32 = 0.4;
/// `transform: scale(0.6)` hors `.visible`.
const SCALE_MIN: f32 = 0.6;
/// `cubic-bezier(0.16, 1.08, 0.38, 0.98)` de la transition de scale.
const EASE_POP: (f32, f32, f32, f32) = (0.16, 1.08, 0.38, 0.98);
/// Marge du `alerteTimer` DOM (`duree_ms + 500 ms`) — expiration de
/// secours si `alerte-stop` ne vient jamais (WS lag, miroir du fallback).
const MARGE_EXPIRATION_MS: u64 = 500;

/// Fond de la carte (`rgba(10,10,10,0.92)`).
const FOND: [u8; 4] = [10, 10, 10, 235];
/// Bord de la carte (`rgba(224,224,224,0.85)`).
const COULEUR_BORD: [u8; 4] = [224, 224, 224, 217];
/// Texte sans cadre actif (`#ffffff` — repli de `playAlerte`).
const BLANC: [u8; 4] = [255, 255, 255, 255];
/// Squelette initial de `alerteOverlay` (miroir
/// `AlertesOverlayConfig::default` — haut-centré 600×120 sur 1920×1080).
const POSITION_DEFAUT: (f64, f64, f64, f64) = (660.0, 60.0, 600.0, 120.0);

// =============================================================================
// FRAMES — mutations décodées par le pont (`src/moteur/mod.rs`).
// =============================================================================

/// Mutation de l'overlay alerte — frames `chat_tx`.
#[derive(Debug, Clone, PartialEq)]
pub enum MajAlerte {
    /// Squelette de position (`alerte-config.overlay` ou
    /// `position-overlay-config.config`) : px canvas. `hauteur` est une
    /// référence visuelle — la carte reste en hauteur auto.
    Position {
        x: f64,
        y: f64,
        largeur: f64,
        hauteur: f64,
    },
    /// `alerte-play` : le domaine a déjà résolu template/cooldowns/file.
    Play {
        /// Texte affiché (template résolu).
        texte: String,
        /// `font-size` (`taille_px` de la config du type).
        taille_px: f32,
        /// Durée d'affichage — base du fallback d'expiration.
        duree_ms: u64,
        /// Chemin relatif `medias/…` d'une image ; `None` = texte seul
        /// (la vidéo n'est pas transmise à ce lot).
        media: Option<String>,
    },
    /// `alerte-stop` : fondu de sortie de la carte courante.
    Stop,
}

// =============================================================================
// ÉTAT LIVE — alimenté pendant la session, jamais par la scène.
// =============================================================================

/// Alerte affichée (l'état interne de `playAlerte`).
#[derive(Debug)]
struct AlerteEnCours {
    texte: String,
    taille_px: f32,
    duree_ms: u64,
    media: Option<PathBuf>,
    /// Début d'affichage — base du pop-in et du fallback d'expiration.
    debut: Instant,
    /// `alerte-stop` reçu — base du fondu de sortie.
    arret: Option<Instant>,
}

/// Ce que la composition peint ce tick-ci (phase entrée/stable/sortie
/// résolue — opacité et échelle déjà évaluées).
pub struct RenduAlerte<'a> {
    /// Texte résolu de l'alerte (template du domaine).
    pub texte: &'a str,
    /// `font-size` (`taille_px`).
    pub taille_px: f32,
    /// Image de l'alerte, chemin résolu (`base` jointe au `medias/` relatif).
    pub media: Option<&'a Path>,
    /// Opacité de la carte (fondus d'entrée/sortie).
    pub opacite: f32,
    /// Échelle du pop (`scale(0,6) → 1`), centrée sur la carte.
    pub echelle: f32,
    /// Squelette de position courant (x, y, largeur, hauteur).
    pub position: (f64, f64, f64, f64),
}

/// État vivant de l'overlay : position + alerte en cours + version de
/// contenu (clé de cache du bloc de rendu).
pub struct EtatAlertes {
    position: (f64, f64, f64, f64),
    en_cours: Option<AlerteEnCours>,
    version: u64,
}

impl Default for EtatAlertes {
    fn default() -> Self {
        Self {
            position: POSITION_DEFAUT,
            en_cours: None,
            version: 0,
        }
    }
}

impl EtatAlertes {
    pub fn nouveau() -> Self {
        Self::default()
    }

    /// Applique une mutation décodée — `base` = dossier des données
    /// (`medias/` y est relatif), passée par le moteur (`configurer_base`).
    pub fn appliquer(&mut self, maj: MajAlerte, base: &Path) {
        self.appliquer_at(maj, base, Instant::now());
    }

    /// `appliquer` à un instant donné (tests : le temps est injecté).
    fn appliquer_at(&mut self, maj: MajAlerte, base: &Path, maintenant: Instant) {
        match maj {
            MajAlerte::Position {
                x,
                y,
                largeur,
                hauteur,
            } => {
                // La clé du bloc inclut la position → re-mesure au tick
                // suivant (miroir `applyAlerteOverlayConfig`).
                self.position = (x, y, largeur, hauteur);
            }
            MajAlerte::Play {
                texte,
                taille_px,
                duree_ms,
                media,
            } => {
                self.en_cours = Some(AlerteEnCours {
                    texte,
                    taille_px: taille_px.max(1.0),
                    // `duree_ms.max(1000)` — même borne que `on_event`.
                    duree_ms: duree_ms.max(1000),
                    media: media.map(|rel| base.join(rel)),
                    debut: maintenant,
                    arret: None,
                });
                self.version = self.version.wrapping_add(1);
            }
            MajAlerte::Stop => {
                if let Some(a) = self.en_cours.as_mut() {
                    // Un stop rejoué ne ré-arme pas le fondu.
                    a.arret.get_or_insert(maintenant);
                }
            }
        }
    }

    /// Compteur monotone (clé de cache du bloc).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Squelette de position courant.
    pub fn position(&self) -> (f64, f64, f64, f64) {
        self.position
    }

    /// Carte à peindre ce tick-ci — `None` quand rien ne joue ou que le
    /// fondu de sortie est fini (`display:none` du DOM).
    pub fn rendu(&self) -> Option<RenduAlerte<'_>> {
        self.rendu_at(Instant::now())
    }

    /// `rendu` à un instant donné (tests : le temps est injecté).
    fn rendu_at(&self, maintenant: Instant) -> Option<RenduAlerte<'_>> {
        let a = self.en_cours.as_ref()?;
        // Fin affichée : `alerte-stop` (autoritaire), sinon le fallback
        // `duree_ms + 500 ms` (le `alerteTimer` local de la diffusion).
        let fin = a.arret.unwrap_or_else(|| {
            a.debut + Duration::from_millis(a.duree_ms + MARGE_EXPIRATION_MS)
        });
        let (opacite, echelle) = if maintenant < fin {
            // Entrée : `opacity` linéaire 0,35 s + `scale` bézier 0,4 s.
            let t = maintenant.duration_since(a.debut).as_secs_f32();
            (
                (t / FONDU_S).min(1.0),
                SCALE_MIN + (1.0 - SCALE_MIN) * bezier_css(EASE_POP, (t / ZOOM_S).min(1.0)),
            )
        } else {
            // Sortie : transition inversée — une entrée incomplète repart
            // de sa valeur courante (comportement des transitions CSS).
            let entree = fin.duration_since(a.debut).as_secs_f32();
            let t = maintenant.duration_since(fin).as_secs_f32();
            let op_in = (entree / FONDU_S).min(1.0);
            let sc_in =
                SCALE_MIN + (1.0 - SCALE_MIN) * bezier_css(EASE_POP, (entree / ZOOM_S).min(1.0));
            let op = op_in * (1.0 - t / FONDU_S);
            let sc = sc_in - (sc_in - SCALE_MIN) * bezier_css(EASE_POP, (t / ZOOM_S).min(1.0));
            // Sortie finie = les deux transitions terminées (`display:none`
            // du DOM). Comparer `sc` à SCALE_MIN resterait fragile en f32 :
            // la bézier converge sous 1 par epsilon → `sc` ~0,60000004.
            if t >= FONDU_S.max(ZOOM_S) {
                return None;
            }
            (op.max(0.0), sc.max(SCALE_MIN))
        };
        Some(RenduAlerte {
            texte: &a.texte,
            taille_px: a.taille_px,
            media: a.media.as_deref(),
            opacite,
            echelle,
            position: self.position,
        })
    }
}

/// Évalue une `cubic-bezier(x1,y1,x2,y2)` CSS à `x` ∈ [0,1] — la courbe
/// x(t) est monotone, donc dichotomie (16 itérations ≪ le pixel).
fn bezier_css((x1, y1, x2, y2): (f32, f32, f32, f32), x: f32) -> f32 {
    fn courbe(a1: f32, a2: f32, t: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * a1 + 3.0 * u * t * t * a2 + t * t * t
    }
    let x = x.clamp(0.0, 1.0);
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    let mut t = x;
    for _ in 0..16 {
        if courbe(x1, x2, t) < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) * 0.5;
    }
    courbe(y1, y2, t)
}

// =============================================================================
// CACHE DE RENDU — un seul bloc (une seule alerte à la fois, contrat
// domaine). Reconstruit quand la clé change, comme les blocs musique.
// =============================================================================

/// Texte de la carte mis en page (couleur posée au dessin).
struct LigneAlerte {
    layout: IDWriteTextLayout,
    /// Le format doit vivre aussi longtemps que le layout.
    _format: IDWriteTextFormat,
    /// Hauteur des métriques (la largeur utile = `w_content`).
    h: f32,
}

/// Cache de la carte : layout + bitmap + polygone du cadre.
pub(super) struct BlocAlerte {
    /// Clé de validité (`cle_alerte`).
    cle: u64,
    /// Rect de la carte `(x, y, w, h)` en px canvas.
    carte: (f32, f32, f32, f32),
    /// Texte de l'alerte (`None` si `texte` vide).
    texte: Option<LigneAlerte>,
    /// Image décodée : texture (garde la bitmap en vie) + vue + taille src.
    media: Option<(ID3D11Texture2D, ID2D1Bitmap1, (f32, f32))>,
    /// Cadre concret de la carte (rect appliqué) + son polygone.
    cadre: Option<(CadreCfg, ID2D1PathGeometry)>,
    /// Origine du bloc texte dans la carte (`x + pad_x`, `y + pad_y`).
    texte_at: (f32, f32),
    /// Destination du média (`contain` centré dans la zone utile).
    media_dst: Option<D2D_RECT_F>,
    /// Couleur du texte (`bpmResoudreCouleurs().p` ou blanc).
    couleur_texte: [u8; 4],
}

/// Clé de cache du bloc : version de l'état + squelette + gabarit de
/// cadre + hauteur du canvas (le `60vh` du média).
fn cle_alerte(
    version: u64,
    position: (f64, f64, f64, f64),
    cadre: Option<&CadreCfg>,
    canvas_h: u32,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    position.0.to_bits().hash(&mut h);
    position.1.to_bits().hash(&mut h);
    position.2.to_bits().hash(&mut h);
    position.3.to_bits().hash(&mut h);
    canvas_h.hash(&mut h);
    if let Some(c) = cadre {
        c.style.hash(&mut h);
        c.stroke_width.to_bits().hash(&mut h);
        c.couleur.hash(&mut h);
        c.couleur_fin.hash(&mut h);
        c.gradient_angle.to_bits().hash(&mut h);
    }
    h.finish()
}

impl DessinateurTexte {
    /// Dessine l'alerte en cours — dans une passe ouverte.
    ///
    /// `cadre` = gabarit du cadre de scène (`cadre_overlay_alerte`, rect
    /// nul) : la carte prend son style à SA taille, comme
    /// `appliquerCadreOverlay` régénère le SVG sur la carte mesurée.
    pub fn dessiner_alerte(
        &mut self,
        etat: &EtatAlertes,
        cadre: Option<&CadreCfg>,
        canvas_h: u32,
    ) -> Result<(), ErreurTexte> {
        let Some(rendu) = etat.rendu() else {
            return Ok(());
        };
        self.synchroniser_bloc_alerte(etat.version(), &rendu, cadre, canvas_h)?;

        // Les handles COM du bloc sont clonés : garder `self.bloc_alerte`
        // emprunté empêcherait les appels `&mut self` (`dessiner_cadre`).
        let (carte, texte_at, layout, bitmap, media_dst, cadre_geo, couleur) = {
            let Some(bloc) = &self.bloc_alerte else {
                return Ok(());
            };
            (
                bloc.carte,
                bloc.texte_at,
                bloc.texte.as_ref().map(|t| t.layout.clone()),
                bloc.media.as_ref().map(|m| m.1.clone()),
                bloc.media_dst,
                bloc.cadre.clone(),
                bloc.couleur_texte,
            )
        };
        let (x, y, w, h) = carte;

        // Pop : `scale` autour du centre de la carte (matrice explicite —
        // `translate(c)·scale(s)·translate(-c)`).
        let zoome = rendu.echelle < 0.999;
        if zoome {
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            let s = rendu.echelle;
            let m = Matrix3x2 {
                M11: s,
                M12: 0.0,
                M21: 0.0,
                M22: s,
                M31: cx * (1.0 - s),
                M32: cy * (1.0 - s),
            };
            unsafe { self.cible_rt.SetTransform(&m) };
        }

        // Couche : opacité du fondu + clip éventuel au polygone du cadre
        // (`clip-path` de la carte — tout le contenu, bordure comprise).
        let masque: Option<ID2D1Geometry> = match &cadre_geo {
            Some((_, geo)) => Some(geo.cast().map_err(|e| {
                ErreurTexte::Gpu(format!("ID2D1Geometry alerte : {}", e.message()))
            })?),
            None => None,
        };
        // Le clip géométrique borne déjà ; la marge ne sert qu'aux
        // débords du trait de cadre (`dessiner` ré-applique son clip).
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
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer alerte : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let resultat = (|| -> Result<(), ErreurTexte> {
            let rt = &self.cible_rt;
            let fond = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            let b_fond = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(FOND), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse alerte : {}", e.message())))?;
            let b_bord = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(COULEUR_BORD), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse alerte : {}", e.message())))?;
            // `border: 2px` centrée sur le bord → trait interne à 1 px.
            let bord_rect = D2D_RECT_F {
                left: x + BORD / 2.0,
                top: y + BORD / 2.0,
                right: x + w - BORD / 2.0,
                bottom: y + h - BORD / 2.0,
            };
            if cadre_geo.is_some() {
                // `clip-path` polygonal : fond et bordure rognés (le radius
                // 14 px disparaît dans la forme, comme le CSS).
                unsafe { rt.FillRectangle(&fond, &b_fond) };
                unsafe { rt.DrawRectangle(&bord_rect, &b_bord, BORD, None) };
            } else {
                unsafe {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: fond,
                            radiusX: RAYON,
                            radiusY: RAYON,
                        },
                        &b_fond,
                    )
                };
                unsafe {
                    rt.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: bord_rect,
                            radiusX: (RAYON - BORD / 2.0).max(0.0),
                            radiusY: (RAYON - BORD / 2.0).max(0.0),
                        },
                        &b_bord,
                        BORD,
                        None,
                    )
                };
            }
            // Média sous le texte (ordre de la colonne flex — sans
            // recouvrement, l'ordre de dessin est indifférent).
            if let (Some(bitmap), Some(dst)) = (bitmap.as_ref(), media_dst.as_ref()) {
                unsafe {
                    rt.DrawBitmap(
                        bitmap,
                        Some(dst),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    )
                };
            }
            if let Some(layout) = &layout {
                let b_texte = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse alerte : {}", e.message()))
                    })?;
                unsafe {
                    rt.DrawTextLayout(
                        D2D_POINT_2F {
                            x: texte_at.0,
                            y: texte_at.1,
                        },
                        layout,
                        &b_texte,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
            }
            // Trait du cadre au-dessus (`.cadre-overlay`, z 5 dans la carte).
            if let Some((cfg, geo)) = &cadre_geo {
                self.dessiner_cadre(cfg, geo)?;
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

    /// (Re)construit le bloc quand la clé a changé — la carte est
    /// re-mesurée à chaque play / repositionnement, comme le DOM
    /// (`requestAnimationFrame` de `playAlerte`).
    fn synchroniser_bloc_alerte(
        &mut self,
        version: u64,
        rendu: &RenduAlerte,
        cadre: Option<&CadreCfg>,
        canvas_h: u32,
    ) -> Result<(), ErreurTexte> {
        let cle = cle_alerte(version, rendu.position, cadre, canvas_h);
        if self.bloc_alerte.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        let (x, y, largeur, _) = rendu.position;
        let (x, y, largeur) = (x as f32, y as f32, largeur as f32);
        // Zone utile de la carte (`padding: 0.9em` des deux côtés).
        let w_content = (largeur - 2.0 * PAD_X).max(1.0);

        // Texte centré (`text-align:center` + `word-break:break-word` →
        // WRAP : DirectWrite casse aussi les mots trop longs).
        let texte = if rendu.texte.is_empty() {
            None
        } else {
            let style = StyleTexte {
                // `system-ui` du body → famille système, pas la collection
                // embarquée (police vide = repli `Segoe UI` de `format`).
                police: String::new(),
                taille: rendu.taille_px,
                gras: true,
                contour_epaisseur: 0.0,
                ..StyleTexte::default()
            };
            let format = self.format(&style)?;
            unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER) }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("alignement alerte : {}", e.message()))
                })?;
            let contenu: Vec<u16> = rendu.texte.encode_utf16().collect();
            let layout =
                unsafe { self.dwrite.CreateTextLayout(&contenu, &format, w_content, 4096.0) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("layout alerte : {}", e.message()))
                    })?;
            let mut m = DWRITE_TEXT_METRICS::default();
            unsafe { layout.GetMetrics(&mut m) }
                .map_err(|e| ErreurTexte::Systeme(format!("métriques alerte : {}", e.message())))?;
            Some(LigneAlerte {
                layout,
                _format: format,
                h: m.height,
            })
        };
        let h_texte = texte.as_ref().map(|t| t.h).unwrap_or(0.0);

        // Média : décodé une fois (échec → texte seul, jamais fatal).
        let media = rendu
            .media
            .and_then(|chemin| self.charger_image_alerte(chemin));

        // `width:100%` + `max-height:60vh` : l'élément fait `w_content`
        // de large, sa hauteur = ratio naturel borné ; `object-fit:contain`
        // centre le bitmap dans l'élément.
        let mut media_dst = None;
        let mut h_media = 0.0_f32;
        if let Some((_, _, (iw, ih))) = &media {
            if *iw > 0.0 && *ih > 0.0 {
                let el_h = (ih * w_content / iw).min(MEDIA_MAX_H * canvas_h as f32);
                let mw = (iw * el_h / ih).min(w_content);
                let mx = x + PAD_X + (w_content - mw) / 2.0;
                let my = y + PAD_Y + h_texte + GAP;
                media_dst = Some(D2D_RECT_F {
                    left: mx,
                    top: my,
                    right: mx + mw,
                    bottom: my + el_h,
                });
                h_media = el_h;
            }
        }

        // Carte : largeur du squelette, hauteur auto (contenu + padding).
        let h = PAD_Y + h_texte + if media_dst.is_some() { GAP + h_media } else { 0.0 } + PAD_Y;
        let carte = (x, y, largeur, h.max(1.0));

        // Cadre concret : le gabarit appliqué au rect de la carte mesurée
        // (`genererCadreSVG` à la taille réelle de la carte).
        let cadre_geo = match cadre {
            Some(gabarit) => {
                let mut cfg = gabarit.clone();
                cfg.rect = (
                    carte.0 as f64,
                    carte.1 as f64,
                    carte.2 as f64,
                    carte.3 as f64,
                );
                cfg.z = Z_ALERTES;
                match self.geometrie_cadre(&cfg) {
                    Ok(geo) => Some((cfg, geo)),
                    Err(e) => {
                        log::warn!("alerte : polygone du cadre ignoré ({e})");
                        None
                    }
                }
            }
            None => None,
        };

        self.bloc_alerte = Some(BlocAlerte {
            cle,
            carte,
            texte,
            media,
            cadre: cadre_geo,
            texte_at: (x + PAD_X, y + PAD_Y),
            media_dst,
            // `couleur` = `bpmResoudreCouleurs().p` si cadre actif, blanc
            // sinon (playAlerte — plus de champ couleur sur les alertes).
            couleur_texte: cadre
                .map(|c| super::horloge::extremite_lumineuse(c.couleur, c.couleur_fin))
                .unwrap_or(BLANC),
        });
        Ok(())
    }

    /// Décode l'image de l'alerte en bitmap Direct2D (une fois — le bloc
    /// la conserve). `None` si illisible : la carte tombe en texte seul.
    fn charger_image_alerte(
        &mut self,
        chemin: &Path,
    ) -> Option<(ID3D11Texture2D, ID2D1Bitmap1, (f32, f32))> {
        let (w, h, pixels, alpha) = match crate::media::decoder_pbgra(chemin) {
            Ok(d) if d.0 > 0 && d.1 > 0 => d,
            Ok(_) => return None,
            Err(e) => {
                log::warn!("alerte : image {} ignorée ({e})", chemin.display());
                return None;
            }
        };
        let texture = DescTexture::new(Format::Bgra8, w, h)
            .map_err(|e| e.to_string())
            .and_then(|desc| self.contexte_texture(&desc));
        let texture = match texture {
            Ok(t) => t,
            Err(e) => {
                log::warn!("alerte : texture {e}");
                return None;
            }
        };
        let ressource: ID3D11Resource = match texture.cast() {
            Ok(r) => r,
            Err(e) => {
                log::warn!("alerte : ressource {}", e.message());
                return None;
            }
        };
        let contexte = match unsafe { self.device_d3d.GetImmediateContext() } {
            Ok(c) => c,
            Err(e) => {
                log::warn!("alerte : contexte {}", e.message());
                return None;
            }
        };
        unsafe {
            contexte.UpdateSubresource(
                Some(&ressource),
                0,
                None,
                pixels.as_ptr() as *const std::ffi::c_void,
                w * 4,
                0,
            );
        }
        match self.bitmap_media(&texture, alpha) {
            Ok(b) => Some((texture, b, (w as f32, h as f32))),
            Err(e) => {
                log::warn!("alerte : bitmap {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from("C:/data")
    }

    fn play() -> MajAlerte {
        MajAlerte::Play {
            texte: "Streamy vient de suivre !".to_string(),
            taille_px: 48.0,
            duree_ms: 5000,
            media: None,
        }
    }

    #[test]
    fn position_par_defaut_suit_le_squelette_dom() {
        let etat = EtatAlertes::nouveau();
        assert_eq!(etat.position(), POSITION_DEFAUT);
        assert_eq!(etat.position().2, 600.0);
        assert!(etat.rendu().is_none(), "aucune alerte au départ");
    }

    #[test]
    fn play_puis_stop_puis_expiration() {
        let t0 = Instant::now();
        let mut etat = EtatAlertes::nouveau();
        etat.appliquer_at(play(), &base(), t0);
        // Entrée à mi-fondu : opacité < 1, échelle en montée.
        let r = etat.rendu_at(t0 + Duration::from_millis(100)).unwrap();
        assert!(r.opacite > 0.0 && r.opacite < 1.0);
        assert!(r.echelle >= SCALE_MIN && r.echelle < 1.0);
        // Stable après 0,4 s.
        let r = etat.rendu_at(t0 + Duration::from_millis(1000)).unwrap();
        assert_eq!(r.opacite, 1.0);
        assert!((r.echelle - 1.0).abs() < 0.001);
        // Stop → fondu de sortie puis disparition.
        etat.appliquer_at(MajAlerte::Stop, &base(), t0 + Duration::from_millis(1100));
        let r = etat.rendu_at(t0 + Duration::from_millis(1200)).unwrap();
        assert!(r.opacite < 1.0);
        assert!(
            etat.rendu_at(t0 + Duration::from_millis(1600)).is_none(),
            "sortie finie → carte masquée"
        );
    }

    #[test]
    fn expiration_de_secours_sans_stop() {
        let t0 = Instant::now();
        let mut etat = EtatAlertes::nouveau();
        etat.appliquer_at(play(), &base(), t0);
        // `duree_ms + 500 ms` → début du fondu de sortie.
        let fin = t0 + Duration::from_millis(5500);
        assert!(etat.rendu_at(fin - Duration::from_millis(1)).is_some());
        assert!(etat.rendu_at(fin + Duration::from_millis(200)).is_some());
        assert!(etat.rendu_at(fin + Duration::from_millis(500)).is_none());
    }

    #[test]
    fn media_rel_est_resolu_sur_base() {
        let t0 = Instant::now();
        let mut etat = EtatAlertes::nouveau();
        etat.appliquer_at(
            MajAlerte::Play {
                texte: "Streamy vient de suivre !".to_string(),
                taille_px: 48.0,
                duree_ms: 5000,
                media: Some("medias/abc.png".to_string()),
            },
            &base(),
            t0,
        );
        let r = etat.rendu_at(t0).unwrap();
        let attendu = Path::new("C:/data").join("medias/abc.png");
        assert_eq!(r.media, Some(attendu.as_path()));
    }

    #[test]
    fn version_n_augmente_qu_au_play() {
        let t0 = Instant::now();
        let mut etat = EtatAlertes::nouveau();
        assert_eq!(etat.version(), 0);
        etat.appliquer_at(
            MajAlerte::Position {
                x: 0.0,
                y: 0.0,
                largeur: 300.0,
                hauteur: 100.0,
            },
            &base(),
            t0,
        );
        assert_eq!(etat.version(), 0, "la position n'est pas du contenu");
        etat.appliquer_at(play(), &base(), t0);
        assert_eq!(etat.version(), 1);
        etat.appliquer_at(MajAlerte::Stop, &base(), t0);
        assert_eq!(etat.version(), 1);
    }

    #[test]
    fn bezier_css_borne_et_monotone() {
        assert_eq!(bezier_css(EASE_POP, 0.0), 0.0);
        assert!((bezier_css(EASE_POP, 1.0) - 1.0).abs() < 1e-6);
        let mut precedent = -1.0_f32;
        for i in 0..=20 {
            let v = bezier_css(EASE_POP, i as f32 / 20.0);
            assert!(v >= precedent, "bezier non monotone à {i}");
            precedent = v;
        }
    }

    #[test]
    fn cle_alerte_sensible_a_version_position_et_cadre() {
        let pos = POSITION_DEFAUT;
        let a = cle_alerte(1, pos, None, 1080);
        assert_ne!(a, cle_alerte(2, pos, None, 1080), "version");
        assert_ne!(
            a,
            cle_alerte(1, (0.0, 60.0, 600.0, 120.0), None, 1080),
            "position"
        );
        assert_ne!(a, cle_alerte(1, pos, None, 720), "canvas");
        let cadre = CadreCfg {
            rect: (0.0, 0.0, 0.0, 0.0),
            style: crate::cadres::StyleCadre::MgStyle,
            stroke_width: 10.0,
            couleur: [0, 0, 0, 255],
            couleur_fin: [0x8b, 0x5c, 0xf6, 255],
            gradient_angle: 135.0,
            z: Z_ALERTES,
        };
        assert_ne!(a, cle_alerte(1, pos, Some(&cadre), 1080), "cadre");
    }
}
