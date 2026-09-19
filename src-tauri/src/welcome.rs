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
use crate::position_overlay::PositionOverlayState;
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
    /// Configuration de l'overlay côté diffusion (position/taille en pixels
    /// canvas). L'overlay est autonome (plus un widget de scène). Défaut :
    /// bas-droite 480x270 sur canvas 1920x1080.
    #[serde(default = "default_overlay_config")]
    pub overlay: OverlayConfig,
    /// Durée d'affichage forcée en millisecondes. 0 = utiliser la durée
    /// naturelle du clip. > 0 = forcer l'affichage pendant exactement cette
    /// durée (le clip est coupé ou prolongé selon le cas).
    #[serde(default)]
    pub duree_affichage_ms: u64,
}

/// Configuration de l'overlay welcome côté diffusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
}

fn default_overlay_config() -> OverlayConfig {
    OverlayConfig {
        x: 1480.0,
        y: 580.0,
        largeur: 320.0,
        hauteur: 352.0,
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        default_overlay_config()
    }
}

impl Default for ConfigGlobale {
    fn default() -> Self {
        Self {
            actif: true,
            overlay: OverlayConfig::default(),
            duree_affichage_ms: 0,
        }
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
    /// Bio Twitch (description de la chaîne), mise à jour à la volée.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
}

/// Item dans la file d'attente.
#[derive(Debug, Clone, Serialize)]
pub struct QueueItem {
    /// ID unique de l'item (login + timestamp).
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub avatar: Option<String>,
    /// Bio Twitch (description de la chaîne) du viewer.
    pub bio: Option<String>,
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
    /// Configuration de l'overlay côté diffusion (pour l'UI de réglage).
    pub overlay: OverlayConfig,
    /// Durée d'affichage forcée en ms (0 = durée naturelle du clip).
    pub duree_affichage_ms: u64,
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

