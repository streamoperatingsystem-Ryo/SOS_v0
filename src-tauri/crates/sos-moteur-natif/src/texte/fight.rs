//! Overlay « Emoji Arena » natif (`!fight`) — arène 1v1, z 9993.
//!
//! Miroir de `initFightOverlay` / `majFightState` / `fightTick` /
//! `fightAnimerCoup` (`diffusion.html` 2133-2550 + 12415-12845) : deux
//! combattants ancrés bas-gauche / bas-droite (barre de PV, emoji 128 px,
//! pseudo + plateforme, chips de coups, badge « À TOI », timer local), colonne
//! centrale (tour + timer de tour + temps de match), bannière (lobby / fin) et
//! toast d'info. La logique (tours, dégâts, bots, timers) reste **entièrement
//! côté app** : ce module ne fait que rendre et animer.
//!
//! Canal : frames `chat_tx` **`fight-state`** (payload `combat`), snapshot
//! COMPLET à chaque transition — auto-réparatrices, rejouables au resync
//! (`fight-resync`). ⚠️ L'event Tauri `fight:update` est l'état **dashboard** :
//! ce n'est jamais le canal du rendu.
//!
//! Les échéances (`reste_tour_ms`, `reste_lobby_ms`, `reste_match_s`) sont
//! décomptées **en local** depuis l'instant de réception, comme le tick 250 ms
//! du DOM : la frame ne revient qu'aux transitions.
//!
//! Limites assumées :
//! - `text-shadow` / `filter: drop-shadow` non portés (aucune ombre de texte
//!   ni halo) ; le flash d'impact (`brightness(2.1)`) est rendu par un voile
//!   blanc translucide sur l'emoji ;
//! - le **grayscale** du KO n'est pas appliqué (rotation + α 0,35 conservés) ;
//! - `font-variant-numeric: tabular-nums` non appliqué (temps de match) ;
//! - `max-width: 70 %` de la bannière est approximé par un retour à la ligne
//!   à 70 % du canvas (le DOM borne la boîte, pas le texte).

use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_EXTRA_BOLD,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_LINE_SPACING_METHOD_UNIFORM, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP,
};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte};

/// z de l'arène (`#fight-root`, z-index 9993) — sous le chrono 2 (9994), le
/// pad (9995), les commandes (9996), les alertes (9997), le bandeau (9998) et
/// le welcome (9999) ; au-dessus de l'écran d'attente (9990).
pub const Z_FIGHT: i32 = 9993;

/// `.fight-zone { bottom: 4%; width: 300px }`, `.fight-zone-a { left: 4% }`,
/// `.fight-zone-b { right: 4% }`.
const ZONE_BAS: f32 = 0.04;
const ZONE_LARGEUR: f32 = 300.0;
const ZONE_MARGE: f32 = 0.04;
/// `.fight-pv { width: 220px; margin-bottom: 8px }` + en-tête (4 px).
const PV_LARGEUR: f32 = 220.0;
const PV_MARGE_BAS: f32 = 8.0;
const PV_TETE_MARGE: f32 = 4.0;
const PV_HAUTEUR: f32 = 13.0;
const PV_BORD: f32 = 1.0;
const PV_RAYON: f32 = 7.0;
const PV_TETE_TAILLE: f32 = 15.0;
/// `.fight-emoji { font-size: 128px; line-height: 1 }`.
const EMOJI_TAILLE: f32 = 128.0;
/// `.fight-nom { margin-top: 6px; font: 700 20px/1.2 }` + `.fight-plat` 11 px.
const NOM_TAILLE: f32 = 20.0;
const NOM_MARGE: f32 = 6.0;
const PLAT_TAILLE: f32 = 11.0;
/// `.fight-chips { gap: 6px; margin-top: 8px }`, `.fight-chip` 13 px,
/// `padding 6px 10px`, `border 1px`, `radius 999`.
const CHIP_TAILLE: f32 = 13.0;
const CHIP_GAP: f32 = 6.0;
const CHIP_MARGE: f32 = 8.0;
const CHIP_PAD_X: f32 = 10.0;
const CHIP_PAD_Y: f32 = 6.0;
const CHIP_BORD: f32 = 1.0;
/// `.fight-badge-actif { top: -34px; font: 800 12px/1 }` + `padding 5px 10px`.
const BADGE_TAILLE: f32 = 12.0;
const BADGE_TOP: f32 = -34.0;
const BADGE_PAD_X: f32 = 10.0;
const BADGE_PAD_Y: f32 = 5.0;
/// `.fight-timer-zone { width: 220px; height: 5px; margin-top: 7px }`.
const TIMER_ZONE_HAUTEUR: f32 = 5.0;
const TIMER_ZONE_MARGE: f32 = 7.0;

/// `.fight-centre { bottom: 5%; gap: 6px }`.
const CENTRE_BAS: f32 = 0.05;
const CENTRE_GAP: f32 = 6.0;
/// `.fight-tour { font: 700 17px/1.2 }` + `padding 6px 16px`, `radius 999`.
const TOUR_TAILLE: f32 = 17.0;
const TOUR_PAD_X: f32 = 16.0;
const TOUR_PAD_Y: f32 = 6.0;
/// `.fight-timer { width: 210px; height: 8px; border: 1px }`, `radius 4`.
const TIMER_LARGEUR: f32 = 210.0;
const TIMER_HAUTEUR: f32 = 8.0;
const TIMER_BORD: f32 = 1.0;
const TIMER_RAYON: f32 = 4.0;
/// `.fight-match { font: 600 13px/1 }`.
const MATCH_TAILLE: f32 = 13.0;

/// `.fight-banner { bottom: 24%; font: 800 26px/1.3 }`, `padding 14px 32px`,
/// `radius 16`, `max-width: 70%`.
const BANNIERE_BAS: f32 = 0.24;
const BANNIERE_TAILLE: f32 = 26.0;
const BANNIERE_PAD_X: f32 = 32.0;
const BANNIERE_PAD_Y: f32 = 14.0;
const BANNIERE_RAYON: f32 = 16.0;
const BANNIERE_MAX: f32 = 0.70;
const BANNIERE_EMOJI: f32 = 72.0;
const BANNIERE_SOUS: f32 = 15.0;

/// `.fight-info { top: 3%; max-width: 72%; padding 10px 22px; radius 14 }`,
/// `font: 600 18px/1.35`, transition 0,18 s.
const INFO_TOP: f32 = 0.03;
const INFO_MAX: f32 = 0.72;
const INFO_PAD_X: f32 = 22.0;
const INFO_PAD_Y: f32 = 10.0;
const INFO_RAYON: f32 = 14.0;
const INFO_TAILLE: f32 = 18.0;
const INFO_FONDU_S: f32 = 0.18;
/// `.fight-info.aide { font-size: 16px; padding: 14px 26px }`.
const INFO_AIDE_TAILLE: f32 = 16.0;
const INFO_AIDE_PAD_Y: f32 = 14.0;

/// Couleurs des barres de PV (`.fight-pv-fill` + classes `mid`/`low`).
const PV_HAUT: ([u8; 4], [u8; 4]) = ([0x3d, 0xdc, 0x6a, 255], [0xa8, 0xe0, 0x4b, 255]);
const PV_MOYEN: ([u8; 4], [u8; 4]) = ([0xe0, 0xb9, 0x3d, 255], [0xe0, 0x84, 0x4b, 255]);
const PV_BAS: ([u8; 4], [u8; 4]) = ([0xe0, 0x4b, 0x4b, 255], [0xc0, 0x22, 0x22, 255]);
/// `.fight-timer-fill` / `.fight-timer-zone-fill` : violet → magenta.
const TIMER_STOPS: ([u8; 4], [u8; 4]) = ([0x8b, 0x5c, 0xf6, 255], [0xd9, 0x46, 0xef, 255]);
const BLANC: [u8; 4] = [255, 255, 255, 255];
/// `.fight-tour` fond / bord.
const TOUR_FOND: [u8; 4] = [10, 12, 20, 153];
const TOUR_BORD: [u8; 4] = [255, 255, 255, 51];
/// `.fight-banner` fond / bord ; `.fight-info` fond / bord.
const BANNIERE_FOND: [u8; 4] = [10, 12, 20, 199];
const BANNIERE_BORD: [u8; 4] = [255, 255, 255, 61];
const INFO_FOND: [u8; 4] = [10, 12, 20, 219];
const INFO_BORD: [u8; 4] = [255, 255, 255, 61];
/// Kinds du toast : `warn` (bord rouge), `ok` (bord vert), `aide` (violet).
const INFO_WARN_BORD: [u8; 4] = [255, 120, 120, 153];
const INFO_WARN_TEXTE: [u8; 4] = [255, 215, 215, 255];
const INFO_OK_BORD: [u8; 4] = [120, 255, 160, 140];
const INFO_OK_TEXTE: [u8; 4] = [216, 255, 228, 255];
const INFO_AIDE_BORD: [u8; 4] = [180, 150, 255, 153];

/// Popup de dégâts (`.fight-dmg`) : 30 px 800, or ; critique 38 px rouge ;
/// soin vert ; raté 21 px gris. Animation `fight-pop` 1,1 s.
const DMG_TAILLE: f32 = 30.0;
const DMG_CRIT_TAILLE: f32 = 38.0;
const DMG_MISS_TAILLE: f32 = 21.0;
const DMG_COULEUR: [u8; 4] = [255, 210, 75, 255];
const DMG_CRIT: [u8; 4] = [255, 93, 93, 255];
const DMG_SOIN: [u8; 4] = [93, 255, 141, 255];
const DMG_MISS: [u8; 4] = [207, 214, 228, 255];
const POPUP_S: f32 = 1.1;
/// Impact (`.fight-fx`) : 54 px, `fight-fx-pop` 0,5 s ; crit 74, special 64,
/// heal 44.
const FX_TAILLE: f32 = 54.0;
const FX_S: f32 = 0.5;
/// `fight-lunge-*` 0,5 s (40 % → ±90 px, ×1,15) ; `fight-shake*` 0,4 s
/// (+200 ms) ; `fight-dodge-*` 0,4 s (40 % → ∓46 px) ; `fight-ko` 0,9 s.
const LUNGE_S: f32 = 0.5;
const LUNGE_PX: f32 = 90.0;
const SHAKE_S: f32 = 0.4;
const SHAKE_RETARD_S: f32 = 0.2;
const DODGE_S: f32 = 0.4;
const DODGE_PX: f32 = 46.0;
const KO_S: f32 = 0.9;
/// `fight-screen-shake` 0,3 s.
const ECRAN_SHAKE_S: f32 = 0.3;
/// `fight-idle` : respiration ±5 px sur 2,6 s.
const IDLE_S: f32 = 2.6;
const IDLE_PX: f32 = 5.0;

