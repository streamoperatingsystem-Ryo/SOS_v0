//! Conso — échantillonneur 1Hz de la consommation de l'application.
//! (modale « Diagnostic » côté dashboard ; le module s'appelle `conso` car
//! `diagnostic` masque le namespace built-in rustc `#[diagnostic::*]` que
//! la macro `tauri::command` génère — collision E0433.)
//!
//! But : rendre visible « qui consomme quoi » (crash thermique 2026-09-21) :
//!   - CPU par SOUS-SYSTÈME (threads nommés : splitter ASL, input viewer…)
//!     en % d'un cœur — la métrique qui a révélé le bug 66Hz.
//!   - CPU/RAM par PROCESSUS (StreamOS, WebView2 dashboard, OBS, jeu, système)
//!     en % du CPU total.
//!   - Compteurs d'activité : messages chat/s, events speedrun/s, publications
//!     input viewer/s — via un tap PASSIF sur chat_tx (zéro modification des
//!     émetteurs, même pattern que la télémétrie).
//!
//! Contraintes :
//!   - NON énergivore : 1 échantillon/s = 1 snapshot processus + 1 snapshot
//!     threads (à comparer aux ~600 snapshots/s du bug d'origine) — et
//!     UNIQUEMENT pendant que la modale pole `conso_etat` : la boucle dort
//!     tant que la dernière demande date de plus de 3 s (auto-expiration ;
//!     correctif 2026-10-13 de l'omission : avant, le thread samplait 1/s
//!     à vie dès le boot alors que l'affichage était déjà à la demande).
//!   - Historique roulant en mémoire : 300 points (5 min), jamais sur disque.
//!     Vidé à la reprise après dormance (les échantillons de la session
//!     d'affichage précédente sont périmés).
//!   - Threads identifiés par registre TID→label (rempli au spawn via
//!     `nommer_thread`) — pas de GetThreadDescription, robuste.
//!   - Toute mesure qui échoue vaut 0 — le diagnostic ne crash jamais l'app.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

/// Taille de l'historique roulant (5 min à 1Hz).
pub const TAILLE_HISTORIQUE: usize = 300;

/// Label du bucket par défaut (threads non enregistrés : runtime tokio,
/// serveur :4321, tauri…).
const LABEL_AUTRES: &str = "App + runtime (chat, WS, serveur)";

// =============================================================================
// COMPTEURS D'ACTIVITÉ (bumpés par le tap chat_tx, lus par l'échantillonneur)
// =============================================================================

static MSGS_CHAT: AtomicU64 = AtomicU64::new(0);
static EVENTS_SPEEDRUN: AtomicU64 = AtomicU64::new(0);
static PUBS_INPUT_VIEWER: AtomicU64 = AtomicU64::new(0);

// =============================================================================
// DEMANDE — gating de l'échantillonneur (2026-10-13 : ne sampler QUE pendant
// que la modale Diagnostic est affichée ; avant, la boucle samplait à vie)
// =============================================================================

/// Millis epoch de l'appel le plus récent à `conso_etat` (0 = jamais appelé).
static DEMANDE_MS: AtomicU64 = AtomicU64::new(0);

/// Fenêtre de validité d'une demande : le frontend pole à 1 Hz → 3 s tolère
/// un poll perdu sans jamais laisser la boucle sampler à vide quand la
/// modale est fermée.
const DEMANDE_EXPIRE_MS: u64 = 3_000;

/// Un écart mesuré > ce seuil signale une reprise après dormance : les taux
/// seraient moyensés sur toute la période d'inactivité → ce tick ne sert
/// qu'à rafraîchir les baselines (même sémantique que premier_tick).
#[cfg(windows)]
const SEUIL_REPRISE_MS: f64 = 2_500.0;

fn maintenant_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// True ssi la modale Diagnostic est considérée ouverte (dernier poll
/// `conso_etat` de moins de DEMANDE_EXPIRE_MS).
fn demande_recente() -> bool {
    let derniere = DEMANDE_MS.load(Ordering::Relaxed);
    derniere > 0 && maintenant_epoch_ms().saturating_sub(derniere) < DEMANDE_EXPIRE_MS
}

/// Nom du processus jeu connecté au splitter (extrait des events speedrun
/// « connected »/« disconnected » du tap chat_tx — zéro couplage moteur).
fn processus_jeu_slot() -> &'static Mutex<Option<String>> {
    static S: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

// =============================================================================
// REGISTRE DES THREADS NOMMÉS (TID → label d'affichage)
// =============================================================================

fn registre_threads_slot() -> &'static Mutex<HashMap<u32, String>> {
    static R: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Enregistre le THREAD COURANT sous un label — à appeler à la première
/// ligne du closure de `thread::spawn`. L'échantillonneur agrège le CPU des
/// threads par label ; les threads non enregistrés tombent dans LABEL_AUTRES.
/// Les labels de threads morts restent dans le registre (taille bornée par
/// le nombre de spawns, sans effet : le TID ne matche plus).
#[cfg(windows)]
pub fn nommer_thread(label: &str) {
    use windows::Win32::System::Threading::GetCurrentThreadId;
    let tid = unsafe { GetCurrentThreadId() };
    registre_threads_slot()
        .lock()
        .unwrap()
        .insert(tid, label.to_string());
}

#[cfg(not(windows))]
pub fn nommer_thread(_label: &str) {}

// =============================================================================
// MODÈLE SÉRIALISABLE (payload de la commande diagnostic_etat)
// =============================================================================

