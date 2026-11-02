//! Overlay « pop de validation des splits » — `z 9993`.
//!
//! Miroir de `.splitpop-root` / `.splitpop-carte` (`diffusion.html` 1150-1310)
//! et de `srPopSplit` / `splitPopCacher` / `applySplitPopOverlayConfig`
//! (6415-6620 + 6870-6990). À chaque split **franchi** (ou **skippé**) pendant
//! une run, un pop s'affiche `pointsDureePop` secondes à la position du
//! squelette **dédié** (`splitpop-pos`, `position_splitpop.json`), puis
//! disparaît : rien ne persiste à l'écran.
//!
//! **Canal** : aucune frame nouvelle pour le contenu — le pop est dérivé des
//! **mêmes** mutations que le widget speedrun (frames `chat_tx`
//! `speedrun-etat`, `MajSpeedrun`), avec les options du widget
//! (`srVoirPoints`, `srPointsDureePop`, `srCouleurAhead`/`Behind`). Seule la
//! **position** arrive par la frame `splitpop-pos` (squelette dédié, poussé au
//! chargement et à chaque réglage — resync `splitpop-resync`).
//!
//! Composition (DOM) : badge « SPLIT n » (pastille teintée), hero = temps
//! gagné/perdu (Anton 1.5em, `skewX(-5deg)`, couleur du résultat) et mot-clé
//! GAGNÉ / PERDU — ou, **sans delta** (skippé / franchi sans PB), le mot
//! SKIP / VALIDÉ devient le hero. Pop-in écrasé (`scale(.15) rotate(-9deg)` →
//! `1.22 rotate(2.5deg)` → `0.95 rotate(-1deg)` → 1 en 0,5 s), re-pop à
//! 2 s / 4 s si la durée le permet, fondu de sortie 0,3 s.
//!
//! Limites assumées :
//! - `filter: blur(2px)` de l'aura et `box-shadow` de la pastille non portés
//!   (même limite qu'E3) — l'aura est un **dégradé radial** sombre + teinté ;
//! - le `flash` (`filter: brightness(2.1) saturate(1.6)` sur le hero) est
//!   approché par un **éclaircissement** de la couleur du hero (mélange vers
//!   le blanc sur les 45 % premiers) ;
//! - `text-shadow` / `drop-shadow` du DOM non portés (E3) : le hero garde sa
//!   couleur pleine, sans glow ;
//! - le son du pop (`srPointsSon`) est **hors périmètre** (audio d'overlay,
//!   lot E6d n'a porté que les pistes de la présentation) — signalé.

use std::hash::{Hash, Hasher};
use std::time::Instant;

use windows::Win32::Graphics::Direct2D::Common::{D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1GradientStopCollection, ID2D1Layer, ID2D1SolidColorBrush,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
};
use windows::core::{Interface, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_EXTRA_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_LINE_SPACING_METHOD_UNIFORM,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte};

/// z du pop (`.splitpop-root`, z-index 9993) — sous le pad (9995), les
/// alertes (9997) et le bandeau (9998), au-dessus des modules HUD (9000).
pub const Z_SPLITPOP: i32 = 9993;

/// `.splitpop-root { transition: opacity .3s ease, transform .3s … }`.
const FONDU_S: f32 = 0.3;
const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);
const POP_EASE: (f32, f32, f32, f32) = (0.16, 1.08, 0.38, 0.98);
const POP_DEPART: f32 = 0.6;
/// `@keyframes splitpop-impact` — 0,5 s.
const IMPACT_S: f32 = 0.5;
const IMPACT_EASE: (f32, f32, f32, f32) = (0.22, 1.32, 0.36, 1.0);
/// `@keyframes splitpopRepop` — 0,45 s, joué à 2 s puis 4 s.
const REPOP_S: f32 = 0.45;
const REPOP_EASE: (f32, f32, f32, f32) = (0.34, 1.56, 0.64, 1.0);
const REPOP_DELAIS: [f32; 2] = [2.0, 4.0];
/// Marge du DOM avant le fondu de sortie (600 ms).
const REPOP_MARGE_S: f32 = 0.6;
/// Le DOM retire le nœud 350 ms après le début du fondu.
const SORTIE_S: f32 = 0.35;
/// Dédup multi-widget : même signature à moins de 1,5 s = même event.
const DEDUP_S: f32 = 1.5;

/// Squelette par défaut (`defaut_splitpop` de `position_overlay.rs`) —
/// référence 1920×1080.
pub const POSITION_DEFAUT: (f64, f64, f64, f64) = (660.0, 60.0, 600.0, 120.0);
/// `Math.max(18, Math.round(hauteur * 0.4))` — base des `em` du pop.
const TAILLE_MIN: f32 = 18.0;
const TAILLE_RATIO: f32 = 0.4;

