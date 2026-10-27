/// Musique — "!rs <titre>" tapé par un viewer → lecture d'un fichier de la
/// bibliothèque LOCALE du streamer côté diffusion + file d'attente.
///
/// Zéro service externe : le catalogue = les fichiers audio importés par le
/// streamer dans `medias/musique/` (servis par :4321 sur /medias/, comme les
/// sons d'alertes). `!rs <titre>` fait un match normalisé (casse/accents
/// ignorés, tous les mots de la requête présents dans le nom du fichier).
///
/// Différences avec commandes.rs : les pistes durent des minutes → la file
/// n'est PAS bornée à 5 ni libérée par un timer ; l'avancement est piloté
/// par la diffusion (`musique-ended` remontée via WS quand l'audio se
/// termine ou erre) + par les contrôles dashboard/chat (skip/stop).
///
/// Flux :
///   1. Module chat appelle `on_message(plateforme, pseudo, texte, est_mod)`
///      à côté du hook commandes (non-fatal si état absent). Retourne
///      Option<String> = accusé à poster dans le chat (Twitch seul a un
///      canal d'envoi — les autres plateformes ignorent la valeur).
///   2. Match → cooldown viewer + max_par_viewer + file_max → file/direct.
///   3. WS chat_tx : `musique-play` / `musique-stop` / `musique-pause` /
///      `musique-volume` → diffusion.html (audio + ligne défilante dans
///      l'en-tête du widget chat, sous la barre viewers — visible aussi en
///      mode bulles).
///   4. Fin de piste : diffusion envoie `musique-ended {id}` en upstream WS
///      → `on_ended` avance la file (id vérifié — dedup multi-clients).
///
/// Commandes chat (noms configurables, défauts) :
///   !rs <titre>  — demander (tout viewer, cooldown + limite par viewer)
///   !skip        — passer la piste courante (mod/broadcaster uniquement)
///   !retirer     — retirer SA dernière demande en file (tout viewer)
///   !file        — afficher en cours + les 3 suivantes (tout viewer)
///
/// Persistance : `data/musique_config.json` (config seule — la bibliothèque
/// = scan du dossier medias/musique, source de vérité = le filesystem).
/// Cooldowns en RAM (clé = pseudo — ChatMessage n'a pas de user_id).
use crate::config::data_dir;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

/// Extensions audio acceptées pour la bibliothèque (formats joués par
/// <audio> Chromium : mp3/ogg/wav sûrs, m4a/flac supportés par CEF/OBS).
const MUSIQUE_EXT: &[&str] = &["mp3", "ogg", "wav", "m4a", "flac"];

/// Taille max d'un fichier musique (10 Mo des alertes est trop juste :
/// ~14 Mo pour 6 min en mp3 320k). 50 Mo couvre même un flac long.
const MAX_MUSIQUE_BYTES: u64 = 50 * 1024 * 1024;

// ===== Config =====

/// Configuration du module musique (persistée `musique_config.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusiqueConfig {
    /// false = !rs ignoré silencieusement (feature invisible).
    #[serde(default = "default_true")]
    pub actif: bool,
    /// Mot de commande SANS le "!" (ex: "rs"). Normalisé lowercase.
    #[serde(default = "default_commande")]
    pub commande: String,
    /// Commande skip (mods/broadcaster). Défaut "skip".
    #[serde(default = "default_skip")]
    pub commande_skip: String,
    /// Commande "retire ma demande". Défaut "retirer".
    #[serde(default = "default_retirer")]
    pub commande_retirer: String,
    /// Commande "affiche la file". Défaut "file".
    #[serde(default = "default_file")]
    pub commande_file: String,
    /// Cooldown par viewer entre deux demandes (secondes, 0 = off).
    #[serde(default = "default_cooldown")]
    pub cooldown_viewer_s: u64,
    /// Nb max de pistes d'un même viewer dans la file + en cours.
    #[serde(default = "default_max_par_viewer")]
    pub max_par_viewer: u32,
    /// Taille max de la file (au-delà → demande refusée, pas d'éviction —
    /// contrairement aux alertes, perdre une piste silencieusement est pire).
    #[serde(default = "default_file_max")]
    pub file_max: usize,
    /// Volume de lecture côté diffusion (0.0–1.0).
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// false = aucun accusé chat (même sur Twitch qui peut répondre).
    #[serde(default = "default_true")]
    pub reponses_chat: bool,
}

