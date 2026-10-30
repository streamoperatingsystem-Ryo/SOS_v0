//! Widget « goal » natif — entête + compteur/cible + barre + statut + célébration.
//!
//! Miroir de `.widget.goal` et de `majGoalValeur` / `goalLabelEntete` /
//! `goalValeur` / `goalCelebrer` (diffusion.html) :
//! - entête : logo de plateforme (glyphes SVG Twitch/Kick, badges YT/TT) +
//!   libellé en MAJUSCULES (`0.06em`) — description du goal Twitch en mode
//!   lié, libellé de la métrique en mode manuel ;
//! - chiffres : `actuel / cible` gras (`N/A` jamais mesuré, `—` sans cible) ;
//! - barre : piste 78 % pilule, remplissage dégradé `--sr-grad-*` + liseré
//!   lumineux quand le compteur atteint la cible (`goal-plein`) ;
//! - statut « Terminé » (mode lié, `channel.goal.end`) — invisible sinon ;
//! - célébration ~6 s sur frame `goal-atteint` : voile 0,55 + texte accent.
//!
//! Les valeurs sont **live** : l'app pousse les frames `communaute`,
//! `viewers`, `goal-twitch`, `goal-atteint` dans [`EtatGoals`] pendant la
//! session (même motif que `FileChat` / `FileInput`) — jamais de
//! reconfiguration de scène pour un compteur.
//!
//! Limites assumées :
//! - `box-shadow` du liseré `goal-plein` ≈ halo translucide élargi (même
//!   approximation que le halo du séparateur horloge) ;
//! - le pulse `goalGlow` (brightness 1,35 ×5) n'est pas porté — la
//!   célébration est le voile + le texte (limite D2D : pas de filtre
//!   luminosité sur le contenu déjà composé) ;
//! - `toLocaleString("fr-FR")` ≈ groupes de 3 chiffres séparés par une
//!   espace fine insécable U+202F ;
//! - poids 500 du séparateur/cible ≈ 400 (StyleTexte n'a que gras/normal) ;
//! - le logo Kick est dessiné en `currentColor` DOM (#53fc18 en dur dans le
//!   SVG) : on garde la couleur marque, pas la couleur d'entête.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FILL_MODE_ALTERNATE,
    D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Factory1, ID2D1Geometry, ID2D1GeometrySink, ID2D1GradientStopCollection,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, ID2D1RenderTarget,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS1_NONE,
    D2D1_LAYER_PARAMETERS1, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1, DWRITE_LINE_METRICS,
    DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::cadres::CadreCfg;

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

/// Durée de la célébration (`setTimeout(…, 6000)` de `goalCelebrer`).
const DUREE_CELEBRATION_S: f32 = 6.0;
/// `--texte` de la page diffusion (#e0e0e0).
const TEXTE: [u8; 4] = [0xe0, 0xe0, 0xe0, 255];
/// `--gris` de la page diffusion (#9a9a9a).
const GRIS: [u8; 4] = [0x9a, 0x9a, 0x9a, 255];
/// Texte « Terminé » : `--texte` sous `opacity: 0.75`.
const TEXTE_STATUT: [u8; 4] = [0xe0, 0xe0, 0xe0, 191];
/// Texte de la célébration — `goalCelebrer` (« 🎉 Objectif atteint ! »).
const TEXTE_CELEBRATION: &str = "🎉 Objectif atteint !";

// =============================================================================
// ÉTAT LIVE — miroir des tables de diffusion.html alimentées par les frames
// `chat_tx` (viewers.rs / twitch_eventsub.rs).
// =============================================================================

/// Compteurs communauté d'une plateforme — sous-ensemble des champs de la
/// frame `{"type":"communaute",…}` (viewers.rs). Champ absent de la frame =
/// métrique non mesurée → « N/A » (jamais 0 inventé, comme `goalValeur`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Communaute {
    pub followers: Option<u64>,
    pub subs: Option<u64>,
    pub sub_points: Option<u64>,
    pub abonnes: Option<u64>,
}

/// Creator Goal Twitch lié — frame `{"type":"goal-twitch",…}` (EventSub
/// channel.goal.* + poll viewers.rs). `atteint` est délibérément absent : le
/// DOM n'en lit jamais rien côté widget (verdict lu via `actuel`/`cible`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalTwitch {
    pub id: String,
    /// `goal_type` Helix (follower, subscription, new_bit, …) — repli du
    /// libellé quand `description` est vide.
    pub goal_type: String,
    pub description: String,
    pub actuel: u64,
    pub cible: u64,
    /// `channel.goal.end` (ou goal retiré du GET /goals) → statut « Terminé ».
    pub termine: bool,
}

/// Une mutation de l'état goal — une variante par frame `chat_tx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MajGoal {
    /// `{"type":"communaute","plateforme":p,…}` — remplace les compteurs de
    /// la plateforme (les champs absents redeviennent non mesurés).
    Communaute { plateforme: String, compteurs: Communaute },
    /// `{"type":"viewers","plateforme":p,"count":n|null}` — null = hors-ligne.
    Viewers { plateforme: String, count: Option<u64> },
    /// `{"type":"goal-twitch",…}` — progression d'un Creator Goal déclaré.
    Twitch(GoalTwitch),
    /// `{"type":"goal-atteint","widget_id":…}` — célébration one-shot ~6 s.
    Atteint { widget_id: String },
}

