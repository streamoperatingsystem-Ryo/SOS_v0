//! Widget « musique » natif — mini-lecteur Winamp (barre titre + LCD
//! défilant + 16 barres d'égaliseur + progression crantée).
//!
//! Miroir de `.widget.musique` / `majMusiqueWidget` / `eqTick` /
//! `majMusiqueEq` (diffusion.html) :
//! - `.mu-titre` : barre « WINAMP » à dégradé bleu nuit (90°) ;
//! - `.mu-lcd` : boîte LED temps (`m:ss` + reste ` -m:ss`) + panneau LCD
//!   vert à texte défilant (« ♪ titre — demandeur », boucle sans saut) ;
//! - `.mu-eq` : 16 colonnes de barres dégradées (rouge → orange → vert)
//!   avec points de crête orange ;
//! - `.mu-posbar` : piste crantée (traits 1 px / 10 px) + remplissage
//!   dégradé vertical ;
//! - `musiqueMode === "eq"` : seule l'égaliseur occupe le widget.
//!
//! L'état est **live** : l'app pousse les snapshots `musique:update`
//! (`en_cours` + `progression {position_ms, pausee, duree_s}` de
//! `MusiqueState::etat`) dans [`EtatMusique`] pendant la session, comme
//! `EtatSpeedrun` — position figée à l'emit puis avancée en local par le
//! rendu (le tick 250 ms du DOM), jamais de reconfiguration de scène.
//!
//! Limites assumées du rendu natif :
//! - **aucune lecture audio** : l'`<audio>` vivait dans le navigateur OBS.
//!   En natif le widget affiche l'état de `MusiqueState` mais rien ne joue —
//!   et la file n'avance pas sans `musique-ended` remontée par le
//!   navigateur. La position est donc **plafonnée à la durée connue** (un
//!   `<audio>` arrêté en fin de piste se fige pareil) ;
//! - bandes EQ **simulées** : miroir exact de la branche `else` de
//!   `eqTick` (sinus + lissage), le mode dégradé que le DOM utilise quand
//!   Web Audio est indisponible. Un spectre réel exige la lecture ;
//! - `text-shadow` (lueur verte) non porté — le texte est plat ;
//! - `border: inset/outset` ≈ traits plats, comme le voile déjà rendu ;
//! - la durée n'est connue qu'après `musique-meta` : jusque-là le reste
//!   ` -m:ss` est masqué et le remplissage reste à 0 (parité DOM).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::core::{Interface, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Geometry, ID2D1GradientStopCollection, ID2D1LinearGradientBrush,
    ID2D1PathGeometry, ID2D1RenderTarget, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::cadres::CadreCfg;

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

/// Colonnes de l'égaliseur (`NB_BANDES` de diffusion.html — le widget
/// n'utilise pas les 24 bandes du module égaliseur plein écran).
const NB_BANDES: usize = 16;
/// Pas de lissage EQ du DOM (~66 ms par tick `eqTick`).
const PAS_EQ_S: f32 = 0.066;
/// Chute des points de crête par tick (66 ms × 11 ≈ 1,5 s de retombée).
const CHUTE_PEAK: f32 = 11.0;

/// Vert LCD du lecteur (`color` de `.widget.musique`).
const VERT: [u8; 4] = [0x4a, 0xff, 0x4a, 255];
/// Vert du reste ` -m:ss` (`#2ea02e`).
const VERT_SOMBRE: [u8; 4] = [0x2e, 0xa0, 0x2e, 255];
/// Fond de la boîte LED temps (`#050805`).
const FOND_TEMPS: [u8; 4] = [0x05, 0x08, 0x05, 255];
/// Fond LCD et de l'égaliseur (`#0a140a`).
const FOND_LCD: [u8; 4] = [0x0a, 0x14, 0x0a, 255];
/// Fond de la barre de progression (`#101410`).
const FOND_BARRE: [u8; 4] = [0x10, 0x14, 0x10, 255];
/// Trait des panneaux inset (`#444`) — approximation plate, comme le
/// `outset #555` du voile (`StyleBoite::Musique`).
const BORD_PANNEAU: [u8; 4] = [0x44, 0x44, 0x44, 255];
/// Bord de la barre titre (`#111`).
const BORD_TITRE: [u8; 4] = [0x11, 0x11, 0x11, 255];
/// Texte « WINAMP » (`#d5d6ff`).
const TEXTE_TITRE: [u8; 4] = [0xd5, 0xd6, 0xff, 255];
/// Point de crête (`#ff9c40`).
const PEAK: [u8; 4] = [0xff, 0x9c, 0x40, 255];
/// Police du lecteur (`font-family: "Lucida Console", Consolas, …` —
/// Consolas est la monospace système équivalente).
const FAMILLE_MU: &str = "Consolas";

// =============================================================================
// ÉTAT LIVE — miroir de `musiqueCourant` + `musiqueAudio.currentTime/duration`,
// alimenté par les snapshots `musique:update` (MusiqueState::etat).
// =============================================================================

/// Piste en cours — sous-ensemble de `MusiqueItem` utile au rendu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piste {
    /// Id de la demande (dedup des snapshots identiques).
    pub id: String,
    /// Titre affiché (nom de fichier nettoyé).
    pub titre: String,
    /// Pseudo du demandeur (« streamer »/« test » pour les voies locales).
    pub demandeur: String,
}

