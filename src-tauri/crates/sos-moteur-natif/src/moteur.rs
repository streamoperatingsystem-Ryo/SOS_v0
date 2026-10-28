//! Orchestrateur du moteur natif (lot 1.6).
//!
//! Assemble les briques validées : WGC (N sources) → compositeur NV12 →
//! encodeur Media Foundation (matériel d'abord, repli logiciel) → sinks →
//! aperçu, cadencé par l'horloge, surveillé par le gouverneur et les compteurs.
//!
//! Contrat : `docs/features/moteur-natif.md` §3.1. `MoteurNatif` implémente
//! `sos_moteur_api::Moteur` ; les sources viennent de `configurer_sources`
//! tant que `appliquer_scene` n'existe pas (lot 1.1 de l'API).
//!
//! Limites assumées de ce lot :
//! - sorties fichier seulement (MP4 fragmenté / MKV) ; RTMP/SRT plus tard ;
//! - pas de caméra (Media Foundation Source Reader), pas de médias ;
//! - le gouverneur **constate** et trace sa décision, il ne reconfigure pas
//!   encore le compositeur/l'encodeur à chaud.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, ID2D1PathGeometry};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

use sos_moteur_api::{
    ApercuFrame, Capacites, ErreurMoteur, Moteur, MoteurEtat, PaquetEncode, Sink, SinkCfg,
    SortieCfg, SortieKind,
};