/// État live des widgets goal — miroir de `communauteParPlateforme` +
/// `viewersParPlateforme` + `goalsTwitchMap` + le timer de `goalCelebrer`.
///
/// L'app pousse pendant l'enregistrement ; le rendu relit à chaque frame.
/// `version` ne bouge que sur changement réel (dedup — les frames sont déjà
/// dédupliquées côté viewers.rs, le garde-fou reste utile aux tests).
#[derive(Debug, Default)]
pub struct EtatGoals {
    communaute: HashMap<String, Communaute>,
    /// `Some(None)` = plateforme vue mais hors-ligne (frame `count: null`).
    viewers: HashMap<String, Option<u64>>,
    twitch: HashMap<String, GoalTwitch>,
    /// Instant de début de la dernière célébration par id de widget.
    celebrations: HashMap<String, Instant>,
    version: u64,
}

impl EtatGoals {
    pub fn nouveau() -> Self {
        Self::default()
    }

    /// Applique une frame ; `version` n'augmente que si l'état a changé
    /// (la célébration change toujours l'état : c'est un événement).
    pub fn appliquer(&mut self, maj: MajGoal) {
        match maj {
            MajGoal::Communaute { plateforme, compteurs } => {
                if self.communaute.get(&plateforme) == Some(&compteurs) {
                    return;
                }
                self.communaute.insert(plateforme, compteurs);
            }
            MajGoal::Viewers { plateforme, count } => {
                if self.viewers.get(&plateforme) == Some(&count) {
                    return;
                }
                self.viewers.insert(plateforme, count);
            }
            MajGoal::Twitch(goal) => {
                if self.twitch.get(&goal.id) == Some(&goal) {
                    return;
                }
                self.twitch.insert(goal.id.clone(), goal);
            }
            MajGoal::Atteint { widget_id } => {
                // clearTimeout + re-arm du DOM : chaque frame relance la
                // fenêtre ; les entrées expirées sont purgées au passage.
                self.celebrations
                    .retain(|_, t| t.elapsed().as_secs_f32() < DUREE_CELEBRATION_S);
                self.celebrations.insert(widget_id, Instant::now());
            }
        }
        self.version = self.version.wrapping_add(1);
    }

    /// Compteur monotone (clé de cache du rendu).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Valeur live d'une métrique pour une plateforme — `goalValeur` :
    /// `viewers` vient des frames `viewers`, le reste des frames
    /// `communaute`. `None` = jamais mesuré / hors-ligne / métrique absente
    /// → « N/A » (jamais de valeur fabriquée).
    pub fn valeur(&self, plateforme: &str, metrique: &str) -> Option<u64> {
        if metrique == "viewers" {
            return self.viewers.get(plateforme).copied().flatten();
        }
        let c = self.communaute.get(plateforme)?;
        match metrique {
            "followers" => c.followers,
            "subs" => c.subs,
            "sub_points" => c.sub_points,
            "abonnes" => c.abonnes,
            _ => None,
        }
    }

    /// Creator Goal déclaré (`goalId` du widget) — `goalsTwitchMap`.
    pub fn goal_twitch(&self, id: &str) -> Option<&GoalTwitch> {
        self.twitch.get(id)
    }

    /// Fenêtre de célébration active (~6 s après une frame `goal-atteint`).
    pub fn en_celebration(&self, widget_id: &str) -> bool {
        self.celebrations
            .get(widget_id)
            .is_some_and(|t| t.elapsed().as_secs_f32() < DUREE_CELEBRATION_S)
    }
}

// =============================================================================
// CONFIG — extraite du Widget (scène) par `goals_depuis_scene`.
// =============================================================================

/// Configuration d'un widget `type: "goal"`, extraite de la scène.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalCfg {
    /// Id du widget — clé des frames `goal-atteint` (célébration ciblée).
    pub id: String,
    /// Rect du widget en px canvas.
    pub rect: (f64, f64, f64, f64),
    /// Ordre de composition (`w.z`).
    pub z: i32,
    /// `goalPlateforme || "twitch"`.
    pub plateforme: String,
    /// `goalMetrique || "followers"` — ignoré en mode lié.
    pub metrique: String,
    /// `goalCible || 0` — 0 = compteur seul (« — », jamais `goal-atteint`).
    pub cible: u64,
    /// `goalId` — mode lié : valeurs via `goal-twitch`, métrique/cible
    /// manuels ignorés (contrat du widget).
    pub goal_id: Option<String>,
    /// `--sr-grad-from`/`--sr-grad-to` (`gradientCadre`) — barre + célébration.
    pub accent: ([u8; 4], [u8; 4]),
    /// Cadre à l'échelle « bulle » (`cadreGoal` = `cadreMsgScale`) — clip du
    /// contenu, trait tracé par le calque cadre au même z.
    pub cadre: Option<CadreCfg>,
}

/// Valeur résolue d'un widget goal — miroir de `majGoalValeur`.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalRendu {
    /// `None` = « N/A » (jamais mesuré / hors-ligne / métrique absente).
    pub actuel: Option<u64>,
    /// Cible effective (0 = « — », barre vide).
    pub cible: u64,
    /// Statut « Terminé » (mode lié seulement).
    pub termine: bool,
    /// Libellé d'entête (avant `text-transform: uppercase`).
    pub label: String,
}

/// `GOAL_MET_LABELS` — libellé d'entête du mode manuel.
fn label_metrique(metrique: &str) -> Option<&'static str> {
    Some(match metrique {
        "followers" => "Followers",
        "subs" => "Subs",
        "sub_points" => "Points de subs",
        "abonnes" => "Abonnés",
        "viewers" => "Viewers",
        _ => return None,
    })
}

