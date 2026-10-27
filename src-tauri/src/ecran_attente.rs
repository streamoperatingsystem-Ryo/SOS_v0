/// Écran d'attente (BRB) — overlay plein écran sur la diffusion :4321.
///
/// Porté depuis l'ancienne app RUST_SOS_2026 (features/ecran-attente/), réécrit
/// pour l'architecture v0 (même pattern que pad_numerique.rs / bandeau.rs) :
///   - Config persistée `ecran_attente.json` : duree_s, titre, afficher_titre,
///     son (défaut ON), volume, media (+kind+nom), source_audio OBS.
///   - Chrono détenu par Rust (tâche tokio 1 Hz) — source de vérité : la
///     diffusion et le dashboard ne font que refléter `temps_restant`.
///   - Frame WS auto-réparatrice `ecran-attente-etat` sur chat_tx à chaque
///     tick/transition → diffusion.html affiche l'overlay plein écran
///     (z 9990 : au-dessus de la scène, sous pad/commandes/alertes/bandeau/
///     welcome). À 0 → « 0:00 » affiché ~1,5 s puis fermeture auto.
///   - Resync : la diffusion envoie `ecran-attente-resync` à la (re)connexion
///     WS → server.rs appelle `reemit()` qui repousse la dernière frame.
///   - Routage audio OBS (porté de l'ancienne app, features/ecran_attente.rs) :
///     au lancer, connexion one-shot → source `wasapi_output_capture`
///     « SOS Audio Écran d'attente » (ou celle choisie en config) réglée au
///     volume configuré + mute des sources audio bureau de la scène SOS ;
///     à l'arrêt → unmute. Non-fatal si OBS absent. Indépendant du toggle
///     `son` (qui pilote le <video> côté diffusion).
///   - Event Tauri `ecran-attente:etat` → dashboard (modale + pillule header).
use crate::config::data_dir;
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

/// Nom de la source audio dédiée créée dans OBS si aucune source n'est choisie.
const NOM_SOURCE_AUDIO: &str = "SOS Audio Écran d'attente";

/// Délai d'affichage de « 0:00 » avant la fermeture auto (comme l'ancienne app).
const DELAI_FIN_MS: u64 = 1500;

// =============================================================================
// CONFIG (sérialisable, persistée dans ecran_attente.json)
// =============================================================================

/// Configuration de l'écran d'attente (persistée).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcranAttenteConfig {
    /// Durée du chrono en secondes. Défaut 600 (10 min).
    #[serde(default = "default_duree_s")]
    pub duree_s: u64,
    /// Titre affiché en haut de l'écran d'attente.
    #[serde(default = "default_titre")]
    pub titre: String,
    /// Afficher le titre (défaut true).
    #[serde(default = "default_true")]
    pub afficher_titre: bool,
    /// Son du média vidéo activé (défaut true — consigne utilisateur).
    #[serde(default = "default_true")]
    pub son: bool,
    /// Volume du média ET de la source audio OBS dédiée (0.0-1.0). Défaut 0.5.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Chemin relatif du média ("medias/<uuid>.<ext>"). None = pas de média.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    /// Kind du média : "image" | "video". None = pas de média.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_kind: Option<String>,
    /// Nom original du fichier importé (affichage UI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_nom: Option<String>,
    /// Nom de la source audio OBS (wasapi_output_capture) choisie par
    /// l'utilisateur. None/vide = créer « SOS Audio Écran d'attente ».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_audio: Option<String>,
    /// Couper les sources audio « bureau » de la scène SOS pendant l'écran
    /// d'attente (défaut true). false = l'audio bureau continue de passer
    /// dans le stream pendant l'attente. Option de la modale « Connexion
    /// OBS » (WebSocket) — persistée ici avec le reste de la config.
    #[serde(default = "default_true")]
    pub muter_bureau: bool,
    /// Centre horizontal de la carte minuteur (px canvas diffusion).
    /// Défaut 960 (centre 1920×1080). Positionnée par drag dans la modale.
    #[serde(default = "default_minuteur_x")]
    pub minuteur_x: f64,
    /// Centre vertical de la carte minuteur (px canvas diffusion). Défaut 540.
    #[serde(default = "default_minuteur_y")]
    pub minuteur_y: f64,
    /// Échelle de la carte minuteur (0.4-3.0, resize dans la modale).
    /// Défaut 1.0.
    #[serde(default = "default_minuteur_echelle")]
    pub minuteur_echelle: f64,
    /// Layout de l'overlay : "signature" (marque StreamOS — Streamy +
    /// stries néon + wordmark, défaut) | "minimal" (rendu historique :
    /// dégradé + titre + carte minuteur seuls).
    #[serde(default = "default_layout")]
    pub layout: String,
    /// Centre horizontal de Streamy (px canvas). Défaut 170 (haut-gauche).
    #[serde(default = "default_streamy_x")]
    pub streamy_x: f64,
    /// Centre vertical de Streamy (px canvas). Défaut 180.
    #[serde(default = "default_streamy_y")]
    pub streamy_y: f64,
    /// Échelle de Streamy (0.3-2.0). Défaut 0.6.
    #[serde(default = "default_streamy_echelle")]
    pub streamy_echelle: f64,
    /// Centre horizontal du wordmark logo (px canvas). Défaut 960.
    #[serde(default = "default_logo_x")]
    pub logo_x: f64,
    /// Centre vertical du wordmark logo (px canvas). Défaut 140 (haut-centre).
    #[serde(default = "default_logo_y")]
    pub logo_y: f64,
    /// Échelle du wordmark logo (0.3-2.0). Défaut 1.0.
    #[serde(default = "default_logo_echelle")]
    pub logo_echelle: f64,
}