/// Snapshot poussé d'un event `musique:update` — `en_cours` + `progression`.
#[derive(Debug, Clone, PartialEq)]
pub struct MajMusique {
    /// Piste en cours (`None` = lecteur à l'arrêt — marquee « ♪ — »).
    pub en_cours: Option<Piste>,
    /// Position figée à l'emit, en ms — avancée en local quand `!pausee`.
    pub position_ms: u64,
    /// Lecture en pause côté domaine.
    pub pausee: bool,
    /// Durée connue de la piste (None avant `musique-meta`).
    pub duree_s: Option<f64>,
}

/// État live du lecteur — position figée à l'emit + avance locale.
///
/// `position_s` reproduit `musiqueAudio.currentTime` : tant que la piste
/// n'est pas en pause, le compteur avance depuis l'horodatage de la
/// dernière maj (le tick 250 ms du DOM, porté à la frame). L'avance est
/// plafonnée à `duree_s` : en natif rien ne joue ni ne remonte
/// `musique-ended` — l'affichage se fige en fin de piste comme le ferait
/// un `<audio>` arrêté.
#[derive(Debug)]
pub struct EtatMusique {
    en_cours: Option<Piste>,
    position_ms: u64,
    pausee: bool,
    duree_s: Option<f64>,
    /// Réception du dernier snapshot — base de l'avance locale.
    horodatage: Instant,
    version: u64,
}

impl Default for EtatMusique {
    fn default() -> Self {
        Self {
            en_cours: None,
            position_ms: 0,
            pausee: false,
            duree_s: None,
            horodatage: Instant::now(),
            version: 0,
        }
    }
}

impl EtatMusique {
    pub fn nouveau() -> Self {
        Self::default()
    }

    /// Applique un snapshot ; `version` n'augmente que sur changement réel
    /// (dedup — le domaine émet l'état complet à chaque mutation).
    /// L'horodatage est ré-armé à chaque changement : c'est la base de
    /// l'avance locale (le `debut` de `LectureEnCours`, transporté).
    pub fn appliquer(&mut self, maj: MajMusique) {
        let identique = self.en_cours == maj.en_cours
            && self.position_ms == maj.position_ms
            && self.pausee == maj.pausee
            && self.duree_s == maj.duree_s;
        if identique {
            return;
        }
        self.en_cours = maj.en_cours;
        self.position_ms = maj.position_ms;
        self.pausee = maj.pausee;
        self.duree_s = maj.duree_s;
        self.horodatage = Instant::now();
        self.version = self.version.wrapping_add(1);
    }

    /// Compteur monotone (clé de cache du rendu).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Position courante en secondes — `musiqueAudio.currentTime` :
    /// avance locale si lecture, figée si pause, plafonnée à la durée.
    /// 0,0 sans piste (le DOM affiche alors « 0:00 »).
    pub fn position_s(&self) -> f64 {
        if self.en_cours.is_none() {
            return 0.0;
        }
        let mut s = self.position_ms as f64 / 1000.0;
        if !self.pausee {
            s += self.horodatage.elapsed().as_secs_f64();
        }
        if let Some(d) = self.duree_s {
            s = s.min(d);
        }
        s
    }

    /// `musiqueAudio.duration` — `None` tant que `musique-meta` manque.
    pub fn duree_s(&self) -> Option<f64> {
        self.duree_s
    }

    /// Piste en cours (`musiqueCourant`).
    pub fn piste(&self) -> Option<&Piste> {
        self.en_cours.as_ref()
    }

    /// `musiqueAudio && !musiqueAudio.paused` — pilote la simulation EQ :
    /// les barres respirent en lecture et retombent à 0 sinon.
    pub fn joue(&self) -> bool {
        self.en_cours.is_some() && !self.pausee
    }
}

// =============================================================================
// CONFIG — extraite du Widget (scène) par `musiques_depuis_scene`.
// =============================================================================

/// Mode d'affichage du widget (`musiqueMode`) : lecteur complet ou
/// égaliseur seul (`mu-eq-only`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModeMusique {
    /// LCD + égaliseur + progression (défaut, `musiqueMode` absent/autre).
    Player,
    /// Bandes de spectre seules (`musiqueMode: "eq"`).
    Eq,
}

/// Configuration d'un widget `type: "musique"`, extraite de la scène.
#[derive(Debug, Clone, PartialEq)]
pub struct MusiqueCfg {
    /// Id du widget.
    pub id: String,
    /// Rect du widget en px canvas.
    pub rect: (f64, f64, f64, f64),
    /// Ordre de composition (`w.z`).
    pub z: i32,
    /// `musiqueMode === "eq"` → [`ModeMusique::Eq`], sinon lecteur.
    pub mode: ModeMusique,
    /// Cadre de scène (`cadre_de_widget`) — clip du contenu, trait tracé
    /// par le calque cadre au même z (comme goal/speedrun).
    pub cadre: Option<CadreCfg>,
}

// =============================================================================
// GÉOMÉTRIE + TEXTES — fonctions pures (testables sans GPU).
// =============================================================================

/// `muFmt` (diffusion.html) : `m:ss`, `--:--` si non fini/négatif.
pub fn mu_fmt(s: f64) -> String {
    if !s.is_finite() || s < 0.0 {
        return "--:--".to_string();
    }
    format!("{}:{:02}", (s / 60.0).floor() as u64, (s % 60.0).floor() as u64)
}

/// Texte de la marquee — `musiqueCourant ? "♪ t — d" : "♪ —"`.
pub fn texte_marquee(etat: &EtatMusique) -> String {
    match etat.piste() {
        Some(p) => format!("♪ {} — {}", p.titre, p.demandeur),
        None => "♪ —".to_string(),
    }
}

