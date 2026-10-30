//! Widget « speedrun » natif — splitter ASL : en-tête jeu, timer, phase,
//! barre de progression et lignes de splits.
//!
//! Miroir de `.widget.speedrun` et de `majSpeedrunEtat` / `srMajHeader` /
//! `srMajTimerEtPhase` / `srMajSegmentsVisuel` / `srRecreerSplits`
//! (diffusion.html) — le DOM diffuse le même rendu que le dashboard
//! (WidgetSpeedrun) :
//! - en-tête : titre du jeu (0,75 em, gras) centré, catégorie (0,7 em,
//!   `--gris`) dessous, pastille de statut en coin (● `#22c55e` connecté,
//!   ○ `#e0a060` en attente), placeholder italique « Aucun splits chargé » ;
//! - timer : valeur 1,9 em gras (espacement 0,04 em) sur fond teinté de
//!   l'accent à 15 %, couleur `srCouleurTimer` ou `--sr-grad-from`, ombre de
//!   contraste (`contrasteOmbre`) ;
//! - meta : pastille de phase (Ready/Running/Paused/Finished, couleurs
//!   sémantiques) + compteur « idx / total » (0,7 em) ;
//! - barre : piste pilule 0,2 em, remplissage dégradé `--sr-grad-*` sur
//!   `idx / total` ;
//! - splits : nom | temps | delta — fond dégradé 135° (accent à 35 % →
//!   noir), split actif = dégradé plein + bordure `--sr-couleur-actif`,
//!   terminé = alpha 0,6, bordure de résultat ahead/behind, fenêtre
//!   `srNbLignes` et scroll centré sur l'actif (`srScrollAuto`) ;
//! - mode « ligne » (`srLayout` ligne, ou auto sous 4 × taille) : splits en
//!   ligne horizontale à gauche, timer 1,4 em épinglé à droite, en-tête /
//!   meta / barre masqués.
//!
//! L'état arrive par [`EtatSpeedrun`] : l'app décode les frames WS
//! `speedrun-etat` (moteur ASL → `chat_tx`) et pousse chaque event pendant
//! la session — même motif qu'`EtatGoals` : jamais de reconfiguration de
//! scène pour un tick du timer. Les métadonnées LSS y persistent : un
//! widget recréé par `appliquer_scene` retrouve jeu + splits tout seul
//! (équivalent des `pendingLss`/`pendingEtat` de la diffusion).
//!
//! Limites assumées :
//! - pas de pop de validation (splitpop), de son, de métronome ni de clic
//!   sur les lignes (hors périmètre du lot) ;
//! - scroll = centrage instantané sur le split actif (translation + clip) —
//!   ni scrollbar ni molette ;
//! - `drop-shadow`/`box-shadow` lumineux (timer running/ended, split actif)
//!   non portés — limite D2D partagée avec goal (pas de filtre sur le
//!   contenu déjà composé) : seule l'ombre de lisibilité est dessinée ;
//! - `backdrop-filter: blur(8px)` du timer ≈ le fond teinté seul ;
//! - font-weight 600 ≈ gras (`StyleTexte` n'a que 400/700), 500 ≈ normal ;
//! - `font-variant-numeric: tabular-nums` non porté ;
//! - split « done » : alpha modulée par couleur (visuellement identique à
//!   `opacity: 0.6` du DOM sur fond opaque) ;
//! - `text-overflow: ellipsis` = troncature DirectWrite réelle (signe « … ») ;
//! - `toFixed` JS ≈ formatage Rust (arrondis possibles différents à
//!   l'extrême limite — les deltas sont déjà arrondis au dixième).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;

use windows::core::Interface;
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
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1,
    DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::cadres::CadreCfg;

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

/// `--texte` de la page diffusion (#e0e0e0).
const TEXTE: [u8; 4] = [0xe0, 0xe0, 0xe0, 255];
/// `--gris` (#9a9a9a).
const GRIS: [u8; 4] = [0x9a, 0x9a, 0x9a, 255];
/// `--message-ok-color` (#22c55e) — phase running/ended, pastille connecté,
/// défaut du delta « ahead » (`srCouleurAhead` le surcharge via la cfg).
const OK: [u8; 4] = [0x22, 0xc5, 0x5e, 255];
/// `--message-user-action-color` (#e0a060) — phase paused, pastille attente.
const ATTENTE: [u8; 4] = [0xe0, 0xa0, 0x60, 255];
/// `--rayon-petit` (5 px) — rayon du timer et des splits par défaut.
const RAYON_PETIT: f32 = 5.0;
/// Valeur du timer à l'arrêt (initialisation du widget DOM et resets).
const TEMPS_ZERO: &str = "00:00.00";
/// Placeholder de l'en-tête tant qu'aucun .lss n'est chargé.
const PLACEHOLDER: &str = "Aucun splits chargé";
/// `opacity: 0.6` du split terminé (`.sr-seg-done`).
const ALPHA_DONE: f32 = 0.6;

// =============================================================================
// ÉTAT LIVE — miroir de l'`entry` de diffusion.html alimentée par les frames
// `speedrun-etat` du canal `chat_tx` (speedrun/commands.rs → moteur ASL).
// =============================================================================

/// Phase du timer — `srPhase` de l'entry (`TimerPhase` Rust sérialisé).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PhaseSpeedrun {
    /// `NotRunning` — et toute phase inconnue (`srPhaseCls` → « ready »).
    #[default]
    NotRunning,
    Running,
    Paused,
    Ended,
}

impl PhaseSpeedrun {
    /// Traduit le nom de phase de l'event (`goto`, `etat`).
    pub fn depuis_nom(nom: &str) -> Self {
        match nom {
            "Running" => Self::Running,
            "Paused" => Self::Paused,
            "Ended" => Self::Ended,
            _ => Self::NotRunning,
        }
    }

    /// Libellé de la pastille (`srPhaseLabel` — uppercased au rendu par
    /// `text-transform`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Ended => "Finished",
            Self::NotRunning => "Ready",
        }
    }

    /// Couleur de texte et alpha de fond de la pastille (`srPhaseCls`).
    fn couleurs(self) -> ([u8; 4], f32) {
        match self {
            Self::Running => (OK, 0.15),
            Self::Paused => (ATTENTE, 0.15),
            Self::Ended => (OK, 0.20),
            Self::NotRunning => (GRIS, 0.15),
        }
    }

    /// `Running || Paused` — chrono vivant (delta/temps de la ligne active).
    fn en_run(self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }
}

/// Un segment du fichier .lss — frame `lss` (`emit_ws_lss_json`).
/// Les PB sont stockés en **cumul** (comme le .lss source), secondes.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentLss {
    pub nom: String,
    pub pb_real_time: Option<f64>,
    pub pb_game_time: Option<f64>,
}

/// Une mutation de l'état speedrun — une variante par type d'event de la
/// frame `{"type":"speedrun-etat","etat":{…}}` (les events sans effet
/// visuel — `error`, `settings-list`, `loaded`, `saved` — ne sont pas
/// portés, comme le `default:` du switch DOM).
#[derive(Debug, Clone, PartialEq)]
pub enum MajSpeedrun {
    /// `{"type":"started","index":…}` — nouvelle tentative.
    Started { index: Option<i32> },
    /// `{"type":"split","time":…,"cumul":…,"index":…}` — index = split
    /// courant **après** incrément (segment bouclé = index − 1).
    Split {
        time: Option<String>,
        cumul: Option<String>,
        index: Option<i32>,
    },
    /// `{"type":"splitSkipped","index":…}`.
    SplitSkipped { index: Option<i32> },
    /// `{"type":"splitUndone","index":…}` — index = split défait (0-based).
    SplitUndone { index: Option<i32> },
    /// `{"type":"reset"}`.
    Reset,
    /// `{"type":"ended","time":…,"cumul":…,"index":…}` — index = total.
    Ended {
        time: Option<String>,
        cumul: Option<String>,
        index: Option<i32>,
    },
    /// `{"type":"goto","index":…,"phase":…}` — armement ou retry de segment.
    Goto {
        index: Option<i32>,
        phase: Option<PhaseSpeedrun>,
    },
    /// `{"type":"etat",…}` — resync complet (reconnexion WS).
    Etat {
        phase: PhaseSpeedrun,
        index: Option<i32>,
        split_times: Vec<Option<String>>,
        split_cumuls: Vec<Option<String>>,
        real_time: Option<String>,
        game_time: Option<String>,
        segment_time: Option<String>,
    },
    /// `{"type":"gameTime","time":…}`.
    GameTime { time: String },
    /// `{"type":"time",…}` — tick du timer (~15 Hz en run). `segment_time`
    /// est `Some(_)` quand le champ était présent dans l'event (`!== undefined`)
    /// — `Some(None)` = segmentTime null → effacé.
    Time {
        real_time: Option<String>,
        game_time: Option<String>,
        segment_time: Option<Option<String>>,
    },
    /// `{"type":"paused"}` / `{"type":"resumed"}`.
    Paused,
    Resumed,
    /// `{"type":"stopped"}` — splitter arrêté (état complet effacé).
    Stopped,
    /// `{"type":"connected","process":…,"pid":…}` — jeu attaché.
    Connected { processus: Option<String> },
    /// `{"type":"disconnected"}` — sortie du jeu (même reset que `reset`).
    Disconnected,
    /// `{"type":"lss","nomJeu":…,"nomCategorie":…,"segments":[…]}`.
    Lss {
        nom_jeu: String,
        nom_categorie: String,
        segments: Vec<SegmentLss>,
    },
}

/// État live du splitter — miroir des `entry.sr*` de la diffusion (un seul
/// état partagé : le DOM applique chaque event à **tous** les widgets
/// speedrun, le partage produit le même affichage).
///
/// `version` augmente à chaque event appliqué (diagnostic / tests) — les
/// blocs de texte se resynchronisent par comparaison de contenu, pas par
/// version (tick 15 Hz : les chaînes inchangées ne coûtent rien).
#[derive(Debug)]
pub struct EtatSpeedrun {
    phase: PhaseSpeedrun,
    /// Index du split courant (0-based, armé inclus) ; `None` = `null`.
    index: Option<i32>,
    real_time: String,
    game_time: Option<String>,
    /// Temps vivant du segment courant (`srSegTime`) ; `None` = `null`.
    seg_time: Option<String>,
    nom_jeu: String,
    nom_categorie: String,
    /// Jeu attaché (●) — events `connected`/`disconnected`.
    connecte: bool,
    /// Splitter en marche (`started` → true, `stopped` → false — l'event
    /// `disconnected` ne le touche PAS, miroir du DOM).
    actif: bool,
    processus: Option<String>,
    /// Durées de segment enregistrées (index = split bouclé, "" = vide).
    split_times: Vec<String>,
    /// Cumuls de run aux splits réalisés (base du delta cumulé LiveSplit).
    split_cumuls: Vec<Option<String>>,
    /// Segments du .lss (PB en **cumul**, secondes).
    segments: Vec<SegmentLss>,
    version: u64,
}

impl Default for EtatSpeedrun {
    fn default() -> Self {
        Self::nouveau()
    }
}

impl EtatSpeedrun {
    /// État initial — miroir de l'entry créée par `renderScene`.
    pub fn nouveau() -> Self {
        Self {
            phase: PhaseSpeedrun::NotRunning,
            index: None,
            real_time: TEMPS_ZERO.to_string(),
            game_time: None,
            seg_time: None,
            nom_jeu: String::new(),
            nom_categorie: String::new(),
            connecte: false,
            actif: false,
            processus: None,
            split_times: Vec::new(),
            split_cumuls: Vec::new(),
            segments: Vec::new(),
            version: 0,
        }
    }