fn default_duree_s() -> u64 {
    600
}
fn default_titre() -> String {
    // Vide par défaut : le wordmark StreamOS (layout signature) fait office
    // de titre — l'utilisateur peut toujours saisir un titre personnalisé.
    String::new()
}
fn default_true() -> bool {
    true
}
fn default_volume() -> f32 {
    0.5
}
fn default_minuteur_x() -> f64 {
    960.0
}
fn default_minuteur_y() -> f64 {
    540.0
}
fn default_minuteur_echelle() -> f64 {
    1.0
}
fn default_layout() -> String {
    "signature".to_string()
}
fn default_streamy_x() -> f64 {
    170.0
}
fn default_streamy_y() -> f64 {
    180.0
}
fn default_streamy_echelle() -> f64 {
    0.6
}
fn default_logo_x() -> f64 {
    960.0
}
fn default_logo_y() -> f64 {
    140.0
}
fn default_logo_echelle() -> f64 {
    1.0
}

impl Default for EcranAttenteConfig {
    fn default() -> Self {
        Self {
            duree_s: default_duree_s(),
            titre: default_titre(),
            afficher_titre: true,
            son: true,
            volume: default_volume(),
            media: None,
            media_kind: None,
            media_nom: None,
            source_audio: None,
            muter_bureau: true,
            minuteur_x: default_minuteur_x(),
            minuteur_y: default_minuteur_y(),
            minuteur_echelle: default_minuteur_echelle(),
            layout: default_layout(),
            streamy_x: default_streamy_x(),
            streamy_y: default_streamy_y(),
            streamy_echelle: default_streamy_echelle(),
            logo_x: default_logo_x(),
            logo_y: default_logo_y(),
            logo_echelle: default_logo_echelle(),
        }
    }
}

// =============================================================================
// ÉTAT
// =============================================================================

/// État runtime (non persisté — l'écran est inactif au boot).
#[derive(Debug, Clone, Default)]
struct EcranAttenteRuntime {
    actif: bool,
    en_pause: bool,
    temps_restant: u64,
}

/// Snapshot complet pour l'UI dashboard (commande + event `ecran-attente:etat`).
#[derive(Debug, Clone, Serialize)]
pub struct EcranAttenteEtat {
    pub config: EcranAttenteConfig,
    pub actif: bool,
    pub en_pause: bool,
    pub temps_restant: u64,
}

/// État partagé écran d'attente. Clonable (Arc internes).
#[derive(Clone)]
pub struct EcranAttenteState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<EcranAttenteConfig>>,
    etat: Arc<Mutex<EcranAttenteRuntime>>,
    /// Jeton d'annulation de la tâche chrono courante.
    timer_cancel: Arc<Mutex<Arc<AtomicBool>>>,
    /// Dernière frame WS émise (resync à la reconnexion diffusion).
    derniere_frame: Arc<Mutex<Option<String>>>,
    /// Creds OBS mémorisés au lancer — la restauration audio doit pouvoir
    /// tourner au stop auto (fin du chrono) sans nouvelle passe de creds.
    obs_creds: Arc<Mutex<Option<(String, u16, String)>>>,
}