/// Texte du reste — `" -" + muFmt(dur - cur)` quand la durée est connue,
/// chaîne vide sinon (le DOM n'écrit rien dans `muRestEl`).
fn texte_reste(etat: &EtatMusique) -> String {
    match etat.duree_s() {
        Some(d) => format!(" -{}", mu_fmt((d - etat.position_s()).max(0.0))),
        None => String::new(),
    }
}

/// Remplissage de la barre 0–100 — 0 sans durée finie (parité
/// `muFillEl.style.width` : `isFinite(dur) && dur > 0` exigés).
fn remplissage(etat: &EtatMusique) -> f32 {
    match etat.duree_s() {
        Some(d) if d > 0.0 => ((etat.position_s() / d) * 100.0).min(100.0) as f32,
        _ => 0.0,
    }
}

/// Durée du cycle de la marquee (`animationDuration = max(8, len·0.22)`).
fn duree_defil(texte: &str) -> f32 {
    (texte.chars().count() as f32 * 0.22).max(8.0)
}

/// Cible d'une bande EQ — branche simulée de `eqTick` (Web Audio absent) :
/// sinus composés quand la piste joue, 0 sinon.
fn cible_eq(k: usize, t: f32, joue: bool) -> f32 {
    if !joue {
        return 0.0;
    }
    let k = k as f32;
    60.0 + 140.0
        * (t * (0.7 + k * 0.13) + k).sin().abs()
        * (0.55 + 0.45 * (t * 2.3 + k * 2.0).sin())
}

/// Un pas de lissage EQ (`eqTick`, 66 ms) : montée/descente amortie des
/// bandes + retombée des crêtes.
fn tick_eq(bloc: &mut BlocMusique, joue: bool) {
    for k in 0..NB_BANDES {
        let cible = cible_eq(k, bloc.sim_t, joue);
        bloc.bandes[k] += (cible - bloc.bandes[k]) * if joue { 0.35 } else { 0.2 };
        bloc.peaks[k] = (bloc.peaks[k] - CHUTE_PEAK).max(bloc.bandes[k]);
    }
    bloc.sim_t += PAS_EQ_S;
}

// =============================================================================
// DESSIN
// =============================================================================

/// Un texte mis en page du widget (même motif que `LigneGoal`) + sa
/// couleur (un pinceau D2D ne peut pas vivre dans le bloc : il est lié au
/// render target — la couleur est donc conservée et la brosse recréée au
/// dessin).
struct LigneMu {
    texte: String,
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    couleur: [u8; 4],
    w: f32,
    h: f32,
    base: f32,
}

/// Contenus résolus d'un widget pour une frame — les slots se
/// resynchronisent par comparaison de chaîne (compteur 1 Hz max, marquee
/// au changement de piste).
struct TextesMu {
    titre: String,
    pos: String,
    rest: String,
    marquee: String,
}

#[derive(Clone, Copy)]
enum SlotMu {
    Titre,
    Pos,
    Rest,
    Marquee,
}

/// Bloc par widget : layouts cachés + état lissé de l'égaliseur +
/// origine du défilement.
#[derive(Default)]
pub(super) struct BlocMusique {
    /// « WINAMP » de la barre titre.
    titre: Option<LigneMu>,
    /// Compteur `m:ss` de la boîte LED.
    pos: Option<LigneMu>,
    /// Reste ` -m:ss` (petit, vert sombre).
    rest: Option<LigneMu>,
    /// Texte défilant du LCD.
    marquee: Option<LigneMu>,
    /// Bandes EQ lissées (0–255), miroir de `eqBands`.
    bandes: [f32; NB_BANDES],
    /// Crêtes lissées, miroir de `eqPeaks`.
    peaks: [f32; NB_BANDES],
    /// Temps simulé (argument des sinus de `cible_eq`).
    sim_t: f32,
    /// Reliquat de temps vers le prochain pas de 66 ms.
    eq_acc: f32,
    /// Dernier tick EQ réel (dt entre deux frames).
    tick_eq: Option<Instant>,
    /// Départ du cycle de la marquee (ré-armé au changement de texte).
    marquee_depart: Option<Instant>,
}

/// Clé de cache d'un bloc : id + hauteur (tailles dérivées de `h`) + mode.
fn cle_bloc(cfg: &MusiqueCfg) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cfg.id.hash(&mut h);
    (cfg.rect.3 as f32).to_bits().hash(&mut h);
    cfg.mode.hash(&mut h);
    h.finish()
}