// =============================================================================
// FRAME — snapshot décodé par le pont (`src/moteur/mod.rs`).
// =============================================================================

/// Un combattant du snapshot `fight-state`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JoueurFight {
    pub pseudo: String,
    /// `twitch` | `kick` | `youtube` | `tiktok` | `bot` | `local`.
    pub plateforme: String,
    pub emoji: String,
    pub pv: u32,
    pub pv_max: u32,
    pub special_utilise: bool,
}

/// Dernière action du combat (`derniere`) — `seq` déduplique l'animation.
#[derive(Debug, Clone, PartialEq)]
pub struct CoupFight {
    pub seq: u64,
    /// `'a'` ou `'b'`.
    pub acteur: char,
    /// `rapide` | `lourd` | `special` | `passe`.
    pub coup: String,
    pub degats: u32,
    pub soin: u32,
    pub touche: bool,
    pub critique: bool,
}

/// Toast / carte d'aide (`info`).
#[derive(Debug, Clone, PartialEq)]
pub struct InfoFight {
    pub seq: u64,
    pub kind: String,
    pub texte: String,
    pub duree_ms: u64,
    /// Âge du message à l'émission — un message trop vieux n'est pas rejoué.
    pub age_ms: u64,
}

/// Noms de commandes affichés par l'overlay (seule source hors Twitch).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CmdsFight {
    pub a: String,
    pub b: String,
    pub c: String,
    pub accept: String,
    pub refuse: String,
}

/// Phase de la partie (union du snapshot).
#[derive(Debug, Clone, PartialEq)]
pub enum PhaseFight {
    Idle,
    Lobby {
        defiant: JoueurFight,
        cible: Option<String>,
        reste_lobby_ms: u64,
        duree_tour_s: u64,
        duree_match_s: u64,
        cmds: CmdsFight,
    },
    Combat {
        ja: JoueurFight,
        jb: JoueurFight,
        tour: char,
        reste_tour_ms: u64,
        duree_tour_ms: u64,
        reste_match_s: u64,
        derniere: Option<CoupFight>,
        cmds: CmdsFight,
    },
    Fin {
        ja: JoueurFight,
        jb: JoueurFight,
        gagnant: Option<char>,
        /// `ko` | `abandon` | `temps` | `stop` | `afk`.
        raison: String,
    },
}

/// Snapshot complet d'une frame `fight-state`.
#[derive(Debug, Clone, PartialEq)]
pub struct FightState {
    pub phase: PhaseFight,
    pub info: Option<InfoFight>,
    pub anims_reduites: bool,
}

impl Default for FightState {
    fn default() -> Self {
        Self {
            phase: PhaseFight::Idle,
            info: None,
            anims_reduites: false,
        }
    }
}

// =============================================================================
// ÉTAT LIVE
// =============================================================================

/// État vivant de l'arène : dernier snapshot + horloges locales.
pub struct EtatFight {
    frame: FightState,
    /// Réception du snapshot — base des échéances locales.
    recu: Instant,
    version: u64,
    /// Action en cours d'animation : (seq, coup, départ).
    action: Option<(u64, CoupFight, Instant)>,
    /// Toast en cours : (seq, réception).
    info: Option<(u64, Instant)>,
    /// Fin du KO (phase `fin`) — la chute dure 0,9 s puis reste figée.
    ko: Option<Instant>,
}

impl Default for EtatFight {
    fn default() -> Self {
        Self {
            frame: FightState::default(),
            recu: Instant::now(),
            version: 0,
            action: None,
            info: None,
            ko: None,
        }
    }
}

impl EtatFight {
    pub fn nouveau() -> Self {
        Self::default()
    }

    /// Applique un snapshot complet (idempotent : un resync rejoue la même
    /// frame sans rejouer l'animation — `derniere.seq` déduplique).
    pub fn appliquer(&mut self, frame: FightState) {
        self.appliquer_at(frame, Instant::now());
    }

    fn appliquer_at(&mut self, frame: FightState, maintenant: Instant) {
        // Nouvelle action : `seq` différent de la dernière animée.
        if let PhaseFight::Combat {
            derniere: Some(coup),
            ..
        } = &frame.phase
        {
            let deja = self.action.as_ref().map(|(seq, _, _)| *seq) == Some(coup.seq);
            if !deja {
                self.action = Some((coup.seq, coup.clone(), maintenant));
            }
        }
        // Toast : dedup par seq, âge vérifié (un vieux message n'est pas
        // rejoué après un resync — même règle que `fightMajInfo`).
        if let Some(info) = &frame.info {
            let deja = self.info.as_ref().map(|(seq, _)| *seq) == Some(info.seq);
            let duree = info.duree_ms.max(1);
            if !deja && info.age_ms <= duree {
                // Décalé de l'âge : un message déjà entamé s'éteint à l'heure.
                let depuis = maintenant
                    .checked_sub(Duration::from_millis(info.age_ms))
                    .unwrap_or(maintenant);
                self.info = Some((info.seq, depuis));
            }
        }
        // KO : démarre à l'entrée en phase `fin` (une seule fois).
        if matches!(frame.phase, PhaseFight::Fin { .. }) && self.ko.is_none() {
            self.ko = Some(maintenant);
        }
        if !matches!(frame.phase, PhaseFight::Fin { .. }) {
            self.ko = None;
        }
        self.recu = maintenant;
        self.frame = frame;
        self.version = self.version.wrapping_add(1);
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn frame(&self) -> &FightState {
        &self.frame
    }

    /// L'arène est-elle visible (toute phase sauf `idle`) ?
    pub fn visible(&self) -> bool {
        !matches!(self.frame.phase, PhaseFight::Idle)
    }

    /// Millisecondes restantes d'une échéance reçue `reste_ms` à la réception.
    fn reste_ms(&self, reste_ms: u64, maintenant: Instant) -> u64 {
        let ecoule = maintenant.saturating_duration_since(self.recu).as_millis() as u64;
        reste_ms.saturating_sub(ecoule)
    }

    /// Échéances et phases résolues au temps courant (ce que la composition
    /// peint ce tick-ci).
    pub fn rendu(&self) -> RenduFight<'_> {
        self.rendu_at(Instant::now())
    }

    fn rendu_at(&self, maintenant: Instant) -> RenduFight<'_> {
        let (reste_tour_ms, reste_lobby_ms, reste_match_s) = match &self.frame.phase {
            PhaseFight::Combat {
                reste_tour_ms,
                reste_match_s,
                ..
            } => (
                self.reste_ms(*reste_tour_ms, maintenant),
                None,
                Some(self.reste_ms(reste_match_s.saturating_mul(1000), maintenant) / 1000),
            ),
            PhaseFight::Lobby { reste_lobby_ms, .. } => {
                (0, Some(self.reste_ms(*reste_lobby_ms, maintenant)), None)
            }
            _ => (0, None, None),
        };
        // Toast : visible `duree_ms` après réception.
        let info = match (&self.frame.info, self.info) {
            (Some(info), Some((_, depuis))) => {
                let ecoule = maintenant.saturating_duration_since(depuis).as_secs_f32();
                let duree = info.duree_ms as f32 / 1000.0;
                if ecoule >= duree {
                    None
                } else {
                    Some(InfoRendue {
                        info,
                        opacite: (ecoule / INFO_FONDU_S).min(1.0),
                    })
                }
            }
            _ => None,
        };
        // Action en cours (0,5 s de lunge + popups 1,1 s).
        let action = self.action.as_ref().map(|(_, coup, depart)| {
            (
                coup,
                maintenant.saturating_duration_since(*depart).as_secs_f32(),
            )
        });
        RenduFight {
            frame: &self.frame,
            reste_tour_ms,
            reste_lobby_ms,
            reste_match_s,
            info,
            action,
            ko_depuis: self.ko.map(|t| maintenant.saturating_duration_since(t).as_secs_f32()),
        }
    }
}

/// Toast résolu (opacité de la transition 0,18 s).
pub struct InfoRendue<'a> {
    pub info: &'a InfoFight,
    pub opacite: f32,
}

/// Ce que la composition peint ce tick-ci (échéances déjà décomptées).
pub struct RenduFight<'a> {
    pub frame: &'a FightState,
    pub reste_tour_ms: u64,
    pub reste_lobby_ms: Option<u64>,
    pub reste_match_s: Option<u64>,
    pub info: Option<InfoRendue<'a>>,
    /// `(coup, secondes écoulées depuis le début de l'animation)`.
    pub action: Option<(&'a CoupFight, f32)>,
    /// Secondes écoulées depuis le début du KO (phase `fin`).
    pub ko_depuis: Option<f32>,
}

/// Pourcentage de PV borné (miroir de `fightMajZone`).
pub fn pourcentage_pv(pv: u32, pv_max: u32) -> f32 {
    let max = if pv_max > 0 { pv_max } else { 100 };
    ((pv as f32 / max as f32) * 100.0).clamp(0.0, 100.0).round()
}

/// Couleurs de la barre selon le pourcentage : `≤ 25` bas, `≤ 55` moyen.
pub fn couleurs_pv(pct: f32) -> ([u8; 4], [u8; 4]) {
    if pct <= 25.0 {
        PV_BAS
    } else if pct <= 55.0 {
        PV_MOYEN
    } else {
        PV_HAUT
    }
}

/// `fightTick` : `M:SS` du temps de match restant.
pub fn formater_match(reste_s: u64) -> String {
    format!("{}:{:02}", reste_s / 60, reste_s % 60)
}