/// Un processus mesuré (lignes « processus » de la modale).
#[derive(Clone, Debug, Serialize)]
pub struct ProcessusMesure {
    pub nom: String,
    /// % du CPU TOTAL (tous cœurs confondus).
    pub cpu_pct: f64,
    /// Mémoire de travail, en Mo.
    pub ram_mo: f64,
    /// % GPU (compteurs « GPU Engine » WDDM, vendor-agnostic). None = mesure
    /// indisponible (VM, vieux Windows, compteur absent) → « N/A » côté UI.
    pub gpu_pct: Option<f64>,
}

/// Un sous-système mesuré (threads de StreamOS agrégés par label).
#[derive(Clone, Debug, Serialize)]
pub struct ThreadMesure {
    pub nom: String,
    /// % D'UN CŒUR (un thread saturé = 100 %).
    pub cpu_pct: f64,
}

/// Compteurs d'activité (événements par seconde, fenêtre du dernier tick).
#[derive(Clone, Debug, Default, Serialize)]
pub struct ActiviteMesure {
    pub chat_msgs: u32,
    pub speedrun_events: u32,
    pub input_viewer_pubs: u32,
}

/// Un échantillon (une seconde d'historique).
#[derive(Clone, Debug, Serialize)]
pub struct Echantillon {
    /// Epoch millisecondes.
    pub ts: u64,
    pub processus: Vec<ProcessusMesure>,
    pub threads: Vec<ThreadMesure>,
    pub activite: ActiviteMesure,
}

/// Réponse de `conso_etat`.
#[derive(Clone, Debug, Serialize)]
pub struct ConsoEtat {
    /// Nombre de cœurs logiques (échelle des %).
    pub nb_coeurs: u32,
    /// Historique roulant (le plus ancien d'abord).
    pub historique: Vec<Echantillon>,
}

fn nb_coeurs() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

struct EtatInterne {
    historique: VecDeque<Echantillon>,
    nb_coeurs: u32,
}

fn etat_slot() -> &'static Mutex<EtatInterne> {
    static E: OnceLock<Mutex<EtatInterne>> = OnceLock::new();
    E.get_or_init(|| {
        Mutex::new(EtatInterne {
            historique: VecDeque::new(),
            nb_coeurs: nb_coeurs(),
        })
    })
}

// =============================================================================
// BOÎTE NOIRE — journal persistant (5 s) qui survit à un gel / coupure
// =============================================================================
//
// Différence avec la boucle 1 Hz ci-dessous : elle DORT quand la modale
// Diagnostic est fermée (contrat « non énergivore », 2026-10-13). La boîte
// noire tourne EN PERMANENCE — c'est la seule trace qui reste après un gel
// total de la machine. Constat 2026-10-27 : `events.jsonl` s'arrêtait à la
// seconde du gel, sans rien sur les secondes précédentes, et la modale était
// fermée → zéro échantillon existait.
//
// Coût volontairement minimal : UN snapshot pour soi (CPU + RSS) + UN
// snapshot pour chercher obs64.exe + `GlobalMemoryStatusEx`. Pas de PDH/GPU,
// pas de snapshot threads. `sync_data()` à chaque ligne : sans lui la
// dernière écriture resterait dans le cache de l'OS et disparaîtrait avec la
// coupure secteur — c'est précisément ce qu'on veut éviter.

/// Cadence de la boîte noire (permanente, modale fermée comprise).
#[cfg(windows)]
const BOITE_NOIRE_INTERVALLE: Duration = Duration::from_secs(5);

/// Borne de rotation : au-delà, le fichier devient `boite_noire.1.jsonl`
/// (UNE seule génération conservée → 2 fichiers max). ~2 Mio ≈ 1 jour à 5 s.
#[cfg(windows)]
const BOITE_NOIRE_MAX_OCTETS: u64 = 2 * 1024 * 1024;

/// Millis epoch du dernier RPC OBS réussi (tous appelants confondus, via
/// `obs::rpc`). 0 = aucun depuis le boot. C'est ce qui révèle un décrochage
/// OBS silencieux : le 2026-10-27, OBS n'a plus rien loggé à 15:18 alors que
/// l'app vivait encore à 15:55 — sans ce champ, indétectable.
static OBS_DERNIER_OK_MS: AtomicU64 = AtomicU64::new(0);

/// À appeler sur CHAQUE RPC OBS réussi (`obs::rpc` — point de passage
/// unique de tous les appels obs-websocket, y compris obs_trou).
pub fn marquer_obs_ok() {
    OBS_DERNIER_OK_MS.store(maintenant_epoch_ms(), Ordering::Relaxed);
}

/// Nombre d'appels à la commande `obs_connect` depuis le boot + origine du
/// dernier appel. Sert à coincer un appelant en boucle : `obs_connect` force
/// `SetCurrentProgramScene("SOS")`, donc chaque appel bascule la scène OBS
/// (constat 2026-10-27 : 24 bascules sur un live où l'utilisateur n'a rien
/// touché). Le compteur + l'origine sont écrits dans la boîte noire.
static OBS_CONNECT_N: AtomicU64 = AtomicU64::new(0);

fn obs_origine_slot() -> &'static Mutex<Option<String>> {
    static O: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    O.get_or_init(|| Mutex::new(None))
}

/// À appeler au début de la commande `obs_connect`. `origine` vient du
/// frontend : "boot" (auto-connect), "bouton" (section OBS), "raccourci"
/// (Stream Deck / barre de raccourcis).
pub fn marquer_obs_connect(origine: &str) {
    OBS_CONNECT_N.fetch_add(1, Ordering::Relaxed);
    *obs_origine_slot().lock().unwrap() = Some(origine.to_string());
}