impl DessinateurTexte {
    /// Dessine un widget musique — dans une passe ouverte.
    ///
    /// Même motif de clips que `dessiner_goal` : rognage au rect
    /// (`overflow` du widget DOM) + couche masquée au polygone du cadre.
    pub fn dessiner_musique(
        &mut self,
        cfg: &MusiqueCfg,
        etat: &EtatMusique,
        clip: Option<&ID2D1PathGeometry>,
    ) -> Result<(), ErreurTexte> {
        let (x, y, w, h) = cfg.rect;
        let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
        if w <= 0.0 || h <= 0.0 {
            return Ok(());
        }
        let zone = D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        unsafe {
            self.cible_rt
                .PushAxisAlignedClip(&zone, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
        };
        let masque: Option<ID2D1Geometry> = match clip {
            Some(geo) => Some(
                geo.cast()
                    .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))?,
            ),
            None => None,
        };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: zone,
            geometricMask: ManuallyDrop::new(masque),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let avec_couche = params.geometricMask.is_some();
        if avec_couche {
            let layer = unsafe { self.cible_rt.CreateLayer(None) }
                .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer musique : {}", e.message())))?;
            unsafe { self.ctx.PushLayer(&params, &layer) };
        }
        let resultat = self.dessiner_musique_interne(cfg, etat, &zone);
        if avec_couche {
            unsafe { self.ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        resultat
    }

    /// Contenu du widget — sous les clips déjà posés.
    fn dessiner_musique_interne(
        &mut self,
        cfg: &MusiqueCfg,
        etat: &EtatMusique,
        zone: &D2D_RECT_F,
    ) -> Result<(), ErreurTexte> {
        let (zx, zy, zw, zh) = (zone.left, zone.top, zone.right - zone.left, zone.bottom - zone.top);
        let textes = TextesMu {
            titre: "WINAMP".to_string(),
            pos: mu_fmt(etat.position_s()),
            rest: texte_reste(etat),
            marquee: texte_marquee(etat),
        };
        self.synchroniser_bloc_musique(cfg, &textes, etat.joue())?;
        let cle = cle_bloc(cfg);
        let Some(bloc) = self.blocs_musique.get(&cle) else {
            return Ok(());
        };

        // --- Métriques (`calc(var(--mu-*) * N)` du CSS) -------------------
        let pad_x = 0.04 * zw;
        let pad_y = 0.07 * zh;
        let gap = 0.06 * zh;
        let fs_lcd = 0.14 * zh;
        let h_titre = (0.11 * zh).max(8.0);
        let h_eq = (0.30 * zh).max(14.0);
        let h_posbar = (0.07 * zh).max(5.0);
        // Panneaux LCD : padding vertical 0,05·h + bord 2 px de chaque côté,
        // hauteur = contenu texte (les deux boîtes partagent la taille).
        let h_lcd = fs_lcd * 1.2 + 0.10 * zh + 4.0;
        let inner_x = zx + pad_x;
        let inner_w = zw - 2.0 * pad_x;
        let inner_h = zh - 2.0 * pad_y;

        let rt = &self.cible_rt;
        let brosse = |c: [u8; 4], nom: &str| -> Result<ID2D1Brush, ErreurTexte> {
            unsafe { rt.CreateSolidColorBrush(&couleur_d2d(c), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("{nom} : {}", e.message())))
                .map(|b| b.into())
        };

        if cfg.mode == ModeMusique::Eq {
            // `mu-eq-only` : l'égaliseur remplit tout l'intérieur paddé.
            let eq_rect = D2D_RECT_F {
                left: inner_x,
                top: zy + pad_y,
                right: inner_x + inner_w,
                bottom: zy + pad_y + inner_h,
            };
            return dessiner_eq(rt, &eq_rect, bloc);
        }

        // `justify-content: center` : la pile de rangées est centrée dans
        // la zone paddée.
        let total = h_titre + h_lcd + h_eq + h_posbar + 3.0 * gap;
        let mut cy = zy + pad_y + ((inner_h - total).max(0.0)) / 2.0;

        // --- `.mu-titre` : dégradé bleu nuit + « WINAMP » italique --------
        let titre_rect = D2D_RECT_F {
            left: inner_x,
            top: cy,
            right: inner_x + inner_w,
            bottom: cy + h_titre,
        };
        let fond_titre = degrade_titre(&titre_rect, rt)?;
        peindre_panneau(rt, &titre_rect, BORD_TITRE, 1.0, &fond_titre)?;
        if let Some(l) = &bloc.titre {
            dessiner_ligne_mu(rt, titre_rect.left + 6.0, cy + (h_titre - l.h) / 2.0, l)?;
        }
        cy += h_titre + gap;

        // --- `.mu-lcd` : boîte LED + marquee -------------------------------
        let h_panneau = h_lcd;
        // `.mu-temps` : largeur = contenu + padding 0,45em des deux côtés.
        let pad_temps = 0.45 * fs_lcd;
        let temps_w = bloc.pos.as_ref().map(|l| l.w).unwrap_or(0.0)
            + bloc.rest.as_ref().map(|l| l.w).unwrap_or(0.0)
            + 2.0 * pad_temps
            + 4.0;
        let temps_rect = D2D_RECT_F {
            left: inner_x,
            top: cy,
            right: inner_x + temps_w,
            bottom: cy + h_panneau,
        };
        let b_temps = brosse(FOND_TEMPS, "brosse led musique")?;
        peindre_panneau(rt, &temps_rect, BORD_PANNEAU, 2.0, &b_temps)?;
        let mut tx = temps_rect.left + pad_temps + 2.0;
        let texte_y = cy + (h_panneau - bloc.pos.as_ref().map(|l| l.h).unwrap_or(0.0)) / 2.0;
        if let Some(l) = &bloc.pos {
            dessiner_ligne_mu(rt, tx, texte_y, l)?;
            tx += l.w;
        }
        if let Some(l) = &bloc.rest {
            // Alignée sur la ligne de base du compteur.
            let top = bloc
                .pos
                .as_ref()
                .map(|p| texte_y + p.base - l.base)
                .unwrap_or(texte_y);
            dessiner_ligne_mu(rt, tx, top, l)?;
        }
        // `.mu-defile-wrap` : reste de la rangée (`flex: 1`, gap 0,02·w).
        let defile_x = temps_rect.right + 0.02 * zw;
        let defile_rect = D2D_RECT_F {
            left: defile_x,
            top: cy,
            right: inner_x + inner_w,
            bottom: cy + h_panneau,
        };
        if defile_rect.right > defile_rect.left {
            let b_lcd = brosse(FOND_LCD, "brosse lcd musique")?;
            peindre_panneau(rt, &defile_rect, BORD_PANNEAU, 2.0, &b_lcd)?;
            dessiner_marquee(rt, &defile_rect, bloc, fs_lcd)?;
        }
        cy += h_panneau + gap;

        // --- `.mu-eq` : 16 barres + crêtes ---------------------------------
        let eq_rect = D2D_RECT_F {
            left: inner_x,
            top: cy,
            right: inner_x + inner_w,
            bottom: cy + h_eq,
        };
        dessiner_eq(rt, &eq_rect, bloc)?;
        cy += h_eq + gap;

        // --- `.mu-posbar` : piste crantée + remplissage --------------------
        let barre_rect = D2D_RECT_F {
            left: inner_x,
            top: cy,
            right: inner_x + inner_w,
            bottom: cy + h_posbar,
        };
        let b_piste = brosse(FOND_BARRE, "brosse piste musique")?;
        unsafe {
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: barre_rect,
                    radiusX: 2.0,
                    radiusY: 2.0,
                },
                &b_piste,
            )
        };
        // Crans : traits sombres 1 px tous les 10 px (repeating-gradient).
        let cran = brosse([0, 0, 0, 140], "brosse cran musique")?;
        let mut cx = barre_rect.left + 9.0;
        while cx < barre_rect.right {
            unsafe {
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: cx,
                        top: barre_rect.top,
                        right: cx + 1.0,
                        bottom: barre_rect.bottom,
                    },
                    &cran,
                )
            };
            cx += 10.0;
        }
        let pct = remplissage(etat);
        if pct > 0.0 {
            let fill = D2D_RECT_F {
                left: barre_rect.left,
                top: barre_rect.top,
                right: barre_rect.left + (barre_rect.right - barre_rect.left) * pct / 100.0,
                bottom: barre_rect.bottom,
            };
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: couleur_d2d([0x7d, 0xff, 0x7d, 255]),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: couleur_d2d([0x2f, 0xd3, 0x2f, 255]),
                },
            ];
            let degrade = degrade_v(&fill, &stops, rt)?;
            unsafe { rt.FillRectangle(&fill, &degrade) };
        }
        tracer_bord(rt, &barre_rect, BORD_PANNEAU, 1.0)?;
        Ok(())
    }

    /// (Re)construit les lignes dont le contenu a changé et avance la
    /// simulation EQ du temps réel écoulé (pas de 66 ms, rattrapage borné —
    /// une pause longue n'empile pas des centaines de pas).
    fn synchroniser_bloc_musique(
        &mut self,
        cfg: &MusiqueCfg,
        textes: &TextesMu,
        joue: bool,
    ) -> Result<(), ErreurTexte> {
        let zh = cfg.rect.3 as f32;
        let fs_lcd = 0.14 * zh;
        let style_titre = StyleTexte {
            police: FAMILLE_MU.to_string(),
            taille: 0.08 * zh,
            gras: true,
            italique: true,
            espacement: 0.2 * 0.08 * zh,
            couleur: TEXTE_TITRE,
            contour_epaisseur: 0.0,
            ..StyleTexte::default()
        };
        let style_lcd = StyleTexte {
            taille: fs_lcd,
            italique: false,
            espacement: 0.04 * fs_lcd,
            couleur: VERT,
            ..style_titre.clone()
        };
        let style_rest = StyleTexte {
            taille: fs_lcd * 0.62,
            couleur: VERT_SOMBRE,
            ..style_lcd.clone()
        };

        // Table bornée : les clés ne divergent que par (widget, h, mode).
        if self.blocs_musique.len() > 32 {
            self.blocs_musique.clear();
        }
        let cle = cle_bloc(cfg);
        let mut maj: Vec<(SlotMu, StyleTexte, String)> = Vec::new();
        {
            let bloc = self.blocs_musique.entry(cle).or_default();
            // EQ : rattrapage du temps réel en pas de 66 ms (borné à ~0,5 s —
            // au-delà la simulation aurait raté des ticks de toute façon).
            let dt = bloc
                .tick_eq
                .map(|t| t.elapsed().as_secs_f32())
                .unwrap_or(0.0)
                .min(0.5);
            bloc.tick_eq = Some(Instant::now());
            bloc.eq_acc += dt;
            let mut pas = 0;
            while bloc.eq_acc >= PAS_EQ_S && pas < 8 {
                tick_eq(bloc, joue);
                bloc.eq_acc -= PAS_EQ_S;
                pas += 1;
            }
            let mut changer = |slot: &mut Option<LigneMu>,
                               kind: SlotMu,
                               texte: Option<&str>,
                               style: &StyleTexte| {
                if slot.as_ref().map(|l| l.texte.as_str()) != texte {
                    *slot = None;
                    if let Some(t) = texte {
                        maj.push((kind, style.clone(), t.to_string()));
                    }
                }
            };
            changer(&mut bloc.titre, SlotMu::Titre, Some(&textes.titre), &style_titre);
            changer(&mut bloc.pos, SlotMu::Pos, Some(&textes.pos), &style_lcd);
            changer(
                &mut bloc.rest,
                SlotMu::Rest,
                (!textes.rest.is_empty()).then_some(textes.rest.as_str()),
                &style_rest,
            );
            if bloc
                .marquee
                .as_ref()
                .map(|l| l.texte.as_str())
                != Some(textes.marquee.as_str())
            {
                bloc.marquee = None;
                // Nouveau texte = nouveau cycle de défilement (la durée
                // dépend de sa longueur — l'animation CSS repart à 0).
                bloc.marquee_depart = Some(Instant::now());
                maj.push((SlotMu::Marquee, style_lcd.clone(), textes.marquee.clone()));
            }
        }
        for (slot, style, texte) in maj {
            let ligne = self.ligne_mu(&style, &texte)?;
            if let Some(bloc) = self.blocs_musique.get_mut(&cle) {
                match slot {
                    SlotMu::Titre => bloc.titre = Some(ligne),
                    SlotMu::Pos => bloc.pos = Some(ligne),
                    SlotMu::Rest => bloc.rest = Some(ligne),
                    SlotMu::Marquee => bloc.marquee = Some(ligne),
                }
            }
        }
        Ok(())
    }

    /// Met en page un texte du widget — même recette que `ligne_goal`,
    /// mais en **monospace système** (`Consolas`, pas la collection
    /// embarquée : le lecteur n'utilise pas les polices de titre).
    fn ligne_mu(&self, style: &StyleTexte, texte: &str) -> Result<LigneMu, ErreurTexte> {
        let format = self.format_mu(style)?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        if contenu.is_empty() {
            return Err(ErreurTexte::Systeme("ligne musique vide".to_string()));
        }
        let layout = unsafe { self.dwrite.CreateTextLayout(&contenu, &format, 8192.0, 2048.0) }
            .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout musique : {}", e.message())))?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap musique : {}", e.message())))?;
        if style.espacement != 0.0 {
            let layout1: IDWriteTextLayout1 = layout
                .cast()
                .map_err(|e| ErreurTexte::Systeme(format!("IDWriteTextLayout1 : {}", e.message())))?;
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
                ErreurTexte::Systeme(format!("SetCharacterSpacing musique : {}", e.message()))
            })?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques musique : {}", e.message())))?;
        let base = base_ligne_mu(&layout).unwrap_or(metriques.height * 0.8);
        Ok(LigneMu {
            texte: texte.to_string(),
            layout,
            _format: format,
            couleur: style.couleur,
            w: metriques.width,
            h: metriques.height,
            base,
        })
    }

    /// Format DirectWrite du lecteur : famille système `Consolas`
    /// (collection **par défaut**, pas les polices embarquées des titres).
    fn format_mu(&self, style: &StyleTexte) -> Result<IDWriteTextFormat, ErreurTexte> {
        let poids = if style.gras {
            DWRITE_FONT_WEIGHT_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        };
        let pente = if style.italique {
            DWRITE_FONT_STYLE_ITALIC
        } else {
            DWRITE_FONT_STYLE_NORMAL
        };
        let nom: Vec<u16> = style
            .police
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(nom.as_ptr()),
                None,
                poids,
                pente,
                DWRITE_FONT_STRETCH_NORMAL,
                style.taille.max(1.0),
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat(mu) : {}", e.message())))
    }
}