fn default_true() -> bool {
    true
}
fn default_commande() -> String {
    "rs".to_string()
}
fn default_skip() -> String {
    "skip".to_string()
}
fn default_retirer() -> String {
    "retirer".to_string()
}
fn default_file() -> String {
    "file".to_string()
}
fn default_cooldown() -> u64 {
    60
}
fn default_max_par_viewer() -> u32 {
    3
}
fn default_file_max() -> usize {
    20
}
fn default_volume() -> f32 {
    0.8
}

impl Default for MusiqueConfig {
    fn default() -> Self {
        Self {
            actif: true,
            commande: default_commande(),
            commande_skip: default_skip(),
            commande_retirer: default_retirer(),
            commande_file: default_file(),
            cooldown_viewer_s: default_cooldown(),
            max_par_viewer: default_max_par_viewer(),
            file_max: default_file_max(),
            volume: default_volume(),
            reponses_chat: true,
        }
    }
}

/// Tous les noms de commandes musique (sans "!") — utilisés pour refuser les
/// collisions quand l'utilisateur crée une commande chat du même nom.
impl MusiqueConfig {
    pub fn noms_reserves(&self) -> Vec<String> {
        vec![
            self.commande.clone(),
            self.commande_skip.clone(),
            self.commande_retirer.clone(),
            self.commande_file.clone(),
        ]
    }
}

// ===== Item de file =====

/// Piste demandée (en cours ou en file).
#[derive(Debug, Clone, Serialize)]
pub struct MusiqueItem {
    pub id: String,
    /// Titre affiché (nom de fichier nettoyé, sans extension).
    pub titre: String,
    /// Chemin relatif servi par :4321 ("medias/musique/<fichier>").
    pub src: String,
    pub demandeur: String,
    pub plateforme: String,
}

/// Entrée de bibliothèque = un fichier de medias/musique.
#[derive(Debug, Clone, Serialize)]
pub struct MusiqueFichier {
    pub src: String,
    pub titre: String,
    pub taille: u64,
}

/// Piste en cours + timing de lecture (progression affichée par le lecteur
/// Winamp et le widget scène). `debut`/`pause_*` pilotent `position_ms` dans
/// etat() ; `duree_s` est remontée par la diffusion (`musique-meta` —
/// loadedmetadata de l'<audio>, seule source fiable de la durée).
#[derive(Debug, Clone)]
pub struct LectureEnCours {
    pub item: MusiqueItem,
    /// Démarrage de la piste (reset par reemit — la diffusion rejoue à 0).
    pub debut: SystemTime,
    /// true = en pause côté diffusion.
    pub pausee: bool,
    /// Instant de mise en pause courante (None si pas en pause).
    pub debut_pause: Option<SystemTime>,
    /// Temps cumulé en pause AVANT la pause courante.
    pub pause_accum: Duration,
    /// Durée de la piste en secondes (None tant que musique-meta pas reçu).
    pub duree_s: Option<f64>,
}

// ===== État partagé =====

/// État partagé musique. Clonable (Arc internes).
#[derive(Clone)]
pub struct MusiqueState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<MusiqueConfig>>,
    /// Piste en cours de lecture côté diffusion (+ timing de progression).
    en_cours: Arc<Mutex<Option<LectureEnCours>>>,
    /// File d'attente (prochaine en tête).
    file: Arc<Mutex<VecDeque<MusiqueItem>>>,
    /// Cooldowns par viewer : clé pseudo lowercase → dernière demande.
    cooldowns: Arc<Mutex<HashMap<String, Instant>>>,
}