/// Une ligne de `boite_noire.jsonl`. Volontairement pauvre : écrivable en
/// quelques microsecondes toutes les 5 s. Pas de `cfg(windows)` : la commande
/// de lecture doit exister sur toutes les plateformes (elle renvoie vide).
#[derive(Serialize, Deserialize)]
pub struct EchantillonBoiteNoire {
    v: u8,
    /// Epoch millisecondes.
    ts: u64,
    /// Mémoire de travail de StreamOS, Mo.
    ram_mo: f64,
    /// % du CPU total (tous cœurs) consommé par StreamOS.
    cpu_pct: f64,
    /// RAM système OCCUPÉE, Mo (croissance = fuite ou charge).
    ram_systeme_mo: f64,
    /// `obs64.exe`/`obs32.exe` présent. ⚠️ Ne dit RIEN de l'état websocket :
    /// `obs.rs` n'a aucun état persistant (chaque RPC = connect→close).
    obs_present: bool,
    /// Secondes écoulées depuis le dernier RPC OBS réussi. `None` = aucun
    /// depuis le boot. Une valeur qui grimpe = OBS ne répond plus.
    obs_silence_s: Option<u64>,
    /// Nombre d'appels à `obs_connect` depuis le boot (chaque appel bascule
    /// la scène OBS sur "SOS" si aucun stream ne tourne).
    obs_connect_n: u64,
    /// Origine du dernier `obs_connect` ("boot" / "bouton" / "raccourci").
    obs_connect_origine: Option<String>,
}

/// Chemin de la boîte noire — crée `telemetrie/` si besoin.
pub fn chemin_boite_noire(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = crate::config::data_dir(app)?.join("telemetrie");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Créer telemetrie/: {}", e))?;
    }
    Ok(dir.join("boite_noire.jsonl"))
}

/// Chemin retenu au boot (rempli par `demarrer`) — lu par la commande de
/// lecture, qui n'a pas d'`AppHandle`.
static CHEMIN_BOITE_NOIRE: OnceLock<PathBuf> = OnceLock::new();

/// Réponse de `boite_noire_etat`.
#[derive(Serialize)]
pub struct BoiteNoireEtat {
    /// Du plus ancien au plus récent.
    pub echantillons: Vec<EchantillonBoiteNoire>,
    /// Lignes illisibles ignorées. ⚠️ Non nul est NORMAL : une coupure
    /// secteur laisse une ligne partielle en fin de fichier.
    pub ignorees: usize,
}

/// Journal persistant de la boîte noire : `limite` derniers points
/// (défaut 720 = 1 h à 5 s). Lecture bornée — le fichier fait ≤ 2 Mio.
/// Ne polle pas : à appeler à l'ouverture de la section.
#[tauri::command]
pub fn boite_noire_etat(limite: Option<usize>) -> BoiteNoireEtat {
    let mut etat = BoiteNoireEtat {
        echantillons: Vec::new(),
        ignorees: 0,
    };
    let Some(chemin) = CHEMIN_BOITE_NOIRE.get() else {
        return etat;
    };
    let Ok(texte) = std::fs::read_to_string(chemin) else {
        return etat;
    };
    let mut tous: Vec<EchantillonBoiteNoire> = Vec::new();
    for ligne in texte.lines() {
        if ligne.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<EchantillonBoiteNoire>(ligne) {
            Ok(e) => tous.push(e),
            Err(_) => etat.ignorees += 1,
        }
    }
    let debut = tous.len().saturating_sub(limite.unwrap_or(720));
    etat.echantillons = tous.split_off(debut);
    etat
}

/// Ouvre le journal en append, rotation faite si la borne est dépassée.
/// None = écriture impossible (dégradé : la boucle continue de mesurer).
#[cfg(windows)]
fn ouvrir_boite_noire(chemin: &std::path::Path) -> Option<std::fs::File> {
    if let Ok(m) = std::fs::metadata(chemin) {
        if m.len() > BOITE_NOIRE_MAX_OCTETS {
            let ancien = chemin.with_extension("1.jsonl");
            let _ = std::fs::remove_file(&ancien);
            let _ = std::fs::rename(chemin, &ancien);
        }
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(chemin)
        .ok()
}

/// Écrit UNE ligne et force le disque. `flush` seul ne suffit pas : les
/// octets resteraient dans le cache de l'OS, perdus à la coupure.
#[cfg(windows)]
fn ecrire_boite_noire(f: &mut std::fs::File, ligne: &str) -> std::io::Result<()> {
    use std::io::Write;
    writeln!(f, "{}", ligne)?;
    f.flush()?;
    f.sync_data()
}

/// `obs64.exe`/`obs32.exe` dans la liste des processus. Snapshot dédié : ne
/// partage AUCUNE map `prev` avec la boucle 1 Hz (sinon les deux
/// échantillonneurs se voleraient mutuellement leurs deltas CPU).
#[cfg(windows)]
fn obs_present() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut trouve = false;
    if unsafe { Process32FirstW(snap, &mut entry) }.is_ok() {
        loop {
            let nom = String::from_utf16_lossy(&entry.szExeFile)
                .trim_end_matches('\0')
                .to_lowercase();
            if nom == "obs64.exe" || nom == "obs32.exe" {
                trouve = true;
                break;
            }
            if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snap) };
    trouve
}

