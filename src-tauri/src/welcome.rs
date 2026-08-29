/// Clips de bienvenue — détection 1er message + file d'attente + émission WS.
///
/// Flux :
///   1. Module chat (twitch_chat.rs, etc.) appelle `on_message(plateforme, cle, display, avatar)`.
///   2. Si cle est dans le registre (streamer enregistré) + actif + pas vu cette session
///      + config globale active → ajoute à `seen` + enfile dans la queue.
///   3. `avancer()` pop le prochain clip → émet `{"type":"welcome-clip-play",...}` sur chat_tx
///      (forwardé par server.rs vers diffusion.html :4321) + démarre un timer.
///   4. Timer (duree_ms + 2s sécurité) → `avancer()` au suivant (ou stop si queue vide).
///   5. Contrôles utilisateur : stop, skip, retirer, remonter, descendre, vider.
///
/// Persistance : `data/clips_bienvenue.json` (registre + config globale).
/// La queue et le seen set sont en RAM uniquement (pas persistés — reset au reboot).
///
/// Clé du registre : par plateforme. Twitch = login (lowercase), direct de l'IRC.
/// Le registre est namespaced : { twitch: { "<login>": ViewerConfig, ... } }.
use crate::config::data_dir;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

/// Jeton d'annulation du timer courant (pour stop/skip).
type TimerCancel = Arc<AtomicBool>;

/// Configuration globale des clips de bienvenue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigGlobale {
    pub actif: bool,
}

impl Default for ConfigGlobale {
    fn default() -> Self {
        Self { actif: true }
    }
}

/// Configuration d'un viewer (streamer fidèle) pour son clip de bienvenue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewerConfig {
    pub actif: bool,
    /// Slug/ID du clip Twitch.
    pub clip_id: String,
    pub clip_titre: String,
    pub clip_thumbnail: String,
    /// URL MP4 signée (résolue via GQL). Peut être vide si pas encore résolue
    /// → résolution à la volée au déclenchement.
    pub clip_mp4_url: String,
    /// Durée du clip en millisecondes.
    pub clip_duree_ms: u64,
    /// Message de bienvenue affiché avec le clip (optionnel).
    #[serde(default)]
    pub message: String,
    /// Display name du streamer (pour l'affichage, pas la clé).
    #[serde(default)]
    pub display_name: String,
    /// Avatar URL (pour l'affichage, mis à jour à la volée).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// Item dans la file d'attente.
#[derive(Debug, Clone, Serialize)]
pub struct QueueItem {
    /// ID unique de l'item (login + timestamp).
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub avatar: Option<String>,
    pub plateforme: String,
    pub clip_id: String,
    pub clip_titre: String,
    pub clip_mp4_url: String,
    pub clip_duree_ms: u64,
    pub message: String,
}

/// État de la file d'attente (pour l'UI dashboard).
#[derive(Debug, Clone, Serialize)]
pub struct QueueEtat {
    pub en_attente: Vec<QueueItem>,
    pub current: Option<QueueItem>,
    pub config_globale_actif: bool,
}

/// Format du fichier clips_bienvenue.json sur disque.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WelcomeFile {
    #[serde(default)]
    config_globale: ConfigGlobale,
    /// Registre Twitch : login (lowercase) → config.
    #[serde(default)]
    twitch: HashMap<String, ViewerConfig>,
}

/// État partagé welcome. Clonable (Arc internes) — passé aux commandes Tauri
/// et aux modules chat via tauri::State.
#[derive(Clone)]
pub struct WelcomeState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    /// Registre par plateforme : "twitch" → { login → ViewerConfig }.
    registry: Arc<Mutex<HashMap<String, HashMap<String, ViewerConfig>>>>,
    /// Logins déjà vus cette session (par plateforme). Reset au nouveau stream.
    seen: Arc<Mutex<HashSet<String>>>,
    /// File d'attente FIFO.
    queue: Arc<Mutex<VecDeque<QueueItem>>>,
    /// Clip en cours de lecture.
    current: Arc<Mutex<Option<QueueItem>>>,
    /// Config globale.
    config_globale: Arc<Mutex<ConfigGlobale>>,
    /// Jeton d'annulation du timer courant.
    timer_cancel: Arc<Mutex<TimerCancel>>,
}

impl WelcomeState {
    /// Crée l'état + charge le registre depuis le disque.
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let file = load_file(&app).unwrap_or_default();
        let mut registry = HashMap::new();
        registry.insert("twitch".to_string(), file.twitch.clone());