/// `.mu-eq` : panneau + 16 colonnes de barres dégradées + crêtes.
///
/// La barre monte du bas et son dégradé est relatif à SA propre hauteur
/// (le CSS pose le gradient sur `.mu-bar`, pas sur la colonne) : une
/// petite barre reste verte, une grande atteint le rouge.
fn dessiner_eq(rt: &ID2D1RenderTarget, rect: &D2D_RECT_F, bloc: &BlocMusique) -> Result<(), ErreurTexte> {
    let b_fond = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(FOND_LCD), None) }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse eq musique : {}", e.message())))?;
    peindre_panneau(rt, rect, BORD_PANNEAU, 2.0, &b_fond)?;
    // Padding CSS : 3 px vertical, 4 px horizontal ; gap 3 px.
    let x0 = rect.left + 4.0;
    let x1 = rect.right - 4.0;
    let y_bas = rect.bottom - 3.0;
    let hauteur = rect.bottom - rect.top - 6.0;
    if x1 <= x0 || hauteur <= 0.0 {
        return Ok(());
    }
    let col_w = (x1 - x0 - 3.0 * (NB_BANDES as f32 - 1.0)) / NB_BANDES as f32;
    if col_w <= 0.0 {
        return Ok(());
    }
    let b_peak = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(PEAK), None) }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse peak musique : {}", e.message())))?;
    for i in 0..NB_BANDES {
        let cx = x0 + i as f32 * (col_w + 3.0);
        // `height = max(4, round(band/255·100))%`, plancher CSS 2 px.
        let pct = (bloc.bandes[i] / 255.0 * 100.0).round().max(4.0);
        let h_bar = (hauteur * pct / 100.0).max(2.0);
        let bar = D2D_RECT_F {
            left: cx,
            top: y_bas - h_bar,
            right: cx + col_w,
            bottom: y_bas,
        };
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: couleur_d2d([0xff, 0x40, 0x40, 255]),
            },
            D2D1_GRADIENT_STOP {
                position: 0.30,
                color: couleur_d2d([0xff, 0xb0, 0x40, 255]),
            },
            D2D1_GRADIENT_STOP {
                position: 0.65,
                color: couleur_d2d(VERT),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: couleur_d2d(VERT),
            },
        ];
        let degrade = degrade_v(&bar, &stops, rt)?;
        unsafe { rt.FillRectangle(&bar, &degrade) };
        // Crête : visible tant qu'elle dépasse la barre (`peak > band+6`).
        if bloc.peaks[i] > bloc.bandes[i] + 6.0 {
            let pct_p = (bloc.peaks[i] / 255.0 * 100.0).round().min(100.0);
            let bas = y_bas - hauteur * pct_p / 100.0;
            unsafe {
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: cx,
                        top: bas - 2.0,
                        right: cx + col_w,
                        bottom: bas,
                    },
                    &b_peak,
                )
            };
        }
    }
    Ok(())
}