/// `GOAL_TTYPE_LABELS` — repli quand `description` du goal lié est vide.
fn label_type_goal(goal_type: &str) -> Option<&'static str> {
    Some(match goal_type {
        "follower" => "Followers",
        "subscription" => "Subs",
        "subscription_count" => "Subs (total)",
        "new_subscription" => "Nouveaux subs",
        "new_subscription_count" => "Nouveaux subs (total)",
        "new_bit" => "Bits",
        "new_cheerer" => "Cheerers",
        _ => return None,
    })
}

/// Résout actuel/cible/terminé/label — miroir de `majGoalValeur` +
/// `goalLabelEntete` (mode lié : description du goal, sinon label du type,
/// sinon « Objectif » ; jamais vue → « Objectif Twitch »).
pub fn resoudre(cfg: &GoalCfg, etat: &EtatGoals) -> GoalRendu {
    if let Some(id) = &cfg.goal_id {
        return match etat.goal_twitch(id) {
            Some(g) => GoalRendu {
                actuel: Some(g.actuel),
                cible: g.cible,
                termine: g.termine,
                label: if g.description.is_empty() {
                    label_type_goal(&g.goal_type).unwrap_or("Objectif").to_string()
                } else {
                    g.description.clone()
                },
            },
            None => GoalRendu {
                actuel: None,
                cible: 0,
                termine: false,
                label: "Objectif Twitch".to_string(),
            },
        };
    }
    GoalRendu {
        actuel: etat.valeur(&cfg.plateforme, &cfg.metrique),
        cible: cfg.cible,
        termine: false,
        label: label_metrique(&cfg.metrique).unwrap_or("Objectif").to_string(),
    }
}

/// `toLocaleString("fr-FR")` : groupes de 3 chiffres, espace fine insécable.
pub fn format_nombre(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

/// Glyphe de plateforme de l'entête (`logoPlateforme`) : les badges YT/TT
/// sont du texte — seuls Twitch et Kick ont un logo vectoriel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum LogoGoal {
    Twitch,
    Kick,
}

/// `logoPlateforme(p)` — SVG pour twitch/kick, badge texte youtube/tiktok.
fn badge_plateforme(plateforme: &str) -> Option<&'static str> {
    match plateforme {
        "youtube" => Some("YT"),
        "tiktok" => Some("TT"),
        _ => None,
    }
}

pub(super) fn logo_plateforme(plateforme: &str) -> Option<LogoGoal> {
    match plateforme {
        "twitch" => Some(LogoGoal::Twitch),
        "kick" => Some(LogoGoal::Kick),
        _ => None,
    }
}

// Contours des glyphes, dans leur viewBox (Twitch 16×16, Kick 24×24) —
// copie exacte des `d` des SVG inline de `logoPlateforme`.
const TWITCH_EXTERNE: &[(f32, f32)] = &[
    (3.0, 1.0), (1.5, 3.5), (1.5, 13.0), (4.5, 13.0), (4.5, 15.0), (6.0, 15.0),
    (8.0, 13.0), (10.5, 13.0), (13.5, 10.0), (13.5, 1.0),
];
const TWITCH_INTERNE: &[(f32, f32)] = &[
    (12.0, 9.5), (10.0, 11.5), (7.0, 11.5), (5.0, 13.5), (5.0, 11.5),
    (3.0, 11.5), (3.0, 2.0), (12.0, 2.0),
];
const TWITCH_OEIL_G: &[(f32, f32)] = &[(6.0, 4.0), (7.0, 4.0), (7.0, 7.0), (6.0, 7.0)];
const TWITCH_OEIL_D: &[(f32, f32)] = &[(9.0, 4.0), (10.0, 4.0), (10.0, 7.0), (9.0, 7.0)];
const KICK: &[(f32, f32)] = &[
    (5.0, 2.0), (9.0, 2.0), (9.0, 8.0), (11.0, 8.0), (11.0, 6.0), (13.0, 6.0),
    (13.0, 4.0), (15.0, 4.0), (15.0, 2.0), (19.0, 2.0), (19.0, 8.0), (17.0, 8.0),
    (17.0, 10.0), (15.0, 10.0), (15.0, 14.0), (17.0, 14.0), (17.0, 16.0),
    (19.0, 16.0), (19.0, 22.0), (15.0, 22.0), (15.0, 20.0), (13.0, 20.0),
    (13.0, 18.0), (11.0, 18.0), (11.0, 22.0), (5.0, 22.0),
];

/// ViewBox carrée du glyphe (les deux logos sont des viewBox carrés).
pub(super) fn viewbox_logo(logo: LogoGoal) -> f32 {
    match logo {
        LogoGoal::Twitch => 16.0,
        LogoGoal::Kick => 24.0,
    }
}

/// Construit le glyphe dans sa viewBox (une fois par plateforme, le scale
/// est posé par `SetTransform` au dessin — jamais de géométrie par frame).
pub(super) fn creer_logo(
    d2d: &ID2D1Factory1,
    logo: LogoGoal,
) -> Result<ID2D1PathGeometry, ErreurTexte> {
    let geo: ID2D1PathGeometry = unsafe { d2d.CreatePathGeometry() }
        .and_then(|g| g.cast())
        .map_err(|e| ErreurTexte::Gpu(format!("CreatePathGeometry logo : {}", e.message())))?;
    let sink: ID2D1GeometrySink = unsafe { geo.Open() }
        .map_err(|e| ErreurTexte::Gpu(format!("Open logo : {}", e.message())))?;
    let figure = |points: &[(f32, f32)]| unsafe {
        sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
        sink.BeginFigure(
            D2D_POINT_2F {
                x: points[0].0,
                y: points[0].1,
            },
            D2D1_FIGURE_BEGIN_FILLED,
        );
        for &(x, y) in &points[1..] {
            sink.AddLine(D2D_POINT_2F { x, y });
        }
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    };
    match logo {
        LogoGoal::Twitch => {
            figure(TWITCH_EXTERNE);
            // Sous-chemin en ALTERNATE = trou du logo (non dépendant du
            // sens de parcours — la règle even-odd le garantit).
            figure(TWITCH_INTERNE);
            figure(TWITCH_OEIL_G);
            figure(TWITCH_OEIL_D);
        }
        LogoGoal::Kick => figure(KICK),
    }
    unsafe { sink.Close() }
        .map_err(|e| ErreurTexte::Gpu(format!("Close logo : {}", e.message())))?;
    Ok(geo)
}