    /// Applique un event — miroir exact du `switch (etat.type)` de
    /// `majSpeedrunEtat`.
    pub fn appliquer(&mut self, maj: MajSpeedrun) {
        match maj {
            MajSpeedrun::Started { index } => {
                self.phase = PhaseSpeedrun::Running;
                self.actif = true;
                self.index = Some(index.unwrap_or(0));
                self.split_times.clear();
                self.split_cumuls.clear();
                self.seg_time = Some(TEMPS_ZERO.to_string());
            }
            MajSpeedrun::Split {
                time,
                cumul,
                index,
            } => {
                if let Some(i) = index {
                    self.index = Some(i);
                    // Segment bouclé = index − 1 (index est post-incrément).
                    // i ≥ 1 : un index 0 n'a pas de segment précédent (le
                    // DOM écrirait en propriété « −1 », sans effet visible).
                    if i >= 1 {
                        let j = (i - 1) as usize;
                        if let Some(t) = time.filter(|t| !t.is_empty()) {
                            self.split_times.resize(j + 1, String::new());
                            self.split_times[j] = t;
                        }
                        if let Some(c) = cumul.filter(|c| !c.is_empty()) {
                            self.split_cumuls.resize(j + 1, None);
                            self.split_cumuls[j] = Some(c);
                        }
                    }
                }
                self.seg_time = Some(TEMPS_ZERO.to_string());
            }
            MajSpeedrun::SplitSkipped { index } => {
                if let Some(i) = index {
                    self.index = Some(i);
                }
                self.seg_time = Some(TEMPS_ZERO.to_string());
            }
            MajSpeedrun::SplitUndone { index } => {
                if let Some(i) = index {
                    self.index = Some(i);
                    if i >= 0 {
                        let j = i as usize;
                        if j < self.split_times.len() {
                            self.split_times[j] = String::new();
                        }
                        if j < self.split_cumuls.len() {
                            self.split_cumuls[j] = None;
                        }
                    }
                }
                self.seg_time = None;
            }
            MajSpeedrun::Reset => {
                self.phase = PhaseSpeedrun::NotRunning;
                self.index = None;
                self.game_time = None;
                self.real_time = TEMPS_ZERO.to_string();
                self.split_times.clear();
                self.split_cumuls.clear();
                self.seg_time = None;
            }
            MajSpeedrun::Ended {
                time,
                cumul,
                index,
            } => {
                self.phase = PhaseSpeedrun::Ended;
                if let Some(i) = index {
                    self.index = Some(i);
                    if i >= 1 {
                        let j = (i - 1) as usize;
                        if let Some(t) = time.filter(|t| !t.is_empty()) {
                            self.split_times.resize(j + 1, String::new());
                            self.split_times[j] = t;
                        }
                        if let Some(c) = cumul.filter(|c| !c.is_empty()) {
                            self.split_cumuls.resize(j + 1, None);
                            self.split_cumuls[j] = Some(c);
                        }
                    }
                }
                self.seg_time = None;
            }
            MajSpeedrun::Goto { index, phase } => {
                if let Some(p) = phase {
                    self.phase = p;
                }
                if let Some(i) = index {
                    self.index = Some(i);
                }
                self.split_times.clear();
                self.split_cumuls.clear();
                // Le DOM teste etat.phase (pas la phase consolidée).
                self.seg_time = phase
                    .filter(|p| p.en_run())
                    .map(|_| TEMPS_ZERO.to_string());
            }
            MajSpeedrun::Etat {
                phase,
                index,
                split_times,
                split_cumuls,
                real_time,
                game_time,
                segment_time,
            } => {
                self.phase = phase;
                self.index = index.filter(|i| *i >= 0);
                self.split_times = split_times
                    .into_iter()
                    .map(|t| t.unwrap_or_default())
                    .collect();
                self.split_cumuls = split_cumuls;
                if let Some(rt) = real_time.as_ref().filter(|t| !t.is_empty()) {
                    self.real_time = rt.clone();
                }
                self.game_time = game_time
                    .filter(|t| !t.is_empty())
                    .or_else(|| real_time.filter(|t| !t.is_empty()));
                self.seg_time = segment_time.filter(|t| !t.is_empty());
            }
            MajSpeedrun::GameTime { time } => {
                if !time.is_empty() {
                    self.game_time = Some(time);
                }
            }
            MajSpeedrun::Time {
                real_time,
                game_time,
                segment_time,
            } => {
                if let Some(rt) = real_time.as_ref().filter(|t| !t.is_empty()) {
                    self.real_time = rt.clone();
                }
                if let Some(gt) = game_time.filter(|t| !t.is_empty()) {
                    self.game_time = Some(gt);
                } else if let Some(rt) = real_time.filter(|t| !t.is_empty()) {
                    // gameTime absent/null → repli sur le real time (DOM).
                    self.game_time = Some(rt);
                }
                if let Some(st) = segment_time {
                    self.seg_time = st;
                }
            }
            MajSpeedrun::Paused => self.phase = PhaseSpeedrun::Paused,
            MajSpeedrun::Resumed => self.phase = PhaseSpeedrun::Running,
            MajSpeedrun::Stopped => {
                self.phase = PhaseSpeedrun::NotRunning;
                self.index = None;
                self.game_time = None;
                self.real_time = TEMPS_ZERO.to_string();
                self.actif = false;
                self.connecte = false;
                self.processus = None;
                self.split_times.clear();
                self.split_cumuls.clear();
                self.seg_time = None;
            }
            MajSpeedrun::Connected { processus } => {
                self.connecte = true;
                if let Some(p) = processus {
                    self.processus = Some(p);
                }
            }
            MajSpeedrun::Disconnected => {
                self.connecte = false;
                self.processus = None;
                self.phase = PhaseSpeedrun::NotRunning;
                self.index = None;
                self.game_time = None;
                self.real_time = TEMPS_ZERO.to_string();
                self.split_times.clear();
                self.split_cumuls.clear();
                self.seg_time = None;
            }
            MajSpeedrun::Lss {
                nom_jeu,
                nom_categorie,
                segments,
            } => {
                self.nom_jeu = nom_jeu;
                self.nom_categorie = nom_categorie;
                self.segments = segments;
            }
        }
        self.version = self.version.wrapping_add(1);
    }

    /// Compteur monotone (diagnostic / tests).
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn phase(&self) -> PhaseSpeedrun {
        self.phase
    }

    pub fn index(&self) -> Option<i32> {
        self.index
    }

    pub fn real_time(&self) -> &str {
        &self.real_time
    }

    pub fn game_time(&self) -> Option<&str> {
        self.game_time.as_deref()
    }

    pub fn seg_time(&self) -> Option<&str> {
        self.seg_time.as_deref()
    }

    pub fn nom_jeu(&self) -> &str {
        &self.nom_jeu
    }

    pub fn nom_categorie(&self) -> &str {
        &self.nom_categorie
    }

    pub fn connecte(&self) -> bool {
        self.connecte
    }

    pub fn actif(&self) -> bool {
        self.actif
    }

    /// Titre de la pastille de statut (`title` du DOM — diagnostic).
    pub fn processus(&self) -> Option<&str> {
        self.processus.as_deref()
    }

    /// Nombre de splits du .lss chargé (`srTotalSplits`).
    pub fn total_splits(&self) -> usize {
        self.segments.len()
    }

    pub fn segments(&self) -> &[SegmentLss] {
        &self.segments
    }

    /// Durée enregistrée du segment j ("" si absente — `srSplitTimes`).
    fn split_time(&self, j: usize) -> &str {
        self.split_times.get(j).map(String::as_str).unwrap_or("")
    }

    /// Cumul enregistré au split j (`srSplitCumuls`).
    fn split_cumul(&self, j: usize) -> Option<&str> {
        self.split_cumuls.get(j).and_then(|c| c.as_deref())
    }
}

// =============================================================================
// CONFIG — extraite du Widget (scène) par `speedruns_depuis_scene`.
// =============================================================================

/// Colonne « temps » des lignes de split (`srModeTemps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModeTemps {
    /// Temps cumulé auquel le split est/sera bouclé (défaut, façon
    /// LiveSplit).
    #[default]
    Cumul,
    /// Durée du segment.
    Segment,
}

/// Colonne « delta » (`srModeDelta`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModeDelta {
    /// Avance/retard cumulé de la run vs PB (défaut).
    #[default]
    Cumul,
    /// SegmentDelta LiveSplit — temps gagné/perdu sur CE segment.
    Segment,
    /// Colonne masquée (`.sr-delta-off`).
    Off,
}

/// Précision d'un affichage de temps (`srDeltaPrecision`/`srTimerPrecision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrecisionTemps {
    Seconde,
    Dixieme,
    Centieme,
}

/// Méthode de temps du timer (`srTimerMethode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MethodeTemps {
    /// Game time si disponible, sinon real time (défaut).
    #[default]
    Auto,
    /// Force le real time.
    Real,
    /// Game time si disponible, sinon real time (comme Auto côté DOM).
    Game,
}

/// Configuration d'un widget `type: "speedrun"`, extraite de la scène.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedrunCfg {
    /// Id du widget — clé du bloc de mise en page.
    pub id: String,
    /// Rect du widget en px canvas.
    pub rect: (f64, f64, f64, f64),
    /// Ordre de composition (`w.z`).
    pub z: i32,
    /// `srPolice || "Rajdhani"` (id `fonts.ts` — résolu par `format`).
    pub police: String,
    /// `srTaillePolice || 16` — base des unités em, en px.
    pub taille: f32,
    /// `srEstLigne` : mode horizontal (splits en ligne, timer à droite).
    pub ligne: bool,
    /// `--sr-grad-from`/`--sr-grad-to` (`gradientCadre`).
    pub accent: ([u8; 4], [u8; 4]),
    /// `--sr-ombre` (`contrasteOmbre` sur l'accent) — ombre du timer.
    pub ombre: [u8; 4],
    /// `--sr-timer-couleur` = `srCouleurTimer` ou `--sr-grad-from`.
    pub couleur_timer: [u8; 4],
    /// `--sr-couleur-ahead` = `srCouleurAhead` ou `--message-ok-color`.
    pub couleur_ahead: [u8; 4],
    /// `--sr-couleur-behind` = `srCouleurBehind` ou `--message-error-color`.
    pub couleur_behind: [u8; 4],
    /// `--sr-couleur-actif` = `srCouleurActif` ou `--sr-grad-from`.
    pub couleur_actif: [u8; 4],
    /// Toggles de visibilité (modale SpeedrunLayout) — `srVoir*` ≠ false.
    pub voir_titre: bool,
    pub voir_cat: bool,
    pub voir_statut: bool,
    pub voir_timer: bool,
    pub voir_phase: bool,
    pub voir_compteur: bool,
    pub voir_barre: bool,
    pub mode_temps: ModeTemps,
    pub mode_delta: ModeDelta,
    pub delta_prec: PrecisionTemps,
    /// `srDeltaDrop !== false` — drop des décimales du delta ≥ 1 min.
    pub delta_drop: bool,
    pub timer_prec: PrecisionTemps,
    pub timer_methode: MethodeTemps,
    /// `--sr-seg-bw` : épaisseur de bordure des lignes (0 = masquée).
    pub seg_bord_ep: f32,
    /// `--sr-seg-br` : rayon des lignes (`srSegBordureRayon` ou --rayon-petit).
    pub seg_bord_rayon: f32,
    /// `srNbLignes` — fenêtre de lignes visibles (0 = toutes).
    pub nb_lignes: u32,
    /// `srScrollAuto !== false` — scroll centré sur le split actif.
    pub scroll_auto: bool,
    /// `srHauteurLigne` — min-height des lignes en px (0 = auto).
    pub hauteur_ligne: f32,
    /// Cadre de scène du widget (clip du contenu, trait par le calque cadre).
    pub cadre: Option<CadreCfg>,
}

