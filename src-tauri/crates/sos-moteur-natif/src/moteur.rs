//! Orchestrateur du moteur natif.
//!
//! Assemble les briques : WGC (N sources) → canvas **BGRA composé
//! par Direct2D** (alpha réel, médias, cadres, voiles, chat, input viewer,
//! morphs, plans à perspective) → conversion NV12 → encodeur (NVENC
//! d'abord, repli MFT) → sinks → aperçu, cadencé par l'horloge, surveillé
//! par le gouverneur et les compteurs.
//!
//! Contrat : `docs/features/moteur-natif.md` §3.1. `MoteurNatif` implémente
//! `sos_moteur_api::Moteur` ; la scène arrive par `configurer_scene`
//! (snapshot de pré-session) et se ré-applique **à chaud** par
//! `appliquer_scene` (trait — différentiel, file bornée drop-oldest).
//!
//! Limites assumées :
//! - sorties fichier (MP4 fragmenté / MKV) et live RTMP/SRT via le sidecar
//!   FFmpeg (`-f flv` / `-f mpegts`) — sans reconnexion ni backoff à ce lot ;
//! - caméra Media Foundation (`capture/camera.rs`) : ni fond vert ni
//!   anneau `cameraBord` à ce lot ;
//! - le gouverneur **constate** et trace sa décision, il ne reconfigure pas
//!   encore le rendu/l'encodeur à chaud.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, ID2D1PathGeometry};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

use sos_moteur_api::{
    ApercuFrame, Capacites, Codec, ErreurMoteur, Moteur, MoteurEtat, PaquetEncode, Sink, SinkCfg,
    SortieCfg, SortieKind,
};

use crate::cadres::CadreCfg;
use crate::capture::camera::CaptureCamera;
use crate::capture::enumeration::enumerer_moniteurs;
use crate::capture::Moniteur;
use crate::capture::wgc::CaptureWgc;
use crate::chat::{ChatCfg, FileChat, MessageChat};
use crate::clock::Horloge;
use crate::color_gpu::ConvertisseurNv12;
use crate::compteurs::{CompteursMoteur, Instantane};
use crate::encode::detect;
use crate::encode::mft::EncodeurMft;
use crate::encode::nvenc::EncodeurNvenc;
use crate::encode::{detect::EncodeurDispo, ErreurEncodage, Preset};
use crate::gouverneur::{Action, Gouverneur, Mesures};
use crate::gpu::{ContexteGpu, DescTexture, Format};
use crate::input::{EtatInput, FileInput, InputCfg};
use crate::media::{CarrouselCfg, MediaCfg, MediaChargee, PerspCfg};
use crate::output::ffmpeg::{FfmpegSink, FormatReseau};
use crate::preview::Apercu;
use crate::scene_sources::BoiteCfg;
use crate::texte::{
    DessinateurTexte, EtatAlertes, EtatGoals, EtatMusique, EtatSpeedrun, GoalCfg, HorlogeCfg,
    MajAlerte, MajGoal, MajMusique, MajSpeedrun, MusiqueCfg, SpeedrunCfg, TexteCfg, Z_ALERTES,
};

/// Ce que le moteur capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CibleCapture {
    Moniteur { index: u32 },
    Fenetre { hwnd: isize },
    /// Caméra locale (Media Foundation) — `device` = nom convivial
    /// (`cameraDeviceId` persisté, FriendlyName MF) ; `None` = device par
    /// défaut du système.
    Camera { device: Option<String> },
}

/// Forme de découpe dédiée d'une source (widget caméra `cameraForme`,
/// contrat 9). `None` = rectangle de la boîte (clip axe-aligné par défaut) :
/// l'absence de forme dédiée suit le cadre de scène en mode OBS — hors
/// périmètre de ce lot, elle est rendue comme `Carre`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormeCamera {
    /// Rectangle : le clip axe-aligné de la boîte (`"carre"` explicite).
    Carre,
    /// Cercle inscrit : diamètre `min(w,h)`, centré — même polygone que
    /// `pathCercle` (`cadres::points_cercle`).
    Cercle,
}

/// Une source du moteur : cible native + destination dans le canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceCfg {
    pub nom: String,
    pub cible: CibleCapture,
    /// Destination `(x, y, largeur, hauteur)` en pixels canvas.
    pub rect: (f64, f64, f64, f64),
    /// Ordre de composition (croissant = dessous).
    pub z: i32,
    /// Cadre du widget porteur : `Some` = la capture est **découpée au
    /// polygone** du cadre (même contrat que les médias).
    pub cadre: Option<CadreCfg>,
    /// Forme dédiée d'un widget caméra (`None` pour toute autre source).
    pub forme_camera: Option<FormeCamera>,
}

impl SourceCfg {
    /// Refuse une source inexploitable **avant** de toucher au GPU.
    pub fn valider(&self) -> Result<(), ErreurMoteur> {
        if self.nom.trim().is_empty() {
            return Err(ErreurMoteur::Configuration(
                "source sans nom".to_string(),
            ));
        }
        let (x, y, w, h) = self.rect;
        if ![x, y, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
            return Err(ErreurMoteur::Configuration(format!(
                "source « {} » : rect invalide {:?}",
                self.nom, self.rect
            )));
        }
        Ok(())
    }
}

/// Identité de l'encodeur réellement retenu (diagnostic, lot 2.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeurInfo {
    pub nom: String,
    pub materiel: bool,
    pub zero_copie: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EtatInterne {
    Inactif,
    Actif,
    Erreur(String),
}

/// Moteur natif : un enregistrement à la fois, sources configurables à chaud
/// hors enregistrement.
pub struct MoteurNatif {
    sources: Mutex<Vec<SourceCfg>>,
    medias: Mutex<Vec<MediaCfg>>,
    /// Plans à perspective (widgets `rotateX/Y`, fond `bgRotX/Y`).
    persps: Mutex<Vec<PerspCfg>>,
    textes: Mutex<Vec<TexteCfg>>,
    cadres: Mutex<Vec<CadreCfg>>,
    chats: Mutex<Vec<ChatCfg>>,
    /// Widgets input-viewer (mode, disposition, skin, couleur).
    inputs: Mutex<Vec<InputCfg>>,
    /// Modules horloge de la scène (`scene.modules.horloge` actif).
    horloges: Mutex<Vec<HorlogeCfg>>,
    /// Widgets `type: "goal"` de la scène (compteur + cible + barre).
    goals: Mutex<Vec<GoalCfg>>,
    /// État vivant des compteurs goal : l'app le pousse pendant
    /// l'enregistrement (frames `communaute`/`viewers`/`goal-twitch`/
    /// `goal-atteint`), comme le chat.
    goals_etat: Arc<Mutex<EtatGoals>>,
    /// Widgets `type: "speedrun"` de la scène (splitter ASL).
    speedruns: Mutex<Vec<SpeedrunCfg>>,
    /// État vivant du splitter : l'app le pousse pendant l'enregistrement
    /// (frames `speedrun-etat` du canal `chat_tx` — tick ~15 Hz en run).
    /// Un seul état partagé : le DOM applique chaque event à tous les
    /// widgets speedrun (équivalent des `pendingLss`/`pendingEtat`).
    speedruns_etat: Arc<Mutex<EtatSpeedrun>>,
    /// Widgets `type: "musique"` de la scène (lecteur Winamp).
    musiques: Mutex<Vec<MusiqueCfg>>,
    /// État vivant du lecteur : l'app le pousse pendant l'enregistrement
    /// (events Tauri `musique:update` — snapshots `MusiqueState::etat`,
    /// position figée à l'emit). Un seul état partagé : le DOM applique
    /// l'état courant à tous les widgets musique, comme le speedrun.
    musiques_etat: Arc<Mutex<EtatMusique>>,
    /// État vivant de l'overlay alerte : l'app le pousse pendant
    /// l'enregistrement (frames `alerte-play` / `alerte-stop` /
    /// `alerte-config` / `position-overlay-config` du canal `chat_tx`) —
    /// transitoire, jamais porté par la scène.
    alertes_etat: Arc<Mutex<EtatAlertes>>,
    /// Gabarit du cadre de scène pour la carte alerte
    /// (`appliquerCadreOverlay` : le style du `cadreWidget` actif est
    /// appliqué à la taille de la carte — rect nul, mesuré au rendu).
    cadre_overlay_alerte: Mutex<Option<CadreCfg>>,
    /// État vivant des touches : l'app le pousse pendant l'enregistrement.
    input: Arc<Mutex<FileInput>>,
    /// Boîtes à voile (`.widget`) : fond + bordure, hors `sansCadre`,
    /// widgets de capture et caméra.
    boites: Mutex<Vec<BoiteCfg>>,
    /// Buffer de chat **vivant** : l'app y pousse pendant l'enregistrement,
    /// le rendu le relit à chaque frame sans reconfigurer la scène.
    chat: Arc<Mutex<FileChat>>,
    etat: Arc<Mutex<EtatInterne>>,
    compteurs: Arc<CompteursMoteur>,
    arret: Mutex<Option<Arc<AtomicBool>>>,
    fil: Mutex<Option<JoinHandle<()>>>,
    apercu: Arc<Mutex<Option<ApercuFrame>>>,
    encodeur: Arc<Mutex<Option<EncodeurInfo>>>,
    /// Session « aperçu seul » du projecteur natif : `boucle` sans encodeur
    /// ni sink qui n'alimente que `apercu`. **N'est pas une sortie** :
    /// `en_cours()`/`etat()`/`natif_actif()` l'ignorent — elle ne bloque ni
    /// la bascule de mode ni l'état « enregistrement ».
    arret_apercu: Mutex<Option<Arc<AtomicBool>>>,
    fil_apercu: Mutex<Option<JoinHandle<()>>>,
    /// Erreur terminale de la session aperçu : persiste tant que
    /// `arreter_apercu` n'a pas acquitté — jamais de respawn en boucle sur
    /// la même panne (le poll de la fenêtre relancerait à chaque appel).
    etat_apercu: Arc<Mutex<Option<String>>>,
    /// Dossier des données (`medias/` y est relatif) — posé par
    /// `configurer_scene` / `configurer_base`, relu par `appliquer_scene`.
    /// Vide tant qu'aucune scène n'a été configurée.
    base: Mutex<PathBuf>,
    /// Canvas de la dernière scène chargée — comparé au canvas de session
    /// à l'application à chaud (mismatch = dégradation signalée).
    canvas: Mutex<(u32, u32)>,
    /// Courrier de la scène à appliquer à chaud : file bornée à **un**
    /// élément — une nouvelle application écrase la précédente non
    /// consommée (drop-oldest, contrat §3.1). Consommée par `boucle` en
    /// début de frame, jamais bloquante côté appelant.
    scene_maj: Arc<Mutex<Option<SceneChargee>>>,
}

impl Default for MoteurNatif {
    fn default() -> Self {
        Self::nouveau()
    }
}

impl MoteurNatif {
    pub fn nouveau() -> Self {
        Self {
            sources: Mutex::new(Vec::new()),
            medias: Mutex::new(Vec::new()),
            persps: Mutex::new(Vec::new()),
            textes: Mutex::new(Vec::new()),
            cadres: Mutex::new(Vec::new()),
            chats: Mutex::new(Vec::new()),
            inputs: Mutex::new(Vec::new()),
            horloges: Mutex::new(Vec::new()),
            goals: Mutex::new(Vec::new()),
            goals_etat: Arc::new(Mutex::new(EtatGoals::nouveau())),
            speedruns: Mutex::new(Vec::new()),
            speedruns_etat: Arc::new(Mutex::new(EtatSpeedrun::nouveau())),
            musiques: Mutex::new(Vec::new()),
            musiques_etat: Arc::new(Mutex::new(EtatMusique::nouveau())),
            alertes_etat: Arc::new(Mutex::new(EtatAlertes::nouveau())),
            cadre_overlay_alerte: Mutex::new(None),
            input: Arc::new(Mutex::new(FileInput::nouveau())),
            boites: Mutex::new(Vec::new()),
            chat: Arc::new(Mutex::new(FileChat::nouveau())),
            etat: Arc::new(Mutex::new(EtatInterne::Inactif)),
            compteurs: Arc::new(CompteursMoteur::nouveau()),
            arret: Mutex::new(None),
            fil: Mutex::new(None),
            apercu: Arc::new(Mutex::new(None)),
            encodeur: Arc::new(Mutex::new(None)),
            arret_apercu: Mutex::new(None),
            fil_apercu: Mutex::new(None),
            etat_apercu: Arc::new(Mutex::new(None)),
            base: Mutex::new(PathBuf::new()),
            canvas: Mutex::new((0, 0)),
            scene_maj: Arc::new(Mutex::new(None)),
        }
    }

    /// Remplace les sources. Refusé pendant un enregistrement (idempotence
    /// des sessions de capture : on ne démonte pas une source à chaud).
    pub fn configurer_sources(&self, sources: Vec<SourceCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        for source in &sources {
            source.valider()?;
        }
        *self.sources.lock().unwrap_or_else(|e| e.into_inner()) = sources;
        Ok(())
    }