impl MusiqueState {
    /// Crée l'état + charge la config depuis le disque.
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let cfg = load_config(&app).unwrap_or_default();
        Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(cfg)),
            en_cours: Arc::new(Mutex::new(None)),
            file: Arc::new(Mutex::new(VecDeque::new())),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // ===== Déclenchement (hook chat) =====

    /// Appelé par les modules chat à chaque message (à côté du hook
    /// commandes). `est_mod` = le pseudo est mod/broadcaster — détecté au
    /// call-site (badges selon la plateforme).
    /// Retourne un accusé à poster dans le chat (Twitch) ou None.
    pub fn on_message(
        &self,
        plateforme: &str,
        pseudo: &str,
        texte: &str,
        est_mod: bool,
    ) -> Option<String> {
        let trimmed = texte.trim();
        let rest = trimmed.strip_prefix('!')?;
        let mut mots = rest.split_whitespace();
        let premier = mots.next()?;
        let nom = premier.to_lowercase();
        let reste = rest[premier.len()..].trim().to_string();

        let cfg = self.config.lock().unwrap().clone();
        if !cfg.actif {
            return None;
        }

        if nom == cfg.commande {
            return self.demander(&cfg, plateforme, pseudo, &reste);
        }
        if nom == cfg.commande_skip {
            if !est_mod {
                return None; // skip réservé mods/broadcaster — silence pour les viewers
            }
            return match self.skip() {
                Some(titre) => self.reponse(format!("⏭ « {} » passé", titre)),
                None => self.reponse("⏭ Rien en cours".to_string()),
            };
        }
        if nom == cfg.commande_retirer {
            return match self.retirer_demande(pseudo) {
                Some(titre) => self.reponse(format!("✓ « {} » retiré de la file", titre)),
                None => self.reponse(format!("✗ {} : rien à retirer", pseudo)),
            };
        }
        if nom == cfg.commande_file {
            return Some(self.decrire_file());
        }
        None
    }

    /// Traite une demande `!rs <titre>` : cooldown → limites → match → play/file.
    /// Retourne l'accusé chat (None = silencieux si reponses_chat off).
    fn demander(
        &self,
        cfg: &MusiqueConfig,
        plateforme: &str,
        pseudo: &str,
        requete: &str,
    ) -> Option<String> {
        if requete.is_empty() {
            return self.reponse(format!("Usage : !{} <titre>", cfg.commande));
        }

        // Cooldown par viewer (clé = pseudo — pas de user_id dans ChatMessage).
        let now = Instant::now();
        if cfg.cooldown_viewer_s > 0 {
            let key = pseudo.to_lowercase();
            let mut cds = self.cooldowns.lock().unwrap();
            if let Some(last) = cds.get(&key) {
                if now.duration_since(*last).as_secs() < cfg.cooldown_viewer_s {
                    return self.reponse(format!("⏳ {} : cooldown", pseudo));
                }
            }
            cds.insert(key, now);
        }

        // Limite par viewer (en cours + file confondus).
        let nb_viewer = self.compter_demandes(pseudo);
        if cfg.max_par_viewer > 0 && nb_viewer >= cfg.max_par_viewer {
            return self.reponse(format!(
                "✗ {} : max {} demandes en file",
                pseudo, cfg.max_par_viewer
            ));
        }

        // Match bibliothèque (scan du dossier — source de vérité = filesystem).
        let Some(fichier) = self.trouver(requete) else {
            return self.reponse(format!("✗ Introuvable : « {} »", requete));
        };

        let item = MusiqueItem {
            id: uuid::Uuid::new_v4().simple().to_string(),
            titre: fichier.titre.clone(),
            src: fichier.src,
            demandeur: pseudo.to_string(),
            plateforme: plateforme.to_string(),
        };

        // En cours → file (bornée, refus explicite — pas d'éviction silencieuse).
        let position = {
            let en_cours = self.en_cours.lock().unwrap();
            if en_cours.is_some() {
                let mut file = self.file.lock().unwrap();
                if file.len() >= cfg.file_max {
                    return self.reponse(format!("✗ File pleine ({} max)", cfg.file_max));
                }
                file.push_back(item);
                file.len()
            } else {
                drop(en_cours);
                self.jouer(item);
                0
            }
        };

        if position == 0 {
            self.reponse(format!("▶ « {} » lancé pour {}", fichier.titre, pseudo))
        } else {
            self.reponse(format!(
                "✓ « {} » ajouté (position {})",
                fichier.titre, position
            ))
        }
    }

    /// Nombre de pistes demandées par ce pseudo (en cours + file).
    fn compter_demandes(&self, pseudo: &str) -> u32 {
        let p = pseudo.to_lowercase();
        let mut n = 0;
        {
            let en_cours = self.en_cours.lock().unwrap();
            if let Some(l) = en_cours.as_ref() {
                if l.item.demandeur.to_lowercase() == p {
                    n += 1;
                }
            }
        }
        let file = self.file.lock().unwrap();
        n += file
            .iter()
            .filter(|it| it.demandeur.to_lowercase() == p)
            .count() as u32;
        n
    }

    /// `!file` : décrit en cours + les 3 suivantes (accusé chat).
    fn decrire_file(&self) -> String {
        let en_cours = self.en_cours.lock().unwrap().clone();
        let file = self.file.lock().unwrap().clone();
        let courant = en_cours
            .as_ref()
            .map(|l| l.item.titre.clone())
            .unwrap_or_else(|| "rien".to_string());
        if file.is_empty() {
            return format!("▶ {} • file vide", courant);
        }
        let suivantes: Vec<String> = file.iter().take(3).map(|it| it.titre.clone()).collect();
        let reste = file.len().saturating_sub(3);
        let suffixe = if reste > 0 {
            format!(" (+{})", reste)
        } else {
            String::new()
        };
        format!("▶ {} • file : {}{}", courant, suivantes.join(", "), suffixe)
    }

    /// Retire la DERNIÈRE demande de ce pseudo encore en file.
    /// Retourne le titre retiré (None = rien de ce viewer en file).
    fn retirer_demande(&self, pseudo: &str) -> Option<String> {
        let p = pseudo.to_lowercase();
        let mut file = self.file.lock().unwrap();
        let pos = file
            .iter()
            .rposition(|it| it.demandeur.to_lowercase() == p)?;
        let retire = file.remove(pos);
        drop(file);
        self.emit_update();
        retire.map(|it| it.titre)
    }

    // ===== Lecture =====

    /// Lance une piste côté diffusion : en_cours = LectureEnCours (timing
    /// neuf) + WS musique-play + event dashboard.
    fn jouer(&self, item: MusiqueItem) {
        *self.en_cours.lock().unwrap() = Some(LectureEnCours {
            item: item.clone(),
            debut: SystemTime::now(),
            pausee: false,
            debut_pause: None,
            pause_accum: Duration::ZERO,
            duree_s: None,
        });
        let volume = self.config.lock().unwrap().volume;
        let msg = serde_json::json!({
            "type": "musique-play",
            "piste": item,
            "volume": volume,
        });
        let _ = self.chat_tx.send(msg.to_string());
        self.emit_update();
    }

    /// La diffusion signale la fin (ended/error) de la piste `id`.
    /// Avance la file si c'est bien la piste courante (dedup : plusieurs
    /// clients WS peuvent remonter le même ended).
    pub fn on_ended(&self, id: &str) {
        let est_courant = {
            let en_cours = self.en_cours.lock().unwrap();
            en_cours
                .as_ref()
                .map(|l| l.item.id == id)
                .unwrap_or(false)
        };
        if est_courant {
            self.avancer();
        }
    }

    /// Passe à la piste suivante (en_cours = pop_front → jouer, sinon stop).
    fn avancer(&self) {
        let next = self.file.lock().unwrap().pop_front();
        match next {
            Some(it) => self.jouer(it),
            None => {
                *self.en_cours.lock().unwrap() = None;
                // Efface la ligne défilante côté diffusion.
                let msg = serde_json::json!({ "type": "musique-stop" });
                let _ = self.chat_tx.send(msg.to_string());
                self.emit_update();
            }
        }
    }

    /// Skip la piste courante → stop WS + suivante. Retourne le titre sauté.
    pub fn skip(&self) -> Option<String> {
        let titre = self
            .en_cours
            .lock()
            .unwrap()
            .as_ref()
            .map(|l| l.item.titre.clone());
        if titre.is_some() {
            let msg = serde_json::json!({ "type": "musique-stop" });
            let _ = self.chat_tx.send(msg.to_string());
            self.avancer();
        }
        titre
    }

    /// Pause/reprendre la piste courante côté diffusion. Met à jour le
    /// timing de LectureEnCours (pause_accum accumule les pauses passées)
    /// puis émet l'état — le lecteur affiche pause/reprise en direct.
    pub fn pause(&self, pause: bool) {
        {
            let mut en_cours = self.en_cours.lock().unwrap();
            if let Some(l) = en_cours.as_mut() {
                if pause && !l.pausee {
                    l.debut_pause = Some(SystemTime::now());
                    l.pausee = true;
                } else if !pause && l.pausee {
                    if let Some(dp) = l.debut_pause.take() {
                        if let Ok(d) = SystemTime::now().duration_since(dp) {
                            l.pause_accum += d;
                        }
                    }
                    l.pausee = false;
                }
            }
        }
        let msg = serde_json::json!({ "type": "musique-pause", "pause": pause });
        let _ = self.chat_tx.send(msg.to_string());
        self.emit_update();
    }

    /// Stoppe tout : piste courante + vide la file.
    pub fn stop_tout(&self) {
        self.file.lock().unwrap().clear();
        *self.en_cours.lock().unwrap() = None;
        let msg = serde_json::json!({ "type": "musique-stop" });
        let _ = self.chat_tx.send(msg.to_string());
        self.emit_update();
    }

    /// Vide la file d'attente (la piste courante continue).
    pub fn vider_file(&self) {
        self.file.lock().unwrap().clear();
        self.emit_update();
    }

    /// Change le volume (config persistée + appliqué à chaud côté diffusion).
    pub fn set_volume(&self, v: f32) -> Result<(), String> {
        let v = v.clamp(0.0, 1.0);
        {
            let mut cfg = self.config.lock().unwrap();
            cfg.volume = v;
        }
        self.save_config()?;
        let msg = serde_json::json!({ "type": "musique-volume", "volume": v });
        let _ = self.chat_tx.send(msg.to_string());
        self.emit_update();
        Ok(())
    }

    /// Retire une entrée de la file par id (contrôle dashboard).
    pub fn retirer_file(&self, id: &str) -> Result<(), String> {
        let mut file = self.file.lock().unwrap();
        let avant = file.len();
        file.retain(|it| it.id != id);
        if file.len() == avant {
            return Err("Demande introuvable dans la file".into());
        }
        drop(file);
        self.emit_update();
        Ok(())
    }

    /// Déplace une entrée de la file (+1 = descendre, -1 = monter).
    pub fn deplacer_file(&self, id: &str, delta: isize) -> Result<(), String> {
        let mut file = self.file.lock().unwrap();
        let len = file.len();
        let Some(pos) = file.iter().position(|it| it.id == id) else {
            return Err("Demande introuvable dans la file".into());
        };
        let cible = pos as isize + delta;
        if cible < 0 || cible >= len as isize {
            return Ok(()); // bord de file — no-op
        }
        file.swap(pos, cible as usize);
        drop(file);
        self.emit_update();
        Ok(())
    }

    /// Ajoute des fichiers de la bibliothèque à la file (contrôle dashboard
    /// « Lire la sélection »). Chemin STREAMER : pas de cooldown, pas de
    /// max_par_viewer, pas de file_max (règles viewer — le streamer n'y est
    /// pas soumis). Ajoute en FIN de file (les demandes !rs déjà en file
    /// passent d'abord) ; si rien ne joue, la 1re piste démarre immédiatement
    /// et le reste s'enchaîne via l'auto-avance existant (musique-ended).
    /// Retourne le nombre de pistes ajoutées (srcs invalides ignorés).
    pub fn ajouter_file(&self, srcs: &[String]) -> Result<usize, String> {
        let biblio = self.bibliotheque();
        let mut items: Vec<MusiqueItem> = srcs
            .iter()
            .filter_map(|src| {
                let f = biblio.iter().find(|f| f.src == *src)?;
                Some(MusiqueItem {
                    id: uuid::Uuid::new_v4().simple().to_string(),
                    titre: f.titre.clone(),
                    src: f.src.clone(),
                    demandeur: "streamer".to_string(),
                    plateforme: "local".to_string(),
                })
            })
            .collect();
        if items.is_empty() {
            return Err("Aucun fichier valide".into());
        }
        let nb = items.len();

        // Rien en cours → la 1re piste part direct, le reste en file.
        // (Guard en_cours lâché avant jouer() — règle non-réentrance.)
        let premiere = {
            let en_cours = self.en_cours.lock().unwrap();
            (en_cours.is_none()).then(|| items.remove(0))
        };
        {
            let mut file = self.file.lock().unwrap();
            for it in items {
                file.push_back(it);
            }
        }
        if let Some(it) = premiere {
            self.jouer(it); // emit_update inclus
        } else {
            self.emit_update();
        }
        Ok(nb)
    }

    /// Test manuel : joue immédiatement un fichier de la bibliothèque sur la
    /// diffusion (remplace la piste courante — c'est un test, pas une demande).
    pub fn tester(&self, src: &str) -> Result<(), String> {
        let biblio = self.bibliotheque();
        let Some(f) = biblio.iter().find(|f| f.src == src) else {
            return Err(format!("Fichier introuvable : {}", src));
        };
        let item = MusiqueItem {
            id: uuid::Uuid::new_v4().simple().to_string(),
            titre: f.titre.clone(),
            src: f.src.clone(),
            demandeur: "test".to_string(),
            plateforme: "local".to_string(),
        };
        self.jouer(item);
        Ok(())
    }

    /// Ré-émet la frame play courante (resync demandé par la diffusion après
    /// reconnexion WS — ex. reload du browser source OBS en cours de piste).
    /// La piste repart à 0 côté diffusion → le timing est reset (duree_s
    /// conservée : même fichier, le lecteur garde la durée sans attendre le
    /// prochain loadedmetadata).
    pub fn reemit(&self) {
        let courant = {
            let mut en_cours = self.en_cours.lock().unwrap();
            if let Some(l) = en_cours.as_mut() {
                l.debut = SystemTime::now();
                l.debut_pause = None;
                l.pause_accum = Duration::ZERO;
                l.pausee = false;
            }
            en_cours.clone()
        };
        if let Some(l) = courant {
            let volume = self.config.lock().unwrap().volume;
            let msg = serde_json::json!({
                "type": "musique-play",
                "piste": l.item,
                "volume": volume,
            });
            let _ = self.chat_tx.send(msg.to_string());
            self.emit_update();
        }
    }

    /// La diffusion remonte la durée de la piste `id` (loadedmetadata de
    /// l'<audio> — seule source fiable). Posée sur LectureEnCours.duree_s si
    /// c'est bien la piste courante, puis emit (le lecteur affiche le total).
    pub fn on_meta(&self, id: &str, duree_s: f64) {
        if !duree_s.is_finite() || duree_s <= 0.0 {
            return;
        }
        let touche = {
            let mut en_cours = self.en_cours.lock().unwrap();
            match en_cours.as_mut() {
                Some(l) if l.item.id == id => {
                    l.duree_s = Some(duree_s);
                    true
                }
                _ => false,
            }
        };
        if touche {
            self.emit_update();
        }
    }

    /// Accusé chat selon config (reponses_chat off → None = silencieux).
    fn reponse(&self, texte: String) -> Option<String> {
        if self.config.lock().unwrap().reponses_chat {
            Some(texte)
        } else {
            None
        }
    }

    // ===== Bibliothèque (medias/musique — source de vérité = filesystem) =====

    /// Liste les fichiers de la bibliothèque (scan du dossier, trié par nom).
    pub fn bibliotheque(&self) -> Vec<MusiqueFichier> {
        let Ok(dir) = musique_dir(&self.app) else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out: Vec<MusiqueFichier> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                if !path.is_file() {
                    return None;
                }
                let ext = path
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|s| s.to_lowercase())?;
                if !MUSIQUE_EXT.contains(&ext.as_str()) {
                    return None;
                }
                let nom = path.file_name()?.to_str()?.to_string();
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&nom);
                let taille = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                Some(MusiqueFichier {
                    src: format!("medias/musique/{}", nom),
                    titre: nettoyer_titre(stem),
                    taille,
                })
            })
            .collect();
        out.sort_by_key(|f| f.titre.to_lowercase());
        out
    }

    /// Match normalisé : tous les mots de la requête doivent apparaître dans
    /// le nom normalisé du fichier ; meilleur candidat = nom le plus court
    /// (le plus spécifique, déterministe).
    fn trouver(&self, requete: &str) -> Option<MusiqueFichier> {
        let tokens: Vec<String> = normaliser(requete)
            .split_whitespace()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if tokens.is_empty() {
            return None;
        }
        self.bibliotheque()
            .into_iter()
            .filter(|f| {
                let nom = normaliser(&f.titre);
                tokens.iter().all(|t| nom.contains(t.as_str()))
            })
            .min_by_key(|f| normaliser(&f.titre).len())
    }

    // ===== État / config =====

    /// Config courante (lecture seule) — utilisée par `!cmd` pour lister
    /// les commandes viewer actives.
    pub fn config(&self) -> MusiqueConfig {
        self.config.lock().unwrap().clone()
    }

    /// État complet pour l'UI (config + en_cours + file + bibliothèque +
    /// progression + cooldowns actifs). `en_cours` reste sérialisé en
    /// MusiqueItem (contrat MusiqueTab/dashboard inchangé) ; le timing vit
    /// dans `progression` (position figée à l'emit — le lecteur l'avance en
    /// tick local quand pausee=false).
    pub fn etat(&self) -> serde_json::Value {
        // Chaque mutex lu UNE fois (règle non-réentrance — voir AGENTS.md).
        let cfg = self.config.lock().unwrap().clone();
        let en_cours = self.en_cours.lock().unwrap().clone();
        let file: Vec<MusiqueItem> = self.file.lock().unwrap().iter().cloned().collect();
        let cooldowns = self.cooldowns.lock().unwrap().clone();

        let now = SystemTime::now();
        let (item, progression) = match &en_cours {
            Some(l) => {
                // Position figée : fin = début de la pause courante si en
                // pause, sinon maintenant. saturating_sub = garde horloge.
                let fin = l.debut_pause.unwrap_or(now);
                let pos = fin
                    .duration_since(l.debut)
                    .unwrap_or_default()
                    .saturating_sub(l.pause_accum);
                (
                    Some(l.item.clone()),
                    serde_json::json!({
                        "position_ms": pos.as_millis() as u64,
                        "pausee": l.pausee,
                        "duree_s": l.duree_s,
                    }),
                )
            }
            None => (None, serde_json::Value::Null),
        };

        let mut cooldowns_actifs: Vec<serde_json::Value> = cooldowns
            .iter()
            .filter_map(|(pseudo, last)| {
                let restant = cfg
                    .cooldown_viewer_s
                    .saturating_sub(last.elapsed().as_secs());
                (restant > 0).then(|| {
                    serde_json::json!({ "pseudo": pseudo, "restant_s": restant })
                })
            })
            .collect();
        cooldowns_actifs.sort_by_key(|c| {
            -(c.get("restant_s").and_then(|v| v.as_u64()).unwrap_or(0) as i64)
        });

        serde_json::json!({
            "config": cfg,
            "en_cours": item,
            "file": file,
            "bibliotheque": self.bibliotheque(),
            "progression": progression,
            "cooldowns_actifs": cooldowns_actifs,
        })
    }

    /// Sauvegarde la config (noms normalisés : trim, sans "!", lowercase,
    /// non vides et mutuellement distincts) + persiste + emit dashboard.
    pub fn set_config(&self, mut cfg: MusiqueConfig) -> Result<MusiqueConfig, String> {
        for champ in [
            &mut cfg.commande,
            &mut cfg.commande_skip,
            &mut cfg.commande_retirer,
            &mut cfg.commande_file,
        ] {
            *champ = champ.trim().trim_start_matches('!').to_lowercase();
            if champ.is_empty() {
                return Err("Nom de commande vide".into());
            }
        }
        // Les 4 commandes doivent être distinctes (sinon !x déclencherait
        // deux actions — la 1ère gagne silencieusement dans on_message).
        let noms = cfg.noms_reserves();
        if noms.iter().any(|n| n == crate::commandes::CMD_LISTE) {
            return Err("« !cmd » est réservé à la liste des commandes".into());
        }
        let mut dedup = noms.clone();
        dedup.sort();
        dedup.dedup();
        if dedup.len() != noms.len() {
            return Err("Les noms de commandes doivent être distincts".into());
        }
        cfg.volume = cfg.volume.clamp(0.0, 1.0);
        *self.config.lock().unwrap() = cfg.clone();
        self.save_config()?;
        self.emit_update();
        Ok(cfg)
    }

    /// Noms de commandes musique réservés (sans "!") — utilisés par
    /// commandes.rs pour refuser les collisions avec les commandes chat.
    pub fn noms_reserves(&self) -> Vec<String> {
        self.config.lock().unwrap().noms_reserves()
    }

    /// Émet `musique:update` vers le dashboard (état complet — l'UI se
    /// contente de remplacer son store, pas de diff).
    pub fn emit_update(&self) {
        let _ = self.app.emit("musique:update", self.etat());
    }

    fn save_config(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize musique_config.json: {}", e))?;
        fs::write(dir.join("musique_config.json"), json)
            .map_err(|e| format!("Écrire musique_config.json: {}", e))?;
        Ok(())
    }
}