// =============================================================================
// RÉSOLUTION — miroir des lectures de `srMaj*` (chaînes + états de lignes).
// =============================================================================

/// Sens du delta LiveSplit — classes `delta-ahead`/`delta-behind` (et les
/// bordures `sr-seg-ahead`/`sr-seg-behind` qui en découlent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaSens {
    Neutre,
    /// Temps gagné vs PB (vert).
    Ahead,
    /// Temps perdu vs PB (rouge).
    Behind,
}

/// Pastille de statut du header (● connecté / ○ en attente).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatutSr {
    pub glyphe: &'static str,
    pub couleur: [u8; 4],
}

/// Contenu résolu d'une ligne de split.
#[derive(Debug, Clone, PartialEq)]
pub struct SegRendu {
    pub nom: String,
    /// Colonne temps ("" si rien à afficher — la case reste visible).
    pub time: String,
    /// Colonne delta ("" si neutre/sans référence).
    pub delta: String,
    pub delta_sens: DeltaSens,
    /// Split en cours (`sr-seg-actif`).
    pub actif: bool,
    /// Split déjà bouclé (`sr-seg-done` — affiché à `ALPHA_DONE`).
    pub done: bool,
    /// Dans la fenêtre `srNbLignes` (`display` vs `none` du DOM).
    pub visible: bool,
}

/// Contenus résolus du widget pour une frame — entrée de
/// `synchroniser_bloc_speedrun` et du dessin.
#[derive(Debug, Clone, PartialEq)]
pub struct TextesSpeedrun {
    /// Titre du jeu ; `placeholder` = « Aucun splits chargé » (italique gris).
    /// `None` = ligne non affichée (toggle ou condition du DOM).
    pub titre: Option<(String, bool)>,
    /// Catégorie — `None` = ligne masquée.
    pub cat: Option<String>,
    /// Pastille de statut — `None` = rien en coin.
    pub statut: Option<StatutSr>,
    /// Valeur du timer — `None` = timer masqué (`srVoirTimer`).
    pub timer: Option<String>,
    /// Pastille de phase : texte + (couleur, alpha de fond) — `None` = masquée.
    pub pill: Option<(String, [u8; 4], f32)>,
    /// Compteur « idx / total » — `None` = masqué ou aucun .lss.
    pub compteur: Option<String>,
    /// Pourcentage de la barre (0–100) — `None` = barre masquée.
    pub barre: Option<f32>,
    /// Colonne delta affichée (`modeDelta != "off"`).
    pub delta_col: bool,
    pub segs: Vec<SegRendu>,
}

/// `srParseTime` — « MM:SS.cc », « H:MM:SS.cc » ou secondes seules → s.
/// Chaîne vide/`--/--/--` → 0 (comportement du DOM).
pub fn parse_time(s: &str) -> f64 {
    if s.is_empty() || s == "--/--/--" {
        return 0.0;
    }
    let parse = |p: &str| p.trim().parse::<f64>().unwrap_or(0.0);
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.len() {
        3 => (parse(parts[0]), parse(parts[1]), parse(parts[2])),
        2 => (0.0, parse(parts[0]), parse(parts[1])),
        _ => (0.0, 0.0, parse(parts[0])),
    };
    h * 3600.0 + m * 60.0 + sec
}

/// `srFormatSegment` — secondes → « MM:SS.cc » (« H:MM:SS.cc » ≥ 1 h).
/// `secs` < 0 ou NaN → « --/--/-- ».
pub fn format_segment(secs: f64) -> String {
    if secs < 0.0 || secs.is_nan() {
        return "--/--/--".to_string();
    }
    let total_cs = (secs * 100.0).round() as i64;
    let h = total_cs / 360000;
    let m = (total_cs % 360000) / 6000;
    let s = (total_cs % 6000) / 100;
    let cs = total_cs % 100;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{cs:02}")
    } else {
        format!("{m:02}:{s:02}.{cs:02}")
    }
}

/// `srFormatCumul` — temps de run affiché en colonne (« M:SS » / « H:MM:SS »,
/// accuracy Seconds — jamais de centièmes). < 0 ou NaN → "".
pub fn format_cumul(secs: f64) -> String {
    if secs < 0.0 || secs.is_nan() {
        return String::new();
    }
    let total = secs.round() as i64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `srFormatDelta` — delta signé, format LiveSplit
/// (`Delta::with_decimal_dropping` + `Accuracy::Tenths`) :
/// |d| < 60 s → « ±S.d » (précision), sinon « ±M:SS » / « ±H:MM:SS »
/// (décimales abandonnées sauf `drop` désactivé).
pub fn format_delta(secs: f64, prec: PrecisionTemps, drop: bool) -> String {
    let signe = if secs < 0.0 { "-" } else { "+" };
    let a = secs.abs();
    let dec = match prec {
        PrecisionTemps::Centieme => 2,
        PrecisionTemps::Seconde => 0,
        PrecisionTemps::Dixieme => 1,
    };
    if a < 60.0 {
        return format!("{signe}{a:.dec$}");
    }
    let (total, frac) = if dec > 0 && !drop {
        // Décimales conservées : le JS tronque la chaîne formatée, puis
        // floor — `floor(a.toFixed(dec))` (identique à floor(a) sauf à la
        // limite d'arrondi vers l'entier supérieur).
        let arr = format!("{a:.dec$}");
        let total = arr.parse::<f64>().unwrap_or(a).floor() as i64;
        let frac = arr[arr.find('.').unwrap_or(arr.len())..].to_string();
        (total, frac)
    } else {
        (a.floor() as i64, String::new())
    };
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    let base = if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    };
    format!("{signe}{base}{frac}")
}

/// `srFormatTimerPrec` — reformate « MM:SS.cc » selon la précision du timer :
/// `centieme` = chaîne brute du moteur (référence), `dixieme`/`seconde`
/// tronquent la fraction.
pub fn format_timer_prec(s: &str, prec: Option<PrecisionTemps>) -> String {
    let Some(prec) = prec else {
        return s.to_string();
    };
    if prec == PrecisionTemps::Centieme || s.is_empty() {
        return s.to_string();
    }
    let secs = parse_time(s);
    if secs.is_nan() || secs < 0.0 {
        return s.to_string();
    }
    let total_cs = (secs * 100.0).round() as i64;
    let v = if prec == PrecisionTemps::Seconde {
        (total_cs as f64 / 100.0).round() as i64 * 100
    } else {
        (total_cs as f64 / 10.0).round() as i64 * 10
    };
    let h = v / 360000;
    let m = (v % 360000) / 6000;
    let s2 = (v % 6000) / 100;
    let frac = if prec == PrecisionTemps::Seconde {
        String::new()
    } else {
        format!(".{}", (v % 100) / 10)
    };
    let base = if h > 0 {
        format!("{h}:{m:02}:{s2:02}")
    } else {
        format!("{m}:{s2:02}")
    };
    format!("{base}{frac}")
}

/// PB cumulé d'un segment (le .lss stocke les PB en CUMUL — real time
/// d'abord, fallback game time).
fn pb_cumul(segments: &[SegmentLss], j: usize) -> Option<f64> {
    segments
        .get(j)
        .and_then(|s| s.pb_real_time.or(s.pb_game_time))
}

/// Durée PB du segment j — `pb[j] − pb[j−1]` (le .lss stocke du cumul ;
/// `None` si le segment OU le précédent n'a pas de PB).
fn pb_segment(segments: &[SegmentLss], j: usize) -> Option<f64> {
    let cur = pb_cumul(segments, j)?;
    if j == 0 {
        return Some(cur);
    }
    pb_cumul(segments, j - 1).map(|prev| cur - prev)
}

/// Résout tous les contenus du widget — miroir de `srMajHeader` +
/// `srMajTimerEtPhase` + `srMajSegmentsVisuel`.
pub fn resoudre(cfg: &SpeedrunCfg, etat: &EtatSpeedrun) -> TextesSpeedrun {
    let idx = etat.index;
    let total = etat.total_splits();
    let segments = etat.segments();

    // --- En-tête (srMajHeader) ---
    let voir_titre_ligne = cfg.voir_titre
        && (!etat.nom_jeu.is_empty() || !(cfg.voir_cat && !etat.nom_categorie.is_empty()));
    let mut titre = voir_titre_ligne.then(|| {
        if etat.nom_jeu.is_empty() {
            (PLACEHOLDER.to_string(), true)
        } else {
            (etat.nom_jeu.clone(), false)
        }
    });
    let mut cat = (cfg.voir_cat && !etat.nom_categorie.is_empty())
        .then(|| etat.nom_categorie.clone());
    let mut statut = if !cfg.voir_statut {
        None
    } else if etat.connecte {
        Some(StatutSr {
            glyphe: "●",
            couleur: OK,
        })
    } else if etat.actif {
        Some(StatutSr {
            glyphe: "○",
            couleur: ATTENTE,
        })
    } else {
        None
    };

    // --- Timer (srMajTimerEtPhase) ---
    let brut = if cfg.timer_methode == MethodeTemps::Real {
        etat.real_time.as_str()
    } else {
        etat.game_time().unwrap_or(etat.real_time.as_str())
    };
    let timer = cfg
        .voir_timer
        .then(|| format_timer_prec(if brut.is_empty() { TEMPS_ZERO } else { brut }, Some(cfg.timer_prec)));

    // --- Pastille de phase + compteur ---
    let (pill_couleur, pill_alpha) = etat.phase.couleurs();
    let mut pill = cfg
        .voir_phase
        .then(|| (etat.phase.label().to_uppercase(), pill_couleur, pill_alpha));
    let mut compteur = (cfg.voir_compteur && total > 0)
        .then(|| format!("{} / {}", idx.unwrap_or(0), total));

    // --- Barre ---
    let mut barre = cfg.voir_barre.then(|| match (total > 0, idx) {
        (true, Some(i)) => ((i as f32 / total as f32) * 100.0).min(100.0),
        _ => 0.0,
    });

    // --- Fenêtre srNbLignes (visual_split_count LiveSplit) ---
    let (vis_start, vis_end) = if cfg.nb_lignes > 0 && total > cfg.nb_lignes as usize {
        let n = cfg.nb_lignes as usize;
        let start = (idx.unwrap_or(0).max(0) as usize)
            .saturating_sub(1)
            .min(total - n);
        (start, start + n)
    } else {
        (0, total)
    };

    // --- Lignes de splits (srMajSegmentsVisuel) ---
    let en_run = etat.phase.en_run();
    let chrono_total = etat
        .game_time()
        .unwrap_or(etat.real_time.as_str())
        .to_string();
    let delta_col = cfg.mode_delta != ModeDelta::Off;
    let segs = (0..total)
        .map(|j| {
            let est_actif = idx == Some(j as i32);
            let en_cours = est_actif && en_run;
            let enregistre = etat.split_time(j);
            let cumul_rec = etat.split_cumul(j);
            let pb_c = pb_cumul(segments, j);
            let pb_s = pb_segment(segments, j);

            // Delta — trois modes (modale SpeedrunLayout), miroir du bloc
            // `o.modeDelta` de srMajSegmentsVisuel.
            let mut dd: Option<f64> = None;
            if delta_col && idx.is_some() && j as i32 <= idx.unwrap_or(0) {
                if cfg.mode_delta == ModeDelta::Segment {
                    let dur = if (j as i32) < idx.unwrap_or(0) {
                        let c_prev = if j > 0 {
                            etat.split_cumul(j - 1)
                        } else {
                            Some("0:00")
                        };
                        match (cumul_rec, c_prev) {
                            (Some(c), Some(p)) => Some(parse_time(c) - parse_time(p)),
                            _ => None,
                        }
                    } else if en_cours {
                        // `srParseTime(entry.srSegTime)` — null → 0.
                        Some(etat.seg_time().map(parse_time).unwrap_or(0.0))
                    } else {
                        None
                    };
                    if let (Some(dur), Some(pb)) = (dur, pb_s) {
                        dd = Some(((dur - pb) * 10.0).round() / 10.0);
                    }
                } else if let Some(pb) = pb_c {
                    let cumul_secs = if (j as i32) < idx.unwrap_or(0) {
                        cumul_rec.map(parse_time)
                    } else if en_cours {
                        Some(parse_time(&chrono_total))
                    } else {
                        None
                    };
                    if let Some(c) = cumul_secs {
                        dd = Some(((c - pb) * 10.0).round() / 10.0);
                    }
                }
            }
            let delta_sens = dd.map(sens_delta).unwrap_or(DeltaSens::Neutre);
            let delta = dd
                .map(|v| format_delta(v, cfg.delta_prec, cfg.delta_drop))
                .unwrap_or_default();

            // Colonne temps — « cumul » (LiveSplit) ou « segment ».
            let time = if cfg.mode_temps == ModeTemps::Segment {
                if est_actif {
                    etat.seg_time().unwrap_or(TEMPS_ZERO).to_string()
                } else if idx.is_some() && (j as i32) < idx.unwrap_or(0) && !enregistre.is_empty() {
                    enregistre.to_string()
                } else {
                    pb_s.map(format_segment).unwrap_or_default()
                }
            } else if idx.is_some() && (j as i32) < idx.unwrap_or(0) {
                cumul_rec.map(|c| format_cumul(parse_time(c))).unwrap_or_default()
            } else if en_cours {
                format_cumul(parse_time(&chrono_total))
            } else {
                pb_c.map(format_cumul).unwrap_or_default()
            };

            SegRendu {
                nom: segments.get(j).map(|s| s.nom.clone()).unwrap_or_default(),
                time,
                delta,
                delta_sens,
                actif: est_actif,
                done: idx.is_some() && (j as i32) < idx.unwrap_or(0) && !est_actif,
                visible: j >= vis_start && j < vis_end,
            }
        })
        .collect();

    // Mode « ligne » : header/meta/barre sont `display: none` dans le DOM —
    // pas de texte à mettre en page pour eux.
    if cfg.ligne {
        titre = None;
        cat = None;
        statut = None;
        pill = None;
        compteur = None;
        barre = None;
    }

    TextesSpeedrun {
        titre,
        cat,
        statut,
        timer,
        pill,
        compteur,
        barre,
        delta_col,
        segs,
    }
}