/// Couleur de remplissage du glyphe — `fill` des SVG inline : Twitch =
/// `currentColor` (couleur d'entête `--gris`), Kick = marque #53fc18.
pub(super) fn couleur_logo(logo: LogoGoal) -> [u8; 4] {
    match logo {
        LogoGoal::Twitch => GRIS,
        LogoGoal::Kick => [0x53, 0xfc, 0x18, 255],
    }
}

// =============================================================================
// DESSIN
// =============================================================================

/// Un texte mis en page du widget (même motif que `LigneHorloge`).
struct LigneGoal {
    texte: String,
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    w: f32,
    h: f32,
    base: f32,
}

/// Contenus résolus d'un widget goal pour une frame — entrée de
/// `synchroniser_bloc_goal` (les slots vides deviennent `None`).
struct TextesGoal {
    /// Libellé d'entête en MAJUSCULES (`text-transform`).
    entete: String,
    /// Badge plateforme (« YT » / « TT ») — `None` pour twitch/kick (logo
    /// vectoriel) et les plateformes sans badge.
    badge: Option<String>,
    /// Compteur actuel formaté (« N/A » si jamais mesuré).
    actuel: String,
    /// « /cible » (ou « /— » sans cible).
    suite: String,
    /// « Terminé » en mode lié terminé.
    statut: Option<String>,
    /// « 🎉 Objectif atteint ! » pendant la fenêtre de célébration.
    celeb: Option<String>,
}

/// Les lignes optionnelles du widget — les slots se resynchronisent par
/// comparaison de contenu (compteur live, aucun re-layout par frame).
#[derive(Default)]
pub(super) struct BlocGoal {
    /// Libellé d'entête (uppercase).
    entete: Option<LigneGoal>,
    /// Badge texte (« YT » / « TT ») — les logos vectoriels n'en ont pas.
    badge: Option<LigneGoal>,
    /// Compteur actuel (gras, `--texte`).
    actuel: Option<LigneGoal>,
    /// « / cible » (gris) — séparateur et cible même style DOM.
    suite: Option<LigneGoal>,
    /// « Terminé » (mode lié terminé).
    statut: Option<LigneGoal>,
    /// « 🎉 Objectif atteint ! » (fenêtre de célébration).
    celeb: Option<LigneGoal>,
}

#[derive(Clone, Copy)]
enum SlotGoal {
    Entete,
    Badge,
    Actuel,
    Suite,
    Statut,
    Celeb,
}

/// Clé de cache d'un bloc : id du widget + tailles (dérivées de h) +
/// plateforme (présence du badge). Le contenu n'y figure pas — il est
/// comparé chaîne à chaîne, comme `cle_bloc` de l'horloge.
fn cle_bloc(cfg: &GoalCfg) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cfg.id.hash(&mut h);
    (cfg.rect.3 as f32).to_bits().hash(&mut h);
    cfg.plateforme.hash(&mut h);
    h.finish()
}

