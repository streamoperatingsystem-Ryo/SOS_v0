// =============================================================================
// Moteur ASL — Boucle de polling et logique du splitter
// (port de ASLEngine.cs + ASLScript.cs)
// -----------------------------------------------------------------------------
// Le moteur tourne dans un thread dédié qui possède :
//   - Le ScriptContext (Boa JS engine — NON Send, doit vivre dans un thread)
//   - Le TimerModel (état du timer)
//   - Le processus jeu (handle Windows)
//
// La communication avec les commandes Tauri se fait par channels (mpsc) :
//   - Commands → Engine thread (load_asl, start, stop, update_settings)
//   - Engine thread → Frontend (events via callback)
// =============================================================================

use std::sync::mpsc;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use crate::speedrun::asl::{self, AslScript};
use crate::speedrun::engine::pont_memoire;
use crate::speedrun::engine::script_bridge::{AslSettings, ScriptContext, SettingDetail};
use crate::speedrun::engine::timer::{self, TimerModel, TimerPhase};
use crate::speedrun::engine::transpiler;
use crate::speedrun::memory::deep_pointer::DeepPointer;
use crate::speedrun::memory::memory_watcher::{MemoryWatcher, MemoryWatcherList, WatcherType, WatchedValue};
use crate::speedrun::memory::process::Process;

/// Event émis par le moteur vers le frontend.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum SpeedrunEvent {
    /// index = split de départ (index armé par clic en NotRunning, sinon 0).
    #[serde(rename = "started")]
    Started { index: i32 },
    /// time = durée du segment bouclé, cumul = temps total de run (champ
    /// séparé), index = current_split_index après incrément (split réalisé =
    /// index − 1).
    #[serde(rename = "split")]
    Split { time: String, cumul: String, index: i32 },
    #[serde(rename = "reset")]
    Reset,
    /// Même contrat que Split : time = durée du dernier segment,
    /// cumul = temps final, index = total_splits.
    #[serde(rename = "ended")]
    Ended { time: String, cumul: String, index: i32 },
    /// Saut vers un index (clic sur une ligne) : NotRunning = armement,
    /// Running/Paused/Ended = retry de segment. phase = phase résultante.
    #[serde(rename = "goto")]
    Goto { index: i32, phase: String },
    /// État consolidé pour resync (reconnexion WS de la diffusion) : phase,
    /// index (armé inclus), durées de segment enregistrées, chronos.
    /// segmentTime null en NotRunning (ligne armée → 0:00.00 côté frontend).
    #[serde(rename = "etat")]
    Etat {
        phase: String,
        index: i32,
        #[serde(rename = "splitTimes")]
        split_times: Vec<Option<String>>,
        #[serde(rename = "realTime")]
        real_time: String,
        #[serde(rename = "gameTime")]
        game_time: Option<String>,
        #[serde(rename = "segmentTime")]
        segment_time: Option<String>,
    },
    #[serde(rename = "gameTime")]
    GameTime { time: String },
    /// Temps régulier émis chaque tick quand Running/Paused (RTA + game time).
    /// gameTime est Option (None si pas de méthode gameTime ou retourne null).
    /// Le frontend utilise realTime comme fallback pour tempsJeu si gameTime est null.
    /// Sans cet event, le chrono reste à 0 après start manuel quand gameTime ASL
    /// retourne null (ex: M["TimerActive"].Current null → gameTime retourne null).
    /// segmentTime = temps vivant du segment courant (primary − seg_start).
    #[serde(rename = "time")]
    Time {
        #[serde(rename = "realTime")]
        real_time: String,
        #[serde(rename = "gameTime")]
        game_time: Option<String>,
        #[serde(rename = "segmentTime")]
        segment_time: Option<String>,
    },
    #[serde(rename = "connected")]
    Connected { process: String, pid: u32 },
    #[serde(rename = "disconnected")]
    Disconnected,
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "settings-list")]
    SettingsList { settings: Vec<SettingInfo> },
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "loaded")]
    Loaded,
    // --- Lot 2 : contrôles manuels ---
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "resumed")]
    Resumed,
    #[serde(rename = "splitSkipped")]
    SplitSkipped { index: i32 },
    #[serde(rename = "splitUndone")]
    SplitUndone { index: i32 },
}