/// Boucle permanente (5 s) — ne dort jamais : c'est le but.
#[cfg(windows)]
fn boucle_boite_noire(chemin: PathBuf) {
    use windows::Win32::System::Threading::GetCurrentProcess;
    // Visible dans la modale Diagnostic (registre TID→label).
    nommer_thread("Boîte noire (écriture)");
    let coeurs = nb_coeurs() as f64;
    let mut fichier = ouvrir_boite_noire(&chemin);
    let mut precedent = Instant::now();
    let mut prev_cpu: Option<u64> = None;
    loop {
        std::thread::sleep(BOITE_NOIRE_INTERVALLE);
        let maintenant = Instant::now();
        let ecoule_ms = maintenant.duration_since(precedent).as_millis() as f64;
        precedent = maintenant;
        let h = unsafe { GetCurrentProcess() };
        let cpu = temps_cpu_processus(h);
        let cpu_pct = match (prev_cpu, cpu) {
            (Some(p), Some(c)) if ecoule_ms > 0.0 => {
                ((c.saturating_sub(p) as f64) / (ecoule_ms * coeurs) * 100.0).min(100.0)
            }
            _ => 0.0,
        };
        prev_cpu = cpu;
        let echantillon = EchantillonBoiteNoire {
            v: 1,
            ts: maintenant_epoch_ms(),
            ram_mo: ram_processus_mo(h),
            cpu_pct,
            ram_systeme_mo: ram_systeme_mo(),
            obs_present: obs_present(),
            obs_silence_s: {
                let dernier = OBS_DERNIER_OK_MS.load(Ordering::Relaxed);
                (dernier > 0)
                    .then(|| maintenant_epoch_ms().saturating_sub(dernier) / 1000)
            },
            obs_connect_n: OBS_CONNECT_N.load(Ordering::Relaxed),
            obs_connect_origine: obs_origine_slot().lock().unwrap().clone(),
        };
        let Ok(ligne) = serde_json::to_string(&echantillon) else {
            continue;
        };
        // Réouverture paresseuse : fichier supprimé/roté à la main, ou
        // première ouverture ratée (disque occupé) — on retente au tick.
        if fichier.is_none() {
            fichier = ouvrir_boite_noire(&chemin);
        }
        if let Some(f) = fichier.as_mut() {
            if ecrire_boite_noire(f, &ligne).is_err() {
                fichier = None;
            }
        }
    }
}

#[cfg(not(windows))]
fn boucle_boite_noire(_chemin: PathBuf) {}

// =============================================================================
// DÉMARRAGE (setup) — tap chat_tx + thread échantillonneur
// =============================================================================

/// Démarre le diagnostic. Appelé UNE fois au boot (lib.rs setup).
/// `chat_tx` : même canal broadcast que le WS :4321 — tap passif pour les
/// compteurs d'activité et le nom du processus jeu (pattern télémétrie).
/// `chemin_boite_noire` : journal persistant 5 s (voir `chemin_boite_noire`).
pub fn demarrer(chat_tx: broadcast::Sender<String>, chemin_boite_noire: PathBuf) {
    // 1. Tap passif : compter les frames par type. Détection par sous-chaîne
    //    (serde_json ne garantit PAS l'ordre des clés — jamais de préfixe).
    let mut rx = chat_tx.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(raw) => compter_frame(&raw),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Canal saturé (rafale chat) — compteurs indicatifs, on suit.
                }
                Err(_) => break, // Émetteur droppé = fin d'app
            }
        }
    });

    // 2. Thread échantillonneur 1Hz (détaché — vit autant que l'app).
    std::thread::spawn(boucle_echantillonnage);

    // 3. Boîte noire — journal persistant, INDÉPENDANT du gating de la modale
    //    (contrairement à la boucle ci-dessus) : c'est la seule trace qui
    //    survit à un gel ou à une coupure secteur.
    let _ = CHEMIN_BOITE_NOIRE.set(chemin_boite_noire.clone());
    std::thread::spawn(move || boucle_boite_noire(chemin_boite_noire));
}

/// Compte une frame chat_tx vers les compteurs d'activité + suit le nom du
/// processus jeu via les events speedrun connected/disconnected.
fn compter_frame(raw: &str) {
    if raw.contains("\"type\":\"chat\"") {
        MSGS_CHAT.fetch_add(1, Ordering::Relaxed);
    } else if raw.contains("\"type\":\"input-viewer-etat\"") {
        PUBS_INPUT_VIEWER.fetch_add(1, Ordering::Relaxed);
    } else if raw.contains("\"type\":\"speedrun-etat\"") {
        EVENTS_SPEEDRUN.fetch_add(1, Ordering::Relaxed);
        // Connexion/déconnexion du jeu : garder le nom pour la ligne « Jeu »
        // (parse complet uniquement sur ces events rares).
        if raw.contains("\"type\":\"connected\"") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
                let nom = v
                    .pointer("/etat/process")
                    .and_then(|p| p.as_str())
                    .map(str::to_string);
                *processus_jeu_slot().lock().unwrap() = nom;
            }
        } else if raw.contains("\"type\":\"disconnected\"") {
            *processus_jeu_slot().lock().unwrap() = None;
        }
    }
}

// =============================================================================
// COMMANDE TAURI
// =============================================================================

/// État courant du diagnostic (historique complet + nb cœurs). Le frontend
/// ne pole QUE quand la modale Diagnostic est ouverte (1 invoke/s max) —
/// chaque poll repousse l'auto-expiration de la boucle d'échantillonnage
/// (voir `demande_recente`) : sampler seulement pendant qu'on est lu.
#[tauri::command]
pub fn conso_etat() -> ConsoEtat {
    DEMANDE_MS.store(maintenant_epoch_ms(), Ordering::Relaxed);
    let etat = etat_slot().lock().unwrap();
    ConsoEtat {
        nb_coeurs: etat.nb_coeurs,
        historique: etat.historique.iter().cloned().collect(),
    }
}

// =============================================================================
// BOUCLE D'ÉCHANTILLONNAGE — Windows (mesures) / autres (no-op)
// =============================================================================