/// Marquee du LCD : texte défilant en boucle (`musiqueDefil`,
/// translateX −50 % sur la paire de spans — période = texte + 3 em).
fn dessiner_marquee(
    rt: &ID2D1RenderTarget,
    rect: &D2D_RECT_F,
    bloc: &BlocMusique,
    fs_lcd: f32,
) -> Result<(), ErreurTexte> {
    let Some(l) = &bloc.marquee else {
        return Ok(());
    };
    let interieur = D2D_RECT_F {
        left: rect.left + 0.5 * fs_lcd,
        top: rect.top,
        right: rect.right - 0.5 * fs_lcd,
        bottom: rect.bottom,
    };
    unsafe { rt.PushAxisAlignedClip(&interieur, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    let periode = l.w + 3.0 * fs_lcd;
    let t = bloc
        .marquee_depart
        .map(|d| d.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    let duree = duree_defil(&l.texte);
    let offset = (t % duree) / duree * periode;
    let y = rect.top + (rect.bottom - rect.top - l.h) / 2.0;
    let x = interieur.left - offset;
    dessiner_ligne_mu(rt, x, y, l)?;
    dessiner_ligne_mu(rt, x + periode, y, l)?;
    unsafe { rt.PopAxisAlignedClip() };
    Ok(())
}

/// Panneau arrondi : fond + bord plat (`inset` ≈ trait plein `#444`,
/// comme l'`outset #555` déjà aplati du voile `StyleBoite::Musique`).
fn peindre_panneau(
    rt: &ID2D1RenderTarget,
    rect: &D2D_RECT_F,
    bord: [u8; 4],
    epaisseur: f32,
    fond: &ID2D1Brush,
) -> Result<(), ErreurTexte> {
    unsafe {
        rt.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: *rect,
                radiusX: 2.0,
                radiusY: 2.0,
            },
            fond,
        )
    };
    tracer_bord(rt, rect, bord, epaisseur)
}

/// Bord plat centré sur le bord extérieur du panneau.
fn tracer_bord(
    rt: &ID2D1RenderTarget,
    rect: &D2D_RECT_F,
    couleur: [u8; 4],
    epaisseur: f32,
) -> Result<(), ErreurTexte> {
    let brosse = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse bord musique : {}", e.message())))?;
    let demi = epaisseur / 2.0;
    unsafe {
        rt.DrawRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: rect.left - demi,
                    top: rect.top - demi,
                    right: rect.right + demi,
                    bottom: rect.bottom + demi,
                },
                radiusX: 2.0,
                radiusY: 2.0,
            },
            &brosse,
            epaisseur,
            None,
        )
    };
    Ok(())
}