/// `.splitpop-carte { gap: 0.08em }`.
const CARTE_GAP_EM: f32 = 0.08;
/// `.splitpop-point` : 0,3em, `padding .24em .8em .24em 1.08em`, `radius 999`.
const POINT_TAILLE_EM: f32 = 0.3;
const POINT_ESPACEMENT_EM: f32 = 0.28;
const POINT_PAD_V_EM: f32 = 0.24;
const POINT_PAD_G_EM: f32 = 0.8;
const POINT_PAD_D_EM: f32 = 1.08;
/// `.splitpop-delta` : Anton 1,5em, `skewX(-5deg)`.
const DELTA_TAILLE_EM: f32 = 1.5;
const DELTA_SKEW: f32 = -5.0;
/// `.splitpop-carte::after` (mot sous le hero) : 0,3em, `letter-spacing .5em`.
const MOT_TAILLE_EM: f32 = 0.3;
const MOT_ESPACEMENT_EM: f32 = 0.5;
/// `.sans-delta::after` (le mot devient le hero) : 0,85em, `.14em`.
const MOT_HERO_TAILLE_EM: f32 = 0.85;
const MOT_HERO_ESPACEMENT_EM: f32 = 0.14;
/// `.splitpop-carte { padding: .1em .6em }`.
const CARTE_PAD_V_EM: f32 = 0.1;
const CARTE_PAD_H_EM: f32 = 0.6;
/// `::before` — aura `inset: -0.5em -1.1em`.
const AURA_INSET_V_EM: f32 = 0.5;
const AURA_INSET_H_EM: f32 = 1.1;
const AURA_TEINTE: f32 = 0.15;
const AURA_SOMBRE: [u8; 4] = [0, 0, 0, 209];

/// Police du mot-clé (`.splitpop-carte` = `"Rajdhani", "Segoe UI"`).
const POLICE_CARTE: &str = "Rajdhani";
/// Police du hero (`.splitpop-delta` = `"Anton", "Rajdhani"`).
const POLICE_HERO: &str = "Anton";

// =============================================================================
// FRAMES — mutations décodées par le pont (`src/moteur/mod.rs`).
// =============================================================================

/// Un pop à afficher : le point franchi/skippé et son résultat.
#[derive(Debug, Clone, PartialEq)]
pub struct PopSplit {
    /// Index du segment **bouclé** (0-based) — le badge affiche `index + 1`.
    pub index: i32,
    /// `true` = en avance/égal (vert), `false` = en retard ou skippé (rouge).
    pub ok: bool,
    /// Delta formaté (`"+1.23"` / `"−0.45"`) ; **vide** = sans delta
    /// (le mot-clé devient le hero).
    pub delta: String,
    /// Signature de dédup (`"split:12:1:23.45"` / `"skip:12"`).
    pub signature: String,
}

/// Mutation du pop.
#[derive(Debug, Clone, PartialEq)]
pub enum MajSplitPop {
    /// Affiche un pop (déjà dédupliqué par l'appelant pour la signature).
    Afficher {
        pop: PopSplit,
        /// `srCouleurAhead` du widget (ou défaut vert).
        couleur_ahead: [u8; 4],
        /// `srCouleurBehind` du widget (ou défaut rouge).
        couleur_behind: [u8; 4],
        /// `srPointsDureePop` en secondes (défaut 2, minimum 1).
        duree_s: u32,
        /// `srPointsSon` — son joué **une fois** à l'ouverture du pop
        /// (`medias/…`, fichier local). `None` = silencieux.
        son: Option<String>,
        /// `srPointsSonVolume` (0-1) — gain du son.
        volume: f32,
    },
    /// Cache le pop (nouvelle tentative : started/reset/goto/stopped).
    Cacher,
    /// Frame `splitpop-pos` : squelette dédié (x, y, largeur, hauteur).
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
struct PopEnCours {
    pop: PopSplit,
    couleur_ahead: [u8; 4],
    couleur_behind: [u8; 4],
    duree_s: f32,
    debut: Instant,
    /// Son du pop (chemin `medias/…`) et gain — joué une fois.
    son: Option<String>,
    volume: f32,
}

/// État vivant du pop : squelette + pop en cours + dédup.
pub struct EtatSplitPop {
    position: (f64, f64, f64, f64),
    en_cours: Option<PopEnCours>,
    /// Dernière signature affichée et son instant (dédup multi-widget).
    derniere_signature: String,
    derniere_ts: Option<Instant>,
    version: u64,
    /// Incrémenté à **chaque** pop accepté : le rendu y voit l'ordre de
    /// (re)jouer le son, sans dépendre de l'horloge du widget.
    son_version: u64,
}

impl Default for EtatSplitPop {
    fn default() -> Self {
        Self {
            position: POSITION_DEFAUT,
            en_cours: None,
            derniere_signature: String::new(),
            derniere_ts: None,
            version: 0,
            son_version: 0,
        }
    }
}

/// Ce que la composition peint ce tick-ci (phases déjà résolues).
pub struct RenduSplitPop<'a> {
    pub pop: &'a PopSplit,
    pub position: (f64, f64, f64, f64),
    /// Opacité du root (fondu d'entrée / de sortie).
    pub opacite: f32,
    /// `scale(0.6 → 1)` du root.
    pub echelle: f32,
    /// Progression de l'impact de la carte (0 → 1 sur 0,5 s).
    pub impact: f32,
    /// Éclaircissement du hero (1 = flash plein → 0).
    pub flash: f32,
    /// Progression d'un re-pop en cours (0 → 1 sur 0,45 s).
    pub repop: Option<f32>,
    pub couleur_ahead: [u8; 4],
    pub couleur_behind: [u8; 4],
    /// Base des `em` en px (`max(18, hauteur × 0,4)`).
    pub em: f32,
}

impl EtatSplitPop {
    pub fn nouveau() -> Self {
        Self::default()
    }

    pub fn appliquer(&mut self, maj: MajSplitPop) {
        self.appliquer_at(maj, Instant::now());
    }