/// Mélange `color-mix(in srgb, c part%, base)` — interpolation par canal.
fn mix(c: [u8; 4], part: f32, base: [u8; 4]) -> [u8; 4] {
    let m = |a: u8, b: u8| (a as f32 * part + b as f32 * (1.0 - part)).round() as u8;
    [
        m(c[0], base[0]),
        m(c[1], base[1]),
        m(c[2], base[2]),
        m(c[3], base[3]),
    ]
}

/// Couleur `c` vue à l'alpha `a` (fond teinté `color-mix(c a%, transparent)`).
fn teinte(c: [u8; 4], a: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (c[3] as f32 * a).round() as u8]
}

/// `opacity` du DOM appliquée à une couleur (split `done` : alpha × 0,6).
fn opacite(c: [u8; 4], k: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (c[3] as f32 * k).round() as u8]
}

// =============================================================================
// DESSIN
// =============================================================================

/// Un texte mis en page du widget (même motif que `LigneGoal`). `max_w`
/// et `gras` font partie de la clé de reconstruction : le nom d'un split
/// se re-mette en page quand sa largeur dispo change (colonnes qui
/// grandissent), et le delta quand son épaisseur change.
struct LigneSr {
    texte: String,
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    w: f32,
    h: f32,
    /// Largeur de boîte passée au layout (cap de l'ellipsis).
    max_w: f32,
    /// Graisse utilisée (delta : 400 neutre, 700 ahead/behind).
    gras: bool,
}

/// Lignes mises en page d'un split.
#[derive(Default)]
struct SegLignes {
    nom: Option<LigneSr>,
    time: Option<LigneSr>,
    delta: Option<LigneSr>,
}

/// Le bloc d'un widget speedrun — les slots se resynchronisent par
/// comparaison (texte, largeur de boîte, graisse), comme `BlocGoal`.
#[derive(Default)]
pub(super) struct BlocSpeedrun {
    /// Titre du jeu (ou placeholder « Aucun splits chargé »).
    titre: Option<LigneSr>,
    cat: Option<LigneSr>,
    /// Pastille de statut (●/○).
    statut: Option<LigneSr>,
    /// Valeur du timer.
    timer: Option<LigneSr>,
    /// Libellé de la pastille de phase.
    pill: Option<LigneSr>,
    /// « idx / total ».
    compteur: Option<LigneSr>,
    /// nom | temps | delta par segment (parallèle à `segs` résolus).
    segs: Vec<SegLignes>,
}

#[derive(Clone, Copy)]
enum SlotSr {
    Titre,
    Cat,
    Statut,
    Timer,
    Pill,
    Compteur,
    SegNom(usize),
    SegTime(usize),
    SegDelta(usize),
}

/// Clé de cache d'un bloc : id du widget + paramètres qui changent la
/// métrique des lignes (taille de base, police, disposition, largeur de
/// boîte pour l'ellipsis). Le contenu est comparé chaîne à chaîne.
fn cle_bloc(cfg: &SpeedrunCfg) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cfg.id.hash(&mut h);
    cfg.taille.to_bits().hash(&mut h);
    (cfg.rect.2 as f32).to_bits().hash(&mut h);
    cfg.police.hash(&mut h);
    cfg.ligne.hash(&mut h);
    cfg.hauteur_ligne.to_bits().hash(&mut h);
    h.finish()
}

/// Un delta déjà arrondi au dixième → sens de couleur (`deltaSens`).
fn sens_delta(dd: f64) -> DeltaSens {
    if dd < 0.0 {
        DeltaSens::Ahead
    } else if dd > 0.0 {
        DeltaSens::Behind
    } else {
        DeltaSens::Neutre
    }
}

/// La _sens_ d'un `SegRendu` suffit à la graisse du delta — helper de sync.
fn gras_delta(s: &SegRendu) -> bool {
    !s.delta.is_empty() && s.delta_sens != DeltaSens::Neutre
}