        let state = Self {
            app,
            chat_tx,
            registry: Arc::new(Mutex::new(registry)),
            seen: Arc::new(Mutex::new(HashSet::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            current: Arc::new(Mutex::new(None)),
            config_globale: Arc::new(Mutex::new(file.config_globale.clone())),
            timer_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
        };

        eprintln!(
            "[Welcome] init : {} viewer(s) Twitch, actif={}",
            file.twitch.len(),
            file.config_globale.actif
        );
        state
    }

    // ===== Détection 1er message =====

    /// Appelé par les modules chat à chaque message.
    /// Vérifie le registre + seen + config globale → enfile si éligible.
    pub fn on_message(
        &self,
        plateforme: &str,
        cle: &str,
        display_name: &str,
        avatar: Option<String>,
    ) {
        // 1. Config globale active ?
        if !self.config_globale.lock().unwrap().actif {
            return;
        }

        // 2. Clé dans le registre + config active ?
        let config = {
            let registry = self.registry.lock().unwrap();
            registry
                .get(plateforme)
                .and_then(|m| m.get(cle))
                .filter(|c| c.actif && !c.clip_id.is_empty())
                .cloned()
        };
        let Some(config) = config else {
            return;
        };

        // 3. Pas déjà vu cette session ?
        let seen_key = format!("{}_{}", plateforme, cle);
        {
            let mut seen = self.seen.lock().unwrap();
            if seen.contains(&seen_key) {
                return;
            }
            seen.insert(seen_key);
        }

        eprintln!(
            "[Welcome] 1er message de {} ({}) → enfile clip {}",
            display_name, plateforme, config.clip_id
        );

        // 4. Enfiler.
        let item = QueueItem {
            id: format!("{}_{}", cle, std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()),
            login: cle.to_string(),
            display_name: display_name.to_string(),
            avatar: avatar.clone(),
            plateforme: plateforme.to_string(),
            clip_id: config.clip_id.clone(),
            clip_titre: config.clip_titre.clone(),
            clip_mp4_url: config.clip_mp4_url.clone(),
            clip_duree_ms: config.clip_duree_ms,
            message: config.message.clone(),
        };

        {
            let mut queue = self.queue.lock().unwrap();
            queue.push_back(item);
        }

        // Mettre à jour l'avatar dans le registre si on en a un (cache).
        if let Some(av) = avatar {
            let mut registry = self.registry.lock().unwrap();
            if let Some(m) = registry.get_mut(plateforme) {
                if let Some(c) = m.get_mut(cle) {
                    c.avatar = Some(av);
                    c.display_name = display_name.to_string();
                }
            }
        }

        self.emit_etat();

        // 5. Si rien en cours → démarrer la lecture.
        let is_playing = self.current.lock().unwrap().is_some();
        if !is_playing {
            self.avancer();
        }
    }

    // ===== File d'attente : lecture =====

    /// Pop le prochain clip de la queue → émet welcome-clip-play + démarre timer.
    /// Si queue vide → émet welcome-clip-stop + clear current.
    /// SYNC : si l'URL MP4 doit être résolue (vide), spawn une tâche async
    /// `jouer_item_async` qui résout puis émet. Évite future non-Send.
    pub fn avancer(&self) {
        // Annuler le timer précédent.
        {
            let cancel = self.timer_cancel.lock().unwrap();
            cancel.store(true, Ordering::SeqCst);
        }

        // Pop le prochain item de la queue.
        let next = {
            let mut queue = self.queue.lock().unwrap();
            queue.pop_front()
        };

        let Some(item) = next else {
            // Queue vide → stop.
            eprintln!("[Welcome] queue vide → stop");
            *self.current.lock().unwrap() = None;
            let msg = serde_json::json!({ "type": "welcome-clip-stop" });
            let _ = self.chat_tx.send(msg.to_string());
            self.emit_etat();
            return;
        };

        if item.clip_mp4_url.is_empty() {
            // URL MP4 à résoudre → spawn tâche async.
            eprintln!("[Welcome] résolution MP4 à la volée pour slug={}", item.clip_id);
            let state = self.clone();
            tokio::spawn(async move {
                state.jouer_item_async(item).await;
            });
        } else {
            // URL déjà résolue → émettre directement.
            self.jouer_item(item);
        }
    }

    /// Résout l'URL MP4 d'un item puis le joue. Si échec → skip (avancer).
    async fn jouer_item_async(&self, item: QueueItem) {
        match crate::twitch_clips::resoudre_mp4(&item.clip_id).await {
            Ok(url) => {
                let mut resolved = item.clone();
                resolved.clip_mp4_url = url;
                // Mettre à jour le registre (cache persistant).
                {
                    let mut registry = self.registry.lock().unwrap();
                    if let Some(m) = registry.get_mut(&item.plateforme) {
                        if let Some(c) = m.get_mut(&item.login) {
                            c.clip_mp4_url = resolved.clip_mp4_url.clone();
                        }
                    }
                }
                let _ = self.save_to_disk();
                self.jouer_item(resolved);
            }
            Err(e) => {
                eprintln!("[Welcome] résolution MP4 échouée pour {} : {} — skip", item.login, e);
                // Skip → avancer au suivant.
                self.avancer();
            }
        }
    }

    /// Émet welcome-clip-play pour un item dont l'URL MP4 est résolue + démarre timer.
    fn jouer_item(&self, item: QueueItem) {
        eprintln!(
            "[Welcome] lecture clip {} ({}) duree={}ms",
            item.display_name, item.clip_id, item.clip_duree_ms
        );

        // Émettre welcome-clip-play sur le WS :4321.
        let msg = serde_json::json!({
            "type": "welcome-clip-play",
            "viewer": {
                "login": item.login,
                "display_name": item.display_name,
                "avatar": item.avatar,
            },
            "clip": {
                "mp4_url": item.clip_mp4_url,
                "titre": item.clip_titre,
                "duree_ms": item.clip_duree_ms,
            },
            "message": item.message,
        });
        let _ = self.chat_tx.send(msg.to_string());

        // Stocker current.
        *self.current.lock().unwrap() = Some(item.clone());

        // Démarrer le timer (duree + 2s sécurité).
        let duree_ms = item.clip_duree_ms.max(1000); // min 1s
        let state = self.clone();
        let new_cancel = Arc::new(AtomicBool::new(false));
        *self.timer_cancel.lock().unwrap() = new_cancel.clone();

        tokio::spawn(async move {
            let cancel = new_cancel;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(duree_ms + 2000)) => {
                    if !cancel.load(Ordering::SeqCst) {
                        eprintln!("[Welcome] timer écoulé → avancer");
                        state.avancer();
                    }
                }
                _ = await_timer_cancel(&cancel) => {
                    eprintln!("[Welcome] timer annulé (stop/skip)");
                }
            }
        });