/// Une ligne — plat (la lueur `text-shadow` du DOM n'est pas portée).
/// La brosse de la couleur de la ligne est recréée à la volée : un pinceau
/// D2D ne peut pas vivre dans `LigneMu` (il est lié au render target).
fn dessiner_ligne_mu(
    rt: &ID2D1RenderTarget,
    x: f32,
    y: f32,
    ligne: &LigneMu,
) -> Result<(), ErreurTexte> {
    let brosse: ID2D1Brush = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(ligne.couleur), None) }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse ligne musique : {}", e.message())))?
        .into();
    unsafe {
        rt.DrawTextLayout(
            D2D_POINT_2F { x, y },
            &ligne.layout,
            &brosse,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }
    Ok(())
}

/// Ligne de base de la première ligne d'un layout (`None` si non mesurable).
fn base_ligne_mu(layout: &IDWriteTextLayout) -> Option<f32> {
    let mut metriques = [DWRITE_LINE_METRICS::default()];
    let mut lignes = 0u32;
    unsafe { layout.GetLineMetrics(Some(&mut metriques), &mut lignes) }.ok()?;
    Some(metriques[0].baseline)
}

/// Dégradé vertical (ou horizontal) d'un remplissage — stops CSS relatifs
/// au rect donné.
fn degrade_v(
    rect: &D2D_RECT_F,
    stops: &[D2D1_GRADIENT_STOP],
    rt: &ID2D1RenderTarget,
) -> Result<ID2D1Brush, ErreurTexte> {
    let collection: ID2D1GradientStopCollection = unsafe {
        rt.CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }
    .map_err(|e| ErreurTexte::Systeme(format!("stops musique : {}", e.message())))?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: D2D_POINT_2F {
            x: rect.left,
            y: rect.top,
        },
        endPoint: D2D_POINT_2F {
            x: rect.left,
            y: rect.bottom,
        },
    };
    let brosse: ID2D1LinearGradientBrush =
        unsafe { rt.CreateLinearGradientBrush(&props, None, &collection) }
            .map_err(|e| ErreurTexte::Systeme(format!("dégradé musique : {}", e.message())))?;
    Ok(brosse.into())
}