#[cfg(windows)]
fn boucle_echantillonnage() {
    // Temps CPU précédents : PID → ms (processus), TID → ms (threads),
    // (idle, total) → système. Remplacés à chaque tick par les valeurs
    // fraîches → les processus/threads disparus sortent naturellement.
    let mut prev_processus: HashMap<u32, u64> = HashMap::new();
    let mut prev_threads: HashMap<u32, u64> = HashMap::new();
    let mut prev_systeme: Option<(u64, u64)> = None;
    let mut prev_activite: (u64, u64, u64) = (0, 0, 0);
    let mut precedent = Instant::now();
    let mut premier_tick = true;
    // Requête PDH « GPU Engine » ouverte UNE fois (None = GPU non mesurable
    // sur cette machine → les lignes affichent « N/A »).
    let mut gpu = gpu_pdh_ouvrir();

    loop {
        std::thread::sleep(Duration::from_secs(1));
        // Gating : dormant tant que la modale n'est pas affichée — zéro
        // snapshot, zéro requête PDH, zéro verrou. `precedent` n'est PAS
        // touché en dormant : l'écart accumulé sert à détecter la reprise.
        if !demande_recente() {
            continue;
        }
        let maintenant = Instant::now();
        let ecoule_ms = maintenant.duration_since(precedent).as_millis() as f64;
        precedent = maintenant;
        if ecoule_ms <= 0.0 {
            continue;
        }
        // Reprise après dormance : l'écart couvre la période d'inactivité →
        // taux moyensés sans signification ET historique mémoire périmé
        // (il date de la session d'affichage précédente). Ce tick ne sert
        // qu'à rafraîchir les baselines (premier_tick = true saute le push)
        // et l'historique repart vide.
        if ecoule_ms > SEUIL_REPRISE_MS {
            premier_tick = true;
            etat_slot().lock().unwrap().historique.clear();
        }
        let coeurs = etat_slot().lock().unwrap().nb_coeurs as f64;

        // 1. GPU (snapshot PDH) puis processus + threads (chaque helper
        //    remplace les maps prev).
        let gpu_map = gpu.as_mut().map(gpu_mesurer);
        let mut processus =
            mesurer_processus(&mut prev_processus, ecoule_ms, coeurs, gpu_map.as_ref());
        let threads = mesurer_threads(&mut prev_threads, ecoule_ms);
        if let Some(mut sys) = mesurer_systeme(&mut prev_systeme) {
            // Ligne « Système » : GPU total = somme des moteurs de tous les
            // processus (approximation Task Manager, bornée à 100).
            if let Some(m) = &gpu_map {
                sys.gpu_pct = Some(m.values().sum::<f64>().min(100.0));
            }
            processus.push(sys);
        }

        // 2. Compteurs d'activité (delta sur la fenêtre de 1s).
        let actuel_activite = (
            MSGS_CHAT.load(Ordering::Relaxed),
            EVENTS_SPEEDRUN.load(Ordering::Relaxed),
            PUBS_INPUT_VIEWER.load(Ordering::Relaxed),
        );
        let activite = if premier_tick {
            ActiviteMesure::default()
        } else {
            ActiviteMesure {
                chat_msgs: actuel_activite.0.saturating_sub(prev_activite.0) as u32,
                speedrun_events: actuel_activite.1.saturating_sub(prev_activite.1) as u32,
                input_viewer_pubs: actuel_activite.2.saturating_sub(prev_activite.2) as u32,
            }
        };
        prev_activite = actuel_activite;

        // 3. Pousser l'échantillon (le premier tick ne sert qu'aux baselines).
        if !premier_tick {
            let echantillon = Echantillon {
                ts: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                processus,
                threads,
                activite,
            };
            let mut etat = etat_slot().lock().unwrap();
            etat.historique.push_back(echantillon);
            while etat.historique.len() > TAILLE_HISTORIQUE {
                etat.historique.pop_front();
            }
        }
        premier_tick = false;
    }
}

#[cfg(not(windows))]
fn boucle_echantillonnage() {}

// =============================================================================
// MESURES WINDOWS — helpers
// =============================================================================

#[cfg(windows)]
fn ft_u64(f: &windows::Win32::Foundation::FILETIME) -> u64 {
    (f.dwLowDateTime as u64) | ((f.dwHighDateTime as u64) << 32)
}

/// Temps CPU (kernel + user, ms) d'un handle PROCESSUS. `h` doit venir de
/// OpenProcess ou GetCurrentProcess (pseudo-handle — jamais fermé).
#[cfg(windows)]
fn temps_cpu_processus(
    h: windows::Win32::Foundation::HANDLE,
) -> Option<u64> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetProcessTimes;
    let mut c = FILETIME::default();
    let mut e = FILETIME::default();
    let mut k = FILETIME::default();
    let mut u = FILETIME::default();
    if unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) }.is_err() {
        return None;
    }
    Some((ft_u64(&k) + ft_u64(&u)) / 10_000)
}

/// Temps CPU (kernel + user, ms) d'un thread par son TID (handle ouvert/
/// fermé localement — THREAD_QUERY_LIMITED_INFORMATION suffit).
#[cfg(windows)]
fn temps_cpu_thread(tid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
    };
    let Ok(h) = (unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, tid) }) else {
        return None;
    };
    let mut c = FILETIME::default();
    let mut e = FILETIME::default();
    let mut k = FILETIME::default();
    let mut u = FILETIME::default();
    let ok = unsafe { GetThreadTimes(h, &mut c, &mut e, &mut k, &mut u) };
    let _ = unsafe { CloseHandle(h) };
    if ok.is_err() {
        return None;
    }
    Some((ft_u64(&k) + ft_u64(&u)) / 10_000)
}

