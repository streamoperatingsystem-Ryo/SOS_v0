/// Pad numérique — 16 touches Numpad × 3 plages, déclenchement sons/images/vidéos.
///
/// Porté depuis l'ancienne app RUST_SOS_2026 (features/pad-numerique/), réécrit
/// pour l'architecture v0 (même pattern que alertes.rs / commandes.rs) :
///   - Config persistée `pad_numerique.json` : actif, plage_actuelle, touches
///     (HashMap clé "Numpad1_0" → media_type/media/son/duree_ms/volume/bounce/zoom).
///   - Capture clavier GLOBALE : thread poll `GetAsyncKeyState` 60Hz (dédié,
///     indépendant de input_viewer.rs). Détecte fronts down des touches Numpad.
///   - NumpadAdd / NumpadSubtract : touches réservées (navigation plages).
///   - Émission WS sur chat_tx : `pad-play` + `pad-stop` → diffusion.html
///     (overlay z 9995, même squelette de position que welcome/alertes/commandes).
///   - Chrono 2 (media_type "chrono") : chronomètre INDÉPENDANT démarrant à
///     zéro à la pression de la touche — tick 10 Hz, frames WS `chrono2-etat`
///     auto-réparatrices (état complet + position du squelette DÉDIÉ
///     `position_chrono.json` à chaque frame), event Tauri miroir
///     `chrono2:etat`, arrêt automatique à `chrono_duree_s` (frame fin +
///     sons). Un seul chrono à la fois : une nouvelle pression le remplace
///     (compteur de génération). Pas d'annulation croisée avec pad-stop.
///   - Émission event Tauri `pad:touche` → dashboard joue l'audio local
///     (le user entend sur son PC, en plus de la diffusion captée par OBS).
///
/// Audio : joué des deux côtés (diffusion OBS capte pour les viewers +
/// dashboard pour le user). Vidéo : son porté par la vidéo côté diffusion
/// uniquement (pas de doublon dashboard).
use crate::config::data_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

// =============================================================================
// CONSTANTES (identiques à l'ancienne app)
// =============================================================================

/// Disposition du pad numérique (4 colonnes × 4 lignes).
/// Référence — le frontend définit sa propre copie dans padNumerique.ts.
#[allow(dead_code)]
pub const DISPOSITION_PAD: &[&[&str]] = &[
    &["Numpad7", "Numpad8", "Numpad9", "NumpadDivide"],
    &["Numpad4", "Numpad5", "Numpad6", "NumpadMultiply"],
    &["Numpad1", "Numpad2", "Numpad3", "NumpadSubtract"],
    &["Numpad0", "NumpadDecimal", "NumpadEnter", "NumpadAdd"],
];

/// Touches réservées (non assignables, navigation entre plages).
/// Référence — le frontend définit sa propre copie dans padNumerique.ts.
#[allow(dead_code)]
pub const TOUCHES_RESERVEES: &[&str] = &["NumpadAdd", "NumpadSubtract"];

/// Codes de touches assignables (exclut + et - qui sont réservés).
pub const CODES_ASSIGNABLES: &[&str] = &[
    "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4",
    "Numpad5", "Numpad6", "Numpad7", "Numpad8", "Numpad9",
    "NumpadDecimal", "NumpadEnter",
    "NumpadMultiply", "NumpadDivide",
];

/// Nombre de plages (0, 1, 2).
pub const NB_PLAGES: u32 = 3;

/// Période du poll clavier (60Hz).
const PERIODE_POLL_MS: u64 = 16;

// =============================================================================
// CONFIG (sérialisable, persistée dans pad_numerique.json)
// =============================================================================

