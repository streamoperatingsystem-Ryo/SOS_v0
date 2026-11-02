//! Registre du moteur — mode actif, persistance, commandes Tauri.
//!
//! Le registre porte les deux moteurs : l'adaptateur OBS (lot 1.3) et le
//! moteur natif (lot 1.4). Il expose l'état du mode, les capacités du moteur
//! **réellement actif** et les commandes de sortie native.

pub mod mode;
pub mod obs;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use sos_moteur_api::{
    ApercuCfg, AudioCfg, Capacites, Codec, Conteneur, GenreAudio, Moteur, MoteurEtat, SinkCfg,
    SortieCfg, SourceAudioCfg,
};
use sos_moteur_natif::chat::{Emote, MessageChat};
use sos_moteur_natif::input::{EtatInput, EtatManette, EtatSouris};
use sos_moteur_natif::moteur::MoteurNatif;
use sos_moteur_natif::texte::{
    url_box_art, AttenteCfg, ClipPresentation, CmdsFight, Communaute, CoupFight, FightState,
    GoalTwitch, InfoFight, JeuRecent, JoueurFight, MajAlerte, MajBandeau, MajGoal, MajMusique,
    MajPad, MajPresentation, MajSpeedrun, MajSplitPop, MajWelcome, ModePresentation, PadPlay, PhaseFight,
    PhaseSpeedrun, Piste, PistePresentation, PositionBandeau, PresentationDonnees, SegmentLss,
};
use tauri::{AppHandle, Listener, Manager, State};

use mode::{verifier_bascule, Mode};
use obs::MoteurObs;

/// Session de sortie native en cours (destination affichée + fps figé).
#[derive(Debug, Clone)]
pub struct SessionNatif {
    /// Destination affichable : chemin du fichier (enregistrement) ou
    /// « schéma://autorité » d'un flux — jamais de clé de stream.
    pub cible: String,
    pub fps: u32,
}

/// État partagé du registre moteur.
pub struct RegistreMoteur {
    mode: Mutex<Mode>,
    /// Le moteur natif est-il branché ? (lot 1.4 : oui)
    natif_disponible: bool,
    /// Adaptateur OBS du contrat `Moteur` (lot 1.3).
    obs: MoteurObs,
    /// Moteur natif — capture, composition, encodage, audio.
    natif: MoteurNatif,
    /// Dernière session native démarrée (effacée à l'arrêt).
    session: Mutex<Option<SessionNatif>>,
}

impl RegistreMoteur {
    pub fn nouveau(mode_initial: Mode) -> Self {
        Self {
            mode: Mutex::new(mode_initial),
            natif_disponible: true,
            obs: MoteurObs::nouveau(),
            natif: MoteurNatif::nouveau(),
            session: Mutex::new(None),
        }
    }

    pub fn mode(&self) -> Mode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn obs(&self) -> &MoteurObs {
        &self.obs
    }

    pub fn natif(&self) -> &MoteurNatif {
        &self.natif
    }

    /// Une sortie native tourne-t-elle ? (verrou court, jamais imbriqué)
    pub fn natif_actif(&self) -> bool {
        self.natif.en_cours()
    }

    /// Session native en cours (chemin affiché par l'UI).
    pub fn session(&self) -> Option<SessionNatif> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn definir_session(&self, session: SessionNatif) {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
    }

    /// Teardown : joint le fil moteur, libère NVENC, finalise la sortie.
    /// Idempotent — appelé à l'arrêt de l'app (`RunEvent::Exit`).
    pub fn arreter_natif(&self) {
        self.natif.arreter();
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// État exposé au frontend.
#[derive(Serialize, Clone)]
pub struct EtatMoteur {
    pub mode: String,
    pub natif_disponible: bool,
    pub obs_disponible: bool,
    /// Capacités du moteur actif — ids stables, libellés par l'i18n.
    pub capacites: Vec<&'static str>,
}

/// Traduit les drapeaux en identifiants stables (contrat avec `moteur.cap.*`).
fn capacites_actives(c: &Capacites) -> Vec<&'static str> {
    let drapeaux = [
        (c.capture_ecran, "capture_ecran"),
        (c.capture_fenetre, "capture_fenetre"),
        (c.capture_jeu, "capture_jeu"),
        (c.camera, "camera"),
        (c.fond_vert, "fond_vert"),
        (c.masque_shader, "masque_shader"),
        (c.cadres_animes, "cadres_animes"),
        (c.audio, "audio"),
        (c.audio_par_app, "audio_par_app"),
        (c.roi_encode, "roi_encode"),
        (c.multi_format, "multi_format"),
        (c.hdr, "hdr"),
        (c.srt, "srt"),
        (c.ndi, "ndi"),
    ];
    drapeaux
        .iter()
        .filter(|(actif, _)| *actif)
        .map(|(_, id)| *id)
        .collect()
}

#[tauri::command]
pub fn moteur_etat(registre: State<'_, RegistreMoteur>) -> EtatMoteur {
    let mode = registre.mode();
    let capacites = match mode {
        Mode::Obs => registre.obs().capacites(),
        Mode::Natif => registre.natif().capacites(),
    };
    EtatMoteur {
        mode: mode.nom().to_string(),
        natif_disponible: registre.natif_disponible,
        obs_disponible: true,
        capacites: capacites_actives(&capacites),
    }
}

/// Diagnostic du moteur — valeurs honnêtes : `None` (`N/A`) tant que rien
/// n'a démarré ; jamais de 0 inventé (plan §10.1).
#[derive(Serialize, Clone)]
pub struct DiagMoteur {
    pub actif: bool,
    pub mode: String,
    pub natif_disponible: bool,
    pub adaptateur: Option<String>,
    pub encodeur: Option<String>,
    pub resolution: Option<(u32, u32)>,
    pub fps: Option<u32>,
}

#[tauri::command]
pub fn moteur_diag(registre: State<'_, RegistreMoteur>) -> DiagMoteur {
    let mode = registre.mode();
    let actif = match mode {
        Mode::Obs => registre.obs().etat().est_actif(),
        Mode::Natif => registre.natif().etat().est_actif(),
    };
    let (encodeur, resolution, fps) = match mode {
        Mode::Natif => {
            let encodeur = registre.natif().encodeur().map(|e| e.nom);
            let instantane = registre.natif().instantane();
            let resolution = (instantane.resolution.0 > 0).then_some(instantane.resolution);
            (encodeur, resolution, registre.session().map(|s| s.fps))
        }
        Mode::Obs => (None, None, None),
    };
    DiagMoteur {
        actif,
        mode: mode.nom().to_string(),
        natif_disponible: registre.natif_disponible,
        adaptateur: Some(mode.nom().to_string()),
        encodeur,
        resolution,
        fps,
    }
}

/// État de la sortie native — ce que l'UI affiche pendant la sortie
/// (enregistrement fichier ou flux réseau).
#[derive(Serialize, Clone)]
pub struct EtatSortieNatif {
    pub actif: bool,
    /// Destination affichable : chemin du fichier ou « schéma://autorité »
    /// d'un flux (la clé de stream n'y figure jamais).
    pub chemin: Option<String>,
    pub kbps: u32,
    pub frames_perdues: u64,
    pub encodeur: Option<String>,
    pub degradation: Option<String>,
    /// Erreur **terminale** du moteur natif (thread arrêté sur erreur).
    /// Distincte de `degradation` : une dégradation n'arrête pas la sortie.
    pub erreur: Option<String>,
}

#[tauri::command]
pub fn moteur_sortie_etat(registre: State<'_, RegistreMoteur>) -> EtatSortieNatif {
    let actif = registre.natif_actif();
    let etat = registre.natif().etat();
    let erreur = match &etat {
        MoteurEtat::Erreur(m) => Some(m.clone()),
        _ => None,
    };
    let instantane = registre.natif().instantane();
    EtatSortieNatif {
        actif,
        // La destination n'est annoncée que tant que le fil tourne : après
        // une erreur terminale, aucun fichier exploitable n'a été écrit.
        chemin: if actif {
            registre.session().map(|s| s.cible.clone())
        } else {
            None
        },
        kbps: if actif { instantane.kbps } else { 0 },
        frames_perdues: instantane.frames_perdues,
        encodeur: registre.natif().encodeur().map(|e| e.nom),
        degradation: instantane.raison_degradation,
        erreur,
    }
}

/// Normalise le chemin choisi par le dialogue : extension du conteneur si
/// absente, dossier parent créé au besoin. Aucun chemin par défaut inventé.
fn normaliser_chemin(chemin: &str, conteneur: Conteneur) -> Result<PathBuf, String> {
    let brut = chemin.trim();
    if brut.is_empty() {
        return Err("chemin vide".to_string());
    }
    let mut chemin = PathBuf::from(brut);
    if chemin.extension().is_none() {
        chemin.set_extension(conteneur.extension());
    }
    if let Some(parent) = chemin.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("dossier de sortie : {e}"))?;
    }
    Ok(chemin)
}

/// Démarre un enregistrement natif. Bloquant (GPU + encodeur) : exécuté hors
/// du thread UI pour ne jamais geler la fenêtre.
#[tauri::command]
pub async fn moteur_sortie_demarrer(app: AppHandle, chemin: String) -> Result<String, String> {
    // Le conteneur persisté fixe l'extension par défaut du dialogue ET le
    // sink — un identifiant inconnu est refusé avant tout contact GPU.
    let conteneur = resoudre_conteneur(Some(&sortie_cfg_persistee(&app).conteneur))?;
    let chemin = normaliser_chemin(&chemin, conteneur)?;
    tauri::async_runtime::spawn_blocking(move || demarrer_bloquant(&app, &chemin, conteneur))
        .await
        .map_err(|e| format!("tâche de démarrage : {e}"))?
}

/// Préambule commun aux sorties natives (fichier ou flux) : mode Rust
/// exigé, une seule sortie à la fois, scène courante poussée au moteur.
/// Rend la scène clonée — son canvas alimente `SortieCfg`.
fn preparer_sortie(app: &AppHandle) -> Result<sos_scene::Scene, String> {
    let registre = app
        .try_state::<RegistreMoteur>()
        .ok_or("registre moteur absent")?;
    if registre.mode() != Mode::Natif {
        return Err("la sortie native exige le mode Rust".to_string());
    }
    if registre.natif_actif() {
        return Err("une sortie native est déjà en cours".to_string());
    }
    let scene = app
        .try_state::<crate::scenes::ScenesState>()
        .ok_or("état des scènes absent")?
        .scene
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let base = crate::config::data_dir(app)?;
    registre
        .natif()
        .configurer_scene(&scene, &HashMap::new(), &base)
        .map_err(|e| e.to_string())?;
    Ok(scene)
}

/// Réglages de sortie persistés (bloc `sortie` de moteur.json) appliqués à
/// une `SortieCfg`. Codec/preset inconnus → erreur honnête avant tout
/// contact GPU (jamais de repli muet sur une config écrite par l'UI).
fn appliquer_sortie_cfg(
    cfg: &mut SortieCfg,
    s: &crate::config::SortieNatifCfg,
) -> Result<(), String> {
    cfg.codec = resoudre_codec(Some(&s.codec))?;
    cfg.debit_kbps = s.debit_kbps;
    cfg.fps = s.fps;
    cfg.preset = resoudre_preset(Some(&s.preset));
    cfg.resolution = resoudre_resolution(s.resolution, cfg.canvas);
    Ok(())
}

/// Traduit le mixeur persisté (`moteur.json`, bloc `audio`) vers le contrat
/// moteur. Genre inconnu, id vide/doublon ou gain hors bornes → erreur
/// honnête **avant** tout contact WASAPI.
fn audio_cfg_depuis_persistee(
    cfg: &crate::config::AudioMoteurCfg,
) -> Result<AudioCfg, String> {
    let mut sources = Vec::with_capacity(cfg.sources.len());
    for source in &cfg.sources {
        let genre = GenreAudio::depuis_id(&source.genre).ok_or_else(|| {
            format!(
                "genre audio inconnu : {} (desktop|micro attendu)",
                source.genre
            )
        })?;
        sources.push(SourceAudioCfg {
            id: source.id.clone(),
            genre,
            peripherique: source
                .peripherique
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string),
            gain: source.gain,
            mute: source.mute,
        });
    }
    let audio = AudioCfg { sources };
    audio.valider().map_err(|e| e.to_string())?;
    Ok(audio)
}

/// Mixeur persisté, prêt pour le moteur — non fatal : un bloc absent ou
/// cassé retombe sur les défauts produit (jamais de sortie muette).
fn audio_cfg_persistee(app: &AppHandle) -> AudioCfg {
    match crate::config::lire_audio_moteur(app) {
        Ok(cfg) => audio_cfg_depuis_persistee(&cfg).unwrap_or_else(|e| {
            log::warn!("audio moteur.json invalide ({e}) — défauts produit");
            AudioCfg::default()
        }),
        Err(e) => {
            log::warn!("audio moteur.json illisible ({e}) — défauts produit");
            AudioCfg::default()
        }
    }
}

/// Relit la config sortie persistée — non-fatal : fichier absent ou cassé
/// → défauts produit (doc §17). Une config *valide mais incohérente* est
/// refusée plus loin par `appliquer_sortie_cfg` / `SortieCfg::valider`.
fn sortie_cfg_persistee(app: &AppHandle) -> crate::config::SortieNatifCfg {
    crate::config::lire_sortie_moteur(app).unwrap_or_default()
}

/// Relit les réglages du projecteur (bloc `apercu` de moteur.json) —
/// non-fatal : fichier absent ou cassé → défauts produit. Traduction vers
/// le type du contrat moteur (les deux structs gardent les mêmes bornes).
fn apercu_cfg_persistee(app: &AppHandle) -> ApercuCfg {
    let a = crate::config::lire_apercu_moteur(app).unwrap_or_default();
    ApercuCfg {
        largeur: a.largeur,
        fps: a.fps,
    }
}

/// Réglages du projecteur persistés (bloc `apercu` de moteur.json) —
/// pré-remplissage du bloc « Projecteur » de la modale Sortie.
#[tauri::command]
pub fn moteur_apercu_config(app: AppHandle) -> Result<crate::config::ApercuNatifCfg, String> {
    crate::config::lire_apercu_moteur(&app)
}

/// Persiste les réglages du projecteur. Refusés si invalides — le disque
/// ne reçoit que des valeurs applicables. Une session « aperçu seul » en
/// cours est arrêtée : le poll suivant la relance sur les nouveaux
/// réglages (application quasi immédiate). Pendant un enregistrement, le
/// réglage s'applique au prochain démarrage de sortie.
#[tauri::command]
pub fn moteur_apercu_config_sauver(
    app: AppHandle,
    registre: State<'_, RegistreMoteur>,
    cfg: crate::config::ApercuNatifCfg,
) -> Result<(), String> {
    // Bornes honnêtes : largeur 0 (canvas) ou 240-4096 px, fps 1-60. La
    // liste d'options de l'UI est un sous-ensemble de ces bornes.
    if cfg.largeur != 0 && !(240..=4096).contains(&cfg.largeur) {
        return Err(format!(
            "largeur aperçu hors bornes : {} (0 = canvas, ou 240-4096)",
            cfg.largeur
        ));
    }
    if cfg.fps == 0 || cfg.fps > 60 {
        return Err(format!("fps aperçu hors bornes : {} (1-60)", cfg.fps));
    }
    crate::config::sauver_apercu_moteur(&app, &cfg)?;
    // Hors enregistrement : la session aperçu détient les captures — la
    // relancer la fait repartir sur les nouveaux réglages. Idempotent si
    // aucune session ne tourne ; sans effet sur une sortie encodée.
    if !registre.natif().en_cours() {
        registre.natif().arreter_apercu();
    }
    Ok(())
}

/// Valide les réglages de sortie envoyés par la modale — mêmes bornes que
/// `SortieCfg::valider`, appliquées AVANT la persistance (un JSON invalide
/// ne doit jamais atteindre le disque).
fn valider_sortie_cfg(s: &crate::config::SortieNatifCfg) -> Result<(), String> {
    resoudre_codec(Some(&s.codec))?;
    resoudre_conteneur(Some(&s.conteneur))?;
    if let Some(h) = s.resolution {
        if !(2..=4320).contains(&h) || h % 2 != 0 {
            return Err(format!(
                "résolution hors bornes : {h} (hauteur paire, 2-4320)"
            ));
        }
    }
    if s.fps == 0 || s.fps > 240 {
        return Err(format!("fps hors bornes : {} (1-240)", s.fps));
    }
    if s.debit_kbps == 0 || s.debit_kbps > 200_000 {
        return Err(format!(
            "débit hors bornes : {} kbps (1-200000)",
            s.debit_kbps
        ));
    }
    if sos_moteur_natif::encode::Preset::depuis_nom(&s.preset).is_none() {
        return Err(format!(
            "preset inconnu : {} (rapide|equilibre|qualite attendu)",
            s.preset
        ));
    }
    Ok(())
}

/// Réglages de sortie natifs persistés (bloc `sortie` de moteur.json) —
/// pré-remplissage de la modale « Sortie ».
#[tauri::command]
pub fn moteur_sortie_config(app: AppHandle) -> Result<crate::config::SortieNatifCfg, String> {
    crate::config::lire_sortie_moteur(&app)
}

/// Persiste les réglages de sortie natifs. Refusés si invalides — le
/// disque ne reçoit que des valeurs applicables telles quelles.
#[tauri::command]
pub fn moteur_sortie_config_sauver(
    app: AppHandle,
    cfg: crate::config::SortieNatifCfg,
) -> Result<(), String> {
    valider_sortie_cfg(&cfg)?;
    crate::config::sauver_sortie_moteur(&app, &cfg)
}

// ===== Mixeur audio — périphériques, niveaux, configuration =====

#[derive(Serialize)]
pub struct PeripheriqueAudioEtat {
    pub id: String,
    pub nom: String,
    pub defaut: bool,
}

#[derive(Serialize)]
pub struct PeripheriquesAudioEtat {
    pub sorties: Vec<PeripheriqueAudioEtat>,
    pub entrees: Vec<PeripheriqueAudioEtat>,
}

/// Périphériques audio actifs (sorties = loopback, entrées = micros).
/// Énumération sans effet de bord (COM MTA), appelée à l'ouverture du
/// mixeur et par le bouton « Rafraîchir » — jamais en tâche de fond.
#[tauri::command]
pub fn moteur_audio_peripheriques() -> Result<PeripheriquesAudioEtat, String> {
    use sos_moteur_natif::audio::{capture, Sens};
    let convertir = |liste: Vec<capture::PeripheriqueAudio>| {
        liste
            .into_iter()
            .map(|p| PeripheriqueAudioEtat {
                id: p.id,
                nom: p.nom,
                defaut: p.defaut,
            })
            .collect::<Vec<_>>()
    };
    let sorties = capture::lister(Sens::Sortie).map_err(|e| e.to_string())?;
    let entrees = capture::lister(Sens::Entree).map_err(|e| e.to_string())?;
    Ok(PeripheriquesAudioEtat {
        sorties: convertir(sorties),
        entrees: convertir(entrees),
    })
}