/// Mémoire de travail d'un handle processus, en Mo.
#[cfg(windows)]
fn ram_processus_mo(h: windows::Win32::Foundation::HANDLE) -> f64 {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    let mut mc = PROCESS_MEMORY_COUNTERS::default();
    let ok = unsafe {
        GetProcessMemoryInfo(h, &mut mc, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32)
    };
    if ok.is_err() {
        return 0.0;
    }
    mc.WorkingSetSize as f64 / (1024.0 * 1024.0)
}

/// (idle, total) système en unités 100ns via GetSystemTimes. NB : le kernel
/// retourné INCLUT l'idle → total = kernel + user compte l'idle une fois.
#[cfg(windows)]
fn temps_systeme_bruts() -> Option<(u64, u64)> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetSystemTimes;
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
        return None;
    }
    Some((ft_u64(&idle), ft_u64(&kernel) + ft_u64(&user)))
}

/// RAM physique utilisée, en Mo.
#[cfg(windows)]
fn ram_systeme_mo() -> f64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut mem = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_err() {
        return 0.0;
    }
    (mem.ullTotalPhys - mem.ullAvailPhys) as f64 / (1024.0 * 1024.0)
}

/// Mesure les processus : StreamOS (soi), WebView2 (enfants msedgewebview2
/// du dashboard), OBS, jeu connecté au splitter. `prev` est REMPLACÉ par les
/// valeurs fraîches (les processus disparus sortent de la map). `gpu` = map
/// PID→% du tick PDH (None = compteur indisponible → gpu_pct null/N/A).
#[cfg(windows)]
fn mesurer_processus(
    prev: &mut HashMap<u32, u64>,
    ecoule_ms: f64,
    coeurs: f64,
    gpu: Option<&HashMap<u32, f64>>,
) -> Vec<ProcessusMesure> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let pid_soi = unsafe { GetCurrentProcessId() };
    let nom_jeu = processus_jeu_slot()
        .lock()
        .unwrap()
        .clone()
        .map(|n| n.to_lowercase());
    let mut resultat: Vec<ProcessusMesure> = Vec::new();
    let mut nouveau: HashMap<u32, u64> = HashMap::new();
    // Agrégat WebView2 (plusieurs processus enfants).
    let mut wv_delta_ms = 0f64;
    let mut wv_ram = 0f64;
    let mut wv_gpu = 0f64;
    let mut wv_vus = 0u32;
    // Agrégat Jeu (plusieurs instances du même exe : launcher + jeu, doublon,
    // zombie — une SEULE ligne sinon clé dupliquée dans le {#each} Svelte de
    // ConsoModal → each_key_duplicate crashait la modale, 2026-09-23).
    let mut jeu_delta_ms = 0f64;
    let mut jeu_ram = 0f64;
    let mut jeu_gpu = 0f64;
    let mut jeu_vus = 0u32;
    // Lookup GPU par PID : Some(map) = compteur dispo (0.0 si le processus
    // n'a pas de moteur GPU), None = « N/A » côté UI.
    let gpu_pct = |pid: u32| gpu.map(|m| m.get(&pid).copied().unwrap_or(0.0).min(100.0));

    // Ouvre un processus par PID, mesure (delta vs prev) et retourne la row.
    let mesurer = |pid: u32,
                  prev: &HashMap<u32, u64>,
                  nouveau: &mut HashMap<u32, u64>|
     -> Option<(u64, f64)> {
        let h: HANDLE = if pid == pid_soi {
            unsafe { GetCurrentProcess() } // pseudo-handle — jamais fermé
        } else {
            let Ok(h) =
                (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
            else {
                return None;
            };
            h
        };
        let t = temps_cpu_processus(h)?;
        let ram = ram_processus_mo(h);
        if pid != pid_soi {
            let _ = unsafe { CloseHandle(h) };
        }
        let delta = t.saturating_sub(*prev.get(&pid).unwrap_or(&t));
        nouveau.insert(pid, t);
        Some((delta, ram))
    };

    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return resultat;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snap, &mut entry) }.is_ok() {
        loop {
            let nom = String::from_utf16_lossy(&entry.szExeFile)
                .trim_end_matches('\0')
                .to_lowercase();
            let pid = entry.th32ProcessID;
            let est_enfant_soi = entry.th32ParentProcessID == pid_soi;

            if pid == pid_soi {
                if let Some((delta, ram)) = mesurer(pid, prev, &mut nouveau) {
                    resultat.push(ProcessusMesure {
                        nom: "StreamOS".to_string(),
                        cpu_pct: ((delta as f64 / (ecoule_ms * coeurs)) * 100.0).min(100.0),
                        ram_mo: ram,
                        gpu_pct: gpu_pct(pid),
                    });
                }
            } else if nom.starts_with("msedgewebview2") && est_enfant_soi {
                if let Some((delta, ram)) = mesurer(pid, prev, &mut nouveau) {
                    wv_delta_ms += delta as f64;
                    wv_ram += ram;
                    wv_gpu += gpu_pct(pid).unwrap_or(0.0);
                    wv_vus += 1;
                }
            } else if nom == "obs64.exe" || nom == "obs32.exe" {
                if let Some((delta, ram)) = mesurer(pid, prev, &mut nouveau) {
                    resultat.push(ProcessusMesure {
                        nom: "OBS".to_string(),
                        cpu_pct: ((delta as f64 / (ecoule_ms * coeurs)) * 100.0).min(100.0),
                        ram_mo: ram,
                        gpu_pct: gpu_pct(pid),
                    });
                }
            } else if nom_jeu.as_deref() == Some(nom.as_str()) {
                if let Some((delta, ram)) = mesurer(pid, prev, &mut nouveau) {
                    jeu_delta_ms += delta as f64;
                    jeu_ram += ram;
                    jeu_gpu += gpu_pct(pid).unwrap_or(0.0);
                    jeu_vus += 1;
                }
            }

            if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snap) };

    if jeu_vus > 0 {
        resultat.push(ProcessusMesure {
            nom: format!("Jeu ({})", nom_jeu.clone().unwrap_or_default()),
            cpu_pct: ((jeu_delta_ms / (ecoule_ms * coeurs)) * 100.0).min(100.0),
            ram_mo: jeu_ram,
            gpu_pct: gpu.map(|_| jeu_gpu.min(100.0)),
        });
    }

    if wv_vus > 0 {
        resultat.push(ProcessusMesure {
            nom: "WebView2 (dashboard)".to_string(),
            cpu_pct: ((wv_delta_ms / (ecoule_ms * coeurs)) * 100.0).min(100.0),
            ram_mo: wv_ram,
            gpu_pct: gpu.map(|_| wv_gpu.min(100.0)),
        });
    }
    *prev = nouveau;
    resultat
}