    fn appliquer_at(&mut self, maj: MajSplitPop, maintenant: Instant) {
        match maj {
            MajSplitPop::Afficher {
                pop,
                couleur_ahead,
                couleur_behind,
                duree_s,
                son,
                volume,
            } => {
                // Dédup multi-widget : un même event traversant plusieurs
                // widgets speedrun ne pop qu'une fois (`srPopSplit`).
                let doublon = self.derniere_ts.is_some_and(|ts| {
                    self.derniere_signature == pop.signature
                        && maintenant.saturating_duration_since(ts).as_secs_f32() < DEDUP_S
                });
                if doublon {
                    return;
                }
                self.derniere_signature = pop.signature.clone();
                self.derniere_ts = Some(maintenant);
                self.en_cours = Some(PopEnCours {
                    pop,
                    couleur_ahead,
                    couleur_behind,
                    duree_s: (duree_s.max(1)) as f32,
                    debut: maintenant,
                    son: son.filter(|s| !s.is_empty()),
                    volume: volume.clamp(0.0, 1.0),
                });
                self.son_version = self.son_version.wrapping_add(1);
                self.version = self.version.wrapping_add(1);
            }
            MajSplitPop::Cacher => {
                if self.en_cours.take().is_some() {
                    self.version = self.version.wrapping_add(1);
                }
            }
            MajSplitPop::Config {
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

    /// Son à jouer : `(chemin, volume, version)`. `version` change à chaque
    /// pop accepté — le rendu ne rejoue le fichier que si elle a bougé
    /// (miroir du `new Audio(...)` du DOM, un par pop).
    pub fn son(&self) -> Option<(&str, f32, u64)> {
        let pop = self.en_cours.as_ref()?;
        pop.son.as_deref().map(|s| (s, pop.volume, self.son_version))
    }

    /// Gain du son du pop en millièmes (0 hors pop) — entrée du mixeur.
    pub fn gain_son_milliemes(&self) -> u32 {
        match self.en_cours.as_ref().filter(|p| p.son.is_some()) {
            Some(p) => (p.volume * 1000.0).round() as u32,
            None => 0,
        }
    }

    pub fn position(&self) -> (f64, f64, f64, f64) {
        self.position
    }

    /// Base des `em` en px — `max(18, round(hauteur × 0,4))` du DOM.
    pub fn em(&self) -> f32 {
        (self.position.3 * TAILLE_RATIO as f64).round() as f32
    }

    pub fn rendu(&self) -> Option<RenduSplitPop<'_>> {
        self.rendu_at(Instant::now())
    }

    fn rendu_at(&self, maintenant: Instant) -> Option<RenduSplitPop<'_>> {
        let pop = self.en_cours.as_ref()?;
        let t = maintenant.saturating_duration_since(pop.debut).as_secs_f32();
        // Le DOM retire le nœud 350 ms après le début du fondu de sortie.
        let fin = pop.duree_s;
        if t >= fin + SORTIE_S {
            return None;
        }
        let (opacite, echelle) = if t < fin {
            let k = (t / FONDU_S).clamp(0.0, 1.0);
            let e = bezier_css(POP_EASE, k);
            (e, POP_DEPART + (1.0 - POP_DEPART) * e)
        } else {
            let k = ((t - fin) / SORTIE_S).clamp(0.0, 1.0);
            let e = bezier_css(EASE, k);
            (1.0 - e, 1.0 - (1.0 - POP_DEPART) * e)
        };
        // Flash : `brightness(2.1)` sur les 45 % premiers, puis retour.
        let flash = if t < IMPACT_S {
            let k = (t / (IMPACT_S * 0.45)).clamp(0.0, 1.0);
            1.0 - bezier_css(EASE, k)
        } else {
            0.0
        };
        // Re-pop à 2 s / 4 s si la durée laisse 600 ms de marge.
        let repop = REPOP_DELAIS
            .iter()
            .filter(|d| fin > **d + REPOP_MARGE_S)
            .find_map(|d| {
                let ecoule = t - d;
                (0.0..REPOP_S).contains(&ecoule).then(|| ecoule / REPOP_S)
            });
        Some(RenduSplitPop {
            pop: &pop.pop,
            position: self.position,
            opacite: opacite.clamp(0.0, 1.0),
            echelle: echelle.max(0.01),
            impact: (t / IMPACT_S).clamp(0.0, 1.0),
            flash,
            repop,
            couleur_ahead: pop.couleur_ahead,
            couleur_behind: pop.couleur_behind,
            em: self.em().max(TAILLE_MIN),
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

/// `color-mix(in srgb, c part%, base)` — mélange sRGB droit (CSS).
fn meler(c: [u8; 4], part: f32, base: [u8; 4]) -> [u8; 4] {
    let p = part.clamp(0.0, 1.0);
    let f = |a: u8, b: u8| (a as f32 * p + b as f32 * (1.0 - p)).round() as u8;
    [
        f(c[0], base[0]),
        f(c[1], base[1]),
        f(c[2], base[2]),
        f(c[3], base[3]),
    ]
}

/// Mot-clé du pop : `GAGNÉ`/`PERDU` avec delta, `VALIDÉ`/`SKIP` sans delta.
pub fn mot_split(ok: bool, delta: &str) -> &'static str {
    match (ok, delta.trim().is_empty()) {
        (true, false) => "GAGNÉ",
        (false, false) => "PERDU",
        (true, true) => "VALIDÉ",
        (false, true) => "SKIP",
    }
}

// =============================================================================
// BLOC DE RENDU
// =============================================================================

struct Ligne {
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    largeur: f32,
    hauteur: f32,
}

pub(super) struct BlocSplitPop {
    cle: u64,
    /// Badge « SPLIT n ».
    point: Option<Ligne>,
    /// Hero (delta) — `None` quand le mot-clé devient le hero.
    delta: Option<Ligne>,
    /// Mot-clé (sous le hero, ou hero lui-même sans delta).
    mot: Option<Ligne>,
    /// Boîte de la carte (largeur × hauteur) et décalage du contenu.
    carte: (f32, f32),
}

fn cle_splitpop(version: u64, em_tenth: u32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    em_tenth.hash(&mut h);
    h.finish()
}

impl DessinateurTexte {
    /// (Re)joue le son du pop quand la version change, puis l'avance.
    ///
    /// Le fichier est **rouvert** à chaque pop (équivalent du `new Audio`
    /// du DOM) : `AudioFichier::ouvrir` remet les PTS à zéro depuis
    /// `temps_s`. Aucun réseau : chemin `medias/…` relatif à `base`.
    fn synchroniser_son_splitpop(&mut self, etat: &EtatSplitPop, temps_s: f64) {
        let temps_us = (temps_s * 1e6) as i64;
        match etat.son() {
            Some((rel, _, version)) => {
                if version != self.splitpop_son_vu {
                    self.splitpop_son_vu = version;
                    self.splitpop_audio = None;
                    self.splitpop_audio_rel = None;
                    let chemin = self.base.join(rel);
                    match crate::media::AudioFichier::ouvrir(&chemin, temps_us) {
                        Ok(audio) => {
                            self.splitpop_audio = Some(audio);
                            self.splitpop_audio_rel = Some(rel.to_string());
                        }
                        // Fichier absent/illisible : pop muet, jamais fatal
                        // (même politique que le pad et les pistes E6d).
                        Err(e) => log::debug!("pop : son {rel} ignoré ({e})"),
                    }
                }
                if let Some(audio) = self.splitpop_audio.as_mut() {
                    if let Err(e) = audio.avancer(temps_us) {
                        log::debug!("pop : son ({e})");
                    }
                }
            }
            None => {
                // Plus de pop (ou pas de son) : le lecteur est libéré.
                if self.splitpop_audio.take().is_some() {
                    self.splitpop_audio_rel = None;
                }
            }
        }
    }

    /// Anneau audio du son du pop — le mixeur y puise le gain du pop.
    pub(crate) fn anneau_audio_pop(
        &self,
    ) -> Option<std::sync::Arc<std::sync::Mutex<crate::audio::live::AnneauAudio>>> {
        self.splitpop_audio.as_ref().map(|a| a.anneau_audio())
    }

    /// Dessine le pop — dans une passe ouverte.
    pub fn dessiner_splitpop(
        &mut self,
        etat: &EtatSplitPop,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        // Son du pop (`srPointsSon`) : ouvert au début de **chaque** pop,
        // avancé à chaque frame — le DOM fait un `new Audio(...)` par pop.
        self.synchroniser_son_splitpop(etat, temps_s);
        let Some(rendu) = etat.rendu() else {
            self.bloc_splitpop = None;
            return Ok(());
        };
        if rendu.opacite <= 0.0 {
            return Ok(());
        }
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let (x, y, lw, lh) = rendu.position;
        let (bx, by, bw, bh) = (x as f32, y as f32, lw as f32, lh as f32);
        let em = rendu.em;
        self.synchroniser_bloc_splitpop(etat.version(), &rendu, em)?;

        // Couche d'opacité + `scale(0.6 → 1)` autour du centre du squelette.
        let (cx, cy) = (bx + bw / 2.0, by + bh / 2.0);
        let echelle = rendu.echelle;
        let mtx = Matrix3x2 {
            M11: echelle,
            M12: 0.0,
            M21: 0.0,
            M22: echelle,
            M31: cx * (1.0 - echelle),
            M32: cy * (1.0 - echelle),
        };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: h,
            },
            geometricMask: std::mem::ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: rendu.opacite,
            opacityBrush: std::mem::ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer splitpop : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        unsafe { self.cible_rt.SetTransform(&mtx) };
        let resultat = self.peindre_splitpop(&rendu, (cx, cy), em);
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        unsafe { self.ctx.PopLayer() };
        unsafe { std::mem::ManuallyDrop::drop(&mut params.geometricMask) };
        resultat
    }

    /// Corps du pop : aura, badge, hero, mot-clé.
    ///
    /// `centre` = centre du squelette (le DOM centre la carte par `flex`).
    fn peindre_splitpop(
        &mut self,
        rendu: &RenduSplitPop<'_>,
        centre: (f32, f32),
        em: f32,
    ) -> Result<(), ErreurTexte> {
        let Some(bloc) = self.bloc_splitpop.as_ref() else {
            return Ok(());
        };
        let (carte_w, carte_h) = bloc.carte;
        // Carte centrée sur le squelette, puis animation d'impact (origine =
        // centre de la carte, comme le `transform` CSS de `.splitpop-carte`).
        let (ccx, ccy) = centre;
        let (angle, echelle) = impact_transform(rendu);
        let mtx = Matrix3x2::rotation(angle, ccx, ccy)
            * Matrix3x2 {
                M11: echelle,
                M12: 0.0,
                M21: 0.0,
                M22: echelle,
                M31: ccx * (1.0 - echelle),
                M32: ccy * (1.0 - echelle),
            };
        unsafe { self.cible_rt.SetTransform(&mtx) };

        let gauche = ccx - carte_w / 2.0;
        let haut = ccy - carte_h / 2.0;

        // `::before` — aura : halo sombre + teinte du résultat (dégradé radial
        // à la place du `blur(2px)`).
        let couleur = if rendu.pop.ok {
            rendu.couleur_ahead
        } else {
            rendu.couleur_behind
        };
        let aura = D2D_RECT_F {
            left: gauche - AURA_INSET_H_EM * em,
            top: haut - AURA_INSET_V_EM * em,
            right: gauche + carte_w + AURA_INSET_H_EM * em,
            bottom: haut + carte_h + AURA_INSET_V_EM * em,
        };
        let teinte = meler(couleur, AURA_TEINTE, [0, 0, 0, 0]);
        let brosse = self.pres_radiale_splitpop(
            &[
                (0.0, AURA_SOMBRE),
                (0.80, [AURA_SOMBRE[0], AURA_SOMBRE[1], AURA_SOMBRE[2], 0]),
                (1.0, [0, 0, 0, 0]),
            ],
            (ccx, ccy),
            ((aura.right - aura.left) / 2.0, (aura.bottom - aura.top) / 2.0),
        )?;
        unsafe { self.cible_rt.FillRectangle(&aura, &brosse) };
        let brosse = self.pres_radiale_splitpop(
            &[
                (0.0, teinte),
                (0.78, [teinte[0], teinte[1], teinte[2], 0]),
                (1.0, [0, 0, 0, 0]),
            ],
            (ccx, ccy),
            ((aura.right - aura.left) / 2.0, (aura.bottom - aura.top) / 2.0),
        )?;
        unsafe { self.cible_rt.FillRectangle(&aura, &brosse) };

        // Colonne : badge → hero → mot, `gap 0.08em`, centrée.
        let gap = CARTE_GAP_EM * em;
        let mut y = haut;
        if let Some(point) = bloc.point.as_ref() {
            // Pastille : `background: color-mix(couleur 26%, rgba(0,0,0,.72))`,
            // bord `color-mix(couleur 75%, transparent)`.
            let (pw, ph) = (point.largeur, point.hauteur);
            let px = ccx - pw / 2.0;
            let fond = meler(couleur, 0.26, [0, 0, 0, 184]);
            let bord = meler(couleur, 0.75, [0, 0, 0, 0]);
            let brosse: ID2D1SolidColorBrush = unsafe {
                self.cible_rt.CreateSolidColorBrush(&couleur_d2d(fond), None)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse badge split : {}", e.message())))?;
            let rayon = ph / 2.0;
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: px,
                            top: y,
                            right: px + pw,
                            bottom: y + ph,
                        },
                        radiusX: rayon,
                        radiusY: rayon,
                    },
                    &brosse,
                )
            };
            let brosse: ID2D1SolidColorBrush = unsafe {
                self.cible_rt.CreateSolidColorBrush(&couleur_d2d(bord), None)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse bord badge : {}", e.message())))?;
            unsafe {
                self.cible_rt.DrawRoundedRectangle(
                    &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: px + 0.5,
                            top: y + 0.5,
                            right: px + pw - 0.5,
                            bottom: y + ph - 0.5,
                        },
                        radiusX: rayon,
                        radiusY: rayon,
                    },
                    &brosse,
                    1.0,
                    None,
                )
            };
            // Texte du badge à `padding-left: 1.08em` (de la police du badge).
            let pad_g = POINT_PAD_D_EM * POINT_TAILLE_EM * em;
            let pad_v = POINT_PAD_V_EM * POINT_TAILLE_EM * em;
            self.dessiner_ligne_splitpop(point, px + pad_g, y + pad_v, [255, 255, 255, 255])?;
            y += ph + gap;
        }
        // Hero : delta (couleur du résultat, `skewX(-5deg)`, flash) ou, sans
        // delta, le mot-clé en Anton.
        let hero_couleur = eclaircir(couleur, rendu.flash);
        if let Some(delta) = bloc.delta.as_ref() {
            let mtx_hero = Matrix3x2 {
                M11: 1.0,
                M12: 0.0,
                M21: DELTA_SKEW.to_radians().tan(),
                M22: 1.0,
                M31: 0.0,
                M32: 0.0,
            };
            unsafe { self.cible_rt.SetTransform(&(mtx_hero * mtx)) };
            self.dessiner_ligne_splitpop(delta, ccx - delta.largeur / 2.0, y, hero_couleur)?;
            unsafe { self.cible_rt.SetTransform(&mtx) };
            y += delta.hauteur + gap;
        }
        if let Some(mot) = bloc.mot.as_ref() {
            let couleur_mot = if bloc.delta.is_some() {
                [255, 255, 255, 255]
            } else {
                hero_couleur
            };
            self.dessiner_ligne_splitpop(mot, ccx - mot.largeur / 2.0, y, couleur_mot)?;
        }
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        Ok(())
    }

    /// `transform` de `.splitpop-carte` : impact (0,5 s) puis re-pop (0,45 s).
    fn synchroniser_bloc_splitpop(
        &mut self,
        version: u64,
        rendu: &RenduSplitPop<'_>,
        em: f32,
    ) -> Result<(), ErreurTexte> {
        let cle = cle_splitpop(version, (em * 10.0).round() as u32);
        if self.bloc_splitpop.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        self.bloc_splitpop = None;
        let pop = rendu.pop;
        // Badge : « SPLIT n » (`::before` + numéro).
        let point = self.ligne_splitpop(
            &format!("SPLIT {}", pop.index + 1),
            POINT_TAILLE_EM * em,
            DWRITE_FONT_WEIGHT_EXTRA_BOLD,
            POINT_ESPACEMENT_EM * em,
            POLICE_CARTE,
        )?;
        // Boîte de la pastille = texte + `padding .24em .8em .24em 1.08em`.
        let point = Ligne {
            largeur: point.largeur + (POINT_PAD_G_EM + POINT_PAD_D_EM) * POINT_TAILLE_EM * em,
            hauteur: point.hauteur.max(POINT_TAILLE_EM * em)
                + 2.0 * POINT_PAD_V_EM * POINT_TAILLE_EM * em,
            ..point
        };
        let delta = if pop.delta.trim().is_empty() {
            None
        } else {
            Some(self.ligne_splitpop(
                pop.delta.trim(),
                DELTA_TAILLE_EM * em,
                DWRITE_FONT_WEIGHT_NORMAL,
                0.02 * DELTA_TAILLE_EM * em,
                POLICE_HERO,
            )?)
        };
        let (mot_taille, mot_espacement, mot_police) = if delta.is_some() {
            (MOT_TAILLE_EM * em, MOT_ESPACEMENT_EM * em, POLICE_CARTE)
        } else {
            (MOT_HERO_TAILLE_EM * em, MOT_HERO_ESPACEMENT_EM * em, POLICE_HERO)
        };
        let mot = self.ligne_splitpop(
            mot_split(pop.ok, &pop.delta),
            mot_taille,
            if delta.is_some() {
                DWRITE_FONT_WEIGHT_EXTRA_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            },
            mot_espacement,
            mot_police,
        )?;
        // Carte : hauteur des enfants + gaps + padding.
        let mut h = 2.0 * CARTE_PAD_V_EM * em;
        let mut l = 0.0_f32;
        let mut premier = true;
        for ligne in [Some(&point), delta.as_ref(), Some(&mot)].into_iter().flatten() {
            if !premier {
                h += CARTE_GAP_EM * em;
            }
            premier = false;
            h += ligne.hauteur;
            l = l.max(ligne.largeur);
        }
        let carte = (l + 2.0 * CARTE_PAD_H_EM * em, h);
        self.bloc_splitpop = Some(BlocSplitPop {
            cle,
            point: Some(point),
            delta,
            mot: Some(mot),
            carte,
        });
        Ok(())
    }

    /// Met en page une ligne du pop (famille embarquée ou `Segoe UI`).
    fn ligne_splitpop(
        &self,
        texte: &str,
        taille: f32,
        poids: windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT,
        espacement: f32,
        police: &str,
    ) -> Result<Ligne, ErreurTexte> {
        let famille = super::famille_police(police).unwrap_or("Segoe UI");
        let famille: Vec<u16> = famille.encode_utf16().chain(std::iter::once(0)).collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        let format = unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(famille.as_ptr()),
                None,
                poids,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                taille,
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat splitpop : {}", e.message())))?;
        // ⚠️ Piège E3 : une ligne `CENTER` est centrée dans *sa boîte de mise
        // en page* (4096 px ici) — l'origine de dessin doit être
        // `centre − largeur / 2`, d'où `LEADING`.
        unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING) }
        .map_err(|e| ErreurTexte::Systeme(format!("alignement splitpop : {}", e.message())))?;
        unsafe {
            format.SetLineSpacing(
                DWRITE_LINE_SPACING_METHOD_UNIFORM,
                taille,
                taille * 0.8,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("interligne splitpop : {}", e.message())))?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        let layout = unsafe { self.dwrite.CreateTextLayout(&contenu, &format, 4096.0, 4096.0) }
            .map_err(|e| ErreurTexte::Systeme(format!("layout splitpop : {}", e.message())))?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap splitpop : {}", e.message())))?;
        if espacement != 0.0 {
            let layout1: windows::Win32::Graphics::DirectWrite::IDWriteTextLayout1 = layout
                .cast()
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("IDWriteTextLayout1 splitpop : {}", e.message()))
                })?;
            unsafe {
                layout1.SetCharacterSpacing(
                    0.0,
                    espacement,
                    0.0,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: contenu.len() as u32,
                    },
                )
            }
            .map_err(|e| ErreurTexte::Systeme(format!("espacement splitpop : {}", e.message())))?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques splitpop : {}", e.message())))?;
        Ok(Ligne {
            layout,
            _format: format,
            largeur: metriques.widthIncludingTrailingWhitespace,
            hauteur: metriques.height,
        })
    }

    fn dessiner_ligne_splitpop(
        &self,
        ligne: &Ligne,
        x: f32,
        y: f32,
        couleur: [u8; 4],
    ) -> Result<(), ErreurTexte> {
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse splitpop : {}", e.message())))?;
        unsafe {
            self.ctx.DrawTextLayout(
                D2D_POINT_2F { x, y },
                &ligne.layout,
                &brosse,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            )
        };
        Ok(())
    }

    fn pres_radiale_splitpop(
        &self,
        stops: &[(f32, [u8; 4])],
        centre: (f32, f32),
        rayons: (f32, f32),
    ) -> Result<ID2D1Brush, ErreurTexte> {
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
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé splitpop : {}", e.message())))?;
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: D2D_POINT_2F {
                x: centre.0,
                y: centre.1,
            },
            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
            radiusX: rayons.0.max(1.0),
            radiusY: rayons.1.max(1.0),
        };
        let brosse = unsafe { self.cible_rt.CreateRadialGradientBrush(&props, None, &collection) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse radiale splitpop : {}", e.message())))?;
        Ok(brosse.into())
    }
}