/// `ease-in-out` CSS (courbes des animations de l'arène).
fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // `cubic-bezier(0.42, 0, 0.58, 1)` — approximation polynomiale exacte
    // à l'aller-retour près (même formule que `media::ease_in_out`).
    let bez = |u: f32, p1: f32, p2: f32| {
        let v = 1.0 - u;
        3.0 * u * v * v * p1 + 3.0 * u * u * v * p2 + u * u * u
    };
    let mut u = t;
    for _ in 0..4 {
        let x = bez(u, 0.42, 0.58);
        let v = 1.0 - u;
        let dx = 3.0 * v * v * 0.42 + 6.0 * u * v * (0.58 - 0.42) + 3.0 * u * u * (1.0 - 0.58);
        if dx.abs() < 1e-6 {
            break;
        }
        u = (u - (x - t) / dx).clamp(0.0, 1.0);
    }
    bez(u, 0.0, 1.0)
}

/// Progression d'une keyframe CSS à 3 points (`0 % → 40 % → 100 %`) avec
/// `ease` (approché par `ease-in-out`) — lunge et esquive.
fn progression_3(t: f32, duree: f32) -> f32 {
    let k = (t / duree).clamp(0.0, 1.0);
    if k < 0.4 {
        ease_in_out(k / 0.4)
    } else {
        1.0 - ease_in_out((k - 0.4) / 0.6)
    }
}

/// Oscillation de la secousse (`fight-shake*`) : ±8 px à 20/40 %, ±6 à 60/80 %.
fn secousse(t: f32, duree: f32) -> f32 {
    let k = (t / duree).clamp(0.0, 1.0);
    let (a, b) = (8.0, 6.0);
    if k < 0.2 {
        -a * ease_in_out(k / 0.2)
    } else if k < 0.4 {
        -a + 2.0 * a * ease_in_out((k - 0.2) / 0.2)
    } else if k < 0.6 {
        a - (a + b) * ease_in_out((k - 0.4) / 0.2)
    } else if k < 0.8 {
        -b + 2.0 * b * ease_in_out((k - 0.6) / 0.2)
    } else {
        b - b * ease_in_out((k - 0.8) / 0.2)
    }
}

// =============================================================================
// BLOC DE RENDU
// =============================================================================

struct LigneF {
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    w: f32,
    h: f32,
    /// Couleur de la spécification (un override reste possible au dessin).
    couleur: [u8; 4],
}

/// Spécification d'une ligne de l'arène.
struct SpecF<'a> {
    texte: &'a str,
    famille: &'a str,
    taille: f32,
    poids: DWRITE_FONT_WEIGHT,
    couleur: [u8; 4],
    /// Largeur de mise en page (mise à la ligne si `wrap`).
    largeur: f32,
    wrap: bool,
    centre: bool,
}

struct ZoneF {
    pv_nom: Option<LigneF>,
    pv_val: Option<LigneF>,
    emoji: Option<LigneF>,
    nom: Option<LigneF>,
    plat: Option<LigneF>,
    chips: [Option<LigneF>; 3],
    badge: Option<LigneF>,
    pv_pct: f32,
    special: bool,
}

impl Default for ZoneF {
    fn default() -> Self {
        Self {
            pv_nom: None,
            pv_val: None,
            emoji: None,
            nom: None,
            plat: None,
            chips: [None, None, None],
            badge: None,
            pv_pct: 0.0,
            special: false,
        }
    }
}

pub(super) struct BlocFight {
    cle: u64,
    a: ZoneF,
    b: ZoneF,
    tour: Option<LigneF>,
    match_txt: Option<LigneF>,
    banniere_emoji: Option<LigneF>,
    banniere_titre: Option<LigneF>,
    banniere_sous: Option<LigneF>,
    info: Option<LigneF>,
}

fn cle_fight(
    version: u64,
    seconde_tour: u64,
    seconde_lobby: u64,
    seconde_match: u64,
    canvas: (u32, u32),
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    seconde_tour.hash(&mut h);
    seconde_lobby.hash(&mut h);
    seconde_match.hash(&mut h);
    canvas.hash(&mut h);
    h.finish()
}

impl DessinateurTexte {
    /// Dessine l'arène — dans une passe ouverte.
    pub fn dessiner_fight(
        &mut self,
        etat: &EtatFight,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        if !etat.visible() {
            self.bloc_fight = None;
            return Ok(());
        }
        let rendu = etat.rendu();
        let seconde_tour = rendu.reste_tour_ms / 1000;
        let seconde_lobby = rendu.reste_lobby_ms.unwrap_or(0) / 1000;
        let seconde_match = rendu.reste_match_s.unwrap_or(0);
        self.synchroniser_bloc_fight(etat.version(), &rendu, seconde_tour, seconde_lobby, seconde_match, canvas)?;

        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        // Secousse d'écran (`fight-screen-shake`, coup lourd / critique).
        let secousse_ecran = match &rendu.action {
            Some((coup, t)) if !rendu.frame.anims_reduites => {
                let demarre = *t >= SHAKE_RETARD_S && *t < SHAKE_RETARD_S + ECRAN_SHAKE_S;
                if demarre && (coup.coup == "lourd" || coup.critique) {
                    let k = (*t - SHAKE_RETARD_S) / ECRAN_SHAKE_S;
                    let dx = if k < 0.25 {
                        -4.0 * (k / 0.25)
                    } else if k < 0.5 {
                        -4.0 + 8.0 * ((k - 0.25) / 0.25)
                    } else if k < 0.75 {
                        4.0 - 6.0 * ((k - 0.5) / 0.25)
                    } else {
                        -2.0 + 2.0 * ((k - 0.75) / 0.25)
                    };
                    (dx, 0.0)
                } else {
                    (0.0, 0.0)
                }
            }
            _ => (0.0, 0.0),
        };
        unsafe {
            self.cible_rt.SetTransform(&Matrix3x2 {
                M11: 1.0,
                M12: 0.0,
                M21: 0.0,
                M22: 1.0,
                M31: secousse_ecran.0,
                M32: secousse_ecran.1,
            })
        };

        let resultat = (|| -> Result<(), ErreurTexte> {
            match &rendu.frame.phase {
                PhaseFight::Idle => Ok(()),
                PhaseFight::Lobby { .. } => self.peindre_banniere(&rendu, canvas),
                PhaseFight::Combat { tour, .. } => {
                    self.peindre_zones(&rendu, canvas, temps_s)?;
                    self.peindre_centre(&rendu, canvas, *tour)
                }
                PhaseFight::Fin { .. } => {
                    self.peindre_zones(&rendu, canvas, temps_s)?;
                    self.peindre_banniere(&rendu, canvas)
                }
            }?;
            self.peindre_info(&rendu, canvas)
        })();
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        let _ = (w, h);
        resultat
    }

    /// (Re)met en page l'arène quand la clé change (version ou seconde
    /// affichée — les échéances sont décomptées en local à 4 Hz).
    fn synchroniser_bloc_fight(
        &mut self,
        version: u64,
        rendu: &RenduFight<'_>,
        seconde_tour: u64,
        seconde_lobby: u64,
        seconde_match: u64,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let cle = cle_fight(version, seconde_tour, seconde_lobby, seconde_match, canvas);
        if self.bloc_fight.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let mut a = ZoneF::default();
        let mut b = ZoneF::default();
        let mut tour = None;
        let mut match_txt = None;
        let mut banniere_emoji = None;
        let mut banniere_titre = None;
        let mut banniere_sous = None;
        let mut info = None;

        match &rendu.frame.phase {
            PhaseFight::Idle => {}
            PhaseFight::Lobby {
                defiant,
                cible,
                duree_tour_s,
                duree_match_s,
                cmds,
                ..
            } => {
                let emoji = if defiant.emoji.is_empty() {
                    "🐲"
                } else {
                    defiant.emoji.as_str()
                };
                banniere_emoji = Some(self.ligne_f(SpecF {
                    texte: emoji,
                    famille: "Segoe UI Emoji",
                    taille: BANNIERE_EMOJI,
                    poids: DWRITE_FONT_WEIGHT_BOLD,
                    couleur: BLANC,
                    largeur: BANNIERE_EMOJI * 2.0,
                    wrap: false,
                    centre: true,
                })?);
                let pseudo = if defiant.pseudo.is_empty() {
                    "?"
                } else {
                    defiant.pseudo.as_str()
                };
                let mini = format!(
                    "3 coups · {} s/tour · {} min",
                    duree_tour_s.max(&1),
                    (*duree_match_s.max(&1) as f32 / 60.0).round().max(1.0) as u64
                );
                let accept = if cmds.accept.is_empty() {
                    "accept"
                } else {
                    cmds.accept.as_str()
                };
                let refuse = if cmds.refuse.is_empty() {
                    "refuse"
                } else {
                    cmds.refuse.as_str()
                };
                let (titre, sous) = match cible {
                    Some(cible) => (
                        format!("⚔ {pseudo} défie @{cible}"),
                        format!("!{accept} {emoji} ou !{refuse} · {mini}"),
                    ),
                    None => (
                        format!("⚔ {pseudo} cherche un adversaire"),
                        format!("!{accept} {emoji} pour combattre · {mini}"),
                    ),
                };
                banniere_titre = Some(self.ligne_f(SpecF {
                    texte: &titre,
                    famille: "Segoe UI",
                    taille: BANNIERE_TAILLE,
                    poids: DWRITE_FONT_WEIGHT_EXTRA_BOLD,
                    couleur: BLANC,
                    largeur: BANNIERE_MAX * w - 2.0 * BANNIERE_PAD_X,
                    wrap: true,
                    centre: true,
                })?);
                let reste = rendu.reste_lobby_ms.unwrap_or(0) / 1000;
                banniere_sous = Some(self.ligne_f(SpecF {
                    texte: &format!("{sous} — {reste} s"),
                    famille: "Segoe UI",
                    taille: BANNIERE_SOUS,
                    poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    couleur: [255, 255, 255, 217],
                    largeur: BANNIERE_MAX * w - 2.0 * BANNIERE_PAD_X,
                    wrap: true,
                    centre: true,
                })?);
            }
            PhaseFight::Combat {
                ja,
                jb,
                tour: t,
                duree_tour_ms,
                cmds,
                ..
            } => {
                a = self.zone_f(ja, Some(cmds), true)?;
                b = self.zone_f(jb, Some(cmds), true)?;
                let actif = if *t == 'b' { jb } else { ja };
                let nom = if actif.pseudo.is_empty() {
                    "?"
                } else {
                    actif.pseudo.as_str()
                };
                let sec = rendu.reste_tour_ms.div_ceil(1000);
                tour = Some(self.ligne_f(SpecF {
                    texte: &format!("⚔ Au tour de {nom} · {sec} s"),
                    famille: "Segoe UI",
                    taille: TOUR_TAILLE,
                    poids: DWRITE_FONT_WEIGHT_BOLD,
                    couleur: BLANC,
                    largeur: w,
                    wrap: false,
                    centre: false,
                })?);
                let _ = duree_tour_ms;
                match_txt = Some(self.ligne_f(SpecF {
                    texte: &formater_match(rendu.reste_match_s.unwrap_or(0)),
                    famille: "Segoe UI",
                    taille: MATCH_TAILLE,
                    poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    couleur: [255, 255, 255, 204],
                    largeur: w,
                    wrap: false,
                    centre: false,
                })?);
            }
            PhaseFight::Fin {
                ja,
                jb,
                gagnant,
                raison,
            } => {
                a = self.zone_f(ja, None, false)?;
                b = self.zone_f(jb, None, false)?;
                let titre = match (raison.as_str(), gagnant) {
                    ("stop", _) => "🛑 Combat arrêté".to_string(),
                    ("afk", _) => "⏹ Match abandonné — inactivité".to_string(),
                    (r, Some('a')) => {
                        let nom = if ja.pseudo.is_empty() { "?" } else { &ja.pseudo };
                        format!("🏆 {nom}{}", suffixe_victoire(r))
                    }
                    (r, Some('b')) => {
                        let nom = if jb.pseudo.is_empty() { "?" } else { &jb.pseudo };
                        format!("🏆 {nom}{}", suffixe_victoire(r))
                    }
                    _ => "🤝 Égalité !".to_string(),
                };
                banniere_titre = Some(self.ligne_f(SpecF {
                    texte: &titre,
                    famille: "Segoe UI",
                    taille: BANNIERE_TAILLE,
                    poids: DWRITE_FONT_WEIGHT_EXTRA_BOLD,
                    couleur: BLANC,
                    largeur: BANNIERE_MAX * w - 2.0 * BANNIERE_PAD_X,
                    wrap: true,
                    centre: true,
                })?);
            }
        }

        if let Some(inf) = &rendu.info {
            let aide = inf.info.kind == "aide";
            info = Some(self.ligne_f(SpecF {
                texte: &inf.info.texte,
                famille: "Segoe UI",
                taille: if aide { INFO_AIDE_TAILLE } else { INFO_TAILLE },
                poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                couleur: match inf.info.kind.as_str() {
                    "warn" => INFO_WARN_TEXTE,
                    "ok" => INFO_OK_TEXTE,
                    _ => BLANC,
                },
                largeur: INFO_MAX * w - 2.0 * INFO_PAD_X,
                wrap: true,
                centre: true,
            })?);
        }
        let _ = h;

        self.bloc_fight = Some(BlocFight {
            cle,
            a,
            b,
            tour,
            match_txt,
            banniere_emoji,
            banniere_titre,
            banniere_sous,
            info,
        });
        Ok(())
    }