use crate::cadres::CadreCfg;
use crate::capture::enumeration::enumerer_moniteurs;
use crate::capture::wgc::CaptureWgc;
use crate::chat::{ChatCfg, FileChat, MessageChat};
use crate::clock::Horloge;
use crate::color_gpu::ConvertisseurNv12;
use crate::compositor::{Calque, Compositeur};
use crate::compteurs::{CompteursMoteur, Instantane};
use crate::encode::detect;
use crate::encode::mft::EncodeurMft;
use crate::encode::nvenc::EncodeurNvenc;
use crate::encode::{detect::EncodeurDispo, ErreurEncodage};
use crate::gouverneur::{Action, Gouverneur, Mesures};
use crate::gpu::{ContexteGpu, DescTexture, Format};
use crate::media::MediaCfg;
use crate::output::ffmpeg::FfmpegSink;
use crate::preview::Apercu;
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
    textes: Mutex<Vec<TexteCfg>>,
    cadres: Mutex<Vec<CadreCfg>>,
    chats: Mutex<Vec<ChatCfg>>,
    /// Buffer de chat **vivant** : l'app y pousse pendant l'enregistrement,
    /// le rendu le relit à chaque frame sans reconfigurer la scène.
    chat: Arc<Mutex<FileChat>>,
    etat: Arc<Mutex<EtatInterne>>,
    compteurs: Arc<CompteursMoteur>,
    arret: Mutex<Option<Arc<AtomicBool>>>,
    fil: Mutex<Option<JoinHandle<()>>>,
    apercu: Arc<Mutex<Option<ApercuFrame>>>,
    encodeur: Arc<Mutex<Option<EncodeurInfo>>>,
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
            textes: Mutex::new(Vec::new()),
            cadres: Mutex::new(Vec::new()),
            chats: Mutex::new(Vec::new()),
            chat: Arc::new(Mutex::new(FileChat::nouveau())),
            etat: Arc::new(Mutex::new(EtatInterne::Inactif)),
            compteurs: Arc::new(CompteursMoteur::nouveau()),
            arret: Mutex::new(None),
            fil: Mutex::new(None),
            apercu: Arc::new(Mutex::new(None)),
            encodeur: Arc::new(Mutex::new(None)),
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

        let sources = self.sources();
        let medias = self
            .medias
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let textes = self.textes();
        let cadres = self.cadres();
        let chats = self.chats();
        if sources.is_empty() && medias.is_empty() && textes.is_empty() && chats.is_empty() {
            return Err(ErreurMoteur::Configuration(
                "aucune source, média, texte ni chat configuré".to_string(),
            ));
        }
        for source in &sources {
            source.valider()?;
        }

        let arret = Arc::new(AtomicBool::new(false));
        *self.etat.lock().unwrap_or_else(|e| e.into_inner()) = EtatInterne::Actif;
        *self.arret.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&arret));

        let etat = Arc::clone(&self.etat);
        let compteurs = Arc::clone(&self.compteurs);
        let apercu = Arc::clone(&self.apercu);
        let encodeur_partage = Arc::clone(&self.encodeur);
        let chat = Arc::clone(&self.chat);
        let fil = std::thread::Builder::new()
            .name("sos-moteur-natif".to_string())
            .spawn(move || {
                let resultat = boucle(
                    cfg,
                    SceneChargee {
                        sources,
                        medias,
                        textes,
                        cadres,
                        chats,
                        chat,
                    },
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
    fn arreter(&self) {
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
/// cadres, chat).
struct SceneChargee {
    sources: Vec<SourceCfg>,
    medias: Vec<MediaCfg>,
    textes: Vec<TexteCfg>,
    cadres: Vec<CadreCfg>,
    chats: Vec<ChatCfg>,
    /// Buffer vivant : l'app y pousse pendant la session.
    chat: Arc<Mutex<FileChat>>,
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
    /// Cadre du widget trou (découpe au polygone).
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
        chat,
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

    // Plan de composition : ordre z, puis contenu → cadre (le cadre d'un
    // widget passe au-dessus de son contenu, comme `.cadre-overlay` z-index 5).
    let mut plan: Vec<(i32, GenreCalque, usize)> = Vec::new();
    for (i, media) in medias.iter().enumerate() {
        plan.push((media.z, GenreCalque::Media, i));
    }
    for (i, capture) in captures.iter().enumerate() {
        plan.push((capture.z, GenreCalque::Capture, i));
    }
    for (i, cfg_chat) in chats.iter().enumerate() {
        plan.push((cfg_chat.z, GenreCalque::Chat, i));
    }
    for (i, cadre) in cadres.iter().enumerate() {
        plan.push((cadre.z, GenreCalque::Cadre, i));
    }
    plan.sort_by_key(|(z, genre, _)| (*z, *genre));

    // Voie A du rendu (2026-10-28) : quand il y a un texte, un cadre ou un
    // chat, la composition se fait dans un canvas **BGRA** (Direct2D y dessine
    // avec un alpha réel), puis une passe `VideoProcessorBlt` convertit ce
    // canvas en NV12 pour l'encodeur. Sans aucun, le chemin direct
    // NV12 → encodeur reste inchangé (aucune passe ajoutée).
    let mut voie = if textes_cfg.is_empty() && cadres.is_empty() && chats.is_empty() {
        None
    } else {
        Some(
            VoieRendu::nouveau(&contexte, cw, ch, fps, textes_cfg, cadres, chats)
                .map_err(|e| format!("rendu : {e}"))?,
        )
    };

    // Bitmaps Direct2D des couches à cadre + index de leur polygone. Construits
    // **une fois** : la géométrie et la vue Direct2D ne changent pas.
    let mut bitmaps_medias: Vec<Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1>> =
        vec![None; medias.len()];
    let mut bitmaps_captures: Vec<Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1>> =
        vec![None; captures.len()];
    let mut index_cadre_media: Vec<Option<usize>> = vec![None; medias.len()];
    let mut index_cadre_capture: Vec<Option<usize>> = vec![None; captures.len()];
    if let Some(v) = voie.as_mut() {
        for (i, media) in medias.iter().enumerate() {
            if let Some(cadre) = &media.cadre {
                index_cadre_media[i] = v.cadres.iter().position(|c| c == cadre);
                let Some(_) = index_cadre_media[i] else {
                    // Pas de polygone : la couche repasse par le video
                    // processor (rectangle), et l'état le dit.
                    compteurs.degrader("cadre de média introuvable : couche non découpée");
                    continue;
                };
                bitmaps_medias[i] = Some(
                    v.bitmap_media(&media.texture)
                        .map_err(|e| format!("bitmap média : {e}"))?,
                );
            }
        }
        for (i, capture) in captures.iter().enumerate() {
            if let Some(cadre) = &capture.cadre {
                index_cadre_capture[i] = v.cadres.iter().position(|c| c == cadre);
                let Some(_) = index_cadre_capture[i] else {
                    compteurs.degrader("cadre de capture introuvable : couche non découpée");
                    continue;
                };
                bitmaps_captures[i] = Some(
                    v.bitmap_media(&capture.cible)
                        .map_err(|e| format!("bitmap capture : {e}"))?,
                );
            }
        }
    }

    // Segments D2D : dès qu'un cadre ou un chat existe, l'ordre z impose une
    // composition par segments (un blt borné par calque), sinon le premier lot
    // du compositeur repeindrait le canvas en noir par-dessus Direct2D.
    let segments = voie
        .as_ref()
        .is_some_and(|v| !v.cadres.is_empty() || !v.chats.is_empty());

    let mut compositeur = match voie.as_ref() {
        Some(v) => Compositeur::nouveau_sur(&contexte, &v.canvas, fps)
            .map_err(|e| format!("compositeur (rendu) : {e}"))?,
        None => Compositeur::nouveau(&contexte, cw, ch, fps)
            .map_err(|e| format!("compositeur : {e}"))?,
    };

    // Texture que l'encodeur consomme : NV12 du compositeur (chemin direct)
    // ou NV12 converti depuis le canvas BGRA (voie de rendu).
    let cible_encode = match voie.as_ref() {
        Some(v) => v.nv12.clone(),
        None => compositeur.sortie().clone(),
    };

    // Tous les encodeurs MFT, matériels puis logiciels : le repli doit
    // pouvoir descendre jusqu'au logiciel même quand un matériel est listé
    // mais refuse de s'initialiser ou d'encoder (exigence plan §11.3).
    let mut disponibles =
        detect::lister_tous(cfg.codec).map_err(|e| format!("encodeurs : {e}"))?;
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
    if !sans_nvenc {
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
    if encodeur.is_none() {
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

    let Some(mut encodeur) = encodeur else {
        return Err(format!(
            "aucun encodeur {} utilisable : {derniere_erreur}",
            cfg.codec.nom()
        ));
    };

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
    if sinks.is_empty() {
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
    let convertisseur = ConvertisseurNv12::nouveau(&contexte, cw, ch, fps).ok();
    if matches!(encodeur, EncodeurActif::Mft(_)) && !zero_copie {
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

        if !segments {
            let mut calques: Vec<Calque<'_>> = Vec::with_capacity(plan.len());
            for (_, genre, index) in &plan {
                if let Some(c) = calque_de(*genre, *index, &medias, &captures, cw, ch) {
                    calques.push(c);
                }
            }
            // Sans calque, le canvas (créé à zéro par le pilote) reste noir :
            // une scène « titre seul » doit quand même s'enregistrer.
            if calques.is_empty() && voie.is_none() {
                compteurs.perdue();
                continue;
            }
            if !calques.is_empty() {
                compositeur
                    .composer(&calques)
                    .map_err(|e| format!("composition : {e}"))?;
                compteurs.composite();
            }
            if let Some(v) = voie.as_mut() {
                v.rendre_textes().map_err(|e| format!("rendu : {e}"))?;
            }
        } else {
            let v = voie.as_mut().ok_or("voie de rendu absente")?;
            v.debut();
            v.effacer();
            let mut calques: Vec<Calque<'_>> = Vec::with_capacity(plan.len());
            let mut compose = false;
            for (_, genre, index) in &plan {
                match genre {
                    GenreCalque::Media => {
                        let media = &medias[*index];
                        match (media.cadre.as_ref(), bitmaps_medias[*index].as_ref()) {
                            (Some(_), Some(bitmap)) => {
                                flush_run(&mut calques, &mut compositeur, v, &mut compose)?;
                                let (source, dest) = crate::media::geometrie(
                                    media.taille,
                                    media.rect,
                                    (cw, ch),
                                    media.fit,
                                );
                                let ci = index_cadre_media[*index]
                                    .ok_or_else(|| format!("cadre du média {index} introuvable"))?;
                                v.dessiner_media(bitmap, source, dest, ci)
                                    .map_err(|e| format!("média : {e}"))?;
                            }
                            _ => {
                                if let Some(c) =
                                    calque_de(*genre, *index, &medias, &captures, cw, ch)
                                {
                                    calques.push(c);
                                }
                            }
                        }
                    }
                    GenreCalque::Capture => {
                        let capture = &captures[*index];
                        match (capture.cadre.as_ref(), bitmaps_captures[*index].as_ref()) {
                            (Some(_), Some(bitmap)) if capture.prete => {
                                flush_run(&mut calques, &mut compositeur, v, &mut compose)?;
                                let (lw, lh) = capture.taille;
                                let source = RECT {
                                    left: 0,
                                    top: 0,
                                    right: lw as i32,
                                    bottom: lh as i32,
                                };
                                let ci = index_cadre_capture[*index].ok_or_else(|| {
                                    format!("cadre de la capture {index} introuvable")
                                })?;
                                v.dessiner_media(bitmap, source, destination(capture.rect, cw, ch), ci)
                                    .map_err(|e| format!("capture : {e}"))?;
                            }
                            _ => {
                                if let Some(c) =
                                    calque_de(*genre, *index, &medias, &captures, cw, ch)
                                {
                                    calques.push(c);
                                }
                            }
                        }
                    }
                    GenreCalque::Chat => {
                        flush_run(&mut calques, &mut compositeur, v, &mut compose)?;
                        let file = chat.lock().unwrap_or_else(|e| e.into_inner());
                        v.dessiner_chat(*index, &file)
                            .map_err(|e| format!("chat : {e}"))?;
                    }
                    GenreCalque::Cadre => {
                        flush_run(&mut calques, &mut compositeur, v, &mut compose)?;
                        v.dessiner_cadre(*index)
                            .map_err(|e| format!("cadre : {e}"))?;
                    }
                }
            }
            flush_run(&mut calques, &mut compositeur, v, &mut compose)?;
            v.dessiner_textes_passe()
                .map_err(|e| format!("texte : {e}"))?;
            v.fin().map_err(|e| format!("rendu : {e}"))?;
            v.convertir().map_err(|e| format!("rendu : {e}"))?;
            if compose {
                compteurs.composite();
            }
        }

        let mut resultat = encoder_actif(
            &mut encodeur,
            &mut zero_copie,
            convertisseur.as_ref(),
            &cible_encode,
            pts_us,
        );
        if let Err(e) = &resultat {
            // Repli : NVENC ou MFT refuse la frame. On descend dans l'ordre
            // de détection MFT puis on rejoue la frame — une seule fois.
            match ouvrir_mft(&disponibles, &mut index, &cfg, cw, ch, fps, &contexte) {
                Some((enc, mut info)) => {
                    zero_copie =
                        info.materiel && std::env::var_os("SOS_MOTEUR_MFT_MEMOIRE").is_none();
                    info.zero_copie = zero_copie;
                    *encodeur_partage
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(info);
                    compteurs.degrader(format!("repli encodeur ({e})"));
                    encodeur = EncodeurActif::Mft(enc);
                    resultat = encoder_actif(
                        &mut encodeur,
                        &mut zero_copie,
                        convertisseur.as_ref(),
                        &cible_encode,
                        pts_us,
                    );
                }
                None => return Err(format!("encodage : {e}")),
            }
        }
        let paquets = resultat.map_err(|e| format!("encodage : {e}"))?;
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
            if matches!(encodeur, EncodeurActif::Mft(_)) {
                let mut diag = encodeur_partage.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(info) = diag.as_mut() {
                    info.zero_copie = zero_copie;
                }
            }
            dernier_echantillon = Instant::now();
        }
    }

    let derniers = match &mut encodeur {
        EncodeurActif::Nvenc(nvenc) => nvenc.terminer(),
        EncodeurActif::Mft(mft) => mft.terminer(),
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

/// Voie de rendu : canvas BGRA, Direct2D (textes, cadres, médias découpés,
/// chat), puis conversion NV12.
///
/// Le canvas BGRA est la cible du compositeur ; la texture `nv12` est celle
/// que l'encodeur a enregistrée **une fois** — la conversion réécrit la même
/// surface à chaque frame (même contrat que le chemin zéro-copie).
struct VoieRendu {
    canvas: ID3D11Texture2D,
    nv12: ID3D11Texture2D,
    convertisseur: ConvertisseurNv12,
    dessinateur: DessinateurTexte,
    textes: Vec<TexteCfg>,
    cadres: Vec<CadreCfg>,
    chats: Vec<ChatCfg>,
    /// Polygone de chaque cadre, **coordonnées canvas** — un seul polygone
    /// pour le trait et la découpe des médias.
    geometries: Vec<ID2D1PathGeometry>,
}

impl VoieRendu {
    fn nouveau(
        contexte: &ContexteGpu,
        largeur: u32,
        hauteur: u32,
        fps: u32,
        textes: Vec<TexteCfg>,
        cadres: Vec<CadreCfg>,
        chats: Vec<ChatCfg>,
    ) -> Result<Self, String> {
        let desc = DescTexture::new(Format::Bgra8, largeur, hauteur)
            .map_err(|e| format!("canvas : {e}"))?;
        let canvas = contexte
            .creer_texture(&desc)
            .map_err(|e| format!("canvas : {e}"))?;
        let convertisseur = ConvertisseurNv12::nouveau(contexte, largeur, hauteur, fps)
            .map_err(|e| e.to_string())?;
        let nv12 = convertisseur.creer_cible().map_err(|e| e.to_string())?;
        let dessinateur = DessinateurTexte::nouveau(contexte, &canvas).map_err(|e| e.to_string())?;
        let mut geometries = Vec::with_capacity(cadres.len());
        for cadre in &cadres {
            geometries.push(
                dessinateur
                    .geometrie_cadre(cadre)
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(Self {
            canvas,
            nv12,
            convertisseur,
            dessinateur,
            textes,
            cadres,
            chats,
            geometries,
        })
    }

    fn debut(&mut self) {
        self.dessinateur.debut();
    }

    fn fin(&mut self) -> Result<(), String> {
        self.dessinateur.fin().map_err(|e| e.to_string())
    }

    /// Efface le canvas en noir (le premier lot du compositeur ne le fait
    /// plus en composition segmentée).
    fn effacer(&mut self) {
        self.dessinateur.effacer([0, 0, 0, 255]);
    }

    fn dessiner_textes_passe(&mut self) -> Result<(), String> {
        self.dessinateur
            .dessiner_textes(&self.textes)
            .map_err(|e| e.to_string())
    }

    /// Chemin sans segments : textes, puis conversion NV12.
    fn rendre_textes(&mut self) -> Result<(), String> {
        self.debut();
        self.dessiner_textes_passe()?;
        self.fin()?;
        self.convertir()
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

    fn dessiner_media(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        source: RECT,
        destination: RECT,
        cadre: usize,
    ) -> Result<(), String> {
        self.dessinateur
            .dessiner_media(bitmap, destination, source, &self.geometries[cadre])
            .map_err(|e| e.to_string())
    }

    fn bitmap_media(&mut self, texture: &ID3D11Texture2D) -> Result<ID2D1Bitmap1, String> {
        self.dessinateur
            .bitmap_media(texture)
            .map_err(|e| e.to_string())
    }

    fn convertir(&mut self) -> Result<(), String> {
        self.convertisseur
            .convertir_vers(&self.canvas, &self.nv12)
            .map_err(|e| e.to_string())
    }
}

/// Nature d'une entrée du plan de composition.
///
/// L'ordre de déclaration **est** l'ordre à `z` égal : le contenu passe avant
/// son cadre (miroir de `.cadre-overlay`, `z-index: 5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum GenreCalque {
    Media,
    Capture,
    Chat,
    Cadre,
}

/// Calque video processor d'une entrée du plan, si elle en produit un.
fn calque_de<'a>(
    genre: GenreCalque,
    index: usize,
    medias: &'a [crate::media::MediaChargee],
    captures: &'a [CaptureActive],
    cw: u32,
    ch: u32,
) -> Option<Calque<'a>> {
    match genre {
        GenreCalque::Media => {
            let media = &medias[index];
            let (source, destination) =
                crate::media::geometrie(media.taille, media.rect, (cw, ch), media.fit);
            Some(Calque {
                texture: &media.texture,
                source,
                destination,
            })
        }
        GenreCalque::Capture => {
            let capture = &captures[index];
            if !capture.prete {
                return None;
            }
            let (lw, lh) = capture.taille;
            Some(Calque {
                texture: &capture.cible,
                source: RECT {
                    left: 0,
                    top: 0,
                    right: lw as i32,
                    bottom: lh as i32,
                },
                destination: destination(capture.rect, cw, ch),
            })
        }
        GenreCalque::Chat | GenreCalque::Cadre => None,
    }
}

/// Vide le lot accumulé en une composition **bornée par calque**, puis
/// rouvre la passe Direct2D.
///
/// Chaque calque n'écrit que sa destination : les dessins Direct2D déjà
/// présents (cadres, médias découpés) ne sont jamais recouverts de noir.
fn flush_run(
    calques: &mut Vec<Calque<'_>>,
    compositeur: &mut Compositeur,
    voie: &mut VoieRendu,
    compose: &mut bool,
) -> Result<(), String> {
    if calques.is_empty() {
        return Ok(());
    }
    voie.fin()?;
    compositeur
        .composer_borne(calques)
        .map_err(|e| format!("composition : {e}"))?;
    *compose = true;
    calques.clear();
    voie.debut();
    Ok(())
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

/// Convertit un rect flottant en `RECT` canvas, borné à la surface.
fn destination(rect: (f64, f64, f64, f64), cw: u32, ch: u32) -> RECT {
    let (cwf, chf) = (cw as f64, ch as f64);
    let x = rect.0.clamp(0.0, cwf);
    let y = rect.1.clamp(0.0, chf);
    let largeur = rect.2.max(0.0);
    let hauteur = rect.3.max(0.0);
    RECT {
        left: x as i32,
        top: y as i32,
        right: (x + largeur).min(cwf) as i32,
        bottom: (y + hauteur).min(chf) as i32,
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
    fn destination_borne_au_canvas() {
        let r = destination((-10.0, -10.0, 2000.0, 2000.0), 1920, 1080);
        assert_eq!(r.left, 0);
        assert_eq!(r.top, 0);
        assert_eq!(r.right, 1920);
        assert_eq!(r.bottom, 1080);

        let r = destination((100.0, 50.0, 200.0, 100.0), 1920, 1080);
        assert_eq!((r.left, r.top, r.right, r.bottom), (100, 50, 300, 150));
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