/// Dégradé 90° de la barre titre — `#000028 → #1d1d6e(45 %) →
/// #4a4ab8(62 %) → #000028` (stops CSS relatifs au rect).
fn degrade_titre(rect: &D2D_RECT_F, rt: &ID2D1RenderTarget) -> Result<ID2D1Brush, ErreurTexte> {
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: couleur_d2d([0x00, 0x00, 0x28, 255]),
        },
        D2D1_GRADIENT_STOP {
            position: 0.45,
            color: couleur_d2d([0x1d, 0x1d, 0x6e, 255]),
        },
        D2D1_GRADIENT_STOP {
            position: 0.62,
            color: couleur_d2d([0x4a, 0x4a, 0xb8, 255]),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: couleur_d2d([0x00, 0x00, 0x28, 255]),
        },
    ];
    let collection: ID2D1GradientStopCollection = unsafe {
        rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }
    .map_err(|e| ErreurTexte::Systeme(format!("stops titre musique : {}", e.message())))?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: D2D_POINT_2F {
            x: rect.left,
            y: rect.top,
        },
        endPoint: D2D_POINT_2F {
            x: rect.right,
            y: rect.top,
        },
    };
    let brosse: ID2D1LinearGradientBrush =
        unsafe { rt.CreateLinearGradientBrush(&props, None, &collection) }
            .map_err(|e| ErreurTexte::Systeme(format!("dégradé titre musique : {}", e.message())))?;
    Ok(brosse.into())
}

/// Cache des blocs musique — clé = [`cle_bloc`].
pub(super) type BlocsMusique = HashMap<u64, BlocMusique>;

#[cfg(test)]
mod tests {
    use super::*;

    fn etat_lecture() -> EtatMusique {
        let mut e = EtatMusique::nouveau();
        e.appliquer(MajMusique {
            en_cours: Some(Piste {
                id: "p1".into(),
                titre: "Morceau".into(),
                demandeur: "viewer".into(),
            }),
            position_ms: 2_000,
            pausee: false,
            duree_s: Some(200.0),
        });
        e
    }

    #[test]
    fn mu_fmt_formatte_et_borne() {
        assert_eq!(mu_fmt(0.0), "0:00");
        assert_eq!(mu_fmt(65.4), "1:05");
        assert_eq!(mu_fmt(600.0), "10:00");
        assert_eq!(mu_fmt(-1.0), "--:--");
        assert_eq!(mu_fmt(f64::NAN), "--:--");
    }

    #[test]
    fn texte_marquee_placeholders() {
        assert_eq!(texte_marquee(&EtatMusique::nouveau()), "♪ —");
        assert_eq!(texte_marquee(&etat_lecture()), "♪ Morceau — viewer");
    }

    #[test]
    fn position_figee_en_pause_et_avancee_en_lecture() {
        let mut e = etat_lecture();
        // Recule l'horodatage de 3 s : la position avance d'autant.
        e.horodatage = Instant::now() - std::time::Duration::from_secs(3);
        assert!((e.position_s() - 5.0).abs() < 1e-3);
        e.appliquer(MajMusique {
            en_cours: e.piste().cloned(),
            position_ms: 5_000,
            pausee: true,
            duree_s: Some(200.0),
        });
        e.horodatage = Instant::now() - std::time::Duration::from_secs(10);
        assert_eq!(e.position_s(), 5.0, "pause = position figée");
    }

    #[test]
    fn position_plafonnee_a_la_duree() {
        let mut e = etat_lecture();
        e.horodatage = Instant::now() - std::time::Duration::from_secs(1_000);
        assert_eq!(e.position_s(), 200.0, "rien ne joue : figé en fin de piste");
        assert_eq!(remplissage(&e), 100.0);
    }

    #[test]
    fn appliquer_dedup_et_version() {
        let mut e = EtatMusique::nouveau();
        let maj = MajMusique {
            en_cours: Some(Piste { id: "a".into(), titre: "T".into(), demandeur: "d".into() }),
            position_ms: 1_000,
            pausee: false,
            duree_s: None,
        };
        e.appliquer(maj.clone());
        let v = e.version();
        e.appliquer(maj);
        assert_eq!(e.version(), v, "snapshot identique = pas de version");
        e.appliquer(MajMusique {
            en_cours: None,
            position_ms: 0,
            pausee: false,
            duree_s: None,
        });
        assert_eq!(e.version(), v + 1);
    }

    #[test]
    fn reste_et_remplissage_sans_duree() {
        let mut e = EtatMusique::nouveau();
        e.appliquer(MajMusique {
            en_cours: Some(Piste { id: "a".into(), titre: "T".into(), demandeur: "d".into() }),
            position_ms: 5_000,
            pausee: true,
            duree_s: None,
        });
        assert_eq!(texte_reste(&e), "");
        assert_eq!(remplissage(&e), 0.0);
    }

    #[test]
    fn cible_eq_nulle_sans_lecture_et_bornee_en_lecture() {
        for k in 0..NB_BANDES {
            assert_eq!(cible_eq(k, 12.5, false), 0.0);
        }
        for k in 0..NB_BANDES {
            let c = cible_eq(k, 3.7, true);
            assert!((60.0..=200.0).contains(&c), "bande {k} hors plage : {c}");
        }
    }

    #[test]
    fn duree_defil_bornee_a_8s() {
        assert_eq!(duree_defil("♪ —"), 8.0);
        assert_eq!(duree_defil(&"x".repeat(100)), 22.0);
    }
}