    pub fn sources(&self) -> Vec<SourceCfg> {
        self.sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remplace les médias statiques (fond + widgets média).
    pub fn configurer_medias(&self, medias: Vec<MediaCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.medias.lock().unwrap_or_else(|e| e.into_inner()) = medias;
        Ok(())
    }

    /// Médias statiques configurés (diagnostic / tests).
    pub fn medias(&self) -> Vec<MediaCfg> {
        self.medias
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remplace les textes (titres de fond et de widgets).
    pub fn configurer_textes(&self, textes: Vec<TexteCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.textes.lock().unwrap_or_else(|e| e.into_inner()) = textes;
        Ok(())
    }

    /// Textes configurés (diagnostic / tests).
    pub fn textes(&self) -> Vec<TexteCfg> {
        self.textes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remplace les cadres (bord du canvas + widgets).
    pub fn configurer_cadres(&self, cadres: Vec<CadreCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.cadres.lock().unwrap_or_else(|e| e.into_inner()) = cadres;
        Ok(())
    }

    /// Cadres configurés (diagnostic / tests).
    pub fn cadres(&self) -> Vec<CadreCfg> {
        self.cadres
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remplace les widgets chat (rect, filtre, taille, style).
    pub fn configurer_chats(&self, chats: Vec<ChatCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.chats.lock().unwrap_or_else(|e| e.into_inner()) = chats;
        Ok(())
    }

    /// Widgets chat configurés (diagnostic / tests).
    pub fn chats(&self) -> Vec<ChatCfg> {
        self.chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remplace les boîtes à voile (`.widget` : fond glass + bordure 1 px).
    pub fn configurer_boites(&self, boites: Vec<BoiteCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.boites.lock().unwrap_or_else(|e| e.into_inner()) = boites;
        Ok(())
    }

    /// Boîtes configurées (diagnostic / tests).
    pub fn boites(&self) -> Vec<BoiteCfg> {
        self.boites
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Plans à perspective configurés (diagnostic / tests).
    pub fn persps(&self) -> Vec<PerspCfg> {
        self.persps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remplace les widgets input-viewer.
    pub fn configurer_inputs(&self, inputs: Vec<InputCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.inputs.lock().unwrap_or_else(|e| e.into_inner()) = inputs;
        Ok(())
    }

    /// Widgets input-viewer configurés (diagnostic / tests).
    pub fn inputs(&self) -> Vec<InputCfg> {
        self.inputs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remplace les modules horloge (rect, options, cadre opt-in).
    pub fn configurer_horloges(&self, horloges: Vec<HorlogeCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.horloges.lock().unwrap_or_else(|e| e.into_inner()) = horloges;
        Ok(())
    }

    /// Modules horloge configurés (diagnostic / tests).
    pub fn horloges(&self) -> Vec<HorlogeCfg> {
        self.horloges
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Remplace les widgets goal (rect, plateforme/métrique/cible ou goalId).
    pub fn configurer_goals(&self, goals: Vec<GoalCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.goals.lock().unwrap_or_else(|e| e.into_inner()) = goals;
        Ok(())
    }

    /// Widgets goal configurés (diagnostic / tests).
    pub fn goals(&self) -> Vec<GoalCfg> {
        self.goals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Pousse une mutation goal (frames `communaute`, `viewers`,
    /// `goal-twitch`, `goal-atteint` du canal `chat_tx`).
    ///
    /// **Jamais refusé pendant un enregistrement**, comme le chat : c'est le
    /// canal par lequel les compteurs arrivent après le démarrage.
    pub fn pousser_goal(&self, maj: MajGoal) {
        self.goals_etat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .appliquer(maj);
    }

    /// Version de l'état goal (diagnostic / tests).
    pub fn goals_version(&self) -> u64 {
        self.goals_etat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .version()
    }

    /// Remplace les widgets speedrun (rect, options `sr*`, cadre).
    pub fn configurer_speedruns(&self, speedruns: Vec<SpeedrunCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.speedruns.lock().unwrap_or_else(|e| e.into_inner()) = speedruns;
        Ok(())
    }

    /// Widgets speedrun configurés (diagnostic / tests).
    pub fn speedruns(&self) -> Vec<SpeedrunCfg> {
        self.speedruns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Pousse un event speedrun (frame `speedrun-etat` du canal `chat_tx` —
    /// tick du timer, split, LSS…).
    ///
    /// **Jamais refusé pendant un enregistrement**, comme `pousser_goal` :
    /// c'est le canal par lequel l'état du splitter arrive après le
    /// démarrage.
    pub fn pousser_speedrun(&self, maj: MajSpeedrun) {
        self.speedruns_etat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .appliquer(maj);
    }

    /// Version de l'état speedrun (diagnostic / tests).
    pub fn speedruns_version(&self) -> u64 {
        self.speedruns_etat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .version()
    }

    /// Remplace les widgets musique (rect, `musiqueMode`, cadre).
    pub fn configurer_musiques(&self, musiques: Vec<MusiqueCfg>) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        *self.musiques.lock().unwrap_or_else(|e| e.into_inner()) = musiques;
        Ok(())
    }

    /// Widgets musique configurés (diagnostic / tests).
    pub fn musiques(&self) -> Vec<MusiqueCfg> {
        self.musiques
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Pousse un snapshot musique (event Tauri `musique:update` — état
    /// complet de `MusiqueState::etat` : piste en cours + progression
    /// figée à l'emit).
    ///
    /// **Jamais refusé pendant un enregistrement**, comme `pousser_goal` :
    /// c'est le canal par lequel l'état du lecteur arrive après le
    /// démarrage.
    pub fn pousser_musique(&self, maj: MajMusique) {
        self.musiques_etat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .appliquer(maj);
    }

    /// Version de l'état musique (diagnostic / tests).
    pub fn musiques_version(&self) -> u64 {
        self.musiques_etat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .version()
    }

    /// Pousse une mutation de l'overlay alerte (frames `alerte-play`,
    /// `alerte-stop`, `alerte-config` et `position-overlay-config` du
    /// canal `chat_tx` — le squelette de position est partagé avec le
    /// clip de bienvenue, comme `applyAlerteOverlayConfig` du DOM).
    ///
    /// **Jamais refusé pendant un enregistrement**, comme `pousser_goal` :
    /// c'est le canal par lequel les alertes arrivent après le démarrage.
    /// `base` (dossier des données — `medias/` y est relatif) est relue à
    /// chaque push : posée par `configurer_scene` / `configurer_base`.
    pub fn pousser_alerte(&self, maj: MajAlerte) {
        let base = self.base.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.alertes_etat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .appliquer(maj, &base);
    }

    /// Version de l'état alerte (diagnostic / tests).
    pub fn alertes_version(&self) -> u64 {
        self.alertes_etat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .version()
    }

    /// Pousse l'état du périphérique (frame `input-viewer-etat`, 30 Hz).
    ///
    /// **Jamais refusé pendant un enregistrement**, comme le chat : c'est le
    /// canal par lequel l'état arrive après le démarrage.
    pub fn pousser_input(&self, etat: EtatInput) {
        self.input
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remplacer(etat);
    }

    /// Nombre de touches pressées (diagnostic / tests).
    pub fn input_len(&self) -> usize {
        self.input.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Pousse un message dans le buffer de chat.
    ///
    /// **Jamais refusé pendant un enregistrement** : c'est le canal par lequel
    /// les messages arrivent après le démarrage (aucune reconfiguration de
    /// scène, donc pas de `DejaActif`). File bornée, non bloquante.
    pub fn pousser_chat(&self, message: MessageChat) {
        self.chat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pousser(message);
    }

    /// Nombre de messages en buffer (diagnostic / tests).
    pub fn chat_len(&self) -> usize {
        self.chat
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Remplace sources et médias depuis une scène + la table widget → cible.
    /// `base` = dossier des données (`medias/` y est relatif).
    ///
    /// Refusé pendant un enregistrement (snapshot de pré-session) :
    /// l'application à chaud passe par `Moteur::appliquer_scene`.
    pub fn configurer_scene(
        &self,
        scene: &sos_scene::Scene,
        cibles: &std::collections::HashMap<String, CibleCapture>,
        base: &std::path::Path,
    ) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        self.configurer_base(base);
        let lot = self.lot_scene(scene, cibles, base);
        for source in &lot.sources {
            source.valider()?;
        }
        self.ecrire_lot(&lot);
        *self.canvas.lock().unwrap_or_else(|e| e.into_inner()) =
            (scene.canvasW, scene.canvasH);
        Ok(())
    }

    /// Dossier des données (`medias/` y est relatif) — relu par
    /// `appliquer_scene`. Posé par `configurer_scene` ; peut être fixé
    /// directement pour une application sans session préalable.
    pub fn configurer_base(&self, base: &std::path::Path) {
        *self.base.lock().unwrap_or_else(|e| e.into_inner()) = base.to_path_buf();
    }

    /// Mapping scène → lot de configurations (partagé par `configurer_scene`
    /// et `Moteur::appliquer_scene` — une seule source de vérité).
    fn lot_scene(
        &self,
        scene: &sos_scene::Scene,
        cibles: &std::collections::HashMap<String, CibleCapture>,
        base: &std::path::Path,
    ) -> LotScene {
        // Un style non porté ne produit aucun cadre : le dire, plutôt que de
        // laisser croire que la scène est rendue à l'identique.
        if let Some(cadre) = scene
            .cadreWidget
            .as_ref()
            .filter(|c| c.actif && c.strokeWidth > 0.0)
        {
            if crate::cadres::StyleCadre::depuis_nom(&cadre.style).is_none() {
                self.compteurs.degrader(format!(
                    "cadre « {} » non porté : aucun tracé",
                    cadre.style
                ));
            }
        }
        LotScene {
            sources: crate::scene_sources::sources_depuis_scene(scene, cibles),
            medias: crate::scene_sources::medias_depuis_scene(scene, base),
            textes: crate::scene_sources::textes_depuis_scene(scene),
            cadres: crate::scene_sources::cadres_depuis_scene(scene),
            chats: crate::scene_sources::chat_depuis_scene(scene),
            boites: crate::scene_sources::boites_depuis_scene(scene),
            inputs: crate::scene_sources::input_depuis_scene(scene),
            horloges: crate::scene_sources::horloges_depuis_scene(scene),
            goals: crate::scene_sources::goals_depuis_scene(scene),
            speedruns: crate::scene_sources::speedruns_depuis_scene(scene),
            musiques: crate::scene_sources::musiques_depuis_scene(scene),
            persps: crate::scene_sources::persps_depuis_scene(scene),
            cadre_overlay_alerte: cadre_overlay_alerte(scene),
        }
    }

    /// Écrit le lot dans les mutexes — la configuration lue par le prochain
    /// `demarrer_*` et photographiée par `scene_chargee`.
    fn ecrire_lot(&self, lot: &LotScene) {
        *self.sources.lock().unwrap_or_else(|e| e.into_inner()) = lot.sources.clone();
        *self.medias.lock().unwrap_or_else(|e| e.into_inner()) = lot.medias.clone();
        *self.textes.lock().unwrap_or_else(|e| e.into_inner()) = lot.textes.clone();
        *self.cadres.lock().unwrap_or_else(|e| e.into_inner()) = lot.cadres.clone();
        *self.chats.lock().unwrap_or_else(|e| e.into_inner()) = lot.chats.clone();
        *self.boites.lock().unwrap_or_else(|e| e.into_inner()) = lot.boites.clone();
        *self.inputs.lock().unwrap_or_else(|e| e.into_inner()) = lot.inputs.clone();
        *self.horloges.lock().unwrap_or_else(|e| e.into_inner()) = lot.horloges.clone();
        *self.goals.lock().unwrap_or_else(|e| e.into_inner()) = lot.goals.clone();
        *self.speedruns.lock().unwrap_or_else(|e| e.into_inner()) = lot.speedruns.clone();
        *self.musiques.lock().unwrap_or_else(|e| e.into_inner()) = lot.musiques.clone();
        *self.persps.lock().unwrap_or_else(|e| e.into_inner()) = lot.persps.clone();
        *self
            .cadre_overlay_alerte
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = lot.cadre_overlay_alerte.clone();
    }

    /// Photographie des compteurs (diagnostic, lot 2.9).
    pub fn instantane(&self) -> Instantane {
        self.compteurs.instantane()
    }

    /// Identité de l'encodeur retenu (`None` tant que rien n'a démarré).
    pub fn encodeur(&self) -> Option<EncodeurInfo> {
        self.encodeur
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Un enregistrement tourne-t-il (fil vivant) ?
    pub fn en_cours(&self) -> bool {
        self.fil
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|f| !f.is_finished())
    }

    /// Photographie la scène chargée : sources, médias, calques + buffers
    /// vivants (chat, input) que l'app alimente pendant la session.
    fn scene_chargee(&self) -> SceneChargee {
        SceneChargee {
            sources: self.sources(),
            medias: self
                .medias
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            textes: self.textes(),
            cadres: self.cadres(),
            chats: self.chats(),
            boites: self.boites(),
            inputs: self.inputs(),
            horloges: self.horloges(),
            goals: self.goals(),
            speedruns: self.speedruns(),
            musiques: self.musiques(),
            persps: self
                .persps
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            canvas: *self.canvas.lock().unwrap_or_else(|e| e.into_inner()),
            chat: Arc::clone(&self.chat),
            input: Arc::clone(&self.input),
            goals_etat: Arc::clone(&self.goals_etat),
            speedruns_etat: Arc::clone(&self.speedruns_etat),
            musiques_etat: Arc::clone(&self.musiques_etat),
            alertes_etat: Arc::clone(&self.alertes_etat),
            cadre_overlay_alerte: self
                .cadre_overlay_alerte
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        }
    }

    /// Démarre la session « aperçu seul » (projecteur natif).
    ///
    /// - **Idempotente** : un fil vivant est réutilisé ; pendant un
    ///   enregistrement, retour immédiat — `boucle` alimente déjà `apercu`.
    /// - Une erreur terminale (`etat_apercu`) bloque la relance tant que
    ///   `arreter_apercu` n'a pas acquitté.
    /// - `fps` borne la composition (le projecteur tourne très léger :
    ///   30 fps de composition, aperçu réduit émis à 10 fps quoi qu'il arrive).
    pub fn demarrer_apercu(&self, canvas: (u32, u32), fps: u32) -> Result<(), ErreurMoteur> {
        {
            let mut fil = self.fil_apercu.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(f) = fil.as_ref() {
                if !f.is_finished() {
                    return Ok(());
                }
            }
            if let Some(f) = fil.take() {
                let _ = f.join();
            }
        }
        if self.en_cours() {
            return Ok(());
        }
        if let Some(erreur) = self
            .etat_apercu
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(ErreurMoteur::Interne(erreur));
        }
        if canvas.0 == 0 || canvas.1 == 0 || fps == 0 {
            return Err(ErreurMoteur::Configuration(format!(
                "aperçu invalide : canvas {}×{}, fps {fps}",
                canvas.0, canvas.1
            )));
        }
        let scene = self.scene_chargee();
        for source in &scene.sources {
            source.valider()?;
        }
        // Le courrier n'a de sens que pour la boucle à venir : toute
        // application déposée avant le snapshot y est déjà incluse.
        self.scene_maj
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let maj_scene = Arc::clone(&self.scene_maj);
        let arret = Arc::new(AtomicBool::new(false));
        *self.arret_apercu.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Arc::clone(&arret));
        let compteurs = Arc::clone(&self.compteurs);
        let apercu = Arc::clone(&self.apercu);
        let encodeur_partage = Arc::clone(&self.encodeur);
        let etat_apercu = Arc::clone(&self.etat_apercu);
        let cfg = SortieCfg {
            // `Apercu` seul : `boucle` ne crée alors ni encodeur ni sink.
            sinks: vec![SinkCfg::Apercu],
            canvas,
            fps,
            codec: Codec::H264,
            debit_kbps: 6000,
            preset: "equilibre".to_string(),
            cle: None,
            audio: None,
        };
        let fil = std::thread::Builder::new()
            .name("sos-moteur-apercu".to_string())
            .spawn(move || {
                let resultat = boucle(
                    cfg,
                    scene,
                    maj_scene,
                    Arc::clone(&compteurs),
                    apercu,
                    arret,
                    encodeur_partage,
                );
                if let Err(m) = resultat {
                    *etat_apercu.lock().unwrap_or_else(|p| p.into_inner()) = Some(m);
                }
            })
            .map_err(|e| ErreurMoteur::Interne(format!("thread aperçu : {e}")))?;
        *self.fil_apercu.lock().unwrap_or_else(|e| e.into_inner()) = Some(fil);
        Ok(())
    }

    /// Arrête la session aperçu (idempotent) et acquitte son erreur
    /// terminale — seule voie de relance après une panne.
    pub fn arreter_apercu(&self) {
        if let Some(drapeau) = self
            .arret_apercu
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            drapeau.store(true, Ordering::Relaxed);
        }
        if let Some(fil) = self.fil_apercu.lock().unwrap_or_else(|e| e.into_inner()).take()
        {
            let _ = fil.join();
        }
        *self.etat_apercu.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Session aperçu vivante ?
    pub fn apercu_en_cours(&self) -> bool {
        self.fil_apercu
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|f| !f.is_finished())
    }

    /// Erreur terminale de la session aperçu (`None` = saine ou jamais lancée).
    pub fn apercu_erreur(&self) -> Option<String> {
        self.etat_apercu
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Moteur for MoteurNatif {
    fn capacites(&self) -> Capacites {
        Capacites {
            capture_ecran: true,
            capture_fenetre: true,
            capture_jeu: false,
            camera: true,
            fond_vert: false,
            masque_shader: false,
            cadres_animes: false,
            // Loopback + micro muxés en AAC dans le fichier (P3 audio).
            audio: true,
            audio_par_app: false,
            roi_encode: false,
            multi_format: false,
            hdr: false,
            // SRT via le sidecar FFmpeg (libsrt dans le binaire embarqué,
            // vérifié : protocole `srt` + muxeur `mpegts` présents).
            srt: true,
            ndi: false,
        }
    }

    /// Application à chaud (contrat §3.1) : la scène est re-mappée, écrite
    /// dans les mutexes puis déposée dans le courrier de la boucle — file
    /// bornée à un élément, drop-oldest, jamais bloquante. La boucle
    /// applique la différence au début de la frame suivante : une capture
    /// ou un média inchangé garde sa session, sa texture et son encodeur.
    ///
    /// La table `cibles` du harnais n'existe pas côté trait : seule la
    /// cible persistée (`captureCible`) est lue — comme l'app, qui passe
    /// toujours une table vide. Une source invalide annule l'application
    /// (dégradation signalée), jamais la session en cours. Hors session,
    /// seuls les mutexes sont mis à jour : le prochain démarrage les
    /// consomme par `scene_chargee`.
    fn appliquer_scene(&self, scene: &sos_scene::Scene) {
        let base = self.base.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let lot = self.lot_scene(scene, &std::collections::HashMap::new(), &base);
        for source in &lot.sources {
            if let Err(e) = source.valider() {
                self.compteurs
                    .degrader(format!("scène refusée à chaud : {e}"));
                return;
            }
        }
        self.ecrire_lot(&lot);
        *self.canvas.lock().unwrap_or_else(|e| e.into_inner()) =
            (scene.canvasW, scene.canvasH);
        *self.scene_maj.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(self.scene_chargee());
    }

    fn demarrer_sortie(&self, cfg: SortieCfg) -> Result<(), ErreurMoteur> {
        cfg.valider()?;

        // Un seul enregistrement : un fil terminé est nettoyé, un fil vivant
        // refuse le démarrage (jamais deux encodeurs sur le même GPU).
        {
            let mut fil = self.fil.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(f) = fil.as_ref() {
                if !f.is_finished() {
                    return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
                }
            }
            if let Some(f) = fil.take() {
                let _ = f.join();
            }
        }

        for sink in &cfg.sinks {
            match sink {
                SinkCfg::Fichier { .. } | SinkCfg::Apercu => {}
                // Validé ici, avant tout contact GPU : une URL au schéma
                // inattendu est une erreur de configuration, pas une panne
                // réseau (ffmpeg la refuserait au premier paquet écrit).
                SinkCfg::Rtmp { url } => {
                    if !(url.starts_with("rtmp://") || url.starts_with("rtmps://")) {
                        return Err(ErreurMoteur::Configuration(
                            "URL RTMP attendue (rtmp://… ou rtmps://…)".to_string(),
                        ));
                    }
                }
                SinkCfg::Srt { url } => {
                    if !url.starts_with("srt://") {
                        return Err(ErreurMoteur::Configuration(
                            "URL SRT attendue (srt://hôte:port)".to_string(),
                        ));
                    }
                }
            }
        }
        if !cfg
            .sinks
            .iter()
            .any(|s| !matches!(s, SinkCfg::Apercu))
        {
            return Err(ErreurMoteur::Configuration(
                "aucune sortie demandée".to_string(),
            ));
        }

        // Une session « aperçu seul » détient les mêmes captures (accès
        // exclusif caméra, doublon WGC inutile) : elle s'arrête avant tout
        // démarrage de sortie — l'enregistrement alimente ensuite l'aperçu.
        self.arreter_apercu();

        let scene = self.scene_chargee();
        if scene.sources.is_empty()
            && scene.medias.is_empty()
            && scene.textes.is_empty()
            && scene.chats.is_empty()
            && scene.inputs.is_empty()
            && scene.horloges.is_empty()
            && scene.goals.is_empty()
            && scene.speedruns.is_empty()
            && scene.musiques.is_empty()
        {
            return Err(ErreurMoteur::Configuration(
                "aucune source, média, texte, chat, input-viewer, goal, horloge, speedrun ni musique configuré"
                    .to_string(),
            ));
        }
        for source in &scene.sources {
            source.valider()?;
        }

        // Le courrier n'a de sens que pour la boucle à venir : toute
        // application déposée avant le snapshot y est déjà incluse.
        self.scene_maj
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let maj_scene = Arc::clone(&self.scene_maj);
        let arret = Arc::new(AtomicBool::new(false));
        *self.etat.lock().unwrap_or_else(|e| e.into_inner()) = EtatInterne::Actif;
        *self.arret.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&arret));

        let etat = Arc::clone(&self.etat);
        let compteurs = Arc::clone(&self.compteurs);
        let apercu = Arc::clone(&self.apercu);
        let encodeur_partage = Arc::clone(&self.encodeur);
        let fil = std::thread::Builder::new()
            .name("sos-moteur-natif".to_string())
            .spawn(move || {
                let resultat = boucle(
                    cfg,
                    scene,
                    maj_scene,
                    Arc::clone(&compteurs),
                    apercu,
                    arret,
                    encodeur_partage,
                );
                let mut e = etat.lock().unwrap_or_else(|p| p.into_inner());
                *e = match resultat {
                    Ok(()) => EtatInterne::Inactif,
                    Err(m) => EtatInterne::Erreur(m),
                };
            })
            .map_err(|e| ErreurMoteur::Interne(format!("thread moteur : {e}")))?;
        *self.fil.lock().unwrap_or_else(|e| e.into_inner()) = Some(fil);
        Ok(())
    }

    fn arreter_sortie(&self, kind: SortieKind) -> Result<(), ErreurMoteur> {
        if !self.en_cours() {
            return Err(ErreurMoteur::Inactif(kind));
        }
        self.arreter();
        Ok(())
    }

    fn etat(&self) -> MoteurEtat {
        match &*self.etat.lock().unwrap_or_else(|e| e.into_inner()) {
            EtatInterne::Inactif => MoteurEtat::Inactif,
            EtatInterne::Actif => MoteurEtat::Actif,
            EtatInterne::Erreur(m) => MoteurEtat::Erreur(m.clone()),
        }
    }

    fn apercu(&self) -> Option<ApercuFrame> {
        self.apercu
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Teardown ordonné, idempotent : arrête la boucle et attend le fil.
    /// La session aperçu s'arrête aussi — la fenêtre la relance au poll
    /// suivant si elle est toujours ouverte.
    fn arreter(&self) {
        self.arreter_apercu();
        if let Some(drapeau) = self.arret.lock().unwrap_or_else(|e| e.into_inner()).take() {
            drapeau.store(true, Ordering::Relaxed);
        }
        if let Some(fil) = self.fil.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = fil.join();
        }
        *self.etat.lock().unwrap_or_else(|e| e.into_inner()) = EtatInterne::Inactif;
    }
}

/// Lot de configurations dérivées d'une scène — produit du mapping
/// `scene_sources`, écrit dans les mutexes par `ecrire_lot`.
struct LotScene {
    sources: Vec<SourceCfg>,
    medias: Vec<MediaCfg>,
    textes: Vec<TexteCfg>,
    cadres: Vec<CadreCfg>,
    chats: Vec<ChatCfg>,
    boites: Vec<BoiteCfg>,
    inputs: Vec<InputCfg>,
    horloges: Vec<HorlogeCfg>,
    goals: Vec<GoalCfg>,
    speedruns: Vec<SpeedrunCfg>,
    musiques: Vec<MusiqueCfg>,
    persps: Vec<PerspCfg>,
    /// Gabarit du cadre de la carte alerte (style de la scène, rect nul).
    cadre_overlay_alerte: Option<CadreCfg>,
}

/// Contenu figé d'une session d'enregistrement (captures, médias, textes,
/// cadres, chat, boîtes).
struct SceneChargee {
    sources: Vec<SourceCfg>,
    medias: Vec<MediaCfg>,
    textes: Vec<TexteCfg>,
    cadres: Vec<CadreCfg>,
    chats: Vec<ChatCfg>,
    boites: Vec<BoiteCfg>,
    inputs: Vec<InputCfg>,
    /// Modules horloge de la scène (config + options, z `Z_MODULES`).
    horloges: Vec<HorlogeCfg>,
    /// Widgets goal de la scène (z du widget, cadre à l'échelle bulle).
    goals: Vec<GoalCfg>,
    /// Widgets speedrun de la scène (z du widget, cadre de scène).
    speedruns: Vec<SpeedrunCfg>,
    /// Widgets musique de la scène (z du widget, cadre de scène).
    musiques: Vec<MusiqueCfg>,
    /// Plans à perspective (offscreen + effet par plan).
    persps: Vec<PerspCfg>,
    /// Canvas de la scène d'origine — comparé au canvas de la session à
    /// l'application à chaud (mismatch = dégradation, jamais de resize).
    canvas: (u32, u32),
    /// Buffer vivant : l'app y pousse pendant la session.
    chat: Arc<Mutex<FileChat>>,
    /// État vivant des touches : l'app y pousse pendant la session.
    input: Arc<Mutex<FileInput>>,
    /// État vivant des compteurs goal : l'app y pousse pendant la session.
    goals_etat: Arc<Mutex<EtatGoals>>,
    /// État vivant du splitter ASL : l'app y pousse pendant la session.
    speedruns_etat: Arc<Mutex<EtatSpeedrun>>,
    /// État vivant du lecteur musique : l'app y pousse pendant la session.
    musiques_etat: Arc<Mutex<EtatMusique>>,
    /// État vivant de l'overlay alerte : l'app y pousse pendant la
    /// session — transitoire, jamais par la scène.
    alertes_etat: Arc<Mutex<EtatAlertes>>,
    /// Gabarit du cadre de la carte alerte (style de la scène, rect nul).
    cadre_overlay_alerte: Option<CadreCfg>,
}

/// Session de capture d'une source : WGC (moniteur/fenêtre) ou Media
/// Foundation (caméra). Même rôle pour le moteur : une session refcountée
/// par source, rafraîchie sur le thread de composition.
enum SessionCapture {
    Wgc(CaptureWgc),
    Camera(CaptureCamera),
}

impl SessionCapture {
    /// Taille de la source capturée, en pixels.
    fn taille(&self) -> (u32, u32) {
        match self {
            SessionCapture::Wgc(s) => s.taille(),
            SessionCapture::Camera(s) => s.taille(),
        }
    }

    /// Rafraîchit `destination` avec la frame la plus récente — **sur le
    /// thread appelant** (composition) : copie de la texture du pool WGC,
    /// ou `UpdateSubresource` du slot dernière-frame caméra. `false` =
    /// aucune frame neuve, la destination garde son contenu.
    fn rafraichir(&self, destination: &ID3D11Texture2D, contexte: &ContexteGpu) -> bool {
        match self {
            SessionCapture::Wgc(session) => {
                if let Ok(texture) = session.prochaine_texture(Duration::ZERO) {
                    unsafe { contexte.context().CopyResource(destination, &texture) };
                    true
                } else {
                    false
                }
            }
            SessionCapture::Camera(session) => {
                session.copier_dans(destination).unwrap_or(false)
            }
        }
    }
}

/// Une capture active + sa copie GPU persistante.
///
/// La texture WGC appartient au frame pool : elle est copiée immédiatement,
/// sinon la frame suivante l'invalide. La caméra reçoit sa frame par
/// `UpdateSubresource` — dans les deux cas `cible` persiste toute la session.
struct CaptureActive {
    session: SessionCapture,
    cible: ID3D11Texture2D,
    taille: (u32, u32),
    /// Source d'origine (nom, cible, destination `rect`, `z`, cadre) —
    /// clé du diff d'application à chaud : même `nom` + même `cible` =
    /// session WGC, texture et bitmap conservées.
    cfg: SourceCfg,
    prete: bool,
}

/// La scène chargée telle qu'elle vit dans la boucle : captures, médias
/// décodés, bitmaps Direct2D, index de cadres et plan de composition.
/// `appliquer` la fait évoluer **différentiellement** en cours de
/// session — contrat §3.1 : un déplacement de widget ne recrée ni la
/// session de capture, ni la texture, ni l'encodeur.
struct SceneVivante {
    captures: Vec<CaptureActive>,
    medias: Vec<MediaChargee>,
    /// Parallèle à `medias` : la config qui a produit chaque média —
    /// nécessaire au diff (`MediaChargee` ne garde pas son `chemin`).
    medias_cfg: Vec<MediaCfg>,
    bitmaps_medias: Vec<Vec<ID2D1Bitmap1>>,
    bitmaps_captures: Vec<Option<ID2D1Bitmap1>>,
    /// Parallèle à `captures` : géométrie du **cercle inscrit** pour les
    /// sources à forme dédiée `FormeCamera::Cercle` (`None` partout ailleurs).
    /// Reconstruite à chaque application — un widget caméra est singleton.
    cercles_capture: Vec<Option<ID2D1PathGeometry>>,
    /// Index du polygone de chaque calque dans `voie.geometries`.
    index_cadre_media: Vec<Option<usize>>,
    index_cadre_capture: Vec<Option<usize>>,
    index_cadre_input: Vec<Option<usize>>,
    plan: Vec<(i32, GenreCalque, usize)>,
}

impl SceneVivante {
    /// Application **différentielle** d'une scène en cours de session.
    ///
    /// Jamais fatale : chaque élément irrécupérable (capture qui refuse de
    /// démarrer, média indécodable, bitmap en échec) est signalé par
    /// `compteurs.degrader` et omis — une scène déposée pendant un
    /// enregistrement ne doit pas le tuer.
    fn appliquer(
        &mut self,
        nouvelle: SceneChargee,
        contexte: &Arc<ContexteGpu>,
        moniteurs: &[Moniteur],
        voie: &mut VoieRendu,
        compteurs: &CompteursMoteur,
        canvas: (u32, u32),
    ) {
        let SceneChargee {
            sources,
            medias: medias_cfg,
            textes,
            cadres,
            chats,
            boites,
            inputs,
            horloges,
            goals,
            speedruns,
            musiques,
            persps,
            canvas: canvas_scene,
            cadre_overlay_alerte,
            ..
        } = nouvelle;
        // Le canvas est figé à la création de la session (textures, NV12) :
        // une scène d'un autre canvas s'applique quand même, mais signalée.
        if canvas_scene != canvas {
            compteurs.degrader(format!(
                "scène {}×{} appliquée sur un canvas {}×{}",
                canvas_scene.0, canvas_scene.1, canvas.0, canvas.1
            ));
        }
        if let Err(e) = voie.mettre_a_jour(
            contexte,
            canvas.0,
            canvas.1,
            ContenuRendu {
                textes,
                cadres,
                chats,
                boites,
                inputs,
                horloges,
                goals,
                speedruns,
                musiques,
                persps,
                cadre_overlay_alerte,
            },
        ) {
            compteurs.degrader(format!("application scène : {e}"));
        }
        self.appliquer_captures(&sources, contexte, moniteurs, voie, compteurs);
        self.appliquer_medias(&medias_cfg, contexte, voie, compteurs);
        self.maj_cercles_capture(voie, compteurs);
        self.maj_index_cadres(voie, compteurs);
        self.plan = construire_plan(self, voie);
    }

    /// Captures : match par `nom` (id du widget) — même `cible` → session,
    /// texture et bitmap conservées ; cible différente ou nom nouveau →
    /// nouvelle session ; nom disparu → relâchée (drop libère le WGC).
    fn appliquer_captures(
        &mut self,
        sources: &[SourceCfg],
        contexte: &Arc<ContexteGpu>,
        moniteurs: &[Moniteur],
        voie: &mut VoieRendu,
        compteurs: &CompteursMoteur,
    ) {
        let anciennes = std::mem::take(&mut self.captures);
        let anciennes_bm = std::mem::take(&mut self.bitmaps_captures);
        let mut pool: Vec<(CaptureActive, Option<ID2D1Bitmap1>)> =
            anciennes.into_iter().zip(anciennes_bm).collect();
        for src in sources {
            let position = pool.iter().position(|(c, _)| c.cfg.nom == src.nom);
            let (capture, bitmap) = match position {
                // Même cible : session WGC, texture et bitmap conservées —
                // seules la géométrie et le cadre sont ré-appliqués.
                Some(i) if pool[i].0.cfg.cible == src.cible => {
                    let (mut c, b) = pool.remove(i);
                    c.cfg = src.clone();
                    (c, b)
                }
                autre => {
                    // Cible changée ou source nouvelle : l'ancienne session
                    // (s'il y en a une) est relâchée, puis re-création.
                    if let Some(i) = autre {
                        pool.remove(i);
                    }
                    let nouvelle = match creer_capture(src, moniteurs, contexte) {
                        Ok(c) => c,
                        Err(e) => {
                            compteurs.degrader(e);
                            continue;
                        }
                    };
                    match voie.bitmap_media(&nouvelle.cible, false) {
                        Ok(b) => (nouvelle, Some(b)),
                        Err(e) => {
                            compteurs
                                .degrader(format!("bitmap capture « {} » : {e}", src.nom));
                            continue;
                        }
                    }
                }
            };
            self.captures.push(capture);
            self.bitmaps_captures.push(bitmap);
        }
    }

    /// (Re)construit la géométrie du cercle inscrit des sources caméra —
    /// suit `cfg.rect` : un widget déplacé/redimensionné reçoit son nouveau
    /// clip sans toucher à la session de capture.
    fn maj_cercles_capture(&mut self, voie: &VoieRendu, compteurs: &CompteursMoteur) {
        self.cercles_capture = self
            .captures
            .iter()
            .map(|c| match c.cfg.forme_camera {
                Some(FormeCamera::Cercle) => match voie.geometrie_cercle(c.cfg.rect) {
                    Ok(geo) => Some(geo),
                    Err(e) => {
                        // Jamais un clip faux silencieux : rendu rectangle.
                        compteurs.degrader(format!("clip cercle « {} » : {e}", c.cfg.nom));
                        None
                    }
                },
                _ => None,
            })
            .collect();
    }

    /// Médias : identité de contenu = même `chemin` + mêmes slides →
    /// texture décodée et bitmap conservées, seuls les paramètres (rect,
    /// z, fit, transform, effets, cadre, morphs, rythme du carrousel) sont
    /// ré-appliqués. Une vidéo dont la lecture change est rechargée : son
    /// lecteur MF interne n'est pas reconfigurable de l'extérieur.
    fn appliquer_medias(
        &mut self,
        cfgs: &[MediaCfg],
        contexte: &ContexteGpu,
        voie: &mut VoieRendu,
        compteurs: &CompteursMoteur,
    ) {
        let anciens_cfg = std::mem::take(&mut self.medias_cfg);
        let anciens = std::mem::take(&mut self.medias);
        let anciennes_bm = std::mem::take(&mut self.bitmaps_medias);
        let mut pool: Vec<(MediaCfg, MediaChargee, Vec<ID2D1Bitmap1>)> = anciens_cfg
            .into_iter()
            .zip(anciens)
            .zip(anciennes_bm)
            .map(|((c, m), b)| (c, m, b))
            .collect();
        for cfg in cfgs {
            let position = pool.iter().position(|(ancienne_cfg, media, _)| {
                ancienne_cfg.chemin == cfg.chemin
                    && memes_slides(&ancienne_cfg.carrousel, &cfg.carrousel)
                    && !(media.est_video() && ancienne_cfg.lecture != cfg.lecture)
            });
            let (media, bitmaps) = match position {
                Some(i) => {
                    let (_, mut media, b) = pool.remove(i);
                    media.rect = cfg.rect;
                    media.z = cfg.z;
                    media.fit = cfg.fit;
                    media.transform = cfg.transform;
                    media.effets = cfg.effets;
                    media.cadre = cfg.cadre.clone();
                    media.morphs = cfg.morphs.clone();
                    media.pixel_morph = cfg.pixel_morph;
                    media.lecture = cfg.lecture;
                    if let (Some(carrousel), Some(c)) = (media.carrousel.as_mut(), &cfg.carrousel)
                    {
                        carrousel.sec = c.sec;
                        carrousel.fondu_s = (c.sec / 2.0).min(0.8);
                        carrousel.ease = c.ease;
                    }
                    (media, b)
                }
                None => {
                    let media = match crate::media::charger(contexte, cfg) {
                        Ok(m) => m,
                        Err(e) => {
                            compteurs
                                .degrader(format!("média {} : {e}", cfg.chemin.display()));
                            continue;
                        }
                    };
                    match bitmaps_media_de(&media, voie) {
                        Ok(b) => (media, b),
                        Err(e) => {
                            compteurs.degrader(e);
                            continue;
                        }
                    }
                }
            };
            self.medias_cfg.push(cfg.clone());
            self.medias.push(media);
            self.bitmaps_medias.push(bitmaps);
        }
    }

    /// Recharge les index cadre → géométrie (`voie.geometries` indexé par
    /// position dans `voie.cadres`) — après une application, le cadre
    /// référencé par un calque a pu changer de place ou disparaître.
    fn maj_index_cadres(&mut self, voie: &VoieRendu, compteurs: &CompteursMoteur) {
        self.index_cadre_media = self
            .medias
            .iter()
            .map(|m| {
                m.cadre
                    .as_ref()
                    .and_then(|cadre| voie.cadres.iter().position(|c| c == cadre))
            })
            .collect();
        if self
            .medias
            .iter()
            .zip(&self.index_cadre_media)
            .any(|(m, i)| m.cadre.is_some() && i.is_none())
        {
            // Jamais un rendu faux silencieux : la couche sort non découpée.
            compteurs.degrader("cadre de média introuvable : couche non découpée");
        }
        self.index_cadre_capture = self
            .captures
            .iter()
            .map(|c| {
                c.cfg
                    .cadre
                    .as_ref()
                    .and_then(|cadre| voie.cadres.iter().position(|c| c == cadre))
            })
            .collect();
        if self
            .captures
            .iter()
            .zip(&self.index_cadre_capture)
            .any(|(c, i)| c.cfg.cadre.is_some() && i.is_none())
        {
            compteurs.degrader("cadre de capture introuvable : couche non découpée");
        }
        self.index_cadre_input = voie
            .inputs
            .iter()
            .map(|i| {
                i.cadre
                    .as_ref()
                    .and_then(|cadre| voie.cadres.iter().position(|c| c == cadre))
            })
            .collect();
    }
}

/// Crée une capture WGC + sa texture de destination — partagé par le
/// démarrage de session et la re-création à l'application à chaud.
fn creer_capture(
    source: &SourceCfg,
    moniteurs: &[Moniteur],
    contexte: &Arc<ContexteGpu>,
) -> Result<CaptureActive, String> {
    let session = match &source.cible {
        CibleCapture::Moniteur { index } => {
            let moniteur = moniteurs
                .iter()
                .find(|m| m.index == *index)
                .ok_or_else(|| format!("moniteur {index} introuvable"))?;
            SessionCapture::Wgc(
                CaptureWgc::demarrer_avec(moniteur, Arc::clone(contexte))
                    .map_err(|e| format!("capture « {} » : {e}", source.nom))?,
            )
        }
        CibleCapture::Fenetre { hwnd } => SessionCapture::Wgc(
            CaptureWgc::demarrer_fenetre_avec(*hwnd, Arc::clone(contexte))
                .map_err(|e| format!("capture « {} » : {e}", source.nom))?,
        ),
        // Bloquant (négociation MF ~1-2 s, bornée à 10 s) — même contrat
        // qu'une session WGC, créée une fois par (nom, cible) du diff.
        CibleCapture::Camera { device } => SessionCapture::Camera(
            CaptureCamera::demarrer_avec(device.as_deref(), Arc::clone(contexte))
                .map_err(|e| format!("capture « {} » : {e}", source.nom))?,
        ),
    };
    let taille = session.taille();
    if taille.0 == 0 || taille.1 == 0 {
        return Err(format!("capture « {} » : taille nulle", source.nom));
    }
    let desc = DescTexture::new(Format::Bgra8, taille.0, taille.1)
        .map_err(|e| format!("texture « {} » : {e}", source.nom))?;
    // Échantillonnable : une capture à cadre est découpée par Direct2D.
    let cible = contexte
        .creer_texture_echantillonnable(&desc)
        .map_err(|e| format!("texture « {} » : {e}", source.nom))?;
    Ok(CaptureActive {
        session,
        cible,
        taille,
        cfg: source.clone(),
        prete: false,
    })
}

/// Bitmaps Direct2D d'un média : une par slide de carrousel (la pile
/// `<img class="slide">` du DOM), sinon une seule. Partagé démarrage /
/// application à chaud.
fn bitmaps_media_de(media: &MediaChargee, voie: &mut VoieRendu) -> Result<Vec<ID2D1Bitmap1>, String> {
    let mut bitmaps = Vec::new();
    if let Some(carrousel) = &media.carrousel {
        for slide in &carrousel.slides {
            bitmaps.push(
                voie.bitmap_media(&slide.texture, slide.alpha)
                    .map_err(|e| format!("bitmap slide : {e}"))?,
            );
        }
    } else {
        bitmaps.push(
            voie.bitmap_media(&media.texture, media.alpha)
                .map_err(|e| format!("bitmap média : {e}"))?,
        );
    }
    Ok(bitmaps)
}

/// Plan de composition : ordre z, puis voile → contenu → bordure → cadre.
/// Un widget est un empilement : `.widget` (fond) sous son contenu, la
/// bordure au-dessus, puis `.cadre-overlay` (z-index 5). La bordure n'existe
/// que sans cadre actif (`applyCadreWidget` met `border: none`).
fn construire_plan(vivante: &SceneVivante, voie: &VoieRendu) -> Vec<(i32, GenreCalque, usize)> {
    let mut plan = Vec::new();
    for (i, boite) in voie.boites.iter().enumerate() {
        plan.push((boite.z, GenreCalque::Boite, i));
        if boite.cadre.is_none() {
            plan.push((boite.z, GenreCalque::Bordure, i));
        }
    }
    for (i, media) in vivante.medias.iter().enumerate() {
        plan.push((media.z, GenreCalque::Media, i));
    }
    for (i, capture) in vivante.captures.iter().enumerate() {
        plan.push((capture.cfg.z, GenreCalque::Capture, i));
    }
    for (i, cfg_chat) in voie.chats.iter().enumerate() {
        plan.push((cfg_chat.z, GenreCalque::Chat, i));
    }
    for (i, cfg_input) in voie.inputs.iter().enumerate() {
        plan.push((cfg_input.z, GenreCalque::Input, i));
    }
    for (i, cfg_horloge) in voie.horloges.iter().enumerate() {
        plan.push((cfg_horloge.z, GenreCalque::Horloge, i));
    }
    for (i, cfg_goal) in voie.goals.iter().enumerate() {
        plan.push((cfg_goal.z, GenreCalque::Goal, i));
    }
    for (i, cfg_sr) in voie.speedruns.iter().enumerate() {
        plan.push((cfg_sr.z, GenreCalque::Speedrun, i));
    }
    for (i, cfg_mu) in voie.musiques.iter().enumerate() {
        plan.push((cfg_mu.z, GenreCalque::Musique, i));
    }
    for (i, cadre) in voie.cadres.iter().enumerate() {
        plan.push((cadre.z, GenreCalque::Cadre, i));
    }
    // Overlay alerte (`.alerte-root`, z 9997) : une seule carte
    // transitoire, peinte à son z entre les calques de scène et le bord.
    plan.push((Z_ALERTES, GenreCalque::Alerte, 0));
    plan.sort_by_key(|(z, genre, _)| (*z, *genre));
    plan
}

/// Gabarit du cadre de la carte alerte — miroir `appliquerCadreOverlay`
/// (diffusion.html) : la carte prend le style du `cadreWidget` actif à SA
/// propre taille (le `rect` est posé par le bloc de rendu, mesuré sur la
/// carte). `None` = cadre inactif ou style non porté (les 2 styles du
/// lot — le signal `degrader` est déjà émis par `lot_scene`, comme pour
/// les widgets).
fn cadre_overlay_alerte(scene: &sos_scene::Scene) -> Option<CadreCfg> {
    let cadre = scene
        .cadreWidget
        .as_ref()
        .filter(|c| c.actif && c.strokeWidth > 0.0)?;
    let couleur = couleur_cadre_alerte(&cadre.couleur, [255, 255, 255, 255]);
    let couleur_fin = couleur_cadre_alerte(&cadre.couleurFin, couleur);
    Some(CadreCfg {
        // Rect nul : `appliquerCadreOverlay` régénère le SVG sur la carte
        // mesurée — le bloc pose sa propre boîte.
        rect: (0.0, 0.0, 0.0, 0.0),
        style: crate::cadres::StyleCadre::depuis_nom(&cadre.style)?,
        stroke_width: cadre.strokeWidth,
        couleur,
        couleur_fin,
        gradient_angle: cadre.gradientAngle,
        z: Z_ALERTES,
    })
}

/// Couleur de cadre validée comme `genererSVG` : `#rrggbb` exact, sinon
/// repli — même règle que `scene_sources::couleur_cadre` (privé là-bas).
fn couleur_cadre_alerte(hex: &str, defaut: [u8; 4]) -> [u8; 4] {
    if hex.len() == 7 && hex.starts_with('#') {
        crate::cadres::couleur_hex(hex).unwrap_or(defaut)
    } else {
        defaut
    }
}

/// Identité de contenu d'un carrousel : les chemins des slides (les
/// textures décodées en dépendent, pas le rythme `sec`/`ease`).
fn memes_slides(a: &Option<CarrouselCfg>, b: &Option<CarrouselCfg>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.slides == b.slides,
        _ => false,
    }
}

/// Registre des anneaux **médias** du mixeur live : les `nb_fixes` entrées
/// de tête (sources WASAPI) sont conservées, le reste est reconstruit sur
/// les médias courants — une vidéo ajoutée ou remplacée à chaud rejoint le
/// mix sans redémarrage (`appliquer_scene`), une vidéo retirée en sort.
///
/// Gain média : 1000 ‰ fixe — les scènes n'ont pas de champ de volume
/// (les vidéos sont muettes dans le DOM, `vid.muted = true`).
fn rafraichir_entrees_media(
    mixeur: &mut crate::audio::live::MixeurLocal,
    medias: &[MediaChargee],
    nb_fixes: usize,
) {
    let entrees = mixeur.entrees();
    entrees.truncate(nb_fixes);
    for media in medias {
        if let Some(anneau) = media.anneau_audio() {
            entrees.push(crate::audio::live::EntreeMix {
                anneau,
                gain_milliemes: 1000,
            });
        }
    }
}

/// Corps du fil moteur. Toute erreur remonte à l'état (`Erreur`) ; aucune
/// panique ne doit traverser le fil.
fn boucle(
    cfg: SortieCfg,
    scene: SceneChargee,
    // Courrier des applications à chaud (`Moteur::appliquer_scene`).
    maj_scene: Arc<Mutex<Option<SceneChargee>>>,
    compteurs: Arc<CompteursMoteur>,
    apercu_partage: Arc<Mutex<Option<ApercuFrame>>>,
    arret: Arc<AtomicBool>,
    encodeur_partage: Arc<Mutex<Option<EncodeurInfo>>>,
) -> Result<(), String> {
    let SceneChargee {
        sources,
        medias: medias_cfg,
        textes: textes_cfg,
        cadres,
        chats,
        boites,
        inputs,
        horloges,
        goals,
        speedruns,
        musiques,
        persps,
        canvas: _,
        chat,
        input,
        goals_etat,
        speedruns_etat,
        musiques_etat,
        alertes_etat,
        cadre_overlay_alerte,
    } = scene;
    let (cw, ch) = cfg.canvas;
    let fps = cfg.fps;

    let moniteurs = enumerer_moniteurs().map_err(|e| format!("moniteurs : {e}"))?;
    let adaptateur = sources.iter().find_map(|s| match &s.cible {
        CibleCapture::Moniteur { index } => moniteurs
            .iter()
            .find(|m| m.index == *index)
            .map(|m| m.adaptateur_index),
        _ => None,
    });
    let contexte = Arc::new(
        ContexteGpu::creer(adaptateur).map_err(|e| format!("contexte GPU : {e}"))?,
    );

    let mut captures: Vec<CaptureActive> = Vec::with_capacity(sources.len());
    for source in &sources {
        captures.push(creer_capture(source, &moniteurs, &contexte)?);
    }

    let mut medias: Vec<MediaChargee> = Vec::with_capacity(medias_cfg.len());
    for cfg_media in &medias_cfg {
        let chargee = crate::media::charger(&contexte, cfg_media)
            .map_err(|e| format!("média {} : {e}", cfg_media.chemin.display()))?;
        medias.push(chargee);
    }

    // Composition **100 % Direct2D** (2026-10-28) : le canvas BGRA est la
    // cible de tous les calques (alpha réel, découpe polygonale, voiles), puis
    // une passe `VideoProcessorBlt` le convertit en NV12 pour l'encodeur.
    // `VideoProcessorBlt` ne sait faire ni alpha par pixel ni clip : il ne
    // sert plus qu'à cette conversion (0,145 ms en 1360×768, §30.3).
    let mut voie = VoieRendu::nouveau(
        &contexte,
        cw,
        ch,
        fps,
        ContenuRendu {
            textes: textes_cfg,
            cadres,
            chats,
            boites,
            inputs,
            horloges,
            goals,
            speedruns,
            musiques,
            persps,
            cadre_overlay_alerte,
        },
    )
    .map_err(|e| format!("rendu : {e}"))?;

    // Bitmaps Direct2D de **toutes** les couches + index de leur polygone.
    // Construits **une fois** : la vue Direct2D ne change pas (une vidéo met à
    // jour sa texture, pas son bitmap). Un carrousel a une bitmap **par
    // slide** (la pile `<img class="slide">` du DOM).
    // Bitmaps Direct2D de **toutes** les couches + index de leur polygone.
    // Construits **une fois** : la vue Direct2D ne change pas (une vidéo met à
    // jour sa texture, pas son bitmap). Un carrousel a une bitmap **par
    // slide** (la pile `<img class="slide">` du DOM).
    let mut bitmaps_medias: Vec<Vec<ID2D1Bitmap1>> = Vec::with_capacity(medias.len());
    for media in &medias {
        bitmaps_medias.push(bitmaps_media_de(media, &mut voie)?);
    }
    let mut bitmaps_captures: Vec<Option<ID2D1Bitmap1>> = Vec::with_capacity(captures.len());
    for capture in &captures {
        bitmaps_captures.push(Some(
            voie.bitmap_media(&capture.cible, false)
                .map_err(|e| format!("bitmap capture : {e}"))?,
        ));
    }
    // État vivant de la composition — `appliquer_scene` le fait évoluer en
    // cours de session par le courrier `maj_scene`.
    let mut vivante = SceneVivante {
        captures,
        medias,
        medias_cfg,
        bitmaps_medias,
        bitmaps_captures,
        cercles_capture: Vec::new(),
        index_cadre_media: Vec::new(),
        index_cadre_capture: Vec::new(),
        index_cadre_input: Vec::new(),
        plan: Vec::new(),
    };
    vivante.maj_cercles_capture(&voie, &compteurs);
    vivante.maj_index_cadres(&voie, &compteurs);
    let plan = construire_plan(&vivante, &voie);
    vivante.plan = plan;

    // Texture que l'encodeur consomme : le NV12 converti depuis le canvas BGRA.
    let cible_encode = voie.nv12.clone();

    // Un sink encodé (fichier/RTMP/SRT) exige un encodeur ; une session
    // « aperçu seul » (`SinkCfg::Apercu` uniquement — projecteur natif) n'en
    // crée JAMAIS : composition + lecture réduite suffisent, la fenêtre
    // reste très légère (pas de NVENC/MFT initialisé pour rien).
    let encode_requis = cfg.sinks.iter().any(|s| !matches!(s, SinkCfg::Apercu));

    // Tous les encodeurs MFT, matériels puis logiciels : le repli doit
    // pouvoir descendre jusqu'au logiciel même quand un matériel est listé
    // mais refuse de s'initialiser ou d'encoder (exigence plan §11.3).
    let mut disponibles = if encode_requis {
        detect::lister_tous(cfg.codec).map_err(|e| format!("encodeurs : {e}"))?
    } else {
        Vec::new()
    };
    let forcer_logiciel = std::env::var_os("SOS_MOTEUR_ENCODEUR_LOGICIEL").is_some();
    if forcer_logiciel {
        disponibles.retain(|d| !d.materiel);
    }
    let mut index = 0usize;

    // 1) NVENC direct (zéro-copie prouvée) si présent. `SOS_MOTEUR_SANS_NVENC`
    //    force le chemin MFT (diagnostic / comparaison).
    let sans_nvenc =
        forcer_logiciel || std::env::var_os("SOS_MOTEUR_SANS_NVENC").is_some();
    // Preset d'encodage : absent/vide → défaut produit ; nom inconnu →
    // dégradation signalée puis défaut (jamais de repli muet).
    let preset = match cfg.preset.trim() {
        "" => Preset::DEFAUT,
        autre => Preset::depuis_nom(autre).unwrap_or_else(|| {
            compteurs.degrader(format!("preset inconnu «{autre}» → equilibre"));
            Preset::DEFAUT
        }),
    };
    let mut encodeur: Option<EncodeurActif> = None;
    let mut derniere_erreur = String::new();
    if !sans_nvenc && encode_requis {
        match EncodeurNvenc::nouveau(&contexte, cfg.codec, cw, ch, fps, cfg.debit_kbps, preset) {
            Ok(mut nvenc) => match nvenc.enregistrer(&cible_encode) {
                Ok(()) => {
                    *encodeur_partage
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(EncodeurInfo {
                        nom: "NVENC H.264".to_string(),
                        materiel: true,
                        zero_copie: true,
                    });
                    encodeur = Some(EncodeurActif::Nvenc(Box::new(nvenc)));
                }
                Err(e) => derniere_erreur = e.to_string(),
            },
            Err(e) => derniere_erreur = e.to_string(),
        }
    }

    // 2) Repli MFT (ou chemin principal si NVENC absent).
    let mut zero_copie = false;
    if encode_requis && encodeur.is_none() {
        match ouvrir_mft(&disponibles, &mut index, &cfg, cw, ch, fps, &contexte, preset) {
            Some((enc, mut info)) => {
                zero_copie =
                    info.materiel && std::env::var_os("SOS_MOTEUR_MFT_MEMOIRE").is_none();
                info.zero_copie = zero_copie;
                *encodeur_partage
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(info);
                encodeur = Some(EncodeurActif::Mft(enc));
            }
            None => {
                return Err(format!(
                    "aucun encodeur {} utilisable : {derniere_erreur}",
                    cfg.codec.nom()
                ));
            }
        }
    }

    if encode_requis && encodeur.is_none() {
        return Err(format!(
            "aucun encodeur {} utilisable : {derniere_erreur}",
            cfg.codec.nom()
        ));
    }

    // Audio demandé : `actif()` = au moins une source cochée. Il pilote
    // **deux** voies — live (pipe nommé → AAC dans le conteneur, en direct)
    // ou post-mux (PCM fichier + `muxer_audio` à l'arrêt, historique).
    let audio_demande = cfg.audio.filter(|a| a.actif());

    let mut sinks: Vec<Box<dyn Sink>> = Vec::new();
    // Chemins des sorties **fichier** : le post-mux (repli/réparation) se
    // fait après fermeture du conteneur, sur le fichier finalisé. Les sinks
    // réseau n'y figurent pas — il n'y a rien à remuxer hors ligne.
    let mut chemins_sortie: Vec<std::path::PathBuf> = Vec::new();
    // Extrémités serveur des pipes audio live (fichier et réseau).
    let mut pipes_audio: Vec<crate::output::ffmpeg::PipeAudio> = Vec::new();
    for (index, sink) in cfg.sinks.iter().enumerate() {
        match sink {
            SinkCfg::Fichier { chemin, conteneur } => {
                // Voie live tentée quand l'audio est demandé ; échec → sink
                // classique + post-mux à l'arrêt (zéro régression).
                if audio_demande.is_some() {
                    match FfmpegSink::nouveau_avec_audio_codec(
                        None, chemin, *conteneur, cfg.codec, fps, index,
                    ) {
                        Ok((s, pipe)) => {
                            sinks.push(Box::new(s));
                            pipes_audio.push(pipe);
                            chemins_sortie.push(chemin.clone());
                            continue;
                        }
                        Err(e) => compteurs.degrader(format!(
                            "audio live {} : {e} — repli post-mux",
                            chemin.display()
                        )),
                    }
                }
                let s = FfmpegSink::nouveau_codec(None, chemin, *conteneur, cfg.codec, fps)
                    .map_err(|e| format!("sortie {} : {e}", chemin.display()))?;
                sinks.push(Box::new(s));
                chemins_sortie.push(chemin.clone());
            }
            SinkCfg::Apercu => {}
            SinkCfg::Rtmp { url } => {
                sink_reseau(
                    &mut sinks,
                    &mut pipes_audio,
                    ReseauCfg {
                        url: &url_rtmp(url, cfg.cle.as_deref()),
                        cle: cfg.cle.as_deref(),
                        format: FormatReseau::Flv,
                        codec: cfg.codec,
                        fps,
                        index,
                        audio: audio_demande.is_some(),
                    },
                    &compteurs,
                )?;
            }
            SinkCfg::Srt { url } => {
                sink_reseau(
                    &mut sinks,
                    &mut pipes_audio,
                    ReseauCfg {
                        url: &url_srt(url, cfg.cle.as_deref()),
                        cle: cfg.cle.as_deref(),
                        format: FormatReseau::Mpegts,
                        codec: cfg.codec,
                        fps,
                        index,
                        audio: audio_demande.is_some(),
                    },
                    &compteurs,
                )?;
            }
        }
    }
    // Chemin live réellement actif : au moins un sink a son pipe.
    let audio_live = !pipes_audio.is_empty();
    // `true` quand un `WriteFile` a cassé en cours — le post-mux réparera.
    let mut audio_live_casse = false;
    if sinks.is_empty() && encode_requis {
        return Err("aucune sortie".to_string());
    }

    let (pw, ph) = taille_apercu(cw, ch);
    let mut apercu = Apercu::nouveau(&contexte, (cw, ch), (pw, ph)).ok();
    let mut horloge = Horloge::nouvelle(fps).map_err(|e| format!("horloge : {e}"))?;
    let mut gouverneur = Gouverneur::nouveau();

    // Chemin zéro-copie par défaut **pour un encodeur MFT matériel**. Un MFT
    // logiciel n'est pas D3D11-aware : il consomme de la mémoire, pas une
    // surface. Certains MFT matériels refusent aussi la surface
    // (« ProcessOutput E_FAIL ») : repli explicite, signalé au diagnostic.
    // `SOS_MOTEUR_MFT_MEMOIRE=1` force la copie.
    let convertisseur = if encode_requis {
        ConvertisseurNv12::nouveau(&contexte, cw, ch, fps).ok()
    } else {
        None
    };
    if matches!(encodeur, Some(EncodeurActif::Mft(_))) && !zero_copie {
        compteurs.degrader("encodeur MFT logiciel : copie NV12");
    }

    compteurs.maj_resolution(cw, ch);

    // Audio : un thread par source, démarré **juste avant** la boucle pour
    // que l'offset A/V reste borné. Une source indisponible dégrade
    // l'enregistrement (muet sur cette source), elle ne l'annule jamais.
    //
    // Deux voies selon le sink réussi :
    // - **live** (`audio_live`) : `SourceLive` pousse dans un anneau (mixage
    //   temps réel par `MixeurLocal`, dans cette boucle) **et** écrit le PCM
    //   fichier — le post-mux reste la réparation si le pipe casse ;
    // - **post-mux** (repli) : `CapturePiste` historique, `muxer_audio` à
    //   l'arrêt — rien ne change par rapport à avant ce lot.
    let mut captures_audio: Vec<crate::audio::piste::CapturePiste> = Vec::new();
    let mut sources_live: Vec<crate::audio::live::SourceLive> = Vec::new();
    let mut mixeur = crate::audio::live::MixeurLocal::nouveau();
    if let Some(audio) = audio_demande {
        if audio_live {
            for (sens, actif, gain, nom) in [
                (
                    crate::audio::Sens::Sortie,
                    audio.loopback,
                    audio.gain_loopback,
                    "loop",
                ),
                (
                    crate::audio::Sens::Entree,
                    audio.micro,
                    audio.gain_micro,
                    "mic",
                ),
            ] {
                if !actif {
                    continue;
                }
                match crate::audio::live::SourceLive::demarrer(sens, gain, nom) {
                    Ok(mut source) => {
                        // Aligne le son sur la première frame : on attend le
                        // premier paquet réel (buffer WASAPI de 200 ms).
                        let _ = source.attendre_demarrage(Duration::from_millis(400));
                        mixeur.entrees().push(crate::audio::live::EntreeMix {
                            anneau: source.anneau(),
                            gain_milliemes: gain,
                        });
                        sources_live.push(source);
                    }
                    Err(e) => compteurs.degrader(format!("audio {nom} indisponible : {e}")),
                }
            }
        } else if !chemins_sortie.is_empty() {
            // Repli post-mux : n'a de sens qu'avec au moins une sortie
            // fichier à réécrire à l'arrêt — sur une sortie 100 % réseau,
            // capturer du PCM jetable serait pur gâchis.
            for (sens, actif, gain, nom) in [
                (
                    crate::audio::Sens::Sortie,
                    audio.loopback,
                    audio.gain_loopback,
                    "loop",
                ),
                (
                    crate::audio::Sens::Entree,
                    audio.micro,
                    audio.gain_micro,
                    "mic",
                ),
            ] {
                if !actif {
                    continue;
                }
                match crate::audio::piste::demarrer(sens, gain, nom) {
                    Ok(mut capture) => {
                        let _ = capture.attendre_demarrage(Duration::from_millis(400));
                        captures_audio.push(capture);
                    }
                    Err(e) => compteurs.degrader(format!("audio {nom} indisponible : {e}")),
                }
            }
        }
    }
    // Entrées « fixes » = les sources WASAPI ; les anneaux des médias sont
    // empilés après et reconstruits à chaque `appliquer_scene` — une vidéo
    // ajoutée à chaud rejoint le mix (contrat « appliquer à chaud »).
    let nb_entrees_fixes = mixeur.entrees().len();
    if audio_live {
        rafraichir_entrees_media(&mut mixeur, &vivante.medias, nb_entrees_fixes);
        // Zéro du flux : purge le pré-roll WASAPI (le buffer de 200 ms rempli
        // pendant l'alignement n'est pas de l'audio « t = 0 »).
        mixeur.demarrer();
    }

    let debut = Instant::now();
    let mut dernier_echantillon = Instant::now();
    let mut octets_fenetre: u64 = 0;
    let mut erreur_sortie: Option<String> = None;

    while !arret.load(Ordering::Relaxed) {
        horloge.attendre();
        let pts_us = debut.elapsed().as_micros() as i64;

        // Application à chaud : la scène la plus récente déposée par
        // `appliquer_scene` remplace la composition — différentiel : les
        // captures et médias inchangés gardent sessions, textures et
        // bitmaps ; l'encodeur et les sinks ne sont jamais touchés.
        if let Some(nouvelle) = maj_scene
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            vivante.appliquer(
                nouvelle,
                &contexte,
                &moniteurs,
                &mut voie,
                &compteurs,
                (cw, ch),
            );
            // Une vidéo ajoutée/remplacée à chaud rejoint le mix audio —
            // ses échantillons arrivent au prochain `avancer_audio`.
            if audio_live {
                rafraichir_entrees_media(&mut mixeur, &vivante.medias, nb_entrees_fixes);
            }
        }
        let SceneVivante {
            captures,
            medias,
            bitmaps_medias,
            bitmaps_captures,
            cercles_capture,
            index_cadre_media,
            index_cadre_capture,
            index_cadre_input,
            plan,
            ..
        } = &mut vivante;

        for media in medias.iter_mut() {
            media
                .avancer(pts_us)
                .map_err(|e| format!("média vidéo : {e}"))?;
            // Son de la vidéo : alimente l'anneau (drain non fatal — une
            // piste audio cassée dégrade le mix, jamais la vidéo).
            if audio_live {
                if let Err(e) = media.avancer_audio(pts_us) {
                    compteurs.degrader(format!("audio média : {e}"));
                }
            }
        }
        // Mixage live : les frames 48 kHz dues depuis le zéro du flux sont
        // poussées dans chaque pipe (interleaving A/V dans le conteneur).
        // Un pipe cassé est retiré — `muxer_audio` réparera à l'arrêt.
        if audio_live {
            let pcm = mixeur.produire();
            if !pcm.is_empty() {
                let mut i = 0;
                while i < pipes_audio.len() {
                    match pipes_audio[i].ecrire(pcm) {
                        Ok(()) => i += 1,
                        Err(e) => {
                            compteurs.degrader(format!("pipe audio : {e}"));
                            audio_live_casse = true;
                            pipes_audio.remove(i);
                        }
                    }
                }
            }
        }
        for capture in captures.iter_mut() {
            // Copie WGC ou `UpdateSubresource` caméra — sur ce thread dans
            // les deux cas ; `false` = pas de frame neuve, texture gardée.
            if capture.session.rafraichir(&capture.cible, &contexte) {
                capture.prete = true;
                compteurs.capture();
            }
        }

        // **Une seule passe Direct2D par frame** : le canvas est remis au noir,
        // puis chaque entrée du plan est peinte dans l'ordre z. Aucun
        // `VideoProcessorBlt` n'est inséré dans la passe (piège §35.7.5) : la
        // conversion NV12 vient après `fin()`, et l'encodeur la consomme.
        voie.debut();
        voie.effacer();
        voie.effacer_persps();
        for (z, genre, index) in plan.iter() {
            // Plans à perspective (`perspective(d) rotateX/Y`, lot T2) : les
            // calques du plan (voile/média/capture/bordure/cadre — pas le
            // contenu chat/input, limite §41) dessinent dans l'offscreen du
            // groupe, projeté sur le canvas quand le plan sort de son z.
            voie.flusher_persps(*z)?;
            let gid = match genre {
                GenreCalque::Boite | GenreCalque::Bordure => {
                    voie.groupe_de(voie.boites[*index].rect, *z)
                }
                GenreCalque::Media => voie.groupe_de(medias[*index].rect, *z),
                GenreCalque::Capture => voie.groupe_de(captures[*index].cfg.rect, *z),
                GenreCalque::Cadre => voie.groupe_de(voie.cadres[*index].rect, *z),
                // HUD et contenus directs : jamais dans un plan à
                // perspective (`#modules-root` et `.alerte-root` sont hors
                // `.widget`).
                GenreCalque::Chat
                | GenreCalque::Input
                | GenreCalque::Horloge
                | GenreCalque::Goal
                | GenreCalque::Speedrun
                | GenreCalque::Musique
                | GenreCalque::Alerte => None,
            };
            voie.cible_pour(gid);
            match genre {
                GenreCalque::Boite => voie
                    .dessiner_boite(*index)
                    .map_err(|e| format!("voile : {e}"))?,
                GenreCalque::Media => {
                    let media = &medias[*index];
                    // Carrousel (`slidesAvancer` / `dessinerContenuFond`) :
                    // slide courante, + précédente pendant le fondu (widget
                    // `ease-in-out`, fond linéaire). Sous morph = swap sec
                    // de `src` du DOM : la courante seule, sans fondu.
                    let (courant, fondu) = media
                        .carrousel
                        .as_ref()
                        .map(|c| c.fenetre(pts_us as f64 / 1e6))
                        .unwrap_or((0, None));
                    let passes: [(usize, f32); 2] = match (media.morphs.is_empty(), fondu)
                    {
                        (true, Some((p, k))) => [(p, 1.0 - k), (courant, k)],
                        _ => [(courant, 1.0), (usize::MAX, 0.0)],
                    };
                    for &(slide, opacite) in &passes {
                        if slide == usize::MAX {
                            break;
                        }
                        let Some(bitmap) = bitmaps_medias[*index].get(slide) else {
                            continue;
                        };
                        // Le fit se recalcule par slide (tailles possibles
                        // différentes, comme les `<img>` du DOM).
                        let taille = media.taille_slide(slide);
                        // Transform 2D (`applyMediaStyle` /
                        // `dessinerContenuFond`) autour du centre de la boîte.
                        // Sous transform, le rognage canvas AVANT rotation/zoom
                        // est faux — la paire du fit est dessinée entière et
                        // c'est le clip de la boîte (ou le polygone du cadre)
                        // qui rogne, comme `overflow:hidden`. `zoom_ecran`
                        // (`bgZoom`) est porté par l'effet du plan : retiré de
                        // la matrice 2D pour une entrée groupée.
                        let mut t = media.transform;
                        if gid.is_some() {
                            t.zoom_ecran = 1.0;
                        }
                        let transforme = (!t.est_neutre() || gid.is_some()).then(|| {
                            t.matrice((
                                media.rect.0 + media.rect.2 / 2.0,
                                media.rect.1 + media.rect.3 / 2.0,
                            ))
                        });
                        // Morphs bulge/pinch (lot T4) : offscreen taille
                        // boîte + warp `MORPH_FRAG` — le `zoom_ecran` vit
                        // hors de l'offscreen (retiré de `t` si groupé).
                        if !media.morphs.is_empty() {
                            voie.dessiner_media_morphe(
                                bitmap,
                                &MorpheCouche {
                                    taille,
                                    boite: media.rect,
                                    fit: media.fit,
                                    transform: &t,
                                    cadre: index_cadre_media[*index],
                                    effets: &media.effets,
                                    morphs: &media.morphs,
                                    pixel_morph: media.pixel_morph,
                                },
                            )
                            .map_err(|e| format!("média morphé : {e}"))?;
                        } else if media.effets.pixel_active() {
                            voie.dessiner_media_pixel(
                                bitmap,
                                &PixelCouche {
                                    taille,
                                    boite: media.rect,
                                    fit: media.fit,
                                    transform: &t,
                                    cadre: index_cadre_media[*index],
                                    effets: &media.effets,
                                    opacite,
                                },
                            )
                            .map_err(|e| format!("média pixelisé : {e}"))?;
                        } else {
                            // `fit_rects` (sans rognage canvas) sous transform OU
                            // sous perspective : les deux peuvent ramener des
                            // pixels hors canvas dans le champ.
                            let (source, dest) = if transforme.is_some() || gid.is_some() {
                                crate::media::fit_rects(taille, media.rect, media.fit)
                            } else {
                                crate::media::geometrie(taille, media.rect, (cw, ch), media.fit)
                            };
                            voie.dessiner_media(
                                bitmap,
                                &TraceCouche {
                                    source,
                                    destination: dest,
                                    boite: rect_boite(media.rect),
                                    transforme,
                                    cadre: index_cadre_media[*index],
                                    clip_geo: None,
                                    effets: &media.effets,
                                    opacite,
                                },
                            )
                            .map_err(|e| format!("média : {e}"))?;
                        }
                    }
                }
                GenreCalque::Capture => {
                    let capture = &captures[*index];
                    let Some(bitmap) = bitmaps_captures[*index].as_ref() else {
                        continue;
                    };
                    if !capture.prete {
                        continue;
                    }
                    // Même sémantique que le mode OBS (`OBS_BOUNDS_SCALE_OUTER`
                    // + crop, contrat 6 d'`AGENTS.md`) : la capture remplit le
                    // widget sans déformation, le débordement est croppé.
                    // `fit_rects` sous perspective (pas de rognage préalable).
                    let (source, dest) = if gid.is_some() {
                        crate::media::fit_rects(
                            capture.taille,
                            capture.cfg.rect,
                            crate::media::Fit::Remplir,
                        )
                    } else {
                        crate::media::geometrie(
                            capture.taille,
                            capture.cfg.rect,
                            (cw, ch),
                            crate::media::Fit::Remplir,
                        )
                    };
                    voie.dessiner_media(
                        bitmap,
                        &TraceCouche {
                            source,
                            destination: dest,
                            boite: rect_boite(capture.cfg.rect),
                            transforme: None,
                            cadre: index_cadre_capture[*index],
                            // Forme dédiée caméra (cercle inscrit) : prime
                            // sur le cadre, absent de `voie.geometries`.
                            clip_geo: cercles_capture[*index].as_ref(),
                            effets: &crate::media::EffetsCfg::NEUTRE,
                            opacite: 1.0,
                        },
                    )
                    .map_err(|e| format!("capture : {e}"))?;
                }
                GenreCalque::Chat => {
                    let file = chat.lock().unwrap_or_else(|e| e.into_inner());
                    // Barre viewers : même canal live que les goals (frames
                    // `viewers`). Ordre de verrouillage chat → goals_etat :
                    // les pousseurs (`pousser_chat`/`pousser_goal`) sont des
                    // verrous simples, aucun chemin inverse — pas de cycle.
                    let etat = goals_etat.lock().unwrap_or_else(|e| e.into_inner());
                    voie.dessiner_chat(*index, &file, &etat)
                        .map_err(|e| format!("chat : {e}"))?;
                }
                GenreCalque::Input => {
                    let file = input.lock().unwrap_or_else(|e| e.into_inner());
                    voie.dessiner_input(*index, &file, index_cadre_input[*index])
                        .map_err(|e| format!("input viewer : {e}"))?;
                }
                GenreCalque::Horloge => voie
                    .dessiner_horloge(*index)
                    .map_err(|e| format!("horloge : {e}"))?,
                GenreCalque::Goal => {
                    let etat = goals_etat.lock().unwrap_or_else(|e| e.into_inner());
                    voie.dessiner_goal(*index, &etat)
                        .map_err(|e| format!("goal : {e}"))?;
                }
                GenreCalque::Speedrun => {
                    // Verrou court : l'état vit hors de la scène (frames
                    // `speedrun-etat` ~15 Hz en run), jamais de
                    // reconfiguration pour un tick.
                    let etat = speedruns_etat.lock().unwrap_or_else(|e| e.into_inner());
                    voie.dessiner_speedrun(*index, &etat)
                        .map_err(|e| format!("speedrun : {e}"))?;
                }
                GenreCalque::Musique => {
                    // Verrou court : l'état vit hors de la scène (events
                    // `musique:update`), jamais de reconfiguration pour un
                    // snapshot du lecteur.
                    let etat = musiques_etat.lock().unwrap_or_else(|e| e.into_inner());
                    voie.dessiner_musique(*index, &etat)
                        .map_err(|e| format!("musique : {e}"))?;
                }
                GenreCalque::Alerte => {
                    // Verrou court : l'état vit hors de la scène (frames
                    // `alerte-*` du canal `chat_tx`), jamais de
                    // reconfiguration pour un play. Aucune alerte en cours
                    // = no-op.
                    let etat = alertes_etat.lock().unwrap_or_else(|e| e.into_inner());
                    voie.dessiner_alerte(&etat, ch)
                        .map_err(|e| format!("alerte : {e}"))?;
                }
                GenreCalque::Bordure => voie
                    .dessiner_bordure_boite(*index)
                    .map_err(|e| format!("bordure : {e}"))?,
                GenreCalque::Cadre => voie
                    .dessiner_cadre(*index)
                    .map_err(|e| format!("cadre : {e}"))?,
            }
        }
        voie.flusher_persps_fin()?;
        voie.dessiner_textes_passe()
            .map_err(|e| format!("texte : {e}"))?;
        voie.fin().map_err(|e| format!("rendu : {e}"))?;
        voie.convertir().map_err(|e| format!("rendu : {e}"))?;
        compteurs.composite();

        let paquets = match encodeur.as_mut() {
            Some(enc) => {
                let mut resultat = encoder_actif(
                    enc,
                    &mut zero_copie,
                    convertisseur.as_ref(),
                    &cible_encode,
                    pts_us,
                );
                if let Err(e) = &resultat {
                    // Repli : NVENC ou MFT refuse la frame. On descend dans
                    // l'ordre de détection MFT puis on rejoue la frame —
                    // une seule fois.
                    match ouvrir_mft(&disponibles, &mut index, &cfg, cw, ch, fps, &contexte, preset)
                    {
                        Some((mft, mut info)) => {
                            zero_copie = info.materiel
                                && std::env::var_os("SOS_MOTEUR_MFT_MEMOIRE").is_none();
                            info.zero_copie = zero_copie;
                            *encodeur_partage
                                .lock()
                                .unwrap_or_else(|e| e.into_inner()) = Some(info);
                            compteurs.degrader(format!("repli encodeur ({e})"));
                            *enc = EncodeurActif::Mft(mft);
                            resultat = encoder_actif(
                                enc,
                                &mut zero_copie,
                                convertisseur.as_ref(),
                                &cible_encode,
                                pts_us,
                            );
                        }
                        None => return Err(format!("encodage : {e}")),
                    }
                }
                resultat.map_err(|e| format!("encodage : {e}"))?
            }
            // Aperçu seul : aucun encodeur, aucun paquet à pousser.
            None => Vec::new(),
        };
        compteurs.maj_file_encode(paquets.len() as u32);
        for paquet in &paquets {
            octets_fenetre += paquet.data.len() as u64;
            let mut i = 0;
            while i < sinks.len() {
                let resultat = sinks[i].pousser(paquet);
                match resultat {
                    Ok(()) => i += 1,
                    Err(e) => {
                        // Sink retiré sans arrêter les autres (contrat §3.2).
                        erreur_sortie = Some(format!("sortie : {e}"));
                        sinks.remove(i);
                    }
                }
            }
            if sinks.is_empty() {
                return Err("toutes les sorties ont échoué".to_string());
            }
        }

        if let Some(a) = apercu.as_mut() {
            let pas = (fps / 10).max(1) as u64;
            if horloge.ticks() % pas == 0 {
                if let Ok(tampon) = a.lire(&cible_encode) {
                    let frame = ApercuFrame {
                        data: tampon.to_vec(),
                        largeur: pw,
                        hauteur: ph,
                        pts_us,
                    };
                    *apercu_partage.lock().unwrap_or_else(|p| p.into_inner()) = Some(frame);
                }
            }
        }

        if dernier_echantillon.elapsed() >= Duration::from_secs(1) {
            let instantane = compteurs.instantane();
            compteurs.maj_kbps((octets_fenetre * 8 / 1000) as u32);
            compteurs.maj_latence(horloge.retard_max().as_millis() as u32);
            octets_fenetre = 0;

            let mesures = Mesures {
                fps_cible: fps,
                fps_reel: horloge.fps_effectif(),
                drops: instantane.frames_perdues,
                file_encode: instantane.file_encode,
                latence_ms: instantane.latence_ms,
                resolution: (cw, ch),
            };
            let decision = gouverneur.evaluer(&mesures);
            if decision.action != Action::Rien {
                compteurs.degrader(decision.to_string());
            }
            if matches!(encodeur, Some(EncodeurActif::Mft(_))) {
                let mut diag = encodeur_partage.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(info) = diag.as_mut() {
                    info.zero_copie = zero_copie;
                }
            }
            dernier_echantillon = Instant::now();
        }
    }

    let derniers = match encodeur.as_mut() {
        Some(EncodeurActif::Nvenc(nvenc)) => nvenc.terminer(),
        Some(EncodeurActif::Mft(mft)) => mft.terminer(),
        None => Ok(Vec::new()),
    };
    if let Ok(paquets) = derniers {
        for paquet in &paquets {
            for sink in sinks.iter_mut() {
                let _ = sink.pousser(paquet);
            }
        }
    }
    // Audio live : dernières frames dues puis **EOF** des pipes — ffmpeg
    // termine la piste AAC au dernier échantillon. Fermé AVANT `fermer()`
    // pour que le conteneur se finalise avec un audio complet.
    if audio_live {
        let pcm = mixeur.produire();
        for pipe in &mut pipes_audio {
            let _ = pipe.ecrire(pcm);
        }
        drop(pipes_audio);
    }
    for sink in sinks.iter_mut() {
        let _ = sink.fermer();
    }

    // Audio : arrêt des captures, puis muxage dans chaque fichier finalisé
    // (post-mux historique, ou **réparation** si le live a cassé en cours —
    // `muxer_audio` remappe `0:v` + les PCM sur le fichier existant).
    // Le PCM temporaire est supprimé dans tous les cas.
    let mut pistes: Vec<crate::audio::piste::Piste> = Vec::new();
    for source in sources_live {
        match source.arreter() {
            Ok(piste) => pistes.push(piste),
            Err(e) => compteurs.degrader(format!("audio : {e}")),
        }
    }
    for capture in captures_audio {
        match capture.arreter() {
            Ok(piste) => pistes.push(piste),
            Err(e) => compteurs.degrader(format!("audio : {e}")),
        }
    }
    // Live sain → le fichier porte déjà son AAC, rien à remuxer.
    let post_mux_requis = !audio_live || audio_live_casse;
    if erreur_sortie.is_none() && post_mux_requis {
        for piste in &pistes {
            log::info!(
                "audio {} : {:.2} s, {} Hz, {} canaux, gain {}‰",
                piste.chemin.display(),
                piste.duree_s,
                piste.taux,
                piste.canaux,
                piste.gain_milliemes
            );
        }
        for chemin in &chemins_sortie {
            if let Err(e) = crate::output::ffmpeg::muxer_audio(chemin, &pistes) {
                compteurs.degrader(format!("muxage audio ({}) : {e}", chemin.display()));
            }
        }
    }
    for piste in &pistes {
        let _ = std::fs::remove_file(&piste.chemin);
    }

    if let Some(e) = erreur_sortie {
        return Err(e);
    }
    Ok(())
}

/// Encodeur actif : NVENC direct (zéro-copie) ou MFT (universel).
enum EncodeurActif {
    Nvenc(Box<EncodeurNvenc>),
    Mft(EncodeurMft),
}

/// URL finale d'un sink RTMP : la clé de flux (`SortieCfg.cle`) est accolée
/// au chemin de l'application (`rtmp://hôte/app/<clé>` — convention
/// Twitch/YouTube). `cle` vide ou absente = l'URL est prise telle quelle.
fn url_rtmp(url: &str, cle: Option<&str>) -> String {
    match cle.filter(|c| !c.is_empty()) {
        Some(c) => format!("{}/{}", url.trim_end_matches('/'), c),
        None => url.to_string(),
    }
}

/// URL finale d'un sink SRT : la clé devient le paramètre `streamid`
/// (convention YouTube SRT / serveurs orientés streamid). `cle` vide ou
/// absente = l'URL est prise telle quelle.
fn url_srt(url: &str, cle: Option<&str>) -> String {
    match cle.filter(|c| !c.is_empty()) {
        Some(c) => {
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{url}{sep}streamid={c}")
        }
        None => url.to_string(),
    }
}

/// Paramètres d'un sink réseau (RTMP/FLV ou SRT/MPEG-TS).
struct ReseauCfg<'a> {
    /// URL complète — n'entre jamais dans un message d'erreur (le sink la
    /// masque lui-même : elle embarque la clé de flux).
    url: &'a str,
    /// Clé de flux seule — masquée en plus de l'URL (ffmpeg peut citer une
    /// forme tronquée où la clé resterait lisible).
    cle: Option<&'a str>,
    format: FormatReseau,
    codec: Codec,
    fps: u32,
    /// Index du sink dans `SortieCfg.sinks` — nomme le pipe audio.
    index: usize,
    /// `true` = tentative audio live (pipe nommé) avant repli vidéo seule.
    audio: bool,
}

/// Crée un sink réseau et son pipe audio si la sortie en demande — la voie
/// live est tentée d'abord ; son échec dégrade vers un flux **vidéo seule**
/// (le post-mux ne peut pas réparer un flux réseau : il n'y a pas de
/// fichier à réécrire à l'arrêt).
fn sink_reseau(
    sinks: &mut Vec<Box<dyn Sink>>,
    pipes_audio: &mut Vec<crate::output::ffmpeg::PipeAudio>,
    cfg: ReseauCfg<'_>,
    compteurs: &CompteursMoteur,
) -> Result<(), String> {
    if cfg.audio {
        match FfmpegSink::nouveau_reseau_avec_audio(
            None, cfg.url, cfg.cle, cfg.format, cfg.codec, cfg.fps, cfg.index,
        ) {
            Ok((s, pipe)) => {
                sinks.push(Box::new(s));
                pipes_audio.push(pipe);
                return Ok(());
            }
            Err(e) => compteurs.degrader(format!(
                "audio live réseau : {e} — repli vidéo seule"
            )),
        }
    }
    let s = FfmpegSink::nouveau_reseau(
        None, cfg.url, cfg.cle, cfg.format, cfg.codec, cfg.fps,
    )
    .map_err(|e| format!("sortie réseau : {e}"))?;
    sinks.push(Box::new(s));
    Ok(())
}

/// Voie de rendu : canvas BGRA composé par Direct2D, puis conversion NV12.
///
/// **Toute** la scène passe par ce canvas (2026-10-28) : médias, captures,
/// voiles, bordures, cadres, chat et textes. `VideoProcessorBlt` ne sert plus
/// qu'à la conversion NV12 — c'est la seule façon d'avoir un alpha par pixel,
/// une découpe polygonale et un ordre z exact. La texture `nv12` est celle que
/// l'encodeur a enregistrée **une fois** (même contrat zéro-copie).
struct VoieRendu {
    canvas: ID3D11Texture2D,
    nv12: ID3D11Texture2D,
    convertisseur: ConvertisseurNv12,
    dessinateur: DessinateurTexte,
    textes: Vec<TexteCfg>,
    cadres: Vec<CadreCfg>,
    chats: Vec<ChatCfg>,
    boites: Vec<BoiteCfg>,
    inputs: Vec<InputCfg>,
    /// Modules horloge (z `Z_MODULES`, `#modules-root`).
    horloges: Vec<HorlogeCfg>,
    /// Widgets goal (z du widget, cadre à l'échelle bulle).
    goals: Vec<GoalCfg>,
    /// Widgets speedrun (z du widget, cadre de scène).
    speedruns: Vec<SpeedrunCfg>,
    /// Widgets musique (z du widget, cadre de scène).
    musiques: Vec<MusiqueCfg>,
    /// Polygone de chaque cadre, **coordonnées canvas** — un seul polygone
    /// pour le trait et la découpe des médias.
    geometries: Vec<ID2D1PathGeometry>,
    /// Index du polygone de chaque boîte (`None` = pas de cadre → bordure).
    index_cadre_boite: Vec<Option<usize>>,
    /// Index du polygone du cadre opt-in de chaque module horloge
    /// (`None` = `m.cadre` absent → aucune découpe).
    index_cadre_horloge: Vec<Option<usize>>,
    /// Index du polygone du cadre à l'échelle bulle de chaque widget goal
    /// (`None` = pas de cadre → contenu rogné au rect seul).
    index_cadre_goal: Vec<Option<usize>>,
    /// Index du polygone du cadre de scène de chaque widget speedrun
    /// (`None` = pas de cadre → contenu rogné au rect seul).
    index_cadre_speedrun: Vec<Option<usize>>,
    /// Index du polygone du cadre de scène de chaque widget musique
    /// (`None` = pas de cadre → contenu rogné au rect seul).
    index_cadre_musique: Vec<Option<usize>>,
    /// Config des plans à perspective actifs — référence du diff de
    /// `mettre_a_jour` (`dessinateur.persps` garde les offscreens, privés).
    persps_cfg: Vec<PerspCfg>,
    /// Gabarit du cadre de la carte alerte (style de la scène, rect nul —
    /// mesuré à la taille de la carte par le bloc de rendu).
    cadre_overlay_alerte: Option<CadreCfg>,
}

/// Contenu dessiné par la voie de rendu (figé au démarrage de la sortie,
/// remplacé par `mettre_a_jour` à l'application à chaud).
struct ContenuRendu {
    textes: Vec<TexteCfg>,
    cadres: Vec<CadreCfg>,
    chats: Vec<ChatCfg>,
    boites: Vec<BoiteCfg>,
    inputs: Vec<InputCfg>,
    /// Modules horloge (z `Z_MODULES`, `#modules-root`).
    horloges: Vec<HorlogeCfg>,
    /// Widgets goal (z du widget, cadre à l'échelle bulle).
    goals: Vec<GoalCfg>,
    /// Widgets speedrun (z du widget, cadre de scène).
    speedruns: Vec<SpeedrunCfg>,
    /// Widgets musique (z du widget, cadre de scène).
    musiques: Vec<MusiqueCfg>,
    /// Plans à perspective — rasterisés en offscreen puis projetés.
    persps: Vec<PerspCfg>,
    /// Gabarit du cadre de la carte alerte (style de la scène, rect nul).
    cadre_overlay_alerte: Option<CadreCfg>,
}

impl VoieRendu {
    fn nouveau(
        contexte: &ContexteGpu,
        largeur: u32,
        hauteur: u32,
        fps: u32,
        contenu: ContenuRendu,
    ) -> Result<Self, String> {
        let ContenuRendu {
            textes,
            cadres,
            chats,
            boites,
            inputs,
            horloges,
            goals,
            speedruns,
            musiques,
            persps,
            cadre_overlay_alerte,
        } = contenu;
        let desc = DescTexture::new(Format::Bgra8, largeur, hauteur)
            .map_err(|e| format!("canvas : {e}"))?;
        let canvas = contexte
            .creer_texture(&desc)
            .map_err(|e| format!("canvas : {e}"))?;
        let convertisseur = ConvertisseurNv12::nouveau(contexte, largeur, hauteur, fps)
            .map_err(|e| e.to_string())?;
        let nv12 = convertisseur.creer_cible().map_err(|e| e.to_string())?;
        let mut dessinateur =
            DessinateurTexte::nouveau(contexte, &canvas).map_err(|e| e.to_string())?;
        dessinateur
            .configurer_persps(contexte, largeur, hauteur, &persps)
            .map_err(|e| e.to_string())?;
        let mut geometries = Vec::with_capacity(cadres.len());
        for cadre in &cadres {
            geometries.push(
                dessinateur
                    .geometrie_cadre(cadre)
                    .map_err(|e| e.to_string())?,
            );
        }
        let index_cadre_boite = boites
            .iter()
            .map(|boite| {
                boite
                    .cadre
                    .as_ref()
                    .and_then(|cadre| cadres.iter().position(|c| c == cadre))
            })
            .collect();
        let index_cadre_horloge = horloges
            .iter()
            .map(|h| {
                h.cadre
                    .as_ref()
                    .and_then(|cadre| cadres.iter().position(|c| c == cadre))
            })
            .collect();
        let index_cadre_goal = goals
            .iter()
            .map(|g| {
                g.cadre
                    .as_ref()
                    .and_then(|cadre| cadres.iter().position(|c| c == cadre))
            })
            .collect();
        let index_cadre_speedrun = speedruns
            .iter()
            .map(|s| {
                s.cadre
                    .as_ref()
                    .and_then(|cadre| cadres.iter().position(|c| c == cadre))
            })
            .collect();
        let index_cadre_musique = musiques
            .iter()
            .map(|m| {
                m.cadre
                    .as_ref()
                    .and_then(|cadre| cadres.iter().position(|c| c == cadre))
            })
            .collect();
        Ok(Self {
            canvas,
            nv12,
            convertisseur,
            dessinateur,
            textes,
            cadres,
            chats,
            boites,
            inputs,
            horloges,
            goals,
            speedruns,
            musiques,
            geometries,
            index_cadre_boite,
            index_cadre_horloge,
            index_cadre_goal,
            index_cadre_speedrun,
            index_cadre_musique,
            persps_cfg: persps,
            cadre_overlay_alerte,
        })
    }

    /// Met à jour le contenu dessiné (application à chaud) : géométries
    /// des cadres et plans à perspective ne sont reconstruits **que s'ils
    /// ont changé** — l'idempotence du contrat §3.1 tient à ce diff. Les
    /// vecs de calques sont toujours remplacés (coût nul).
    fn mettre_a_jour(
        &mut self,
        contexte: &ContexteGpu,
        largeur: u32,
        hauteur: u32,
        contenu: ContenuRendu,
    ) -> Result<(), String> {
        let ContenuRendu {
            textes,
            cadres,
            chats,
            boites,
            inputs,
            horloges,
            goals,
            speedruns,
            musiques,
            persps,
            cadre_overlay_alerte,
        } = contenu;
        if persps != self.persps_cfg {
            self.dessinateur
                .configurer_persps(contexte, largeur, hauteur, &persps)
                .map_err(|e| e.to_string())?;
            self.persps_cfg = persps;
        }
        if cadres != self.cadres {
            let mut geometries = Vec::with_capacity(cadres.len());
            for cadre in &cadres {
                geometries.push(
                    self.dessinateur
                        .geometrie_cadre(cadre)
                        .map_err(|e| e.to_string())?,
                );
            }
            self.geometries = geometries;
            self.cadres = cadres;
        }
        self.index_cadre_boite = boites
            .iter()
            .map(|boite| {
                boite
                    .cadre
                    .as_ref()
                    .and_then(|cadre| self.cadres.iter().position(|c| c == cadre))
            })
            .collect();
        self.index_cadre_horloge = horloges
            .iter()
            .map(|h| {
                h.cadre
                    .as_ref()
                    .and_then(|cadre| self.cadres.iter().position(|c| c == cadre))
            })
            .collect();
        self.index_cadre_goal = goals
            .iter()
            .map(|g| {
                g.cadre
                    .as_ref()
                    .and_then(|cadre| self.cadres.iter().position(|c| c == cadre))
            })
            .collect();
        self.index_cadre_speedrun = speedruns
            .iter()
            .map(|s| {
                s.cadre
                    .as_ref()
                    .and_then(|cadre| self.cadres.iter().position(|c| c == cadre))
            })
            .collect();
        self.index_cadre_musique = musiques
            .iter()
            .map(|m| {
                m.cadre
                    .as_ref()
                    .and_then(|cadre| self.cadres.iter().position(|c| c == cadre))
            })
            .collect();
        self.textes = textes;
        self.chats = chats;
        self.boites = boites;
        self.inputs = inputs;
        self.horloges = horloges;
        self.goals = goals;
        self.speedruns = speedruns;
        self.musiques = musiques;
        self.cadre_overlay_alerte = cadre_overlay_alerte;
        Ok(())
    }

    fn debut(&mut self) {
        self.dessinateur.debut();
    }

    fn fin(&mut self) -> Result<(), String> {
        self.dessinateur.fin().map_err(|e| e.to_string())
    }

    /// Efface le canvas en noir, une fois par frame (base de composition).
    fn effacer(&mut self) {
        self.dessinateur.effacer([0, 0, 0, 255]);
    }

    fn dessiner_textes_passe(&mut self) -> Result<(), String> {
        self.dessinateur
            .dessiner_textes(&self.textes)
            .map_err(|e| e.to_string())
    }

    /// Widget chat — `etat` alimente la barre viewers en tête
    /// (`.chat-viewers`), comme le widget goal.
    fn dessiner_chat(
        &mut self,
        index: usize,
        file: &FileChat,
        etat: &EtatGoals,
    ) -> Result<(), String> {
        self.dessinateur
            .dessiner_chat(std::slice::from_ref(&self.chats[index]), file, etat)
            .map_err(|e| e.to_string())
    }

    fn dessiner_cadre(&mut self, index: usize) -> Result<(), String> {
        self.dessinateur
            .dessiner_cadre(&self.cadres[index], &self.geometries[index])
            .map_err(|e| e.to_string())
    }

    /// Module horloge (`#modules-root`, z `Z_MODULES`) — voile + contenu,
    /// clippés au polygone de son cadre opt-in (`m.cadre`) ; le trait est
    /// tracé par le calque cadre au même z.
    fn dessiner_horloge(&mut self, index: usize) -> Result<(), String> {
        let clip = self.index_cadre_horloge[index].map(|i| &self.geometries[i]);
        self.dessinateur
            .dessiner_horloge(&self.horloges[index], clip)
            .map_err(|e| e.to_string())
    }

    /// Widget goal — contenu (entête + chiffres + barre + célébration)
    /// clippé au polygone de son cadre à l'échelle bulle ; le voile et le
    /// trait sont les calques Boite/Cadre au même z.
    fn dessiner_goal(&mut self, index: usize, etat: &EtatGoals) -> Result<(), String> {
        let clip = self.index_cadre_goal[index].map(|i| &self.geometries[i]);
        self.dessinateur
            .dessiner_goal(&self.goals[index], etat, clip)
            .map_err(|e| e.to_string())
    }

    /// Widget speedrun — splitter ASL (en-tête + timer + phase + barre +
    /// lignes de splits) clippé au polygone de son cadre de scène ; le
    /// voile et le trait sont les calques Boite/Cadre au même z.
    fn dessiner_speedrun(&mut self, index: usize, etat: &EtatSpeedrun) -> Result<(), String> {
        let clip = self.index_cadre_speedrun[index].map(|i| &self.geometries[i]);
        self.dessinateur
            .dessiner_speedrun(&self.speedruns[index], etat, clip)
            .map_err(|e| e.to_string())
    }

    /// Widget musique — lecteur Winamp (barre titre, LCD défilant, EQ,
    /// progression) clippé au polygone de son cadre de scène ; le voile et
    /// le trait sont les calques Boite/Cadre au même z.
    fn dessiner_musique(&mut self, index: usize, etat: &EtatMusique) -> Result<(), String> {
        let clip = self.index_cadre_musique[index].map(|i| &self.geometries[i]);
        self.dessinateur
            .dessiner_musique(&self.musiques[index], etat, clip)
            .map_err(|e| e.to_string())
    }

    /// Overlay alerte (`.alerte-root`, z `Z_ALERTES`) — carte transitoire
    /// alimentée par `chat_tx`, jamais par la scène ; le gabarit du cadre
    /// de scène est appliqué à la taille de la carte
    /// (`appliquerCadreOverlay`). `canvas_h` sert au `max-height:60vh` du
    /// média. Aucune alerte en cours = no-op.
    fn dessiner_alerte(&mut self, etat: &EtatAlertes, canvas_h: u32) -> Result<(), String> {
        self.dessinateur
            .dessiner_alerte(etat, self.cadre_overlay_alerte.as_ref(), canvas_h)
            .map_err(|e| e.to_string())
    }

    /// Widget input-viewer (clavier/numpad/souris/manette) avec l'état courant.
    fn dessiner_input(
        &mut self,
        index: usize,
        file: &FileInput,
        cadre: Option<usize>,
    ) -> Result<(), String> {
        let clip = cadre.map(|i| &self.geometries[i]);
        self.dessinateur
            .dessiner_input(&self.inputs[index], file, clip)
            .map_err(|e| e.to_string())
    }

    /// Voile d'un widget (`.widget`), clippé à son cadre s'il en a un.
    fn dessiner_boite(&mut self, index: usize) -> Result<(), String> {
        let rect = rect_boite(self.boites[index].rect);
        let style = self.boites[index].style;
        let clip = self.index_cadre_boite[index].map(|i| &self.geometries[i]);
        self.dessinateur
            .dessiner_fond(style, rect, clip)
            .map_err(|e| e.to_string())
    }

    /// Bordure 1 px d'un widget — sans effet si un cadre est actif.
    fn dessiner_bordure_boite(&mut self, index: usize) -> Result<(), String> {
        if self.boites[index].cadre.is_some() {
            return Ok(());
        }
        let rect = rect_boite(self.boites[index].rect);
        let style = self.boites[index].style;
        self.dessinateur
            .dessiner_bordure(style, rect)
            .map_err(|e| e.to_string())
    }

    fn dessiner_media(&mut self, bitmap: &ID2D1Bitmap1, t: &TraceCouche) -> Result<(), String> {
        // `clip_geo` (forme dédiée caméra, hors `geometries`) prime sur le
        // polygone du cadre — une caméra n'a jamais de cadre, mais la
        // priorité est explicitée pour toute future combinaison.
        let clip = t
            .clip_geo
            .or_else(|| t.cadre.map(|index| &self.geometries[index]));
        let trace = crate::texte::TraceMedia {
            destination: t.destination,
            source: t.source,
            boite: t.boite,
            transforme: t.transforme,
            clip,
            effets: t.effets,
            opacite: t.opacite,
        };
        self.dessinateur
            .dessiner_media(bitmap, &trace)
            .map_err(|e| e.to_string())
    }

    /// Média morphé (bulge/pinch, lot T4) : offscreen taille boîte +
    /// warp `MORPH_FRAG` — détail dans
    /// `DessinateurTexte::dessiner_media_morphe`.
    fn dessiner_media_morphe(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        t: &MorpheCouche,
    ) -> Result<(), String> {
        let trace = crate::texte::TraceMorphe {
            taille: t.taille,
            boite: t.boite,
            fit: t.fit,
            transform: t.transform,
            clip: t.cadre.map(|index| &self.geometries[index]),
            effets: t.effets,
            morphs: t.morphs,
            pixel_morph: t.pixel_morph,
        };
        self.dessinateur
            .dessiner_media_morphe(bitmap, &trace)
            .map_err(|e| e.to_string())
    }

    /// Média pixelisé (`bgPixel`) : rendu en buffer réduit + upscale sans
    /// lissage — détail dans `DessinateurTexte::dessiner_media_pixel`.
    fn dessiner_media_pixel(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        t: &PixelCouche,
    ) -> Result<(), String> {
        let trace = crate::texte::TracePixel {
            taille: t.taille,
            boite: t.boite,
            fit: t.fit,
            transform: t.transform,
            clip: t.cadre.map(|index| &self.geometries[index]),
            effets: t.effets,
            opacite: t.opacite,
        };
        self.dessinateur
            .dessiner_media_pixel(bitmap, &trace)
            .map_err(|e| e.to_string())
    }

    /// Vide les offscreens de perspective — début de frame.
    fn effacer_persps(&mut self) {
        self.dessinateur.effacer_persps();
    }

    /// Plan à perspective couvrant (`rect`,`z`), `None` = cible canvas.
    fn groupe_de(&self, rect: (f64, f64, f64, f64), z: i32) -> Option<usize> {
        self.dessinateur.groupe_de(rect, z)
    }

    /// Redirige la cible : offscreen du plan ou canvas.
    fn cible_pour(&mut self, groupe: Option<usize>) {
        self.dessinateur.cible_pour(groupe);
    }

    /// Projette les plans dépassés (`g.z < z`) qui ont du contenu.
    fn flusher_persps(&mut self, z: i32) -> Result<(), String> {
        self.dessinateur
            .flusher_persps(z)
            .map_err(|e| e.to_string())
    }

    /// Projette tous les plans restants — fin du plan de composition.
    fn flusher_persps_fin(&mut self) -> Result<(), String> {
        self.dessinateur
            .flusher_persps_fin()
            .map_err(|e| e.to_string())
    }

    fn bitmap_media(
        &mut self,
        texture: &ID3D11Texture2D,
        alpha: bool,
    ) -> Result<ID2D1Bitmap1, String> {
        self.dessinateur
            .bitmap_media(texture, alpha)
            .map_err(|e| e.to_string())
    }

    /// Géométrie du **cercle inscrit** du rect (coordonnées canvas) — forme
    /// dédiée `FormeCamera::Cercle`. Même polygone que `pathCercle`
    /// (`cadres::points_cercle`) : une seule source de vérité pour le trou
    /// diffusion, le clip DOM et le masque OBS — contrat 9 d'AGENTS.md.
    fn geometrie_cercle(&self, rect: (f64, f64, f64, f64)) -> Result<ID2D1PathGeometry, String> {
        let (x, y, w, h) = rect;
        let points: Vec<(f64, f64)> = crate::cadres::points_cercle(w, h)
            .into_iter()
            .map(|(px, py)| (px + x, py + y))
            .collect();
        self.dessinateur
            .geometrie_points(&points)
            .map_err(|e| e.to_string())
    }

    fn convertir(&mut self) -> Result<(), String> {
        self.convertisseur
            .convertir_vers(&self.canvas, &self.nv12)
            .map_err(|e| e.to_string())
    }
}

/// Rect canvas d'une boîte flottante (`left`/`top` + `width`/`height` du CSS).
fn rect_boite(rect: (f64, f64, f64, f64)) -> RECT {
    RECT {
        left: rect.0 as i32,
        top: rect.1 as i32,
        right: (rect.0 + rect.2) as i32,
        bottom: (rect.1 + rect.3) as i32,
    }
}

/// Paramètres de tracé d'un calque média/capture — regroupement clippy
/// (`too_many_arguments`, convention §37.4.5). `cadre` = **index** dans
/// `VoieRendu::geometries` ; la résolution vers le polygone est faite par
/// le wrapper (`TraceMedia::clip`).
struct TraceCouche<'a> {
    source: RECT,
    destination: RECT,
    boite: RECT,
    transforme: Option<Matrix3x2>,
    cadre: Option<usize>,
    /// Clip géométrique direct (cercle caméra) — prime sur `cadre`, qui
    /// n'indexe que les polygones de `VoieRendu::geometries`.
    clip_geo: Option<&'a ID2D1PathGeometry>,
    effets: &'a crate::media::EffetsCfg,
    /// Opacité du calque (fondu du carrousel — 1,0 hors fondu/capture).
    opacite: f32,
}

/// Paramètres d'un média morphé (bulge/pinch, lot T4) — idem
/// `TraceCouche` + points de morph et pixelisation en chemin morph.
struct MorpheCouche<'a> {
    taille: (u32, u32),
    boite: (f64, f64, f64, f64),
    fit: crate::media::Fit,
    transform: &'a crate::media::Transform2D,
    cadre: Option<usize>,
    effets: &'a crate::media::EffetsCfg,
    morphs: &'a [sos_scene::MorphPoint],
    pixel_morph: f64,
}

/// Paramètres d'un média pixelisé (`bgPixel`) — idem `TraceCouche`.
struct PixelCouche<'a> {
    taille: (u32, u32),
    boite: (f64, f64, f64, f64),
    fit: crate::media::Fit,
    transform: &'a crate::media::Transform2D,
    cadre: Option<usize>,
    effets: &'a crate::media::EffetsCfg,
    /// Opacité du calque (fondu du carrousel de fond).
    opacite: f32,
}

/// Nature d'une entrée du plan de composition.
///
/// L'ordre de déclaration **est** l'ordre à `z` égal : le voile passe sous le
/// contenu, la bordure au-dessus, puis le cadre (miroir de `.widget` puis
/// `.cadre-overlay`, `z-index: 5`). Le module horloge est un contenu : il
/// passe avant sa décoration éventuelle (le calque `Cadre` de `m.cadre`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum GenreCalque {
    Boite,
    Media,
    Capture,
    Chat,
    Input,
    Horloge,
    Goal,
    Speedrun,
    Musique,
    Alerte,
    Bordure,
    Cadre,
}

/// Ouvre le prochain MFT disponible, en avançant `index` (repli monotone).
#[allow(clippy::too_many_arguments)]
fn ouvrir_mft(
    disponibles: &[EncodeurDispo],
    index: &mut usize,
    cfg: &SortieCfg,
    cw: u32,
    ch: u32,
    fps: u32,
    contexte: &ContexteGpu,
    preset: Preset,
) -> Option<(EncodeurMft, EncodeurInfo)> {
    while *index < disponibles.len() {
        let dispo = &disponibles[*index];
        *index += 1;
        if let Ok(encodeur) = ouvrir_encodeur(dispo, cfg, cw, ch, fps, contexte, preset) {
            return Some((
                encodeur,
                EncodeurInfo {
                    nom: dispo.nom.clone(),
                    materiel: dispo.materiel,
                    zero_copie: false,
                },
            ));
        }
    }
    None
}

/// Encode une frame avec l'encodeur actif.
fn encoder_actif(
    encodeur: &mut EncodeurActif,
    zero_copie: &mut bool,
    convertisseur: Option<&ConvertisseurNv12>,
    sortie: &ID3D11Texture2D,
    pts_us: i64,
) -> Result<Vec<PaquetEncode>, ErreurEncodage> {
    match encodeur {
        EncodeurActif::Nvenc(nvenc) => nvenc.encoder_texture(sortie, pts_us),
        EncodeurActif::Mft(mft) => {
            encoder_frame(mft, zero_copie, convertisseur, sortie, pts_us)
        }
    }
}

/// Ouvre un encodeur MFT pour une sortie donnée.
fn ouvrir_encodeur(
    dispo: &EncodeurDispo,
    cfg: &SortieCfg,
    cw: u32,
    ch: u32,
    fps: u32,
    contexte: &ContexteGpu,
    preset: Preset,
) -> Result<EncodeurMft, ErreurEncodage> {
    EncodeurMft::nouveau_avec_preset(
        dispo.activate(),
        cfg.codec,
        cw,
        ch,
        fps,
        cfg.debit_kbps,
        Some(contexte),
        preset,
    )
}

/// Encode une frame : surface GPU (zéro-copie) ou copie NV12.
///
/// Un refus de la surface bascule **durablement** en copie (`zero_copie`) :
/// on ne retente pas le chemin GPU à chaque frame.
fn encoder_frame(
    encodeur: &mut EncodeurMft,
    zero_copie: &mut bool,
    convertisseur: Option<&ConvertisseurNv12>,
    sortie: &ID3D11Texture2D,
    pts_us: i64,
) -> Result<Vec<PaquetEncode>, ErreurEncodage> {
    if *zero_copie {
        match encodeur.encoder_texture(sortie, pts_us) {
            Ok(paquets) => Ok(paquets),
            Err(e) => {
                let conv = convertisseur.ok_or(e)?;
                let nv12 = conv.lire_texture_nv12(sortie)?;
                *zero_copie = false;
                encodeur.encoder(&nv12, pts_us)
            }
        }
    } else {
        let conv = convertisseur.ok_or_else(|| {
            ErreurEncodage::Gpu("convertisseur NV12 absent".to_string())
        })?;
        let nv12 = conv.lire_texture_nv12(sortie)?;
        encodeur.encoder(&nv12, pts_us)
    }
}

/// Aperçu 480 px de large, ratio conservé, dimensions paires.
fn taille_apercu(cw: u32, ch: u32) -> (u32, u32) {
    let largeur = cw.clamp(2, 480) & !1;
    let hauteur = ((ch as u64 * largeur as u64 / cw.max(1) as u64) as u32).max(2) & !1;
    (largeur, hauteur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sos_moteur_api::Conteneur;

    fn cfg_fichier() -> SortieCfg {
        SortieCfg::fichier("C:/tmp/moteur-natif.mp4", Conteneur::Mp4)
    }

    fn source() -> SourceCfg {
        SourceCfg {
            nom: "moniteur 0".to_string(),
            cible: CibleCapture::Moniteur { index: 0 },
            rect: (0.0, 0.0, 1920.0, 1080.0),
            z: 0,
            cadre: None,
            forme_camera: None,
        }
    }

    #[test]
    fn rect_boite_depuis_rect_flottant() {
        let r = rect_boite((334.368, 669.365, 692.548, 88.625));
        assert_eq!((r.left, r.top, r.right, r.bottom), (334, 669, 1026, 757));
        let r = rect_boite((454.9, -83.04, 452.3, 257.2));
        assert_eq!((r.left, r.top, r.right, r.bottom), (454, -83, 907, 174));
    }

    #[test]
    fn apercu_pair_et_ratio() {
        let (w, h) = taille_apercu(1920, 1080);
        assert_eq!((w, h), (480, 270));
        let (w, h) = taille_apercu(1360, 768);
        assert_eq!(w % 2, 0);
        assert_eq!(h % 2, 0);
        assert_eq!((w, h), (480, 270));
    }

    #[test]
    fn source_valide_refuse_rect_nul() {
        assert!(source().valider().is_ok());
        let mut s = source();
        s.rect.2 = 0.0;
        assert!(matches!(s.valider(), Err(ErreurMoteur::Configuration(_))));
        let mut s = source();
        s.nom = "  ".to_string();
        assert!(matches!(s.valider(), Err(ErreurMoteur::Configuration(_))));
    }

    #[test]
    fn encodeur_inconnu_avant_demarrage() {
        assert!(MoteurNatif::nouveau().encodeur().is_none());
    }

    #[test]
    fn etat_initial_et_arret_idempotent() {
        let moteur = MoteurNatif::nouveau();
        assert_eq!(moteur.etat(), MoteurEtat::Inactif);
        assert!(!moteur.en_cours());
        moteur.arreter();
        moteur.arreter();
        assert_eq!(moteur.etat(), MoteurEtat::Inactif);
        assert!(moteur.apercu().is_none());
    }

    #[test]
    fn apercu_arret_idempotent_et_erreur_vide() {
        let moteur = MoteurNatif::nouveau();
        assert!(!moteur.apercu_en_cours());
        assert!(moteur.apercu_erreur().is_none());
        moteur.arreter_apercu();
        moteur.arreter_apercu();
        assert!(!moteur.apercu_en_cours());
        assert!(moteur.apercu_erreur().is_none());
    }

    #[test]
    fn apercu_refuse_canvas_nul() {
        let moteur = MoteurNatif::nouveau();
        assert!(matches!(
            moteur.demarrer_apercu((0, 1080), 30),
            Err(ErreurMoteur::Configuration(_))
        ));
        assert!(matches!(
            moteur.demarrer_apercu((1920, 1080), 0),
            Err(ErreurMoteur::Configuration(_))
        ));
        // Un démarrage refusé ne laisse ni fil ni erreur derrière.
        assert!(!moteur.apercu_en_cours());
        assert!(moteur.apercu_erreur().is_none());
    }

    #[test]
    fn demarrer_sans_source_refuse() {
        let moteur = MoteurNatif::nouveau();
        assert!(matches!(
            moteur.demarrer_sortie(cfg_fichier()),
            Err(ErreurMoteur::Configuration(_))
        ));
    }

    #[test]
    fn rtmp_srt_acceptes_mais_schema_valide() {
        // Sink réseau accepté : avec une URL valide, le refus vient de la
        // scène vide (aucune source) — preuve que la validation des sinks
        // est passée (le rejet `Indisponible` aurait primé avant ce lot).
        let moteur = MoteurNatif::nouveau();
        let mut cfg = cfg_fichier();
        cfg.sinks = vec![SinkCfg::Rtmp {
            url: "rtmp://live.example/app".to_string(),
        }];
        match moteur.demarrer_sortie(cfg) {
            Err(ErreurMoteur::Configuration(m)) => assert!(m.contains("aucune source"), "{m}"),
            autre => panic!("attendu Configuration(aucune source), reçu {autre:?}"),
        }

        let moteur = MoteurNatif::nouveau();
        let mut cfg = cfg_fichier();
        cfg.sinks = vec![SinkCfg::Srt {
            url: "srt://live.example:9000".to_string(),
        }];
        match moteur.demarrer_sortie(cfg) {
            Err(ErreurMoteur::Configuration(m)) => assert!(m.contains("aucune source"), "{m}"),
            autre => panic!("attendu Configuration(aucune source), reçu {autre:?}"),
        }

        // Schémas inattendus : refusés avant tout contact GPU.
        let moteur = MoteurNatif::nouveau();
        let mut cfg = cfg_fichier();
        cfg.sinks = vec![SinkCfg::Rtmp {
            url: "http://live.example/app".to_string(),
        }];
        assert!(matches!(
            moteur.demarrer_sortie(cfg),
            Err(ErreurMoteur::Configuration(_))
        ));
        let moteur = MoteurNatif::nouveau();
        let mut cfg = cfg_fichier();
        cfg.sinks = vec![SinkCfg::Srt {
            url: "udp://live.example:9000".to_string(),
        }];
        assert!(matches!(
            moteur.demarrer_sortie(cfg),
            Err(ErreurMoteur::Configuration(_))
        ));
    }

    #[test]
    fn urls_live_assemblees_avec_la_cle() {
        // RTMP : la clé s'accolée au chemin de l'application.
        assert_eq!(
            url_rtmp("rtmp://live/app", Some("cle-de-flux")),
            "rtmp://live/app/cle-de-flux"
        );
        assert_eq!(
            url_rtmp("rtmp://live/app/", Some("k")),
            "rtmp://live/app/k"
        );
        assert_eq!(url_rtmp("rtmp://live/app", None), "rtmp://live/app");
        assert_eq!(url_rtmp("rtmp://live/app", Some("")), "rtmp://live/app");

        // SRT : la clé devient `streamid`, quel que soit le query existant.
        assert_eq!(
            url_srt("srt://h:9000", Some("cle-de-flux")),
            "srt://h:9000?streamid=cle-de-flux"
        );
        assert_eq!(
            url_srt("srt://h:9000?latency=200", Some("k")),
            "srt://h:9000?latency=200&streamid=k"
        );
        assert_eq!(url_srt("srt://h:9000", None), "srt://h:9000");
    }

    #[test]
    fn sources_refusees_pendant_un_enregistrement() {
        // Aucun GPU touché : le refus vient de l'état, pas de la capture.
        let moteur = MoteurNatif::nouveau();
        *moteur.etat.lock().unwrap() = EtatInterne::Actif;
        *moteur.fil.lock().unwrap() = Some(std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(50));
        }));
        assert!(matches!(
            moteur.configurer_sources(vec![source()]),
            Err(ErreurMoteur::DejaActif(_))
        ));
        moteur.arreter();
    }

    #[test]
    fn capacites_du_lot() {
        let c = MoteurNatif::nouveau().capacites();
        assert!(c.capture_ecran && c.capture_fenetre);
        assert!(c.camera && c.srt);
        assert!(!c.fond_vert);
    }

    #[test]
    fn configurer_scene_remplit_cadres_et_chats() {
        let scene: sos_scene::Scene = serde_json::from_str(
            r##"{
                "canvasW": 1360, "canvasH": 768,
                "cadreWidget": {"style":"mgstyle","strokeWidth":20,"couleur":"#000000",
                                "couleurFin":"#8c00ff","actif":true,"bordActif":true},
                "widgets": [
                    {"id":"c","type":"chat","x":334,"y":669,"largeur":692,"hauteur":88,"z":14,
                     "chatFiltre":"unifie","taillePolice":11,"chatStyle":"bulles"}
                ]
            }"##,
        )
        .expect("scène");
        let moteur = MoteurNatif::nouveau();
        moteur
            .configurer_scene(&scene, &std::collections::HashMap::new(), std::path::Path::new("."))
            .expect("scène acceptée");
        // Bord du canvas seul : le chat en bulles n'a pas de cadre de widget.
        assert_eq!(moteur.cadres().len(), 1);
        assert_eq!(moteur.cadres()[0].z, i32::MAX);
        assert_eq!(moteur.chats().len(), 1);
        assert!(moteur.chats()[0].bulles);
        // Le chat garde son voile et sa bordure (pas de cadre, pas `sansCadre`).
        assert_eq!(moteur.boites().len(), 1);
        assert!(moteur.boites()[0].cadre.is_none());
        assert_eq!(moteur.boites()[0].rect, (334.0, 669.0, 692.0, 88.0));
    }

    #[test]
    fn style_de_cadre_non_porte_est_signale() {
        let scene: sos_scene::Scene = serde_json::from_str(
            r##"{"cadreWidget":{"style":"cyberpunk","strokeWidth":20,"actif":true},
                 "widgets":[{"id":"a","type":"media","x":0,"y":0,"largeur":10,"hauteur":10,"z":0}]}"##,
        )
        .expect("scène");
        let moteur = MoteurNatif::nouveau();
        moteur
            .configurer_scene(&scene, &std::collections::HashMap::new(), std::path::Path::new("."))
            .expect("scène acceptée");
        assert!(moteur.cadres().is_empty());
        assert!(
            moteur
                .instantane()
                .raison_degradation
                .is_some_and(|r| r.contains("cyberpunk")),
            "un style non porté doit être signalé, jamais silencieux"
        );
    }

    #[test]
    fn chat_pousse_meme_pendant_un_enregistrement_et_reste_borne() {
        use crate::chat::{MessageChat, MAX_CHAT};
        let moteur = MoteurNatif::nouveau();
        *moteur.etat.lock().unwrap() = EtatInterne::Actif;
        for i in 0..MAX_CHAT + 5 {
            moteur.pousser_chat(MessageChat::nouveau(
                "twitch",
                "viewer",
                format!("m{i}"),
                None,
            ));
        }
        // Aucun `DejaActif` : le chat est le canal des messages en direct.
        assert_eq!(moteur.chat_len(), MAX_CHAT);
    }

    #[test]
    fn appliquer_scene_hors_session_met_a_jour_et_est_idempotente() {
        let scene: sos_scene::Scene = serde_json::from_str(
            r#"{"canvasW":1360,"canvasH":768,
                "widgets":[{"id":"c","type":"chat","x":1,"y":2,"largeur":100,"hauteur":80,"z":3}]}"#,
        )
        .expect("scène");
        let moteur = MoteurNatif::nouveau();
        moteur.appliquer_scene(&scene);
        assert_eq!(moteur.chats().len(), 1);
        // Idempotente : une seconde application identique, aucun effet.
        moteur.appliquer_scene(&scene);
        assert_eq!(moteur.chats().len(), 1);
        assert_eq!(moteur.chats()[0].rect, (1.0, 2.0, 100.0, 80.0));
        assert_eq!(moteur.chats()[0].z, 3);
        // Le courrier attend la prochaine session.
        assert!(moteur.scene_maj.lock().unwrap().is_some());
    }