/// Rotation et échelle de `.splitpop-carte` à l'instant courant.
fn impact_transform(rendu: &RenduSplitPop<'_>) -> (f32, f32) {
    let (mut angle, mut echelle) = impact(rendu.impact);
    if let Some(k) = rendu.repop {
        let e = bezier_css(REPOP_EASE, k);
        // 1 → 1,12 rotate(-1.5deg) → 0,97 → 1.
        let (a, s) = if e < 0.3 {
            let t = e / 0.3;
            (-1.5 * t, 1.0 + 0.12 * t)
        } else if e < 0.6 {
            let t = (e - 0.3) / 0.3;
            (-1.5 * (1.0 - t), 1.12 - 0.15 * t)
        } else {
            let t = (e - 0.6) / 0.4;
            (-1.5 * (1.0 - t) * 0.0, 0.97 + 0.03 * t)
        };
        angle += a;
        echelle *= s;
    }
    (angle, echelle)
}

/// `@keyframes splitpop-impact` : `scale(.15) rotate(-9deg)` → `1.22
/// rotate(2.5deg)` (55 %) → `0.95 rotate(-1deg)` (78 %) → `1` (100 %).
fn impact(k: f32) -> (f32, f32) {
    let e = bezier_css(IMPACT_EASE, k.clamp(0.0, 1.0));
    let (a0, s0) = (-9.0_f32, 0.15_f32);
    let (a1, s1) = (2.5, 1.22);
    let (a2, s2) = (-1.0, 0.95);
    let (a3, s3) = (0.0, 1.0);
    if e <= 0.55 {
        let t = e / 0.55;
        (a0 + (a1 - a0) * t, s0 + (s1 - s0) * t)
    } else if e <= 0.78 {
        let t = (e - 0.55) / 0.23;
        (a1 + (a2 - a1) * t, s1 + (s2 - s1) * t)
    } else {
        let t = (e - 0.78) / 0.22;
        (a2 + (a3 - a2) * t, s2 + (s3 - s2) * t)
    }
}