#[derive(Serialize)]
pub struct CanalNiveauEtat {
    pub peak_db: f32,
    pub rms_db: f32,
    pub hold_db: f32,
}

#[derive(Serialize)]
pub struct NiveauAudioEtat {
    pub gauche: CanalNiveauEtat,
    pub droite: CanalNiveauEtat,
    pub ecretage: bool,
}

#[derive(Serialize)]
pub struct SourceAudioEtat {
    pub id: String,
    pub genre: String,
    pub appareil: Option<String>,
    pub repli: bool,
    pub erreur: Option<String>,
    pub gain: u32,
    pub mute: bool,
    pub niveau: NiveauAudioEtat,
}

#[derive(Serialize)]
pub struct AudioEtat {
    pub actif: bool,
    pub sources: Vec<SourceAudioEtat>,
    pub master: NiveauAudioEtat,
}

fn convertir_niveau(niveau: sos_moteur_natif::audio::NiveauAudio) -> NiveauAudioEtat {
    let canal = |c: sos_moteur_natif::audio::CanalNiveau| CanalNiveauEtat {
        peak_db: c.peak_db,
        rms_db: c.rms_db,
        hold_db: c.hold_db,
    };
    NiveauAudioEtat {
        gauche: canal(niveau.gauche),
        droite: canal(niveau.droite),
        ecretage: niveau.ecretage,
    }
}

/// Niveaux et état des sources — lu par le mixeur UI à ~20 Hz **uniquement
/// pendant que la section Audio est ouverte**.
#[tauri::command]
pub fn moteur_audio_etat(registre: State<'_, RegistreMoteur>) -> AudioEtat {
    let etat = registre.natif().etat_audio();
    AudioEtat {
        actif: etat.actif,
        sources: etat
            .sources
            .into_iter()
            .map(|s| SourceAudioEtat {
                id: s.id,
                genre: s.genre.id().to_string(),
                appareil: s.appareil,
                repli: s.repli,
                erreur: s.erreur,
                gain: s.gain,
                mute: s.mute,
                niveau: convertir_niveau(s.niveau),
            })
            .collect(),
        master: convertir_niveau(etat.master),
    }
}

/// Mixeur persisté (moteur.json, bloc `audio`) — pré-remplissage du panneau.
#[tauri::command]
pub fn moteur_audio_config(app: AppHandle) -> Result<crate::config::AudioMoteurCfg, String> {
    crate::config::lire_audio_moteur(&app)
}

/// Sauve le mixeur et l'applique **à chaud** : la boucle diffère par `id`
/// (gain/mute immédiats, périphérique/ajout/retrait = redémarrage de la
/// seule source concernée). Refusé si invalide — le disque ne reçoit que
/// des valeurs applicables.
#[tauri::command]
pub fn moteur_audio_sauver(
    app: AppHandle,
    registre: State<'_, RegistreMoteur>,
    cfg: crate::config::AudioMoteurCfg,
) -> Result<(), String> {
    let audio = audio_cfg_depuis_persistee(&cfg)?;
    crate::config::sauver_audio_moteur(&app, &cfg)?;
    registre.natif().maj_audio(audio).map_err(|e| e.to_string())
}

/// Efface les latches d'écrêtage (toutes sources + bus programme).
#[tauri::command]
pub fn moteur_audio_reset_ecretage(registre: State<'_, RegistreMoteur>) {
    registre.natif().reset_ecretage();
}

fn demarrer_bloquant(
    app: &AppHandle,
    chemin: &Path,
    conteneur: Conteneur,
) -> Result<String, String> {
    let scene = preparer_sortie(app)?;
    let registre = app
        .try_state::<RegistreMoteur>()
        .ok_or("registre moteur absent")?;
    let mut cfg = SortieCfg::fichier(chemin, conteneur);
    cfg.canvas = (scene.canvasW, scene.canvasH);
    cfg.apercu = apercu_cfg_persistee(app);
    let sortie = sortie_cfg_persistee(app);
    appliquer_sortie_cfg(&mut cfg, &sortie)?;
    cfg.audio = Some(audio_cfg_persistee(app));
    let fps = cfg.fps;
    registre
        .natif()
        .demarrer_sortie(cfg)
        .map_err(|e| e.to_string())?;
    registre.definir_session(SessionNatif {
        cible: chemin.display().to_string(),
        fps,
    });
    Ok(chemin.display().to_string())
}

/// Arrête l'enregistrement natif et retourne le chemin finalisé (`None` si
/// rien ne tournait). Le muxage audio se fait pendant cet appel (~1 s) :
/// exécuté hors du thread UI.
#[tauri::command]
pub async fn moteur_sortie_arreter(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let registre = app
            .try_state::<RegistreMoteur>()
            .ok_or("registre moteur absent")?;
        let chemin = registre.session().map(|s| s.cible.clone());
        registre.arreter_natif();
        Ok::<Option<String>, String>(chemin)
    })
    .await
    .map_err(|e| format!("tâche d'arrêt : {e}"))?
}

/// Démarre un flux réseau natif (RTMP ou SRT) — même moteur et même session
/// qu'un enregistrement : une seule sortie à la fois, arrêt par
/// `moteur_sortie_arreter`, état par `moteur_sortie_etat`. Bloquant (GPU +
/// encodeur) : exécuté hors du thread UI.
///
/// `cle` = clé de stream : jamais journalisée (masquée dans le `Debug` de
/// `SortieCfg`, dans les erreurs du sink et dans le stderr du sidecar
/// FFmpeg). La destination renvoyée est « schéma://autorité » seulement.
///
/// Paramètres optionnels (défauts produit : H.264, 6000 kbps, 60 fps) :
/// `codec` = id stable (`h264|hevc|av1`), `srt_passphrase`/`srt_latency_ms`
/// réservés au protocole `srt`. La config qui a réellement démarré est
/// persistée dans le coffre keyring (`moteur_stream_config` /
/// `moteur_stream_oublier`).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn moteur_stream_demarrer(
    app: AppHandle,
    protocole: String,
    url: String,
    cle: Option<String>,
    codec: Option<String>,
    debit_kbps: Option<u32>,
    fps: Option<u32>,
    preset: Option<String>,
    srt_passphrase: Option<String>,
    srt_latency_ms: Option<u32>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        demarrer_stream_bloquant(
            &app,
            &protocole,
            &url,
            cle.as_deref(),
            codec.as_deref(),
            debit_kbps,
            fps,
            preset.as_deref(),
            srt_passphrase.as_deref(),
            srt_latency_ms,
        )
    })
    .await
    .map_err(|e| format!("tâche de démarrage : {e}"))?
}

/// Codec demandé par le frontend → enum du contrat. `None`/vide = défaut
/// H.264 ; identifiant inconnu = erreur honnête (jamais de repli muet).
fn resoudre_codec(codec: Option<&str>) -> Result<Codec, String> {
    match codec.map(str::trim).filter(|c| !c.is_empty()) {
        None => Ok(Codec::H264),
        Some(id) => Codec::depuis_id(id)
            .ok_or_else(|| format!("codec inconnu : {id} (h264|hevc|av1 attendu)")),
    }
}

/// Conteneur demandé par le frontend → enum du contrat. `None`/vide =
/// défaut MP4 fragmenté ; identifiant inconnu = erreur honnête (jamais
/// de repli muet sur une config écrite).
fn resoudre_conteneur(conteneur: Option<&str>) -> Result<Conteneur, String> {
    match conteneur.map(str::trim).filter(|c| !c.is_empty()) {
        None | Some("mp4") => Ok(Conteneur::Mp4),
        Some("mkv") => Ok(Conteneur::Mkv),
        Some(id) => Err(format!("conteneur inconnu : {id} (mp4|mkv attendu)")),
    }
}

/// Hauteur de sortie persistée → dims encodées `(w, h)` : la largeur est
/// dérivée du ratio du canvas, arrondie au pair supérieur (chroma 4:2:0).
/// `None` = canvas (comportement historique).
fn resoudre_resolution(hauteur: Option<u32>, (cw, ch): (u32, u32)) -> Option<(u32, u32)> {
    let h = hauteur?;
    let w = ((cw as u64 * h as u64 + ch as u64 / 2) / ch.max(1) as u64) as u32;
    Some(((w + 1) & !1, h))
}

/// Preset demandé par le frontend : `None`/vide → `"equilibre"` (défaut
/// produit). Champ transporté par `SortieCfg` — l'encodeur applique
/// aujourd'hui son propre réglage interne (NVENC P4) ; le preset choisi
/// est conservé dans la config persistée pour les lots qui le brancheront.
fn resoudre_preset(preset: Option<&str>) -> String {
    preset
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("equilibre")
        .to_string()
}

/// URL SRT finale : `passphrase`/`latency` deviennent des paramètres de
/// query — FFmpeg les lit nativement (`srt://hôte:port?passphrase=…&
/// latency=…`), sans changement du contrat `SinkCfg::Srt`. Un paramètre
/// déjà présent dans l'URL n'est pas dupliqué (l'utilisateur prime). La
/// valeur est refusée si elle casserait la query (`&`, `?`, `#`, espace) —
/// jamais d'URL corrompue silencieuse. L'URL produite embarque le secret :
/// couverte par le masquage `Debug` de `SinkCfg::Srt` et par
/// `destination_publique` (schéma + autorité seulement).
fn srt_url_avec_params(
    url: &str,
    passphrase: Option<&str>,
    latency_ms: Option<u32>,
) -> Result<String, String> {
    let mut sortie = url.to_string();
    let mut ajouter = |nom: &str, valeur: String| -> Result<(), String> {
        if valeur
            .chars()
            .any(|c| matches!(c, '&' | '?' | '#') || c.is_whitespace())
        {
            return Err(format!("paramètre SRT {nom} invalide (caractères réservés)"));
        }
        if sortie.contains(&format!("{nom}=")) {
            return Ok(());
        }
        let sep = if sortie.contains('?') { '&' } else { '?' };
        sortie = format!("{sortie}{sep}{nom}={valeur}");
        Ok(())
    };
    if let Some(p) = passphrase {
        ajouter("passphrase", p.to_string())?;
    }
    if let Some(ms) = latency_ms {
        ajouter("latency", ms.to_string())?;
    }
    Ok(sortie)
}

const KEYRING_STREAM_SERVICE: &str = "streamos-v0-stream";
const KEYRING_STREAM_ACCOUNT: &str = "default";

/// Config stream persistée dans le coffre keyring — même modèle que
/// `twitch_auth`/`youtube_auth` (JSON sérialisé, compte "default"). La clé
/// de flux et la passphrase SRT vivent dans le coffre OS, jamais dans un
/// fichier en clair (contrat vie privée) ; `Debug` est absent par prudence.
#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct StreamCfgPersistee {
    #[serde(default)]
    pub protocole: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub cle: Option<String>,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub debit_kbps: u32,
    #[serde(default)]
    pub fps: u32,
    /// Blobs écrits avant l'exposition du preset → `"equilibre"` (défaut
    /// produit, jamais de chaîne vide qui mentirait à l'UI).
    #[serde(default = "preset_defaut")]
    pub preset: String,
    #[serde(default)]
    pub srt_passphrase: Option<String>,
    #[serde(default)]
    pub srt_latency_ms: Option<u32>,
}

/// Persiste la config stream dans le coffre (JSON). Les messages d'erreur
/// ne portent jamais la valeur — ni clé ni passphrase ne peuvent fuiter
/// dans un log.
fn sauver_stream_cfg(cfg: &StreamCfgPersistee) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_STREAM_SERVICE, KEYRING_STREAM_ACCOUNT)
        .map_err(|e| format!("Keyring entry: {e}"))?;
    let json = serde_json::to_string(cfg).map_err(|e| format!("Serialize stream cfg: {e}"))?;
    entry
        .set_password(&json)
        .map_err(|e| format!("Keyring set: {e}"))
}

/// Lit la config stream persistée. `None` si le coffre est vide.
fn lire_stream_cfg() -> Result<Option<StreamCfgPersistee>, String> {
    let entry = keyring::Entry::new(KEYRING_STREAM_SERVICE, KEYRING_STREAM_ACCOUNT)
        .map_err(|e| format!("Keyring entry: {e}"))?;
    match entry.get_password() {
        Ok(json) => serde_json::from_str(&json)
            .map(Some)
            .map_err(|e| format!("Deserialize stream cfg: {e}")),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Keyring get: {e}")),
    }
}

fn preset_defaut() -> String {
    "equilibre".to_string()
}

/// Efface la config stream du coffre. Idempotent — coffre déjà vide = Ok.
fn effacer_stream_cfg() -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_STREAM_SERVICE, KEYRING_STREAM_ACCOUNT)
        .map_err(|e| format!("Keyring entry: {e}"))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Keyring delete: {e}")),
    }
}

/// « Oublier » : efface la config stream persistée (URL, clé, passphrase —
/// tout le secret). Best-effort : un coffre déjà vide est un succès, pas
/// une erreur.
#[tauri::command]
pub fn moteur_stream_oublier() -> Result<(), String> {
    effacer_stream_cfg()
}

/// Dernière config stream persistée (pré-remplissage du formulaire UI).
/// Renvoyée telle quelle — clé et passphrase comprises : le coffre
/// appartient à l'utilisateur de la machine, comme les tokens
/// Twitch/YouTube ; rien ne sort réseau.
#[tauri::command]
pub fn moteur_stream_config() -> Result<Option<StreamCfgPersistee>, String> {
    lire_stream_cfg()
}

#[allow(clippy::too_many_arguments)]
fn demarrer_stream_bloquant(
    app: &AppHandle,
    protocole: &str,
    url: &str,
    cle: Option<&str>,
    codec: Option<&str>,
    debit_kbps: Option<u32>,
    fps: Option<u32>,
    preset: Option<&str>,
    srt_passphrase: Option<&str>,
    srt_latency_ms: Option<u32>,
) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("URL de flux vide".to_string());
    }
    let cle = cle
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    let srt_passphrase = srt_passphrase
        .map(str::trim)
        .filter(|p| !p.is_empty());
    // Paramètres absents → réglages persistés de la modale « Sortie »
    // (moteur.json). Le défaut produit n'est qu'un dernier recours.
    let sortie = sortie_cfg_persistee(app);
    let codec = match codec {
        Some(c) => resoudre_codec(Some(c))?,
        None => resoudre_codec(Some(&sortie.codec))?,
    };
    let preset = resoudre_preset(preset.or(Some(&sortie.preset)));
    let fps = fps.unwrap_or(sortie.fps);
    let debit_kbps = debit_kbps.unwrap_or(sortie.debit_kbps);
    // Le schéma exact (rtmp://, rtmps://, srt://) est validé par le moteur
    // avant tout contact GPU — pas de doublon ici.
    let sink = match protocole {
        "rtmp" => {
            // Paramètres SRT sur un flux RTMP : erreur honnête — un
            // paramètre ignoré silencieusement mentirait à l'utilisateur.
            if srt_passphrase.is_some() || srt_latency_ms.is_some() {
                return Err(
                    "srt_passphrase / srt_latency_ms exigent le protocole srt".to_string(),
                );
            }
            SinkCfg::Rtmp {
                url: url.to_string(),
            }
        }
        "srt" => SinkCfg::Srt {
            url: srt_url_avec_params(url, srt_passphrase, srt_latency_ms)?,
        },
        autre => {
            return Err(format!("protocole inconnu : {autre} (rtmp|srt attendu)"));
        }
    };
    let scene = preparer_sortie(app)?;
    let registre = app
        .try_state::<RegistreMoteur>()
        .ok_or("registre moteur absent")?;
    let cfg = SortieCfg {
        sinks: vec![sink],
        canvas: (scene.canvasW, scene.canvasH),
        resolution: resoudre_resolution(sortie.resolution, (scene.canvasW, scene.canvasH)),
        fps,
        apercu: apercu_cfg_persistee(app),
        codec,
        debit_kbps,
        preset: preset.clone(),
        cle: cle.clone(),
        audio: Some(audio_cfg_persistee(app)),
    };
    registre
        .natif()
        .demarrer_sortie(cfg)
        .map_err(|e| e.to_string())?;
    // Seule une config qui a réellement démarré est persistée ; un refus
    // du coffre ne fait jamais échouer un stream déjà parti.
    let persistee = StreamCfgPersistee {
        protocole: protocole.to_string(),
        url: url.to_string(),
        cle,
        codec: codec.id().to_string(),
        debit_kbps,
        fps,
        preset,
        srt_passphrase: srt_passphrase.map(str::to_string),
        srt_latency_ms,
    };
    if let Err(e) = sauver_stream_cfg(&persistee) {
        log::warn!("config stream non persistée (keyring) : {e}");
    }
    let cible = destination_publique(url);
    registre.definir_session(SessionNatif {
        cible: cible.clone(),
        fps,
    });
    Ok(cible)
}

/// Destination affichable d'un flux : « schéma://autorité » seulement —
/// le chemin d'une URL RTMP peut embarquer la clé (URL complète collée
/// par l'utilisateur) et la query le `streamid` SRT. Ni l'un ni l'autre
/// ne doit être affiché ni journalisé.
fn destination_publique(url: &str) -> String {
    let Some(pos) = url.find("://") else {
        return "<flux>".to_string();
    };
    let autorite = url[pos + 3..].split(['/', '?']).next().unwrap_or_default();
    format!("{}://{}", &url[..pos], autorite)
}

/// Bascule de mode. Refusée proprement si une sortie OBS **ou** native est
/// active (jamais de coupure implicite, plan §11.2).
#[tauri::command]
pub fn moteur_basculer(
    app: AppHandle,
    registre: State<'_, RegistreMoteur>,
    cible: String,
    live_actif: bool,
) -> Result<String, String> {
    let cible = Mode::depuis_nom(&cible).ok_or_else(|| format!("mode inconnu : {cible}"))?;
    let actuel = registre.mode();
    let natif_actif = registre.natif_actif();
    let nouveau =
        verifier_bascule(actuel, cible, live_actif, natif_actif).map_err(|e| e.to_string())?;
    if nouveau != actuel {
        *registre.mode.lock().unwrap_or_else(|e| e.into_inner()) = nouveau;
        // Bascule hors du mode Rust : la session aperçu n'a plus lieu de
        // composer (la fenêtre se ferme côté page ; le poll refuse hors
        // mode natif). Idempotent si aucune session ne tourne.
        registre.natif().arreter_apercu();
        crate::config::sauver_mode_moteur(&app, nouveau.nom())?;
    }
    Ok(nouveau.nom().to_string())
}