        Self {
            app,
            chat_tx,
            registry: Arc::new(Mutex::new(registry)),
            seen: Arc::new(Mutex::new(HashSet::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            current: Arc::new(Mutex::new(None)),
            config_globale: Arc::new(Mutex::new(file.config_globale.clone())),
            timer_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
        }
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
        bio: Option<String>,
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

        // 4. Enfiler.
        let item = QueueItem {
            id: format!("{}_{}", cle, std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()),
            login: cle.to_string(),
            display_name: display_name.to_string(),
            avatar: avatar.clone(),
            bio: bio.clone(),
            plateforme: plateforme.to_string(),
            clip_id: config.clip_id.clone(),
            clip_titre: config.clip_titre.clone(),
            clip_mp4_url: config.clip_mp4_url.clone(),
            clip_duree_ms: config.clip_duree_ms,
            message: config.message.clone(),
        };

        {
            let mut queue = self.queue.lock().unwrap();
            queue.push_back(item.clone());
        }

        // Pré-résoudre l'URL MP4 en arrière-plan (toujours, même si l'URL
        // existe dans le registre — elle peut être expirée côté CDN Twitch).
        // resoudre_mp4 vérifie le cache (TTL 10 min) → cache hit = instantané.
        // Ainsi, quand avancer() poppera cet item, le cache est chaud →
        // résolution immédiate sans latence réseau.
        {
            let state = self.clone();
            let item_id = item.id.clone();
            let clip_id = item.clip_id.clone();
            let plateforme = item.plateforme.clone();
            let login = item.login.clone();
            tauri::async_runtime::spawn(async move {
                state.preresoudre_queue_item(&item_id, &clip_id, &plateforme, &login).await;
            });
        }

        // Mettre à jour l'avatar + la bio dans le registre si on en a (cache).
        if avatar.is_some() || bio.is_some() {
            let mut registry = self.registry.lock().unwrap();
            if let Some(m) = registry.get_mut(plateforme) {
                if let Some(c) = m.get_mut(cle) {
                    if let Some(av) = avatar {
                        c.avatar = Some(av);
                    }
                    if let Some(b) = bio {
                        c.bio = Some(b);
                    }
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
            *self.current.lock().unwrap() = None;
            let msg = serde_json::json!({ "type": "welcome-clip-stop" });
            let _ = self.chat_tx.send(msg.to_string());
            self.emit_etat();
            return;
        };

        // Marquer `current` IMMÉDIATEMENT (avant la résolution async) pour
        // bloquer les appels concurrents à avancer() depuis on_message.
        // Sans ça, plusieurs viewers arrivant en grappe pendant la résolution
        // réseau (1-3s) déclenchent plusieurs avancer() → timers orphelins →
        // welcome-clip-stop prématuré qui coupe le clip du 3ème viewer.
        *self.current.lock().unwrap() = Some(item.clone());
        self.emit_etat();

        // Toujours résoudre via le cache (resoudre_mp4 vérifie le TTL 10 min).
        // Si l'URL vient du registre persistant (sauvé lors d'une session
        // précédente), elle peut être expirée côté CDN Twitch → re-résoudre.
        // Cache hit = instantané (HashMap lookup), cache miss = réseau GQL.
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            state.jouer_item_async(item).await;
        });
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

    /// Pré-résout l'URL MP4 d'un item de la queue (en arrière-plan, appelé
    /// dès l'enqueue). Met à jour l'item in-place dans la queue + le registre.
    /// Si l'item a déjà été poppé par avancer() entre-temps, ne fait rien
    /// (avancer() aura spawné jouer_item_async qui gère la résolution).
    async fn preresoudre_queue_item(
        &self,
        item_id: &str,
        clip_id: &str,
        plateforme: &str,
        login: &str,
    ) {
        match crate::twitch_clips::resoudre_mp4(clip_id).await {
            Ok(url) => {
                // Mettre à jour l'item dans la queue (s'il y est encore).
                let mut found = false;
                {
                    let mut queue = self.queue.lock().unwrap();
                    for item in queue.iter_mut() {
                        if item.id == item_id {
                            item.clip_mp4_url = url.clone();
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    // Mettre à jour le registre (cache persistant).
                    {
                        let mut registry = self.registry.lock().unwrap();
                        if let Some(m) = registry.get_mut(plateforme) {
                            if let Some(c) = m.get_mut(login) {
                                c.clip_mp4_url = url;
                            }
                        }
                    }
                    let _ = self.save_to_disk();
                } else {
                    // Item déjà poppé par avancer() → jouer_item_async gère.
                }
            }
            Err(e) => {
                eprintln!("[Welcome] pré-résolution MP4 échouée pour {} : {}", login, e);
            }
        }
    }

    /// Émet welcome-clip-play pour un item dont l'URL MP4 est résolue + démarre timer.
    fn jouer_item(&self, mut item: QueueItem) {
        // Fallback avatar/bio depuis le registre (cache) si absents de l'item.
        if item.avatar.is_none() || item.bio.is_none() {
            let registry = self.registry.lock().unwrap();
            if let Some(c) = registry.get(&item.plateforme).and_then(|m| m.get(&item.login)) {
                if item.avatar.is_none() {
                    item.avatar = c.avatar.clone();
                }
                if item.bio.is_none() {
                    item.bio = c.bio.clone();
                }
            }
        }

        // Pousser la config overlay avant le play (garantit que le client
        // applique la position/taille courante, même s'il a (re)connecté après
        // un changement de config sans clip en cours).
        self.emit_overlay_config();

        // Émettre welcome-clip-play sur le WS :4321.
        let msg = serde_json::json!({
            "type": "welcome-clip-play",
            "viewer": {
                "login": item.login,
                "display_name": item.display_name,
                "avatar": item.avatar,
                "bio": item.bio,
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

        // Démarrer le timer.
        // - Si duree_affichage_ms > 0 (config globale) : forcer cette durée.
        // - Sinon : durée naturelle du clip (+ 2s sécurité pour le buffering).
        let duree_affichage = self.config_globale.lock().unwrap().duree_affichage_ms;
        let timer_ms = if duree_affichage > 0 {
            duree_affichage
        } else {
            item.clip_duree_ms.max(1000) + 2000 // min 1s + sécurité
        };
        let state = self.clone();
        let new_cancel = Arc::new(AtomicBool::new(false));
        *self.timer_cancel.lock().unwrap() = new_cancel.clone();

        tauri::async_runtime::spawn(async move {
            let cancel = new_cancel;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(timer_ms)) => {
                    if !cancel.load(Ordering::SeqCst) {
                        state.avancer();
                    }
                }
                _ = await_timer_cancel(&cancel) => {}
            }
        });

        self.emit_etat();
    }

    // ===== Test manuel (modale Interaction viewer) =====

    /// Lance un clip côté diffusion SANS passer par la queue (test manuel
    /// depuis la modale). Résout l'URL MP4 du clip, émet welcome-clip-play,
    /// démarre un timer qui émet welcome-clip-stop à la fin. N'interfère pas
    /// avec la queue réelle (current est mis à jour pour que le timer soit
    /// annulable via stop, mais la queue n'est pas modifiée).
    pub async fn tester_clip(
        &self,
        clip_id: &str,
        clip_titre: &str,
        clip_duree_ms: u64,
        display_name: &str,
        avatar: Option<String>,
        bio: Option<String>,
    ) -> Result<(), String> {
        // Résoudre l'URL MP4.
        let mp4_url = crate::twitch_clips::resoudre_mp4(clip_id).await?;

        // Annuler le timer précédent (si un clip tournait).
        {
            let cancel = self.timer_cancel.lock().unwrap();
            cancel.store(true, Ordering::SeqCst);
        }

        // Pousser la config overlay (garantit position/taille courante).
        self.emit_overlay_config();

        // Fallback avatar/bio depuis le registre (cache) si absents.
        let login = display_name.to_lowercase();
        let (avatar, bio) = {
            let registry = self.registry.lock().unwrap();
            let cached = registry.get("twitch").and_then(|m| m.get(&login));
            (
                avatar.or_else(|| cached.and_then(|c| c.avatar.clone())),
                bio.or_else(|| cached.and_then(|c| c.bio.clone())),
            )
        };

        // Émettre welcome-clip-play.
        let msg = serde_json::json!({
            "type": "welcome-clip-play",
            "viewer": {
                "login": login,
                "display_name": display_name,
                "avatar": avatar.clone(),
                "bio": bio.clone(),
            },
            "clip": {
                "mp4_url": mp4_url,
                "titre": clip_titre,
                "duree_ms": clip_duree_ms,
            },
            "message": format!("Bienvenue {} !", display_name),
        });
        let _ = self.chat_tx.send(msg.to_string());

        // Stocker un current factice (pour que stop puisse annuler le timer).
        let item = QueueItem {
            id: format!("test_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()),
            login: display_name.to_lowercase(),
            display_name: display_name.to_string(),
            avatar,
            bio,
            plateforme: "twitch".to_string(),
            clip_id: clip_id.to_string(),
            clip_titre: clip_titre.to_string(),
            clip_mp4_url: String::new(),
            clip_duree_ms,
            message: format!("Bienvenue {} !", display_name),
        };
        *self.current.lock().unwrap() = Some(item);

        // Démarrer le timer → welcome-clip-stop.
        // - Si duree_affichage_ms > 0 (config globale) : forcer cette durée.
        // - Sinon : durée naturelle du clip (+ 2s sécurité).
        let duree_affichage = self.config_globale.lock().unwrap().duree_affichage_ms;
        let timer_ms = if duree_affichage > 0 {
            duree_affichage
        } else {
            clip_duree_ms.max(1000) + 2000
        };
        let state = self.clone();
        let new_cancel = Arc::new(AtomicBool::new(false));
        *self.timer_cancel.lock().unwrap() = new_cancel.clone();

        tauri::async_runtime::spawn(async move {
            let cancel = new_cancel;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(timer_ms)) => {
                    if !cancel.load(Ordering::SeqCst) {
                        let msg = serde_json::json!({ "type": "welcome-clip-stop" });
                        let _ = state.chat_tx.send(msg.to_string());
                        *state.current.lock().unwrap() = None;
                        state.emit_etat();
                    }
                }
                _ = await_timer_cancel(&cancel) => {}
            }
        });

        self.emit_etat();
        Ok(())
    }

    /// Met à jour la durée d'affichage globale (0 = durée naturelle du clip).
    /// Persiste + émet l'état. N'affecte que les prochains clips (le clip
    /// courant garde son timer déjà démarré).
    pub fn set_duree_affichage(&self, ms: u64) -> Result<(), String> {
        self.config_globale.lock().unwrap().duree_affichage_ms = ms;
        self.save_to_disk()?;
        self.emit_etat();
        Ok(())
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
        self.emit_etat();
    }

    /// Skip le clip courant → passe au suivant.
    pub fn skip(&self) {
        self.avancer();
    }

    /// Retire un item spécifique de la queue (par id).
    pub fn retirer(&self, id: &str) {
        let mut queue = self.queue.lock().unwrap();
        let before = queue.len();
        queue.retain(|item| item.id != id);
        let after = queue.len();
        if before != after {
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
        self.emit_etat();
    }

    /// Reset le seen set (nouveau stream → tous les streamers redeviennent éligibles).
    pub fn reset_session(&self) {
        self.seen.lock().unwrap().clear();
    }

    // ===== État (pour l'UI dashboard) =====

    /// Retourne l'état courant de la queue (snapshot).
    pub fn etat(&self) -> QueueEtat {
        let queue: Vec<QueueItem> = self.queue.lock().unwrap().iter().cloned().collect();
        let current = self.current.lock().unwrap().clone();
        let cfg = self.config_globale.lock().unwrap();
        // La position/taille de l'overlay est désormais pilotée par le
        // squelette de position unifié (position_overlay.rs). On lit la
        // valeur depuis ce state partagé (fallback sur la config locale si
        // le state n'est pas encore enregistré — ne devrait pas arriver).
        let overlay = PositionOverlayState::from_app(&self.app)
            .map(|s| {
                let p = s.etat();
                OverlayConfig {
                    x: p.x,
                    y: p.y,
                    largeur: p.largeur,
                    hauteur: p.hauteur,
                }
            })
            .unwrap_or_else(|| cfg.overlay.clone());
        QueueEtat {
            en_attente: queue,
            current,
            config_globale_actif: cfg.actif,
            overlay,
            duree_affichage_ms: cfg.duree_affichage_ms,
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
                .or_default();
            twitch.insert(login.to_lowercase(), config);
        }
        self.save_to_disk()?;
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
        Ok(())
    }

    /// Active/désactive la config globale.
    pub fn set_config_globale_actif(&self, actif: bool) -> Result<(), String> {
        self.config_globale.lock().unwrap().actif = actif;
        self.save_to_disk()?;
        self.emit_etat();
        Ok(())
    }

    /// Met à jour la config de l'overlay (position/taille) + persiste + émet
    /// welcome-clip-config sur le WS :4321 (mise à jour live côté diffusion).
    ///
    /// Désormais, la source de vérité est le squelette de position unifié
    /// (position_overlay.rs). Cette méthode délègue à `PositionOverlayState::set`
    /// qui persiste `position_overlay.json` et émet `position-overlay-config`
    /// sur le WS (applique aux deux overlays en live). On garde l'émission
    /// `welcome-clip-config` pour rétro-compat (au cas où diffusion n'écoute
    /// que ce handler).
    pub fn set_overlay_config(&self, cfg: OverlayConfig) -> Result<(), String> {
        if let Some(state) = PositionOverlayState::from_app(&self.app) {
            state.set(crate::position_overlay::PositionOverlayConfig {
                x: cfg.x,
                y: cfg.y,
                largeur: cfg.largeur,
                hauteur: cfg.hauteur,
            })?;
        }
        // Aussi émettre welcome-clip-config (rétro-compat diffusion.html).
        self.emit_overlay_config();
        Ok(())
    }

    /// Émet welcome-clip-config sur le WS :4321 (forwardé vers diffusion.html).
    /// Lit la position depuis le squelette unifié (PositionOverlayState).
    fn emit_overlay_config(&self) {
        let cfg = PositionOverlayState::from_app(&self.app)
            .map(|s| {
                let p = s.etat();
                OverlayConfig {
                    x: p.x,
                    y: p.y,
                    largeur: p.largeur,
                    hauteur: p.hauteur,
                }
            })
            .unwrap_or_else(|| self.config_globale.lock().unwrap().overlay.clone());
        let msg = serde_json::json!({
            "type": "welcome-clip-config",
            "config": {
                "x": cfg.x,
                "y": cfg.y,
                "largeur": cfg.largeur,
                "hauteur": cfg.hauteur,
            },
        });
        let _ = self.chat_tx.send(msg.to_string());
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
/// Migre l'overlay vers le ratio portrait 9:13 si l'ancienne config était en
/// paysage (versions précédentes utilisaient 480×270).
fn load_file(app: &AppHandle) -> Result<WelcomeFile, String> {
    let dir = data_dir(app)?;
    let path = dir.join("clips_bienvenue.json");
    if !path.exists() {
        return Ok(WelcomeFile::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire clips_bienvenue.json: {}", e))?;
    let mut file: WelcomeFile = serde_json::from_str(&content)
        .map_err(|e| format!("Parse clips_bienvenue.json: {}", e))?;

    // Migration : si l'overlay est en paysage (hauteur < largeur), le passer
    // au ratio de la carte (avatar + bio + clip 16:9) en gardant la largeur.
    let wc_ratio = 11.0 / 10.0;
    if file.config_globale.overlay.hauteur < file.config_globale.overlay.largeur {
        let w = file.config_globale.overlay.largeur;
        let new_h = (w * wc_ratio).round();
        file.config_globale.overlay.hauteur = new_h;
    }

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