/// Action manuelle du timer (déclenchée depuis le widget, pas par l'ASL).
/// Lot 2 — contrôles manuels exposed sur WidgetSpeedrun uniquement.
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionManuelle {
    Start,
    Split,
    Skip,
    Undo,
    Reset,
    Pause,
    /// Clic sur une ligne de split : arme l'index (NotRunning) ou saute à
    /// l'index en retry de segment (Running/Paused/Ended).
    GoTo { index: i32 },
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SettingInfo {
    pub id: String,
    pub label: String,
    pub value: bool,
    pub parent: Option<String>,
}

/// Résultat du chargement d'un ASL : settings de base (start/split/reset) +
/// settings individuels (120+ pour MGS) + dictionnaire code→nom (D.Names.Split).
/// Le frontend utilise les settings individuels + noms pour afficher la modale
/// de configuration auto et mapper les segments LSS vers les codes-signature.
#[derive(Clone, Debug, serde::Serialize)]
pub struct AslLoadResult {
    /// Settings de base (3 toggles globaux).
    pub settings: Vec<SettingInfo>,
    /// Settings individuels (ex: "OL-s00a" → "Dock"). Vide pour les ASL simples.
    pub settings_detailles: Vec<SettingDetail>,
    /// Dictionnaire code→nom lisible (D.Names.Split). Vide pour les ASL simples.
    /// Vec de [code, nom] (sérialisé en tableau de paires pour le frontend).
    pub noms_splits: Vec<(String, String)>,
}

/// Commandes envoyées au thread du moteur.
enum EngineCommand {
    LoadAsl {
        path: String,
        respond: mpsc::Sender<Result<AslLoadResult, String>>,
    },
    Start {
        total_splits: i32,
        settings: AslSettings,
    },
    Stop,
    UpdateSettings {
        settings: AslSettings,
    },
    ActionManuelle {
        action: ActionManuelle,
    },
    /// Met à jour les settings ASL individuels (120+ pour MGS) dans le Context Boa.
    MajSettingsAsl {
        valeurs: std::collections::HashMap<String, bool>,
    },
    /// Émet l'état consolidé (event `etat`) — resync diffusion à la
    /// reconnexion WS (reload OBS en cours de run).
    EmitEtat,
    Shutdown,
}

/// Le moteur ASL — interface thread-safe vers le thread de polling.
pub struct AslEngine {
    sender: mpsc::Sender<EngineCommand>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AslEngine {
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(SpeedrunEvent) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<EngineCommand>();

        let handle = thread::spawn(move || {
            // Visible dans la modale Diagnostic (CPU % d'un cœur — cf.
            // crash thermique 2026-09-21 : c'est CE thread qui grillait le PC
            // à 66Hz). Registre TID→label, aucun coût.
            crate::conso::nommer_thread("Splitter (moteur ASL)");
            engine_thread_main(rx, callback);
        });

        Self {
            sender: tx,
            thread: Some(handle),
        }
    }

    /// Charge un fichier ASL (bloquant — attend la réponse du thread).
    pub fn load_asl(&self, path: &str) -> Result<AslLoadResult, String> {
        let (resp_tx, resp_rx) = mpsc::channel();
        self.sender
            .send(EngineCommand::LoadAsl {
                path: path.to_string(),
                respond: resp_tx,
            })
            .map_err(|e| format!("Channel: {}", e))?;
        resp_rx
            .recv()
            .map_err(|e| format!("Channel recv: {}", e))?
    }

    /// Démarre la boucle de polling.
    pub fn start(&self, total_splits: i32, settings: AslSettings) -> Result<(), String> {
        self.sender
            .send(EngineCommand::Start {
                total_splits,
                settings,
            })
            .map_err(|e| format!("Channel: {}", e))
    }

    /// Arrête la boucle de polling.
    pub fn stop(&self) -> Result<(), String> {
        self.sender
            .send(EngineCommand::Stop)
            .map_err(|e| format!("Channel: {}", e))
    }

    /// Met à jour les settings.
    pub fn update_settings(&self, settings: AslSettings) -> Result<(), String> {
        self.sender
            .send(EngineCommand::UpdateSettings { settings })
            .map_err(|e| format!("Channel: {}", e))
    }

    /// Met à jour les settings ASL individuels (120+ pour MGS) dans le Context
    /// Boa. Appelé quand l'utilisateur valide la modale de configuration.
    pub fn maj_settings_asl(
        &self,
        valeurs: std::collections::HashMap<String, bool>,
    ) -> Result<(), String> {
        self.sender
            .send(EngineCommand::MajSettingsAsl { valeurs })
            .map_err(|e| format!("Channel: {}", e))
    }

    /// Envoie une action manuelle au thread moteur (Lot 2 — contrôles manuels).
    /// L'action est exécutée dans le thread moteur pour éviter tout accès
    /// concurrent au TimerModel. Retourne Ok même si l'action n'a aucun effet
    /// (ex: split en phase NotRunning — le timer ignore silencieusement).
    pub fn action_manuelle(&self, action: ActionManuelle) -> Result<(), String> {
        self.sender
            .send(EngineCommand::ActionManuelle { action })
            .map_err(|e| format!("Channel: {}", e))
    }

    /// Demande au thread moteur d'émettre l'état consolidé (event `etat`).
    /// Appelé par reemit_lss quand la diffusion demande un resync WS.
    pub fn emit_etat(&self) -> Result<(), String> {
        self.sender
            .send(EngineCommand::EmitEtat)
            .map_err(|e| format!("Channel: {}", e))
    }
}

impl Drop for AslEngine {
    fn drop(&mut self) {
        let _ = self.sender.send(EngineCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// État interne du thread moteur.
struct EngineInner {
    script: Option<AslScript>,
    timer: TimerModel,
    settings: AslSettings,
    /// Processus jeu attaché, partagé via Rc avec le pont mémoire (lectures JS
    /// via __rust_deref). Rc car tout vit dans le thread de polling du moteur
    /// (pas de Send — le handle Win32 n'est pas Send). Le handle est fermé une
    /// seule fois quand le dernier Rc est libéré.
    process: Option<Rc<Process>>,
    /// Watchers mémoire construits depuis les state() du script ASL.
    /// Permet d'injecter current/old dans les méthodes ASL (start, split, etc.).
    watchers: MemoryWatcherList,
    refresh_rate: f64,
    init_completed: bool,
    running: bool,
    script_ctx: ScriptContext,
    /// Durées de segment enregistrées pour la tentative en cours
    /// (index = segment 0-based). Alimenté à chaque split ; effacé par
    /// reset/goto (retry de segment : les précédents ne comptent pas comme
    /// finis) ; lu par undo_split (reprend le segment défait à d_k).
    temps_segs: Vec<Option<f64>>,
    /// Compteur de ticks d'attente d'attach. Double usage : throttle du log
    /// "en attente" ET throttle du scan de processus (1 scan/s — le scan est
    /// un CreateToolhelp32Snapshot complet, coûteux en kernel, et il y a un
    /// snapshot PAR nom de state() : le script MGS en a 9). Remis à 0 à chaque
    /// attach réussi. Période ~15 ticks (~1s à 15Hz).
    ticks_attente_attach: u32,
}

impl EngineInner {
    fn new() -> Self {
        Self {
            script: None,
            timer: TimerModel::new(),
            settings: AslSettings::default(),
            process: None,
            watchers: MemoryWatcherList::new(),
            // ⚠️ En HERTZ (le thread moteur dort 1000/refresh_rate ms).
            // Bug 2026-09-21 (crash thermique PC) : la valeur était 1000.0/15.0
            // (= 66,67 : une période en ms) lue comme une fréquence → sleep
            // 1000/66,67 = 15ms → ~66 Hz réels au lieu de 15 (4,4× trop rapide
            // : evals Boa + events Time + maj DOM bombardées à 66Hz, un cœur
            // CPU à 100%). Ne JAMAIS stocker une période ici.
            refresh_rate: 15.0,
            init_completed: false,
            running: false,
            script_ctx: ScriptContext::new(),
            temps_segs: Vec::new(),
            ticks_attente_attach: 0,
        }
    }

    fn handle_load_asl(&mut self, path: &str) -> Result<AslLoadResult, String> {
        let source =
            std::fs::read_to_string(path).map_err(|e| format!("Lecture fichier: {}", e))?;
        let script = asl::parse(&source).map_err(|e| format!("Parse ASL: {}", e))?;

        // LOT 1 — splits bloqués : reset de l'état script persistant (vars +
        // settings) avant de compiler le nouveau script. Sans ça, un script
        // précédent laisse ses vars/settings dans globalThis → le nouveau
        // script hérite d'état stale (ex: settings d'un autre jeu).
        self.script_ctx.reset_etat_script();

        // Transpiler et compiler chaque méthode
        let method_names = [
            "startup",
            "shutdown",
            "init",
            "exit",
            "update",
            "start",
            "split",
            "reset",
            "isLoading",
            "gameTime",
            "onStart",
            "onSplit",
            "onReset",
        ];

        for &name in &method_names {
            if let Some(code) = script.methods.get(name) {
                let js_code = transpiler::transpile_cs_to_js(code);
                log::debug!(
                    "[ASL] Compilation '{}' ({}→{} chars)",
                    name,
                    code.len(),
                    js_code.len()
                );
                if let Err(e) = self.script_ctx.compile_method(name, &js_code) {
                    log::warn!("[ASL] Méthode '{}' non compilée: {}", name, e);
                }
            }
        }

        // Settings
        self.settings = AslSettings {
            start: script.methods.start.is_some(),
            split: script.methods.split.is_some(),
            reset: script.methods.reset.is_some(),
        };

        // Exécuter startup
        self.script = Some(script);
        if self.script_ctx.has_method("startup") {
            if let Err(e) = self.script_ctx.run_startup(&self.settings) {
                log::warn!("[ASL] Error in startup: {}", e);
            } else {
                log::debug!("[ASL] Startup exécuté");
            }
        }

        // Lire les settings individuels (120+ pour MGS) et le dictionnaire
        // code→nom (D.Names.Split) depuis le Context Boa après startup. Ces
        // données sont utilisées par la modale de configuration auto pour
        // afficher l'arbre des splits et mapper les segments LSS.
        let settings_detailles = self.script_ctx.lire_settings_asl();
        let noms_splits = self.script_ctx.lire_noms_splits();
        log::info!(
            "[ASL] {} setting(s) individuel(s), {} nom(s) de split",
            settings_detailles.len(),
            noms_splits.len()
        );

        let settings_list = vec![
            SettingInfo {
                id: "start".to_string(),
                label: "Start the timer".to_string(),
                value: self.settings.start,
                parent: None,
            },
            SettingInfo {
                id: "split".to_string(),
                label: "Split the timer".to_string(),
                value: self.settings.split,
                parent: None,
            },
            SettingInfo {
                id: "reset".to_string(),
                label: "Reset the timer".to_string(),
                value: self.settings.reset,
                parent: None,
            },
        ];

        Ok(AslLoadResult {
            settings: settings_list,
            settings_detailles,
            noms_splits,
        })
    }

    fn handle_start(&mut self, total_splits: i32, settings: AslSettings) {
        self.settings = settings;
        self.timer = TimerModel::new();
        self.timer.state.total_splits = total_splits;
        self.temps_segs = vec![None; total_splits.max(0) as usize];
        self.init_completed = false;
        self.process = None;
        self.running = true;
        // Compteur de scan à 0 → le premier tick scanne immédiatement (si le
        // jeu tourne déjà, l'attach est instantané au démarrage du splitter).
        self.ticks_attente_attach = 0;
        log::info!("[ASL] Moteur démarré ({} splits)", total_splits);
    }

    fn handle_stop(&mut self) {
        self.running = false;
        log::info!("[ASL] Moteur arrêté");
    }

    /// Une itération de polling. Retourne un event si quelque chose s'est passé.
    /// `callback` permet d'émettre des events supplémentaires dans le même
    /// tick (ex: Reset avant Disconnected quand le processus meurt).
    fn poll_tick<F>(&mut self, callback: &F) -> Option<SpeedrunEvent>
    where
        F: Fn(SpeedrunEvent),
    {
        if !self.running || self.script.is_none() {
            return None;
        }

        let script = self.script.as_ref().unwrap().clone();

        // 1. Connexion au processus
        if self.process.is_none() {
            // Throttle du scan (fix 2026-09-21, crash thermique PC) :
            // find_by_name fait un CreateToolhelp32Snapshot COMPLET (énumère
            // tous les processus du système) et on en fait un PAR nom de
            // state() — le script MGS en a 9 → 9 snapshots système par tick.
            // À 15Hz ça restait 135 snapshots/s pour rien ; 1 scan/s suffit
            // largement (le jeu met toujours >1s à démarrer après activation
            // du splitter). ticks_attente_attach démarre à 0 → le PREMIER
            // tick scanne immédiatement (pas de latence d'attach à l'activation
            // ni au relancement du jeu après fermeture).
            let scanner_ce_tick = self.ticks_attente_attach % 15 == 0;
            self.ticks_attente_attach = self.ticks_attente_attach.wrapping_add(1);
            if scanner_ce_tick {
                let process_names: Vec<String> = script.states.keys().cloned().collect();
                for name in &process_names {
                    if let Some(proc) = Process::find_by_name(name) {
                        log::info!(
                            "[ASL] Connecté au processus '{}' (PID {})",
                            proc.name,
                            proc.id
                        );
                        // Partager le Process via Rc entre EngineInner (is_alive,
                        // serialize_process, watchers.update_all) et le pont mémoire
                        // (lectures JS via __rust_deref). E8 Bug A : avant ce fix,
                        // le Process était move-d dans le pont → self.process restait
                        // None → init/update/start n'étaient jamais appelés.
                        let proc = Rc::new(proc);
                        let pid = proc.id;
                        let proc_name = proc.name.clone();
                        self.process = Some(Rc::clone(&proc));
                        // Construire les watchers mémoire depuis les state() du script
                        // pour le processus connecté (première state def correspondante).
                        self.construire_watchers(name, &script);
                        // Connecter le processus au pont mémoire (thread_local) pour
                        // que les callbacks natifs __rust_deref puissent lire la mémoire.
                        pont_memoire::connecter_lecteur(proc as Rc<dyn crate::speedrun::memory::LecteurMemoire>);
                        self.init_completed = false;
                        self.script_ctx.set_pid(Some(pid));
                        self.ticks_attente_attach = 0;
                        return Some(SpeedrunEvent::Connected {
                            process: proc_name,
                            pid,
                        });
                    }
                }
                // E8 Bug A — log throttle : aucun process trouvé ce scan. On
                // log à chaque scan (~1s) pour ne pas spammer. Avant ce fix,
                // le matching échouait silencieusement car find_by_name
                // comparait "mgsi.exe" (Windows) == "mgsi" (ASL) → toujours false.
                log::debug!(
                    "[ASL] En attente du processus (scan: {})",
                    process_names.join(", ")
                );
            }
            return None;
        }

        // 2. Vérifier que le processus est vivant
        if !self.process.as_ref().unwrap().is_alive() {
            log::info!("[ASL] Processus jeu fermé");
            // Sortie du jeu : le timer reset → émettre Reset pour que
            // l'affichage (store + diffusion) suive le timer Rust. Couvre
            // aussi l'armement d'un split (armed_index effacé par reset).
            if self.timer.state.current_phase != TimerPhase::NotRunning
                || self.timer.state.armed_index.is_some()
            {
                self.timer.reset();
                callback(SpeedrunEvent::Reset);
            }
            self.process = None;
            self.init_completed = false;
            self.script_ctx.set_pid(None);
            pont_memoire::deconnecter_lecteur();
            self.watchers = MemoryWatcherList::new();
            self.ticks_attente_attach = 0;
            return Some(SpeedrunEvent::Disconnected);
        }

        // 3. Préparer les données pour le script
        let proc = self.process.as_ref().unwrap();
        let proc_json = serialize_process(proc);
        let timer_json = serialize_timer(&self.timer.state);

        // Mettre à jour les watchers mémoire (lecture des variables state())
        // et sérialiser current/old pour les passer aux méthodes ASL.
        // Règle §2bis : la lecture ne se fait QUE quand le moteur tourne (ici).
        // proc est &Rc<Process> ; &**proc déréférence en &Process qui coerce
        // en &dyn LecteurMemoire (Process impl LecteurMemoire).
        let _changed = self.watchers.update_all(&**proc);
        let current_json = serialize_watchers_current(&self.watchers);
        let old_json = serialize_watchers_old(&self.watchers);

        // 4. Init si pas encore fait
        if !self.init_completed {
            if self.script_ctx.has_method("init") {
                match self.script_ctx.call_method(
                    "init",
                    &timer_json,
                    &old_json,
                    &current_json,
                    Some(&proc_json),
                    &self.settings,
                ) {
                    Ok(_) => {}
                    Err(e) => log::warn!("[ASL] Error in init (Rust): {}", e),
                }
            }
            // E8 Bug A — détecter si init a échoué côté JS (le try/catch du
            // wrapper attrape l'erreur, imprime un log, retourne null —
            // call_method retourne Ok("null"), impossible à distinguer d'un
            // succès sans le flag globalThis.__asl_last_error).
            // Si init a eu une erreur JS, on NE set PAS init_completed = true
            // → init sera re-tenté au prochain tick. On log throttle pour ne
            // pas spammer (init peut échouer durablement si le script utilise
            // une feature C# non transpilée).
            if let Some(err) = self.script_ctx.derniere_erreur() {
                self.ticks_attente_attach = self.ticks_attente_attach.wrapping_add(1);
                if self.ticks_attente_attach % 15 == 0 {
                    log::warn!("[ASL] Init échoué (retry): {}", err);
                }
                return None; // Skip step 5 — gameTime/split/reset sur vars.D.Mem vide → throw
            }
            self.init_completed = true;
            self.ticks_attente_attach = 0;
            log::debug!("[ASL] Init terminé");
        } else {
            // Update
            if self.script_ctx.has_method("update") {
                match self.script_ctx.call_method(
                    "update",
                    &timer_json,
                    &old_json,
                    &current_json,
                    Some(&proc_json),
                    &self.settings,
                ) {
                    Ok(_) => {}
                    Err(e) => log::warn!("[ASL] Error in update: {}", e),
                }
            }
        }

        // 5. Logique start/split/reset
        // E8 Bug A — guard : si init n'est pas terminé, ne pas appeler les
        // méthodes start/split/reset/gameTime/isLoading. Ces méthodes accèdent
        // à vars.D.Mem (MemoryWatcherList JS) qui n'est peuplée que par init.
        // Sans ce guard, M["Location"] retourne undefined → undefined.Changed
        // → "cannot convert 'null' or 'undefined' to object" (Boa).
        if !self.init_completed {
            return None;
        }

        let phase = self.timer.state.current_phase;

        // Start
        if phase == TimerPhase::NotRunning && self.script_ctx.has_method("start") {
            match self.script_ctx.call_method(
                "start",
                &timer_json,
                &old_json,
                &current_json,
                Some(&proc_json),
                &self.settings,
            ) {
                Ok(result) => {
                    if result == "true" && self.settings.start {
                        self.timer.start();
                        let index = self.timer.state.current_split_index;
                        log::info!("[ASL] Timer démarré (split #{})", index);
                        return Some(SpeedrunEvent::Started { index });
                    }
                    // E8 Bug A — log throttle si start() retourne false
                    // (diagnostic auto-start : settings.start=false ou script
                    // retourne false/null). Log throttled ~1/s pour ne pas spammer.
                    // result="null" = erreur JS (ex: M["Progress"].Current null → .Equals throw)
                    // result="false" = script a évalué les conditions → pas encore
                    self.ticks_attente_attach = self.ticks_attente_attach.wrapping_add(1);
                    if self.ticks_attente_attach % 15 == 0 {
                        log::debug!(
                            "[ASL] start()={} (settings.start={}) — en attente condition jeu",
                            result,
                            self.settings.start
                        );
                    }
                }
                Err(e) => log::warn!("[ASL] Error in start: {}", e),
            }
        }

        // isLoading + gameTime + split + reset
        if phase == TimerPhase::Running || phase == TimerPhase::Paused {
            // isLoading
            if self.script_ctx.has_method("isLoading") {
                match self.script_ctx.call_method(
                    "isLoading",
                    &timer_json,
                    &old_json,
                    &current_json,
                    Some(&proc_json),
                    &self.settings,
                ) {
                    Ok(result) => {
                        let is_paused = result == "true";
                        self.timer.state.set_is_game_time_paused(is_paused);
                    }
                    Err(e) => log::warn!("[ASL] Error in isLoading: {}", e),
                }
            }

            // gameTime
            if self.script_ctx.has_method("gameTime") {
                match self.script_ctx.call_method(
                    "gameTime",
                    &timer_json,
                    &old_json,
                    &current_json,
                    Some(&proc_json),
                    &self.settings,
                ) {
                    Ok(result) => {
                        if let Ok(gt) = result.parse::<f64>() {
                            self.timer.state.set_game_time(gt);
                        }
                    }
                    Err(e) => log::warn!("[ASL] Error in gameTime: {}", e),
                }
            }

            // reset
            let mut do_reset = false;
            if self.script_ctx.has_method("reset") {
                match self.script_ctx.call_method(
                    "reset",
                    &timer_json,
                    &old_json,
                    &current_json,
                    Some(&proc_json),
                    &self.settings,
                ) {
                    Ok(result) => do_reset = result == "true",
                    Err(e) => log::warn!("[ASL] Error in reset: {}", e),
                }
            }

            // split
            let mut do_split = false;
            if !do_reset && self.script_ctx.has_method("split") {
                match self.script_ctx.call_method(
                    "split",
                    &timer_json,
                    &old_json,
                    &current_json,
                    Some(&proc_json),
                    &self.settings,
                ) {
                    Ok(result) => do_split = result == "true",
                    Err(e) => log::warn!("[ASL] Error in split: {}", e),
                }
            }

            if do_reset && self.settings.reset {
                self.timer.reset();
                self.temps_segs.iter_mut().for_each(|t| *t = None);
                return Some(SpeedrunEvent::Reset);
            } else if do_split && self.settings.split {
                if let Some(event) = self.faire_split() {
                    return Some(event);
                }
            }
        }

        // Émettre le temps régulièrement (RTA + game time) chaque tick quand
        // Running/Paused. Sans cet event, le chrono du widget reste à 0 :
        // l'ancien code n'émettait GameTime que si ct.game_time.is_some(),
        // or le script MGS gameTime retourne null quand M["TimerActive"].Current
        // est null (mémoire pas encore lue) → aucun event → tempsJeu figé.
        // L'event Time porte toujours realTime (RTA) + gameTime (Option).
        // Le frontend affiche tempsJeu = gameTime || realTime (fallback RTA).
        if self.timer.state.current_phase == TimerPhase::Running
            || self.timer.state.current_phase == TimerPhase::Paused
        {
            let ct = self.timer.state.current_time();
            return Some(SpeedrunEvent::Time {
                real_time: ct
                    .real_time
                    .map(|t| timer::format_time(t))
                    .unwrap_or_else(|| "00:00.00".to_string()),
                game_time: ct.game_time.map(|gt| timer::format_time(gt)),
                segment_time: self
                    .timer
                    .state
                    .segment_elapsed()
                    .map(|s| timer::format_time(s)),
            });
        }

        None
    }

    /// Split (auto ASL ou manuel) : enregistre la durée du segment bouclé
    /// dans temps_segs et retourne l'event Split (ou Ended sur le dernier).
    /// Contrat : `time` = durée du segment, `cumul` = temps total de run.
    /// None si le timer a refusé le split (phase ou temps nul).
    fn faire_split(&mut self) -> Option<SpeedrunEvent> {
        let seg_dur = self.timer.split()?;
        let index = self.timer.state.current_split_index;
        let seg_idx = (index - 1).max(0) as usize;
        if self.temps_segs.len() <= seg_idx {
            self.temps_segs.resize(seg_idx + 1, None);
        }
        self.temps_segs[seg_idx] = Some(seg_dur);
        let cumul = self.timer.state.current_time().format();
        let time = timer::format_time(seg_dur);
        if self.timer.state.current_phase == TimerPhase::Ended {
            return Some(SpeedrunEvent::Ended { time, cumul, index });
        }
        Some(SpeedrunEvent::Split { time, cumul, index })
    }

    /// État consolidé pour resync (event `etat`) : phase, index (armé
    /// inclus), durées de segment enregistrées, chronos. Émis à la demande
    /// (reconnexion WS de la diffusion — un reload OBS retrouve la run).
    fn etat_courant(&self) -> SpeedrunEvent {
        let st = &self.timer.state;
        let ct = st.current_time();
        SpeedrunEvent::Etat {
            phase: format!("{:?}", st.current_phase),
            index: st.current_split_index,
            split_times: self
                .temps_segs
                .iter()
                .map(|d| d.map(timer::format_time))
                .collect(),
            real_time: ct
                .real_time
                .map(timer::format_time)
                .unwrap_or_else(|| "00:00.00".to_string()),
            game_time: ct.game_time.map(timer::format_time),
            // NotRunning (armé ou non) → null : la ligne armée affiche
            // 0:00.00 côté frontend.
            segment_time: st.segment_elapsed().map(timer::format_time),
        }
    }

    /// Traite une action manuelle (depuis le widget) et retourne l'event
    /// éventuel à émettre vers le frontend. Le timer est manipulé dans le
    /// thread moteur (pas d'accès concurrent). Les phases invalides sont
    /// ignorées silencieusement (le timer fait déjà les gardes).
    fn handle_action_manuelle(&mut self, action: ActionManuelle) -> Option<SpeedrunEvent> {
        let phase_before = self.timer.state.current_phase;
        match action {
            ActionManuelle::Start => {
                if phase_before == TimerPhase::NotRunning {
                    self.timer.start();
                    let index = self.timer.state.current_split_index;
                    log::info!("[ASL] Timer démarré (manuel) — split #{}", index);
                    return Some(SpeedrunEvent::Started { index });
                }
                None
            }
            ActionManuelle::Split => {
                if phase_before == TimerPhase::Running {
                    if let Some(event) = self.faire_split() {
                        match &event {
                            SpeedrunEvent::Ended { cumul, .. } => {
                                log::info!("[ASL] Run terminée (manuel) — {}", cumul);
                            }
                            SpeedrunEvent::Split { index, time, .. } => {
                                log::info!("[ASL] Split (manuel) #{} — {}", index, time);
                            }
                            _ => {}
                        }
                        return Some(event);
                    }
                }
                None
            }
            ActionManuelle::Skip => {
                if phase_before == TimerPhase::Running || phase_before == TimerPhase::Paused {
                    let skipped = self.timer.state.current_split_index.max(0) as usize;
                    self.timer.skip_split();
                    // Le segment sauté ne compte pas : aucun temps enregistré.
                    if skipped < self.temps_segs.len() {
                        self.temps_segs[skipped] = None;
                    }
                    let index = self.timer.state.current_split_index;
                    log::info!("[ASL] Split skip (manuel) → #{}", index);
                    return Some(SpeedrunEvent::SplitSkipped { index });
                }
                None
            }
            ActionManuelle::Undo => {
                if phase_before != TimerPhase::NotRunning
                    && self.timer.state.current_split_index > 0
                {
                    // Durée du segment défait (lue dans temps_segs AVANT
                    // undo — seg_start recule de d_k, le chrono réaffiche
                    // d_k au lieu de repartir de zéro). L'entrée est ensuite
                    // effacée : le temps reprendra sa place au prochain split.
                    let undone = (self.timer.state.current_split_index - 1).max(0) as usize;
                    let duree = self.temps_segs.get(undone).copied().flatten();
                    if undone < self.temps_segs.len() {
                        self.temps_segs[undone] = None;
                    }
                    self.timer.undo_split(duree);
                    let index = self.timer.state.current_split_index;
                    log::info!("[ASL] Split undo (manuel) → #{}", index);
                    return Some(SpeedrunEvent::SplitUndone { index });
                }
                None
            }
            ActionManuelle::Reset => {
                // Un index armé en NotRunning compte comme un état à reset.
                if phase_before != TimerPhase::NotRunning
                    || self.timer.state.armed_index.is_some()
                {
                    self.timer.reset();
                    self.temps_segs.iter_mut().for_each(|t| *t = None);
                    log::info!("[ASL] Reset (manuel)");
                    return Some(SpeedrunEvent::Reset);
                }
                None
            }
            ActionManuelle::GoTo { index } => {
                let total = self.timer.state.total_splits;
                if index < 0 || (total > 0 && index >= total) {
                    return None;
                }
                self.timer.aller_a(index);
                // Retry de segment : tous les temps de la tentative sont
                // effacés (avant et après l'index) — les précédents ne
                // comptent pas comme finis. Le PB reste la référence
                // affichée (colonne droite).
                self.temps_segs.iter_mut().for_each(|t| *t = None);
                let phase = format!("{:?}", self.timer.state.current_phase);
                log::info!("[ASL] Goto split #{} (phase {})", index, phase);
                Some(SpeedrunEvent::Goto { index, phase })
            }
            ActionManuelle::Pause => {
                // pause() est un toggle : Running → Paused, Paused → Running
                self.timer.pause();
                match self.timer.state.current_phase {
                    TimerPhase::Paused => {
                        log::info!("[ASL] Pause (manuel)");
                        Some(SpeedrunEvent::Paused)
                    }
                    TimerPhase::Running => {
                        log::info!("[ASL] Resume (manuel)");
                        Some(SpeedrunEvent::Resumed)
                    }
                    _ => None,
                }
            }
        }
    }

    /// Construit la MemoryWatcherList depuis les state() du script ASL pour
    /// le processus connecté. Prend la première state def correspondant au
    /// nom du processus (la gestion multi-versions est en P3, hors scope Lot 1).
    fn construire_watchers(&mut self, process_name: &str, script: &AslScript) {
        let mut watchers = MemoryWatcherList::new();
        if let Some(states) = script.states.get(process_name) {
            // Première state def (version non discriminée en v1)
            if let Some(state) = states.first() {
                for var in &state.variables {
                    let wtype = match type_name_vers_watcher(&var.type_name) {
                        Some(t) => t,
                        None => {
                            log::warn!(
                                "[ASL] Type '{}' non supporté pour la variable '{}'",
                                var.type_name,
                                var.identifier
                            );
                            continue;
                        }
                    };
                    // Construction du DeepPointer depuis la variable de state().
                    // new_state_var applique la sémantique LiveSplit exacte :
                    //   - module vide → module principal du processus (et NON
                    //     process_name : "Fusion" ne matchait jamais "Fusion.exe"
                    //     dans trouver_module → watchers null → start()=false à
                    //     vie, RCA-1 SoR/Fusion).
                    //   - prepend 0 aux offsets (LiveSplit InitializeOffsets :
                    //     deref base en premier, RCA-2 SoR/Fusion).
                    // Scope : réservé au chemin state(). MGS utilise from_absolute
                    // (state() vides) → non affecté.
                    let ptr = if var.module.is_empty() {
                        DeepPointer::new_state_var(None, var.base, &var.offsets)
                    } else {
                        DeepPointer::new_state_var(Some(&var.module), var.base, &var.offsets)
                    };
                    let mut watcher = MemoryWatcher::new(&var.identifier, ptr, wtype);
                    watcher.enabled = true;
                    watchers.add(watcher);
                }
                log::debug!(
                    "[ASL] {} watcher(s) construit(s) depuis state('{}')",
                    watchers.len(),
                    process_name
                );
            }
        }
        self.watchers = watchers;
    }
}

/// Mappe un nom de type ASL (ex: "int", "float", "string16") vers WatcherType.
fn type_name_vers_watcher(type_name: &str) -> Option<WatcherType> {
    match type_name {
        "int" => Some(WatcherType::Int),
        "uint" => Some(WatcherType::UInt),
        "long" => Some(WatcherType::Long),
        "ulong" => Some(WatcherType::ULong),
        "float" => Some(WatcherType::Float),
        "double" => Some(WatcherType::Double),
        "byte" => Some(WatcherType::Byte),
        "sbyte" => Some(WatcherType::SByte),
        "short" => Some(WatcherType::Short),
        "ushort" => Some(WatcherType::UShort),
        "bool" => Some(WatcherType::Bool),
        s if s.starts_with("string") => {
            let len: usize = s[6..].parse().ok()?;
            Some(WatcherType::String(len))
        }
        b if b.starts_with("byte") && !b.starts_with("sbyte") => {
            let len: usize = b[4..].parse().ok()?;
            Some(WatcherType::Bytes(len))
        }
        _ => None,
    }
}

/// Sérialise les valeurs courantes des watchers en JSON pour le passage au JS.
/// Format : { "VarName": valeur, ... } (valeur = number/string/bool/null).
pub fn serialize_watchers_current(watchers: &MemoryWatcherList) -> String {
    let mut map = serde_json::Map::new();
    for w in watchers.iter() {
        let val = w.current.as_ref().map(watched_value_to_json).unwrap_or(serde_json::Value::Null);
        map.insert(w.name.clone(), val);
    }
    serde_json::Value::Object(map).to_string()
}

/// Sérialise les valeurs précédentes des watchers en JSON.
pub fn serialize_watchers_old(watchers: &MemoryWatcherList) -> String {
    let mut map = serde_json::Map::new();
    for w in watchers.iter() {
        let val = w.old.as_ref().map(watched_value_to_json).unwrap_or(serde_json::Value::Null);
        map.insert(w.name.clone(), val);
    }
    serde_json::Value::Object(map).to_string()
}

/// Convertit une WatchedValue en serde_json::Value.
fn watched_value_to_json(v: &WatchedValue) -> serde_json::Value {
    match v {
        WatchedValue::Int(x) => serde_json::json!(x),
        WatchedValue::UInt(x) => serde_json::json!(x),
        WatchedValue::Long(x) => serde_json::json!(x),
        WatchedValue::ULong(x) => serde_json::json!(x),
        WatchedValue::Float(x) => serde_json::json!(x),
        WatchedValue::Double(x) => serde_json::json!(x),
        WatchedValue::Byte(x) => serde_json::json!(x),
        WatchedValue::SByte(x) => serde_json::json!(x),
        WatchedValue::Short(x) => serde_json::json!(x),
        WatchedValue::UShort(x) => serde_json::json!(x),
        WatchedValue::Bool(x) => serde_json::json!(x),
        WatchedValue::String(s) => serde_json::json!(s),
        WatchedValue::Bytes(b) => serde_json::json!(b),
    }
}

/// Fonction principale du thread moteur.
fn engine_thread_main<F>(rx: mpsc::Receiver<EngineCommand>, callback: F)
where
    F: Fn(SpeedrunEvent),
{
    let mut engine = EngineInner::new();

    loop {
        // Traiter les commandes (non-bloquant)
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                EngineCommand::LoadAsl { path, respond } => {
                    let result = engine.handle_load_asl(&path);
                    let _ = respond.send(result);
                }
                EngineCommand::Start {
                    total_splits,
                    settings,
                } => {
                    engine.handle_start(total_splits, settings);
                }
                EngineCommand::Stop => {
                    engine.handle_stop();
                    callback(SpeedrunEvent::Stopped);
                }
                EngineCommand::UpdateSettings { settings } => {
                    engine.settings = settings;
                }
                EngineCommand::ActionManuelle { action } => {
                    if let Some(event) = engine.handle_action_manuelle(action) {
                        callback(event);
                    }
                }
                EngineCommand::EmitEtat => {
                    callback(engine.etat_courant());
                }
                EngineCommand::MajSettingsAsl { valeurs } => {
                    engine.script_ctx.maj_settings_asl(&valeurs);
                    log::debug!(
                        "[ASL] {} setting(s) individuel(s) mis à jour",
                        valeurs.len()
                    );
                }
                EngineCommand::Shutdown => {
                    engine.handle_stop();
                    return;
                }
            }
        }

        // Polling
        if engine.running {
            if let Some(event) = engine.poll_tick(&callback) {
                callback(event);
            }
        }

        // Sleep selon le refresh rate
        let interval_ms = if engine.running {
            (1000.0 / engine.refresh_rate) as u64
        } else {
            50 // Idle : 50ms entre les checks de commandes
        };
        thread::sleep(Duration::from_millis(interval_ms.max(10)));
    }
}

/// Sérialise l'état du timer en JSON pour le passage vers JS.
fn serialize_timer(state: &crate::speedrun::engine::timer::TimerState) -> String {
    let ct = state.current_time();
    serde_json::json!({
        "currentPhase": format!("{:?}", state.current_phase),
        "currentSplitIndex": state.current_split_index,
        "totalSplits": state.total_splits,
        "currentTime": ct.format(),
        "gameTime": ct.game_time,
        "realTime": ct.real_time,
    })
    .to_string()
}

/// Sérialise un processus en JSON pour le passage vers JS.
fn serialize_process(proc: &Process) -> String {
    let modules: Vec<serde_json::Value> = proc
        .modules()
        .iter()
        .map(|m| {
            serde_json::json!({
                "moduleName": m.module_name,
                "baseAddress": m.base_address,
                "moduleMemorySize": m.module_memory_size,
            })
        })
        .collect();

    serde_json::json!({
        "id": proc.id,
        "processName": proc.name,
        "is64Bit": proc.is_64_bit(),
        "modules": modules,
    })
    .to_string()
}

// =============================================================================
// Tests unitaires — sérialisation des events (camelCase pour le frontend JS)
// =============================================================================
#[cfg(test)]
mod tests_event_serialization {
    use super::SpeedrunEvent;

    /// L'event Time doit sérialiser realTime/gameTime en camelCase (pas snake_case).
    /// Le frontend JS lit evt.realTime / evt.gameTime — si la sérialisation produit
    /// real_time/game_time (snake_case), les champs sont undefined en JS et le
    /// chrono reste à 00:00.00.
    #[test]
    fn test_time_event_camelcase_serialization() {
        let event = SpeedrunEvent::Time {
            real_time: "00:05.123".to_string(),
            game_time: Some("00:03.456".to_string()),
            segment_time: Some("00:02.000".to_string()),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains("\"realTime\""),
            "real_time doit être sérialisé en realTime (camelCase), obtenu: {}", json
        );
        assert!(
            json.contains("\"gameTime\""),
            "game_time doit être sérialisé en gameTime (camelCase), obtenu: {}", json
        );
        assert!(
            json.contains("\"type\":\"time\""),
            "Le tag type doit être 'time', obtenu: {}", json
        );
    }

    /// L'event Time avec game_time None doit sérialiser gameTime: null
    #[test]
    fn test_time_event_null_game_time() {
        let event = SpeedrunEvent::Time {
            real_time: "00:01.000".to_string(),
            game_time: None,
            segment_time: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains("\"realTime\":\"00:01.000\""),
            "realTime doit être présent, obtenu: {}", json
        );
        assert!(
            json.contains("\"gameTime\":null"),
            "gameTime doit être null, obtenu: {}", json
        );
    }
}