/// Mesure les threads de StreamOS, agrégés par label du registre. `prev` est
/// REMPLACÉ par les valeurs fraîches.
#[cfg(windows)]
fn mesurer_threads(prev: &mut HashMap<u32, u64>, ecoule_ms: f64) -> Vec<ThreadMesure> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;

    let pid_soi = unsafe { GetCurrentProcessId() };
    let registre = registre_threads_slot().lock().unwrap().clone();
    let mut buckets: HashMap<String, f64> = HashMap::new();
    let mut nouveau: HashMap<u32, u64> = HashMap::new();

    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }) else {
        return Vec::new();
    };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    if unsafe { Thread32First(snap, &mut entry) }.is_ok() {
        loop {
            if entry.th32OwnerProcessID == pid_soi {
                let tid = entry.th32ThreadID;
                if let Some(t) = temps_cpu_thread(tid) {
                    let delta = t.saturating_sub(*prev.get(&tid).unwrap_or(&t));
                    if delta > 0 {
                        let label = registre
                            .get(&tid)
                            .cloned()
                            .unwrap_or_else(|| LABEL_AUTRES.to_string());
                        *buckets.entry(label).or_insert(0.0) += delta as f64;
                    }
                    nouveau.insert(tid, t);
                }
            }
            if unsafe { Thread32Next(snap, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snap) };
    *prev = nouveau;

    let mut mesures: Vec<ThreadMesure> = buckets
        .into_iter()
        .map(|(nom, delta_ms)| ThreadMesure {
            nom,
            cpu_pct: (delta_ms / ecoule_ms) * 100.0,
        })
        .collect();
    // Trier par CPU décroissant (lignes chaudes en tête — lecture immédiate).
    mesures.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
    mesures
}

/// Ligne « Système » : CPU total + RAM physique utilisée. `prev` garde
/// (idle, total) du tick précédent.
#[cfg(windows)]
fn mesurer_systeme(prev: &mut Option<(u64, u64)>) -> Option<ProcessusMesure> {
    let (idle, total) = temps_systeme_bruts()?;
    let Some((prev_idle, prev_total)) = *prev else {
        *prev = Some((idle, total));
        return None; // Premier tick : baseline seulement.
    };
    *prev = Some((idle, total));
    let d_idle = idle.saturating_sub(prev_idle);
    let d_total = total.saturating_sub(prev_total);
    if d_total == 0 {
        return None;
    }
    let busy = 1.0 - (d_idle as f64 / d_total as f64);
    Some(ProcessusMesure {
        nom: "Système (total)".to_string(),
        cpu_pct: (busy * 100.0).clamp(0.0, 100.0),
        ram_mo: ram_systeme_mo(),
        // Rempli par la boucle (somme des moteurs GPU de tous les processus).
        gpu_pct: None,
    })
}

// =============================================================================
// GPU PAR PROCESSUS — compteurs PDH « GPU Engine » (vendor-agnostic WDDM)
// =============================================================================
//
// Principe : Windows expose pour CHAQUE moteur GPU de CHAQUE processus un
// compteur « \GPU Engine(<instance>)\Utilization Percentage » où l'instance
// s'appelle « pid_<PID>_luid_..._eng_<n>_engtype_<3D|Copy|VideoDecode|...> ».
// La somme des moteurs d'un PID ≈ l'utilisation GPU du processus (même
// lecture que le Gestionnaire des tâches). Fonctionne sur Intel/AMD/NVIDIA/
// ARM (WDDM 2.x, Win10 1709+), AUCUN chemin spécifique constructeur.
// Si la requête échoue (VM sans GPU WDDM, vieux Windows, compteur désactivé)
// → GpuPdh absent → gpu_pct null → « N/A » affiché, jamais de crash.

/// Requête PDH ouverte (query + counter). Handles = isize bruts (crate
/// windows 0.58). Vit dans le thread échantillonneur ; Drop ferme la query.
#[cfg(windows)]
struct GpuPdh {
    query: isize,
    counter: isize,
}

#[cfg(windows)]
impl Drop for GpuPdh {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::System::Performance::PdhCloseQuery(self.query) };
    }
}