/// Configuration d'une touche du pad.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToucheConfig {
    /// Type de média : "audio" | "image" | "video". None = touche vide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Chemin relatif du média ("medias/<uuid>.<ext>"). None = pas de média.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    /// Kind du média : "image" | "video" (pour image/video).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_kind: Option<String>,
    /// Chemin relatif du son ("medias/<uuid>.<ext>"). None = silence.
    /// Pour audio : c'est le média principal. Pour image : son séparé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub son: Option<String>,
    /// Durée d'affichage en ms (1000-60000). Défaut 5000.
    #[serde(default = "default_duree_ms")]
    pub duree_ms: u64,
    /// Volume de lecture (0.0-1.0). Défaut 1.0.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Effet bounce activé (image/video).
    #[serde(default)]
    pub bounce: bool,
    /// Effet zoom activé (image/video).
    #[serde(default)]
    pub zoom: bool,
    /// Affichage plein écran (image/video) : le média couvre tout le canvas
    /// de diffusion — position d'overlay ignorée, pas de cadre SVG.
    #[serde(default)]
    pub plein_ecran: bool,
    /// Transparence (superposition) : opacité réduite du média plein écran
    /// pour laisser la scène visible derrière.
    #[serde(default)]
    pub transparence: bool,
    /// Niveau d'opacité de la transparence (0.0-1.0). Défaut 0.5.
    #[serde(default = "default_transparence_opacite")]
    pub transparence_opacite: f32,
    /// Durée cible préprogrammée du chrono en secondes (media_type "chrono").
    /// Le chrono démarre à zéro et s'arrête automatiquement à cette durée.
    #[serde(default = "default_chrono_duree_s")]
    pub chrono_duree_s: u64,
    /// Son de départ du chrono ("medias/<uuid>.<ext>"). None = silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrono_son_depart: Option<String>,
    /// Son d'alerte de fin du chrono ("medias/<uuid>.<ext>"). None = silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrono_son_fin: Option<String>,
}

fn default_duree_ms() -> u64 {
    5000
}
fn default_volume() -> f32 {
    1.0
}
fn default_transparence_opacite() -> f32 {
    0.5
}
fn default_chrono_duree_s() -> u64 {
    60
}

impl Default for ToucheConfig {
    fn default() -> Self {
        Self {
            media_type: None,
            media: None,
            media_kind: None,
            son: None,
            duree_ms: default_duree_ms(),
            volume: default_volume(),
            bounce: false,
            zoom: false,
            plein_ecran: false,
            transparence: false,
            transparence_opacite: default_transparence_opacite(),
            chrono_duree_s: default_chrono_duree_s(),
            chrono_son_depart: None,
            chrono_son_fin: None,
        }
    }
}

/// Config globale du pad (persistée).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PadConfig {
    #[serde(default)]
    pub actif: bool,
    #[serde(default)]
    pub plage_actuelle: u32,
    /// Clé "NumpadX_plage" → config de la touche.
    #[serde(default = "default_touches")]
    pub touches: HashMap<String, ToucheConfig>,
}

fn default_touches() -> HashMap<String, ToucheConfig> {
    let mut touches = HashMap::new();
    for plage in 0..NB_PLAGES {
        for code in CODES_ASSIGNABLES {
            touches.insert(format!("{}_{}", code, plage), ToucheConfig::default());
        }
    }
    touches
}

impl Default for PadConfig {
    fn default() -> Self {
        Self {
            actif: true,
            plage_actuelle: 0,
            touches: default_touches(),
        }
    }
}

// =============================================================================
// ÉTAT PARTAGÉ
// =============================================================================

/// État partagé pad numérique. Clonable (Arc internes).
#[derive(Clone)]
pub struct PadNumeriqueState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<PadConfig>>,
    /// Flag d'activation de la capture clavier globale.
    actif_flag: Arc<AtomicBool>,
    /// Compteur de génération du thread de capture. Chaque nouveau thread
    /// vérifie que sa génération est toujours la courante — sinon il sort.
    /// Évite la race condition OFF→ON rapide où l'ancien thread n'a pas eu
    /// le temps de sortir avant que le nouveau démarre.
    generation: Arc<AtomicU32>,
    /// Génération du chrono 2 : une nouvelle pression de touche chrono
    /// invalide la tâche tick précédente — un seul chrono à la fois,
    /// une nouvelle pression = reset à zéro.
    chrono_generation: Arc<AtomicU32>,
    /// Jeton d'annulation du timer courant (pad-stop).
    timer_cancel: Arc<Mutex<Arc<AtomicBool>>>,
}