impl EcranAttenteState {
    /// Crée l'état + charge la config depuis le disque.
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let config = load_file(&app).unwrap_or_default();
        Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(config)),
            etat: Arc::new(Mutex::new(EcranAttenteRuntime::default())),
            timer_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
            derniere_frame: Arc::new(Mutex::new(None)),
            obs_creds: Arc::new(Mutex::new(None)),
        }
    }

    // ===== État (pour l'UI dashboard) =====

    /// Snapshot complet (config + runtime) pour la commande et l'event.
    pub fn etat_complet(&self) -> EcranAttenteEtat {
        let config = self.config.lock().unwrap().clone();
        let e = self.etat.lock().unwrap().clone();
        EcranAttenteEtat {
            config,
            actif: e.actif,
            en_pause: e.en_pause,
            temps_restant: e.temps_restant,
        }
    }

    /// Remplace la config + persiste + re-émet la frame WS si l'écran est
    /// actif (média/titre/son/volume appliqués en direct côté diffusion).
    /// Ne touche pas au chrono en cours (comme l'ancienne app).
    pub fn set_config(&self, config: EcranAttenteConfig) -> Result<(), String> {
        {
            *self.config.lock().unwrap() = config;
        }
        self.save_to_disk()?;
        if self.etat.lock().unwrap().actif {
            self.emit_frame();
        }
        self.emit_etat();
        Ok(())
    }

    // ===== Contrôles =====

    /// Lance l'écran d'attente : reset chrono à duree_s, frame WS, event
    /// Tauri, tâche tokio 1 Hz + routage audio OBS (spawn non-fatal).
    /// Relancer alors que l'écran est actif réinitialise le chrono
    /// (comportement de l'ancienne app).
    pub fn lancer(&self, host: &str, port: u16, password: &str) {
        // Mémoriser les creds AVANT tout : la restauration audio au stop auto
        // (fin du chrono) s'en sert sans nouvelle passe.
        *self.obs_creds.lock().unwrap() =
            Some((host.to_string(), port, password.to_string()));

        let config = self.config.lock().unwrap().clone();
        {
            let mut e = self.etat.lock().unwrap();
            e.actif = true;
            e.en_pause = false;
            e.temps_restant = config.duree_s.max(1);
        }

        self.emit_frame();
        self.emit_etat();
        self.demarrer_timer();
        self.spawn_config_audio(config);
    }

    /// Bascule pause/reprise du chrono. No-op si l'écran est inactif.
    /// Le média n'est pas affecté (la pause ne concerne que le chrono —
    /// comportement de l'ancienne app).
    pub fn toggle_pause(&self) {
        {
            let mut e = self.etat.lock().unwrap();
            if !e.actif {
                return;
            }
            e.en_pause = !e.en_pause;
        }
        self.emit_frame();
        self.emit_etat();
    }

    /// Arrête l'écran : annule le chrono, frame actif:false + restauration
    /// audio OBS (spawn non-fatal). Creds passés en priorité, sinon ceux
    /// mémorisés au lancer.
    pub fn arreter(&self, host: &str, port: u16, password: &str) {
        if !host.is_empty() {
            *self.obs_creds.lock().unwrap() =
                Some((host.to_string(), port, password.to_string()));
        }
        self.arreter_interne();
    }

    /// Arrêt interne : utilisé par la commande ET la fin de chrono (creds
    /// mémorisés au lancer). Idempotent.
    fn arreter_interne(&self) {
        {
            let mut e = self.etat.lock().unwrap();
            if !e.actif {
                return;
            }
            e.actif = false;
            e.en_pause = false;
        }
        // Annuler la tâche chrono (no-op si elle est déjà finie).
        self.timer_cancel.lock().unwrap().store(true, Ordering::SeqCst);
        self.emit_frame();
        self.emit_etat();
        self.spawn_restaurer_audio();
    }

    // ===== Chrono (tâche tokio 1 Hz — source de vérité) =====

    /// Démarre la tâche chrono : tick 1 s, décrémente `temps_restant`, émet
    /// frame WS + event Tauri. En pause → pas de décrément ni d'émission.
    /// À 0 → émet « 0:00 », attend ~1,5 s puis arrêt interne (avec
    /// restauration audio via les creds mémorisés).
    fn demarrer_timer(&self) {
        // Annuler la tâche précédente (relancer pendant un écran actif).
        {
            self.timer_cancel
                .lock()
                .unwrap()
                .store(true, Ordering::SeqCst);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.timer_cancel.lock().unwrap() = cancel.clone();

        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                // UN seul lock par statement (piège Mutex non réentrant).
                let (actif, en_pause, fini) = {
                    let mut e = state.etat.lock().unwrap();
                    if e.actif && !e.en_pause {
                        e.temps_restant = e.temps_restant.saturating_sub(1);
                    }
                    (e.actif, e.en_pause, e.actif && !e.en_pause && e.temps_restant == 0)
                };
                if !actif {
                    break;
                }
                if en_pause {
                    continue;
                }
                state.emit_frame();
                state.emit_etat();
                if fini {
                    // Laisser « 0:00 » visible ~1,5 s puis fermeture auto.
                    tokio::time::sleep(Duration::from_millis(DELAI_FIN_MS)).await;
                    if !cancel.load(Ordering::SeqCst) {
                        state.arreter_interne();
                    }
                    break;
                }
            }
        });
    }

    // ===== Émissions =====

    /// Construit et émet la frame `ecran-attente-etat` sur chat_tx →
    /// diffusion.html. La frame est auto-réparatrice : elle porte la config
    /// ET le runtime, ré-émise à chaque tick/transition. Gardée dans
    /// `derniere_frame` pour le resync à la reconnexion WS.
    fn emit_frame(&self) {
        let config = self.config.lock().unwrap().clone();
        let e = self.etat.lock().unwrap().clone();
        let frame = json!({
            "type": "ecran-attente-etat",
            "etat": {
                "actif": e.actif,
                "en_pause": e.en_pause,
                "temps_restant": e.temps_restant,
                "duree_s": config.duree_s,
                "titre": config.titre,
                "afficher_titre": config.afficher_titre,
                "son": config.son,
                "volume": config.volume,
                "media": config.media,
                "media_kind": config.media_kind,
                "minuteur_x": config.minuteur_x,
                "minuteur_y": config.minuteur_y,
                "minuteur_echelle": config.minuteur_echelle.clamp(0.4, 3.0),
                "layout": config.layout,
                "streamy_x": config.streamy_x,
                "streamy_y": config.streamy_y,
                "streamy_echelle": config.streamy_echelle.clamp(0.3, 2.0),
                "logo_x": config.logo_x,
                "logo_y": config.logo_y,
                "logo_echelle": config.logo_echelle.clamp(0.3, 2.0),
            }
        })
        .to_string();
        *self.derniere_frame.lock().unwrap() = Some(frame.clone());
        let _ = self.chat_tx.send(frame);
    }

    /// Émet `ecran-attente:etat` vers le dashboard (modale + pillule header).
    fn emit_etat(&self) {
        let _ = self.app.emit("ecran-attente:etat", self.etat_complet());
    }

    // ===== Audio OBS (one-shot, porté de l'ancienne app) =====

    /// Spawn non-fatal : configure l'audio OBS pour l'écran d'attente.
    fn spawn_config_audio(&self, config: EcranAttenteConfig) {
        let creds = self.obs_creds.lock().unwrap().clone();
        let Some((host, port, password)) = creds else {
            return;
        };
        if host.is_empty() {
            return;
        }
        tauri::async_runtime::spawn(async move {
            if let Err(e) = configurer_audio(
                &host,
                port,
                &password,
                config.source_audio.as_deref(),
                config.volume,
                config.muter_bureau,
            )
            .await
            {
                eprintln!("[EcranAttente] config audio OBS (non-fatal): {}", e);
            }
        });
    }

    /// Spawn non-fatal : réactive les sources audio bureau mutées au lancer.
    /// Prend (et consomme) les creds mémorisés.
    fn spawn_restaurer_audio(&self) {
        let creds = self.obs_creds.lock().unwrap().take();
        let Some((host, port, password)) = creds else {
            return;
        };
        if host.is_empty() {
            return;
        }
        tauri::async_runtime::spawn(async move {
            if let Err(e) = restaurer_audio(&host, port, &password).await {
                eprintln!("[EcranAttente] restauration audio OBS (non-fatal): {}", e);
            }
        });
    }

    // ===== Persistance =====

    /// Sauve la config sur disque (ecran_attente.json).
    fn save_to_disk(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let path = dir.join("ecran_attente.json");
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize ecran_attente.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire ecran_attente.json: {}", e))
    }
}