/// Ouvre la requête « \GPU Engine(*)\Utilization Percentage ». Le chemin
/// ANGLAIS est accepté quelle que soit la langue de Windows
/// (PdhAddEnglishCounterW). None = GPU non mesurable sur cette machine.
#[cfg(windows)]
fn gpu_pdh_ouvrir() -> Option<GpuPdh> {
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhOpenQueryW,
    };
    let mut query: isize = 0;
    if unsafe { PdhOpenQueryW(windows::core::PCWSTR::null(), 0, &mut query) } != 0 {
        return None;
    }
    let mut counter: isize = 0;
    if unsafe {
        PdhAddEnglishCounterW(
            query,
            windows::core::w!("\\GPU Engine(*)\\Utilization Percentage"),
            0,
            &mut counter,
        )
    } != 0
    {
        let _ = unsafe { PdhCloseQuery(query) };
        return None;
    }
    Some(GpuPdh { query, counter })
}

/// Collecte les compteurs → map PID → % GPU (somme des moteurs du processus).
/// Un appel par tick : 1 PdhCollectQueryData + 1 lecture formatée, coût
/// négligeable. Map vide si la collecte échoue (tick sautée, pas de crash).
#[cfg(windows)]
fn gpu_mesurer(gpu: &mut GpuPdh) -> HashMap<u32, f64> {
    use windows::Win32::System::Performance::{
        PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_DOUBLE, PDH_MORE_DATA,
    };
    let mut out: HashMap<u32, f64> = HashMap::new();
    if unsafe { PdhCollectQueryData(gpu.query) } != 0 {
        return out;
    }
    // Dimensionnement : 1er appel → taille requise (PDH_MORE_DATA attendu),
    // 2e appel → données. Le buffer est un Vec d'items typés (alignement OK).
    let mut taille = 0u32;
    let mut nb = 0u32;
    let r = unsafe {
        PdhGetFormattedCounterArrayW(gpu.counter, PDH_FMT_DOUBLE, &mut taille, &mut nb, None)
    };
    if r != PDH_MORE_DATA || taille == 0 {
        return out;
    }
    let capacite =
        (taille as usize).div_ceil(std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>()) + 1;
    let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = Vec::with_capacity(capacite);
    let r = unsafe {
        PdhGetFormattedCounterArrayW(
            gpu.counter,
            PDH_FMT_DOUBLE,
            &mut taille,
            &mut nb,
            Some(buf.as_mut_ptr()),
        )
    };
    if r != 0 {
        return out;
    }
    let items = unsafe { std::slice::from_raw_parts(buf.as_ptr(), nb as usize) };
    for it in items {
        if it.szName.is_null() {
            continue;
        }
        let nom = unsafe { windows::core::PCWSTR(it.szName.0 as *const u16).to_string() }
            .unwrap_or_default();
        // Instance « pid_1234_luid_..._eng_0_engtype_3D » → extraire le PID.
        let pid = nom
            .strip_prefix("pid_")
            .and_then(|s| s.split('_').next())
            .and_then(|s| s.parse::<u32>().ok());
        if let Some(pid) = pid {
            let v = unsafe { it.FmtValue.Anonymous.doubleValue };
            if v.is_finite() && v > 0.0 {
                *out.entry(pid).or_insert(0.0) += v;
            }
        }
    }
    out
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Les deux tests touchent les mêmes compteurs globaux → sérialisation
    /// (sinon race : l'autre test incrémente entre la lecture et l'assert).
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static L: OnceLock<Mutex<()>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    /// Le tap doit classer les frames connues sans planter sur du JSON inconnu.
    #[test]
    fn test_compter_frame_types() {
        let _g = test_lock();
        let avant_chat = MSGS_CHAT.load(Ordering::Relaxed);
        let avant_iv = PUBS_INPUT_VIEWER.load(Ordering::Relaxed);
        let avant_sr = EVENTS_SPEEDRUN.load(Ordering::Relaxed);
        compter_frame(r#"{"type":"chat","message":{"pseudo":"x"}}"#);
        compter_frame(r#"{"type":"input-viewer-etat","etat":{}}"#);
        compter_frame(r#"{"type":"speedrun-etat","etat":{"type":"time"}}"#);
        compter_frame(r#"{"type":"snapshot","scene":{}}"#);
        assert_eq!(MSGS_CHAT.load(Ordering::Relaxed), avant_chat + 1);
        assert_eq!(PUBS_INPUT_VIEWER.load(Ordering::Relaxed), avant_iv + 1);
        assert_eq!(EVENTS_SPEEDRUN.load(Ordering::Relaxed), avant_sr + 1);
    }

    /// L'event speedrun « connected » doit renseigner le nom du processus jeu.
    #[test]
    fn test_compter_frame_jeu_connecte() {
        let _g = test_lock();
        compter_frame(
            r#"{"type":"speedrun-etat","etat":{"type":"connected","process":"mgsi.exe","pid":123}}"#,
        );
        assert_eq!(
            processus_jeu_slot().lock().unwrap().as_deref(),
            Some("mgsi.exe")
        );
        compter_frame(r#"{"type":"speedrun-etat","etat":{"type":"disconnected"}}"#);
        assert!(processus_jeu_slot().lock().unwrap().is_none());
    }

    /// Gating de la boucle : jamais demandé → false ; demande de 10 s →
    /// false (modale fermée) ; poll courant → true. Remis à 0 en fin de
    /// test pour ne pas activer la boucle des autres tests.
    #[test]
    fn test_demande_recente() {
        let _g = test_lock();
        DEMANDE_MS.store(0, Ordering::Relaxed);
        assert!(!demande_recente());
        DEMANDE_MS.store(
            maintenant_epoch_ms().saturating_sub(10_000),
            Ordering::Relaxed,
        );
        assert!(!demande_recente());
        DEMANDE_MS.store(maintenant_epoch_ms(), Ordering::Relaxed);
        assert!(demande_recente());
        DEMANDE_MS.store(0, Ordering::Relaxed);
    }
}