/// `filter: brightness(2.1) saturate(1.6)` approché : la couleur est tirée
/// vers le blanc (le `flash` vaut 1 au début, 0 à 45 % de l'impact).
fn eclaircir(c: [u8; 4], flash: f32) -> [u8; 4] {
    let k = (flash.clamp(0.0, 1.0) * 0.65).min(1.0);
    let f = |v: u8| (v as f32 + (255.0 - v as f32) * k).round() as u8;
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn pop(index: i32, ok: bool, delta: &str) -> MajSplitPop {
        MajSplitPop::Afficher {
            pop: PopSplit {
                index,
                ok,
                delta: delta.to_string(),
                signature: format!("split:{index}:{delta}"),
            },
            couleur_ahead: [0x22, 0xc5, 0x5e, 255],
            couleur_behind: [0xc0, 0x39, 0x2b, 255],
            duree_s: 2,
            son: None,
            volume: 1.0,
        }
    }

    #[test]
    fn son_du_pop_une_version_par_pop_et_gain_suivi() {
        let mut etat = EtatSplitPop::nouveau();
        let t0 = Instant::now();
        // Sans pop : rien à jouer, gain nul.
        assert!(etat.son().is_none());
        assert_eq!(etat.gain_son_milliemes(), 0);

        let avec_son = |index: i32, son: Option<&str>, volume: f32| MajSplitPop::Afficher {
            pop: PopSplit {
                index,
                ok: true,
                delta: "+0.10".to_string(),
                signature: format!("split:{index}:1.00"),
            },
            couleur_ahead: [0x22, 0xc5, 0x5e, 255],
            couleur_behind: [0xc0, 0x39, 0x2b, 255],
            duree_s: 2,
            son: son.map(|s| s.to_string()),
            volume,
        };

        etat.appliquer_at(avec_son(1, Some("medias/pop.wav"), 0.5), t0);
        let (rel, volume, v1) = etat.son().expect("son du pop");
        assert_eq!(rel, "medias/pop.wav");
        assert_eq!(volume, 0.5);
        assert_eq!(etat.gain_son_milliemes(), 500);

        // Doublon (même signature < 1,5 s) : ni nouveau pop, ni rejeu du son.
        etat.appliquer_at(avec_son(1, Some("medias/pop.wav"), 0.5), t0);
        assert_eq!(etat.son().expect("son").2, v1, "pas de rejeu sur doublon");

        // Pop suivant : version différente → le rendu rouvre le fichier.
        etat.appliquer_at(
            avec_son(2, Some("medias/pop.wav"), 1.0),
            t0 + Duration::from_secs(3),
        );
        assert_ne!(etat.son().expect("son").2, v1);
        assert_eq!(etat.gain_son_milliemes(), 1000);

        // Pop sans son : gain nul, rien à jouer.
        etat.appliquer_at(
            avec_son(3, None, 1.0),
            t0 + Duration::from_secs(6),
        );
        assert!(etat.son().is_none());
        assert_eq!(etat.gain_son_milliemes(), 0);
    }

    #[test]
    fn pop_entre_puis_sort_apres_la_duree() {        let mut etat = EtatSplitPop::nouveau();
        assert_eq!(etat.position(), POSITION_DEFAUT);
        assert!(etat.rendu().is_none());

        let t0 = Instant::now();
        etat.appliquer_at(pop(3, true, "+1.23"), t0);
        assert_eq!(etat.version(), 1);
        let depart = etat.rendu_at(t0).expect("visible");
        assert!(depart.opacite.abs() < 1e-3, "opacity 0 au départ");
        assert!((depart.echelle - POP_DEPART).abs() < 1e-3, "scale(0.6)");
        assert_eq!(depart.pop.index, 3);
        assert!((depart.em - 48.0).abs() < 0.01, "max(18, 120 × 0,4)");

        let stable = etat.rendu_at(t0 + Duration::from_millis(500)).expect("stable");
        assert!((stable.opacite - 1.0).abs() < 1e-3);
        assert!((stable.echelle - 1.0).abs() < 1e-3);
        assert!((stable.impact - 1.0).abs() < 1e-3, "impact terminé");

        // Fondu de sortie 0,3 s après `duree_s`, nœud retiré 350 ms plus tard.
        let sortie = etat.rendu_at(t0 + Duration::from_millis(2100)).expect("fondu");
        assert!(sortie.opacite < 1.0);
        assert!(etat.rendu_at(t0 + Duration::from_millis(2400)).is_none());
    }

    #[test]
    fn dedup_de_la_meme_signature_sous_1_5_s() {
        let mut etat = EtatSplitPop::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(pop(3, true, "+1.23"), t0);
        assert_eq!(etat.version(), 1);
        // Même signature < 1,5 s → ignorée (dédup multi-widget).
        etat.appliquer_at(pop(3, true, "+1.23"), t0 + Duration::from_millis(800));
        assert_eq!(etat.version(), 1);
        // Après 1,5 s, elle repasse.
        etat.appliquer_at(pop(3, true, "+1.23"), t0 + Duration::from_millis(1600));
        assert_eq!(etat.version(), 2);
    }

    #[test]
    fn re_pop_a_deux_et_quatre_secondes_selon_la_duree() {
        let mut etat = EtatSplitPop::nouveau();
        let t0 = Instant::now();
        // Durée 2 s : pas de re-pop (2 + 0,6 > 2).
        etat.appliquer_at(pop(1, true, "+0.10"), t0);
        assert!(etat
            .rendu_at(t0 + Duration::from_millis(1500))
            .expect("visible")
            .repop
            .is_none());
        // Durée 6 s : re-pop à 2 s puis 4 s.
        let mut long = EtatSplitPop::nouveau();
        long.appliquer_at(
            MajSplitPop::Afficher {
                pop: PopSplit {
                    index: 2,
                    ok: false,
                    delta: "-0.50".to_string(),
                    signature: "skip:2".to_string(),
                },
                couleur_ahead: [0x22, 0xc5, 0x5e, 255],
                couleur_behind: [0xc0, 0x39, 0x2b, 255],
                duree_s: 6,
                son: None,
                volume: 1.0,
            },
            t0,
        );
        assert!(long
            .rendu_at(t0 + Duration::from_millis(2200))
            .expect("visible")
            .repop
            .is_some());
        assert!(long
            .rendu_at(t0 + Duration::from_millis(4300))
            .expect("visible")
            .repop
            .is_some());
        assert!(long
            .rendu_at(t0 + Duration::from_millis(3600))
            .expect("visible")
            .repop
            .is_none());
    }

    #[test]
    fn config_position_et_taille_de_police() {
        let mut etat = EtatSplitPop::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(
            MajSplitPop::Config {
                x: 100.0,
                y: 200.0,
                largeur: 500.0,
                hauteur: 50.0,
            },
            t0,
        );
        assert_eq!(etat.version(), 1);
        assert_eq!(etat.position(), (100.0, 200.0, 500.0, 50.0));
        // 50 × 0,4 = 20 px ; la borne basse est 18.
        assert!((etat.em() - 20.0).abs() < 0.01);
        etat.appliquer_at(
            MajSplitPop::Config {
                x: 0.0,
                y: 0.0,
                largeur: 10.0,
                hauteur: 10.0,
            },
            t0,
        );
        assert!((etat.em() - 4.0).abs() < 0.01, "ratio brut");
        // Identique → aucune version.
        etat.appliquer_at(
            MajSplitPop::Config {
                x: 0.0,
                y: 0.0,
                largeur: 10.0,
                hauteur: 10.0,
            },
            t0,
        );
        assert_eq!(etat.version(), 2);
    }

    #[test]
    fn cacher_et_mots_du_dom() {
        let mut etat = EtatSplitPop::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(pop(0, true, ""), t0);
        assert!(etat.rendu_at(t0).is_some());
        etat.appliquer_at(MajSplitPop::Cacher, t0);
        assert!(etat.rendu_at(t0).is_none());
        // `Cacher` idempotent.
        etat.appliquer_at(MajSplitPop::Cacher, t0);
        assert_eq!(etat.version(), 2);

        assert_eq!(mot_split(true, "+1.23"), "GAGNÉ");
        assert_eq!(mot_split(false, "-0.45"), "PERDU");
        assert_eq!(mot_split(true, ""), "VALIDÉ");
        assert_eq!(mot_split(false, ""), "SKIP");
    }

    #[test]
    fn melange_css_et_eclaircissement() {
        assert_eq!(meler([255, 0, 0, 255], 0.5, [0, 0, 255, 255]), [128, 0, 128, 255]);
        assert_eq!(meler([10, 20, 30, 255], 1.0, [0, 0, 0, 0]), [10, 20, 30, 255]);
        // Flash plein → couleur tirée vers le blanc ; flash nul → inchangée.
        assert_eq!(eclaircir([0, 0, 0, 255], 0.0), [0, 0, 0, 255]);
        let clair = eclaircir([0, 0, 0, 255], 1.0);
        assert!(clair[0] > 100, "éclairci : {clair:?}");
    }

    #[test]
    fn impact_commence_ecrase_et_finit_droit() {
        let (a, s) = impact(0.0);
        assert!((a + 9.0).abs() < 1e-3, "rotate(-9deg)");
        assert!((s - 0.15).abs() < 1e-3, "scale(.15)");
        let (a, s) = impact(1.0);
        assert!(a.abs() < 1e-3);
        assert!((s - 1.0).abs() < 1e-3);
        // Overshoot au milieu.
        assert!(impact(0.5).1 > 1.0);
    }
}