    #[test]
    fn appliquer_scene_pendant_un_enregistrement_n_est_pas_refusee() {
        // Jamais `DejaActif` : c'est le canal de mise à jour à chaud —
        // les mutexes sont écrits, la boucle consomme le courrier à la
        // frame suivante.
        let moteur = MoteurNatif::nouveau();
        *moteur.etat.lock().unwrap() = EtatInterne::Actif;
        *moteur.fil.lock().unwrap() = Some(std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(50));
        }));
        let scene: sos_scene::Scene = serde_json::from_str(
            r#"{"widgets":[{"id":"c","type":"chat","x":0,"y":0,"largeur":10,"hauteur":10,"z":1}]}"#,
        )
        .expect("scène");
        moteur.appliquer_scene(&scene);
        assert_eq!(moteur.chats().len(), 1);
        moteur.arreter();
    }

    #[test]
    fn configurer_scene_et_appliquer_remplissent_les_horloges() {
        let scene: sos_scene::Scene = serde_json::from_str(
            r##"{"canvasW":1360,"canvasH":768,
                 "cadreWidget":{"style":"mgstyle","strokeWidth":20,"actif":true,"bordActif":false},
                 "widgets":[],
                 "modules":{"horloge":{"actif":true,"x":10,"y":20,"largeur":300,"hauteur":60,
                            "cadre":true,"secondes":true}}}"##,
        )
        .expect("scène");
        let moteur = MoteurNatif::nouveau();
        moteur
            .configurer_scene(&scene, &std::collections::HashMap::new(), std::path::Path::new("."))
            .expect("scène acceptée");
        assert_eq!(moteur.horloges().len(), 1);
        assert_eq!(moteur.horloges()[0].z, crate::texte::Z_MODULES);
        assert!(moteur.horloges()[0].secondes);
        // Le cadre opt-in du module est porté par `cadres` (trait + clip).
        assert_eq!(moteur.cadres().len(), 1);
        assert_eq!(moteur.cadres()[0].z, crate::texte::Z_MODULES);
        // Application à chaud : retrait du module → vecs vidés.
        let scene_sans: sos_scene::Scene =
            serde_json::from_str(r#"{"canvasW":1360,"canvasH":768,"widgets":[]}"#)
                .expect("scène");
        moteur.appliquer_scene(&scene_sans);
        assert!(moteur.horloges().is_empty());
        assert!(moteur.cadres().is_empty());
    }

    #[test]
    fn appliquer_scene_avec_source_invalide_est_annulee_et_signalee() {
        let scene: sos_scene::Scene = serde_json::from_str(
            r#"{"widgets":[{"id":"","type":"media","x":0,"y":0,"largeur":100,"hauteur":100,"z":0,
                "captureCible":{"kind":"moniteur","name":"\\\\.\\DISPLAY1","index":0}}]}"#,
        )
        .expect("scène");
        let moteur = MoteurNatif::nouveau();
        moteur.appliquer_scene(&scene);
        assert!(moteur.sources().is_empty(), "application annulée");
        assert!(
            moteur
                .instantane()
                .raison_degradation
                .is_some_and(|r| r.contains("source sans nom")),
            "le refus doit être signalé, jamais silencieux"
        );
    }
}