impl DessinateurTexte {
    /// Dessine un widget speedrun — dans une passe ouverte.
    ///
    /// Contenu rogné au rect (`overflow: hidden` du widget) et au polygone
    /// du cadre de scène quand il y en a un — même motif de clip + couche
    /// que `dessiner_goal`.
    pub fn dessiner_speedrun(
        &mut self,
        cfg: &SpeedrunCfg,
        etat: &EtatSpeedrun,
        clip: Option<&ID2D1PathGeometry>,
    ) -> Result<(), ErreurTexte> {
        let (x, y, w, h) = cfg.rect;
        let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
        if w <= 0.0 || h <= 0.0 || cfg.taille <= 0.0 {
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
                .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer speedrun : {}", e.message())))?;
            unsafe { self.ctx.PushLayer(&params, &layer) };
        }
        let resultat = self.dessiner_speedrun_interne(cfg, etat, &zone);
        if avec_couche {
            unsafe { self.ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        resultat
    }

    /// Contenu du widget — sous les clips déjà posés.
    fn dessiner_speedrun_interne(
        &mut self,
        cfg: &SpeedrunCfg,
        etat: &EtatSpeedrun,
        zone: &D2D_RECT_F,
    ) -> Result<(), ErreurTexte> {
        let textes = resoudre(cfg, etat);
        self.synchroniser_bloc_speedrun(cfg, &textes)?;
        let cle = cle_bloc(cfg);
        let Some(bloc) = self.blocs_speedrun.get(&cle) else {
            return Ok(());
        };
        let t = cfg.taille;
        let (zx, zy, zw, zh) = (zone.left, zone.top, zone.right - zone.left, zone.bottom - zone.top);

        let brosse = |c: [u8; 4], nom: &str| -> Result<ID2D1Brush, ErreurTexte> {
            unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(c), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("{nom} : {}", e.message())))
                .map(|b| b.into())
        };
        let ombre = brosse([0, 0, 0, 255], "brosse ombre speedrun")?;
        let ombre_timer = brosse(cfg.ombre, "brosse ombre timer")?;
        let texte = brosse(TEXTE, "brosse texte speedrun")?;
        let gris = brosse(GRIS, "brosse gris speedrun")?;

        if cfg.ligne {
            return self.dessiner_speedrun_ligne(
                cfg, &textes, bloc, zone, &ombre, &ombre_timer,
            );
        }

        // ===== Mode colonne (défaut) =====
        let pad_v = 0.8 * t;
        let inner_x = zx + 10.0;
        let inner_w = (zw - 20.0).max(0.0);
        let cx = inner_x + inner_w / 2.0;
        let gap_row = 6.0;
        let mut y = zy + pad_v;

        // --- En-tête : titre + catégorie centrés, statut en coin ---------
        let mut header_h = 0.0;
        if let Some(l) = &bloc.titre {
            header_h += l.h;
        }
        if let Some(l) = &bloc.cat {
            header_h += if header_h > 0.0 { 2.0 + l.h } else { l.h };
        }
        if header_h > 0.0 {
            let mut hy = y;
            if let Some(l) = &bloc.titre {
                let placeholder = textes.titre.as_ref().is_some_and(|(_, p)| *p);
                let b = if placeholder { &gris } else { &texte };
                dessiner_ligne_sr(&self.cible_rt, cx - l.w / 2.0, hy, l, &ombre, b, 1.0);
                hy += l.h + 2.0;
            }
            if let Some(l) = &bloc.cat {
                dessiner_ligne_sr(&self.cible_rt, cx - l.w / 2.0, hy, l, &ombre, &gris, 1.0);
            }
            if let Some(l) = &bloc.statut {
                let couleur = textes.statut.map(|s| s.couleur).unwrap_or(GRIS);
                let b = brosse(couleur, "brosse statut speedrun")?;
                dessiner_ligne_sr(
                    &self.cible_rt,
                    inner_x + inner_w - l.w,
                    y,
                    l,
                    &ombre,
                    &b,
                    1.0,
                );
            }
            y += header_h + gap_row;
        }

        // --- Timer : fond teinté + valeur centrée -------------------------
        if let Some(l) = &bloc.timer {
            let box_x = inner_x + 0.8 * t;
            let box_w = (inner_w - 1.6 * t).max(0.0);
            let box_h = l.h + 0.3 * t;
            let fond = brosse(teinte(cfg.accent.0, 0.15), "brosse fond timer")?;
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: box_x,
                            top: y,
                            right: box_x + box_w,
                            bottom: y + box_h,
                        },
                        radiusX: RAYON_PETIT,
                        radiusY: RAYON_PETIT,
                    },
                    &fond,
                )
            };
            let b = brosse(cfg.couleur_timer, "brosse timer")?;
            dessiner_ligne_sr(
                &self.cible_rt,
                box_x + (box_w - l.w) / 2.0,
                y + 0.15 * t,
                l,
                &ombre_timer,
                &b,
                2.0,
            );
            y += box_h + gap_row;
        }

        // --- Meta : pastille de phase + compteur --------------------------
        // `padding: 0.06em 0.5em` relatif à la fonte de la pastille (0,65 t).
        let pill_h = bloc
            .pill
            .as_ref()
            .map(|l| l.h + 0.12 * 0.65 * t)
            .unwrap_or(0.0);
        let comp_h = bloc.compteur.as_ref().map(|l| l.h).unwrap_or(0.0);
        let meta_h = pill_h.max(comp_h);
        if meta_h > 0.0 {
            let pill_box_w = bloc
                .pill
                .as_ref()
                .map(|l| l.w + 1.0 * (0.65 * t))
                .unwrap_or(0.0);
            let comp_w = bloc.compteur.as_ref().map(|l| l.w).unwrap_or(0.0);
            let total_w = pill_box_w
                + comp_w
                + if pill_box_w > 0.0 && comp_w > 0.0 {
                    0.5 * t
                } else {
                    0.0
                };
            let mut px = cx - total_w / 2.0;
            if let Some(l) = &bloc.pill {
                if let Some((_, couleur, alpha)) = &textes.pill {
                    let fond = brosse(teinte(*couleur, *alpha), "brosse fond pill")?;
                    unsafe {
                        self.cible_rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: px,
                                    top: y + (meta_h - pill_h) / 2.0,
                                    right: px + pill_box_w,
                                    bottom: y + (meta_h - pill_h) / 2.0 + pill_h,
                                },
                                radiusX: pill_h / 2.0,
                                radiusY: pill_h / 2.0,
                            },
                            &fond,
                        )
                    };
                    let b = brosse(*couleur, "brosse pill")?;
                    dessiner_ligne_sr(
                        &self.cible_rt,
                        px + 0.5 * (0.65 * t),
                        y + (meta_h - l.h) / 2.0,
                        l,
                        &ombre,
                        &b,
                        1.0,
                    );
                    px += pill_box_w + 0.5 * t;
                }
            }
            if let Some(l) = &bloc.compteur {
                dessiner_ligne_sr(
                    &self.cible_rt,
                    px,
                    y + (meta_h - l.h) / 2.0,
                    l,
                    &ombre,
                    &gris,
                    1.0,
                );
            }
            y += meta_h + gap_row;
        }

        // --- Barre de progression ------------------------------------------
        if let Some(pct) = textes.barre {
            let bar_h = 0.2 * t;
            if bar_h > 0.0 {
                let piste = D2D_RECT_F {
                    left: inner_x,
                    top: y,
                    right: inner_x + inner_w,
                    bottom: y + bar_h,
                };
                let b_piste = brosse(teinte(GRIS, 0.20), "brosse piste speedrun")?;
                unsafe {
                    self.cible_rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: piste,
                            radiusX: bar_h / 2.0,
                            radiusY: bar_h / 2.0,
                        },
                        &b_piste,
                    )
                };
                if pct > 0.0 {
                    let fill_w = inner_w * pct / 100.0;
                    self.fill_degrade(
                        D2D_RECT_F {
                            left: inner_x,
                            top: y,
                            right: inner_x + fill_w,
                            bottom: y + bar_h,
                        },
                        D2D_POINT_2F { x: inner_x, y },
                        D2D_POINT_2F {
                            x: inner_x + fill_w,
                            y,
                        },
                        cfg.accent.0,
                        cfg.accent.1,
                        bar_h / 2.0,
                    )?;
                }
            }
            y += bar_h + gap_row;
        }

        // --- Splits : nom | temps | delta, scroll centré sur l'actif ------
        let splits_top = y + 0.25 * t;
        let splits_bottom = zy + zh - pad_v;
        let container_h = splits_bottom - y;
        if container_h > 0.0 && !textes.segs.is_empty() {
            let zone_splits = D2D_RECT_F {
                left: inner_x,
                top: y,
                right: inner_x + inner_w,
                bottom: splits_bottom.max(y),
            };
            self.dessiner_splits(
                cfg,
                &textes,
                bloc,
                inner_x,
                splits_top,
                inner_w,
                container_h - 0.25 * t,
                &zone_splits,
                &ombre,
                &brosse,
            )?;
        }
        Ok(())
    }

    /// Liste des splits (mode colonne) — fond dégradé, bordure, trois
    /// colonnes, fenêtre `srNbLignes`, scroll centré sur l'actif.
    #[allow(clippy::too_many_arguments)]
    fn dessiner_splits(
        &self,
        cfg: &SpeedrunCfg,
        textes: &TextesSpeedrun,
        bloc: &BlocSpeedrun,
        x: f32,
        top: f32,
        w: f32,
        container_h: f32,
        zone_clip: &D2D_RECT_F,
        ombre: &ID2D1Brush,
        brosse: &FabriqueBrosse<'_>,
    ) -> Result<(), ErreurTexte> {
        let t = cfg.taille;
        let em_seg = 0.78 * t;
        let pad_seg = 0.4 * em_seg;
        let gap_seg = 0.4 * em_seg;
        let em_col = 0.9 * em_seg;
        let gap_lignes = 0.2 * t;

        // Hauteurs et offsets des lignes visibles.
        let mut hauteurs = Vec::with_capacity(textes.segs.len());
        let mut total_h = 0.0;
        let mut actif_top = None;
        let mut actif_h = 0.0;
        for (j, seg) in textes.segs.iter().enumerate() {
            if !seg.visible {
                hauteurs.push(0.0);
                continue;
            }
            let lignes = &bloc.segs[j];
            let content_h = [
                lignes.nom.as_ref().map(|l| l.h),
                lignes.time.as_ref().map(|l| l.h),
                lignes.delta.as_ref().map(|l| l.h),
            ]
            .into_iter()
            .flatten()
            .fold(0.0_f32, f32::max);
            let seg_h = cfg.hauteur_ligne.max(2.0 * pad_seg + content_h);
            if seg.actif {
                actif_top = Some(total_h);
                actif_h = seg_h;
            }
            total_h += seg_h + gap_lignes;
            hauteurs.push(seg_h);
        }

        // Scroll instantané : centrage sur l'actif (`srScrollAuto`).
        let mut scroll = 0.0_f32;
        if cfg.scroll_auto {
            if let Some(at) = actif_top {
                let cible = at - (container_h - actif_h) / 2.0;
                scroll = cible.clamp(0.0, (total_h - gap_lignes - container_h).max(0.0));
            }
        }

        unsafe {
            self.cible_rt
                .PushAxisAlignedClip(zone_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
        };
        let mut sy = top - scroll;
        for (j, seg) in textes.segs.iter().enumerate() {
            let seg_h = hauteurs[j];
            if seg_h <= 0.0 {
                continue;
            }
            let lignes = &bloc.segs[j];
            let seg_rect = D2D_RECT_F {
                left: x,
                top: sy,
                right: x + w,
                bottom: sy + seg_h,
            };
            // Alpha du split terminé (`opacity: 0.6` — modulée par couleur,
            // identique au compositing sur un fond opaque).
            let k = if seg.done { ALPHA_DONE } else { 1.0 };

            // Fond : dégradé 135° accent→noir (actif : accent plein ;
            // sinon accent à 35 % sur noir).
            let debut = if seg.actif {
                cfg.accent.0
            } else {
                mix(cfg.accent.0, 0.35, [0, 0, 0, 255])
            };
            let b_fond = brosse_fond_seg(
                &self.cible_rt,
                &seg_rect,
                opacite(debut, k),
                [0, 0, 0, (255.0 * k) as u8],
            )?;
            unsafe { self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: seg_rect,
                    radiusX: cfg.seg_bord_rayon,
                    radiusY: cfg.seg_bord_rayon,
                },
                &b_fond,
            )};

            // Bordure : résultat (ahead/behind) > actif > transparente.
            if cfg.seg_bord_ep > 0.0 {
                let couleur = match seg.delta_sens {
                    DeltaSens::Ahead => Some(cfg.couleur_ahead),
                    DeltaSens::Behind => Some(cfg.couleur_behind),
                    DeltaSens::Neutre => seg.actif.then_some(cfg.couleur_actif),
                };
                if let Some(c) = couleur {
                    let demi = cfg.seg_bord_ep / 2.0;
                    let b_bord = brosse(opacite(c, k), "brosse bord split")?;
                    unsafe {
                        self.cible_rt.DrawRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: seg_rect.left + demi,
                                    top: seg_rect.top + demi,
                                    right: seg_rect.right - demi,
                                    bottom: seg_rect.bottom - demi,
                                },
                                radiusX: (cfg.seg_bord_rayon - demi).max(0.0),
                                radiusY: (cfg.seg_bord_rayon - demi).max(0.0),
                            },
                            &b_bord,
                            cfg.seg_bord_ep,
                            None,
                        )
                    };
                }
            }

            // Textes centrés verticalement dans la ligne.
            let cont_x = seg_rect.left + pad_seg + cfg.seg_bord_ep;
            if let Some(l) = &lignes.nom {
                let c = if seg.done { GRIS } else { TEXTE };
                let b = brosse(opacite(c, k), "brosse nom split")?;
                let ty = sy + (seg_h - l.h) / 2.0;
                dessiner_ligne_sr(&self.cible_rt, cont_x, ty, l, ombre, &b, 1.0);
            }
            // Colonnes droite : delta dernière, temps juste avant.
            let mut droite = seg_rect.right - pad_seg - cfg.seg_bord_ep;
            if textes.delta_col {
                if let Some(l) = &lignes.delta {
                    let c = match seg.delta_sens {
                        DeltaSens::Ahead => cfg.couleur_ahead,
                        DeltaSens::Behind => cfg.couleur_behind,
                        DeltaSens::Neutre => GRIS,
                    };
                    let b = brosse(opacite(c, k), "brosse delta split")?;
                    let ty = sy + (seg_h - l.h) / 2.0;
                    dessiner_ligne_sr(&self.cible_rt, droite - l.w, ty, l, ombre, &b, 1.0);
                }
                let col_w = lignes
                    .delta
                    .as_ref()
                    .map(|l| l.w.max(5.0 * em_col))
                    .unwrap_or(5.0 * em_col);
                droite -= col_w + gap_seg;
            }
            if let Some(l) = &lignes.time {
                let b = brosse(opacite(cfg.accent.0, k), "brosse temps split")?;
                let ty = sy + (seg_h - l.h) / 2.0;
                dessiner_ligne_sr(&self.cible_rt, droite - l.w, ty, l, ombre, &b, 1.0);
            }
            sy += seg_h + gap_lignes;
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        Ok(())
    }

    /// Mode « ligne » (`srEstLigne`) : splits en cellules horizontales à
    /// gauche (scroll horizontal), timer épinglé à droite, header/meta/
    /// barre masqués — miroir des règles `.sr-ligne`.
    fn dessiner_speedrun_ligne(
        &self,
        cfg: &SpeedrunCfg,
        textes: &TextesSpeedrun,
        bloc: &BlocSpeedrun,
        zone: &D2D_RECT_F,
        ombre: &ID2D1Brush,
        ombre_timer: &ID2D1Brush,
    ) -> Result<(), ErreurTexte> {
        let t = cfg.taille;
        let (zx, zy, zw, zh) = (zone.left, zone.top, zone.right - zone.left, zone.bottom - zone.top);
        let pad_v = 0.3 * t;
        let pad_h = 0.5 * t;
        let gap = 0.4 * t;

        let brosse = |c: [u8; 4], nom: &str| -> Result<ID2D1Brush, ErreurTexte> {
            unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(c), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("{nom} : {}", e.message())))
                .map(|b| b.into())
        };

        // --- Timer à droite (valeur 1,4 em, order: 2) ---------------------
        let mut splits_right = zx + zw - pad_h;
        if let Some(l) = &bloc.timer {
            let box_w = l.w + 1.0 * t;
            let box_h = l.h + 0.3 * t;
            let box_x = splits_right - box_w;
            let box_y = zy + (zh - box_h) / 2.0;
            let fond = brosse(teinte(cfg.accent.0, 0.15), "brosse fond timer")?;
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: box_x,
                            top: box_y,
                            right: box_x + box_w,
                            bottom: box_y + box_h,
                        },
                        radiusX: RAYON_PETIT,
                        radiusY: RAYON_PETIT,
                    },
                    &fond,
                )
            };
            let b = brosse(cfg.couleur_timer, "brosse timer")?;
            dessiner_ligne_sr(
                &self.cible_rt,
                box_x + (box_w - l.w) / 2.0,
                box_y + 0.15 * t,
                l,
                ombre_timer,
                &b,
                2.0,
            );
            splits_right = box_x - gap;
        }

        // --- Cellules horizontales ---------------------------------------
        let em_seg = 0.78 * t;
        let cell_pad_v = 0.15 * em_seg;
        let cell_pad_h = 0.4 * em_seg;
        let gap_lignes = 0.2 * t;
        let splits_x = zx + pad_h;
        let splits_w = (splits_right - splits_x).max(0.0);
        let splits_top = zy + pad_v;
        let splits_h = (zh - 2.0 * pad_v).max(0.0);
        if splits_w <= 0.0 || splits_h <= 0.0 {
            return Ok(());
        }

        // Largeur de chaque cellule visible (min 5,5 em / max 9 em du CSS).
        let mut largeurs = Vec::with_capacity(textes.segs.len());
        let mut total_w = 0.0;
        let mut actif_left = None;
        let mut actif_w = 0.0;
        for (j, seg) in textes.segs.iter().enumerate() {
            if !seg.visible {
                largeurs.push(0.0);
                continue;
            }
            let lignes = &bloc.segs[j];
            let nom_w = lignes.nom.as_ref().map(|l| l.w).unwrap_or(0.0);
            let time_w = lignes.time.as_ref().map(|l| l.w).unwrap_or(0.0);
            let delta_w = if textes.delta_col {
                lignes.delta.as_ref().map(|l| l.w).unwrap_or(0.0)
            } else {
                0.0
            };
            let row_w = time_w + delta_w + if delta_w > 0.0 { 0.4 * em_seg } else { 0.0 };
            let content = nom_w.max(row_w) + 2.0 * cell_pad_h;
            let cell_w = content.clamp(5.5 * em_seg, 9.0 * em_seg);
            if seg.actif {
                actif_left = Some(total_w);
                actif_w = cell_w;
            }
            total_w += cell_w + gap_lignes;
            largeurs.push(cell_w);
        }

        let mut scroll = 0.0_f32;
        if cfg.scroll_auto {
            if let Some(al) = actif_left {
                let cible = al - (splits_w - actif_w) / 2.0;
                scroll = cible.clamp(0.0, (total_w - gap_lignes - splits_w).max(0.0));
            }
        }

        let zone_splits = D2D_RECT_F {
            left: splits_x,
            top: splits_top,
            right: splits_x + splits_w,
            bottom: splits_top + splits_h,
        };
        unsafe {
            self.cible_rt
                .PushAxisAlignedClip(&zone_splits, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
        };
        let mut sx = splits_x - scroll;
        for (j, seg) in textes.segs.iter().enumerate() {
            let cell_w = largeurs[j];
            if cell_w <= 0.0 {
                continue;
            }
            let lignes = &bloc.segs[j];
            let cell_rect = D2D_RECT_F {
                left: sx,
                top: splits_top,
                right: sx + cell_w,
                bottom: splits_top + splits_h,
            };
            let k = if seg.done { ALPHA_DONE } else { 1.0 };

            let debut = if seg.actif {
                cfg.accent.0
            } else {
                mix(cfg.accent.0, 0.35, [0, 0, 0, 255])
            };
            let b_fond = brosse_fond_seg(
                &self.cible_rt,
                &cell_rect,
                opacite(debut, k),
                [0, 0, 0, (255.0 * k) as u8],
            )?;
            unsafe { self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: cell_rect,
                    radiusX: cfg.seg_bord_rayon,
                    radiusY: cfg.seg_bord_rayon,
                },
                &b_fond,
            )};
            if cfg.seg_bord_ep > 0.0 {
                let couleur = match seg.delta_sens {
                    DeltaSens::Ahead => Some(cfg.couleur_ahead),
                    DeltaSens::Behind => Some(cfg.couleur_behind),
                    DeltaSens::Neutre => seg.actif.then_some(cfg.couleur_actif),
                };
                if let Some(c) = couleur {
                    let demi = cfg.seg_bord_ep / 2.0;
                    let b_bord = brosse(opacite(c, k), "brosse bord split")?;
                    unsafe {
                        self.cible_rt.DrawRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: cell_rect.left + demi,
                                    top: cell_rect.top + demi,
                                    right: cell_rect.right - demi,
                                    bottom: cell_rect.bottom - demi,
                                },
                                radiusX: (cfg.seg_bord_rayon - demi).max(0.0),
                                radiusY: (cfg.seg_bord_rayon - demi).max(0.0),
                            },
                            &b_bord,
                            cfg.seg_bord_ep,
                            None,
                        )
                    };
                }
            }

            // Contenu : nom pleine largeur, puis temps (gauche) + delta.
            let content_x = cell_rect.left + cell_pad_h + cfg.seg_bord_ep;
            let content_w =
                (cell_w - 2.0 * cell_pad_h - 2.0 * cfg.seg_bord_ep).max(0.0);
            let inner_h = (splits_h - 2.0 * cell_pad_v - 2.0 * cfg.seg_bord_ep).max(0.0);
            let second_h = [
                lignes.time.as_ref().map(|l| l.h),
                lignes.delta.as_ref().map(|l| l.h),
            ]
            .into_iter()
            .flatten()
            .fold(0.0_f32, f32::max);
            let block_h = lignes.nom.as_ref().map(|l| l.h).unwrap_or(0.0) + second_h;
            let mut ly = splits_top + cell_pad_v + cfg.seg_bord_ep + (inner_h - block_h) / 2.0;
            if let Some(l) = &lignes.nom {
                let c = if seg.done { GRIS } else { TEXTE };
                let b = brosse(opacite(c, k), "brosse nom split")?;
                dessiner_ligne_sr(&self.cible_rt, content_x, ly, l, ombre, &b, 1.0);
                ly += l.h;
            }
            if let Some(l) = &lignes.time {
                let b = brosse(opacite(cfg.accent.0, k), "brosse temps split")?;
                dessiner_ligne_sr(&self.cible_rt, content_x, ly, l, ombre, &b, 1.0);
            }
            if textes.delta_col {
                if let Some(l) = &lignes.delta {
                    let c = match seg.delta_sens {
                        DeltaSens::Ahead => cfg.couleur_ahead,
                        DeltaSens::Behind => cfg.couleur_behind,
                        DeltaSens::Neutre => GRIS,
                    };
                    let b = brosse(opacite(c, k), "brosse delta split")?;
                    dessiner_ligne_sr(
                        &self.cible_rt,
                        content_x + content_w - l.w,
                        ly,
                        l,
                        ombre,
                        &b,
                        1.0,
                    );
                }
            }
            sx += cell_w + gap_lignes;
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        Ok(())
    }

    /// (Re)construit les lignes dont le contenu a changé — au plus une
    /// recréation par mutation visible (tick du timer, split, LSS, scène).
    fn synchroniser_bloc_speedrun(
        &mut self,
        cfg: &SpeedrunCfg,
        textes: &TextesSpeedrun,
    ) -> Result<(), ErreurTexte> {
        let t = cfg.taille;
        let em_seg = 0.78 * t;
        let em_col = if cfg.ligne { 0.75 * em_seg } else { 0.9 * em_seg };

        let base = StyleTexte {
            police: cfg.police.clone(),
            taille: t,
            espacement: 0.0,
            couleur: TEXTE,
            contour_epaisseur: 0.0,
            ..StyleTexte::default()
        };
        let style_titre = StyleTexte {
            taille: 0.75 * t,
            gras: true,
            ..base.clone()
        };
        let style_cat = StyleTexte {
            taille: 0.70 * t,
            ..base.clone()
        };
        let style_statut = StyleTexte {
            taille: 0.70 * t,
            ..base.clone()
        };
        let timer_taille = if cfg.ligne { 1.4 * t } else { 1.9 * t };
        let style_timer = StyleTexte {
            taille: timer_taille,
            gras: true,
            espacement: 0.04 * timer_taille,
            ..base.clone()
        };
        let style_pill = StyleTexte {
            taille: 0.65 * t,
            gras: true,
            espacement: 0.08 * 0.65 * t,
            ..base.clone()
        };
        let style_compteur = StyleTexte {
            taille: 0.70 * t,
            ..base.clone()
        };
        let style_nom = StyleTexte {
            taille: em_seg,
            ..base.clone()
        };
        let style_time = StyleTexte {
            taille: em_col,
            gras: true,
            ..base.clone()
        };
        let style_delta = StyleTexte {
            taille: em_col,
            ..base.clone()
        };
        let style_delta_gras = StyleTexte {
            gras: true,
            ..style_delta.clone()
        };
        let style_placeholder = StyleTexte {
            italique: true,
            ..style_cat.clone()
        };

        // Table bornée : les clés ne divergent que par (widget, métriques).
        if self.blocs_speedrun.len() > 16 {
            self.blocs_speedrun.clear();
        }
        let cle = cle_bloc(cfg);

        // --- Passe 1 : slots hors noms + colonnes temps/delta -------------
        let mut maj: Vec<(SlotSr, StyleTexte, String, f32, bool)> = Vec::new();
        {
            let bloc = self.blocs_speedrun.entry(cle).or_default();
            bloc.segs.resize_with(textes.segs.len(), SegLignes::default);
            let mut changer = |slot: &mut Option<LigneSr>,
                               kind: SlotSr,
                               texte: Option<&str>,
                               style: &StyleTexte,
                               max_w: f32| {
                let courant = slot.as_ref();
                let change = courant.map(|l| l.texte.as_str()) != texte
                    || courant.is_some_and(|l| {
                        (l.max_w - max_w).abs() > 0.5 || l.gras != style.gras
                    });
                if change {
                    *slot = None;
                    if let Some(s) = texte {
                        maj.push((kind, style.clone(), s.to_string(), max_w, max_w < 8000.0));
                    }
                }
            };
            let titre_txt = textes.titre.as_ref().map(|(s, _)| s.as_str());
            let placeholder = textes.titre.as_ref().is_some_and(|(_, p)| *p);
            changer(
                &mut bloc.titre,
                SlotSr::Titre,
                titre_txt,
                if placeholder {
                    &style_placeholder
                } else {
                    &style_titre
                },
                // `.sr-titre-jeu` : max-width 100 % de l'en-tête.
                (cfg.rect.2 as f32 - 20.0).max(1.0),
            );
            changer(
                &mut bloc.cat,
                SlotSr::Cat,
                textes.cat.as_deref(),
                &style_cat,
                8192.0,
            );
            changer(
                &mut bloc.statut,
                SlotSr::Statut,
                textes.statut.map(|s| s.glyphe),
                &style_statut,
                8192.0,
            );
            changer(
                &mut bloc.timer,
                SlotSr::Timer,
                textes.timer.as_deref(),
                &style_timer,
                8192.0,
            );
            changer(
                &mut bloc.pill,
                SlotSr::Pill,
                textes.pill.as_ref().map(|(s, _, _)| s.as_str()),
                &style_pill,
                8192.0,
            );
            changer(
                &mut bloc.compteur,
                SlotSr::Compteur,
                textes.compteur.as_deref(),
                &style_compteur,
                8192.0,
            );
            for (j, seg) in textes.segs.iter().enumerate() {
                changer(
                    &mut bloc.segs[j].time,
                    SlotSr::SegTime(j),
                    if seg.time.is_empty() {
                        None
                    } else {
                        Some(seg.time.as_str())
                    },
                    &style_time,
                    8192.0,
                );
                changer(
                    &mut bloc.segs[j].delta,
                    SlotSr::SegDelta(j),
                    if seg.delta.is_empty() {
                        None
                    } else {
                        Some(seg.delta.as_str())
                    },
                    if gras_delta(seg) {
                        &style_delta_gras
                    } else {
                        &style_delta
                    },
                    8192.0,
                );
            }
        }
        self.reconstruire_lignes(cle, maj)?;

        // --- Passe 2 : noms des splits (largeur dispo = reste des colonnes)
        let mut maj: Vec<(SlotSr, StyleTexte, String, f32, bool)> = Vec::new();
        {
            let Some(bloc) = self.blocs_speedrun.get_mut(&cle) else {
                return Ok(());
            };
            for (j, seg) in textes.segs.iter().enumerate() {
                let nom = if seg.nom.is_empty() {
                    None
                } else {
                    Some(seg.nom.as_str())
                };
                let max_w = if cfg.ligne {
                    // Cellule plafonnée à 9 em — cap fixe (le DOM ellipsise
                    // dans la cellule déjà dimensionnée).
                    9.0 * em_seg - 0.8 * em_seg
                } else {
                    // flex: 1 du nom = boîte moins colonnes temps/delta.
                    let time_w = bloc.segs[j]
                        .time
                        .as_ref()
                        .map(|l| l.w.max(4.5 * em_col))
                        .unwrap_or(4.5 * em_col);
                    let delta_w = if textes.delta_col {
                        bloc.segs[j]
                            .delta
                            .as_ref()
                            .map(|l| l.w.max(5.0 * em_col))
                            .unwrap_or(5.0 * em_col)
                    } else {
                        0.0
                    };
                    let gaps = if textes.delta_col { 2.0 } else { 1.0 } * 0.4 * em_seg;
                    (cfg.rect.2 as f32 - 20.0 - 2.0 * 0.4 * em_seg
                        - 2.0 * cfg.seg_bord_ep
                        - time_w
                        - delta_w
                        - gaps)
                        .max(1.0)
                };
                let slot = &mut bloc.segs[j].nom;
                let courant = slot.as_ref();
                let change = courant.map(|l| l.texte.as_str()) != nom
                    || courant.is_some_and(|l| (l.max_w - max_w).abs() > 0.5);
                if change {
                    *slot = None;
                    if let Some(s) = nom {
                        maj.push((SlotSr::SegNom(j), style_nom.clone(), s.to_string(), max_w, true));
                    }
                }
            }
        }
        self.reconstruire_lignes(cle, maj)
    }

    /// Recrée les lignes modifiées d'un bloc (format, ellipsis, métriques).
    fn reconstruire_lignes(
        &mut self,
        cle: u64,
        maj: Vec<(SlotSr, StyleTexte, String, f32, bool)>,
    ) -> Result<(), ErreurTexte> {
        for (slot, style, texte, max_w, ellipse) in maj {
            let ligne = self.ligne_speedrun(&style, &texte, max_w, ellipse)?;
            if let Some(bloc) = self.blocs_speedrun.get_mut(&cle) {
                match slot {
                    SlotSr::Titre => bloc.titre = Some(ligne),
                    SlotSr::Cat => bloc.cat = Some(ligne),
                    SlotSr::Statut => bloc.statut = Some(ligne),
                    SlotSr::Timer => bloc.timer = Some(ligne),
                    SlotSr::Pill => bloc.pill = Some(ligne),
                    SlotSr::Compteur => bloc.compteur = Some(ligne),
                    SlotSr::SegNom(j) => {
                        if let Some(s) = bloc.segs.get_mut(j) {
                            s.nom = Some(ligne);
                        }
                    }
                    SlotSr::SegTime(j) => {
                        if let Some(s) = bloc.segs.get_mut(j) {
                            s.time = Some(ligne);
                        }
                    }
                    SlotSr::SegDelta(j) => {
                        if let Some(s) = bloc.segs.get_mut(j) {
                            s.delta = Some(ligne);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Met en page un texte du widget — même recette que `ligne_goal`, plus
    /// la troncature « … » (`text-overflow: ellipsis` du DOM) quand
    /// `ellipse` est posé : `max_w` borne alors la boîte du layout.
    fn ligne_speedrun(
        &self,
        style: &StyleTexte,
        texte: &str,
        max_w: f32,
        ellipse: bool,
    ) -> Result<LigneSr, ErreurTexte> {
        let format = self.format(style)?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        if contenu.is_empty() {
            return Err(ErreurTexte::Systeme("ligne speedrun vide".to_string()));
        }
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, max_w.max(1.0), 2048.0)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout speedrun : {}", e.message())))?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap speedrun : {}", e.message())))?;
        if ellipse {
            // Troncature caractère + signe « … » — `text-overflow: ellipsis`.
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
            .map_err(|e| ErreurTexte::Systeme(format!("SetTrimming speedrun : {}", e.message())))?;
        }
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
                ErreurTexte::Systeme(format!("SetCharacterSpacing speedrun : {}", e.message()))
            })?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques speedrun : {}", e.message())))?;
        Ok(LigneSr {
            texte: texte.to_string(),
            layout,
            _format: format,
            w: metriques.width.min(max_w.max(1.0)),
            h: metriques.height,
            max_w: max_w.max(1.0),
            gras: style.gras,
        })
    }

    /// Remplit un rect arrondi du dégradé accent (borné à sa propre
    /// largeur, comme `linear-gradient(90deg)` relatif à l'élément).
    fn fill_degrade(
        &self,
        rect: D2D_RECT_F,
        debut: D2D_POINT_2F,
        fin: D2D_POINT_2F,
        from: [u8; 4],
        to: [u8; 4],
        rayon: f32,
    ) -> Result<(), ErreurTexte> {
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: couleur_d2d(from),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: couleur_d2d(to),
            },
        ];
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("stops speedrun : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: debut,
            endPoint: fin,
        };
        let degrade: ID2D1LinearGradientBrush = unsafe {
            self.cible_rt
                .CreateLinearGradientBrush(&props, None, &collection)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé speedrun : {}", e.message())))?;
        let degrade: ID2D1Brush = degrade.into();
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: rayon,
                    radiusY: rayon,
                },
                &degrade,
            )
        };
        Ok(())
    }
}

