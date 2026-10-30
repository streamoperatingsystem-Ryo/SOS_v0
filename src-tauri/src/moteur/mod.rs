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
    ApercuCfg, AudioCfg, Capacites, Codec, Conteneur, Moteur, MoteurEtat, SinkCfg, SortieCfg,
};
use sos_moteur_natif::chat::MessageChat;
use sos_moteur_natif::input::{EtatInput, EtatManette, EtatSouris};
use sos_moteur_natif::moteur::MoteurNatif;
use sos_moteur_natif::texte::{
    Communaute, GoalTwitch, MajAlerte, MajGoal, MajMusique, MajSpeedrun, PhaseSpeedrun, Piste,
    SegmentLss,
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
    cfg.audio = Some(AudioCfg {
        loopback: s.audio_loopback,
        micro: s.audio_micro,
        gain_loopback: s.gain_loopback,
        gain_micro: s.gain_micro,
    });
    Ok(())
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
    for (nom, gain) in [("loopback", s.gain_loopback), ("micro", s.gain_micro)] {
        if gain > 4000 {
            return Err(format!("gain {nom} hors bornes : {gain} millièmes (max 4000)"));
        }
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
        audio: Some(AudioCfg {
            loopback: sortie.audio_loopback,
            micro: sortie.audio_micro,
            gain_loopback: sortie.gain_loopback,
            gain_micro: sortie.gain_micro,
        }),
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

/// Message chat tel qu'il circule dans `chat_tx` (`ChatMessage` sérialisé).
#[derive(serde::Deserialize)]
struct MessageChatBrut {
    plateforme: String,
    pseudo: String,
    texte: String,
    #[serde(default)]
    color: Option<String>,
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
    Some(MessageChat::nouveau(
        message.plateforme,
        message.pseudo,
        message.texte,
        message.color.as_deref(),
    ))
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
                    if let Some(message) = decoder_frame_chat(&frame) {
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
                    } else if let Some(maj) = decoder_frame_speedrun(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_speedrun(maj);
                        }
                    } else if let Some(maj) = decoder_frame_alerte(&frame) {
                        if let Some(registre) = app.try_state::<RegistreMoteur>() {
                            registre.natif().pousser_alerte(maj);
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
/// vide tant qu'aucune frame n'a été produite. `pts_us` permet à la page
/// de sauter une frame déjà affichée. Réservé au mode Rust
/// (`moteur_basculer` arrête la session en quittant le mode natif).
#[tauri::command]
pub fn moteur_apercu(
    app: AppHandle,
    registre: State<'_, RegistreMoteur>,
    scenes: State<'_, crate::scenes::ScenesState>,
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
        natif
            .demarrer_apercu(
                (scene.canvasW, scene.canvasH),
                30,
                apercu_cfg_persistee(&app),
            )
            .map_err(|e| e.to_string())?;
    }
    let frame = match natif.apercu() {
        Some(f) if f.valide() => f,
        _ => return Ok(tauri::ipc::Response::new(Vec::new())),
    };
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
}