// ===== Import / dossier =====

/// Dossier de la bibliothèque : `data/medias/musique/` — servi par :4321 via
/// le ServeDir /medias existant (sous-dossier = zéro changement serveur).
pub fn musique_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(data_dir(app)?.join("medias").join("musique"))
}

/// Valide un fichier audio et le copie dans medias/musique en CONSERVANT un
/// nom lisible (le nom = le titre demandé par les viewers, contrairement aux
/// sons d'alertes renommés en uuid). Collision → suffixe "-2", "-3"…
/// Retourne l'entrée de bibliothèque créée.
pub fn valider_et_copier(app: &AppHandle, src: &std::path::Path) -> Result<MusiqueFichier, String> {
    use std::io::Read;

    // 1. Extension autorisée.
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| "Extension manquante".to_string())?;
    if !MUSIQUE_EXT.contains(&ext.as_str()) {
        return Err(format!("Extension .{} non autorisée", ext));
    }

    // 2. Taille ≤ limite.
    let meta = fs::metadata(src).map_err(|e| format!("metadata: {}", e))?;
    if meta.len() > MAX_MUSIQUE_BYTES {
        return Err(format!(
            "Fichier trop volumineux ({} octets > {} Mo)",
            meta.len(),
            MAX_MUSIQUE_BYTES / 1024 / 1024
        ));
    }

    // 3. Magic bytes (selon l'extension déclarée).
    let mut f = fs::File::open(src).map_err(|e| format!("open: {}", e))?;
    let mut head = [0u8; 32];
    let n = f.read(&mut head).map_err(|e| format!("read: {}", e))?;
    if !check_magic_musique(&head[..n], &ext) {
        return Err("Format audio non supporté (magic bytes invalides)".into());
    }

    // 4. Nom lisible + collision → suffixe -2/-3…
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("musique");
    let stem = sanitiser_nom(stem);
    let dir = musique_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| format!("mkdir musique: {}", e))?;
    let mut nom = format!("{}.{}", stem, ext);
    for i in 2..100 {
        if !dir.join(&nom).exists() {
            break;
        }
        nom = format!("{}-{}.{}", stem, i, ext);
    }
    fs::copy(src, dir.join(&nom)).map_err(|e| format!("copy: {}", e))?;

    Ok(MusiqueFichier {
        src: format!("medias/musique/{}", nom),
        titre: nettoyer_titre(&stem),
        taille: meta.len(),
    })
}