/// Connexion obs-websocket pour l'adaptateur `MoteurObs` (lot 1.3).
/// Le mot de passe reste en mémoire du backend : jamais persisté ni loggé.
/// Appelée par le frontend avant toute bascule ou action moteur (lot 1.5).
#[tauri::command]
pub fn moteur_obs_connexion(
    registre: State<'_, RegistreMoteur>,
    host: String,
    port: u16,
    password: String,
) {
    registre.obs().configurer_connexion(host, port, password);
}

/// Segment d'emote tel que sérialisé par `twitch_emotes::EmoteSeg`.
#[derive(serde::Deserialize)]
struct EmoteBrut {
    /// Offsets UTF-16 (`fin` EXCLUSIVE) — unités de `String.slice`.
    debut: usize,
    fin: usize,
    url: String,
    nom: String,
}

/// Message chat tel qu'il circule dans `chat_tx` (`ChatMessage` sérialisé).
///
/// `badges`/`avatar`/`emotes` sont **facultatifs** (YouTube/TikTok n'ont
/// pas d'emotes, Kick n'a pas d'avatar) — le crate les rend s'ils sont là.
#[derive(serde::Deserialize)]
struct MessageChatBrut {
    plateforme: String,
    pseudo: String,
    texte: String,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    badges: Option<String>,
    #[serde(default)]
    avatar: Option<String>,
    #[serde(default)]
    emotes: Option<Vec<EmoteBrut>>,
}

/// Frame `chat_tx` : `{ "type": "chat", "message": { … } }`.
#[derive(serde::Deserialize)]
struct FrameChat {
    #[serde(rename = "type")]
    type_frame: String,
    #[serde(default)]
    message: Option<MessageChatBrut>,
}

/// Décode un frame du canal ; `None` si ce n'est pas un message de chat.
///
/// ⚠️ **`serde_json::json!` trie les clés** (pas de `preserve_order` dans ce
/// dépôt) : une frame réelle commence par `{"message"…`, jamais par
/// `{"type"…`. Un filtre `starts_with("{\"type\"…")` ne matchait **aucune**
/// frame — le pont chat du moteur natif était muet (bug trouvé le 2026-10-28,
/// verrouillé par `decoder_chat_accepte_une_frame_serde_json`).
///
/// Le test reste un `contains` sur la sous-chaîne : `chat_tx` transporte aussi
/// les spectres de l'égaliseur et l'état de l'input viewer (plusieurs
/// dizaines de frames par seconde) — inutile de les désérialiser.
fn decoder_frame_chat(frame: &str) -> Option<MessageChat> {
    if !frame.contains("\"type\":\"chat\"") {
        return None;
    }
    let frame: FrameChat = serde_json::from_str(frame).ok()?;
    if frame.type_frame != "chat" {
        return None;
    }
    let message = frame.message?;
    let emotes = message
        .emotes
        .unwrap_or_default()
        .into_iter()
        .map(|e| Emote {
            debut: e.debut,
            fin: e.fin,
            url: e.url,
            nom: e.nom,
        })
        .collect();
    Some(
        MessageChat::nouveau(
            message.plateforme,
            message.pseudo,
            message.texte,
            message.color.as_deref(),
        )
        .avec_medias(message.badges, message.avatar, emotes),
    )
}