/// Fabrique de brosses unies passée aux helpers de dessin (clippy
/// `type_complexity`).
type FabriqueBrosse<'a> = dyn Fn([u8; 4], &str) -> Result<ID2D1Brush, ErreurTexte> + 'a;

/// Brosse dégradée 135° (haut-gauche → bas-droite) d'une ligne de split.
fn brosse_fond_seg(
    rt: &ID2D1RenderTarget,
    rect: &D2D_RECT_F,
    debut: [u8; 4],
    fin: [u8; 4],
) -> Result<ID2D1Brush, ErreurTexte> {
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: couleur_d2d(debut),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: couleur_d2d(fin),
        },
    ];
    let collection: ID2D1GradientStopCollection = unsafe {
        rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }
    .map_err(|e| ErreurTexte::Systeme(format!("stops split : {}", e.message())))?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: D2D_POINT_2F {
            x: rect.left,
            y: rect.top,
        },
        endPoint: D2D_POINT_2F {
            x: rect.right,
            y: rect.bottom,
        },
    };
    let brosse = unsafe { rt.CreateLinearGradientBrush(&props, None, &collection) }
        .map_err(|e| ErreurTexte::Systeme(format!("fond split : {}", e.message())))?;
    Ok(brosse.into())
}

/// Une ligne + son ombre — `text-shadow: 0 1px 2px #000` du CSS (`dy` = 1)
/// ; `dy` = 2 pour le timer (`0 2px 4px var(--sr-ombre)` — ombre colorée
/// passée en `ombre`, pas de flou : limite documentée).
fn dessiner_ligne_sr(
    rt: &ID2D1RenderTarget,
    x: f32,
    y: f32,
    ligne: &LigneSr,
    ombre: &ID2D1Brush,
    brosse: &ID2D1Brush,
    dy: f32,
) {
    unsafe {
        rt.DrawTextLayout(
            D2D_POINT_2F { x, y: y + dy },
            &ligne.layout,
            ombre,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        rt.DrawTextLayout(
            D2D_POINT_2F { x, y },
            &ligne.layout,
            brosse,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }
}

/// Cache des blocs speedrun — déclaré ici pour que `mod.rs` ne connaisse
/// pas le détail (clé = [`cle_bloc`]).
pub(super) type BlocsSpeedrun = HashMap<u64, BlocSpeedrun>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_time_supporte_les_trois_formes() {
        assert_eq!(parse_time("01:23.45"), 83.45);
        assert_eq!(parse_time("1:02:33.5"), 3753.5);
        assert_eq!(parse_time("42.5"), 42.5);
        assert_eq!(parse_time(""), 0.0);
        assert_eq!(parse_time("--/--/--"), 0.0);
        assert_eq!(parse_time("xx"), 0.0);
    }

    #[test]
    fn format_segment_et_cumul() {
        assert_eq!(format_segment(83.45), "01:23.45");
        assert_eq!(format_segment(3753.5), "1:02:33.50");
        assert_eq!(format_segment(-1.0), "--/--/--");
        assert_eq!(format_cumul(83.45), "1:23");
        assert_eq!(format_cumul(3753.5), "1:02:34");
        assert_eq!(format_cumul(-1.0), "");
    }

    #[test]
    fn format_delta_suit_livesplit() {
        let d = PrecisionTemps::Dixieme;
        assert_eq!(format_delta(-2.4, d, true), "-2.4");
        assert_eq!(format_delta(23.1, d, true), "+23.1");
        assert_eq!(format_delta(754.0, d, true), "+12:34");
        assert_eq!(format_delta(-3753.0, d, true), "-1:02:33");
        // drop désactivé : les dixièmes survivent au-delà d'1 min.
        assert_eq!(format_delta(754.5, PrecisionTemps::Dixieme, false), "+12:34.5");
        assert_eq!(format_delta(754.5, PrecisionTemps::Centieme, true), "+12:34");
        assert_eq!(format_delta(59.96, PrecisionTemps::Seconde, true), "+60");
    }

    #[test]
    fn format_timer_prec_reformate_la_chaine() {
        assert_eq!(format_timer_prec("12:34.56", Some(PrecisionTemps::Centieme)), "12:34.56");
        assert_eq!(format_timer_prec("12:34.56", Some(PrecisionTemps::Dixieme)), "12:34.6");
        assert_eq!(format_timer_prec("12:34.56", Some(PrecisionTemps::Seconde)), "12:35");
        assert_eq!(format_timer_prec("1:02:33.44", Some(PrecisionTemps::Dixieme)), "1:02:33.4");
        assert_eq!(format_timer_prec("00:00.00", None), "00:00.00");
    }

    fn etat_avec_lss() -> EtatSpeedrun {
        let mut etat = EtatSpeedrun::nouveau();
        etat.appliquer(MajSpeedrun::Lss {
            nom_jeu: "MGS".to_string(),
            nom_categorie: "Any%".to_string(),
            segments: vec![
                SegmentLss {
                    nom: "Dock".to_string(),
                    pb_real_time: Some(83.45),
                    pb_game_time: None,
                },
                SegmentLss {
                    nom: "Heliport".to_string(),
                    pb_real_time: Some(225.0),
                    pb_game_time: None,
                },
                SegmentLss {
                    nom: "Tank".to_string(),
                    pb_real_time: Some(630.0),
                    pb_game_time: None,
                },
            ],
        });
        etat
    }

    fn cfg_defaut() -> SpeedrunCfg {
        SpeedrunCfg {
            id: "sr".to_string(),
            rect: (0.0, 0.0, 300.0, 700.0),
            z: 2,
            police: "Rajdhani".to_string(),
            taille: 16.0,
            ligne: false,
            accent: ([0x8b, 0x5c, 0xf6, 255], [0x8b, 0x5c, 0xf6, 255]),
            ombre: [0, 0, 0, 255],
            couleur_timer: [0x8b, 0x5c, 0xf6, 255],
            couleur_ahead: OK,
            couleur_behind: [0xc0, 0x39, 0x2b, 255],
            couleur_actif: [0x8b, 0x5c, 0xf6, 255],
            voir_titre: true,
            voir_cat: true,
            voir_statut: true,
            voir_timer: true,
            voir_phase: true,
            voir_compteur: true,
            voir_barre: true,
            mode_temps: ModeTemps::Cumul,
            mode_delta: ModeDelta::Cumul,
            delta_prec: PrecisionTemps::Dixieme,
            delta_drop: true,
            timer_prec: PrecisionTemps::Centieme,
            timer_methode: MethodeTemps::Auto,
            seg_bord_ep: 1.0,
            seg_bord_rayon: RAYON_PETIT,
            nb_lignes: 0,
            scroll_auto: true,
            hauteur_ligne: 0.0,
            cadre: None,
        }
    }

    #[test]
    fn etat_initial_miroir_du_dom() {
        let e = EtatSpeedrun::nouveau();
        assert_eq!(e.phase(), PhaseSpeedrun::NotRunning);
        assert_eq!(e.index(), None);
        assert_eq!(e.real_time(), "00:00.00");
        assert_eq!(e.total_splits(), 0);
        assert!(!e.connecte() && !e.actif());
    }

    #[test]
    fn sequence_started_split_ended() {
        let mut e = etat_avec_lss();
        e.appliquer(MajSpeedrun::Started { index: Some(0) });
        assert_eq!(e.phase(), PhaseSpeedrun::Running);
        assert_eq!(e.index(), Some(0));
        assert!(e.actif());
        e.appliquer(MajSpeedrun::Split {
            time: Some("01:20.00".to_string()),
            cumul: Some("01:20.00".to_string()),
            index: Some(1),
        });
        assert_eq!(e.index(), Some(1));
        assert_eq!(e.split_time(0), "01:20.00");
        assert_eq!(e.split_cumul(0), Some("01:20.00"));
        e.appliquer(MajSpeedrun::SplitSkipped { index: Some(2) });
        assert_eq!(e.index(), Some(2));
        assert_eq!(e.split_cumul(1), None, "skippé = pas de cumul");
        e.appliquer(MajSpeedrun::Ended {
            time: Some("02:00.00".to_string()),
            cumul: Some("06:00.00".to_string()),
            index: Some(3),
        });
        assert_eq!(e.phase(), PhaseSpeedrun::Ended);
        assert_eq!(e.split_cumul(2), Some("06:00.00"));
        assert_eq!(e.seg_time(), None);
    }

    #[test]
    fn reset_goto_et_stopped_reinitialisent() {
        let mut e = etat_avec_lss();
        e.appliquer(MajSpeedrun::Started { index: Some(0) });
        e.appliquer(MajSpeedrun::Reset);
        assert_eq!(e.phase(), PhaseSpeedrun::NotRunning);
        assert_eq!(e.index(), None);
        e.appliquer(MajSpeedrun::Goto {
            index: Some(1),
            phase: Some(PhaseSpeedrun::NotRunning),
        });
        assert_eq!(e.index(), Some(1));
        assert_eq!(e.seg_time(), None, "armé hors run = pas de chrono");
        e.appliquer(MajSpeedrun::Goto {
            index: Some(1),
            phase: Some(PhaseSpeedrun::Running),
        });
        assert_eq!(e.seg_time().as_deref(), Some("00:00.00"));
        e.appliquer(MajSpeedrun::Stopped);
        assert_eq!(e.index(), None);
        assert!(!e.actif() && !e.connecte());
    }

    #[test]
    fn tick_time_met_a_jour_les_chronos() {
        let mut e = etat_avec_lss();
        e.appliquer(MajSpeedrun::Started { index: Some(0) });
        e.appliquer(MajSpeedrun::Time {
            real_time: Some("01:10.50".to_string()),
            game_time: None,
            segment_time: Some(Some("00:30.00".to_string())),
        });
        assert_eq!(e.real_time(), "01:10.50");
        // gameTime null → repli sur le real time (fallback du DOM).
        assert_eq!(e.game_time(), Some("01:10.50"));
        assert_eq!(e.seg_time(), Some("00:30.00"));
        // segmentTime absent → inchangé ; null → effacé.
        e.appliquer(MajSpeedrun::Time {
            real_time: Some("01:11.00".to_string()),
            game_time: Some("01:09.00".to_string()),
            segment_time: None,
        });
        assert_eq!(e.seg_time(), Some("00:30.00"));
        e.appliquer(MajSpeedrun::Time {
            real_time: None,
            game_time: None,
            segment_time: Some(None),
        });
        assert_eq!(e.seg_time(), None);
    }

    #[test]
    fn disconnected_agit_comme_reset_mais_garde_actif() {
        let mut e = etat_avec_lss();
        e.appliquer(MajSpeedrun::Started { index: Some(0) });
        e.appliquer(MajSpeedrun::Connected {
            processus: Some("mgs.exe".to_string()),
        });
        e.appliquer(MajSpeedrun::Disconnected);
        assert!(!e.connecte());
        assert_eq!(e.phase(), PhaseSpeedrun::NotRunning);
        assert_eq!(e.index(), None);
        // Miroir DOM : `disconnected` ne touche pas `srActif`.
        assert!(e.actif());
    }

    #[test]
    fn resoudre_colonnes_cumul_et_delta() {
        let mut e = etat_avec_lss();
        e.appliquer(MajSpeedrun::Started { index: Some(0) });
        e.appliquer(MajSpeedrun::Time {
            real_time: Some("01:00.00".to_string()),
            game_time: None,
            segment_time: None,
        });
        let cfg = cfg_defaut();
        let r = resoudre(&cfg, &e);
        assert_eq!(r.segs.len(), 3);
        assert!(r.delta_col);
        assert!(r.segs[0].actif && !r.segs[0].done);
        // Actif en run, mode cumul : temps = chrono vivant, delta = vivant
        // (60 s) − PB cumulé (83,45) → ahead −23,5.
        assert_eq!(r.segs[0].time, "1:00");
        assert_eq!(r.segs[0].delta, "-23.5");
        assert_eq!(r.segs[0].delta_sens, DeltaSens::Ahead);
        // Non atteint : temps = PB cumulé, delta vide.
        assert_eq!(r.segs[1].time, "3:45");
        assert!(r.segs[1].delta.is_empty());
        assert_eq!(r.timer.as_deref(), Some("01:00.00"));
        assert_eq!(r.compteur.as_deref(), Some("0 / 3"));
        assert_eq!(r.barre, Some(0.0));
    }

    #[test]
    fn resoudre_apres_split_delta_boucle() {
        let mut e = etat_avec_lss();
        e.appliquer(MajSpeedrun::Started { index: Some(0) });
        e.appliquer(MajSpeedrun::Split {
            time: Some("01:30.00".to_string()),
            cumul: Some("01:30.00".to_string()),
            index: Some(1),
        });
        let cfg = cfg_defaut();
        let r = resoudre(&cfg, &e);
        // Split 0 bouclé en 90 s vs PB 83,45 → behind +6,5 ; temps = cumul figé.
        assert_eq!(r.segs[0].time, "1:30");
        assert_eq!(r.segs[0].delta, "+6.5");
        assert_eq!(r.segs[0].delta_sens, DeltaSens::Behind);
        assert!(r.segs[0].done && !r.segs[0].actif);
        assert!(r.segs[1].actif);
        // Barre : 1 / 3.
        assert_eq!(r.barre.map(|p| p.round()), Some(33.0));
    }

    #[test]
    fn resoudre_mode_segment_affiche_durees() {
        let mut e = etat_avec_lss();
        e.appliquer(MajSpeedrun::Started { index: Some(0) });
        e.appliquer(MajSpeedrun::Split {
            time: Some("01:30.00".to_string()),
            cumul: Some("01:30.00".to_string()),
            index: Some(1),
        });
        e.appliquer(MajSpeedrun::Time {
            real_time: Some("02:00.00".to_string()),
            game_time: None,
            segment_time: Some(Some("00:30.00".to_string())),
        });
        let mut cfg = cfg_defaut();
        cfg.mode_temps = ModeTemps::Segment;
        cfg.mode_delta = ModeDelta::Segment;
        let r = resoudre(&cfg, &e);
        // Durée figée du split 0 (mode segment).
        assert_eq!(r.segs[0].time, "01:30.00");
        // Delta segment : durée 90 − PB segment 83,45 → +6,5 (arrondi 0,1).
        assert_eq!(r.segs[0].delta, "+6.5");
        // Actif : chrono de segment vivant ; delta 30 − 141,55 = −111,55,
        // drop des décimales au-delà d'1 min (LiveSplit) → « -1:51 ».
        assert_eq!(r.segs[1].time, "00:30.00");
        assert_eq!(r.segs[1].delta, "-1:51");
        // Non atteint : durée PB du segment 2 = 630−225 = 405 → 06:45.00.
        assert_eq!(r.segs[2].time, "06:45.00");
    }

    #[test]
    fn resoudre_toggles_et_fenetre() {
        let mut e = etat_avec_lss();
        e.appliquer(MajSpeedrun::Started { index: Some(2) });
        let mut cfg = cfg_defaut();
        cfg.voir_timer = false;
        cfg.mode_delta = ModeDelta::Off;
        cfg.nb_lignes = 1;
        let r = resoudre(&cfg, &e);
        assert!(r.timer.is_none());
        assert!(!r.delta_col);
        // Fenêtre de 1 ligne, actif en 2e position : seul le PRÉCÉDENT
        // est visible (comportement exact de `srNbLignes` du DOM).
        assert!(r.segs[1].visible && !r.segs[0].visible && !r.segs[2].visible);
    }

    #[test]
    fn sens_du_delta_arrondi() {
        assert_eq!(sens_delta(-0.1), DeltaSens::Ahead);
        assert_eq!(sens_delta(0.0), DeltaSens::Neutre);
        assert_eq!(sens_delta(0.4), DeltaSens::Behind);
    }
}