// =============================================================================
// FONCTIONS LIBRES
// =============================================================================

/// Re-émet la dernière frame WS vers la diffusion (resync à la reconnexion
/// WS — la diffusion envoie `ecran-attente-resync` à l'ouverture). Appelée
/// par server.rs. No-op si l'état n'est pas enregistré ou si aucune frame
/// n'a encore été émise.
pub fn reemit(app: &AppHandle) {
    let Some(state) = app.try_state::<EcranAttenteState>() else {
        return;
    };
    // Un seul lock par statement (piège Mutex non réentrant — AGENTS.md).
    let frame = state.derniere_frame.lock().unwrap().clone();
    if let Some(f) = frame {
        let _ = state.chat_tx.send(f);
    }
}

/// Liste les sources audio de sortie OBS (inputKind `wasapi_output_capture`)
/// — picker optionnel de la modale. Connexion one-shot (connect → request →
/// close) comme toutes les commandes OBS. Retourne les `inputName`.
pub async fn lister_sources_audio(
    host: &str,
    port: u16,
    password: &str,
) -> Result<Vec<String>, String> {
    let (mut write, mut read) = crate::obs::connect(host, port, password).await?;
    let result = crate::obs::rpc(
        &mut write,
        &mut read,
        "GetInputList",
        "ea_inputs",
        json!({}),
    )
    .await;
    let _ = write.close().await;

    let data = result?;
    let noms = data["inputs"]
        .as_array()
        .map(|inputs| {
            inputs
                .iter()
                .filter(|i| i["inputKind"].as_str() == Some("wasapi_output_capture"))
                .filter_map(|i| i["inputName"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(noms)
}

/// Configure l'audio de l'écran d'attente sur la scène SOS (connexion
/// one-shot, non-fatal côté appelant) :
///   1. Source `wasapi_output_capture` = celle choisie, sinon création de
///      « SOS Audio Écran d'attente » (idempotent : GetInputList d'abord).
///   2. `SetInputVolume` au volume configuré.
///   3. `SetSceneItemEnabled false` sur les items `wasapi_output_capture`
///      de la scène SOS dont le nom contient « desktop » ou « bureau »
///      (sauf la source dédiée) — l'audio du bureau ne fuit plus dans le
///      stream pendant l'attente. Étape sautée quand `muter_bureau` vaut
///      false (option de la modale « Connexion OBS ») — la restauration au
///      stop reste inconditionnelle (idempotente).
///
/// Porté de l'ancienne app (`configurer_audio_ecran_attente`). Seule la
/// scène « SOS » est touchée — jamais les autres scènes de l'utilisateur.
async fn configurer_audio(
    host: &str,
    port: u16,
    password: &str,
    source_audio: Option<&str>,
    volume: f32,
    muter_bureau: bool,
) -> Result<(), String> {
    let (mut write, mut read) = crate::obs::connect(host, port, password).await?;

    let result: Result<String, String> = async {
        // 1. Source audio dédiée : choisie ou créée.
        let nom_source = match source_audio {
            Some(nom) if !nom.is_empty() => nom.to_string(),
            _ => {
                let nom = NOM_SOURCE_AUDIO.to_string();
                let inputs = crate::obs::rpc(
                    &mut write,
                    &mut read,
                    "GetInputList",
                    "ea_inputs",
                    json!({}),
                )
                .await?;
                let existe = inputs["inputs"]
                    .as_array()
                    .map(|a| a.iter().any(|i| i["inputName"].as_str() == Some(nom.as_str())))
                    .unwrap_or(false);
                if !existe {
                    crate::obs::rpc(
                        &mut write,
                        &mut read,
                        "CreateInput",
                        "ea_create_audio",
                        json!({
                            "sceneName": crate::obs::SCENE_NAME,
                            "inputName": nom,
                            "inputKind": "wasapi_output_capture",
                            "sceneItemEnabled": true
                        }),
                    )
                    .await?;
                }
                nom
            }
        };

        // 2. Volume de la source dédiée (0.0 → 1.0).
        crate::obs::rpc(
            &mut write,
            &mut read,
            "SetInputVolume",
            "ea_volume",
            json!({
                "inputName": nom_source,
                "inputVolumeMul": volume.clamp(0.0, 1.0) as f64
            }),
        )
        .await?;

        // 3. Muter les sources audio bureau de la scène SOS (sauf la nôtre).
        //    Option muter_bureau (modale « Connexion OBS ») : false = skip —
        //    l'utilisateur garde son audio bureau pendant l'écran d'attente.
        if !muter_bureau {
            return Ok(nom_source);
        }
        let items = crate::obs::rpc(
            &mut write,
            &mut read,
            "GetSceneItemList",
            "ea_items",
            json!({ "sceneName": crate::obs::SCENE_NAME }),
        )
        .await?;
        if let Some(items) = items["sceneItems"].as_array() {
            for item in items {
                let nom = item["sourceName"].as_str().unwrap_or("");
                let kind = item["inputKind"].as_str().unwrap_or("");
                let item_id = item["sceneItemId"].as_i64().unwrap_or(0);
                let est_bureau = kind == "wasapi_output_capture"
                    && (nom.to_lowercase().contains("desktop")
                        || nom.to_lowercase().contains("bureau"))
                    && nom != nom_source;
                if est_bureau {
                    crate::obs::rpc(
                        &mut write,
                        &mut read,
                        "SetSceneItemEnabled",
                        "ea_mute_bureau",
                        json!({
                            "sceneName": crate::obs::SCENE_NAME,
                            "sceneItemId": item_id,
                            "sceneItemEnabled": false
                        }),
                    )
                    .await?;
                    log::info!("[EcranAttente] Audio bureau '{}' désactivé", nom);
                }
            }
        }
        Ok(nom_source)
    }
    .await;

    let _ = write.close().await;
    let nom = result?;
    log::info!("[EcranAttente] Audio OBS configuré (source='{}')", nom);
    Ok(())
}

/// Restaure l'audio à l'arrêt de l'écran d'attente : ré-active les items
/// `wasapi_output_capture` « desktop|bureau » de la scène SOS (connexion
/// one-shot). Porté de l'ancienne app (`restaurer_audio_ecran_attente`) —
/// réactivation par motif de nom, idempotent.
async fn restaurer_audio(host: &str, port: u16, password: &str) -> Result<(), String> {
    let (mut write, mut read) = crate::obs::connect(host, port, password).await?;

    let result: Result<(), String> = async {
        let items = crate::obs::rpc(
            &mut write,
            &mut read,
            "GetSceneItemList",
            "ea_items",
            json!({ "sceneName": crate::obs::SCENE_NAME }),
        )
        .await?;
        if let Some(items) = items["sceneItems"].as_array() {
            for item in items {
                let nom = item["sourceName"].as_str().unwrap_or("");
                let kind = item["inputKind"].as_str().unwrap_or("");
                let item_id = item["sceneItemId"].as_i64().unwrap_or(0);
                let est_bureau = kind == "wasapi_output_capture"
                    && (nom.to_lowercase().contains("desktop")
                        || nom.to_lowercase().contains("bureau"));
                if est_bureau {
                    crate::obs::rpc(
                        &mut write,
                        &mut read,
                        "SetSceneItemEnabled",
                        "ea_unmute_bureau",
                        json!({
                            "sceneName": crate::obs::SCENE_NAME,
                            "sceneItemId": item_id,
                            "sceneItemEnabled": true
                        }),
                    )
                    .await?;
                    log::info!("[EcranAttente] Audio bureau '{}' réactivé", nom);
                }
            }
        }
        Ok(())
    }
    .await;

    let _ = write.close().await;
    result
}

/// Charge la config depuis ecran_attente.json. Defaults si absent.
fn load_file(app: &AppHandle) -> Result<EcranAttenteConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("ecran_attente.json");
    if !path.exists() {
        return Ok(EcranAttenteConfig::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire ecran_attente.json: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Parse ecran_attente.json: {}", e))
}