/// Supprime un fichier de la bibliothèque (par nom de fichier — jamais de
/// chemin, pour interdire toute traversée hors de medias/musique).
pub fn supprimer_fichier(app: &AppHandle, nom_fichier: &str) -> Result<(), String> {
    if nom_fichier.contains(['/', '\\']) || nom_fichier.contains("..") {
        return Err("Nom de fichier invalide".into());
    }
    let path = musique_dir(app)?.join(nom_fichier);
    if !path.exists() {
        return Err("Fichier introuvable".into());
    }
    fs::remove_file(&path).map_err(|e| format!("remove: {}", e))?;
    Ok(())
}

/// Rend le nom de fichier sûr : retire les séparateurs/caractères interdits
/// Windows, borne la longueur, "musique" si vide.
fn sanitiser_nom(stem: &str) -> String {
    let clean: String = stem
        .chars()
        .map(|c| match c {
            '/' | '\\' | '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>' => ' ',
            _ => c,
        })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean: String = clean.chars().take(100).collect();
    let clean = clean.trim().trim_end_matches('.').to_string();
    if clean.is_empty() {
        "musique".to_string()
    } else {
        clean
    }
}

/// Titre affiché = stem avec "_" → espace + espaces resserrés.
fn nettoyer_titre(stem: &str) -> String {
    stem.replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normalisation matching : lowercase + accents latins courants → ascii +
/// tout le reste non-alphanumérique → espace.
fn normaliser(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ÿ' => 'y',
            c if c.is_alphanumeric() => c.to_lowercase().next().unwrap_or(c),
            _ => ' ',
        })
        .collect()
}

/// Magic bytes par format : mp3 (ID3 ou frame sync 0xFF Ex), ogg (OggS),
/// wav (RIFF), flac (fLaC), m4a (ftyp à l'offset 4).
fn check_magic_musique(head: &[u8], ext: &str) -> bool {
    if head.is_empty() {
        return false;
    }
    match ext {
        "mp3" => {
            head.starts_with(b"ID3")
                || (head.len() >= 2 && head[0] == 0xFF && (head[1] & 0xE0) == 0xE0)
        }
        "ogg" => head.starts_with(b"OggS"),
        "wav" => head.starts_with(b"RIFF"),
        "flac" => head.starts_with(b"fLaC"),
        "m4a" => head.len() >= 8 && &head[4..8] == b"ftyp",
        _ => false,
    }
}

/// Charge musique_config.json (absent → défaut).
fn load_config(app: &AppHandle) -> Result<MusiqueConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("musique_config.json");
    if !path.exists() {
        return Ok(MusiqueConfig::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire musique_config.json: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Parse musique_config.json: {}", e))
}