        self.emit_etat();
    }

    // ===== File d'attente : contrôles utilisateur =====

    /// Stop le clip courant + vide la queue. Émet welcome-clip-stop.
    pub fn stop(&self) {
        // Annuler le timer.
        self.timer_cancel.lock().unwrap().store(true, Ordering::SeqCst);
        // Vider la queue.
        self.queue.lock().unwrap().clear();
        // Clear current.
        *self.current.lock().unwrap() = None;
        // Émettre stop.
        let msg = serde_json::json!({ "type": "welcome-clip-stop" });
        let _ = self.chat_tx.send(msg.to_string());
        eprintln!("[Welcome] stop + queue vidée");
        self.emit_etat();
    }

    /// Skip le clip courant → passe au suivant.
    pub fn skip(&self) {
        eprintln!("[Welcome] skip clip courant");
        self.avancer();
    }

    /// Retire un item spécifique de la queue (par id).
    pub fn retirer(&self, id: &str) {
        let mut queue = self.queue.lock().unwrap();
        let before = queue.len();
        queue.retain(|item| item.id != id);
        let after = queue.len();
        if before != after {
            eprintln!("[Welcome] retiré item {} (queue {}→{})", id, before, after);
            self.emit_etat();
        }
    }

    /// Remonte un item dans la queue (vers le début).
    pub fn remonter(&self, id: &str) {
        let mut queue = self.queue.lock().unwrap();
        if let Some(pos) = queue.iter().position(|item| item.id == id) {
            if pos > 0 {
                let item = queue.remove(pos).unwrap();
                queue.insert(pos - 1, item);
                self.emit_etat();
            }
        }
    }

    /// Descend un item dans la queue (vers la fin).
    pub fn descendre(&self, id: &str) {
        let mut queue = self.queue.lock().unwrap();
        if let Some(pos) = queue.iter().position(|item| item.id == id) {
            if pos < queue.len() - 1 {
                let item = queue.remove(pos).unwrap();
                queue.insert(pos + 1, item);
                self.emit_etat();
            }
        }
    }

    /// Vide la queue (sans stopper le clip courant).
    pub fn vider(&self) {
        self.queue.lock().unwrap().clear();
        eprintln!("[Welcome] queue vidée");
        self.emit_etat();
    }

    /// Reset le seen set (nouveau stream → tous les streamers redeviennent éligibles).
    pub fn reset_session(&self) {
        self.seen.lock().unwrap().clear();
        eprintln!("[Welcome] session reset (seen cleared)");
    }

    // ===== État (pour l'UI dashboard) =====

    /// Retourne l'état courant de la queue (snapshot).
    pub fn etat(&self) -> QueueEtat {
        let queue: Vec<QueueItem> = self.queue.lock().unwrap().iter().cloned().collect();
        let current = self.current.lock().unwrap().clone();
        let actif = self.config_globale.lock().unwrap().actif;
        QueueEtat {
            en_attente: queue,
            current,
            config_globale_actif: actif,
        }
    }

    /// Émet l'état vers le dashboard (event Tauri welcome:etat).
    fn emit_etat(&self) {
        let etat = self.etat();
        let _ = self.app.emit("welcome:etat", &etat);
    }

    // ===== Registre CRUD =====

    /// Lit le registre Twitch complet (pour l'UI).
    pub fn lire_registre_twitch(&self) -> Vec<(String, ViewerConfig)> {
        let registry = self.registry.lock().unwrap();
        registry
            .get("twitch")
            .map(|m| {
                let mut v: Vec<(String, ViewerConfig)> = m.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                v.sort_by(|a, b| a.0.cmp(&b.0));
                v
            })
            .unwrap_or_default()
    }

    /// Sauvegarde la config d'un viewer dans le registre + sur disque.
    pub fn sauver_viewer_twitch(&self, login: &str, config: ViewerConfig) -> Result<(), String> {
        {
            let mut registry = self.registry.lock().unwrap();
            let twitch = registry
                .entry("twitch".to_string())
                .or_insert_with(HashMap::new);
            twitch.insert(login.to_lowercase(), config);
        }
        self.save_to_disk()?;
        eprintln!("[Welcome] viewer Twitch sauvé : {}", login);
        Ok(())
    }

    /// Supprime un viewer du registre + sur disque.
    pub fn supprimer_viewer_twitch(&self, login: &str) -> Result<(), String> {
        {
            let mut registry = self.registry.lock().unwrap();
            if let Some(twitch) = registry.get_mut("twitch") {
                twitch.remove(&login.to_lowercase());
            }
        }
        self.save_to_disk()?;
        eprintln!("[Welcome] viewer Twitch supprimé : {}", login);
        Ok(())
    }

    /// Active/désactive la config globale.
    pub fn set_config_globale_actif(&self, actif: bool) -> Result<(), String> {
        self.config_globale.lock().unwrap().actif = actif;
        self.save_to_disk()?;
        self.emit_etat();
        eprintln!("[Welcome] config globale actif={}", actif);
        Ok(())
    }

    // ===== Persistance =====

    /// Sauve le registre + config globale sur disque (clips_bienvenue.json).
    fn save_to_disk(&self) -> Result<(), String> {
        let registry = self.registry.lock().unwrap();
        let config = self.config_globale.lock().unwrap();
        let twitch = registry.get("twitch").cloned().unwrap_or_default();

        let file = WelcomeFile {
            config_globale: config.clone(),
            twitch,
        };

        let dir = data_dir(&self.app)?;
        let path = dir.join("clips_bienvenue.json");
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("Serialize clips_bienvenue.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire clips_bienvenue.json: {}", e))?;
        Ok(())
    }
}

// ===== Fonctions libres =====

/// Charge le fichier clips_bienvenue.json depuis le disque.
fn load_file(app: &AppHandle) -> Result<WelcomeFile, String> {
    let dir = data_dir(app)?;
    let path = dir.join("clips_bienvenue.json");
    if !path.exists() {
        return Ok(WelcomeFile::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire clips_bienvenue.json: {}", e))?;
    let file: WelcomeFile = serde_json::from_str(&content)
        .map_err(|e| format!("Parse clips_bienvenue.json: {}", e))?;
    Ok(file)
}

/// Attend que le jeton d'annulation passe à true (polling 100ms).
async fn await_timer_cancel(cancel: &TimerCancel) {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
