//! StreamOS — contrats des moteurs de diffusion (anneau 2).
//!
//! Ce crate ne contient **que** des types et des traits : aucune dépendance,
//! aucun accès à Tauri, au domaine ou à `windows`. C'est le point de rencontre
//! unique entre l'application, l'adaptateur OBS (`src/moteur/obs.rs`) et le
//! moteur natif (`sos-moteur-natif`).
//!
//! Contrat : `docs/features/moteur-natif.md` §3.1 à §3.3.
//!
//! État (2026-10-29) : `appliquer_scene` existe sur le trait `Moteur`
//! (application à chaud, différentielle, idempotente) — la scène arrive
//! aussi au moteur natif par `MoteurNatif::configurer_scene` (snapshot de
//! pré-session, hors trait). `appliquer_etat` / `MoteurDiag` restent à
//! faire (lot 2.9).

use std::fmt;
use std::path::PathBuf;

/// Codec de sortie vidéo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

impl Codec {
    /// Nom lisible par l'utilisateur (diagnostic, UI).
    pub fn nom(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::Hevc => "HEVC",
            Codec::Av1 => "AV1",
        }
    }

    /// Identifiant stable pour `moteur.json` et le frontend.
    pub fn id(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::Hevc => "hevc",
            Codec::Av1 => "av1",
        }
    }

    /// Inverse de [`Codec::id`]. `None` si l'identifiant est inconnu.
    pub fn depuis_id(id: &str) -> Option<Self> {
        match id {
            "h264" => Some(Codec::H264),
            "hevc" => Some(Codec::Hevc),
            "av1" => Some(Codec::Av1),
            _ => None,
        }
    }
}

/// Un paquet encodé (bitstream), horodaté.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaquetEncode {
    pub data: Vec<u8>,
    /// Presentation timestamp, en microsecondes.
    pub pts_us: i64,
    /// `true` si le paquet contient une image-clé (IDR).
    pub cle: bool,
}

/// Conteneur de sortie fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Conteneur {
    /// MP4 **fragmenté** (`frag_keyframe+empty_moov`) : lisible même si le
    /// processus meurt en cours d'enregistrement.
    Mp4,
    /// Matroska : clusters, robuste à la coupure.
    Mkv,
}

impl Conteneur {
    pub fn extension(self) -> &'static str {
        match self {
            Conteneur::Mp4 => "mp4",
            Conteneur::Mkv => "mkv",
        }
    }
}

/// Erreurs de sortie. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurSortie {
    /// Fichier local (création, écriture, flush).
    Fichier(String),
    /// Processus sidecar (lancement, statut, kill).
    Processus(String),
}

impl fmt::Display for ErreurSortie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurSortie::Fichier(m) => write!(f, "fichier : {m}"),
            ErreurSortie::Processus(m) => write!(f, "processus : {m}"),
        }
    }
}

impl std::error::Error for ErreurSortie {}

/// Destinataire des paquets encodés.
///
/// Un seul encodeur, N sorties. Implémentés dans `sos-moteur-natif` :
/// `FfmpegSink` (fMP4/MKV, production) et `FileSink` (bitstream brut,
/// harnais). Un sink en erreur est retiré sans arrêter les autres — règle
/// portée par la boucle de `moteur.rs` (le fan-out y est direct).
pub trait Sink: Send {
    /// Pousse un paquet (backpressure par `write_all`).
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurMoteur>;
    /// Termine proprement (fermeture du conteneur, attente du processus).
    fn fermer(&mut self) -> Result<(), ErreurMoteur>;
}

/// Ce que le moteur sait faire — l'UI s'en sert **sans** `if mode == …`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capacites {
    pub capture_ecran: bool,
    pub capture_fenetre: bool,
    pub capture_jeu: bool,
    pub camera: bool,
    pub fond_vert: bool,
    pub masque_shader: bool,
    pub cadres_animes: bool,
    /// Sortie audio muxée dans le fichier (loopback + micro).
    pub audio: bool,
    pub audio_par_app: bool,
    pub roi_encode: bool,
    pub multi_format: bool,
    pub hdr: bool,
    pub srt: bool,
    pub ndi: bool,
}

impl Capacites {
    /// Aucune capacité : les 14 drapeaux à `false`.
    pub fn aucune() -> Self {
        Self::default()
    }