    /// Met en page une zone combattant (PV, emoji, nom, chips, badge).
    fn zone_f(
        &self,
        j: &JoueurFight,
        cmds: Option<&CmdsFight>,
        actif_possible: bool,
    ) -> Result<ZoneF, ErreurTexte> {
        let pct = pourcentage_pv(j.pv, j.pv_max);
        let max = if j.pv_max > 0 { j.pv_max } else { 100 };
        let pseudo = if j.pseudo.is_empty() { "?" } else { j.pseudo.as_str() };
        let emoji = if j.emoji.is_empty() { "❓" } else { j.emoji.as_str() };
        let plat = if !j.plateforme.is_empty()
            && j.plateforme != "bot"
            && j.plateforme != "local"
        {
            Some(format!(" · {}", j.plateforme))
        } else {
            None
        };
        let mut zone = ZoneF {
            pv_pct: pct,
            special: j.special_utilise,
            ..ZoneF::default()
        };
        zone.pv_nom = Some(self.ligne_f(SpecF {
            texte: pseudo,
            famille: "Segoe UI",
            taille: PV_TETE_TAILLE,
            poids: DWRITE_FONT_WEIGHT_BOLD,
            couleur: BLANC,
            largeur: PV_LARGEUR,
            wrap: false,
            centre: false,
        })?);
        zone.pv_val = Some(self.ligne_f(SpecF {
            texte: &format!("{}/{}", j.pv, max),
            famille: "Segoe UI",
            taille: PV_TETE_TAILLE,
            poids: DWRITE_FONT_WEIGHT_BOLD,
            couleur: BLANC,
            largeur: PV_LARGEUR,
            wrap: false,
            centre: false,
        })?);
        zone.emoji = Some(self.ligne_f(SpecF {
            texte: emoji,
            famille: "Segoe UI Emoji",
            taille: EMOJI_TAILLE,
            poids: DWRITE_FONT_WEIGHT_BOLD,
            couleur: BLANC,
            largeur: EMOJI_TAILLE * 2.0,
            wrap: false,
            centre: false,
        })?);
        zone.nom = Some(self.ligne_f(SpecF {
            texte: pseudo,
            famille: "Segoe UI",
            taille: NOM_TAILLE,
            poids: DWRITE_FONT_WEIGHT_BOLD,
            couleur: BLANC,
            largeur: ZONE_LARGEUR,
            wrap: false,
            centre: false,
        })?);
        if let Some(p) = &plat {
            zone.plat = Some(self.ligne_f(SpecF {
                texte: p,
                famille: "Segoe UI",
                taille: PLAT_TAILLE,
                poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                couleur: [255, 255, 255, 179],
                largeur: ZONE_LARGEUR,
                wrap: false,
                centre: false,
            })?);
        }
        if let (Some(cmds), true) = (cmds, actif_possible) {
            let noms = [
                format!("⚡ !{} rapide", nom_ou(&cmds.a, "a")),
                format!("💥 !{} lourd", nom_ou(&cmds.b, "b")),
                format!("🩸 !{} spécial", nom_ou(&cmds.c, "c")),
            ];
            for (i, texte) in noms.iter().enumerate() {
                zone.chips[i] = Some(self.ligne_f(SpecF {
                    texte,
                    famille: "Segoe UI",
                    taille: CHIP_TAILLE,
                    poids: DWRITE_FONT_WEIGHT_BOLD,
                    couleur: BLANC,
                    largeur: ZONE_LARGEUR,
                    wrap: false,
                    centre: false,
                })?);
            }
            zone.badge = Some(self.ligne_f(SpecF {
                texte: "À TOI",
                famille: "Segoe UI",
                taille: BADGE_TAILLE,
                poids: DWRITE_FONT_WEIGHT_EXTRA_BOLD,
                couleur: [26, 20, 8, 255],
                largeur: 200.0,
                wrap: false,
                centre: false,
            })?);
        }
        Ok(zone)
    }