impl PadNumeriqueState {
    /// Crée l'état + charge la config depuis le disque.
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let config = load_file(&app).unwrap_or_default();
        let state = Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(config)),
            actif_flag: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU32::new(0)),
            chrono_generation: Arc::new(AtomicU32::new(0)),
            timer_cancel: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
        };
        let c = state.config.lock().unwrap().clone();
        // Si la config était active au dernier arrêt, démarrer la capture.
        if c.actif {
            let _ = state.demarrer_capture();
        }
        state
    }

    /// Retourne la config complète (pour l'UI dashboard).
    pub fn etat(&self) -> PadConfig {
        self.config.lock().unwrap().clone()
    }

    /// Active/désactive le pad + démarre/arrête la capture clavier + persiste.
    pub fn set_actif(&self, actif: bool) -> Result<(), String> {
        {
            let mut c = self.config.lock().unwrap();
            c.actif = actif;
        }
        if actif {
            self.demarrer_capture()?;
        } else {
            self.arreter_capture();
        }
        self.save_to_disk()?;
        self.emit_etat();
        Ok(())
    }

    /// Remplace la config d'une touche + persiste + émet état.
    pub fn set_touche(&self, code: &str, plage: u32, cfg: ToucheConfig) -> Result<(), String> {
        if !CODES_ASSIGNABLES.contains(&code) {
            return Err(format!("Touche non assignable : {}", code));
        }
        if plage >= NB_PLAGES {
            return Err(format!("Plage invalide : {}", plage));
        }
        let cle = format!("{}_{}", code, plage);
        {
            let mut c = self.config.lock().unwrap();
            c.touches.insert(cle, cfg);
        }
        self.save_to_disk()?;
        self.emit_etat();
        Ok(())
    }

    /// Supprime (reset) la config d'une touche + persiste + émet état.
    pub fn supprimer_touche(&self, code: &str, plage: u32) -> Result<(), String> {
        let cle = format!("{}_{}", code, plage);
        {
            let mut c = self.config.lock().unwrap();
            c.touches.insert(cle, ToucheConfig::default());
        }
        self.save_to_disk()?;
        self.emit_etat();
        Ok(())
    }

    /// Change la plage courante (0-2) + persiste + émet état.
    pub fn set_plage(&self, plage: u32) -> Result<(), String> {
        if plage >= NB_PLAGES {
            return Err(format!("Plage invalide : {}", plage));
        }
        {
            let mut c = self.config.lock().unwrap();
            c.plage_actuelle = plage;
        }
        self.save_to_disk()?;
        self.emit_etat();
        Ok(())
    }

    /// Test manuel : déclenche une touche (bypass capture, pour le bouton Test).
    pub fn tester_touche(&self, code: &str, plage: u32) -> Result<(), String> {
        self.declencher_touche(code, plage);
        Ok(())
    }

    /// Test manuel du chrono 2 (bouton « Tester le rendu » de l'onglet
    /// Squelette de position) : démarre un chrono factice de 10 s,
    /// silencieux — les frames `chrono2-etat` empruntent le pipeline réel,
    /// position du squelette dédié relue à chaque frame incluse.
    pub fn tester_chrono(&self) {
        self.declencher_chrono(10, None, None, 1.0);
    }

    // ===== Déclenchement =====

    /// Logique commune (capture + test) : lit la config touche, émet WS + event.
    fn declencher_touche(&self, code: &str, plage: u32) {
        let cle = format!("{}_{}", code, plage);
        let touche = {
            let c = self.config.lock().unwrap();
            c.touches.get(&cle).cloned()
        };
        let Some(touche) = touche else { return };
        let Some(media_type) = &touche.media_type else { return };

        // Chrono 2 (media_type "chrono") : action INDÉPENDANTE des médias —
        // pas de carte pad, pas d'annulation du timer média courant (une
        // image/vidéo peut jouer pendant que le chrono tourne, et une
        // pression média ne coupe pas le chrono — pas de pad-stop croisé).
        if media_type == "chrono" {
            self.declencher_chrono(
                touche.chrono_duree_s,
                touche.chrono_son_depart.clone(),
                touche.chrono_son_fin.clone(),
                touche.volume,
            );
            return;
        }

        // Annuler le timer courant (si une touche était en cours).
        {
            let cancel = self.timer_cancel.lock().unwrap().clone();
            cancel.store(true, Ordering::SeqCst);
        }

        // Émettre event Tauri vers le dashboard (audio local + feedback UI).
        let touche_evt = serde_json::json!({
            "code": code,
            "plage": plage,
            "media_type": media_type,
            "media": touche.media,
            "media_kind": touche.media_kind,
            "son": touche.son,
            "volume": touche.volume,
            "duree_ms": touche.duree_ms,
        });
        let _ = self.app.emit("pad:touche", &touche_evt);

        // Émettre pad-play vers la diffusion (image/video uniquement — l'audio
        // pur n'a pas d'affichage, juste le son joué côté dashboard + diffusion).
        if media_type == "image" || media_type == "video" {
            let msg = serde_json::json!({
                "type": "pad-play",
                "pad": {
                    "media_type": media_type,
                    "media": touche.media,
                    "media_kind": touche.media_kind,
                    "son": touche.son,
                    "volume": touche.volume,
                    "duree_ms": touche.duree_ms,
                    "bounce": touche.bounce,
                    "zoom": touche.zoom,
                    "plein_ecran": touche.plein_ecran,
                    "transparence": touche.transparence,
                    "transparence_opacite": touche.transparence_opacite,
                },
            });
            let _ = self.chat_tx.send(msg.to_string());
        } else if media_type == "audio" {
            // Audio pur : émettre pad-play avec media_type="audio" pour que la
            // diffusion joue le son (OBS capte). Pas d'image affichée.
            let msg = serde_json::json!({
                "type": "pad-play",
                "pad": {
                    "media_type": "audio",
                    "media": touche.media,
                    "media_kind": None::<String>,
                    "son": touche.media,
                    "volume": touche.volume,
                    "duree_ms": touche.duree_ms,
                    "bounce": false,
                    "zoom": false,
                    "plein_ecran": false,
                    "transparence": false,
                    "transparence_opacite": 1.0,
                },
            });
            let _ = self.chat_tx.send(msg.to_string());
        }

        // Timer → pad-stop + event Tauri pad:touche-stop (dashboard stop audio).
        let state = self.clone();
        let new_cancel = Arc::new(AtomicBool::new(false));
        *self.timer_cancel.lock().unwrap() = new_cancel.clone();
        let duree_ms = touche.duree_ms.max(500);
        let code_owned = code.to_string();
        let plage_owned = plage;

        tauri::async_runtime::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(duree_ms)) => {
                    if !new_cancel.load(Ordering::SeqCst) {
                        let stop = serde_json::json!({ "type": "pad-stop" });
                        let _ = state.chat_tx.send(stop.to_string());
                        let _ = state.app.emit("pad:touche-stop", &serde_json::json!({
                            "code": code_owned, "plage": plage_owned,
                        }));
                    }
                }
                _ = await_timer_cancel(&new_cancel) => {}
            }
        });
    }

    // ===== Chrono 2 =====

    /// Démarre le chrono 2 : tick 10 Hz (tâche tokio), frames WS
    /// `chrono2-etat` auto-réparatrices — état COMPLET (actif, fin, écoulé,
    /// durée, sons, volume) + position du squelette dédié à CHAQUE frame :
    /// un client WS reconnecté se réaligne à la frame suivante, un
    /// déplacement du squelette pendant que le chrono tourne est suivi live.
    /// Event Tauri miroir `chrono2:etat` (pillule dashboard + sons locaux,
    /// double face comme les sons pad).
    ///
    /// Arrêt automatique à `duree_s` : dernière frame `actif:false, fin:true`
    /// → la diffusion fade-out + joue `son_fin`, le dashboard joue `son_fin`.
    ///
    /// Génération : une nouvelle pression de touche chrono invalide la tâche
    /// précédente (un seul chrono à la fois, reset à zéro).
    fn declencher_chrono(
        &self,
        duree_s: u64,
        son_depart: Option<String>,
        son_fin: Option<String>,
        volume: f32,
    ) {
        let ma_generation = self.chrono_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            let start = std::time::Instant::now();
            let duree_ms = (duree_s.max(1)).saturating_mul(1000) as u128;
            // u128::MAX force l'émission immédiate de la 1re frame (dixième 0
            // ≠ MAX) — le chrono apparaît sans attendre le 1er tick.
            let mut dernier_dixieme: u128 = u128::MAX;
            loop {
                if state.chrono_generation.load(Ordering::Relaxed) != ma_generation {
                    return;
                }
                let mut ecoule_ms = start.elapsed().as_millis();
                let fin = ecoule_ms >= duree_ms;
                if fin {
                    ecoule_ms = duree_ms;
                }
                // Throttle WS : une frame par changement de dixième (10 Hz max),
                // pattern du splitter (skip si signature identique).
                let dixieme = ecoule_ms / 100;
                if dixieme != dernier_dixieme || fin {
                    dernier_dixieme = dixieme;
                    // Position relue à chaque frame (une seule lecture lock →
                    // clone — règle Mutex non réentrant).
                    let position =
                        crate::position_overlay::PositionOverlayState::from_app(&state.app)
                            .map(|s| s.etat_chrono())
                            .unwrap_or_else(crate::position_overlay::defaut_chrono);
                    let frame = serde_json::json!({
                        "type": "chrono2-etat",
                        "actif": !fin,
                        "fin": fin,
                        "run_id": ma_generation,
                        "ecoule_ms": ecoule_ms,
                        "duree_s": duree_s,
                        "son_depart": &son_depart,
                        "son_fin": &son_fin,
                        "volume": volume,
                        "config": {
                            "x": position.x,
                            "y": position.y,
                            "largeur": position.largeur,
                            "hauteur": position.hauteur,
                        },
                    });
                    let _ = state.chat_tx.send(frame.to_string());
                    let evt = serde_json::json!({
                        "actif": !fin,
                        "fin": fin,
                        "run_id": ma_generation,
                        "ecoule_ms": ecoule_ms,
                        "duree_s": duree_s,
                        "son_depart": &son_depart,
                        "son_fin": &son_fin,
                        "volume": volume,
                    });
                    let _ = state.app.emit("chrono2:etat", &evt);
                    if fin {
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
    }

    // ===== Capture clavier globale =====

    /// Démarre le thread de capture clavier (poll GetAsyncKeyState 60Hz).
    /// Utilise un compteur de génération : chaque nouveau thread vérifie que
    /// sa génération est toujours la courante. Évite la race condition où
    /// l'ancien thread n'a pas eu le temps de sortir avant qu'un nouveau
    /// démarre (OFF→ON rapide) — l'ancien thread voit sa génération obsolète
    /// et sort immédiatement.
    fn demarrer_capture(&self) -> Result<(), String> {
        // Nouvelle génération → invalide l'ancien thread s'il tourne encore.
        let ma_generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.actif_flag.store(true, Ordering::Relaxed);

        let state = self.clone();
        thread::spawn(move || {
            let mut etat_precedent = [false; 256];
            let mut numlock_precedent: Option<bool> = None;
            while state.actif_flag.load(Ordering::Relaxed)
                && state.generation.load(Ordering::Relaxed) == ma_generation
            {
                #[cfg(windows)]
                {
                    let app = state.app.clone();
                    poll_numpad_une_fois(&mut etat_precedent, &app, &state);
                    // Détection NumLock (vk=0x90) — émet pad:erreur quand OFF.
                    // GetAsyncKeyState reflete l'état toggled via le bit 0x0001.
                    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
                    let numlock_toggled =
                        unsafe { GetAsyncKeyState(0x90) as u16 & 0x0001 != 0 };
                    if Some(numlock_toggled) != numlock_precedent {
                        numlock_precedent = Some(numlock_toggled);
                        if numlock_toggled {
                            let _ = state.app.emit("pad:erreur", serde_json::json!(null));
                        } else {
                            let _ = state.app.emit(
                                "pad:erreur",
                                serde_json::json!(
                                    "NumLock est OFF — les touches Numpad envoient Home/End/flèches. Active NumLock pour utiliser le pad."
                                ),
                            );
                        }
                    }
                }
                thread::sleep(Duration::from_millis(PERIODE_POLL_MS));
            }
        });
        Ok(())
    }

    /// Arrête le thread de capture clavier.
    fn arreter_capture(&self) {
        if !self.actif_flag.load(Ordering::Relaxed) {
            return;
        }
        self.actif_flag.store(false, Ordering::Relaxed);
    }

    // ===== Émission =====

    /// Émet l'état vers le dashboard (event Tauri pad:etat).
    fn emit_etat(&self) {
        let cfg = self.etat();
        let _ = self.app.emit("pad:etat", &cfg);
    }

    // ===== Persistance =====

    fn save_to_disk(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let path = dir.join("pad_numerique.json");
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize pad_numerique.json: {}", e))?;
        fs::write(&path, json).map_err(|e| format!("Écrire pad_numerique.json: {}", e))?;
        Ok(())
    }
}

// =============================================================================
// CAPTEUR CLAVIER — poll GetAsyncKeyState (Windows)
// =============================================================================

/// vkCodes des touches Numpad à surveiller (0x60-0x69, 0x6A-0x6F).
const NUMPAD_VK_CODES: &[(u32, &str)] = &[
    (0x60, "Numpad0"),
    (0x61, "Numpad1"),
    (0x62, "Numpad2"),
    (0x63, "Numpad3"),
    (0x64, "Numpad4"),
    (0x65, "Numpad5"),
    (0x66, "Numpad6"),
    (0x67, "Numpad7"),
    (0x68, "Numpad8"),
    (0x69, "Numpad9"),
    (0x6A, "NumpadMultiply"),
    (0x6B, "NumpadAdd"),
    (0x6D, "NumpadSubtract"),
    (0x6E, "NumpadDecimal"),
    (0x6F, "NumpadDivide"),
    // NumpadEnter = 0x0D (même vkCode que Enter principal — on l'ignore pour
    // éviter les faux positifs quand le user tape Enter sur le clavier principal).
];

/// Un passage de poll : détecte les fronts down des touches Numpad.
#[cfg(windows)]
fn poll_numpad_une_fois(
    etat_precedent: &mut [bool; 256],
    app: &AppHandle,
    state: &PadNumeriqueState,
) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    // Lire la plage courante UNE fois (évite les locks multiples).
    let plage = {
        let c = state.config.lock().unwrap();
        c.plage_actuelle
    };

    for &(vk, code) in NUMPAD_VK_CODES {
        let presse = unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 };
        let idx = vk as usize;
        if etat_precedent[idx] != presse {
            etat_precedent[idx] = presse;
            if presse {
                // Front down détecté.
                if code == "NumpadAdd" {
                    // Plage suivante (max 2).
                    let _ = state.set_plage((plage + 1).min(NB_PLAGES - 1));
                    let _ = app.emit(
                        "pad:plage-changee",
                        &serde_json::json!({ "plage": (plage + 1).min(NB_PLAGES - 1) }),
                    );
                } else if code == "NumpadSubtract" {
                    // Plage précédente (min 0).
                    let _ = state.set_plage(plage.saturating_sub(1));
                    let _ = app.emit(
                        "pad:plage-changee",
                        &serde_json::json!({ "plage": plage.saturating_sub(1) }),
                    );
                } else {
                    // Touche assignable → déclencher le média.
                    state.declencher_touche(code, plage);
                }
            }
        }
    }
}

// =============================================================================
// PERSISTANCE
// =============================================================================

/// Charge pad_numerique.json (absent → défaut).
fn load_file(app: &AppHandle) -> Result<PadConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("pad_numerique.json");
    if !path.exists() {
        return Ok(PadConfig::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire pad_numerique.json: {}", e))?;
    let cfg: PadConfig =
        serde_json::from_str(&content).map_err(|e| format!("Parse pad_numerique.json: {}", e))?;
    Ok(cfg)
}

/// Attend que le jeton d'annulation passe à true (polling 100ms).
async fn await_timer_cancel(cancel: &Arc<AtomicBool>) {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