    /// Nombre de capacités actives (affichage compact éventuel).
    pub fn actives(&self) -> usize {
        [
            self.capture_ecran,
            self.capture_fenetre,
            self.capture_jeu,
            self.camera,
            self.fond_vert,
            self.masque_shader,
            self.cadres_animes,
            self.audio,
            self.audio_par_app,
            self.roi_encode,
            self.multi_format,
            self.hdr,
            self.srt,
            self.ndi,
        ]
        .iter()
        .filter(|c| **c)
        .count()
    }
}

/// Nature d'une sortie (une même sortie peut diffuser et enregistrer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortieKind {
    Diffusion,
    Enregistrement,
}

impl SortieKind {
    /// Identifiant stable pour les commandes Tauri et le frontend.
    pub fn id(self) -> &'static str {
        match self {
            SortieKind::Diffusion => "diffusion",
            SortieKind::Enregistrement => "enregistrement",
        }
    }

    /// Inverse de [`SortieKind::id`]. `None` si l'identifiant est inconnu.
    pub fn depuis_id(id: &str) -> Option<Self> {
        match id {
            "diffusion" => Some(SortieKind::Diffusion),
            "enregistrement" => Some(SortieKind::Enregistrement),
            _ => None,
        }
    }
}

/// Configuration d'une sortie.
///
/// ⚠️ `Debug` est **manuel** : `cle` et les URLs RTMP/SRT sont masquées,
/// pour qu'aucun journal ne puisse publier une clé de flux.
#[derive(Clone, PartialEq, Eq)]
pub enum SinkCfg {
    Fichier { chemin: PathBuf, conteneur: Conteneur },
    Rtmp { url: String },
    Srt { url: String },
    Apercu,
}

impl fmt::Debug for SinkCfg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SinkCfg::Fichier { chemin, conteneur } => f
                .debug_struct("Fichier")
                .field("chemin", chemin)
                .field("conteneur", conteneur)
                .finish(),
            SinkCfg::Rtmp { .. } => f.write_str("Rtmp { url: \"<masquée>\" }"),
            SinkCfg::Srt { .. } => f.write_str("Srt { url: \"<masquée>\" }"),
            SinkCfg::Apercu => f.write_str("Apercu"),
        }
    }
}

/// Sources audio d'un enregistrement — loopback (son du PC) + micro.
///
/// Gains en **millièmes** (1000 = unité, 0 = muet) : `SortieCfg` dérive `Eq`,
/// un `f32` casserait le contrat partagé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioCfg {
    /// Loopback du périphérique de sortie par défaut (= « desktop audio »).
    pub loopback: bool,
    /// Micro du périphérique d'entrée par défaut.
    pub micro: bool,
    /// Gain du loopback, en millièmes (1000 = 1.0).
    pub gain_loopback: u32,
    /// Gain du micro, en millièmes (1000 = 1.0).
    pub gain_micro: u32,
}

impl AudioCfg {
    /// `true` si au moins une source est demandée.
    pub fn actif(&self) -> bool {
        self.loopback || self.micro
    }

    /// Gain en facteur flottant (1000 → 1.0).
    pub fn facteur(gain_milliemes: u32) -> f32 {
        gain_milliemes as f32 / 1000.0
    }
}

impl Default for AudioCfg {
    fn default() -> Self {
        Self {
            loopback: true,
            micro: true,
            gain_loopback: 1000,
            gain_micro: 1000,
        }
    }
}

/// Configuration complète d'un démarrage de sortie.
#[derive(Clone, PartialEq, Eq)]
pub struct SortieCfg {
    pub sinks: Vec<SinkCfg>,
    pub canvas: (u32, u32),
    pub fps: u32,
    pub codec: Codec,
    pub debit_kbps: u32,
    pub preset: String,
    /// Clé de flux : **jamais journalisée** (masquée dans `Debug`).
    pub cle: Option<String>,
    /// Pistes audio à enregistrer. `None` = sortie muette (comportement
    /// historique des harnais vidéo).
    pub audio: Option<AudioCfg>,
}