    /// Met en page une ligne (famille explicite : `Segoe UI` ou
    /// `Segoe UI Emoji`).
    fn ligne_f(&self, spec: SpecF<'_>) -> Result<LigneF, ErreurTexte> {
        let famille: Vec<u16> = spec
            .famille
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
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
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat fight : {}", e.message())))?;
        unsafe {
            format.SetTextAlignment(if spec.centre {
                DWRITE_TEXT_ALIGNMENT_CENTER
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            })
        }
        .map_err(|e| ErreurTexte::Systeme(format!("alignement fight : {}", e.message())))?;
        // `line-height` CSS : la hauteur de ligne est imposée par le style.
        unsafe {
            format.SetLineSpacing(
                DWRITE_LINE_SPACING_METHOD_UNIFORM,
                spec.taille * 1.2,
                spec.taille * 0.8,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("interligne fight : {}", e.message())))?;
        let contenu: Vec<u16> = spec.texte.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, spec.largeur.max(1.0), 4096.0)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("layout fight : {}", e.message())))?;
        unsafe {
            layout.SetWordWrapping(if spec.wrap {
                DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            })
        }
        .map_err(|e| ErreurTexte::Systeme(format!("wrap fight : {}", e.message())))?;
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques fight : {}", e.message())))?;
        Ok(LigneF {
            layout,
            _format: format,
            w: metriques.widthIncludingTrailingWhitespace,
            h: metriques.height,
            couleur: spec.couleur,
        })
    }

    /// Dessine une ligne à `(x, y)` (brosse unie, glyphes **couleur**
    /// activés — l'arène est pleine d'emojis).
    ///
    /// ⚠️ `ID2D1RenderTarget::DrawTextLayout` n'active pas les glyphes
    /// COLR/CPAL : il faut le `ID2D1DeviceContext` avec
    /// `D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT`, sinon les emojis sortent en
    /// blanc (vérifié au rendu, 2026-11-01).
    fn dessiner_ligne_f(
        &self,
        ligne: &LigneF,
        x: f32,
        y: f32,
        override_couleur: Option<[u8; 4]>,
        opacite: f32,
    ) -> Result<(), ErreurTexte> {
        let couleur = override_couleur.unwrap_or(ligne.couleur);
        let c = [
            couleur[0],
            couleur[1],
            couleur[2],
            (couleur[3] as f32 * opacite.clamp(0.0, 1.0)).round() as u8,
        ];
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(c), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse fight : {}", e.message())))?;
        unsafe {
            self.ctx.DrawTextLayout(
                windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x, y },
                &ligne.layout,
                &brosse,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            )
        };
        Ok(())
    }

    /// Rectangle arrondi plein (fond d'une pastille / bannière).
    fn remplir_arrondi(
        &self,
        rect: D2D_RECT_F,
        rayon: f32,
        couleur: [u8; 4],
    ) -> Result<(), ErreurTexte> {
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse arrondi : {}", e.message())))?;
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: rayon,
                    radiusY: rayon,
                },
                &brosse,
            )
        };
        Ok(())
    }

    /// Contour arrondi (bordure 1 px).
    fn border_arrondi(
        &self,
        rect: D2D_RECT_F,
        rayon: f32,
        couleur: [u8; 4],
    ) -> Result<(), ErreurTexte> {
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse bord fight : {}", e.message())))?;
        unsafe {
            self.cible_rt.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: rayon,
                    radiusY: rayon,
                },
                &brosse,
                1.0,
                None,
            )
        };
        Ok(())
    }

    /// Deux zones combattants (barre PV, emoji animé, nom, chips, timer).
    fn peindre_zones(
        &mut self,
        rendu: &RenduFight<'_>,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let tour = match &rendu.frame.phase {
            PhaseFight::Combat { tour, .. } => Some(*tour),
            _ => None,
        };
        for (cote, zone) in [('a', 0), ('b', 1)] {
            let actif = tour == Some(cote);
            self.peindre_zone(rendu, canvas, temps_s, cote, zone, actif, (w, h))?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn peindre_zone(
        &mut self,
        rendu: &RenduFight<'_>,
        canvas: (u32, u32),
        temps_s: f64,
        cote: char,
        index: usize,
        actif: bool,
        dims: (f32, f32),
    ) -> Result<(), ErreurTexte> {
        let (w, h) = dims;
        let _ = canvas;
        let Some(bloc) = self.bloc_fight.as_ref() else {
            return Ok(());
        };
        let zone = if index == 0 { &bloc.a } else { &bloc.b };
        // Position : `left 4%` / `right 4%`, `bottom 4%`, largeur 300.
        let x = if cote == 'a' {
            ZONE_MARGE * w
        } else {
            w - ZONE_MARGE * w - ZONE_LARGEUR
        };
        // La colonne est empilée depuis le bas : on mesure la hauteur totale
        // (PV + emoji + nom + chips + timer) et on part du bas.
        let chips_visibles = actif && zone.chips[0].is_some();
        let h_chips = if chips_visibles {
            CHIP_MARGE + CHIP_PAD_Y * 2.0 + CHIP_BORD * 2.0 + CHIP_TAILLE
        } else {
            0.0
        };
        let h_timer = if actif {
            TIMER_ZONE_MARGE + TIMER_ZONE_HAUTEUR + TIMER_BORD * 2.0
        } else {
            0.0
        };
        let h_nom = NOM_MARGE
            + zone.nom.as_ref().map(|l| l.h).unwrap_or(0.0)
            + zone.plat.as_ref().map(|l| l.h).unwrap_or(0.0);
        let h_emoji = zone.emoji.as_ref().map(|l| l.h).unwrap_or(0.0);
        let h_pv = zone.pv_nom.as_ref().map(|l| l.h).unwrap_or(0.0)
            + PV_TETE_MARGE
            + PV_HAUTEUR
            + PV_BORD * 2.0
            + PV_MARGE_BAS;
        let total = h_pv + h_emoji + h_nom + h_chips + h_timer;
        let bas = h - ZONE_BAS * h;
        let mut y = bas - total;

        // PV : en-tête (pseudo / valeur) puis barre 220 px.
        let pv_x = x + (ZONE_LARGEUR - PV_LARGEUR) / 2.0;
        if let (Some(nom), Some(val)) = (&zone.pv_nom, &zone.pv_val) {
            self.dessiner_ligne_f(nom, pv_x, y, None, 1.0)?;
            self.dessiner_ligne_f(val, pv_x + PV_LARGEUR - val.w, y, None, 1.0)?;
            y += nom.h.max(val.h) + PV_TETE_MARGE;
        }
        let barre = D2D_RECT_F {
            left: pv_x,
            top: y,
            right: pv_x + PV_LARGEUR,
            bottom: y + PV_HAUTEUR + PV_BORD * 2.0,
        };
        self.remplir_arrondi(barre, PV_RAYON, [0, 0, 0, 140])?;
        let (c1, c2) = couleurs_pv(zone.pv_pct);
        let interieur = D2D_RECT_F {
            left: barre.left + PV_BORD,
            top: barre.top + PV_BORD,
            right: barre.right - PV_BORD,
            bottom: barre.bottom - PV_BORD,
        };
        if zone.pv_pct > 0.0 {
            let largeur = (interieur.right - interieur.left) * zone.pv_pct / 100.0;
            let rempli = D2D_RECT_F {
                left: interieur.left,
                top: interieur.top,
                right: interieur.left + largeur,
                bottom: interieur.bottom,
            };
            // Dégradé horizontal 90° (vert → vert clair, etc.).
            let brosse = self.brosse_horizontale(rempli, c1, c2)?;
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: rempli,
                        radiusX: (PV_RAYON - PV_BORD).max(0.0),
                        radiusY: (PV_RAYON - PV_BORD).max(0.0),
                    },
                    &brosse,
                )
            };
        }
        self.border_arrondi(barre, PV_RAYON, [255, 255, 255, 71])?;
        y += PV_HAUTEUR + PV_BORD * 2.0 + PV_MARGE_BAS;

        // Emoji : respiration `fight-idle` (±5 px / 2,6 s), lunge/secousse/
        // esquive selon l'action, KO en phase `fin`.
        let mut y_emoji = y;
        if let Some(emoji) = &zone.emoji {
            y_emoji = y;
            let mut dx = 0.0_f32;
            let mut dy = 0.0_f32;
            let mut echelle = 1.0_f32;
            let mut rotation = 0.0_f32;
            let mut opacite = 1.0_f32;
            let ko = rendu.ko_depuis.map(|t| (t / KO_S).min(1.0));
            if let Some(k) = ko {
                // `fight-ko` : rotate(75deg) translateY(28px), α 0,35.
                let gagnant_perd = match &rendu.frame.phase {
                    PhaseFight::Fin { gagnant, .. } => match gagnant {
                        Some('a') => cote == 'b',
                        Some('b') => cote == 'a',
                        _ => false,
                    },
                    _ => false,
                };
                if gagnant_perd {
                    let e = ease_in_out(k);
                    rotation = 75.0 * e;
                    dy += 28.0 * e;
                    opacite = 1.0 - 0.65 * e;
                }
            }
            if ko.is_none() && !rendu.frame.anims_reduites {
                // Respiration.
                let p = ((temps_s as f32) / IDLE_S).rem_euclid(1.0);
                let k = if p < 0.5 {
                    ease_in_out(p * 2.0)
                } else {
                    1.0 - ease_in_out((p - 0.5) * 2.0)
                };
                dy -= IDLE_PX * k;
            }
            if let Some((coup, t)) = rendu.action {
                let attaquant = coup.acteur == cote;
                if attaquant {
                    if t < LUNGE_S && coup.coup != "passe" {
                        let k = progression_3(t, LUNGE_S);
                        let sens = if cote == 'a' { 1.0 } else { -1.0 };
                        dx += sens * LUNGE_PX * k;
                        echelle = 1.0 + 0.15 * k;
                    }
                    if coup.soin > 0 && (SHAKE_RETARD_S..SHAKE_RETARD_S + POPUP_S).contains(&t) {
                        // Popup « +N » sur l'attaquant (voir plus bas).
                    }
                } else if coup.coup != "passe" {
                    let debut = t - SHAKE_RETARD_S;
                    if !coup.touche && t < DODGE_S {
                        // Esquive : 40 % → ±46 px (sens opposé au recul).
                        let k = progression_3(t, DODGE_S);
                        let sens = if cote == 'a' { -1.0 } else { 1.0 };
                        dx += sens * DODGE_PX * k;
                    } else if coup.touche && (0.0..SHAKE_S).contains(&debut) {
                        dx += secousse(debut, SHAKE_S);
                    }
                }
            }
            let (cx, cy) = (x + ZONE_LARGEUR / 2.0, y + emoji.h / 2.0);
            // Composition CSS `scaleX(-1) rotate(θ)` autour du centre, puis
            // échelle et translation : matrices appliquées DANS CET ORDRE
            // (convention ligne de Direct2D, `mul_mtx`).
            let mtx = mul_mtx(
                mul_mtx(
                    mul_mtx(
                        echelle_autour(cx, cy, echelle),
                        rotation_autour(cx, cy, rotation),
                    ),
                    if cote == 'b' {
                        miroir_horizontal(cx)
                    } else {
                        Matrix3x2::identity()
                    },
                ),
                translation(dx, dy),
            );
            unsafe { self.cible_rt.SetTransform(&mtx) };
            self.dessiner_ligne_f(emoji, cx - emoji.w / 2.0, y, None, opacite)?;
            // Flash d'impact : voile blanc 0,26 s sur l'emoji (le `brightness`
            // du DOM n'a pas d'équivalent Direct2D).
            if let Some((coup, t)) = rendu.action {
                let debut = t - SHAKE_RETARD_S;
                if coup.touche && coup.acteur != cote && (0.0..0.26).contains(&debut) {
                    let a = (1.0 - debut / 0.26).clamp(0.0, 1.0);
                    self.remplir_arrondi(
                        D2D_RECT_F {
                            left: cx - emoji.w / 2.0,
                            top: y,
                            right: cx + emoji.w / 2.0,
                            bottom: y + emoji.h,
                        },
                        8.0,
                        [255, 255, 255, (a * 90.0) as u8],
                    )?;
                }
            }
            unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
            y += emoji.h;
        }

        // Nom + plateforme.
        if let Some(nom) = &zone.nom {
            y += NOM_MARGE;
            self.dessiner_ligne_f(nom, x + (ZONE_LARGEUR - nom.w) / 2.0, y, None, 1.0)?;
            y += nom.h;
        }
        if let Some(plat) = &zone.plat {
            self.dessiner_ligne_f(plat, x + (ZONE_LARGEUR - plat.w) / 2.0, y, None, 1.0)?;
            y += plat.h;
        }

        // Chips de coups (zone active) — rangée centrée.
        if chips_visibles {
            y += CHIP_MARGE;
            let largeurs: Vec<f32> = zone
                .chips
                .iter()
                .filter_map(|c| c.as_ref().map(|l| l.w + 2.0 * CHIP_PAD_X + 2.0 * CHIP_BORD))
                .collect();
            let total_chips = largeurs.iter().sum::<f32>() + CHIP_GAP * (largeurs.len() - 1) as f32;
            let mut cx = x + (ZONE_LARGEUR - total_chips) / 2.0;
            for (i, chip) in zone.chips.iter().enumerate() {
                let Some(chip) = chip else { continue };
                let largeur = chip.w + 2.0 * CHIP_PAD_X + 2.0 * CHIP_BORD;
                let rect = D2D_RECT_F {
                    left: cx,
                    top: y,
                    right: cx + largeur,
                    bottom: y + CHIP_PAD_Y * 2.0 + CHIP_BORD * 2.0 + CHIP_TAILLE,
                };
                let eteint = i == 2 && zone.special;
                let opacite = if eteint { 0.35 } else { 1.0 };
                self.remplir_arrondi(rect, 999.0, [10, 12, 20, 179])?;
                self.border_arrondi(rect, 999.0, [255, 255, 255, 56])?;
                self.dessiner_ligne_f(
                    chip,
                    cx + CHIP_PAD_X + CHIP_BORD,
                    y + CHIP_PAD_Y + CHIP_BORD,
                    None,
                    opacite,
                )?;
                cx += largeur + CHIP_GAP;
            }
            y += CHIP_PAD_Y * 2.0 + CHIP_BORD * 2.0 + CHIP_TAILLE;
        }

        // Timer local de la zone active (même fraction que le timer central).
        if actif && h_timer > 0.0 {
            y += TIMER_ZONE_MARGE;
            let fraction = if let PhaseFight::Combat {
                duree_tour_ms, ..
            } = &rendu.frame.phase
            {
                let duree = (*duree_tour_ms).max(1) as f32;
                (rendu.reste_tour_ms as f32 / duree).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let rect = D2D_RECT_F {
                left: x + (ZONE_LARGEUR - PV_LARGEUR) / 2.0,
                top: y,
                right: x + (ZONE_LARGEUR - PV_LARGEUR) / 2.0 + PV_LARGEUR,
                bottom: y + TIMER_ZONE_HAUTEUR + TIMER_BORD * 2.0,
            };
            self.remplir_arrondi(rect, 3.0, [0, 0, 0, 140])?;
            self.border_arrondi(rect, 3.0, [255, 255, 255, 46])?;
            if fraction > 0.0 {
                let rempli = D2D_RECT_F {
                    left: rect.left + TIMER_BORD,
                    top: rect.top + TIMER_BORD,
                    right: rect.left + TIMER_BORD + (PV_LARGEUR - 2.0 * TIMER_BORD) * fraction,
                    bottom: rect.bottom - TIMER_BORD,
                };
                let brosse = self.brosse_horizontale(rempli, TIMER_STOPS.0, TIMER_STOPS.1)?;
                unsafe { self.cible_rt.FillRectangle(&rempli, &brosse) };
            }
        }

        // Badge « À TOI » (zone active) — au-dessus de la zone.
        if actif {
            if let Some(badge) = &zone.badge {
                let largeur = badge.w + 2.0 * BADGE_PAD_X;
                let rect = D2D_RECT_F {
                    left: x + ZONE_LARGEUR / 2.0 - largeur / 2.0,
                    top: bas - total + BADGE_TOP,
                    right: x + ZONE_LARGEUR / 2.0 + largeur / 2.0,
                    bottom: bas - total + BADGE_TOP + BADGE_PAD_Y * 2.0 + BADGE_TAILLE,
                };
                self.remplir_arrondi(rect, 999.0, [255, 210, 75, 255])?;
                self.dessiner_ligne_f(
                    badge,
                    rect.left + BADGE_PAD_X,
                    rect.top + BADGE_PAD_Y,
                    None,
                    1.0,
                )?;
            }
        }

        // Impacts (`.fight-fx`) puis popups de dégâts / soin / raté.
        self.peindre_fx(rendu, cote, x, bas - total, y_emoji)?;
        self.peindre_popups(rendu, cote, x, bas - total)?;
        Ok(())
    }

    /// Impact `.fight-fx` (💥 / 💢 / 🩸 / ✨) — 0,5 s, jamais en `anims_reduites`.
    fn peindre_fx(
        &mut self,
        rendu: &RenduFight<'_>,
        cote: char,
        x: f32,
        haut_zone: f32,
        y_emoji: f32,
    ) -> Result<(), ErreurTexte> {
        if rendu.frame.anims_reduites {
            return Ok(());
        }
        let Some((coup, t)) = rendu.action else {
            return Ok(());
        };
        if coup.coup == "passe" {
            return Ok(());
        }
        let attaquant = coup.acteur == cote;
        // (kind, texte, taille, départ) — `fightImpact` du DOM.
        let (texte, taille, depart) = if attaquant {
            if coup.soin > 0 {
                ("✨", FX_TAILLE * 0.81, SHAKE_RETARD_S)
            } else {
                return Ok(());
            }
        } else if !coup.touche {
            return Ok(());
        } else if coup.coup == "special" {
            ("🩸", FX_TAILLE * 1.19, SHAKE_RETARD_S)
        } else if coup.critique {
            ("💢", FX_TAILLE * 1.37, SHAKE_RETARD_S)
        } else {
            ("💥", FX_TAILLE, SHAKE_RETARD_S)
        };
        let ecoule = t - depart;
        if ecoule < 0.0 || ecoule >= FX_S {
            return Ok(());
        }
        // `fight-fx-pop` : 0 % (α 0, ×0,4, −12°) → 30 % (α 1, ×1,2, 4°) →
        // 100 % (α 0, ×1, 0°), centre du wrap à `top: 42 %`.
        let k = ecoule / FX_S;
        let (echelle, rotation, opacite, dy) = if k < 0.3 {
            let e = ease_in_out(k / 0.3);
            (0.4 + 0.8 * e, -12.0 + 16.0 * e, e, 0.0)
        } else {
            let e = ease_in_out((k - 0.3) / 0.7);
            (1.2 - 0.2 * e, 4.0 - 4.0 * e, 1.0 - e, -0.35 * e)
        };
        let ligne = self.ligne_f(SpecF {
            texte,
            famille: "Segoe UI Emoji",
            taille,
            poids: DWRITE_FONT_WEIGHT_BOLD,
            couleur: BLANC,
            largeur: taille * 2.0,
            wrap: false,
            centre: false,
        })?;
        let cx = x + ZONE_LARGEUR / 2.0;
        let cy = y_emoji + (haut_zone - y_emoji) * 0.42 + dy * 54.0;
        let cos = rotation.to_radians().cos();
        let sin = rotation.to_radians().sin();
        let mtx = Matrix3x2 {
            M11: echelle * cos,
            M12: echelle * sin,
            M21: -echelle * sin,
            M22: echelle * cos,
            M31: cx * (1.0 - echelle * cos) + cy * echelle * sin,
            M32: cy * (1.0 - echelle * cos) - cx * echelle * sin,
        };
        unsafe { self.cible_rt.SetTransform(&mtx) };
        self.dessiner_ligne_f(&ligne, cx - ligne.w / 2.0, cy - ligne.h / 2.0, None, opacite)?;
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        Ok(())
    }

    /// Popups `fight-dmg` au-dessus de l'emoji (dégâts, soin, raté, passe).
    fn peindre_popups(
        &mut self,
        rendu: &RenduFight<'_>,
        cote: char,
        x: f32,
        haut_zone: f32,
    ) -> Result<(), ErreurTexte> {
        let Some((coup, t)) = rendu.action else {
            return Ok(());
        };
        let attaquant = coup.acteur == cote;
        let (texte, taille, couleur, depart) = if coup.coup == "passe" && attaquant {
            ("⏳ passe…".to_string(), DMG_MISS_TAILLE, DMG_MISS, 0.0)
        } else if attaquant && coup.soin > 0 {
            (
                format!("+{}", coup.soin),
                DMG_TAILLE,
                DMG_SOIN,
                SHAKE_RETARD_S,
            )
        } else if !attaquant && coup.coup != "passe" {
            if !coup.touche {
                ("raté".to_string(), DMG_MISS_TAILLE, DMG_MISS, 0.0)
            } else if coup.degats > 0 {
                (
                    format!("-{}", coup.degats),
                    if coup.critique {
                        DMG_CRIT_TAILLE
                    } else {
                        DMG_TAILLE
                    },
                    if coup.critique { DMG_CRIT } else { DMG_COULEUR },
                    0.0,
                )
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };
        let ecoule = t - depart;
        if ecoule < 0.0 || ecoule >= POPUP_S {
            return Ok(());
        }
        // `fight-pop` : 0 % (α 0, scale 0,6) → 15 % (−16 px, ×1,15, α 1) →
        // 100 % (−70 px, ×1, α 0).
        let k = ecoule / POPUP_S;
        let (dy, echelle, opacite) = if k < 0.15 {
            let e = ease_in_out(k / 0.15);
            (-16.0 * e, 0.6 + 0.55 * e, e)
        } else {
            let e = ease_in_out((k - 0.15) / 0.85);
            (-16.0 - 54.0 * e, 1.15 - 0.15 * e, 1.0 - e)
        };
        let ligne = self.ligne_f(SpecF {
            texte: &texte,
            famille: "Segoe UI",
            taille,
            poids: DWRITE_FONT_WEIGHT_EXTRA_BOLD,
            couleur,
            largeur: ZONE_LARGEUR,
            wrap: false,
            centre: false,
        })?;
        let cx = x + ZONE_LARGEUR / 2.0;
        // `top: -14px` du wrap + `translate(-50%, …)`.
        let cy = haut_zone - 14.0 + dy;
        let mtx = Matrix3x2 {
            M11: echelle,
            M12: 0.0,
            M21: 0.0,
            M22: echelle,
            M31: cx * (1.0 - echelle),
            M32: cy * (1.0 - echelle),
        };
        unsafe { self.cible_rt.SetTransform(&mtx) };
        self.dessiner_ligne_f(&ligne, cx - ligne.w / 2.0, cy, None, opacite)?;
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        Ok(())
    }

    /// Colonne centrale : pastille de tour, timer de tour, temps de match.
    fn peindre_centre(
        &mut self,
        rendu: &RenduFight<'_>,
        canvas: (u32, u32),
        _tour: char,
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let Some(bloc) = self.bloc_fight.as_ref() else {
            return Ok(());
        };
        let tour = bloc.tour.as_ref().map(|l| (l.layout.clone(), l.w, l.h));
        let match_txt = bloc.match_txt.as_ref().map(|l| (l.layout.clone(), l.w, l.h));
        // Hauteur totale de la colonne : pastille + gap + timer + gap + match.
        let h_tour = tour
            .as_ref()
            .map(|(_, _, lh)| lh + 2.0 * TOUR_PAD_Y)
            .unwrap_or(0.0);
        let h_timer = TIMER_HAUTEUR + 2.0 * TIMER_BORD;
        let h_match = match_txt.as_ref().map(|(_, _, lh)| *lh).unwrap_or(0.0);
        let total = h_tour + CENTRE_GAP + h_timer + CENTRE_GAP + h_match;
        let mut y = h - CENTRE_BAS * h - total;

        if let Some((layout, lw, lh)) = &tour {
            let largeur = lw + 2.0 * TOUR_PAD_X;
            let rect = D2D_RECT_F {
                left: w / 2.0 - largeur / 2.0,
                top: y,
                right: w / 2.0 + largeur / 2.0,
                bottom: y + lh + 2.0 * TOUR_PAD_Y,
            };
            self.remplir_arrondi(rect, 999.0, TOUR_FOND)?;
            self.border_arrondi(rect, 999.0, TOUR_BORD)?;
            let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(BLANC), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse tour : {}", e.message())))?;
            unsafe {
                self.ctx.DrawTextLayout(
                    windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                        x: rect.left + TOUR_PAD_X,
                        y: y + TOUR_PAD_Y,
                    },
                    layout,
                    &brosse,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                )
            };
            y += lh + 2.0 * TOUR_PAD_Y + CENTRE_GAP;
        }

        // Timer de tour : fraction `reste / duree`.
        let fraction = match &rendu.frame.phase {
            PhaseFight::Combat {
                duree_tour_ms, ..
            } => {
                let duree = (*duree_tour_ms).max(1) as f32;
                (rendu.reste_tour_ms as f32 / duree).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };
        let rect = D2D_RECT_F {
            left: w / 2.0 - TIMER_LARGEUR / 2.0,
            top: y,
            right: w / 2.0 + TIMER_LARGEUR / 2.0,
            bottom: y + TIMER_HAUTEUR + 2.0 * TIMER_BORD,
        };
        self.remplir_arrondi(rect, TIMER_RAYON, [0, 0, 0, 140])?;
        self.border_arrondi(rect, TIMER_RAYON, [255, 255, 255, 46])?;
        if fraction > 0.0 {
            let rempli = D2D_RECT_F {
                left: rect.left + TIMER_BORD,
                top: rect.top + TIMER_BORD,
                right: rect.left + TIMER_BORD + (TIMER_LARGEUR - 2.0 * TIMER_BORD) * fraction,
                bottom: rect.bottom - TIMER_BORD,
            };
            let brosse = self.brosse_horizontale(rempli, TIMER_STOPS.0, TIMER_STOPS.1)?;
            unsafe { self.cible_rt.FillRectangle(&rempli, &brosse) };
        }
        y += TIMER_HAUTEUR + 2.0 * TIMER_BORD + CENTRE_GAP;

        if let Some((layout, lw, _)) = &match_txt {
            let brosse = unsafe {
                self.cible_rt
                    .CreateSolidColorBrush(&couleur_d2d([255, 255, 255, 204]), None)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse match : {}", e.message())))?;
            unsafe {
                self.ctx.DrawTextLayout(
                    windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                        x: w / 2.0 - lw / 2.0,
                        y,
                    },
                    layout,
                    &brosse,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                )
            };
        }
        Ok(())
    }

    /// Bannière (lobby ou fin) : emoji + titre + sous-titre.
    fn peindre_banniere(
        &mut self,
        rendu: &RenduFight<'_>,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let Some(bloc) = self.bloc_fight.as_ref() else {
            return Ok(());
        };
        let emoji = bloc
            .banniere_emoji
            .as_ref()
            .map(|l| (l.layout.clone(), l.w, l.h));
        let titre = bloc
            .banniere_titre
            .as_ref()
            .map(|l| (l.layout.clone(), l.w, l.h));
        let sous = bloc
            .banniere_sous
            .as_ref()
            .map(|l| (l.layout.clone(), l.w, l.h));
        let _ = rendu;
        // Largeur de la boîte : le plus large des contenus + padding.
        let largeur_contenu = titre
            .as_ref()
            .map(|(_, lw, _)| *lw)
            .unwrap_or(0.0)
            .max(sous.as_ref().map(|(_, lw, _)| *lw).unwrap_or(0.0))
            .max(emoji.as_ref().map(|(_, lw, _)| *lw).unwrap_or(0.0))
            .min(BANNIERE_MAX * w - 2.0 * BANNIERE_PAD_X);
        let hauteur_contenu = emoji.as_ref().map(|(_, _, lh)| *lh + 4.0).unwrap_or(0.0)
            + titre.as_ref().map(|(_, _, lh)| *lh).unwrap_or(0.0)
            + sous.as_ref().map(|(_, _, lh)| *lh + 5.0).unwrap_or(0.0);
        let bw = largeur_contenu + 2.0 * BANNIERE_PAD_X;
        let bh = hauteur_contenu + 2.0 * BANNIERE_PAD_Y;
        let rect = D2D_RECT_F {
            left: w / 2.0 - bw / 2.0,
            top: h - BANNIERE_BAS * h - bh,
            right: w / 2.0 + bw / 2.0,
            bottom: h - BANNIERE_BAS * h,
        };
        self.remplir_arrondi(rect, BANNIERE_RAYON, BANNIERE_FOND)?;
        self.border_arrondi(rect, BANNIERE_RAYON, BANNIERE_BORD)?;
        // Les lignes sont centrées dans leur boîte de mise en page : origine =
        // centre du canvas − moitié de la largeur de la boîte.
        let largeur_boite = BANNIERE_MAX * w - 2.0 * BANNIERE_PAD_X;
        let mut y = rect.top + BANNIERE_PAD_Y;
        if let Some((layout, _lw, lh)) = &emoji {
            let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(BLANC), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse bannière : {}", e.message())))?;
            unsafe {
                self.ctx.DrawTextLayout(
                    windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                        x: w / 2.0 - BANNIERE_EMOJI,
                        y,
                    },
                    layout,
                    &brosse,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                )
            };
            y += lh + 4.0;
        }
        if let Some((layout, _, lh)) = &titre {
            let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(BLANC), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse bannière : {}", e.message())))?;
            unsafe {
                self.ctx.DrawTextLayout(
                    windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                        x: w / 2.0 - largeur_boite / 2.0,
                        y,
                    },
                    layout,
                    &brosse,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                )
            };
            y += lh;
        }
        if let Some((layout, _, lh)) = &sous {
            let brosse = unsafe {
                self.cible_rt
                    .CreateSolidColorBrush(&couleur_d2d([255, 255, 255, 217]), None)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse sous-titre : {}", e.message())))?;
            unsafe {
                self.ctx.DrawTextLayout(
                    windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                        x: w / 2.0 - largeur_boite / 2.0,
                        y: y + 5.0,
                    },
                    layout,
                    &brosse,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                )
            };
            let _ = lh;
        }
        Ok(())
    }

    /// Toast d'info (top 3 %, centré) — bord coloré selon `kind`.
    fn peindre_info(
        &mut self,
        rendu: &RenduFight<'_>,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let Some(inf) = &rendu.info else {
            return Ok(());
        };
        let Some(bloc) = self.bloc_fight.as_ref() else {
            return Ok(());
        };
        let Some(ligne) = bloc.info.as_ref() else {
            return Ok(());
        };
        let (layout, lw, lh) = (ligne.layout.clone(), ligne.w, ligne.h);
        let aide = inf.info.kind == "aide";
        let pad_y = if aide { INFO_AIDE_PAD_Y } else { INFO_PAD_Y };
        let bw = lw.min(INFO_MAX * w - 2.0 * INFO_PAD_X) + 2.0 * INFO_PAD_X;
        let bh = lh + 2.0 * pad_y;
        let rect = D2D_RECT_F {
            left: w / 2.0 - bw / 2.0,
            top: INFO_TOP * h,
            right: w / 2.0 + bw / 2.0,
            bottom: INFO_TOP * h + bh,
        };
        // Transition : opacité 0,18 s + translation −8 px → 0.
        let dy = -8.0 * (1.0 - inf.opacite.clamp(0.0, 1.0));
        let opacite = inf.opacite.clamp(0.0, 1.0);
        let rect = D2D_RECT_F {
            left: rect.left,
            top: rect.top + dy,
            right: rect.right,
            bottom: rect.bottom + dy,
        };
        let fond = [
            INFO_FOND[0],
            INFO_FOND[1],
            INFO_FOND[2],
            (INFO_FOND[3] as f32 * opacite) as u8,
        ];
        self.remplir_arrondi(rect, INFO_RAYON, fond)?;
        let bord = match inf.info.kind.as_str() {
            "warn" => INFO_WARN_BORD,
            "ok" => INFO_OK_BORD,
            "aide" => INFO_AIDE_BORD,
            _ => INFO_BORD,
        };
        let bord = [
            bord[0],
            bord[1],
            bord[2],
            (bord[3] as f32 * opacite) as u8,
        ];
        self.border_arrondi(rect, INFO_RAYON, bord)?;
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(BLANC), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse info : {}", e.message())))?;
        unsafe {
            self.ctx.DrawTextLayout(
                windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F {
                    x: w / 2.0 - (INFO_MAX * w - 2.0 * INFO_PAD_X) / 2.0,
                    y: rect.top + pad_y,
                },
                &layout,
                &brosse,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            )
        };
        Ok(())
    }

    /// Brosse linéaire horizontale (barres de PV et timers).
    fn brosse_horizontale(
        &self,
        rect: D2D_RECT_F,
        gauche: [u8; 4],
        droite: [u8; 4],
    ) -> Result<windows::Win32::Graphics::Direct2D::ID2D1Brush, ErreurTexte> {
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D1_GRADIENT_STOP, D2D_POINT_2F,
        };
        use windows::Win32::Graphics::Direct2D::{
            ID2D1GradientStopCollection, ID2D1LinearGradientBrush,
            D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
        };
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: couleur_d2d(gauche),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: couleur_d2d(droite),
            },
        ];
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé fight : {}", e.message())))?;
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
            unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse dégradé fight : {}", e.message()))
                })?;
        Ok(brosse.into())
    }
}

