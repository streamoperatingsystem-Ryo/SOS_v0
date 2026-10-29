//! Orchestrateur du moteur natif (lot 1.6).
//!
//! Assemble les briques validées : WGC (N sources) → canvas **BGRA composé
//! par Direct2D** (alpha réel, cadres, voiles, chat) → conversion NV12 →
//! encodeur (NVENC d'abord, repli MFT) → sinks → aperçu, cadencé par
//! l'horloge, surveillé par le gouverneur et les compteurs.
//!
//! Contrat : `docs/features/moteur-natif.md` §3.1. `MoteurNatif` implémente
//! `sos_moteur_api::Moteur` ; les sources viennent de `configurer_sources`
//! tant que `appliquer_scene` n'existe pas (lot 1.1 de l'API).
//!
//! Limites assumées de ce lot :
//! - sorties fichier seulement (MP4 fragmenté / MKV) ; RTMP/SRT plus tard ;
//! - pas de caméra (Media Foundation Source Reader) ;
//! - le gouverneur **constate** et trace sa décision, il ne reconfigure pas
//!   encore le compositeur/l'encodeur à chaud.

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
use crate::capture::enumeration::enumerer_moniteurs;
use crate::capture::wgc::CaptureWgc;
use crate::chat::{ChatCfg, FileChat, MessageChat};
use crate::clock::Horloge;
use crate::color_gpu::ConvertisseurNv12;
use crate::compteurs::{CompteursMoteur, Instantane};
use crate::encode::detect;
use crate::encode::mft::EncodeurMft;
use crate::encode::nvenc::EncodeurNvenc;
use crate::encode::{detect::EncodeurDispo, ErreurEncodage};
use crate::gouverneur::{Action, Gouverneur, Mesures};
use crate::gpu::{ContexteGpu, DescTexture, Format};
use crate::input::{EtatInput, FileInput, InputCfg};
use crate::media::{MediaCfg, PerspCfg};
use crate::output::ffmpeg::FfmpegSink;
use crate::preview::Apercu;
use crate::scene_sources::BoiteCfg;
use crate::texte::{DessinateurTexte, TexteCfg};