impl SortieCfg {
    /// Enregistrement fichier avec les défauts du produit (1080p60, H.264,
    /// 6000 kbps, preset « equilibre »).
    pub fn fichier(chemin: impl Into<PathBuf>, conteneur: Conteneur) -> Self {
        Self {
            sinks: vec![SinkCfg::Fichier {
                chemin: chemin.into(),
                conteneur,
            }],
            canvas: (1920, 1080),
            fps: 60,
            codec: Codec::H264,
            debit_kbps: 6000,
            preset: "equilibre".to_string(),
            cle: None,
            // Un enregistrement « produit » a du son par défaut ; les
            // harnais vidéo passent explicitement `None`.
            audio: Some(AudioCfg::default()),
        }
    }

    /// Refuse une configuration inexploitable **avant** de toucher au GPU.
    pub fn valider(&self) -> Result<(), ErreurMoteur> {
        if self.sinks.is_empty() {
            return Err(ErreurMoteur::Configuration(
                "aucune sortie demandée".to_string(),
            ));
        }
        if self.canvas.0 == 0 || self.canvas.1 == 0 {
            return Err(ErreurMoteur::Configuration(format!(
                "canvas invalide : {}×{}",
                self.canvas.0, self.canvas.1
            )));
        }
        if self.fps == 0 {
            return Err(ErreurMoteur::Configuration("fps nul".to_string()));
        }
        if self.debit_kbps == 0 {
            return Err(ErreurMoteur::Configuration(
                "débit nul".to_string(),
            ));
        }
        if let Some(audio) = self.audio {
            for (nom, gain) in [
                ("loopback", audio.gain_loopback),
                ("micro", audio.gain_micro),
            ] {
                if gain > 4000 {
                    return Err(ErreurMoteur::Configuration(format!(
                        "gain {nom} hors bornes : {gain} millièmes (max 4000)"
                    )));
                }
            }
        }
        Ok(())
    }
}

impl fmt::Debug for SortieCfg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SortieCfg")
            .field("sinks", &self.sinks)
            .field("canvas", &self.canvas)
            .field("fps", &self.fps)
            .field("codec", &self.codec)
            .field("debit_kbps", &self.debit_kbps)
            .field("preset", &self.preset)
            .field("cle", &self.cle.as_ref().map(|_| "<masquée>"))
            .field("audio", &self.audio)
            .finish()
    }
}

/// État du moteur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoteurEtat {
    Inactif,
    Actif,
    Erreur(String),
}

impl MoteurEtat {
    pub fn est_actif(&self) -> bool {
        matches!(self, MoteurEtat::Actif)
    }

    /// Identifiant stable pour l'event `moteur:etat` et le frontend.
    pub fn id(&self) -> &'static str {
        match self {
            MoteurEtat::Inactif => "inactif",
            MoteurEtat::Actif => "actif",
            MoteurEtat::Erreur(_) => "erreur",
        }
    }
}

/// Une image d'aperçu basse résolution (BGRA8, `largeur × hauteur × 4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApercuFrame {
    pub data: Vec<u8>,
    pub largeur: u32,
    pub hauteur: u32,
    pub pts_us: i64,
}

impl ApercuFrame {
    /// `true` si la taille du tampon correspond aux dimensions annoncées.
    pub fn valide(&self) -> bool {
        let attendu = self.largeur as usize * self.hauteur as usize * 4;
        self.largeur > 0 && self.hauteur > 0 && self.data.len() == attendu
    }
}

/// Erreurs du moteur. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurMoteur {
    /// Moteur absent de cette plateforme ou non branché.
    Indisponible(String),
    /// Configuration refusée avant démarrage.
    Configuration(String),
    /// La sortie demandée tourne déjà.
    DejaActif(SortieKind),
    /// La sortie demandée ne tourne pas.
    Inactif(SortieKind),
    /// Défaillance d'un sink.
    Sortie(ErreurSortie),
    /// Défaillance interne (GPU, encodeur, horloge).
    Interne(String),
}

impl fmt::Display for ErreurMoteur {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurMoteur::Indisponible(m) => write!(f, "moteur indisponible : {m}"),
            ErreurMoteur::Configuration(m) => write!(f, "configuration : {m}"),
            ErreurMoteur::DejaActif(k) => write!(f, "sortie déjà active : {}", k.id()),
            ErreurMoteur::Inactif(k) => write!(f, "sortie inactive : {}", k.id()),
            ErreurMoteur::Sortie(e) => write!(f, "sortie : {e}"),
            ErreurMoteur::Interne(m) => write!(f, "interne : {m}"),
        }
    }
}