impl DessinateurTexte {
    /// Dessine un widget goal — dans une passe ouverte.
    ///
    /// Contenu rogné au rect (`overflow` implicite du widget DOM) et au
    /// polygone du cadre à l'échelle bulle (`cadreGoal`) — même motif de
    /// clip + couche que `dessiner_horloge`.
    pub fn dessiner_goal(
        &mut self,
        cfg: &GoalCfg,
        etat: &EtatGoals,
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
                .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer goal : {}", e.message())))?;
            unsafe { self.ctx.PushLayer(&params, &layer) };
        }
        let resultat = self.dessiner_goal_interne(cfg, etat, &zone);
        if avec_couche {
            unsafe { self.ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        resultat
    }

    /// Contenu du widget — sous les clips déjà posés.
    fn dessiner_goal_interne(
        &mut self,
        cfg: &GoalCfg,
        etat: &EtatGoals,
        zone: &D2D_RECT_F,
    ) -> Result<(), ErreurTexte> {
        let (zx, zy, zw, zh) = (zone.left, zone.top, zone.right - zone.left, zone.bottom - zone.top);
        let rendu = resoudre(cfg, etat);
        let celebration = etat.en_celebration(&cfg.id);

        // Contenus (chaînés à `synchroniser_bloc_goal` — zéro re-layout si
        // rien n'a changé, comme le tick seconde de l'horloge).
        let textes = TextesGoal {
            entete: rendu.label.to_uppercase(),
            badge: badge_plateforme(&cfg.plateforme).map(str::to_string),
            actuel: rendu.actuel.map(format_nombre).unwrap_or_else(|| "N/A".to_string()),
            suite: format!(
                "/{}",
                if rendu.cible > 0 {
                    format_nombre(rendu.cible)
                } else {
                    "—".to_string()
                }
            ),
            statut: rendu.termine.then(|| "Terminé".to_string()),
            celeb: celebration.then(|| TEXTE_CELEBRATION.to_string()),
        };

        // Glyphe pré-chargé AVANT l'emprunt de `blocs_goal` (le cache logo
        // vit dans le même `DessinateurTexte` — retour possédé AddRef).
        let logo = logo_plateforme(&cfg.plateforme);
        let geo = logo.map(|l| self.logo_goal(l)).transpose()?;
        self.synchroniser_bloc_goal(cfg, &textes)?;
        let cle = cle_bloc(cfg);
        let Some(bloc) = self.blocs_goal.get(&cle) else {
            return Ok(());
        };

        // --- Géométrie des rangées (calc(var(--goal-h) * N) du CSS) -------
        let gap = 0.06 * zh;
        let logo_cote = 0.16 * zh;
        let gap_entete = 0.06 * zh;
        let bar_w = 0.78 * zw;
        let bar_h = (0.07 * zh).max(4.0);

        let entete_h = bloc
            .entete
            .as_ref()
            .map(|l| l.h)
            .unwrap_or(0.0)
            .max(logo_cote);
        let chiffres_h = bloc.actuel.as_ref().map(|l| l.h).unwrap_or(0.0);
        let statut_h = bloc.statut.as_ref().map(|l| l.h).unwrap_or(0.0);
        let mut total = entete_h + gap + chiffres_h + gap + bar_h;
        if statut_h > 0.0 {
            total += gap + statut_h;
        }
        // `justify-content: center` : le bloc entier est centré en hauteur.
        let mut cy = zy + (zh - total) / 2.0;
        let cx = zx + zw / 2.0;

        // --- Brosses -------------------------------------------------------
        let brosse = |c: [u8; 4], nom: &str| -> Result<ID2D1Brush, ErreurTexte> {
            unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(c), None) }
                .map_err(|e| ErreurTexte::Systeme(format!("{nom} : {}", e.message())))
                .map(|b| b.into())
        };
        let ombre = brosse([0, 0, 0, 255], "brosse ombre goal")?;
        let texte = brosse(TEXTE, "brosse texte goal")?;
        let gris = brosse(GRIS, "brosse gris goal")?;
        let accent = brosse(cfg.accent.0, "brosse accent goal")?;