/// Ce que le moteur capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CibleCapture {
    Moniteur { index: u32 },
    Fenetre { hwnd: isize },
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
    pub fn configurer_scene(
        &self,
        scene: &sos_scene::Scene,
        cibles: &std::collections::HashMap<String, CibleCapture>,
        base: &std::path::Path,
    ) -> Result<(), ErreurMoteur> {
        if self.en_cours() {
            return Err(ErreurMoteur::DejaActif(SortieKind::Enregistrement));
        }
        let sources = crate::scene_sources::sources_depuis_scene(scene, cibles);
        let medias = crate::scene_sources::medias_depuis_scene(scene, base);
        let textes = crate::scene_sources::textes_depuis_scene(scene);
        let cadres = crate::scene_sources::cadres_depuis_scene(scene);
        let chats = crate::scene_sources::chat_depuis_scene(scene);
        let boites = crate::scene_sources::boites_depuis_scene(scene);
        let inputs = crate::scene_sources::input_depuis_scene(scene);
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
        for source in &sources {
            source.valider()?;
        }
        *self.sources.lock().unwrap_or_else(|e| e.into_inner()) = sources;
        *self.medias.lock().unwrap_or_else(|e| e.into_inner()) = medias;
        *self.textes.lock().unwrap_or_else(|e| e.into_inner()) = textes;
        *self.cadres.lock().unwrap_or_else(|e| e.into_inner()) = cadres;
        *self.chats.lock().unwrap_or_else(|e| e.into_inner()) = chats;
        *self.boites.lock().unwrap_or_else(|e| e.into_inner()) = boites;
        *self.inputs.lock().unwrap_or_else(|e| e.into_inner()) = inputs;
        *self.persps.lock().unwrap_or_else(|e| e.into_inner()) =
            crate::scene_sources::persps_depuis_scene(scene);
        Ok(())
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
            persps: self
                .persps
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            chat: Arc::clone(&self.chat),
            input: Arc::clone(&self.input),
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
            camera: false,
            fond_vert: false,
            masque_shader: false,
            cadres_animes: false,
            // Loopback + micro muxés en AAC dans le fichier (P3 audio).
            audio: true,
            audio_par_app: false,
            roi_encode: false,
            multi_format: false,
            hdr: false,
            srt: false,
            ndi: false,
        }
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
                SinkCfg::Rtmp { .. } | SinkCfg::Srt { .. } => {
                    return Err(ErreurMoteur::Indisponible(
                        "RTMP/SRT : lot ultérieur (muxeur portant les PTS)".to_string(),
                    ));
                }
            }
        }
        if !cfg
            .sinks
            .iter()
            .any(|s| matches!(s, SinkCfg::Fichier { .. }))
        {
            return Err(ErreurMoteur::Configuration(
                "aucune sortie fichier demandée".to_string(),
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
        {
            return Err(ErreurMoteur::Configuration(
                "aucune source, média, texte, chat ni input-viewer configuré".to_string(),
            ));
        }
        for source in &scene.sources {
            source.valider()?;
        }

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
    /// Plans à perspective (offscreen + effet par plan).
    persps: Vec<PerspCfg>,
    /// Buffer vivant : l'app y pousse pendant la session.
    chat: Arc<Mutex<FileChat>>,
    /// État vivant des touches : l'app y pousse pendant la session.
    input: Arc<Mutex<FileInput>>,
}

/// Une capture active + sa copie GPU persistante.
///
/// La texture WGC appartient au frame pool : elle est copiée immédiatement,
/// sinon la frame suivante l'invalide.
struct CaptureActive {
    session: CaptureWgc,
    cible: ID3D11Texture2D,
    taille: (u32, u32),
    rect: (f64, f64, f64, f64),
    z: i32,
    /// Cadre du widget de capture (découpe au polygone).
    cadre: Option<CadreCfg>,
    prete: bool,
}

/// Corps du fil moteur. Toute erreur remonte à l'état (`Erreur`) ; aucune
/// panique ne doit traverser le fil.
fn boucle(
    cfg: SortieCfg,
    scene: SceneChargee,
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
        persps,
        chat,
        input,
    } = scene;
    let (cw, ch) = cfg.canvas;
    let fps = cfg.fps;

    let moniteurs = enumerer_moniteurs().map_err(|e| format!("moniteurs : {e}"))?;
    let adaptateur = sources.iter().find_map(|s| match s.cible {
        CibleCapture::Moniteur { index } => moniteurs
            .iter()
            .find(|m| m.index == index)
            .map(|m| m.adaptateur_index),
        CibleCapture::Fenetre { .. } => None,
    });
    let contexte = Arc::new(
        ContexteGpu::creer(adaptateur).map_err(|e| format!("contexte GPU : {e}"))?,
    );

    let mut captures: Vec<CaptureActive> = Vec::with_capacity(sources.len());
    for source in &sources {
        let session = match source.cible {
            CibleCapture::Moniteur { index } => {
                let moniteur = moniteurs
                    .iter()
                    .find(|m| m.index == index)
                    .ok_or_else(|| format!("moniteur {index} introuvable"))?;
                CaptureWgc::demarrer_avec(moniteur, Arc::clone(&contexte))
                    .map_err(|e| format!("capture « {} » : {e}", source.nom))?
            }
            CibleCapture::Fenetre { hwnd } => {
                CaptureWgc::demarrer_fenetre_avec(hwnd, Arc::clone(&contexte))
                    .map_err(|e| format!("capture « {} » : {e}", source.nom))?
            }
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
        captures.push(CaptureActive {
            session,
            cible,
            taille,
            rect: source.rect,
            z: source.z,
            cadre: source.cadre.clone(),
            prete: false,
        });
    }

    let mut medias: Vec<crate::media::MediaChargee> = Vec::with_capacity(medias_cfg.len());
    for cfg_media in &medias_cfg {
        let chargee = crate::media::charger(&contexte, cfg_media)
            .map_err(|e| format!("média {} : {e}", cfg_media.chemin.display()))?;
        medias.push(chargee);
    }

    // Plan de composition : ordre z, puis voile → contenu → bordure → cadre.
    // Un widget est un empilement : `.widget` (fond) sous son contenu, la
    // bordure au-dessus, puis `.cadre-overlay` (z-index 5). La bordure n'existe
    // que sans cadre actif (`applyCadreWidget` met `border: none`).
    let mut plan: Vec<(i32, GenreCalque, usize)> = Vec::new();
    for (i, boite) in boites.iter().enumerate() {
        plan.push((boite.z, GenreCalque::Boite, i));
        if boite.cadre.is_none() {
            plan.push((boite.z, GenreCalque::Bordure, i));
        }
    }
    for (i, media) in medias.iter().enumerate() {
        plan.push((media.z, GenreCalque::Media, i));
    }
    for (i, capture) in captures.iter().enumerate() {
        plan.push((capture.z, GenreCalque::Capture, i));
    }
    for (i, cfg_chat) in chats.iter().enumerate() {
        plan.push((cfg_chat.z, GenreCalque::Chat, i));
    }
    for (i, cfg_input) in inputs.iter().enumerate() {
        plan.push((cfg_input.z, GenreCalque::Input, i));
    }
    for (i, cadre) in cadres.iter().enumerate() {
        plan.push((cadre.z, GenreCalque::Cadre, i));
    }
    plan.sort_by_key(|(z, genre, _)| (*z, *genre));

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
            persps,
        },
    )
    .map_err(|e| format!("rendu : {e}"))?;

    // Bitmaps Direct2D de **toutes** les couches + index de leur polygone.
    // Construits **une fois** : la vue Direct2D ne change pas (une vidéo met à
    // jour sa texture, pas son bitmap). Un carrousel a une bitmap **par
    // slide** (la pile `<img class="slide">` du DOM).
    let mut bitmaps_medias: Vec<Vec<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1>> =
        Vec::with_capacity(medias.len());
    let mut bitmaps_captures: Vec<Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1>> =
        vec![None; captures.len()];
    let mut index_cadre_media: Vec<Option<usize>> = vec![None; medias.len()];
    let mut index_cadre_capture: Vec<Option<usize>> = vec![None; captures.len()];
    let mut index_cadre_input: Vec<Option<usize>> = vec![None; voie.inputs.len()];
    {
        for (i, media) in medias.iter().enumerate() {
            index_cadre_media[i] = media
                .cadre
                .as_ref()
                .and_then(|cadre| voie.cadres.iter().position(|c| c == cadre));
            if media.cadre.is_some() && index_cadre_media[i].is_none() {
                // Jamais un rendu faux silencieux : la couche sort non découpée.
                compteurs.degrader("cadre de média introuvable : couche non découpée");
            }
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
            bitmaps_medias.push(bitmaps);
        }
        for (i, capture) in captures.iter().enumerate() {
            index_cadre_capture[i] = capture
                .cadre
                .as_ref()
                .and_then(|cadre| voie.cadres.iter().position(|c| c == cadre));
            if capture.cadre.is_some() && index_cadre_capture[i].is_none() {
                compteurs.degrader("cadre de capture introuvable : couche non découpée");
            }
            bitmaps_captures[i] = Some(
                voie.bitmap_media(&capture.cible, false)
                    .map_err(|e| format!("bitmap capture : {e}"))?,
            );
        }
        for (i, cfg_input) in voie.inputs.iter().enumerate() {
            index_cadre_input[i] = cfg_input
                .cadre
                .as_ref()
                .and_then(|cadre| voie.cadres.iter().position(|c| c == cadre));
        }
    }
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
    let mut encodeur: Option<EncodeurActif> = None;
    let mut derniere_erreur = String::new();
    if !sans_nvenc && encode_requis {
        match EncodeurNvenc::nouveau(&contexte, cfg.codec, cw, ch, fps, cfg.debit_kbps) {
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
        match ouvrir_mft(&disponibles, &mut index, &cfg, cw, ch, fps, &contexte) {
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

    let mut sinks: Vec<Box<dyn Sink>> = Vec::new();
    // Chemins des sorties fichier : le muxage audio se fait **après** la
    // fermeture du conteneur, sur le fichier finalisé.
    let mut chemins_sortie: Vec<std::path::PathBuf> = Vec::new();
    for sink in &cfg.sinks {
        match sink {
            SinkCfg::Fichier { chemin, conteneur } => {
                let s = FfmpegSink::nouveau(None, chemin, *conteneur, fps)
                    .map_err(|e| format!("sortie {} : {e}", chemin.display()))?;
                sinks.push(Box::new(s));
                chemins_sortie.push(chemin.clone());
            }
            SinkCfg::Apercu => {}
            SinkCfg::Rtmp { .. } | SinkCfg::Srt { .. } => {
                return Err("sortie réseau non supportée par ce lot".to_string());
            }
        }
    }
    if sinks.is_empty() && encode_requis {
        return Err("aucune sortie fichier".to_string());
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
    let mut captures_audio: Vec<crate::audio::piste::CapturePiste> = Vec::new();
    if let Some(audio) = cfg.audio.filter(|a| a.actif()) {
        if audio.loopback {
            match crate::audio::piste::demarrer(
                crate::audio::Sens::Sortie,
                audio.gain_loopback,
                "loop",
            ) {
                Ok(mut capture) => {
                    // Aligne le son sur la première frame : on attend le
                    // premier paquet réel (buffer WASAPI de 200 ms).
                    let _ = capture.attendre_demarrage(Duration::from_millis(400));
                    captures_audio.push(capture);
                }
                Err(e) => compteurs.degrader(format!("audio loopback indisponible : {e}")),
            }
        }
        if audio.micro {
            match crate::audio::piste::demarrer(
                crate::audio::Sens::Entree,
                audio.gain_micro,
                "mic",
            ) {
                Ok(mut capture) => {
                    let _ = capture.attendre_demarrage(Duration::from_millis(400));
                    captures_audio.push(capture);
                }
                Err(e) => compteurs.degrader(format!("audio micro indisponible : {e}")),
            }
        }
    }

    let debut = Instant::now();
    let mut dernier_echantillon = Instant::now();
    let mut octets_fenetre: u64 = 0;
    let mut erreur_sortie: Option<String> = None;

    while !arret.load(Ordering::Relaxed) {
        horloge.attendre();
        let pts_us = debut.elapsed().as_micros() as i64;

        for media in medias.iter_mut() {
            media
                .avancer(pts_us)
                .map_err(|e| format!("média vidéo : {e}"))?;
        }
        for capture in captures.iter_mut() {
            if let Ok(texture) = capture.session.prochaine_texture(Duration::ZERO) {
                unsafe { contexte.context().CopyResource(&capture.cible, &texture) };
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
        for (z, genre, index) in &plan {
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
                GenreCalque::Capture => voie.groupe_de(captures[*index].rect, *z),
                GenreCalque::Cadre => voie.groupe_de(voie.cadres[*index].rect, *z),
                GenreCalque::Chat | GenreCalque::Input => None,
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
                            capture.rect,
                            crate::media::Fit::Remplir,
                        )
                    } else {
                        crate::media::geometrie(
                            capture.taille,
                            capture.rect,
                            (cw, ch),
                            crate::media::Fit::Remplir,
                        )
                    };
                    voie.dessiner_media(
                        bitmap,
                        &TraceCouche {
                            source,
                            destination: dest,
                            boite: rect_boite(capture.rect),
                            transforme: None,
                            cadre: index_cadre_capture[*index],
                            effets: &crate::media::EffetsCfg::NEUTRE,
                            opacite: 1.0,
                        },
                    )
                    .map_err(|e| format!("capture : {e}"))?;
                }
                GenreCalque::Chat => {
                    let file = chat.lock().unwrap_or_else(|e| e.into_inner());
                    voie.dessiner_chat(*index, &file)
                        .map_err(|e| format!("chat : {e}"))?;
                }
                GenreCalque::Input => {
                    let file = input.lock().unwrap_or_else(|e| e.into_inner());
                    voie.dessiner_input(*index, &file, index_cadre_input[*index])
                        .map_err(|e| format!("input viewer : {e}"))?;
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
                    match ouvrir_mft(&disponibles, &mut index, &cfg, cw, ch, fps, &contexte)
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
    for sink in sinks.iter_mut() {
        let _ = sink.fermer();
    }

    // Audio : arrêt des captures, puis muxage dans chaque fichier finalisé.
    // Le PCM temporaire est supprimé dans tous les cas.
    let mut pistes: Vec<crate::audio::piste::Piste> = Vec::new();
    for capture in captures_audio {
        match capture.arreter() {
            Ok(piste) => pistes.push(piste),
            Err(e) => compteurs.degrader(format!("audio : {e}")),
        }
    }
    if erreur_sortie.is_none() {
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
    /// Polygone de chaque cadre, **coordonnées canvas** — un seul polygone
    /// pour le trait et la découpe des médias.
    geometries: Vec<ID2D1PathGeometry>,
    /// Index du polygone de chaque boîte (`None` = pas de cadre → bordure).
    index_cadre_boite: Vec<Option<usize>>,
}

/// Contenu dessiné par la voie de rendu (figé au démarrage de la sortie).
struct ContenuRendu {
    textes: Vec<TexteCfg>,
    cadres: Vec<CadreCfg>,
    chats: Vec<ChatCfg>,
    boites: Vec<BoiteCfg>,
    inputs: Vec<InputCfg>,
    /// Plans à perspective — rasterisés en offscreen puis projetés.
    persps: Vec<PerspCfg>,
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
            persps,
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
            geometries,
            index_cadre_boite,
        })
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

    fn dessiner_chat(&mut self, index: usize, file: &FileChat) -> Result<(), String> {
        self.dessinateur
            .dessiner_chat(std::slice::from_ref(&self.chats[index]), file)
            .map_err(|e| e.to_string())
    }

    fn dessiner_cadre(&mut self, index: usize) -> Result<(), String> {
        self.dessinateur
            .dessiner_cadre(&self.cadres[index], &self.geometries[index])
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
        let trace = crate::texte::TraceMedia {
            destination: t.destination,
            source: t.source,
            boite: t.boite,
            transforme: t.transforme,
            clip: t.cadre.map(|index| &self.geometries[index]),
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
/// `.cadre-overlay`, `z-index: 5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum GenreCalque {
    Boite,
    Media,
    Capture,
    Chat,
    Input,
    Bordure,
    Cadre,
}

/// Ouvre le prochain MFT disponible, en avançant `index` (repli monotone).
fn ouvrir_mft(
    disponibles: &[EncodeurDispo],
    index: &mut usize,
    cfg: &SortieCfg,
    cw: u32,
    ch: u32,
    fps: u32,
    contexte: &ContexteGpu,
) -> Option<(EncodeurMft, EncodeurInfo)> {
    while *index < disponibles.len() {
        let dispo = &disponibles[*index];
        *index += 1;
        if let Ok(encodeur) = ouvrir_encodeur(dispo, cfg, cw, ch, fps, contexte) {
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
) -> Result<EncodeurMft, ErreurEncodage> {
    EncodeurMft::nouveau_avec(
        dispo.activate(),
        cfg.codec,
        cw,
        ch,
        fps,
        cfg.debit_kbps,
        Some(contexte),
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
    fn rtmp_refuse_avant_toute_capture() {
        let moteur = MoteurNatif::nouveau();
        moteur.configurer_sources(vec![source()]).expect("sources");
        let mut cfg = cfg_fichier();
        cfg.sinks.push(SinkCfg::Rtmp {
            url: "rtmp://live.example/app".to_string(),
        });
        assert!(matches!(
            moteur.demarrer_sortie(cfg),
            Err(ErreurMoteur::Indisponible(_))
        ));
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
        assert!(!c.camera && !c.fond_vert && !c.srt);
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
}