/// Extrait l'état d'une frame `input-viewer-etat` (30 Hz).
///
/// Même piège de tri des clés que [`decoder_frame_chat`] : test par
/// sous-chaîne, puis validation du champ `type` après désérialisation.
fn decoder_frame_input(frame: &str) -> Option<EtatInput> {
    if !frame.contains("\"type\":\"input-viewer-etat\"") {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct FrameInput {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        etat: EtatInputBrut,
    }
    #[derive(serde::Deserialize, Default)]
    struct EtatInputBrut {
        #[serde(default)]
        touches: Vec<String>,
        #[serde(default)]
        souris: SourisBrute,
        #[serde(default)]
        manette: ManetteBrute,
    }
    // Le crate moteur n'a pas serde (CONTRAT : aucune dépendance d'app) :
    // la désérialisation vit ici, dans le pont.
    #[derive(serde::Deserialize, Default)]
    struct SourisBrute {
        #[serde(default)]
        gauche: bool,
        #[serde(default)]
        milieu: bool,
        #[serde(default)]
        droite: bool,
        #[serde(default)]
        x: i32,
        #[serde(default)]
        y: i32,
    }
    #[derive(serde::Deserialize, Default)]
    struct ManetteBrute {
        #[serde(default)]
        connectee: bool,
        #[serde(default)]
        boutons: Vec<bool>,
        #[serde(default)]
        axes: Vec<f32>,
    }
    let frame: FrameInput = serde_json::from_str(frame).ok()?;
    if frame.type_frame != "input-viewer-etat" {
        return None;
    }
    // Vecteurs de la frame → tableaux fixes : le payload peut porter moins
    // de 20 boutons / 6 axes (voire `[]`) — padding/troncage tolérant.
    let mut boutons = [false; 20];
    for (i, b) in frame.etat.manette.boutons.iter().enumerate().take(20) {
        boutons[i] = *b;
    }
    let mut axes = [0.0; 6];
    for (i, a) in frame.etat.manette.axes.iter().enumerate().take(6) {
        axes[i] = *a;
    }
    Some(EtatInput {
        touches: frame.etat.touches,
        souris: EtatSouris {
            gauche: frame.etat.souris.gauche,
            milieu: frame.etat.souris.milieu,
            droite: frame.etat.souris.droite,
            x: frame.etat.souris.x,
            y: frame.etat.souris.y,
        },
        manette: EtatManette {
            connectee: frame.etat.manette.connectee,
            boutons,
            axes,
        },
    })
}

/// Extrait une mutation goal d'une frame `chat_tx` — `communaute`,
/// `viewers`, `goal-twitch`, `goal-atteint` (viewers.rs, déjà dédupliquées).
///
/// Même piège de tri des clés que [`decoder_frame_chat`] : test par
/// sous-chaîne, puis validation du champ `type` après désérialisation. Le
/// crate moteur n'a pas serde : la désérialisation vit ici, dans le pont.
///
/// `count: null` (viewers hors-ligne) et les champs communauté absents
/// restent `None` — jamais de valeur fabriquée (« N/A » côté rendu).
fn decoder_frame_goal(frame: &str) -> Option<MajGoal> {
    let interesse = frame.contains("\"type\":\"communaute\"")
        || frame.contains("\"type\":\"viewers\"")
        || frame.contains("\"type\":\"goal-twitch\"")
        || frame.contains("\"type\":\"goal-atteint\"");
    if !interesse {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct FrameGoal {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        plateforme: Option<String>,
        // communaute
        #[serde(default)]
        followers: Option<u64>,
        #[serde(default)]
        subs: Option<u64>,
        #[serde(default)]
        sub_points: Option<u64>,
        #[serde(default)]
        abonnes: Option<u64>,
        // viewers
        #[serde(default)]
        count: Option<u64>,
        // goal-twitch
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        goal_type: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        actuel: Option<u64>,
        #[serde(default)]
        cible: Option<u64>,
        #[serde(default)]
        termine: Option<bool>,
        // goal-atteint
        #[serde(default)]
        widget_id: Option<String>,
    }
    let frame: FrameGoal = serde_json::from_str(frame).ok()?;
    match frame.type_frame.as_str() {
        "communaute" => Some(MajGoal::Communaute {
            plateforme: frame.plateforme?,
            compteurs: Communaute {
                followers: frame.followers,
                subs: frame.subs,
                sub_points: frame.sub_points,
                abonnes: frame.abonnes,
            },
        }),
        "viewers" => Some(MajGoal::Viewers {
            plateforme: frame.plateforme?,
            count: frame.count,
        }),
        "goal-twitch" => Some(MajGoal::Twitch(GoalTwitch {
            id: frame.id?,
            goal_type: frame.goal_type.unwrap_or_default(),
            description: frame.description.unwrap_or_default(),
            actuel: frame.actuel.unwrap_or(0),
            cible: frame.cible.unwrap_or(0),
            termine: frame.termine.unwrap_or(false),
        })),
        "goal-atteint" => Some(MajGoal::Atteint {
            widget_id: frame.widget_id?,
        }),
        _ => None,
    }
}

/// Extrait un event speedrun d'une frame `chat_tx` — `speedrun-etat`
/// (speedrun/commands.rs : le moteur ASL publie `{"type":"speedrun-etat",
/// "etat":<SpeedrunEvent>}`).
///
/// Même motif que [`decoder_frame_goal`] : test par sous-chaîne, validation
/// du champ `type` après désérialisation — le crate moteur n'a pas serde,
/// la désérialisation vit ici. `etat` est l'enum `SpeedrunEvent` taguée
/// `type` (asl_engine.rs) + l'event `lss` construit à la main par
/// `emit_ws_lss_json`. Les events sans effet visuel du splitter (`error`,
/// `settings-list`, `loaded`, `saved`) tombent en `Autre` → `None`, comme
/// le `default:` du switch DOM.
///
/// `segmentTime` de l'event `time` garde sa sémantique tri-état du DOM
/// (`!== undefined`) : absent → `None` (inchangé), `null` → `Some(None)`
/// (effacé), chaîne → `Some(Some(…))`.
fn decoder_frame_speedrun(frame: &str) -> Option<MajSpeedrun> {
    if !frame.contains("\"type\":\"speedrun-etat\"") {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct SegmentBrut {
        nom: String,
        #[serde(rename = "pbRealTime", default)]
        pb_real_time: Option<f64>,
        #[serde(rename = "pbGameTime", default)]
        pb_game_time: Option<f64>,
    }
    #[derive(serde::Deserialize)]
    #[serde(tag = "type")]
    enum EtatBrut {
        #[serde(rename = "started")]
        Started {
            #[serde(default)]
            index: Option<i32>,
        },
        #[serde(rename = "split")]
        Split {
            #[serde(default)]
            time: Option<String>,
            #[serde(default)]
            cumul: Option<String>,
            #[serde(default)]
            index: Option<i32>,
        },
        #[serde(rename = "splitSkipped")]
        SplitSkipped {
            #[serde(default)]
            index: Option<i32>,
        },
        #[serde(rename = "splitUndone")]
        SplitUndone {
            #[serde(default)]
            index: Option<i32>,
        },
        #[serde(rename = "reset")]
        Reset,
        #[serde(rename = "ended")]
        Ended {
            #[serde(default)]
            time: Option<String>,
            #[serde(default)]
            cumul: Option<String>,
            #[serde(default)]
            index: Option<i32>,
        },
        #[serde(rename = "goto")]
        Goto {
            #[serde(default)]
            index: Option<i32>,
            #[serde(default)]
            phase: Option<String>,
        },
        #[serde(rename = "etat")]
        Etat {
            #[serde(default)]
            phase: Option<String>,
            #[serde(default)]
            index: Option<i32>,
            #[serde(rename = "splitTimes", default)]
            split_times: Vec<Option<String>>,
            #[serde(rename = "splitCumuls", default)]
            split_cumuls: Vec<Option<String>>,
            #[serde(rename = "realTime", default)]
            real_time: Option<String>,
            #[serde(rename = "gameTime", default)]
            game_time: Option<String>,
            #[serde(rename = "segmentTime", default)]
            segment_time: Option<String>,
        },
        #[serde(rename = "gameTime")]
        GameTime {
            #[serde(default)]
            time: Option<String>,
        },
        #[serde(rename = "time")]
        Time {
            #[serde(rename = "realTime", default)]
            real_time: Option<String>,
            #[serde(rename = "gameTime", default)]
            game_time: Option<String>,
            #[serde(rename = "segmentTime", default)]
            segment_time: ChampOptionnel<String>,
        },
        #[serde(rename = "paused")]
        Paused,
        #[serde(rename = "resumed")]
        Resumed,
        #[serde(rename = "stopped")]
        Stopped,
        #[serde(rename = "connected")]
        Connected {
            #[serde(default)]
            process: Option<String>,
        },
        #[serde(rename = "disconnected")]
        Disconnected,
        #[serde(rename = "lss")]
        Lss {
            #[serde(rename = "nomJeu", default)]
            nom_jeu: String,
            #[serde(rename = "nomCategorie", default)]
            nom_categorie: String,
            #[serde(default)]
            segments: Vec<SegmentBrut>,
        },
        /// `error`, `settings-list`, `loaded`, `saved` et toute nouveauté :
        /// sans effet visuel sur le widget — ignorées comme le `default:`
        /// du DOM.
        #[serde(other)]
        Autre,
    }
    /// Tri-état JSON (`undefined`/`null`/valeur) : serde mappe `null` comme
    /// l'absence dans `Option<Option<T>>` — ce type décode le champ *quand
    /// il est présent* : `Absent` = non fourni (inchangé), `Present(v)` =
    /// fourni, `v = None` quand `null` (effacé). Sémantique `!== undefined`
    /// du DOM.
    #[derive(Default)]
    enum ChampOptionnel<T> {
        #[default]
        Absent,
        Present(Option<T>),
    }
    impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for ChampOptionnel<T> {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            Option::<T>::deserialize(d).map(Self::Present)
        }
    }
    #[derive(serde::Deserialize)]
    struct FrameSpeedrun {
        #[serde(rename = "type")]
        type_frame: String,
        etat: EtatBrut,
    }
    let frame: FrameSpeedrun = serde_json::from_str(frame).ok()?;
    if frame.type_frame != "speedrun-etat" {
        return None;
    }
    Some(match frame.etat {
        EtatBrut::Started { index } => MajSpeedrun::Started { index },
        EtatBrut::Split { time, cumul, index } => {
            MajSpeedrun::Split { time, cumul, index }
        }
        EtatBrut::SplitSkipped { index } => MajSpeedrun::SplitSkipped { index },
        EtatBrut::SplitUndone { index } => MajSpeedrun::SplitUndone { index },
        EtatBrut::Reset => MajSpeedrun::Reset,
        EtatBrut::Ended { time, cumul, index } => {
            MajSpeedrun::Ended { time, cumul, index }
        }
        EtatBrut::Goto { index, phase } => MajSpeedrun::Goto {
            index,
            phase: phase.map(|p| PhaseSpeedrun::depuis_nom(&p)),
        },
        EtatBrut::Etat {
            phase,
            index,
            split_times,
            split_cumuls,
            real_time,
            game_time,
            segment_time,
        } => MajSpeedrun::Etat {
            phase: PhaseSpeedrun::depuis_nom(phase.as_deref().unwrap_or("")),
            index,
            split_times,
            split_cumuls,
            real_time,
            game_time,
            segment_time,
        },
        EtatBrut::GameTime { time } => MajSpeedrun::GameTime { time: time? },
        EtatBrut::Time {
            real_time,
            game_time,
            segment_time,
        } => MajSpeedrun::Time {
            real_time,
            game_time,
            // `!== undefined` du DOM : champ présent → assigné (null →
            // efface le chrono de segment), absent → inchangé.
            segment_time: match segment_time {
                ChampOptionnel::Absent => None,
                ChampOptionnel::Present(v) => Some(v),
            },
        },
        EtatBrut::Paused => MajSpeedrun::Paused,
        EtatBrut::Resumed => MajSpeedrun::Resumed,
        EtatBrut::Stopped => MajSpeedrun::Stopped,
        EtatBrut::Connected { process } => MajSpeedrun::Connected {
            processus: process,
        },
        EtatBrut::Disconnected => MajSpeedrun::Disconnected,
        EtatBrut::Lss {
            nom_jeu,
            nom_categorie,
            segments,
        } => MajSpeedrun::Lss {
            nom_jeu,
            nom_categorie,
            segments: segments
                .into_iter()
                .map(|s| SegmentLss {
                    nom: s.nom,
                    pb_real_time: s.pb_real_time,
                    pb_game_time: s.pb_game_time,
                })
                .collect(),
        },
        EtatBrut::Autre => return None,
    })
}

/// Extrait une mutation de l'overlay alerte d'une frame `chat_tx` —
/// `alerte-play` / `alerte-stop` / `alerte-config` (squelette
/// `config.overlay`) / `position-overlay-config` (squelette unifié à la
/// racine de `config`, partagé avec le clip de bienvenue — comme
/// `applyAlerteOverlayConfig` du DOM).
///
/// Même motif que [`decoder_frame_goal`] : test par sous-chaîne, puis
/// validation du champ `type` après désérialisation. `media_kind:"video"`
/// n'est pas porté par le moteur → le média n'est transmis que pour une
/// image (limite assumée, `texte/alertes.rs`) ; `son` n'est jamais
/// transmis. Le crate moteur n'a pas serde : la désérialisation vit ici.
fn decoder_frame_alerte(frame: &str) -> Option<MajAlerte> {
    let interesse = frame.contains("\"type\":\"alerte-play\"")
        || frame.contains("\"type\":\"alerte-stop\"")
        || frame.contains("\"type\":\"alerte-config\"")
        || frame.contains("\"type\":\"position-overlay-config\"");
    if !interesse {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct AlerteBrute {
        #[serde(default)]
        texte: Option<String>,
        #[serde(default)]
        taille_px: Option<f32>,
        #[serde(default)]
        duree_ms: Option<u64>,
        #[serde(default)]
        media: Option<String>,
        #[serde(default)]
        media_kind: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct SqueletteBrut {
        x: f64,
        y: f64,
        largeur: f64,
        hauteur: f64,
    }
    #[derive(serde::Deserialize, Default)]
    struct ConfigBrute {
        // `position-overlay-config` : squelette à la racine de `config`.
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        #[serde(default)]
        largeur: Option<f64>,
        #[serde(default)]
        hauteur: Option<f64>,
        // `alerte-config` : squelette dans `config.overlay`.
        #[serde(default)]
        overlay: Option<SqueletteBrut>,
    }
    #[derive(serde::Deserialize)]
    struct FrameAlerte {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        alerte: Option<AlerteBrute>,
        #[serde(default)]
        config: Option<ConfigBrute>,
    }
    let frame: FrameAlerte = serde_json::from_str(frame).ok()?;
    match frame.type_frame.as_str() {
        "alerte-play" => {
            let a = frame.alerte?;
            Some(MajAlerte::Play {
                // Mêmes replis que `playAlerte` sans `alerte-config` :
                // texte vide, taille 48 px, durée 5000 ms.
                texte: a.texte.unwrap_or_default(),
                taille_px: a.taille_px.unwrap_or(48.0),
                duree_ms: a.duree_ms.unwrap_or(5000),
                // Vidéo non portée par ce lot : carte texte seule — le
                // DOM jouerait le `<video>`, le moteur n'a pas de lecteur.
                media: match a.media_kind.as_deref() {
                    Some("video") => None,
                    _ => a.media,
                },
            })
        }
        "alerte-stop" => Some(MajAlerte::Stop),
        "alerte-config" => frame
            .config
            .and_then(|c| c.overlay)
            .map(|s| MajAlerte::Position {
                x: s.x,
                y: s.y,
                largeur: s.largeur,
                hauteur: s.hauteur,
            }),
        "position-overlay-config" => frame.config.and_then(|c| {
            Some(MajAlerte::Position {
                x: c.x?,
                y: c.y?,
                largeur: c.largeur?,
                hauteur: c.hauteur?,
            })
        }),
        _ => None,
    }
}

/// Extrait une mutation de l'overlay clip de bienvenue d'une frame
/// `chat_tx` — `welcome-clip-play` / `welcome-clip-stop` /
/// `welcome-clip-config` (squelette + mode dans `config`) /
/// `position-overlay-config` (squelette seul, partagé avec les alertes —
/// comme `applyWelcomeOverlayConfig` du DOM).
///
/// Même motif que [`decoder_frame_alerte`] : test par sous-chaîne, puis
/// validation du champ `type` après désérialisation. Les médias du clip
/// (`clip.mp4_url`, `viewer.avatar`, `viewer.banniere`, `favoris`) sont des
/// **URLs distantes** : le moteur ne charge que des fichiers locaux → non
/// transmis (limite assumée, `texte/welcome.rs`). Le crate moteur n'a pas
/// serde : la désérialisation vit ici.
fn decoder_frame_welcome(frame: &str) -> Option<MajWelcome> {
    let interesse = frame.contains("\"type\":\"welcome-clip-play\"")
        || frame.contains("\"type\":\"welcome-clip-stop\"")
        || frame.contains("\"type\":\"welcome-clip-config\"")
        || frame.contains("\"type\":\"position-overlay-config\"");
    if !interesse {
        return None;
    }
    #[derive(serde::Deserialize, Default)]
    struct ViewerBrut {
        #[serde(default)]
        login: Option<String>,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        bio: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct SqueletteBrut {
        x: f64,
        y: f64,
        largeur: f64,
        hauteur: f64,
        /// `welcome-clip-config` le porte ; `position-overlay-config` non
        /// (le mode est alors conservé, comme `applyWelcomeOverlayConfig`).
        #[serde(default)]
        utiliser_squelette: Option<bool>,
    }
    #[derive(serde::Deserialize)]
    struct FrameWelcome {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        viewer: Option<ViewerBrut>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        utiliser_squelette: Option<bool>,
        #[serde(default)]
        config: Option<SqueletteBrut>,
    }
    let frame: FrameWelcome = serde_json::from_str(frame).ok()?;
    match frame.type_frame.as_str() {
        "welcome-clip-play" => {
            let viewer = frame.viewer.unwrap_or_default();
            let nom = viewer
                .display_name
                .filter(|n| !n.trim().is_empty())
                .or(viewer.login)
                .unwrap_or_default();
            Some(MajWelcome::Play {
                display_name: nom,
                bio: viewer.bio.filter(|b| !b.trim().is_empty()),
                message: frame.message.unwrap_or_default(),
                // Absent = squelette (défaut du domaine : `utiliser_squelette`
                // vaut `true` dans `ConfigGlobale::default`).
                spotlight: !frame.utiliser_squelette.unwrap_or(true),
            })
        }
        "welcome-clip-stop" => Some(MajWelcome::Stop),
        "welcome-clip-config" | "position-overlay-config" => {
            let c = frame.config?;
            Some(MajWelcome::Config {
                x: c.x,
                y: c.y,
                largeur: c.largeur,
                hauteur: c.hauteur,
                spotlight: c.utiliser_squelette.map(|s| !s),
            })
        }
        _ => None,
    }
}

/// Extrait une mutation du bandeau premier message d'une frame `chat_tx` —
/// `bandeau-premier-message-play` / `-stop` / `-config`.
///
/// Même motif que [`decoder_frame_alerte`] : test par sous-chaîne, puis
/// validation du champ `type`. ⚠️ `bandeau:etat` (event Tauri) est l'état
/// **dashboard** — ce n'est pas le canal du rendu (leçon du lot E1).
/// L'avatar est une URL : l'app précharge son fichier de cache, le moteur ne
/// lit que le disque.
fn decoder_frame_bandeau(frame: &str) -> Option<MajBandeau> {
    let interesse = frame.contains("\"type\":\"bandeau-premier-message-play\"")
        || frame.contains("\"type\":\"bandeau-premier-message-stop\"")
        || frame.contains("\"type\":\"bandeau-premier-message-config\"");
    if !interesse {
        return None;
    }
    #[derive(serde::Deserialize, Default)]
    struct ViewerBrut {
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        avatar: Option<String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct ConfigBrut {
        #[serde(default)]
        position: Option<String>,
        #[serde(default)]
        police: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct FrameBandeau {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        viewer: Option<ViewerBrut>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        config: Option<ConfigBrut>,
    }
    let frame: FrameBandeau = serde_json::from_str(frame).ok()?;
    match frame.type_frame.as_str() {
        "bandeau-premier-message-play" => {
            let viewer = frame.viewer.unwrap_or_default();
            Some(MajBandeau::Play {
                display_name: viewer.display_name.unwrap_or_default(),
                avatar: viewer.avatar.filter(|a| !a.trim().is_empty()),
                message: frame.message.unwrap_or_default(),
            })
        }
        "bandeau-premier-message-stop" => Some(MajBandeau::Stop),
        "bandeau-premier-message-config" => {
            let c = frame.config.unwrap_or_default();
            Some(MajBandeau::Config {
                position: match c.position.as_deref() {
                    Some("haut") => Some(PositionBandeau::Haut),
                    Some("bas") => Some(PositionBandeau::Bas),
                    _ => None,
                },
                police: c.police,
            })
        }
        _ => None,
    }
}

/// Extrait un snapshot complet de l'arène `!fight` d'une frame `chat_tx` —
/// **`fight-state`** (payload `combat`), émise à chaque transition et au
/// resync (`fight-resync`).
///
/// ⚠️ L'event Tauri `fight:update` est l'état **dashboard** (modale + pillule)
/// — ce n'est pas le canal du rendu (même leçon que E1→E4). Les échéances
/// (`reste_*`) sont relatives à l'émission : `EtatFight` les décompte en
/// local, comme le tick 250 ms du DOM.
fn decoder_frame_fight(frame: &str) -> Option<FightState> {
    if !frame.contains("\"type\":\"fight-state\"") {
        return None;
    }
    #[derive(serde::Deserialize, Default)]
    struct JoueurBrut {
        pseudo: Option<String>,
        plateforme: Option<String>,
        emoji: Option<String>,
        pv: Option<u32>,
        pv_max: Option<u32>,
        special_utilise: Option<bool>,
    }
    impl JoueurBrut {
        fn vers(&self) -> JoueurFight {
            let d = JoueurFight::default();
            JoueurFight {
                pseudo: self.pseudo.clone().unwrap_or(d.pseudo),
                plateforme: self.plateforme.clone().unwrap_or(d.plateforme),
                emoji: self.emoji.clone().unwrap_or(d.emoji),
                pv: self.pv.unwrap_or(d.pv),
                pv_max: self.pv_max.unwrap_or(d.pv_max),
                special_utilise: self.special_utilise.unwrap_or(d.special_utilise),
            }
        }
    }
    #[derive(serde::Deserialize, Default)]
    struct CoupBrut {
        seq: Option<u64>,
        acteur: Option<String>,
        coup: Option<String>,
        degats: Option<u32>,
        soin: Option<u32>,
        touche: Option<bool>,
        critique: Option<bool>,
    }
    #[derive(serde::Deserialize, Default)]
    struct InfoBrut {
        seq: Option<u64>,
        kind: Option<String>,
        texte: Option<String>,
        duree_ms: Option<u64>,
        age_ms: Option<u64>,
    }
    #[derive(serde::Deserialize, Default)]
    struct CmdsBrut {
        a: Option<String>,
        b: Option<String>,
        c: Option<String>,
        accept: Option<String>,
        refuse: Option<String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct CombatBrut {
        phase: Option<String>,
        ja: Option<JoueurBrut>,
        jb: Option<JoueurBrut>,
        defiant: Option<JoueurBrut>,
        cible: Option<String>,
        tour: Option<String>,
        reste_tour_ms: Option<u64>,
        duree_tour_ms: Option<u64>,
        reste_match_s: Option<u64>,
        reste_lobby_ms: Option<u64>,
        duree_tour_s: Option<u64>,
        duree_match_s: Option<u64>,
        gagnant: Option<String>,
        raison: Option<String>,
        derniere: Option<CoupBrut>,
        info: Option<InfoBrut>,
        anims_reduites: Option<bool>,
        cmds: Option<CmdsBrut>,
    }
    #[derive(serde::Deserialize)]
    struct FrameFight {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        combat: Option<CombatBrut>,
    }
    let frame: FrameFight = serde_json::from_str(frame).ok()?;
    if frame.type_frame != "fight-state" {
        return None;
    }
    let c = frame.combat.unwrap_or_default();
    let cmds = c.cmds.unwrap_or_default();
    let cmds = CmdsFight {
        a: cmds.a.unwrap_or_default(),
        b: cmds.b.unwrap_or_default(),
        c: cmds.c.unwrap_or_default(),
        accept: cmds.accept.unwrap_or_default(),
        refuse: cmds.refuse.unwrap_or_default(),
    };
    let derniere = c.derniere.map(|d| CoupFight {
        seq: d.seq.unwrap_or(0),
        acteur: match d.acteur.as_deref() {
            Some("b") => 'b',
            _ => 'a',
        },
        coup: d.coup.unwrap_or_default(),
        degats: d.degats.unwrap_or(0),
        soin: d.soin.unwrap_or(0),
        touche: d.touche.unwrap_or(false),
        critique: d.critique.unwrap_or(false),
    });
    let phase = match c.phase.as_deref().unwrap_or("idle") {
        "lobby" => PhaseFight::Lobby {
            defiant: c.defiant.as_ref().map(|j| j.vers()).unwrap_or_default(),
            cible: c.cible.filter(|v| !v.trim().is_empty()),
            reste_lobby_ms: c.reste_lobby_ms.unwrap_or(0),
            duree_tour_s: c.duree_tour_s.unwrap_or(15),
            duree_match_s: c.duree_match_s.unwrap_or(180),
            cmds,
        },
        "combat" => PhaseFight::Combat {
            ja: c.ja.as_ref().map(|j| j.vers()).unwrap_or_default(),
            jb: c.jb.as_ref().map(|j| j.vers()).unwrap_or_default(),
            tour: match c.tour.as_deref() {
                Some("b") => 'b',
                _ => 'a',
            },
            reste_tour_ms: c.reste_tour_ms.unwrap_or(0),
            duree_tour_ms: c.duree_tour_ms.unwrap_or(15_000),
            reste_match_s: c.reste_match_s.unwrap_or(0),
            derniere,
            cmds,
        },
        "fin" => PhaseFight::Fin {
            ja: c.ja.as_ref().map(|j| j.vers()).unwrap_or_default(),
            jb: c.jb.as_ref().map(|j| j.vers()).unwrap_or_default(),
            gagnant: match c.gagnant.as_deref() {
                Some("a") => Some('a'),
                Some("b") => Some('b'),
                _ => None,
            },
            raison: c.raison.unwrap_or_else(|| "ko".to_string()),
        },
        _ => PhaseFight::Idle,
    };
    let info = c.info.map(|i| InfoFight {
        seq: i.seq.unwrap_or(0),
        kind: i.kind.unwrap_or_else(|| "info".to_string()),
        texte: i.texte.unwrap_or_default(),
        duree_ms: i.duree_ms.unwrap_or(2600),
        age_ms: i.age_ms.unwrap_or(0),
    });
    Some(FightState {
        phase,
        info,
        anims_reduites: c.anims_reduites.unwrap_or(false),
    })
}

/// Extrait le **squelette dédié du pop de validation des splits** d'une frame
/// `chat_tx` `splitpop-pos` — miroir de `applySplitPopOverlayConfig`
/// (`diffusion.html`), indépendant du squelette partagé
/// `position-overlay-config` (le pop arrive à chaque split et pouvait
/// chevaucher les alertes).
fn decoder_frame_splitpop(frame: &str) -> Option<MajSplitPop> {
    if !frame.contains("\"type\":\"splitpop-pos\"") {
        return None;
    }
    #[derive(serde::Deserialize, Default)]
    struct ConfigBrut {
        x: Option<f64>,
        y: Option<f64>,
        largeur: Option<f64>,
        hauteur: Option<f64>,
    }
    #[derive(serde::Deserialize)]
    struct FrameSplitPop {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        config: Option<ConfigBrut>,
    }
    let frame: FrameSplitPop = serde_json::from_str(frame).ok()?;
    if frame.type_frame != "splitpop-pos" {
        return None;
    }
    let c = frame.config.unwrap_or_default();
    let (x, y, largeur, hauteur) = sos_moteur_natif::texte::splitpop::POSITION_DEFAUT;
    Some(MajSplitPop::Config {
        x: c.x.unwrap_or(x),
        y: c.y.unwrap_or(y),
        largeur: c.largeur.unwrap_or(largeur),
        hauteur: c.hauteur.unwrap_or(hauteur),
    })
}


/// Extrait une mutation de la **présentation streamer** d'une frame `chat_tx`
/// — `presentation-show` (payload `mode` + `donnees`), `presentation-hide` et
/// `presentation-mode` (bascule à chaud theatrical/card).
///
/// ⚠️ L'event Tauri `presentation:etat` est l'état **dashboard** (favoris +
/// contrôles live) — jamais le canal du rendu (même leçon que E1→E5). Les
/// champs absents retombent sur les défauts du domaine : une frame partielle
/// doit peindre le même overlay que le DOM.
fn decoder_frame_presentation(frame: &str) -> Option<MajPresentation> {
    let interesse = frame.contains("\"type\":\"presentation-show\"")
        || frame.contains("\"type\":\"presentation-hide\"")
        || frame.contains("\"type\":\"presentation-mode\"")
        || frame.contains("\"type\":\"presentation-clip-play\"")
        || frame.contains("\"type\":\"presentation-clip-stop\"")
        || frame.contains("\"type\":\"presentation-audio-play\"")
        || frame.contains("\"type\":\"presentation-audio-stop\"");
    if !interesse {
        return None;
    }
    #[derive(serde::Deserialize, Default)]
    struct JeuBrut {
        id: Option<String>,
        name: Option<String>,
        box_art_url: Option<String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct ClipBrut {
        id: Option<String>,
        title: Option<String>,
        thumbnail_url: Option<String>,
        duration: Option<f64>,
    }
    #[derive(serde::Deserialize, Default)]
    struct DonneesBrut {
        user_id: Option<String>,
        login: Option<String>,
        display_name: Option<String>,
        avatar: Option<String>,
        bio: Option<String>,
        banniere: Option<String>,
        broadcaster_type: Option<String>,
        en_live: Option<bool>,
        viewers_live: Option<u32>,
        titre_live: Option<String>,
        jeu_courant: Option<String>,
        tags: Option<Vec<String>>,
        jeux_recents: Option<Vec<JeuBrut>>,
        clips: Option<Vec<ClipBrut>>,
        mode: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct FramePresentation {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        donnees: Option<DonneesBrut>,
        #[serde(default)]
        mp4_url: Option<String>,
        #[serde(default)]
        titre: Option<String>,
        #[serde(default)]
        duree_ms: Option<u64>,
        #[serde(default)]
        piste: Option<String>,
        #[serde(default)]
        src: Option<String>,
    }
    let frame: FramePresentation = serde_json::from_str(frame).ok()?;
    match frame.type_frame.as_str() {
        "presentation-show" => {
            let d = frame.donnees.unwrap_or_default();
            // `msg.mode` du DOM ; repli sur `donnees.mode` (même valeur, mais
            // une frame tronquée ne doit pas changer de mode).
            let mode = frame
                .mode
                .or(d.mode.clone())
                .unwrap_or_else(|| ModePresentation::Theatrical.nom().to_string());
            Some(MajPresentation::Show {
                mode: ModePresentation::depuis_nom(&mode),
                donnees: Box::new(PresentationDonnees {
                    user_id: d.user_id.unwrap_or_default(),
                    login: d.login.unwrap_or_default(),
                    display_name: d.display_name.unwrap_or_default(),
                    avatar: d.avatar.unwrap_or_default(),
                    bio: d.bio.unwrap_or_default(),
                    banniere: d.banniere.unwrap_or_default(),
                    broadcaster_type: d.broadcaster_type.unwrap_or_default(),
                    en_live: d.en_live.unwrap_or(false),
                    viewers_live: d.viewers_live,
                    titre_live: d.titre_live.unwrap_or_default(),
                    jeu_courant: d.jeu_courant.unwrap_or_default(),
                    tags: d.tags.unwrap_or_default(),
                    jeux_recents: d
                        .jeux_recents
                        .unwrap_or_default()
                        .into_iter()
                        .map(|j| JeuRecent {
                            id: j.id.unwrap_or_default(),
                            name: j.name.unwrap_or_default(),
                            box_art_url: j.box_art_url.unwrap_or_default(),
                        })
                        .collect(),
                    clips: d
                        .clips
                        .unwrap_or_default()
                        .into_iter()
                        .map(|c| ClipPresentation {
                            id: c.id.unwrap_or_default(),
                            title: c.title.unwrap_or_default(),
                            thumbnail_url: c.thumbnail_url.unwrap_or_default(),
                            duration: c.duration.unwrap_or(0.0),
                        })
                        .collect(),
                    mode,
                }),
            })
        }
        "presentation-hide" => Some(MajPresentation::Hide),
        "presentation-mode" => Some(MajPresentation::Mode(ModePresentation::depuis_nom(
            frame.mode.as_deref()?,
        ))),
        // `mp4_url` = URL **signée** distante : elle sert de clé au fichier
        // préchargé par l'app (`medias/clips/<fnv16(url)>.mp4`), le crate ne
        // fait aucun réseau. Vide = frame ignorée (rien à jouer).
        "presentation-clip-play" => {
            let mp4_url = frame.mp4_url.filter(|u| !u.trim().is_empty())?;
            Some(MajPresentation::ClipPlay {
                mp4_url,
                titre: frame.titre.unwrap_or_default(),
                duree_ms: frame.duree_ms.unwrap_or(0),
            })
        }
        "presentation-clip-stop" => Some(MajPresentation::ClipStop),
        // `src` = `medias/…` : fichier **local**, aucune sortie réseau.
        // Piste inconnue ou `src` vide → frame ignorée (jamais de piste
        // inventée).
        "presentation-audio-play" => {
            let piste = PistePresentation::depuis_nom(frame.piste.as_deref()?)?;
            let src = frame.src.filter(|s| !s.trim().is_empty())?;
            Some(MajPresentation::AudioPlay { piste, src })
        }
        "presentation-audio-stop" => {
            let piste = frame.piste.as_deref()?;
            if piste == "toutes" {
                Some(MajPresentation::AudioStop(None))
            } else {
                Some(MajPresentation::AudioStop(Some(
                    PistePresentation::depuis_nom(piste)?,
                )))
            }
        }
        _ => None,
    }
}

/// Extrait une mutation du **pad numérique** d'une frame `chat_tx` —
/// `pad-play` (payload `pad`), `pad-stop`, et `position-overlay-config`
/// (squelette **partagé** welcome + alertes + commandes + pad).
///
/// Même motif que [`decoder_frame_alerte`] : test par sous-chaîne, puis
/// validation du champ `type`. ⚠️ Les events Tauri `pad:touche` /
/// `pad:touche-stop` sont l'état **dashboard** (audio local + retour UI) —
/// ce n'est pas le canal du rendu. Les chemins sont relatifs à la base
/// (`medias/…`) : le moteur ne lit que des fichiers locaux.
fn decoder_frame_pad(frame: &str) -> Option<MajPad> {
    let interesse = frame.contains("\"type\":\"pad-play\"")
        || frame.contains("\"type\":\"pad-stop\"")
        || frame.contains("\"type\":\"position-overlay-config\"");
    if !interesse {
        return None;
    }
    #[derive(serde::Deserialize, Default)]
    struct PadBrut {
        media_type: Option<String>,
        media: Option<String>,
        media_kind: Option<String>,
        son: Option<String>,
        volume: Option<f64>,
        duree_ms: Option<u64>,
        bounce: Option<bool>,
        zoom: Option<bool>,
        plein_ecran: Option<bool>,
        transparence: Option<bool>,
        transparence_opacite: Option<f64>,
    }
    #[derive(serde::Deserialize, Default)]
    struct ConfigBrut {
        x: Option<f64>,
        y: Option<f64>,
        largeur: Option<f64>,
        hauteur: Option<f64>,
    }
    #[derive(serde::Deserialize)]
    struct FramePad {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        pad: Option<PadBrut>,
        #[serde(default)]
        config: Option<ConfigBrut>,
    }
    let frame: FramePad = serde_json::from_str(frame).ok()?;
    match frame.type_frame.as_str() {
        "pad-play" => {
            let p = frame.pad.unwrap_or_default();
            let defaut = PadPlay::default();
            Some(MajPad::Play(PadPlay {
                media_type: p.media_type.unwrap_or(defaut.media_type),
                media: p.media.filter(|m| !m.trim().is_empty()),
                media_kind: p.media_kind,
                son: p.son.filter(|s| !s.trim().is_empty()),
                volume: p.volume.map(|v| v as f32).unwrap_or(defaut.volume),
                duree_ms: p.duree_ms.unwrap_or(defaut.duree_ms),
                bounce: p.bounce.unwrap_or(defaut.bounce),
                zoom: p.zoom.unwrap_or(defaut.zoom),
                plein_ecran: p.plein_ecran.unwrap_or(defaut.plein_ecran),
                transparence: p.transparence.unwrap_or(defaut.transparence),
                transparence_opacite: p
                    .transparence_opacite
                    .map(|v| v as f32)
                    .unwrap_or(defaut.transparence_opacite),
            }))
        }
        "pad-stop" => Some(MajPad::Stop),
        "position-overlay-config" => {
            let c = frame.config.unwrap_or_default();
            let (x, y, largeur, hauteur) = sos_moteur_natif::texte::POSITION_PAD_DEFAUT;
            Some(MajPad::Config {
                x: c.x.unwrap_or(x),
                y: c.y.unwrap_or(y),
                largeur: c.largeur.unwrap_or(largeur),
                hauteur: c.hauteur.unwrap_or(hauteur),
            })
        }
        _ => None,
    }
}

/// Extrait l'état complet de l'écran d'attente d'une frame `chat_tx` —
/// `ecran-attente-etat` (payload `etat`, ré-émis à chaque tick 1 Hz du chrono
/// Rust : la frame est auto-réparatrice, le moteur remplace son état).
///
/// ⚠️ `ecran-attente:etat` (event Tauri) est l'état **dashboard** (modale +
/// pillule) — ce n'est pas le canal du rendu (même leçon que les lots E1/E2).
/// Champs absents → défauts du domaine (`AttenteCfg::default`), jamais des
/// zéros : une frame partielle doit peindre le même écran que le DOM.
fn decoder_frame_attente(frame: &str) -> Option<AttenteCfg> {
    if !frame.contains("\"type\":\"ecran-attente-etat\"") {
        return None;
    }
    #[derive(serde::Deserialize, Default)]
    struct EtatBrut {
        actif: Option<bool>,
        en_pause: Option<bool>,
        temps_restant: Option<u64>,
        titre: Option<String>,
        afficher_titre: Option<bool>,
        son: Option<bool>,
        volume: Option<f64>,
        media: Option<String>,
        media_kind: Option<String>,
        minuteur_x: Option<f64>,
        minuteur_y: Option<f64>,
        minuteur_echelle: Option<f64>,
        layout: Option<String>,
        streamy_x: Option<f64>,
        streamy_y: Option<f64>,
        streamy_echelle: Option<f64>,
        logo_x: Option<f64>,
        logo_y: Option<f64>,
        logo_echelle: Option<f64>,
        muter_bureau: Option<bool>,
    }
    #[derive(serde::Deserialize)]
    struct FrameAttente {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        etat: Option<EtatBrut>,
    }
    let frame: FrameAttente = serde_json::from_str(frame).ok()?;
    if frame.type_frame != "ecran-attente-etat" {
        return None;
    }
    let brut = frame.etat.unwrap_or_default();
    let defaut = AttenteCfg::default();
    let mut cfg = defaut.clone();
    cfg.actif = brut.actif.unwrap_or(defaut.actif);
    cfg.en_pause = brut.en_pause.unwrap_or(defaut.en_pause);
    cfg.temps_restant = brut.temps_restant.unwrap_or(defaut.temps_restant);
    cfg.titre = brut.titre.unwrap_or(defaut.titre);
    cfg.afficher_titre = brut.afficher_titre.unwrap_or(defaut.afficher_titre);
    cfg.son = brut.son.unwrap_or(defaut.son);
    cfg.volume = brut.volume.map(|v| v as f32).unwrap_or(defaut.volume);
    cfg.media = brut.media.filter(|m| !m.trim().is_empty());
    cfg.media_kind = brut.media_kind;
    cfg.minuteur = (
        brut.minuteur_x.unwrap_or(defaut.minuteur.0),
        brut.minuteur_y.unwrap_or(defaut.minuteur.1),
        brut.minuteur_echelle
            .unwrap_or(defaut.minuteur.2)
            .clamp(0.4, 3.0),
    );
    // `layout !== "minimal"` = layout « signature » (marque StreamOS).
    cfg.signature = brut
        .layout
        .map(|l| l != "minimal")
        .unwrap_or(defaut.signature);
    cfg.streamy = (
        brut.streamy_x.unwrap_or(defaut.streamy.0),
        brut.streamy_y.unwrap_or(defaut.streamy.1),
        brut.streamy_echelle
            .unwrap_or(defaut.streamy.2)
            .clamp(0.3, 2.0),
    );
    cfg.logo = (
        brut.logo_x.unwrap_or(defaut.logo.0),
        brut.logo_y.unwrap_or(defaut.logo.1),
        brut.logo_echelle
            .unwrap_or(defaut.logo.2)
            .clamp(0.3, 2.0),
    );
    cfg.muter_bureau = brut.muter_bureau.unwrap_or(defaut.muter_bureau);
    Some(cfg)
}

/// Extrait un snapshot musique du payload de l'event Tauri `musique:update`
/// (`MusiqueState::emit_update` → `etat()` : `en_cours` + `progression`
/// figée à l'emit — le lecteur natif l'avance en local, comme le DOM).
///
/// Contrairement aux décodeurs `chat_tx`, le payload n'a **pas** de champ
/// `type` : le nom de l'event Tauri le porte déjà — pas de filtre par
/// sous-chaîne. `en_cours: null` = lecteur à l'arrêt ; `progression: null`
/// (aucune piste) → position 0, non-pause, durée inconnue, comme l'état
/// initial du DOM. Le crate moteur n'a pas serde : la désérialisation vit
/// ici, dans le pont.
fn decoder_payload_musique(payload: &str) -> Option<MajMusique> {
    #[derive(serde::Deserialize, Default)]
    struct ProgressionBrute {
        #[serde(default)]
        position_ms: u64,
        #[serde(default)]
        pausee: bool,
        #[serde(default)]
        duree_s: Option<f64>,
    }
    #[derive(serde::Deserialize)]
    struct PisteBrute {
        id: String,
        titre: String,
        demandeur: String,
    }
    #[derive(serde::Deserialize, Default)]
    struct EtatMusiqueBrut {
        #[serde(default)]
        en_cours: Option<PisteBrute>,
        #[serde(default)]
        progression: Option<ProgressionBrute>,
    }
    let etat: EtatMusiqueBrut = serde_json::from_str(payload).ok()?;
    let progression = etat.progression.unwrap_or_default();
    Some(MajMusique {
        en_cours: etat.en_cours.map(|p| Piste {
            id: p.id,
            titre: p.titre,
            demandeur: p.demandeur,
        }),
        position_ms: progression.position_ms,
        pausee: progression.pausee,
        duree_s: progression.duree_s,
    })
}

/// Extrait la scène d'une frame `snapshot` (`{"scene":…,"sceneId":…,"type":…}`).
///
/// Même piège de tri des clés que [`decoder_frame_chat`] : test par
/// sous-chaîne, puis validation du champ `type` après désérialisation.
/// La scène de la frame porte déjà l'override « !cadres » (injecté par
/// `snapshot_json`) : le moteur applique exactement ce que la diffusion
/// affiche.
fn decoder_frame_snapshot(frame: &str) -> Option<sos_scene::Scene> {
    if !frame.contains("\"type\":\"snapshot\"") {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct FrameSnapshot {
        #[serde(rename = "type")]
        type_frame: String,
        scene: sos_scene::Scene,
    }
    let frame: FrameSnapshot = serde_json::from_str(frame).ok()?;
    (frame.type_frame == "snapshot").then_some(frame.scene)
}

/// Pousse un message dans le moteur natif (harnais, tests, intégrations).
///
/// Le chemin de production est [`demarrer_ecoute_chat`] : cette commande
/// existe pour injecter un message sans passer par une plateforme.
#[tauri::command]
pub fn moteur_chat_message(
    registre: State<'_, RegistreMoteur>,
    plateforme: String,
    pseudo: String,
    texte: String,
    couleur: Option<String>,
) {
    registre.natif().pousser_chat(MessageChat::nouveau(
        plateforme,
        pseudo,
        texte,
        couleur.as_deref(),
    ));
}

/// Applique une scène **fournie** au moteur natif (lot F — Just Chatting).
///
/// Le mode JC est **non destructif** : la scène persistée n'est jamais mutée,
/// la dérivée vit en mémoire côté frontend. OBS reçoit cette copie par
/// `scene_sync_captures` ; le moteur natif la reçoit ici, par le même chemin
/// qu'une scène appliquée à chaud (`MoteurNatif::appliquer_scene`, application
/// différentielle : ni session de capture ni encodeur recréés). Aucune
/// écriture disque, aucun état métier dupliqué (règle « une seule vérité de
/// scène »).
#[tauri::command]
pub fn moteur_appliquer_scene(registre: State<'_, RegistreMoteur>, scene: sos_scene::Scene) {
    registre.natif().appliquer_scene(&scene);
}

/// Extrait les bandes d'une frame `eq-spectrum` du canal `chat_tx` —
/// `{"type":"eq-spectrum","bandes":[24 floats 0..1]}` (equaliseur.rs :
/// capture système/micro → FFT → spectre normalisé, ~30 Hz).
///
/// Même motif que [`decoder_frame_goal`] : test par sous-chaîne puis
/// validation du `type` et du nombre de bandes — le crate moteur n'a
/// pas serde, la désérialisation vit ici.
fn decoder_frame_eq(frame: &str) -> Option<[f32; 24]> {
    if !frame.contains("\"type\":\"eq-spectrum\"") {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct FrameEq {
        #[serde(rename = "type")]
        type_frame: String,
        #[serde(default)]
        bandes: Vec<f32>,
    }
    let frame: FrameEq = serde_json::from_str(frame).ok()?;
    if frame.type_frame != "eq-spectrum" || frame.bandes.len() != 24 {
        return None;
    }
    let mut bandes = [0.0f32; 24];
    bandes.copy_from_slice(&frame.bandes[..24]);
    Some(bandes)
}

/// Abonne le moteur natif au flux chat de l'app (`chat_tx`).
///
/// Le crate du moteur reste **sans tokio** : c'est l'app qui écoute et pousse.
/// Un seul abonné pour les 4 plateformes, aucun accès réseau supplémentaire.
pub fn demarrer_ecoute_chat(app: AppHandle) {
    let Some(state) = app.try_state::<crate::scenes::ScenesState>() else {
        log::warn!("chat moteur : état des scènes absent, pont non démarré");
        return;
    };
    let mut rx = state.chat_tx.subscribe();
    // L'état live musique ne transite PAS par chat_tx : `emit_update` émet
    // l'event Tauri `musique:update` (dashboard). Le backend s'y abonne
    // comme les webviews — même recette que `telemetrie/collecte.rs`.
    let app_mu = app.clone();
    app.listen_any("musique:update", move |event| {
        if let Some(maj) = decoder_payload_musique(event.payload()) {
            if let Some(registre) = app_mu.try_state::<RegistreMoteur>() {
                registre.natif().pousser_musique(maj);
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    // `position-overlay-config` est PARTAGÉ (welcome + alertes
                    // + commandes + pad) : `appliquerSnapshot` du DOM
                    // l'applique aux deux overlays. Il alimente donc les deux
                    // états ici, avant la chaîne exclusive.
                    if frame.contains("\"type\":\"position-overlay-config\"") {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            if let Some(maj) = decoder_frame_alerte(&frame) {
                                registre.natif().pousser_alerte(maj);
                            }
                            if let Some(maj) = decoder_frame_welcome(&frame) {
                                registre.natif().pousser_welcome(maj);
                            }
                            if let Some(maj) = decoder_frame_pad(&frame) {
                                registre.natif().pousser_pad(maj);
                            }
                        }
                        continue;
                    }
                    if let Some(message) = decoder_frame_chat(&frame) {
                        // Images du chat (avatars + emotes) : le moteur ne
                        // lit que des fichiers locaux — l'app télécharge
                        // ici, à la volée (lot G). `precharger` ne bloque
                        // jamais : le 1er rendu retombe sur le texte.
                        crate::cache_images::precharger_urls(
                            &app,
                            message
                                .avatar
                                .iter()
                                .map(String::as_str)
                                .chain(message.emotes.iter().map(|e| e.url.as_str())),
                        );
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_chat(message);
                        }
                    } else if let Some(touches) = decoder_frame_input(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_input(touches);
                        }
                    } else if let Some(maj) = decoder_frame_goal(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_goal(maj);
                        }
                    } else if let Some(maj) = decoder_frame_splitpop(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_splitpop(maj);
                        }
                    } else if let Some(maj) = decoder_frame_speedrun(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_speedrun(maj);
                        }
                    } else if let Some(maj) = decoder_frame_alerte(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_alerte(maj);
                        }
                    } else if let Some(maj) = decoder_frame_welcome(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_welcome(maj);
                        }
                    } else if let Some(maj) = decoder_frame_bandeau(&frame) {
                        // Avatar du bandeau : précharge du cache disque (le
                        // moteur ne lit que des fichiers locaux).
                        if let MajBandeau::Play { avatar: Some(url), .. } = &maj {
                            crate::cache_images::precharger(&app, url);
                        }
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_bandeau(maj);
                        }
                    } else if let Some(cfg) = decoder_frame_attente(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_attente(cfg);
                        }
                    } else if let Some(maj) = decoder_frame_pad(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_pad(maj);
                        }
                    } else if let Some(etat) = decoder_frame_fight(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_fight(etat);
                        }
                    } else if let Some(maj) = decoder_frame_presentation(&frame) {
                        // Images distantes de la présentation (bannière, avatar,
                        // boîtes de jeux) : cache disque — le moteur ne lit que
                        // des fichiers locaux (§13.8). Le décor retombe sur le
                        // dégradé / l'absence d'image tant que le GET n'a pas
                        // abouti, puis se répare seul (re-tentative 500 ms).
                        if let MajPresentation::Show { donnees, .. } = &maj {
                            crate::cache_images::precharger(&app, &donnees.banniere);
                            crate::cache_images::precharger(&app, &donnees.avatar);
                            // Les `box_art_url` Helix portent `{width}x{height}` :
                            // même substitution que le moteur (`url_box_art`),
                            // sinon les deux côtés n'ont pas la même empreinte.
                            let boites: Vec<String> = donnees
                                .jeux_recents
                                .iter()
                                .map(|j| url_box_art(&j.box_art_url))
                                .collect();
                            crate::cache_images::precharger_urls(
                                &app,
                                boites.iter().map(String::as_str),
                            );
                            // Vignettes de la grille 2×2 (même hôte que le MP4).
                            crate::cache_images::precharger_urls(
                                &app,
                                donnees
                                    .clips
                                    .iter()
                                    .take(4)
                                    .map(|c| c.thumbnail_url.as_str()),
                            );
                        }
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_presentation(maj);
                        }
                    } else if let Some(bandes) = decoder_frame_eq(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_eq(bandes);
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("chat moteur : {n} frame(s) non traitées");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Applique la scène à chaud dans le moteur natif à chaque mutation
/// (`snapshot_tx`) : la session aperçu — comme un enregistrement — reflète
/// la scène courante sans être tuée ni relancée (application différentielle,
/// trait `Moteur`). Hors session, l'application ne fait que mettre à jour
/// la configuration consommée par le prochain démarrage — idempotent.
pub fn demarrer_veille_apercu(app: AppHandle) {
    let Some(state) = app.try_state::<crate::scenes::ScenesState>() else {
        log::warn!("veille aperçu : état des scènes absent, veille non démarrée");
        return;
    };
    let mut rx = state.snapshot_tx.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    let Some(scene) = decoder_frame_snapshot(&frame) else {
                        continue;
                    };
                    if let Some(registre) = app.try_state::<RegistreMoteur>() {
                        // `medias/` est relatif à `base` — posée par
                        // `configurer_scene`, elle peut manquer si le
                        // snapshot précède toute session : la fixer garantit
                        // la résolution des chemins médias.
                        if let Ok(base) = crate::config::data_dir(&app) {
                            registre.natif().configurer_base(&base);
                        }
                        registre.natif().appliquer_scene(&scene);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Aperçu natif : dernière frame réduite du moteur (RGBA8, cadence et
/// résolution réglables — bloc `apercu` de moteur.json).
///
/// Effet de bord : si ni enregistrement ni session aperçu ne tournent, la
/// commande **démarre** la session aperçu (fenêtre du projecteur ouverte =
/// la demande — aucun autre déclencheur, rien ne compose pour rien). Un
/// enregistrement actif alimente déjà l'aperçu partagé : les frames sont
/// servies telles quelles.
///
/// Corps binaire : `[w u32 LE][h u32 LE][pts_us i64 LE][pixels RGBA]` —
/// vide tant qu'aucune frame n'a été produite, **ou** quand `depuis` est
/// déjà le `pts_us` courant (la page n'a rien à retélécharger : le tampon
/// n'est plus recopié ni transféré sur une scène figée). Réservé au mode
/// Rust (`moteur_basculer` arrête la session en quittant le mode natif).
#[tauri::command]
pub fn moteur_apercu(
    app: AppHandle,
    registre: State<'_, RegistreMoteur>,
    scenes: State<'_, crate::scenes::ScenesState>,
    depuis: Option<i64>,
) -> Result<tauri::ipc::Response, String> {
    if registre.mode() != Mode::Natif {
        return Err("l'aperçu du projecteur exige le mode Rust".to_string());
    }
    let natif = registre.natif();
    if !natif.en_cours() && !natif.apercu_en_cours() {
        if let Some(erreur) = natif.apercu_erreur() {
            return Err(erreur);
        }
        let scene = scenes
            .scene
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let base = crate::config::data_dir(&app)?;
        natif
            .configurer_scene(&scene, &HashMap::new(), &base)
            .map_err(|e| e.to_string())?;
        // La composition suit la cadence d'émission réglée : à 10 fps
        // d'aperçu, composer 30 fois par seconde ne produirait que du
        // travail jeté (passe D2D plein canvas + copies WGC).
        let cfg = apercu_cfg_persistee(&app);
        natif
            .demarrer_apercu((scene.canvasW, scene.canvasH), cfg.fps.clamp(1, 60), cfg)
            .map_err(|e| e.to_string())?;
    }
    let frame = match natif.apercu() {
        Some(f) if f.valide() => f,
        _ => return Ok(tauri::ipc::Response::new(Vec::new())),
    };
    if depuis == Some(frame.pts_us) {
        return Ok(tauri::ipc::Response::new(Vec::new()));
    }
    let mut corps = Vec::with_capacity(16 + frame.data.len());
    corps.extend_from_slice(&frame.largeur.to_le_bytes());
    corps.extend_from_slice(&frame.hauteur.to_le_bytes());
    corps.extend_from_slice(&frame.pts_us.to_le_bytes());
    corps.extend_from_slice(&frame.data);
    Ok(tauri::ipc::Response::new(corps))
}

/// Ouvre ou ferme le projecteur natif (toggle) — équivalent du projecteur
/// OBS en mode Rust : fenêtre détachée, déplaçable, maximisable, qui
/// affiche l'aperçu réduit du moteur (~10 fps, composition sans encodeur).
///
/// Navigation : custom protocol `streamos-apercu://localhost/` → HTML
/// embarqué servi par `register_uri_scheme_protocol` (même recette que
/// chat/speedpop — PAS d'about:blank, PAS d'URL :4321 directe).
/// ASYNC obligatoire : sur Windows, créer une webview dans une commande
/// sync deadlock Webview2 (`chat_popout_toggle`).
#[tauri::command]
pub async fn apercu_fenetre_toggle(
    app: AppHandle,
    registre: State<'_, RegistreMoteur>,
) -> Result<bool, String> {
    use tauri::webview::WebviewWindowBuilder;

    const LABEL: &str = "apercu-natif";

    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
        return Ok(false);
    }
    if registre.mode() != Mode::Natif {
        return Err("le projecteur natif exige le mode Rust".to_string());
    }
    // Acquitte une éventuelle erreur/session précédente avant le premier poll.
    registre.natif().arreter_apercu();

    let url = tauri::Url::parse("streamos-apercu://localhost/")
        .map_err(|e| format!("URL invalide : {e}"))?;
    let win = WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::CustomProtocol(url))
        .title("Aperçu — StreamOS")
        .inner_size(640.0, 360.0)
        .resizable(true)
        .decorations(true)
        .build()
        .map_err(|e| format!("Création fenêtre aperçu : {e}"))?;

    // Fenêtre détruite → la session aperçu s'arrête : plus aucun poll ne
    // viendra, le moteur ne doit pas continuer à composer pour rien.
    let app_clone = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            if let Some(registre) = app_clone.try_state::<RegistreMoteur>() {
                registre.natif().arreter_apercu();
            }
        }
    });

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_chat_accepte_une_frame_serde_json() {
        // `chat_tx` transporte des frames construites par `serde_json::json!`.
        // Sans `preserve_order`, les clés sortent **triées** (`message` avant
        // `type`) : un filtre `starts_with("{\"type\"…")` ne matche jamais.
        let frame = serde_json::json!({
            "type": "chat",
            "message": {
                "plateforme": "twitch",
                "pseudo": "viewer",
                "texte": "salut",
            },
        })
        .to_string();
        assert!(
            decoder_frame_chat(&frame).is_some(),
            "frame non reconnue : {frame}"
        );
    }

    #[test]
    fn decoder_chat_medias_badges_avatar_emotes() {
        let frame = serde_json::json!({
            "type": "chat",
            "message": {
                "plateforme": "twitch",
                "pseudo": "Ryo",
                "texte": "Kappa salut",
                "color": "#ff0000",
                "badges": "moderator/1,subscriber/12",
                "avatar": "https://static-cdn.jtvnw.net/av.png",
                "emotes": [{
                    "debut": 0, "fin": 5,
                    "url": "https://static-cdn.jtvnw.net/emoticons/v2/25/default/dark/2.0",
                    "nom": "Kappa",
                }],
            },
        })
        .to_string();
        let m = decoder_frame_chat(&frame).expect("message décodé");
        assert_eq!(m.couleur, Some([255, 0, 0, 255]));
        assert_eq!(m.badges.as_deref(), Some("moderator/1,subscriber/12"));
        assert_eq!(
            m.avatar.as_deref(),
            Some("https://static-cdn.jtvnw.net/av.png")
        );
        assert_eq!(m.emotes.len(), 1);
        assert_eq!((m.emotes[0].debut, m.emotes[0].fin), (0, 5));
        assert_eq!(m.emotes[0].nom, "Kappa");
    }

    #[test]
    fn decoder_chat_sans_medias_reste_valide() {
        // Frame minimale (autres plateformes : pas de badges/emotes).
        let frame = serde_json::json!({
            "type": "chat",
            "message": {"plateforme": "kick", "pseudo": "a", "texte": "b"},
        })
        .to_string();
        let m = decoder_frame_chat(&frame).expect("message décodé");
        assert!(m.badges.is_none());
        assert!(m.avatar.is_none());
        assert!(m.emotes.is_empty());
    }

    #[test]
    fn decoder_bandeau_play_stop_config() {
        let play = serde_json::json!({
            "type": "bandeau-premier-message-play",
            "viewer": {
                "display_name": "Manu",
                "avatar": "https://static-cdn.jtvnw.net/a.png",
                "plateforme": "twitch",
            },
            "message": "Salut !",
        })
        .to_string();
        assert_eq!(
            decoder_frame_bandeau(&play),
            Some(MajBandeau::Play {
                display_name: "Manu".to_string(),
                avatar: Some("https://static-cdn.jtvnw.net/a.png".to_string()),
                message: "Salut !".to_string(),
            })
        );
        let stop = serde_json::json!({"type": "bandeau-premier-message-stop"}).to_string();
        assert_eq!(decoder_frame_bandeau(&stop), Some(MajBandeau::Stop));
        let config = serde_json::json!({
            "type": "bandeau-premier-message-config",
            "config": {"duree_ms": 6000, "position": "haut", "police": "Anton"},
        })
        .to_string();
        assert_eq!(
            decoder_frame_bandeau(&config),
            Some(MajBandeau::Config {
                position: Some(PositionBandeau::Haut),
                police: Some("Anton".to_string()),
            })
        );
    }

    #[test]
    fn decoder_bandeau_ignore_les_autres_frames() {
        for frame in [
            r#"{"type":"chat","message":{}}"#,
            r#"{"type":"bandeau:etat"}"#,
            r#"{"type":"welcome-clip-play"}"#,
            "pas du json",
        ] {
            assert!(decoder_frame_bandeau(frame).is_none(), "frame : {frame}");
        }
    }

    #[test]
    fn decoder_attente_frame_complete() {
        let frame = serde_json::json!({
            "type": "ecran-attente-etat",
            "etat": {
                "actif": true,
                "en_pause": true,
                "temps_restant": 3599,
                "duree_s": 3600,
                "titre": "On revient",
                "afficher_titre": true,
                "son": false,
                "volume": 0.75,
                "media": "medias/a.mp4",
                "media_kind": "video",
                "minuteur_x": 800.0,
                "minuteur_y": 400.0,
                "minuteur_echelle": 9.0,
                "layout": "minimal",
                "streamy_x": 100.0,
                "streamy_y": 120.0,
                "streamy_echelle": 9.0,
                "logo_x": 900.0,
                "logo_y": 100.0,
                "logo_echelle": 0.5,
                "muter_bureau": false,
            },
        })
        .to_string();
        let cfg = decoder_frame_attente(&frame).expect("frame attente reconnue");
        assert!(cfg.actif && cfg.en_pause);
        assert_eq!(cfg.temps_restant, 3599);
        assert_eq!(cfg.titre, "On revient");
        assert!(!cfg.son);
        assert!((cfg.volume - 0.75).abs() < 1e-6);
        assert_eq!(cfg.media.as_deref(), Some("medias/a.mp4"));
        assert_eq!(cfg.media_kind.as_deref(), Some("video"));
        // Bornes du domaine : échelles clampées, position libre.
        assert_eq!(cfg.minuteur, (800.0, 400.0, 3.0));
        assert_eq!(cfg.streamy, (100.0, 120.0, 2.0));
        assert_eq!(cfg.logo, (900.0, 100.0, 0.5));
        assert!(!cfg.signature, "layout minimal → pas de marque");
        assert!(!cfg.muter_bureau);
    }

    #[test]
    fn decoder_attente_frame_partielle_prend_les_defauts() {
        // Une frame sans `etat` ni champ optionnel : défauts du domaine, et
        // jamais un écran actif par accident.
        let frame = serde_json::json!({"type": "ecran-attente-etat"}).to_string();
        let cfg = decoder_frame_attente(&frame).expect("frame reconnue");
        let defaut = AttenteCfg::default();
        assert!(!cfg.actif);
        assert_eq!(cfg.minuteur, defaut.minuteur);
        assert_eq!(cfg.streamy, defaut.streamy);
        assert_eq!(cfg.logo, defaut.logo);
        assert!(cfg.signature, "layout signature par défaut");
        assert!(cfg.muter_bureau);
        assert_eq!(cfg.media, None);
        // `layout: "signature"` explicite → signature aussi.
        let frame = serde_json::json!({
            "type": "ecran-attente-etat",
            "etat": {"actif": true, "layout": "signature"},
        })
        .to_string();
        assert!(decoder_frame_attente(&frame).expect("reconnue").signature);
        // Média vide = aucun média (jamais un chemin vide joint à la base).
        let frame = serde_json::json!({
            "type": "ecran-attente-etat",
            "etat": {"actif": true, "media": ""},
        })
        .to_string();
        assert_eq!(decoder_frame_attente(&frame).expect("reconnue").media, None);
    }

    #[test]
    fn decoder_attente_ignore_les_autres_frames() {
        for frame in [
            r#"{"type":"chat","message":{}}"#,
            r#"{"type":"ecran-attente:etat","actif":true}"#,
            r#"{"type":"bandeau-premier-message-stop"}"#,
            "pas du json",
        ] {
            assert!(decoder_frame_attente(frame).is_none(), "frame : {frame}");
        }
    }

    #[test]
    fn decoder_pad_play_stop_et_position() {
        let play = serde_json::json!({
            "type": "pad-play",
            "pad": {
                "media_type": "video",
                "media": "medias/a.mp4",
                "media_kind": "video",
                "son": "medias/son.mp3",
                "volume": 0.35,
                "duree_ms": 4200,
                "bounce": true,
                "zoom": false,
                "plein_ecran": true,
                "transparence": true,
                "transparence_opacite": 0.4,
            },
        })
        .to_string();
        let Some(MajPad::Play(p)) = decoder_frame_pad(&play) else {
            panic!("frame pad-play reconnue");
        };
        assert_eq!(p.media_type, "video");
        assert_eq!(p.media.as_deref(), Some("medias/a.mp4"));
        assert_eq!(p.media_kind.as_deref(), Some("video"));
        assert_eq!(p.son.as_deref(), Some("medias/son.mp3"));
        assert!((p.volume - 0.35).abs() < 1e-6);
        assert_eq!(p.duree_ms, 4200);
        assert!(p.bounce && !p.zoom && p.plein_ecran && p.transparence);
        assert!((p.transparence_opacite - 0.4).abs() < 1e-6);

        let stop = serde_json::json!({"type": "pad-stop"}).to_string();
        assert_eq!(decoder_frame_pad(&stop), Some(MajPad::Stop));

        // Squelette PARTAGÉ : champs absents → défauts du pad (660/60/600/120).
        let config = serde_json::json!({
            "type": "position-overlay-config",
            "config": {"x": 120.0, "y": 40.0, "largeur": 480.0, "hauteur": 120.0},
        })
        .to_string();
        assert_eq!(
            decoder_frame_pad(&config),
            Some(MajPad::Config {
                x: 120.0,
                y: 40.0,
                largeur: 480.0,
                hauteur: 120.0,
            })
        );
        let partiel = serde_json::json!({
            "type": "position-overlay-config",
            "config": {"x": 10.0},
        })
        .to_string();
        let (dx, dy, dl, dh) = sos_moteur_natif::texte::POSITION_PAD_DEFAUT;
        assert_eq!(
            decoder_frame_pad(&partiel),
            Some(MajPad::Config {
                x: 10.0,
                y: dy,
                largeur: dl,
                hauteur: dh,
            })
        );
        let _ = dx;
    }

    #[test]
    fn decoder_pad_touche_audio_sans_visuel() {
        // Touche « audio » : `media` = le son, `son` = le média (branche du
        // domaine) ; `media_kind` absent → aucun visuel.
        let play = serde_json::json!({
            "type": "pad-play",
            "pad": {
                "media_type": "audio",
                "media": "medias/bruit.mp3",
                "media_kind": null,
                "son": "medias/bruit.mp3",
                "volume": 1.0,
                "duree_ms": 900,
            },
        })
        .to_string();
        let Some(MajPad::Play(p)) = decoder_frame_pad(&play) else {
            panic!("frame audio reconnue");
        };
        assert_eq!(p.media_type, "audio");
        assert_eq!(p.media_kind, None);
        assert_eq!(
            sos_moteur_natif::texte::source_audio_pad(&p),
            Some("medias/bruit.mp3")
        );
        assert_eq!(sos_moteur_natif::texte::media_affiche_pad(&p), None);
        // Défauts : pas de plein écran, pas d'effet.
        assert!(!p.plein_ecran && !p.bounce && !p.zoom && !p.transparence);
        assert!((p.volume - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decoder_pad_ignore_les_autres_frames() {
        for frame in [
            r#"{"type":"chat","message":{}}"#,
            r#"{"type":"pad:touche","code":"Numpad1"}"#,
            r#"{"type":"pad:touche-stop","code":"Numpad1"}"#,
            r#"{"type":"ecran-attente-etat"}"#,
            "pas du json",
        ] {
            assert!(decoder_frame_pad(frame).is_none(), "frame : {frame}");
        }
    }

    #[test]
    fn decoder_fight_combat_lobby_et_fin() {
        let combat = serde_json::json!({
            "type": "fight-state",
            "combat": {
                "phase": "combat",
                "ja": {"pseudo": "Manu", "plateforme": "twitch", "emoji": "🐲",
                       "pv": 80, "pv_max": 100, "special_utilise": false},
                "jb": {"pseudo": "Ryu", "plateforme": "kick", "emoji": "🐉",
                       "pv": 40, "pv_max": 100, "special_utilise": true},
                "tour": "b",
                "reste_tour_ms": 8500,
                "duree_tour_ms": 15000,
                "reste_match_s": 90,
                "derniere": {"seq": 12, "acteur": "a", "coup": "lourd",
                             "degats": 18, "soin": 0, "touche": true, "critique": true},
                "info": {"seq": 3, "kind": "ok", "texte": "Coup porté",
                         "duree_ms": 2600, "age_ms": 120},
                "anims_reduites": true,
                "cmds": {"a": "a", "b": "b", "c": "c", "ff": "ff"},
            },
        })
        .to_string();
        let etat = decoder_frame_fight(&combat).expect("frame combat reconnue");
        let PhaseFight::Combat {
            ja,
            jb,
            tour,
            reste_tour_ms,
            derniere,
            cmds,
            ..
        } = &etat.phase
        else {
            panic!("phase combat");
        };
        assert_eq!(ja.pseudo, "Manu");
        assert_eq!(jb.plateforme, "kick");
        assert!(jb.special_utilise);
        assert_eq!(*tour, 'b');
        assert_eq!(*reste_tour_ms, 8500);
        let coup = derniere.as_ref().expect("dernière action");
        assert_eq!(coup.seq, 12);
        assert_eq!(coup.acteur, 'a');
        assert!(coup.touche && coup.critique);
        assert_eq!(coup.degats, 18);
        assert_eq!(cmds.c.as_str(), "c");
        assert!(etat.anims_reduites);
        let info = etat.info.as_ref().expect("toast");
        assert_eq!(info.kind, "ok");
        assert_eq!(info.age_ms, 120);

        let lobby = serde_json::json!({
            "type": "fight-state",
            "combat": {
                "phase": "lobby",
                "defiant": {"pseudo": "Manu", "emoji": "🦊"},
                "cible": "ryu",
                "reste_lobby_ms": 12000,
                "duree_tour_s": 20,
                "duree_match_s": 240,
                "cmds": {"accept": "go", "refuse": "no"},
            },
        })
        .to_string();
        let etat = decoder_frame_fight(&lobby).expect("frame lobby reconnue");
        let PhaseFight::Lobby {
            defiant,
            cible,
            reste_lobby_ms,
            cmds,
            ..
        } = &etat.phase
        else {
            panic!("phase lobby");
        };
        assert_eq!(defiant.pseudo, "Manu");
        assert_eq!(cible.as_deref(), Some("ryu"));
        assert_eq!(*reste_lobby_ms, 12000);
        assert_eq!(cmds.accept.as_str(), "go");

        let fin = serde_json::json!({
            "type": "fight-state",
            "combat": {
                "phase": "fin",
                "ja": {"pseudo": "Manu", "pv": 30, "pv_max": 100},
                "jb": {"pseudo": "Ryu", "pv": 0, "pv_max": 100},
                "gagnant": "a",
                "raison": "temps",
            },
        })
        .to_string();
        let etat = decoder_frame_fight(&fin).expect("frame fin reconnue");
        let PhaseFight::Fin { gagnant, raison, .. } = &etat.phase else {
            panic!("phase fin");
        };
        assert_eq!(*gagnant, Some('a'));
        assert_eq!(raison, "temps");

        // `idle` : snapshot sans joueurs, arène masquée.
        let idle = serde_json::json!({"type": "fight-state", "combat": {"phase": "idle"}})
            .to_string();
        assert_eq!(
            decoder_frame_fight(&idle).map(|e| e.phase),
            Some(PhaseFight::Idle)
        );
    }

    #[test]
    fn decoder_fight_ignore_les_autres_frames() {
        for frame in [
            r#"{"type":"chat","message":{}}"#,
            r#"{"type":"fight:update","combat":{}}"#,
            r#"{"type":"fight-resync"}"#,
            r#"{"type":"pad-play","pad":{}}"#,
            "pas du json",
        ] {
            assert!(decoder_frame_fight(frame).is_none(), "frame : {frame}");
        }
    }

    #[test]
    fn decoder_presentation_show_decode_le_payload() {
        let frame = serde_json::json!({
            "type": "presentation-show",
            "mode": "card",
            "donnees": {
                "user_id": "42",
                "login": "manu",
                "display_name": "Manu",
                "avatar": "https://static-cdn.jtvnw.net/avatar.png",
                "bio": "Bienvenue",
                "banniere": "https://static-cdn.jtvnw.net/banner.jpg",
                "broadcaster_type": "affiliate",
                "en_live": true,
                "viewers_live": 1234,
                "titre_live": "Speedrun MGS",
                "jeu_courant": "Metal Gear Solid",
                "tags": ["FR", "Speedrun"],
                "jeux_recents": [
                    {"id": "1", "name": "MGS", "box_art_url": "https://cdn/{width}x{height}.jpg"}
                ],
                "clips": [
                    {"id": "slug", "title": "Le clip", "thumbnail_url": "https://cdn/t.jpg",
                     "duration": 29.6, "game_id": "1"}
                ],
                "mode": "card",
            },
        })
        .to_string();
        let Some(MajPresentation::Show { mode, donnees }) = decoder_frame_presentation(&frame)
        else {
            panic!("frame presentation-show reconnue");
        };
        assert_eq!(mode, ModePresentation::Card);
        assert_eq!(donnees.login, "manu");
        assert_eq!(donnees.banniere, "https://static-cdn.jtvnw.net/banner.jpg");
        assert!(donnees.en_live);
        assert_eq!(donnees.viewers_live, Some(1234));
        assert_eq!(donnees.jeu_courant, "Metal Gear Solid");
        assert_eq!(donnees.tags, vec!["FR".to_string(), "Speedrun".to_string()]);
        assert_eq!(donnees.jeux_recents.len(), 1);
        assert_eq!(donnees.jeux_recents[0].name, "MGS");
        assert_eq!(donnees.clips.len(), 1);
        assert_eq!(donnees.clips[0].id, "slug");
        assert!((donnees.clips[0].duration - 29.6).abs() < 1e-6);
    }

    #[test]
    fn decoder_presentation_hide_mode_et_frames_partielles() {
        let hide = r#"{"type":"presentation-hide"}"#;
        assert_eq!(
            decoder_frame_presentation(hide),
            Some(MajPresentation::Hide)
        );

        let mode = r#"{"type":"presentation-mode","mode":"theatrical"}"#;
        assert_eq!(
            decoder_frame_presentation(mode),
            Some(MajPresentation::Mode(ModePresentation::Theatrical))
        );
        // Mode absent → frame ignorée (aucune bascule devinée).
        assert!(decoder_frame_presentation(r#"{"type":"presentation-mode"}"#).is_none());

        // Show minimal : défauts du domaine, jamais des zéros inventés.
        let minimal = r#"{"type":"presentation-show","donnees":{"login":"manu"}}"#;
        let Some(MajPresentation::Show { mode, donnees }) =
            decoder_frame_presentation(minimal)
        else {
            panic!("frame minimale reconnue");
        };
        assert_eq!(mode, ModePresentation::Theatrical, "défaut = théâtral");
        assert_eq!(donnees.login, "manu");
        assert!(donnees.banniere.is_empty());
        assert!(!donnees.en_live);
        assert!(donnees.clips.is_empty());
    }

    #[test]
    fn decoder_presentation_audio_play_et_stop() {
        let play = serde_json::json!({
            "type": "presentation-audio-play",
            "piste": "musique",
            "src": "medias/musique.mp3",
        })
        .to_string();
        assert_eq!(
            decoder_frame_presentation(&play),
            Some(MajPresentation::AudioPlay {
                piste: PistePresentation::Musique,
                src: "medias/musique.mp3".to_string(),
            })
        );

        // `-audio-stop` : une piste, ou « toutes ».
        assert_eq!(
            decoder_frame_presentation(
                r#"{"type":"presentation-audio-stop","piste":"voix"}"#
            ),
            Some(MajPresentation::AudioStop(Some(PistePresentation::Voix)))
        );
        assert_eq!(
            decoder_frame_presentation(
                r#"{"type":"presentation-audio-stop","piste":"toutes"}"#
            ),
            Some(MajPresentation::AudioStop(None))
        );

        // Piste inconnue, `src` vide ou absent : frame ignorée.
        for frame in [
            r#"{"type":"presentation-audio-play","piste":"basse","src":"medias/a.mp3"}"#,
            r#"{"type":"presentation-audio-play","piste":"voix","src":"  "}"#,
            r#"{"type":"presentation-audio-play","piste":"voix"}"#,
            r#"{"type":"presentation-audio-stop","piste":"basse"}"#,
        ] {
            assert!(
                decoder_frame_presentation(frame).is_none(),
                "frame : {frame}"
            );
        }
    }

    #[test]
    fn decoder_presentation_ignore_les_autres_frames() {
        for frame in [
            r#"{"type":"chat","message":{}}"#,
            r#"{"type":"presentation:etat","visible":true}"#,
            "pas du json",
        ] {
            assert!(
                decoder_frame_presentation(frame).is_none(),
                "frame : {frame}"
            );
        }
    }

    #[test]
    fn decoder_splitpop_pos() {
        let frame = serde_json::json!({
            "type": "splitpop-pos",
            "config": {"x": 100.0, "y": 200.0, "largeur": 500.0, "hauteur": 90.0},
        })
        .to_string();
        assert_eq!(
            decoder_frame_splitpop(&frame),
            Some(MajSplitPop::Config {
                x: 100.0,
                y: 200.0,
                largeur: 500.0,
                hauteur: 90.0,
            })
        );
        // Config partielle → défauts du squelette dédié (660/60/600/120).
        let partiel = r#"{"type":"splitpop-pos","config":{"x":10.0}}"#;
        assert_eq!(
            decoder_frame_splitpop(partiel),
            Some(MajSplitPop::Config {
                x: 10.0,
                y: 60.0,
                largeur: 600.0,
                hauteur: 120.0,
            })
        );
        // Le squelette PARTAGÉ ne doit pas être confondu avec le dédié.
        for frame in [
            r#"{"type":"position-overlay-config","config":{"x":1.0}}"#,
            r#"{"type":"chrono2-pos","config":{"x":1.0}}"#,
            "pas du json",
        ] {
            assert!(decoder_frame_splitpop(frame).is_none(), "frame : {frame}");
        }
    }

    #[test]
    fn decoder_presentation_clip_play_et_stop() {
        let frame = serde_json::json!({
            "type": "presentation-clip-play",
            "mp4_url": "https://clips-media-assets2.twitch.tv/abc-1080.mp4?sig=x",
            "titre": "Le clip",
            "duree_ms": 29600,
        })
        .to_string();
        let Some(MajPresentation::ClipPlay {
            mp4_url,
            titre,
            duree_ms,
        }) = decoder_frame_presentation(&frame)
        else {
            panic!("frame presentation-clip-play reconnue");
        };
        assert_eq!(mp4_url, "https://clips-media-assets2.twitch.tv/abc-1080.mp4?sig=x");
        assert_eq!(titre, "Le clip");
        assert_eq!(duree_ms, 29600);

        assert_eq!(
            decoder_frame_presentation(r#"{"type":"presentation-clip-stop"}"#),
            Some(MajPresentation::ClipStop)
        );
        // URL absente ou vide : rien à jouer (frame ignorée).
        for frame in [
            r#"{"type":"presentation-clip-play","titre":"x"}"#,
            r#"{"type":"presentation-clip-play","mp4_url":"  "}"#,
        ] {
            assert!(
                decoder_frame_presentation(frame).is_none(),
                "frame : {frame}"
            );
        }
    }

    #[test]
    fn decoder_input_accepte_une_frame_serde_json() {
        let frame = serde_json::json!({
            "type": "input-viewer-etat",
            "etat": {
                "touches": ["KeyW", "Space"],
                "souris": { "gauche": true, "milieu": false, "droite": false, "x": 120, "y": 340 },
                "manette": { "connectee": true, "boutons": [true, false, true], "axes": [0.5] },
            },
        })
        .to_string();
        let etat = decoder_frame_input(&frame).expect("frame input reconnue");
        assert_eq!(etat.touches, vec!["KeyW".to_string(), "Space".to_string()]);
        assert!(etat.souris.gauche && !etat.souris.droite);
        assert_eq!((etat.souris.x, etat.souris.y), (120, 340));
        // Vecteurs courts → tableaux fixes paddés (boutons 0 et 2, axe 0).
        assert!(etat.manette.connectee);
        assert!(etat.manette.boutons[0] && !etat.manette.boutons[1] && etat.manette.boutons[2]);
        assert!(!etat.manette.boutons[19]);
        assert_eq!(etat.manette.axes[0], 0.5);
        assert_eq!(etat.manette.axes[5], 0.0);
        // Les autres frames du canal ne sont pas des états de touches.
        assert!(decoder_frame_input(r#"{"etat":{"spectre":[1,2]},"type":"eq"}"#).is_none());
        assert!(decoder_frame_input("pas du json").is_none());
    }

    #[test]
    fn capacites_aucune_ne_produit_aucun_id() {
        assert!(capacites_actives(&Capacites::aucune()).is_empty());
    }

    #[test]
    fn capacites_ids_stables_pour_l_i18n() {
        let c = Capacites {
            camera: true,
            audio: true,
            srt: true,
            ..Capacites::aucune()
        };
        assert_eq!(capacites_actives(&c), vec!["camera", "audio", "srt"]);
    }

    #[test]
    fn registre_natif_disponible_et_inactif_au_depart() {
        let registre = RegistreMoteur::nouveau(Mode::Obs);
        assert!(registre.natif_disponible);
        assert!(!registre.natif_actif());
        assert!(registre.session().is_none());
        assert_eq!(registre.natif().encodeur(), None);
    }

    #[test]
    fn capacites_du_moteur_actif() {
        let registre = RegistreMoteur::nouveau(Mode::Obs);
        // OBS : capture de jeu ; natif : pas de capture de jeu.
        assert!(registre.obs().capacites().capture_jeu);
        assert!(!registre.natif().capacites().capture_jeu);
        assert!(registre.obs().capacites().audio);
        assert!(registre.natif().capacites().audio);
    }

    #[test]
    fn chemin_normalise_ajoute_l_extension_du_conteneur() {
        let dossier = std::env::temp_dir().join("sos-moteur-chemin");
        let p = normaliser_chemin(
            &dossier.join("sortie").display().to_string(),
            Conteneur::Mp4,
        )
        .expect("chemin");
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("mp4"));
        let p = normaliser_chemin(
            &dossier.join("sortie").display().to_string(),
            Conteneur::Mkv,
        )
        .expect("chemin");
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("mkv"));
        // Une extension explicite prime toujours.
        let p = normaliser_chemin(
            &dossier.join("sortie.avi").display().to_string(),
            Conteneur::Mkv,
        )
        .expect("chemin");
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("avi"));
        assert!(normaliser_chemin("   ", Conteneur::Mp4).is_err());
    }

    #[test]
    fn resoudre_conteneur_defauts_et_erreurs() {
        assert_eq!(resoudre_conteneur(None).unwrap(), Conteneur::Mp4);
        assert_eq!(resoudre_conteneur(Some("  ")).unwrap(), Conteneur::Mp4);
        assert_eq!(resoudre_conteneur(Some("mkv")).unwrap(), Conteneur::Mkv);
        assert!(resoudre_conteneur(Some("webm")).is_err());
    }

    #[test]
    fn resoudre_resolution_derive_la_largeur_du_ratio() {
        // Canvas absent → rien à dériver.
        assert_eq!(resoudre_resolution(None, (1920, 1080)), None);
        // 16:9 : largeurs exactes.
        assert_eq!(
            resoudre_resolution(Some(720), (1920, 1080)),
            Some((1280, 720))
        );
        assert_eq!(
            resoudre_resolution(Some(360), (1920, 1080)),
            Some((640, 360))
        );
        // Ratio non standard (1360×768) : arrondi au pair supérieur.
        let (w, h) = resoudre_resolution(Some(720), (1360, 768)).unwrap();
        assert_eq!(h, 720);
        assert_eq!(w % 2, 0);
        assert!((w as f64 / h as f64 - 1360.0 / 768.0).abs() < 0.01);
    }

    #[test]
    fn destination_publique_ne_publie_jamais_la_cle() {
        // Le chemin RTMP ou la query SRT peuvent embarquer la clé : seuls
        // le schéma et l'autorité sont affichables/journalisables.
        assert_eq!(
            destination_publique("rtmp://live.example/app/cle-secrete"),
            "rtmp://live.example"
        );
        assert_eq!(
            destination_publique("rtmps://live.example/app"),
            "rtmps://live.example"
        );
        assert_eq!(
            destination_publique("srt://live.example:9000?streamid=cle-secrete"),
            "srt://live.example:9000"
        );
        assert_eq!(destination_publique("pas-une-url"), "<flux>");
    }

    #[test]
    fn resoudre_codec_defauts_et_erreurs() {
        // Absent ou vide → défaut produit H.264 ; id stable accepté ;
        // inconnu → erreur honnête, jamais de repli muet.
        assert_eq!(resoudre_codec(None).unwrap(), Codec::H264);
        assert_eq!(resoudre_codec(Some("  ")).unwrap(), Codec::H264);
        assert_eq!(resoudre_codec(Some("hevc")).unwrap(), Codec::Hevc);
        assert_eq!(resoudre_codec(Some("av1")).unwrap(), Codec::Av1);
        assert!(resoudre_codec(Some("vp9")).is_err());
    }

    #[test]
    fn resoudre_preset_defaut_et_valeur() {
        // Absent ou vide → défaut produit ; valeur fournie conservée telle
        // quelle (champ transporté, pas de liste blanche figée).
        assert_eq!(resoudre_preset(None), "equilibre");
        assert_eq!(resoudre_preset(Some("  ")), "equilibre");
        assert_eq!(resoudre_preset(Some("qualite")), "qualite");
        assert_eq!(resoudre_preset(Some(" performance ")), "performance");
    }

    #[test]
    fn srt_url_avec_params_assemble_et_ne_duplique_pas() {
        assert_eq!(
            srt_url_avec_params("srt://h:9000", Some("pass"), Some(200)).unwrap(),
            "srt://h:9000?passphrase=pass&latency=200"
        );
        assert_eq!(
            srt_url_avec_params("srt://h:9000?streamid=k", Some("pass"), None).unwrap(),
            "srt://h:9000?streamid=k&passphrase=pass"
        );
        // Déjà présent dans l'URL → l'utilisateur prime, pas de doublon.
        assert_eq!(
            srt_url_avec_params("srt://h:9000?latency=100", None, Some(200)).unwrap(),
            "srt://h:9000?latency=100"
        );
        assert_eq!(
            srt_url_avec_params("srt://h:9000?passphrase=x", Some("y"), None).unwrap(),
            "srt://h:9000?passphrase=x"
        );
        // Caractères réservés refusés proprement — jamais de query cassée.
        assert!(srt_url_avec_params("srt://h:9000", Some("a&b"), None).is_err());
        assert!(srt_url_avec_params("srt://h:9000", Some("a b"), None).is_err());
        // Aucun paramètre → URL inchangée.
        assert_eq!(
            srt_url_avec_params("srt://h:9000", None, None).unwrap(),
            "srt://h:9000"
        );
    }

    #[test]
    fn stream_cfg_persistee_serde_tolere_les_champs_absents() {
        // Blob minimal d'une ancienne version : tout champ manquant a un
        // défaut — une config persistée ne casse jamais à la lecture.
        let cfg: StreamCfgPersistee =
            serde_json::from_str(r#"{"url":"srt://h:9000","protocole":"srt"}"#).unwrap();
        assert_eq!(cfg.url, "srt://h:9000");
        assert_eq!(cfg.preset, "equilibre");
        assert!(cfg.cle.is_none() && cfg.srt_passphrase.is_none());
    }

    #[test]
    fn arret_idempotent_sans_session() {
        let registre = RegistreMoteur::nouveau(Mode::Natif);
        registre.arreter_natif();
        registre.arreter_natif();
        assert!(!registre.natif_actif());
        assert!(registre.session().is_none());
    }

    #[test]
    fn frame_chat_decode_et_couleur_conservee() {
        let frame = r##"{"type":"chat","message":{"plateforme":"twitch","pseudo":"Ryo","texte":"salut","color":"#ff0000"}}"##;
        let message = decoder_frame_chat(frame).expect("message décodé");
        assert_eq!(message.plateforme, "twitch");
        assert_eq!(message.pseudo, "Ryo");
        assert_eq!(message.texte, "salut");
        assert_eq!(message.couleur, Some([255, 0, 0, 255]));
    }

    #[test]
    fn frames_non_chat_ignores() {
        // Spectre de l'égaliseur : plusieurs dizaines de frames par seconde.
        assert!(decoder_frame_chat(r#"{"type":"eq-spectrum","data":[1,2,3]}"#).is_none());
        assert!(decoder_frame_chat("pas du json").is_none());
        assert!(decoder_frame_chat(r#"{"type":"chat"}"#).is_none());
    }

    #[test]
    fn decoder_goal_accepte_les_quatre_frames_serde_json() {
        // `serde_json::json!` trie les clés — les frames réelles ne
        // commencent pas par "type" ; le décodeur matche par sous-chaîne.
        let frame = serde_json::json!({
            "type": "communaute", "plateforme": "twitch",
            "followers": 42, "subs": 7,
        })
        .to_string();
        let Some(MajGoal::Communaute {
            plateforme,
            compteurs,
        }) = decoder_frame_goal(&frame)
        else {
            panic!("frame communaute non reconnue : {frame}");
        };
        assert_eq!(plateforme, "twitch");
        assert_eq!(compteurs.followers, Some(42));
        assert_eq!(compteurs.subs, Some(7));
        assert_eq!(compteurs.sub_points, None, "champ absent = non mesuré");

        let frame =
            serde_json::json!({"type": "viewers", "plateforme": "kick", "count": 12}).to_string();
        assert!(matches!(
            decoder_frame_goal(&frame),
            Some(MajGoal::Viewers {
                count: Some(12), ..
            })
        ));
        // count:null = hors-ligne → None (jamais Some(0) inventé).
        let frame =
            serde_json::json!({"type": "viewers", "plateforme": "kick", "count": null}).to_string();
        assert!(matches!(
            decoder_frame_goal(&frame),
            Some(MajGoal::Viewers { count: None, .. })
        ));

        let frame = serde_json::json!({
            "type": "goal-twitch", "id": "g1", "goal_type": "follower",
            "description": "", "actuel": 10, "cible": 50,
            "atteint": false, "termine": false,
        })
        .to_string();
        let Some(MajGoal::Twitch(g)) = decoder_frame_goal(&frame) else {
            panic!("frame goal-twitch non reconnue : {frame}");
        };
        assert_eq!(g.id, "g1");
        assert_eq!((g.actuel, g.cible), (10, 50));
        assert!(!g.termine);

        let frame =
            serde_json::json!({"type": "goal-atteint", "widget_id": "w1", "count": 500}).to_string();
        assert!(matches!(
            decoder_frame_goal(&frame),
            Some(MajGoal::Atteint { widget_id }) if widget_id == "w1"
        ));
    }

    #[test]
    fn decoder_goal_ignore_les_autres_frames() {
        assert!(decoder_frame_goal(r#"{"type":"chat","message":{}}"#).is_none());
        assert!(decoder_frame_goal(r#"{"type":"eq-spectrum","data":[]}"#).is_none());
        assert!(decoder_frame_goal("pas du json").is_none());
        // Type intéressant mais clé obligatoire absente → None.
        assert!(decoder_frame_goal(r#"{"type":"viewers","count":5}"#).is_none());
        assert!(decoder_frame_goal(r#"{"type":"goal-atteint"}"#).is_none());
        assert!(decoder_frame_goal(r#"{"type":"goal-twitch","actuel":1}"#).is_none());
    }

    #[test]
    fn decoder_speedrun_accepte_lss_started_split_et_ended() {
        // `lss` : event construit à la main par `emit_ws_lss_json` (camelCase).
        let frame = serde_json::json!({
            "type": "speedrun-etat",
            "etat": {
                "type": "lss",
                "nomJeu": "MGS",
                "nomCategorie": "Any%",
                "segments": [
                    {"nom": "Dock", "pbRealTime": 83.45, "pbGameTime": null},
                    {"nom": "Heliport", "pbRealTime": 225.0}
                ],
            },
        })
        .to_string();
        let Some(MajSpeedrun::Lss {
            nom_jeu,
            nom_categorie,
            segments,
        }) = decoder_frame_speedrun(&frame)
        else {
            panic!("frame lss non reconnue : {frame}");
        };
        assert_eq!((nom_jeu.as_str(), nom_categorie.as_str()), ("MGS", "Any%"));
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].pb_real_time, Some(83.45));
        assert_eq!(segments[0].pb_game_time, None);
        assert_eq!(segments[1].nom, "Heliport");

        let frame = serde_json::json!({
            "type": "speedrun-etat", "etat": {"type": "started", "index": 0}
        })
        .to_string();
        assert!(matches!(
            decoder_frame_speedrun(&frame),
            Some(MajSpeedrun::Started { index: Some(0) })
        ));

        let frame = serde_json::json!({
            "type": "speedrun-etat",
            "etat": {"type": "split", "time": "01:20.00", "cumul": "01:20.00", "index": 1},
        })
        .to_string();
        assert!(matches!(
            decoder_frame_speedrun(&frame),
            Some(MajSpeedrun::Split { index: Some(1), time: Some(_), cumul: Some(_) })
        ));

        let frame = serde_json::json!({
            "type": "speedrun-etat",
            "etat": {"type": "ended", "time": "10:00.00", "cumul": "10:00.00", "index": 3},
        })
        .to_string();
        assert!(matches!(
            decoder_frame_speedrun(&frame),
            Some(MajSpeedrun::Ended { index: Some(3), .. })
        ));
    }

    #[test]
    fn decoder_speedrun_time_distingue_null_et_absent() {
        // `segmentTime: null` → Some(None) (le DOM assigne la valeur présente,
        // null compris) ; absent → None (inchangé, `!== undefined`).
        let frame = serde_json::json!({
            "type": "speedrun-etat",
            "etat": {"type": "time", "realTime": "01:10.50", "gameTime": null, "segmentTime": null},
        })
        .to_string();
        let Some(MajSpeedrun::Time {
            real_time,
            game_time,
            segment_time,
        }) = decoder_frame_speedrun(&frame)
        else {
            panic!("frame time non reconnue : {frame}");
        };
        assert_eq!(real_time.as_deref(), Some("01:10.50"));
        assert_eq!(game_time, None);
        assert_eq!(segment_time, Some(None), "segmentTime null = effacer");

        let frame = serde_json::json!({
            "type": "speedrun-etat",
            "etat": {"type": "time", "realTime": "01:11.00", "gameTime": "01:09.00"},
        })
        .to_string();
        assert!(matches!(
            decoder_frame_speedrun(&frame),
            Some(MajSpeedrun::Time { segment_time: None, .. })
        ));
    }

    #[test]
    fn decoder_speedrun_etat_goto_et_connexion() {
        // `etat` : resync complet (phase, index, splits, chronos).
        let frame = serde_json::json!({
            "type": "speedrun-etat",
            "etat": {
                "type": "etat", "phase": "Running", "index": 1,
                "splitTimes": ["01:20.00", null], "splitCumuls": ["01:20.00", null],
                "realTime": "02:00.00", "gameTime": "01:58.00", "segmentTime": "00:40.00",
            },
        })
        .to_string();
        let Some(MajSpeedrun::Etat {
            phase,
            index,
            split_times,
            split_cumuls,
            segment_time,
            ..
        }) = decoder_frame_speedrun(&frame)
        else {
            panic!("frame etat non reconnue : {frame}");
        };
        assert_eq!(phase, PhaseSpeedrun::Running);
        assert_eq!(index, Some(1));
        assert_eq!(split_times.len(), 2);
        assert_eq!(split_cumuls[0].as_deref(), Some("01:20.00"));
        assert_eq!(segment_time.as_deref(), Some("00:40.00"));

        // `goto` : le nom de phase est traduit en enum.
        let frame = serde_json::json!({
            "type": "speedrun-etat",
            "etat": {"type": "goto", "index": 2, "phase": "Paused"},
        })
        .to_string();
        assert!(matches!(
            decoder_frame_speedrun(&frame),
            Some(MajSpeedrun::Goto {
                index: Some(2),
                phase: Some(PhaseSpeedrun::Paused),
            })
        ));

        let frame = serde_json::json!({
            "type": "speedrun-etat",
            "etat": {"type": "connected", "process": "mgs.exe", "pid": 1234},
        })
        .to_string();
        assert!(matches!(
            decoder_frame_speedrun(&frame),
            Some(MajSpeedrun::Connected {
                processus: Some(ref p)
            }) if p == "mgs.exe"
        ));
    }

    #[test]
    fn decoder_speedrun_ignore_les_autres_frames_et_events_muets() {
        assert!(decoder_frame_speedrun(r#"{"type":"chat","message":{}}"#).is_none());
        assert!(decoder_frame_speedrun("pas du json").is_none());
        assert!(decoder_frame_speedrun(r#"{"type":"speedrun-etat"}"#).is_none());
        // Events sans effet visuel sur le widget (`default:` du DOM).
        for etat in [
            serde_json::json!({"type": "error", "message": "oops"}),
            serde_json::json!({"type": "settings-list", "settings": []}),
            serde_json::json!({"type": "loaded"}),
            serde_json::json!({"type": "saved", "chemin": "run.lss"}),
        ] {
            let frame = serde_json::json!({
                "type": "speedrun-etat", "etat": etat,
            })
            .to_string();
            assert!(
                decoder_frame_speedrun(&frame).is_none(),
                "event sans effet visuel accepté : {frame}"
            );
        }
        // Events de contrôle : poussés au moteur (phase, index, splits).
        for etat in [
            serde_json::json!({"type": "reset"}),
            serde_json::json!({"type": "stopped"}),
            serde_json::json!({"type": "paused"}),
            serde_json::json!({"type": "resumed"}),
            serde_json::json!({"type": "disconnected"}),
            serde_json::json!({"type": "splitSkipped", "index": 2}),
            serde_json::json!({"type": "splitUndone", "index": 1}),
            serde_json::json!({"type": "gameTime", "time": "01:00.00"}),
        ] {
            let frame = serde_json::json!({
                "type": "speedrun-etat", "etat": etat,
            })
            .to_string();
            assert!(
                decoder_frame_speedrun(&frame).is_some(),
                "event de contrôle ignoré : {frame}"
            );
        }
    }

    #[test]
    fn decoder_musique_accepte_le_payload_etat() {
        // Payload de `musique:update` = `MusiqueState::etat()` sérialisé
        // (pas de champ `type` — le nom de l'event le porte déjà).
        let payload = serde_json::json!({
            "config": {},
            "en_cours": {
                "id": "p1", "titre": "Morceau",
                "src": "medias/musique/m.mp3",
                "demandeur": "viewer", "plateforme": "twitch",
            },
            "file": [],
            "bibliotheque": [],
            "progression": {"position_ms": 5000, "pausee": false, "duree_s": 200.0},
            "cooldowns_actifs": [],
        })
        .to_string();
        let maj = decoder_payload_musique(&payload).expect("payload décodé");
        let piste = maj.en_cours.expect("piste en cours");
        assert_eq!(
            (piste.id.as_str(), piste.titre.as_str(), piste.demandeur.as_str()),
            ("p1", "Morceau", "viewer")
        );
        assert_eq!(maj.position_ms, 5000);
        assert!(!maj.pausee);
        assert_eq!(maj.duree_s, Some(200.0));

        // Lecteur à l'arrêt : `en_cours`/`progression` null → état initial.
        let payload = serde_json::json!({"en_cours": null, "progression": null}).to_string();
        let maj = decoder_payload_musique(&payload).expect("payload arrêt décodé");
        assert!(maj.en_cours.is_none());
        assert_eq!(maj.position_ms, 0);
        assert!(maj.duree_s.is_none());
        assert!(decoder_payload_musique("pas du json").is_none());
    }

    #[test]
    fn decoder_welcome_play_extrait_pseudo_bio_message_et_mode() {
        let frame = serde_json::json!({
            "type": "welcome-clip-play",
            "viewer": {
                "login": "manu",
                "display_name": "Manu",
                "avatar": "https://static-cdn.jtvnw.net/av.png",
                "bio": "Streamer retro",
                "banniere": "https://static-cdn.jtvnw.net/ban.png",
            },
            "clip": {
                "mp4_url": "https://clips-media-assets2.twitch.tv/x.mp4",
                "titre": "Le run",
                "duree_ms": 30000,
            },
            "message": "Bienvenue !",
            "utiliser_squelette": false,
            "favoris": [],
        })
        .to_string();
        match decoder_frame_welcome(&frame).expect("play décodé") {
            MajWelcome::Play {
                display_name,
                bio,
                message,
                spotlight,
            } => {
                assert_eq!(display_name, "Manu");
                assert_eq!(bio.as_deref(), Some("Streamer retro"));
                assert_eq!(message, "Bienvenue !");
                assert!(spotlight, "utiliser_squelette=false → spotlight");
            }
            autre => panic!("mutation inattendue : {autre:?}"),
        }
    }

    #[test]
    fn decoder_welcome_play_sans_mode_replie_sur_squelette_et_login() {
        let frame = serde_json::json!({
            "type": "welcome-clip-play",
            "viewer": {"login": "manu", "display_name": "", "bio": ""},
            "clip": {"mp4_url": "", "titre": "", "duree_ms": 0},
            "message": "",
        })
        .to_string();
        match decoder_frame_welcome(&frame).expect("play décodé") {
            MajWelcome::Play {
                display_name,
                bio,
                spotlight,
                ..
            } => {
                assert_eq!(display_name, "manu", "repli sur login");
                assert!(bio.is_none(), "bio vide = absente");
                assert!(!spotlight, "mode absent → squelette");
            }
            autre => panic!("mutation inattendue : {autre:?}"),
        }
    }

    #[test]
    fn decoder_welcome_stop_et_config() {
        let stop = serde_json::json!({"type": "welcome-clip-stop"}).to_string();
        assert_eq!(decoder_frame_welcome(&stop), Some(MajWelcome::Stop));

        // `welcome-clip-config` : squelette + mode (spotlight = !squelette).
        let cfg = serde_json::json!({
            "type": "welcome-clip-config",
            "config": {
                "x": 1480.0, "y": 580.0, "largeur": 320.0,
                "hauteur": 352.0, "utiliser_squelette": false,
            },
        })
        .to_string();
        assert_eq!(
            decoder_frame_welcome(&cfg),
            Some(MajWelcome::Config {
                x: 1480.0,
                y: 580.0,
                largeur: 320.0,
                hauteur: 352.0,
                spotlight: Some(true),
            })
        );

        // `position-overlay-config` : squelette seul, mode conservé.
        let pos = serde_json::json!({
            "type": "position-overlay-config",
            "config": {"x": 100.0, "y": 200.0, "largeur": 400.0, "hauteur": 300.0},
        })
        .to_string();
        assert_eq!(
            decoder_frame_welcome(&pos),
            Some(MajWelcome::Config {
                x: 100.0,
                y: 200.0,
                largeur: 400.0,
                hauteur: 300.0,
                spotlight: None,
            })
        );
    }

    #[test]
    fn decoder_welcome_ignore_les_autres_frames() {
        for frame in [
            r#"{"type":"chat","message":{}}"#,
            r#"{"type":"alerte-play","alerte":{"texte":"x"}}"#,
            r#"{"type":"welcome-clip-config"}"#,
            "pas du json",
        ] {
            assert!(decoder_frame_welcome(frame).is_none(), "frame : {frame}");
        }
    }

    #[test]
    fn decoder_snapshot_extrait_la_scene_de_la_frame() {
        let scene = sos_scene::Scene {
            canvasW: 1360,
            canvasH: 768,
            ..Default::default()
        };
        // Clés triées par serde_json::json! : "scene" < "sceneId" < "type" —
        // même piège que les décodeurs chat/input (starts_with interdit).
        let frame = serde_json::json!({
            "type": "snapshot",
            "sceneId": "abc",
            "scene": scene,
        })
        .to_string();
        let scene = decoder_frame_snapshot(&frame).expect("scène décodée");
        assert_eq!((scene.canvasW, scene.canvasH), (1360, 768));
        // Les autres frames du canal ne sont pas des snapshots ; un snapshot
        // sans champ `scene` ne désérialise pas.
        assert!(decoder_frame_snapshot(r#"{"type":"chat","message":{}}"#).is_none());
        assert!(decoder_frame_snapshot(r#"{"type":"snapshot"}"#).is_none());
        assert!(decoder_frame_snapshot("pas du json").is_none());
    }

    #[test]
    fn audio_persistee_vers_contrat_moteur() {
        let cfg = crate::config::AudioMoteurCfg {
            sources: vec![
                crate::config::SourceAudioPersistee {
                    id: "desktop".to_string(),
                    genre: "desktop".to_string(),
                    peripherique: Some("  {dev-sortie}  ".to_string()),
                    gain: 800,
                    mute: true,
                },
                crate::config::SourceAudioPersistee {
                    id: "micro".to_string(),
                    genre: "micro".to_string(),
                    peripherique: Some("   ".to_string()),
                    gain: 1000,
                    mute: false,
                },
            ],
        };
        let audio = audio_cfg_depuis_persistee(&cfg).expect("config valide");
        assert_eq!(audio.sources.len(), 2);
        assert_eq!(audio.sources[0].genre, GenreAudio::Desktop);
        // ID trimé, chaîne vide → périphérique par défaut.
        assert_eq!(
            audio.sources[0].peripherique.as_deref(),
            Some("{dev-sortie}")
        );
        assert_eq!(audio.sources[1].peripherique, None);
        assert_eq!(audio.sources[0].gain_effectif(), 0);
        assert_eq!(audio.sources[1].gain_effectif(), 1000);
    }

    #[test]
    fn audio_persistee_refuse_genre_inconnu_et_doublon() {
        let genre = crate::config::AudioMoteurCfg {
            sources: vec![crate::config::SourceAudioPersistee {
                id: "x".to_string(),
                genre: "app".to_string(),
                peripherique: None,
                gain: 1000,
                mute: false,
            }],
        };
        assert!(audio_cfg_depuis_persistee(&genre).is_err());

        let doublon = crate::config::AudioMoteurCfg {
            sources: vec![
                crate::config::SourceAudioPersistee {
                    id: "micro".to_string(),
                    genre: "micro".to_string(),
                    peripherique: None,
                    gain: 1000,
                    mute: false,
                },
                crate::config::SourceAudioPersistee {
                    id: "micro".to_string(),
                    genre: "micro".to_string(),
                    peripherique: None,
                    gain: 1000,
                    mute: false,
                },
            ],
        };
        assert!(audio_cfg_depuis_persistee(&doublon).is_err());

        let gain = crate::config::AudioMoteurCfg {
            sources: vec![crate::config::SourceAudioPersistee {
                id: "micro".to_string(),
                genre: "micro".to_string(),
                peripherique: None,
                gain: 4001,
                mute: false,
            }],
        };
        assert!(audio_cfg_depuis_persistee(&gain).is_err());
    }
}