/// Produit de deux matrices Direct2D (convention ligne : `p·(a·b)` =
/// `(p·a)·b` — « appliquer a puis b »).
fn mul_mtx(a: Matrix3x2, b: Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}

/// Échelle uniforme autour de `(cx, cy)`.
fn echelle_autour(cx: f32, cy: f32, echelle: f32) -> Matrix3x2 {
    let e = echelle.max(0.01);
    Matrix3x2 {
        M11: e,
        M12: 0.0,
        M21: 0.0,
        M22: e,
        M31: cx * (1.0 - e),
        M32: cy * (1.0 - e),
    }
}

/// Rotation `deg` (degrés, sens horaire écran) autour de `(cx, cy)`.
fn rotation_autour(cx: f32, cy: f32, deg: f32) -> Matrix3x2 {
    if deg.abs() < 1e-4 {
        return Matrix3x2::identity();
    }
    let rad = deg.to_radians();
    let (c, s) = (rad.cos(), rad.sin());
    Matrix3x2 {
        M11: c,
        M12: s,
        M21: -s,
        M22: c,
        M31: cx - cx * c + cy * s,
        M32: cy - cx * s - cy * c,
    }
}

/// Miroir horizontal (`scaleX(-1)`) autour de l'axe `x = cx`.
fn miroir_horizontal(cx: f32) -> Matrix3x2 {
    Matrix3x2 {
        M11: -1.0,
        M12: 0.0,
        M21: 0.0,
        M22: 1.0,
        M31: 2.0 * cx,
        M32: 0.0,
    }
}