        // --- Entête : logo/badge + label -----------------------------------
        // `.plat-badge` : padding 0 0.2rem (3,2 px), radius 0.15rem (2,4 px),
        // line-height 1.4 — rem fixe, indépendant de --goal-h.
        let puce = if let Some(b) = &bloc.badge {
            (b.w + 6.4, 0.14 * zh * 1.4)
        } else if logo.is_some() {
            (logo_cote, logo_cote)
        } else {
            (0.0, 0.0)
        };
        let entete_w = puce.0 + gap_entete + bloc.entete.as_ref().map(|l| l.w).unwrap_or(0.0);
        let mut px = cx - entete_w / 2.0;
        if let Some(b) = &bloc.badge {
            let fond = match cfg.plateforme.as_str() {
                "youtube" => [0xff, 0, 0, 255],
                _ => [0, 0, 0, 255], // tiktok : badge noir (`.plat-badge.tiktok`).
            };
            let brosse_fond = brosse(fond, "brosse badge goal")?;
            let badge_rect = D2D_RECT_F {
                left: px,
                top: cy + (entete_h - puce.1) / 2.0,
                right: px + puce.0,
                bottom: cy + (entete_h - puce.1) / 2.0 + puce.1,
            };
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: badge_rect,
                        radiusX: 2.4,
                        radiusY: 2.4,
                    },
                    &brosse_fond,
                )
            };
            dessiner_ligne_goal(
                &self.cible_rt,
                px + (puce.0 - b.w) / 2.0,
                cy + (entete_h - b.h) / 2.0,
                b,
                &ombre,
                &texte,
            );
            px += puce.0 + gap_entete;
        } else if let (Some(logo), Some(geo)) = (logo, &geo) {
            let brosse_logo = brosse(couleur_logo(logo), "brosse logo goal")?;
            let boite = viewbox_logo(logo);
            let echelle = logo_cote / boite;
            let transform = Matrix3x2 {
                M11: echelle,
                M22: echelle,
                M31: px,
                M32: cy + (entete_h - logo_cote) / 2.0,
                ..Matrix3x2::identity()
            };
            unsafe {
                self.cible_rt.SetTransform(&transform);
                self.cible_rt.FillGeometry(geo, &brosse_logo, None);
                self.cible_rt.SetTransform(&Matrix3x2::identity());
            }
            px += puce.0 + gap_entete;
        }
        if let Some(l) = &bloc.entete {
            dessiner_ligne_goal(&self.cible_rt, px, cy + (entete_h - l.h) / 2.0, l, &ombre, &gris);
        }
        cy += entete_h + gap;

        // --- Chiffres : « actuel / cible » centrés --------------------------
        let chiffres_w = bloc.actuel.as_ref().map(|l| l.w).unwrap_or(0.0)
            + bloc.suite.as_ref().map(|l| l.w).unwrap_or(0.0);
        let mut sx = cx - chiffres_w / 2.0;
        if let Some(a) = &bloc.actuel {
            dessiner_ligne_goal(&self.cible_rt, sx, cy + (chiffres_h - a.h) / 2.0, a, &ombre, &texte);
            sx += a.w;
        }
        if let Some(s) = &bloc.suite {
            // Alignée sur la ligne de base du compteur (même taille de fonte).
            let top = bloc
                .actuel
                .as_ref()
                .map(|a| cy + (chiffres_h - a.h) / 2.0 + a.base - s.base)
                .unwrap_or(cy);
            dessiner_ligne_goal(&self.cible_rt, sx, top, s, &ombre, &gris);
        }
        cy += chiffres_h + gap;

        // --- Barre de progression -------------------------------------------
        let bar_x = cx - bar_w / 2.0;
        let piste = D2D_RECT_F {
            left: bar_x,
            top: cy,
            right: bar_x + bar_w,
            bottom: cy + bar_h,
        };
        let brosse_piste = brosse([0xe0, 0xe0, 0xe0, 0x52], "brosse piste goal")?;
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: piste,
                    radiusX: bar_h / 2.0,
                    radiusY: bar_h / 2.0,
                },
                &brosse_piste,
            )
        };
        // pct = min(100, round(actuel/cible*100)) — 0 sans cible ni mesure.
        let pct = match (rendu.actuel, rendu.cible) {
            (Some(a), c) if c > 0 => (((a as f64 / c as f64) * 100.0).round() as u64).min(100),
            _ => 0,
        };
        if pct > 0 {
            let fill_w = bar_w * pct as f32 / 100.0;
            let fill = D2D_RECT_F {
                left: bar_x,
                top: cy,
                right: bar_x + fill_w,
                bottom: cy + bar_h,
            };
            // `goal-plein` : liseré lumineux ≈ box-shadow 0 0 0.03em accent.
            if pct >= 100 {
                let halo_r = (0.03 * zh).max(2.0);
                let mut c = cfg.accent.0;
                c[3] = 0x60;
                let halo = brosse(c, "brosse halo goal")?;
                unsafe {
                    self.cible_rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: fill.left - halo_r,
                                top: fill.top - halo_r,
                                right: fill.right + halo_r,
                                bottom: fill.bottom + halo_r,
                            },
                            radiusX: bar_h / 2.0 + halo_r,
                            radiusY: bar_h / 2.0 + halo_r,
                        },
                        &halo,
                    )
                };
            }
            // Dégradé 90° du fill — borné à SA propre largeur (gradient CSS
            // relatif à l'élément, pas à la piste).
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: couleur_d2d(cfg.accent.0),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: couleur_d2d(cfg.accent.1),
                },
            ];
            let collection: ID2D1GradientStopCollection = unsafe {
                self.cible_rt.CreateGradientStopCollection(
                    &stops,
                    D2D1_GAMMA_2_2,
                    D2D1_EXTEND_MODE_CLAMP,
                )
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("stops goal : {}", e.message()))
            })?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: D2D_POINT_2F { x: bar_x, y: cy },
                endPoint: D2D_POINT_2F {
                    x: bar_x + fill_w,
                    y: cy,
                },
            };
            let degrade: ID2D1LinearGradientBrush = unsafe {
                self.cible_rt.CreateLinearGradientBrush(&props, None, &collection)
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("dégradé goal : {}", e.message()))
            })?;
            let degrade: ID2D1Brush = degrade.into();
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: fill,
                        radiusX: bar_h / 2.0,
                        radiusY: bar_h / 2.0,
                    },
                    &degrade,
                )
            };
        }
        cy += bar_h;

        // --- Statut « Terminé » (mode lié) -----------------------------------
        if let Some(s) = &bloc.statut {
            cy += gap;
            let brosse_statut = brosse(TEXTE_STATUT, "brosse statut goal")?;
            dessiner_ligne_goal(&self.cible_rt, cx - s.w / 2.0, cy, s, &ombre, &brosse_statut);
        }

        // --- Célébration : voile + texte, au-dessus de tout ------------------
        if let Some(c) = &bloc.celeb {
            let voile = brosse([0, 0, 0, 0x8c], "brosse voile goal")?;
            unsafe { self.cible_rt.FillRectangle(zone, &voile) };
            dessiner_ligne_goal(
                &self.cible_rt,
                cx - c.w / 2.0,
                zy + (zh - c.h) / 2.0,
                c,
                &ombre,
                &accent,
            );
        }
        Ok(())
    }

    /// Glyphe de plateforme (viewBox) — créé à la demande, partagé par tous
    /// les widgets goal (le scale est posé au dessin). Retour **possédé**
    /// (AddRef) : le bloc de texte garde son emprunt de `blocs_goal`.
    fn logo_goal(&mut self, logo: LogoGoal) -> Result<ID2D1PathGeometry, ErreurTexte> {
        if !self.logos_goal.contains_key(&logo) {
            let geo = creer_logo(&self.d2d, logo)?;
            self.logos_goal.insert(logo, geo);
        }
        Ok(self.logos_goal[&logo].clone())
    }

    /// (Re)construit les lignes dont le contenu a changé — au plus une
    /// recréation par mutation `EtatGoals` (ou changement de scène).
    fn synchroniser_bloc_goal(
        &mut self,
        cfg: &GoalCfg,
        textes: &TextesGoal,
    ) -> Result<(), ErreurTexte> {
        let zh = cfg.rect.3 as f32;
        let style_entete = StyleTexte {
            police: "Segoe UI".to_string(),
            taille: 0.14 * zh,
            gras: false,
            espacement: 0.06 * 0.14 * zh,
            couleur: GRIS,
            contour_epaisseur: 0.0,
            ..StyleTexte::default()
        };
        let style_badge = StyleTexte {
            taille: 0.14 * zh,
            gras: true,
            espacement: 0.0,
            couleur: TEXTE,
            ..style_entete.clone()
        };
        let style_actuel = StyleTexte {
            taille: 0.26 * zh,
            gras: true,
            espacement: 0.0,
            couleur: TEXTE,
            ..style_entete.clone()
        };
        // `.goal-sep`/`.goal-cible` : --gris, font-weight 500 ≈ normal.
        let style_suite = StyleTexte {
            gras: false,
            couleur: GRIS,
            ..style_actuel.clone()
        };
        let style_statut = StyleTexte {
            taille: 0.11 * zh,
            espacement: 0.05 * 0.11 * zh,
            couleur: TEXTE_STATUT,
            ..style_entete.clone()
        };
        let style_celeb = StyleTexte {
            taille: 0.16 * zh,
            gras: true,
            espacement: 0.0,
            couleur: cfg.accent.0,
            ..style_entete.clone()
        };

        // Table bornée : les clés ne divergent que par (widget, h, plateforme).
        if self.blocs_goal.len() > 32 {
            self.blocs_goal.clear();
        }
        let cle = cle_bloc(cfg);
        let mut maj: Vec<(SlotGoal, StyleTexte, String)> = Vec::new();
        {
            let bloc = self.blocs_goal.entry(cle).or_default();
            let mut changer = |slot: &mut Option<LigneGoal>,
                               kind: SlotGoal,
                               texte: Option<&str>,
                               style: &StyleTexte| {
                if slot.as_ref().map(|l| l.texte.as_str()) != texte {
                    *slot = None;
                    if let Some(t) = texte {
                        maj.push((kind, style.clone(), t.to_string()));
                    }
                }
            };
            changer(&mut bloc.entete, SlotGoal::Entete, Some(&textes.entete), &style_entete);
            changer(&mut bloc.badge, SlotGoal::Badge, textes.badge.as_deref(), &style_badge);
            changer(&mut bloc.actuel, SlotGoal::Actuel, Some(&textes.actuel), &style_actuel);
            changer(&mut bloc.suite, SlotGoal::Suite, Some(&textes.suite), &style_suite);
            changer(&mut bloc.statut, SlotGoal::Statut, textes.statut.as_deref(), &style_statut);
            changer(&mut bloc.celeb, SlotGoal::Celeb, textes.celeb.as_deref(), &style_celeb);
        }
        for (slot, style, texte) in maj {
            let ligne = self.ligne_goal(&style, &texte)?;
            if let Some(bloc) = self.blocs_goal.get_mut(&cle) {
                match slot {
                    SlotGoal::Entete => bloc.entete = Some(ligne),
                    SlotGoal::Badge => bloc.badge = Some(ligne),
                    SlotGoal::Actuel => bloc.actuel = Some(ligne),
                    SlotGoal::Suite => bloc.suite = Some(ligne),
                    SlotGoal::Statut => bloc.statut = Some(ligne),
                    SlotGoal::Celeb => bloc.celeb = Some(ligne),
                }
            }
        }
        Ok(())
    }

    /// Met en page un texte du widget (format, espacement, métriques) —
    /// même recette que `ligne_horloge`.
    fn ligne_goal(&self, style: &StyleTexte, texte: &str) -> Result<LigneGoal, ErreurTexte> {
        let format = self.format(style)?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        if contenu.is_empty() {
            return Err(ErreurTexte::Systeme("ligne goal vide".to_string()));
        }
        let layout = unsafe { self.dwrite.CreateTextLayout(&contenu, &format, 8192.0, 2048.0) }
            .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout goal : {}", e.message())))?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap goal : {}", e.message())))?;
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
                ErreurTexte::Systeme(format!("SetCharacterSpacing goal : {}", e.message()))
            })?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques goal : {}", e.message())))?;
        let base = base_ligne_goal(&layout).unwrap_or(metriques.height * 0.8);
        Ok(LigneGoal {
            texte: texte.to_string(),
            layout,
            _format: format,
            w: metriques.width,
            h: metriques.height,
            base,
        })
    }
}