impl std::error::Error for ErreurMoteur {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ErreurMoteur::Sortie(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ErreurSortie> for ErreurMoteur {
    fn from(e: ErreurSortie) -> Self {
        ErreurMoteur::Sortie(e)
    }
}

/// Contrat de l'anneau 2 — implémenté par `MoteurNatif` et `MoteurObs`.
///
/// Sémantique obligatoire (contrat §3.1) :
/// - `appliquer_scene` **ne bloque pas** (file bornée, drop-oldest) et est
///   **différentielle** : un déplacement de widget ne recrée ni la session
///   de capture, ni la texture, ni l'encodeur ;
/// - `appliquer_scene` est **idempotente** : deux fois la même scène =
///   aucun effet ; hors session, elle met à jour la configuration que le
///   prochain démarrage consommera ;
/// - `demarrer_sortie` et `arreter_sortie` ne bloquent pas l'appelant ;
/// - `arreter` est appelable dans n'importe quel état, plusieurs fois ;
/// - `apercu` est optionnel : `None` si le moteur n'en produit pas.
///
/// `appliquer_etat(&RuntimeState)` n'est pas encore dans le trait : les
/// données vivantes (chat, input viewer) arrivent au moteur natif par
/// `pousser_chat` / `pousser_input`.
pub trait Moteur: Send + Sync {
    fn capacites(&self) -> Capacites;
    fn appliquer_scene(&self, scene: &sos_scene::Scene);
    fn demarrer_sortie(&self, cfg: SortieCfg) -> Result<(), ErreurMoteur>;
    fn arreter_sortie(&self, kind: SortieKind) -> Result<(), ErreurMoteur>;
    fn etat(&self) -> MoteurEtat;
    fn apercu(&self) -> Option<ApercuFrame>;
    fn arreter(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_identifiants_stables() {
        for codec in [Codec::H264, Codec::Hevc, Codec::Av1] {
            assert_eq!(Codec::depuis_id(codec.id()), Some(codec));
        }
        assert_eq!(Codec::depuis_id("vp9"), None);
        assert_eq!(Codec::H264.nom(), "H.264");
    }

    #[test]
    fn conteneur_extension() {
        assert_eq!(Conteneur::Mp4.extension(), "mp4");
        assert_eq!(Conteneur::Mkv.extension(), "mkv");
    }

    #[test]
    fn sortie_kind_identifiants_stables() {
        for kind in [SortieKind::Diffusion, SortieKind::Enregistrement] {
            assert_eq!(SortieKind::depuis_id(kind.id()), Some(kind));
        }
        assert_eq!(SortieKind::depuis_id("replay"), None);
    }

    #[test]
    fn capacites_aucune_par_defaut() {
        let c = Capacites::aucune();
        assert_eq!(c.actives(), 0);
        assert!(!c.capture_ecran);
        assert!(!c.camera);
        assert!(!c.fond_vert);
        assert!(!c.audio);
    }

    #[test]
    fn capacites_compte_les_drapeaux() {
        let c = Capacites {
            capture_ecran: true,
            camera: true,
            ..Capacites::aucune()
        };
        assert_eq!(c.actives(), 2);
    }

    #[test]
    fn sortie_cfg_fichier_valide() {
        let cfg = SortieCfg::fichier("C:/tmp/x.mp4", Conteneur::Mp4);
        assert!(cfg.valider().is_ok());
        assert_eq!(cfg.sinks.len(), 1);
    }

    #[test]
    fn sortie_cfg_refuse_les_valeurs_nulles() {
        let mut cfg = SortieCfg::fichier("C:/tmp/x.mp4", Conteneur::Mp4);
        cfg.fps = 0;
        assert!(matches!(cfg.valider(), Err(ErreurMoteur::Configuration(_))));

        let mut cfg = SortieCfg::fichier("C:/tmp/x.mp4", Conteneur::Mp4);
        cfg.canvas = (0, 1080);
        assert!(matches!(cfg.valider(), Err(ErreurMoteur::Configuration(_))));

        let mut cfg = SortieCfg::fichier("C:/tmp/x.mp4", Conteneur::Mp4);
        cfg.debit_kbps = 0;
        assert!(matches!(cfg.valider(), Err(ErreurMoteur::Configuration(_))));

        let mut cfg = SortieCfg::fichier("C:/tmp/x.mp4", Conteneur::Mp4);
        cfg.sinks.clear();
        assert!(matches!(cfg.valider(), Err(ErreurMoteur::Configuration(_))));

        let mut cfg = SortieCfg::fichier("C:/tmp/x.mp4", Conteneur::Mp4);
        cfg.audio = Some(AudioCfg {
            gain_micro: 4001,
            ..AudioCfg::default()
        });
        assert!(matches!(cfg.valider(), Err(ErreurMoteur::Configuration(_))));
    }

    #[test]
    fn audio_cfg_defauts_et_gains() {
        let cfg = AudioCfg::default();
        assert!(cfg.loopback && cfg.micro && cfg.actif());
        assert_eq!(AudioCfg::facteur(cfg.gain_loopback), 1.0);
        assert_eq!(AudioCfg::facteur(500), 0.5);
        assert!(!AudioCfg {
            loopback: false,
            micro: false,
            gain_loopback: 1000,
            gain_micro: 1000,
        }
        .actif());
        // Un enregistrement « produit » a du son ; un harnais vidéo non.
        assert!(SortieCfg::fichier("C:/tmp/x.mp4", Conteneur::Mp4).audio.is_some());
    }

    #[test]
    fn debug_ne_publie_jamais_la_cle_ni_les_urls() {
        let cfg = SortieCfg {
            sinks: vec![
                SinkCfg::Rtmp {
                    url: "rtmp://live.example/app".to_string(),
                },
                SinkCfg::Srt {
                    url: "srt://live.example:9000".to_string(),
                },
            ],
            canvas: (1920, 1080),
            fps: 60,
            codec: Codec::H264,
            debit_kbps: 6000,
            preset: "equilibre".to_string(),
            cle: Some("secret-de-flux".to_string()),
            audio: None,
        };
        let texte = format!("{cfg:?}");
        assert!(!texte.contains("secret-de-flux"));
        assert!(!texte.contains("live.example"));
        assert!(texte.contains("<masquée>"));
    }

    #[test]
    fn erreur_moteur_affiche_et_source() {
        let e = ErreurMoteur::from(ErreurSortie::Fichier("disque plein".to_string()));
        assert!(e.to_string().contains("disque plein"));
        assert!(std::error::Error::source(&e).is_some());
        assert_eq!(ErreurMoteur::DejaActif(SortieKind::Diffusion).to_string(), "sortie déjà active : diffusion");
    }

    #[test]
    fn apercu_valide_controle_la_taille() {
        let ok = ApercuFrame {
            data: vec![0; 4 * 2 * 2],
            largeur: 2,
            hauteur: 2,
            pts_us: 0,
        };
        assert!(ok.valide());

        let tronque = ApercuFrame {
            data: vec![0; 3],
            ..ok
        };
        assert!(!tronque.valide());
    }

    /// Vérifie que le trait reste utilisable en objet (`dyn Moteur`) et que
    /// les implémentations de test sont bien `Send + Sync`.
    #[test]
    fn trait_utilisable_en_objet() {
        struct Factice;

        impl Moteur for Factice {
            fn capacites(&self) -> Capacites {
                Capacites::aucune()
            }

            fn appliquer_scene(&self, _scene: &sos_scene::Scene) {}

            fn demarrer_sortie(&self, cfg: SortieCfg) -> Result<(), ErreurMoteur> {
                cfg.valider()
            }

            fn arreter_sortie(&self, _kind: SortieKind) -> Result<(), ErreurMoteur> {
                Ok(())
            }

            fn etat(&self) -> MoteurEtat {
                MoteurEtat::Inactif
            }

            fn apercu(&self) -> Option<ApercuFrame> {
                None
            }

            fn arreter(&self) {}
        }

        let moteur: Box<dyn Moteur> = Box::new(Factice);
        assert_eq!(moteur.etat(), MoteurEtat::Inactif);
        assert!(!moteur.etat().est_actif());
        assert_eq!(moteur.etat().id(), "inactif");
        assert!(moteur.apercu().is_none());
    }
}