/// Translation simple.
fn translation(dx: f32, dy: f32) -> Matrix3x2 {
    Matrix3x2 {
        M11: 1.0,
        M12: 0.0,
        M21: 0.0,
        M22: 1.0,
        M31: dx,
        M32: dy,
    }
}

/// Nom de commande non vide (repli `a`/`b`/`c` du DOM).
fn nom_ou<'a>(valeur: &'a str, defaut: &'a str) -> &'a str {
    if valeur.is_empty() {
        defaut
    } else {
        valeur
    }
}

/// Suffixe du titre de victoire (`🏆 X gagne …`).
fn suffixe_victoire(raison: &str) -> &'static str {
    match raison {
        "abandon" => " gagne par abandon !",
        "temps" => " gagne au temps !",
        _ => " gagne !",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn joueur(pseudo: &str, pv: u32, max: u32) -> JoueurFight {
        JoueurFight {
            pseudo: pseudo.to_string(),
            plateforme: "twitch".to_string(),
            emoji: "🐲".to_string(),
            pv,
            pv_max: max,
            special_utilise: false,
        }
    }

    fn combat(reste_tour_ms: u64) -> FightState {
        FightState {
            phase: PhaseFight::Combat {
                ja: joueur("Manu", 80, 100),
                jb: joueur("Ryu", 40, 100),
                tour: 'a',
                reste_tour_ms,
                duree_tour_ms: 15_000,
                reste_match_s: 90,
                derniere: Some(CoupFight {
                    seq: 1,
                    acteur: 'a',
                    coup: "rapide".to_string(),
                    degats: 12,
                    soin: 0,
                    touche: true,
                    critique: false,
                }),
                cmds: CmdsFight::default(),
            },
            info: None,
            anims_reduites: false,
        }
    }

    #[test]
    fn pourcentage_et_couleurs_de_pv() {
        assert_eq!(pourcentage_pv(80, 100), 80.0);
        assert_eq!(pourcentage_pv(0, 100), 0.0);
        assert_eq!(pourcentage_pv(5, 0), 5.0, "max 0 → 100 (défaut du DOM)");
        assert_eq!(pourcentage_pv(150, 100), 100.0, "borné à 100");
        assert_eq!(couleurs_pv(80.0), PV_HAUT);
        assert_eq!(couleurs_pv(55.0), PV_MOYEN, "≤ 55 = moyen");
        assert_eq!(couleurs_pv(26.0), PV_MOYEN);
        assert_eq!(couleurs_pv(25.0), PV_BAS, "≤ 25 = bas");
    }

    #[test]
    fn match_formate_en_m_ss() {
        assert_eq!(formater_match(0), "0:00");
        assert_eq!(formater_match(9), "0:09");
        assert_eq!(formater_match(90), "1:30");
        assert_eq!(formater_match(600), "10:00");
    }

    #[test]
    fn idle_invisible_puis_combat_visible() {
        let mut etat = EtatFight::nouveau();
        assert!(!etat.visible(), "idle au boot");
        let t0 = Instant::now();
        etat.appliquer_at(combat(5_000), t0);
        assert_eq!(etat.version(), 1);
        assert!(etat.visible());
        // Échéance décomptée en local (pas de nouvelle frame).
        let r = etat.rendu_at(t0 + Duration::from_millis(2_000));
        assert_eq!(r.reste_tour_ms, 3_000);
        // Le temps de match est décompté localement lui aussi (fightTick).
        assert_eq!(r.reste_match_s, Some(88));
        // Une nouvelle frame recale l'échéance.
        etat.appliquer_at(combat(9_000), t0 + Duration::from_millis(2_000));
        let r = etat.rendu_at(t0 + Duration::from_millis(2_500));
        assert_eq!(r.reste_tour_ms, 8_500);
    }

    #[test]
    fn action_dedupliquee_par_seq() {
        let mut etat = EtatFight::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(combat(5_000), t0);
        let r = etat.rendu_at(t0 + Duration::from_millis(100));
        let (coup, t) = r.action.expect("action animée");
        assert_eq!(coup.seq, 1);
        assert!((t - 0.1).abs() < 1e-3);
        // Resync : même seq → l'animation ne repart pas.
        etat.appliquer_at(combat(4_000), t0 + Duration::from_millis(500));
        let r = etat.rendu_at(t0 + Duration::from_millis(600));
        let (_, t) = r.action.expect("toujours la même action");
        assert!((t - 0.6).abs() < 1e-3, "chrono d'animation conservé");
    }

    #[test]
    fn info_agee_non_rejouee() {
        let mut etat = EtatFight::nouveau();
        let t0 = Instant::now();
        let mut frame = combat(5_000);
        frame.info = Some(InfoFight {
            seq: 7,
            kind: "ok".to_string(),
            texte: "Combat lancé".to_string(),
            duree_ms: 2_000,
            age_ms: 5_000, // plus vieux que sa durée → ignoré
        });
        etat.appliquer_at(frame.clone(), t0);
        assert!(etat.rendu_at(t0).info.is_none(), "message trop vieux");
        // Message frais : visible puis éteint à sa durée.
        frame.info = Some(InfoFight {
            seq: 8,
            kind: "ok".to_string(),
            texte: "Combat lancé".to_string(),
            duree_ms: 2_000,
            age_ms: 0,
        });
        etat.appliquer_at(frame, t0);
        assert!(etat.rendu_at(t0 + Duration::from_millis(100)).info.is_some());
        assert!(
            etat.rendu_at(t0 + Duration::from_millis(2_100)).info.is_none(),
            "toast éteint après sa durée"
        );
    }

    #[test]
    fn phase_fin_arret_et_gagnant() {
        let mut etat = EtatFight::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(
            FightState {
                phase: PhaseFight::Fin {
                    ja: joueur("Manu", 80, 100),
                    jb: joueur("Ryu", 0, 100),
                    gagnant: Some('a'),
                    raison: "ko".to_string(),
                },
                info: None,
                anims_reduites: true,
            },
            t0,
        );
        assert!(etat.visible(), "la bannière de fin reste affichée");
        let r = etat.rendu_at(t0 + Duration::from_millis(100));
        assert!(r.ko_depuis.is_some(), "le KO démarre à l'entrée en `fin`");
        assert!(r.frame.anims_reduites);
        // Retour en idle : tout est éteint.
        etat.appliquer_at(FightState::default(), t0 + Duration::from_millis(500));
        assert!(!etat.visible());
        assert!(etat.rendu_at(t0 + Duration::from_millis(600)).ko_depuis.is_none());
    }

    #[test]
    fn animations_bornees_et_symetriques() {
        // Lunge : 0 au départ, 1 au pic (40 %), retour à 0.
        assert!(progression_3(0.0, LUNGE_S).abs() < 1e-3);
        assert!((progression_3(0.4 * LUNGE_S, LUNGE_S) - 1.0).abs() < 1e-2);
        assert!(progression_3(LUNGE_S, LUNGE_S).abs() < 1e-2);
        // Secousse : nulle aux extrémités, bornée à ±8 px.
        assert!(secousse(0.0, SHAKE_S).abs() < 1e-3);
        assert!(secousse(SHAKE_S, SHAKE_S).abs() < 1e-3);
        let mut max = 0.0_f32;
        for i in 0..=100 {
            max = max.max(secousse(i as f32 / 100.0 * SHAKE_S, SHAKE_S).abs());
        }
        assert!(max <= 8.0 + 1e-3, "secousse bornée : {max}");
    }

    #[test]
    fn nom_de_commande_avec_repli() {
        assert_eq!(nom_ou("", "a"), "a");
        assert_eq!(nom_ou("frappe", "a"), "frappe");
        assert_eq!(suffixe_victoire("abandon"), " gagne par abandon !");
        assert_eq!(suffixe_victoire("temps"), " gagne au temps !");
        assert_eq!(suffixe_victoire("ko"), " gagne !");
    }
}