/// Une ligne + son ombre — `text-shadow: 0 1px 2px #000` du CSS.
fn dessiner_ligne_goal(
    rt: &ID2D1RenderTarget,
    x: f32,
    y: f32,
    ligne: &LigneGoal,
    ombre: &ID2D1Brush,
    brosse: &ID2D1Brush,
) {
    unsafe {
        rt.DrawTextLayout(
            D2D_POINT_2F { x, y: y + 1.0 },
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

/// Ligne de base de la première ligne d'un layout (`None` si non mesurable).
fn base_ligne_goal(layout: &IDWriteTextLayout) -> Option<f32> {
    let mut metriques = [DWRITE_LINE_METRICS::default()];
    let mut lignes = 0u32;
    unsafe { layout.GetLineMetrics(Some(&mut metriques), &mut lignes) }.ok()?;
    Some(metriques[0].baseline)
}

/// Cache des blocs goal — déclaré ici pour que `mod.rs` ne connaisse pas le
/// détail (clé = [`cle_bloc`]).
pub(super) type BlocsGoal = HashMap<u64, BlocGoal>;

/// Cache des glyphes plateforme (Twitch/Kick), viewBox → scale au dessin.
pub(super) type LogosGoal = HashMap<LogoGoal, ID2D1PathGeometry>;

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: &str) -> GoalCfg {
        let mut c = GoalCfg {
            id: "g1".to_string(),
            rect: (0.0, 0.0, 280.0, 90.0),
            z: 5,
            plateforme: "twitch".to_string(),
            metrique: "followers".to_string(),
            cible: 500,
            goal_id: None,
            accent: ([0xe0, 0xe0, 0xe0, 255], [0x9a, 0x9a, 0x9a, 255]),
            cadre: None,
        };
        if !json.is_empty() {
            let w: serde_json::Value = serde_json::from_str(json).expect("json");
            if let Some(p) = w["plateforme"].as_str() {
                c.plateforme = p.to_string();
            }
            if let Some(m) = w["metrique"].as_str() {
                c.metrique = m.to_string();
            }
            if let Some(i) = w["goal_id"].as_str() {
                c.goal_id = Some(i.to_string());
            }
            if let Some(v) = w["cible"].as_u64() {
                c.cible = v;
            }
        }
        c
    }

    #[test]
    fn format_nombre_groupes_fr() {
        assert_eq!(format_nombre(0), "0");
        assert_eq!(format_nombre(999), "999");
        assert_eq!(format_nombre(1234), "1\u{202f}234");
        assert_eq!(format_nombre(1_234_567), "1\u{202f}234\u{202f}567");
    }

    #[test]
    fn valeur_metriques_et_jamais_mesure() {
        let mut etat = EtatGoals::nouveau();
        // Jamais mesuré → N/A (jamais de valeur fabriquée).
        assert_eq!(etat.valeur("twitch", "followers"), None);
        assert_eq!(etat.valeur("twitch", "viewers"), None);
        etat.appliquer(MajGoal::Communaute {
            plateforme: "twitch".into(),
            compteurs: Communaute {
                followers: Some(42),
                subs: Some(7),
                ..Communaute::default()
            },
        });
        etat.appliquer(MajGoal::Viewers { plateforme: "twitch".into(), count: Some(15) });
        assert_eq!(etat.valeur("twitch", "followers"), Some(42));
        assert_eq!(etat.valeur("twitch", "subs"), Some(7));
        // Champ absent de la frame = non mesuré → N/A.
        assert_eq!(etat.valeur("twitch", "sub_points"), None);
        assert_eq!(etat.valeur("twitch", "viewers"), Some(15));
        assert_eq!(etat.valeur("youtube", "followers"), None);
        // Hors-ligne (count:null) → N/A.
        etat.appliquer(MajGoal::Viewers { plateforme: "twitch".into(), count: None });
        assert_eq!(etat.valeur("twitch", "viewers"), None);
    }

    #[test]
    fn appliquer_dedup_et_version() {
        let mut etat = EtatGoals::nouveau();
        let maj = MajGoal::Viewers { plateforme: "kick".into(), count: Some(3) };
        etat.appliquer(maj.clone());
        let v = etat.version();
        etat.appliquer(maj);
        assert_eq!(etat.version(), v, "frame identique = pas de nouvelle version");
        etat.appliquer(MajGoal::Viewers { plateforme: "kick".into(), count: Some(4) });
        assert_eq!(etat.version(), v + 1);
    }

    #[test]
    fn resoudre_mode_manuel_et_lie() {
        let mut etat = EtatGoals::nouveau();
        let manuel = cfg("");
        let r = resoudre(&manuel, &etat);
        assert_eq!(r.actuel, None);
        assert_eq!(r.cible, 500);
        assert_eq!(r.label, "Followers");
        etat.appliquer(MajGoal::Communaute {
            plateforme: "twitch".into(),
            compteurs: Communaute { followers: Some(128), ..Communaute::default() },
        });
        assert_eq!(resoudre(&manuel, &etat).actuel, Some(128));

        // Mode lié : actuel/cible viennent du goal Twitch, jamais des compteurs.
        etat.appliquer(MajGoal::Twitch(GoalTwitch {
            id: "goal-1".into(),
            goal_type: "follower".into(),
            description: String::new(),
            actuel: 12,
            cible: 50,
            termine: false,
        }));
        let lie = cfg(r#"{"goal_id":"goal-1"}"#);
        let r = resoudre(&lie, &etat);
        assert_eq!((r.actuel, r.cible), (Some(12), 50));
        assert_eq!(r.label, "Followers", "repli sur le type si description vide");
        // Goal inconnu → « Objectif Twitch » en attente.
        let attente = resoudre(&cfg(r#"{"goal_id":"inconnu"}"#), &etat);
        assert_eq!(attente.actuel, None);
        assert_eq!(attente.label, "Objectif Twitch");
    }

    #[test]
    fn resoudre_description_et_termine() {
        let mut etat = EtatGoals::nouveau();
        etat.appliquer(MajGoal::Twitch(GoalTwitch {
            id: "g".into(),
            goal_type: "new_bit".into(),
            description: "Objectif du mois".into(),
            actuel: 100,
            cible: 100,
            termine: true,
        }));
        let r = resoudre(&cfg(r#"{"goal_id":"g"}"#), &etat);
        assert_eq!(r.label, "Objectif du mois");
        assert!(r.termine);
    }

    #[test]
    fn celebration_fenetre_6s_ciblee_par_widget() {
        let mut etat = EtatGoals::nouveau();
        etat.appliquer(MajGoal::Atteint { widget_id: "g1".into() });
        assert!(etat.en_celebration("g1"));
        assert!(!etat.en_celebration("g2"));
        // Expirée : simulée en reculant l'instant directement.
        etat.celebrations.insert("g1".into(), Instant::now() - std::time::Duration::from_secs(7));
        assert!(!etat.en_celebration("g1"));
        // Une nouvelle frame réarme sa fenêtre et purge les expirées.
        etat.appliquer(MajGoal::Atteint { widget_id: "g2".into() });
        assert!(etat.en_celebration("g2"));
        assert_eq!(etat.celebrations.len(), 1);
    }

    #[test]
    fn metriques_inconnues_replient_sur_objectif() {
        let etat = EtatGoals::nouveau();
        let r = resoudre(&cfg(r#"{"metrique":"bits"}"#), &etat);
        assert_eq!(r.label, "Objectif");
        assert_eq!(r.actuel, None);
    }
}
