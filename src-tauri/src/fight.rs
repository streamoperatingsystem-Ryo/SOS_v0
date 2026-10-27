/// Emoji Arena — combat d'emojis 1v1 piloté par le chat ("!fight").
///
/// Un viewer tape "!fight" (défi ouvert), "!fight @pseudo" (défi ciblé) ou
/// "!fight bot" (contre l'ordinateur) → les deux emojis s'affichent en bas
/// de la diffusion (gauche/droite) avec barres de vie au-dessus. Tour par
/// tour : le joueur actif a `duree_tour_s` secondes pour taper "!a"/"!b"/"!c"
/// (rapide sûr / lourd risqué / spécial vol de vie à usage unique). Timer
/// Rust autoritaire : tour sauté au timeout, match capé, lobby expiré.
///
/// Flux :
///   1. Module chat appelle `on_message(plateforme, pseudo, texte, est_mod)`
///      à côté du hook musique (non-fatal si état absent). Retourne
///      Option<String> = accusé à poster dans le chat (Twitch seul a un
///      canal d'envoi — les autres plateformes ignorent la valeur).
///   2. Machine à états Idle → Lobby → Combat → Fin → Idle. Les timers
///      tokio sont gardés par un compteur de génération (`gen`) : toute
///      mutation de la partie l'incrémente → un timer en vol dont le gen
///      est périmé se suicide (pas de cancel tokens, un seq suffit).
///   3. WS chat_tx : frame `fight-state` = snapshot COMPLET à chaque
///      transition (resync-able — la diffusion n'est qu'un renderer).
///   4. Reload OBS : upstream `fight-resync` → `reemit()` repousse l'état.
///
/// Persistance : `data/fight_config.json` (config seule — un combat en
/// cours ne survit pas au redémarrage).
/// Identité joueur : clé "plateforme:pseudo_lower" (ChatMessage n'a pas
/// de user_id) — combat multi-plateforme possible (Twitch vs Kick…).
use crate::config::data_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

// ===== Équilibrage (constantes — pas de config utilisateur) =====

/// Coup rapide : dégâts faibles quasi sûrs.
const RAPIDE_DEG_MIN: u32 = 8;
const RAPIDE_DEG_MAX: u32 = 12;
const RAPIDE_HIT_PCT: u32 = 90;
/// Coup lourd : gros dégâts, risqué.
const LOURD_DEG_MIN: u32 = 18;
const LOURD_DEG_MAX: u32 = 26;
const LOURD_HIT_PCT: u32 = 55;
/// Coup spécial : vol de vie (dégâts + soin), 1 seul usage par combat.
const SPECIAL_DEG: u32 = 10;
const SPECIAL_SOIN: u32 = 10;
const SPECIAL_HIT_PCT: u32 = 85;
/// Chance de critique (dégâts ×2) — rapide et lourd seulement.
const CRIT_PCT: u32 = 10;
/// Durée d'affichage de la bannière de fin avant retour à Idle.
const FIN_AFFICHAGE_MS: u64 = 6000;
/// Délais simulés du bot (humainement crédible : pas de réponse instantanée).
const BOT_DELAI_MIN_MS: u64 = 1500;
const BOT_DELAI_MAX_MS: u64 = 3000;
/// Seuil PV sous lequel le bot envisage le spécial (vol de vie).
const BOT_SEUIL_SPECIAL_PCT: u32 = 50;
/// Passes consécutifs par joueur avant la fin AFK (match mort).
const PASSES_AFK: u32 = 3;
/// Durée d'affichage d'un toast d'information (ms).
const INFO_DUREE_MS: u64 = 2600;
/// Durée d'affichage de la carte d'aide (ms).
const AIDE_DUREE_MS: u64 = 12000;
/// Anti-spam : même commande d'un même viewer (s).
const ANTI_SPAM_S: u64 = 2;
/// Anti-spam des réponses de statut (s).
const STATUT_SPAM_S: u64 = 10;
/// Cooldown de l'aide/liste (s).
const AIDE_COOLDOWN_S: u64 = 30;
/// Longueur max d'un nom de commande (affichage overlay + chat).
const COMMANDE_MAX_CHARS: usize = 20;
/// Nombre max d'emojis mémorisés par viewer (session).
const EMOJIS_MEMO_MAX: usize = 500;

const EMOJI_BOT: &str = "🤖";
/// Pool d'emojis tirés quand le joueur n'en choisit pas un dans sa commande.
const EMOJIS_POOL: &[&str] = &[
    "🦊", "🐯", "🐸", "🐵", "🐲", "👹", "👺", "🐙", "🦁", "🐺", "🦄", "🐷", "🐼", "🐨", "🦖",
    "💀", "🥷", "🐔", "🐧", "🦝",
];

// ===== Config =====

/// Configuration du module Emoji Arena (persistée `fight_config.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FightConfig {
    /// false = toutes les commandes ignorées silencieusement.
    #[serde(default)]
    pub actif: bool,
    /// "!fight [@pseudo|bot] [emoji]" — défi ouvert / ciblé / vs CPU.
    #[serde(default = "def_fight")]
    pub commande_fight: String,
    /// Accepter un défi (ouvert : tout le monde sauf le défiant ; ciblé :
    /// uniquement le pseudo visé — "!accept [emoji]" possible).
    #[serde(default = "def_accept")]
    pub commande_accept: String,
    /// Refuser un défi ciblé (le défié seulement).
    #[serde(default = "def_refuse")]
    pub commande_refuse: String,
    /// Coups (tour actif seulement) : rapide / lourd / spécial.
    #[serde(default = "def_a")]
    pub commande_a: String,
    #[serde(default = "def_b")]
    pub commande_b: String,
    #[serde(default = "def_c")]
    pub commande_c: String,
    /// Abandonner le combat en cours (ou annuler son lobby).
    #[serde(default = "def_ff")]
    pub commande_ff: String,
    /// Arrêt forcé par mods/broadcaster.
    #[serde(default = "def_stop")]
    pub commande_stop: String,
    /// Points de vie de départ des deux combattants.
    #[serde(default = "def_vie")]
    pub vie_max: u32,
    /// Durée d'attente d'un adversaire avant expiration du défi (s).
    #[serde(default = "def_lobby")]
    pub duree_lobby_s: u64,
    /// Temps alloué par tour ; timeout = tour sauté (anti-AFK).
    #[serde(default = "def_tour")]
    pub duree_tour_s: u64,
    /// Durée max du match (s) ; à l'échéance le plus haut en PV gagne.
    #[serde(default = "def_match")]
    pub duree_match_s: u64,
    /// Cooldown par viewer entre deux combats (s, 0 = off) — appliqué aux
    /// deux participants à la fin du combat (gagnant ET perdant).
    #[serde(default)]
    pub cooldown_combat_s: u64,
    /// false = aucun accusé chat (même sur Twitch qui peut répondre).
    #[serde(default = "def_true")]
    pub reponses_chat: bool,
    /// true = annonce chaque tour humain dans le chat Twitch.
    #[serde(default = "def_true")]
    pub annonce_tour: bool,
    /// true = rappel « 5 s » avant la fin du tour (Twitch).
    #[serde(default = "def_true")]
    pub rappel_tour: bool,
    /// true = FX réduits (impacts, secousses) côté diffusion.
    #[serde(default)]
    pub animations_reduites: bool,
}

fn def_true() -> bool {
    true
}
fn def_fight() -> String {
    "fight".to_string()
}
fn def_accept() -> String {
    "accept".to_string()
}
fn def_refuse() -> String {
    "refuse".to_string()
}
fn def_a() -> String {
    "a".to_string()
}
fn def_b() -> String {
    "b".to_string()
}
fn def_c() -> String {
    "c".to_string()
}
fn def_ff() -> String {
    "ff".to_string()
}
fn def_stop() -> String {
    "fightstop".to_string()
}
fn def_vie() -> u32 {
    100
}
fn def_lobby() -> u64 {
    30
}
fn def_tour() -> u64 {
    15
}
fn def_match() -> u64 {
    180
}

impl Default for FightConfig {
    fn default() -> Self {
        Self {
            actif: false, // opt-in : les commandes !a/!b/!c sont génériques
            commande_fight: def_fight(),
            commande_accept: def_accept(),
            commande_refuse: def_refuse(),
            commande_a: def_a(),
            commande_b: def_b(),
            commande_c: def_c(),
            commande_ff: def_ff(),
            commande_stop: def_stop(),
            vie_max: def_vie(),
            duree_lobby_s: def_lobby(),
            duree_tour_s: def_tour(),
            duree_match_s: def_match(),
            cooldown_combat_s: 120,
            reponses_chat: true,
            annonce_tour: true,
            rappel_tour: true,
            animations_reduites: false,
        }
    }
}

/// Tous les noms de commandes du module (sans "!") — utilisés pour refuser
/// les collisions quand l'utilisateur crée une commande chat du même nom.
impl FightConfig {
    pub fn noms_reserves(&self) -> Vec<String> {
        vec![
            self.commande_fight.clone(),
            self.commande_accept.clone(),
            self.commande_refuse.clone(),
            self.commande_a.clone(),
            self.commande_b.clone(),
            self.commande_c.clone(),
            self.commande_ff.clone(),
            self.commande_stop.clone(),
        ]
    }
}

// ===== État de partie =====

/// Combattant (viewer ou CPU).
#[derive(Debug, Clone)]
struct Joueur {
    /// "plateforme:pseudo_lower" — identité cross-plateforme.
    cle: String,
    /// Pseudo d'affichage (casse d'origine).
    pseudo: String,
    /// "twitch"|"youtube"|"kick"|"tiktok"|"bot"|"local"(test dashboard).
    plateforme: String,
    emoji: String,
    bot: bool,
}

/// Dernière action résolue — transportée dans la frame pour l'animation
/// côté diffusion (dedup par `seq` : les frames se répètent au resync).
#[derive(Debug, Clone)]
struct ActionCoup {
    seq: u64,
    /// 'a' | 'b'.
    acteur: char,
    /// "rapide"|"lourd"|"special"|"passe" (timeout = tour sauté).
    coup: String,
    degats: u32,
    soin: u32,
    /// false = le coup a raté (esquive visuelle côté diffusion).
    touche: bool,
    critique: bool,
}

/// Dernier message d'information poussé à la diffusion (toast ou carte
/// d'aide). Inclus dans chaque snapshot : le client déduplique par `seq`
/// et ignore un message trop vieux (`age_ms` > `duree_ms`).
#[derive(Debug, Clone)]
struct Info {
    seq: u64,
    /// "info" | "warn" | "ok" | "aide".
    kind: String,
    texte: String,
    duree_ms: u64,
    instant: Instant,
}

/// Résultat de `statut_si_actif` : Idle = on peut lancer un défi ;
/// Actif(Some) = réponse contextuelle ; Actif(None) = actif mais throttlé.
enum StatutArene {
    Idle,
    Actif(Option<String>),
}

/// Machine à états de la partie. Sérialisée manuellement dans la frame WS.
enum Phase {
    Idle,
    /// Défi en attente d'adversaire (ouvert : premier "!accept" ; ciblé :
    /// seul le pseudo visé). `fin_lobby` = deadline d'expiration.
    Lobby {
        defiant: Joueur,
        cible_lower: Option<String>,
        ouvert: bool,
        fin_lobby: Instant,
    },
    Combat {
        ja: Joueur,
        jb: Joueur,
        pv_a: u32,
        pv_b: u32,
        /// 'a' = challenger (gauche) | 'b' = adversaire (droite).
        tour: char,
        /// Spécial déjà consommé par ce joueur (usage unique par combat).
        special_a: bool,
        special_b: bool,
        /// Passes consécutifs (timeouts) — fin AFK après PASSES_AFK chacun.
        passes_a: u32,
        passes_b: u32,
        fin_tour: Instant,
        fin_match: Instant,
        derniere: Option<ActionCoup>,
    },
    /// État transitoire : bannière de fin affichée `FIN_AFFICHAGE_MS` puis
    /// retour Idle (le timer Fin porte la deadline — pas de champ dédié).
    Fin {
        ja: Joueur,
        jb: Joueur,
        pv_a: u32,
        pv_b: u32,
        /// 'a' | 'b' | None (égalité au temps / arrêt).
        gagnant: Option<char>,
        /// "ko"|"abandon"|"temps"|"stop".
        raison: String,
    },
}

/// Minuteurs identifiés par type — le réveil vérifie la génération (`gen`)
/// de la partie ; un gen périmé = l'état a changé entre-temps → no-op.
/// Exception : Match compare `fin_match` (la partie avance au fil des
/// actions — le gen change, mais la deadline match reste la même).
enum Minuteur {
    Lobby,
    Tour,
    Match(Instant),
    Fin,
    Bot,
    /// Rappel « 5 s » du tour humain (même garde de génération que Tour).
    RappelTour,
}

struct Inner {
    phase: Phase,
    /// Compteur de génération incrémenté à chaque mutation — invalide les
    /// timers lobby/tour/fin/bot en vol (le timer match a son propre garde).
    gen: u64,
    /// Séquence monotone des actions résolues — identifie `derniere` côté
    /// diffusion (dedup : la même frame peut être re-émise au resync sans
    /// rejouer l'animation).
    action_seq: u64,
    /// Cooldowns par joueur : clé "plateforme:pseudo_lower" → fin du dernier
    /// combat (gagnant et perdant — anti-rematch en boucle).
    cooldowns: HashMap<String, Instant>,
    /// Dernier message d'info (toast/carte) — inclus dans chaque snapshot.
    derniere_info: Option<Info>,
    /// Séquence monotone des messages d'info (dédup côté diffusion).
    info_seq: u64,
    /// Emoji mémorisé par viewer (session) — réutilisé sans argument.
    derniers_emojis: HashMap<String, String>,
    /// Anti-spam : clé "viewer|commande" → dernier envoi.
    derniere_cmd: HashMap<String, Instant>,
    /// Anti-spam global des réponses de statut.
    dernier_statut: Option<Instant>,
    /// Cooldown de l'aide/liste.
    derniere_aide: Option<Instant>,
    /// État du PRNG xorshift64 (zéro dépendance externe — pas de rand dans
    /// le Cargo.toml, un xorshift seedé à l'horloge suffit pour un jeu chat).
    rng: u64,
}

// ===== PRNG xorshift64 (u64 — pas de crate externe) =====

fn rng_suivant(s: &mut u64) -> u64 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *s = x;
    x
}

/// Entier uniforme dans [min, max] (bornes incluses ; max <= min → min).
fn rng_plage(s: &mut u64, min: u32, max: u32) -> u32 {
    if max <= min {
        return min;
    }
    min + (rng_suivant(s) % ((max - min + 1) as u64)) as u32
}

/// true avec probabilité pct/100.
fn rng_pct(s: &mut u64, pct: u32) -> bool {
    (rng_suivant(s) % 100) < pct as u64
}

// ===== État partagé =====

/// État partagé Emoji Arena. Clonable (Arc internes) — enregistré via
/// app.manage() et récupéré par les hooks chat via try_state.
#[derive(Clone)]
pub struct FightState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    config: Arc<Mutex<FightConfig>>,
    inner: Arc<Mutex<Inner>>,
}

fn cle_joueur(plateforme: &str, pseudo: &str) -> String {
    format!("{}:{}", plateforme, pseudo.to_lowercase())
}

impl FightState {
    /// Crée l'état + charge `fight_config.json` (absent → défaut).
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let config = load_file(&app).unwrap_or_default();
        // Seed PRNG : horloge (nanos ⊕ secs décalés) — non reproductible,
        // forcément impair pour éviter le point fixe 0 du xorshift.
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| (d.subsec_nanos() as u64) ^ (d.as_secs().wrapping_shl(1)))
            .unwrap_or(0x9e3779b97f4a7c15);
        Self {
            app,
            chat_tx,
            config: Arc::new(Mutex::new(config)),
            inner: Arc::new(Mutex::new(Inner {
                phase: Phase::Idle,
                gen: 0,
                action_seq: 0,
                cooldowns: HashMap::new(),
                derniere_info: None,
                info_seq: 0,
                derniers_emojis: HashMap::new(),
                derniere_cmd: HashMap::new(),
                dernier_statut: None,
                derniere_aide: None,
                rng: seed | 1,
            })),
        }
    }

    // ===== Déclenchement (hook chat) =====

    /// Appelé par les modules chat à chaque message (à côté du hook musique).
    /// `est_mod` = mod/broadcaster — détecté au call-site (badges selon la
    /// plateforme, mêmes règles que musique.rs).
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
        // Args = mots restants rejoints (résiste à "!  fight @x" — même
        // pattern que musique.rs, pas de slicing par len()).
        let reste = mots.collect::<Vec<_>>().join(" ");

        let cfg = self.config.lock().unwrap().clone();
        // Aide/liste disponibles MÊME quand l'arène est inactive : la
        // découverte des règles ne doit jamais dépendre de l'opt-in.
        if nom == cfg.commande_fight {
            let premier = reste
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_lowercase();
            if matches!(premier.as_str(), "aide" | "help" | "regles" | "règles") {
                return self.cmd_aide(&cfg);
            }
            if matches!(premier.as_str(), "liste" | "emojis") {
                return self.cmd_liste(&cfg);
            }
        }
        if !cfg.actif {
            return None;
        }

        // Anti-spam : même commande d'un même viewer à moins de ANTI_SPAM_S
        // est ignorée. Les coups sont exclus (le tour les cadence déjà).
        // Lock court : jamais pendant le dispatch (Mutex non réentrant).
        if nom != cfg.commande_a && nom != cfg.commande_b && nom != cfg.commande_c {
            let cle = cle_joueur(plateforme, pseudo);
            let mut inner = self.inner.lock().unwrap();
            let key = format!("{}|{}", cle, nom);
            if let Some(t) = inner.derniere_cmd.get(&key) {
                if t.elapsed() < Duration::from_secs(ANTI_SPAM_S) {
                    return None;
                }
            }
            if inner.derniere_cmd.len() >= 1000 {
                inner.derniere_cmd.clear();
            }
            inner.derniere_cmd.insert(key, Instant::now());
        }

        if nom == cfg.commande_fight {
            return self.cmd_fight(&cfg, plateforme, pseudo, &reste);
        }
        if nom == cfg.commande_accept {
            return self.cmd_accept(&cfg, plateforme, pseudo, &reste);
        }
        if nom == cfg.commande_refuse {
            return self.cmd_refuse(&cfg, pseudo);
        }
        if nom == cfg.commande_ff {
            return self.cmd_ff(&cfg, plateforme, pseudo);
        }
        if nom == cfg.commande_stop {
            // Réservé mods/broadcaster — silence pour les viewers.
            if !est_mod {
                return None;
            }
            return self.cmd_stop(&cfg);
        }
        if nom == cfg.commande_a || nom == cfg.commande_b || nom == cfg.commande_c {
            let coup = if nom == cfg.commande_a {
                "rapide"
            } else if nom == cfg.commande_b {
                "lourd"
            } else {
                "special"
            };
            return self.cmd_coup(&cfg, plateforme, pseudo, coup);
        }
        None
    }

    // ===== Commandes =====

    /// "!fight [@pseudo|bot] [emoji]" — défi ouvert / ciblé / vs CPU.
    /// Sous-commandes : "aide"/"help"/"regles", "liste"/"emojis".
    fn cmd_fight(
        &self,
        cfg: &FightConfig,
        plateforme: &str,
        pseudo: &str,
        reste: &str,
    ) -> Option<String> {
        let premier = reste
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        if matches!(premier.as_str(), "aide" | "help" | "regles" | "règles") {
            return self.cmd_aide(cfg);
        }
        if matches!(premier.as_str(), "liste" | "emojis") {
            return self.cmd_liste(cfg);
        }

        // Args : "@pseudo" → défi ciblé ; "bot" → CPU ; autre → emoji joueur.
        let mut cible: Option<String> = None;
        let mut bot = false;
        let mut emoji: Option<String> = None;
        for tok in reste.split_whitespace() {
            if let Some(t) = tok.strip_prefix('@') {
                cible = Some(t.to_lowercase());
            } else if tok.eq_ignore_ascii_case("bot") {
                bot = true;
            } else if emoji.is_none() {
                emoji = Some(tok.to_string());
            }
        }
        if bot {
            cible = None; // "!fight bot" = CPU, pas de lobby
        }
        if let Some(c) = &cible {
            if *c == pseudo.to_lowercase() {
                let texte = format!("✗ {} : tu ne peux pas te défier toi-même", pseudo);
                self.info("warn", texte.clone(), INFO_DUREE_MS);
                return self.reponse(cfg, texte);
            }
        }

        // Statut contextuel : lobby/combat/fin → info utile, pas une erreur.
        match self.statut_si_actif(cfg) {
            StatutArene::Idle => {}
            StatutArene::Actif(rep) => {
                return rep.and_then(|texte| self.reponse(cfg, texte));
            }
        }

        let cle = cle_joueur(plateforme, pseudo);
        {
            let mut inner = self.inner.lock().unwrap();
            // Course inter-plateformes : un défi/combat a pu partir entre le
            // statut et ce verrou — ne jamais écraser une partie active.
            if !matches!(inner.phase, Phase::Idle) {
                return None;
            }
            if let Some(restant) = cooldown_restant(&inner, &cle, cfg.cooldown_combat_s) {
                let texte = format!("⏳ {} : combat en cooldown ({} s)", pseudo, restant);
                pousser_info(&mut inner, "warn", texte.clone(), INFO_DUREE_MS);
                drop(inner);
                self.emit_state();
                return self.reponse(cfg, texte);
            }

            let (emoji, avert) = emoji_pour(&mut inner, &cle, emoji, None);
            if let Some(a) = avert {
                pousser_info(&mut inner, "warn", a, INFO_DUREE_MS);
            }
            let j = Joueur {
                cle,
                pseudo: pseudo.to_string(),
                plateforme: plateforme.to_string(),
                emoji,
                bot: false,
            };

            if bot {
                let jb = Joueur {
                    cle: "bot:bot".to_string(),
                    pseudo: "BOT".to_string(),
                    plateforme: "bot".to_string(),
                    emoji: EMOJI_BOT.to_string(),
                    bot: true,
                };
                let info = format!("⚔ {} affronte le BOT — FIGHT !", pseudo);
                let texte = format!(
                    "{} — au tour de {} : !{} rapide · !{} lourd · !{} spécial ({} s)",
                    info, pseudo, cfg.commande_a, cfg.commande_b, cfg.commande_c, cfg.duree_tour_s
                );
                pousser_info(&mut inner, "ok", info, INFO_DUREE_MS);
                drop(inner);
                self.demarrer_combat(cfg, j, jb);
                return self.reponse(cfg, texte);
            }

            // Lobby (ouvert ou ciblé) + timer d'expiration.
            let fin_lobby = Instant::now() + Duration::from_secs(cfg.duree_lobby_s);
            inner.phase = Phase::Lobby {
                defiant: j,
                cible_lower: cible.clone(),
                ouvert: cible.is_none(),
                fin_lobby,
            };
            inner.gen += 1;
            let gen = inner.gen;
            drop(inner);
            self.spawn_timer(Duration::from_secs(cfg.duree_lobby_s), gen, Minuteur::Lobby);
            self.emit_state();
            self.emit_update();
        }

        if let Some(c) = cible {
            self.reponse(
                cfg,
                format!("⚔ {} défie @{} — !{} ou !{}", pseudo, c, cfg.commande_accept, cfg.commande_refuse),
            )
        } else {
            self.reponse(
                cfg,
                format!("⚔ {} cherche un adversaire — !{}", pseudo, cfg.commande_accept),
            )
        }
    }

    /// "!accept [emoji]" — rejoindre le lobby (ouvert : tout sauf le
    /// défiant ; ciblé : le pseudo visé seulement).
    fn cmd_accept(
        &self,
        cfg: &FightConfig,
        plateforme: &str,
        pseudo: &str,
        reste: &str,
    ) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        let lobby = match &inner.phase {
            Phase::Lobby {
                defiant,
                ouvert,
                cible_lower,
                ..
            } => Some((defiant.clone(), *ouvert, cible_lower.clone())),
            _ => None,
        };
        let Some((defiant, ouvert, cible_lower)) = lobby else {
            drop(inner);
            let texte = format!(
                "✗ Aucun défi en attente — !{} pour en lancer un",
                cfg.commande_fight
            );
            self.info("warn", texte.clone(), INFO_DUREE_MS);
            return self.reponse(cfg, texte);
        };
        let cle = cle_joueur(plateforme, pseudo);
        if cle == defiant.cle {
            drop(inner);
            let texte = "✗ Tu ne peux pas accepter ton propre défi".to_string();
            self.info("warn", texte.clone(), INFO_DUREE_MS);
            return self.reponse(cfg, texte);
        }
        if !ouvert && cible_lower.as_deref() != Some(pseudo.to_lowercase().as_str()) {
            return None; // défi ciblé, pas pour lui — silence
        }
        if let Some(restant) = cooldown_restant(&inner, &cle, cfg.cooldown_combat_s) {
            let texte = format!("⏳ {} : combat en cooldown ({} s)", pseudo, restant);
            pousser_info(&mut inner, "warn", texte.clone(), INFO_DUREE_MS);
            drop(inner);
            self.emit_state();
            return self.reponse(cfg, texte);
        }
        let (emoji, avert) = emoji_pour(
            &mut inner,
            &cle,
            reste.split_whitespace().next().map(|s| s.to_string()),
            Some(&defiant.emoji),
        );
        if let Some(a) = avert {
            pousser_info(&mut inner, "warn", a, INFO_DUREE_MS);
        }
        let jb = Joueur {
            cle,
            pseudo: pseudo.to_string(),
            plateforme: plateforme.to_string(),
            emoji,
            bot: false,
        };
        let info = format!("⚔ FIGHT : {} vs {} !", defiant.pseudo, jb.pseudo);
        let texte = format!(
            "{} — au tour de {} : !{} rapide · !{} lourd · !{} spécial ({} s)",
            info, defiant.pseudo, cfg.commande_a, cfg.commande_b, cfg.commande_c, cfg.duree_tour_s
        );
        pousser_info(&mut inner, "ok", info, INFO_DUREE_MS);
        drop(inner);
        self.demarrer_combat(cfg, defiant, jb);
        self.reponse(cfg, texte)
    }

    /// "!refuse" — le défié d'un défi ciblé refuse → retour Idle.
    fn cmd_refuse(&self, cfg: &FightConfig, pseudo: &str) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        match &inner.phase {
            Phase::Lobby {
                cible_lower: Some(c),
                defiant,
                ..
            } if *c == pseudo.to_lowercase() => {
                let pseudo_defiant = defiant.pseudo.clone();
                inner.phase = Phase::Idle;
                inner.gen += 1;
                let texte = format!("✗ {} a refusé le défi de {}", pseudo, pseudo_defiant);
                pousser_info(&mut inner, "warn", texte.clone(), INFO_DUREE_MS);
                drop(inner);
                self.emit_state();
                self.emit_update();
                self.reponse(cfg, texte)
            }
            _ => None,
        }
    }

    /// "!ff" — abandonner le combat en cours (ou annuler son propre lobby).
    fn cmd_ff(&self, cfg: &FightConfig, plateforme: &str, pseudo: &str) -> Option<String> {
        let cle = cle_joueur(plateforme, pseudo);
        let mut inner = self.inner.lock().unwrap();
        if let Phase::Lobby { defiant, .. } = &inner.phase {
            if defiant.cle == cle {
                inner.phase = Phase::Idle;
                inner.gen += 1;
                let texte = format!("✗ {} annule son défi", pseudo);
                pousser_info(&mut inner, "warn", texte.clone(), INFO_DUREE_MS);
                drop(inner);
                self.emit_state();
                self.emit_update();
                return self.reponse(cfg, texte);
            }
        }
        let gagnant = match &inner.phase {
            Phase::Combat { ja, jb, .. } => {
                if ja.cle == cle {
                    Some('b')
                } else if jb.cle == cle {
                    Some('a')
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(g) = gagnant {
            let texte = format!("🏳 {} abandonne", pseudo);
            pousser_info(&mut inner, "warn", texte.clone(), INFO_DUREE_MS);
            drop(inner);
            self.terminer(Some(g), "abandon");
            return self.reponse(cfg, texte);
        }
        None
    }

    /// "!fightstop" — arrêt forcé (mods/broadcaster). Lobby → Idle direct ;
    /// combat/fin → bannière "stop" courte puis Idle. Aucun cooldown posé
    /// (décision 2026-10-27 : un arrêt de mod ne pénalise pas les joueurs).
    fn cmd_stop(&self, cfg: &FightConfig) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        let (texte, fin) = match &inner.phase {
            Phase::Idle => return None,
            Phase::Lobby { .. } => {
                inner.phase = Phase::Idle;
                inner.gen += 1;
                ("🛑 Défi annulé".to_string(), false)
            }
            Phase::Fin { .. } => {
                inner.phase = Phase::Idle;
                inner.gen += 1;
                ("🛑 Combat arrêté".to_string(), false)
            }
            _ => ("🛑 Combat arrêté".to_string(), true),
        };
        pousser_info(&mut inner, "warn", texte.clone(), INFO_DUREE_MS);
        drop(inner);
        if fin {
            self.terminer(None, "stop");
        } else {
            self.emit_state();
            self.emit_update();
        }
        self.reponse(cfg, texte)
    }

    /// "!a"/"!b"/"!c" — coup du joueur actif. Silence pour les non-joueurs ;
    /// un participant hors tour reçoit un toast throttlé (pas de spam chat).
    fn cmd_coup(
        &self,
        cfg: &FightConfig,
        plateforme: &str,
        pseudo: &str,
        coup: &str,
    ) -> Option<String> {
        let cle = cle_joueur(plateforme, pseudo);
        let ko: bool;
        {
            let mut inner = self.inner.lock().unwrap();
            let etat = match &inner.phase {
                Phase::Combat { ja, jb, tour, .. } => {
                    let qui = if ja.cle == cle {
                        Some('a')
                    } else if jb.cle == cle {
                        Some('b')
                    } else {
                        None
                    };
                    let nom_actif = if *tour == 'a' {
                        ja.pseudo.clone()
                    } else {
                        jb.pseudo.clone()
                    };
                    qui.map(|q| (q, nom_actif, *tour))
                }
                _ => None,
            };
            let (qui, nom_actif, tour) = etat?;
            if qui != tour {
                let key = format!("{}|tour", cle);
                let autorise = !matches!(
                    inner.derniere_cmd.get(&key),
                    Some(t) if t.elapsed() < Duration::from_secs(ANTI_SPAM_S)
                );
                if autorise {
                    inner.derniere_cmd.insert(key, Instant::now());
                    pousser_info(
                        &mut inner,
                        "info",
                        format!("⏳ C'est au tour de {}", nom_actif),
                        INFO_DUREE_MS,
                    );
                    drop(inner);
                    self.emit_state();
                }
                return None;
            }
            if coup == "special" && special_utilise(&inner, qui) {
                let texte = format!("✗ {} : spécial déjà utilisé ce combat", pseudo);
                pousser_info(&mut inner, "warn", texte.clone(), INFO_DUREE_MS);
                drop(inner);
                self.emit_state();
                return self.reponse(cfg, texte);
            }
            ko = self.resoudre(&mut inner, cfg, qui, coup);
            if !ko {
                // Tour suivant : swap + nouvelle deadline + gen++.
                avancer_tour(&mut inner, cfg);
            }
            inner.gen += 1;
        }
        if ko {
            let gagnant = self.gagnant_ko();
            let nom_gagnant = self.pseudo_gagnant(gagnant);
            let texte = format!("🏆 {} gagne le combat !", nom_gagnant);
            {
                let mut inner = self.inner.lock().unwrap();
                pousser_info(&mut inner, "ok", texte.clone(), INFO_DUREE_MS);
            }
            self.terminer(Some(gagnant), "ko");
            return self.reponse(cfg, texte);
        }
        self.apres_action(cfg);
        None // les coups ne répondent pas dans le chat (anti-spam)
    }

    /// Gagnant après un KO : 'b' si A est tombé à 0 PV, 'a' sinon.
    /// Lu après résolution alors que la phase est encore Combat.
    fn gagnant_ko(&self) -> char {
        let inner = self.inner.lock().unwrap();
        match &inner.phase {
            Phase::Combat { pv_a, .. } if *pv_a == 0 => 'b',
            _ => 'a',
        }
    }

    /// Pseudo du gagnant ('a'|'b') — lu après terminer (phase Fin).
    fn pseudo_gagnant(&self, gagnant: char) -> String {
        let inner = self.inner.lock().unwrap();
        match &inner.phase {
            Phase::Fin { ja, jb, .. } | Phase::Combat { ja, jb, .. } => {
                if gagnant == 'a' {
                    ja.pseudo.clone()
                } else {
                    jb.pseudo.clone()
                }
            }
            _ => "?".to_string(),
        }
    }

    /// Après une action résolue (humain ou timeout) : timers du tour suivant
    /// + bot éventuel + frame WS + event dashboard.
    fn apres_action(&self, cfg: &FightConfig) {
        // UN lock pour tout (gen + flag bot + délai bot) — le délai tiré
        // sous le même gen que le timer, sinon une action concurrente
        // pourrait invalider le gen entre les deux verrous.
        let (gen, bot_au_tour, tour_s, delai_bot) = {
            let mut inner = self.inner.lock().unwrap();
            let bot = match &inner.phase {
                Phase::Combat { ja, jb, tour, .. } => {
                    if *tour == 'a' {
                        ja.bot
                    } else {
                        jb.bot
                    }
                }
                _ => false,
            };
            let delai = if bot {
                rng_plage(
                    &mut inner.rng,
                    BOT_DELAI_MIN_MS as u32,
                    BOT_DELAI_MAX_MS as u32,
                ) as u64
            } else {
                0
            };
            (inner.gen, bot, cfg.duree_tour_s, delai)
        };
        self.spawn_timer(Duration::from_secs(tour_s), gen, Minuteur::Tour);
        if bot_au_tour {
            self.spawn_timer(Duration::from_millis(delai_bot), gen, Minuteur::Bot);
        }
        self.emit_state();
        self.emit_update();
        self.annoncer_tour(cfg, gen);
    }

    /// Annonce le tour humain dans le chat Twitch + programme le rappel 5 s.
    /// `gen` = génération du tour : toute action ultérieure invalide le rappel.
    fn annoncer_tour(&self, cfg: &FightConfig, gen: u64) {
        let (pseudo, tour_s) = {
            let inner = self.inner.lock().unwrap();
            if inner.gen != gen {
                return;
            }
            match &inner.phase {
                Phase::Combat { ja, jb, tour, .. } => {
                    let j = if *tour == 'a' { ja } else { jb };
                    if j.bot || j.plateforme == "local" {
                        return;
                    }
                    (j.pseudo.clone(), cfg.duree_tour_s)
                }
                _ => return,
            }
        };
        if cfg.annonce_tour {
            self.dire_chat(
                cfg,
                format!(
                    "🎯 Au tour de {} — !{} rapide · !{} lourd · !{} spécial ({} s)",
                    pseudo, cfg.commande_a, cfg.commande_b, cfg.commande_c, tour_s
                ),
            );
        }
        if cfg.rappel_tour && tour_s > 5 {
            self.spawn_timer(
                Duration::from_secs(tour_s - 5),
                gen,
                Minuteur::RappelTour,
            );
        }
    }

    /// Poste un message asynchrone dans le chat Twitch (tour, AFK, expiration).
    /// Sans effet si `reponses_chat` est off ou si l'IRC n'est pas connecté.
    fn dire_chat(&self, cfg: &FightConfig, texte: String) {
        if !cfg.reponses_chat {
            return;
        }
        if let Some(twitch) = self.app.try_state::<crate::TwitchState>() {
            let _ = twitch.envoyer_message_chat(&texte);
        }
    }

    /// Pousse un toast d'information + émet le snapshot (hors verrou).
    fn info(&self, kind: &str, texte: String, duree_ms: u64) {
        {
            let mut inner = self.inner.lock().unwrap();
            pousser_info(&mut inner, kind, texte, duree_ms);
        }
        self.emit_state();
    }

    /// Réponse contextuelle quand un défi/combat est déjà en cours.
    /// Throttle global STATUT_SPAM_S pour éviter le spam de `!fight`.
    fn statut_si_actif(&self, cfg: &FightConfig) -> StatutArene {
        let mut inner = self.inner.lock().unwrap();
        let texte = match &inner.phase {
            Phase::Idle => return StatutArene::Idle,
            Phase::Lobby {
                defiant,
                cible_lower,
                fin_lobby,
                ..
            } => {
                let reste = fin_lobby
                    .saturating_duration_since(Instant::now())
                    .as_secs();
                if let Some(c) = cible_lower {
                    format!(
                        "⚔ {} défie @{} — !{} ou !{} ({} s)",
                        defiant.pseudo, c, cfg.commande_accept, cfg.commande_refuse, reste
                    )
                } else {
                    format!(
                        "⚔ Défi de {} en attente — !{} pour rejoindre ({} s)",
                        defiant.pseudo, cfg.commande_accept, reste
                    )
                }
            }
            Phase::Combat {
                ja,
                jb,
                tour,
                fin_tour,
                ..
            } => {
                let reste = fin_tour
                    .saturating_duration_since(Instant::now())
                    .as_secs();
                let actif = if *tour == 'a' { &ja.pseudo } else { &jb.pseudo };
                format!(
                    "⚔ Combat : {} vs {} — au tour de {} ({} s)",
                    ja.pseudo, jb.pseudo, actif, reste
                )
            }
            Phase::Fin { .. } => {
                "⚔ Le combat vient de se terminer — réessaie dans un instant".to_string()
            }
        };
        let autorise = statut_autorise(&mut inner);
        if autorise {
            pousser_info(&mut inner, "info", texte.clone(), INFO_DUREE_MS);
        }
        drop(inner);
        if autorise {
            self.emit_state();
            StatutArene::Actif(Some(texte))
        } else {
            StatutArene::Actif(None)
        }
    }

    /// "!fight aide" — carte d'aide sur l'overlay + rappel compact en chat.
    /// Disponible même quand l'arène est inactive (avertissement explicite).
    fn cmd_aide(&self, cfg: &FightConfig) -> Option<String> {
        let texte_chat = if cfg.actif {
            tronquer(aide_chat(cfg), 480)
        } else {
            tronquer(format!("⚠ Arène inactive — {}", aide_chat(cfg)), 480)
        };
        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(t) = inner.derniere_aide {
                if t.elapsed() < Duration::from_secs(AIDE_COOLDOWN_S) {
                    return None;
                }
            }
            inner.derniere_aide = Some(Instant::now());
            let base = if matches!(inner.phase, Phase::Combat { .. }) {
                aide_overlay_compact(cfg)
            } else {
                aide_overlay(cfg)
            };
            let overlay = if cfg.actif {
                base
            } else {
                format!(
                    "⚠ Arène inactive — le streamer doit l'activer.\n{}",
                    base
                )
            };
            pousser_info(&mut inner, "aide", overlay, AIDE_DUREE_MS);
        }
        self.emit_state();
        self.reponse(cfg, texte_chat)
    }

    /// "!fight liste" — rappelle le pool d'emojis aléatoires.
    fn cmd_liste(&self, cfg: &FightConfig) -> Option<String> {
        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(t) = inner.derniere_aide {
                if t.elapsed() < Duration::from_secs(AIDE_COOLDOWN_S) {
                    return None;
                }
            }
            inner.derniere_aide = Some(Instant::now());
            pousser_info(
                &mut inner,
                "aide",
                format!(
                    "🎭 Emojis disponibles\n{}\n… ou n'importe quel emoji",
                    EMOJIS_POOL.join("  ")
                ),
                8000,
            );
        }
        self.emit_state();
        self.reponse(
            cfg,
            format!(
                "🎭 Emojis : {} (ou n'importe quel emoji)",
                EMOJIS_POOL.join(" ")
            ),
        )
    }

    /// Bouton dashboard : pousse la carte d'aide sur l'overlay, sans chat.
    pub fn previsualiser_aide(&self) {
        let cfg = self.config.lock().unwrap().clone();
        let overlay = aide_overlay(&cfg);
        {
            let mut inner = self.inner.lock().unwrap();
            pousser_info(&mut inner, "aide", overlay, AIDE_DUREE_MS);
        }
        self.emit_state();
    }

    /// Démarre le combat (appelé par accept / !fight bot / tester).
    fn demarrer_combat(&self, cfg: &FightConfig, ja: Joueur, jb: Joueur) {
        let now = Instant::now();
        let fin_match = now + Duration::from_secs(cfg.duree_match_s);
        let gen = {
            let mut inner = self.inner.lock().unwrap();
            inner.phase = Phase::Combat {
                ja,
                jb,
                pv_a: cfg.vie_max,
                pv_b: cfg.vie_max,
                tour: 'a',
                special_a: false,
                special_b: false,
                passes_a: 0,
                passes_b: 0,
                fin_tour: now + Duration::from_secs(cfg.duree_tour_s),
                fin_match,
                derniere: None,
            };
            inner.gen += 1;
            inner.gen
        };
        self.spawn_timer(Duration::from_secs(cfg.duree_tour_s), gen, Minuteur::Tour);
        self.spawn_timer(Duration::from_secs(cfg.duree_match_s), gen, Minuteur::Match(fin_match));
        self.emit_state();
        self.emit_update();
    }

    /// Résout un coup : tirage touche/critique/dégâts, applique PV + soin,
    /// enregistre `derniere` (seq++). Retourne true si KO.
    /// ⚠ `inner` doit déjà être verrouillé — ne pas re-locker ici.
    fn resoudre(&self, inner: &mut Inner, cfg: &FightConfig, qui: char, coup: &str) -> bool {
        let Phase::Combat {
            pv_a,
            pv_b,
            special_a,
            special_b,
            passes_a,
            passes_b,
            derniere,
            ..
        } = &mut inner.phase
        else {
            return false;
        };

        // Une action réelle (même ratée) casse la série de passes AFK.
        if coup != "passe" {
            if qui == 'a' {
                *passes_a = 0;
            } else {
                *passes_b = 0;
            }
        }

        let (deg_min, deg_max, hit_pct, soin_potentiel) = match coup {
            "rapide" => (RAPIDE_DEG_MIN, RAPIDE_DEG_MAX, RAPIDE_HIT_PCT, 0),
            "lourd" => (LOURD_DEG_MIN, LOURD_DEG_MAX, LOURD_HIT_PCT, 0),
            "special" => {
                // Marque l'usage AVANT le tirage (un spécial raté est quand
                // même consommé — règle de jeu assumée, le coût du risque).
                if qui == 'a' {
                    *special_a = true;
                } else {
                    *special_b = true;
                }
                (SPECIAL_DEG, SPECIAL_DEG, SPECIAL_HIT_PCT, SPECIAL_SOIN)
            }
            // "passe" (timeout) : touche garantie, dégâts nuls.
            _ => (0, 0, 100, 0),
        };
        let touche = rng_pct(&mut inner.rng, hit_pct);
        let critique = touche
            && coup != "special"
            && coup != "passe"
            && rng_pct(&mut inner.rng, CRIT_PCT);
        let mut degats = 0u32;
        let mut soin = 0u32;
        if touche {
            degats = rng_plage(&mut inner.rng, deg_min, deg_max);
            if critique {
                degats = degats.saturating_mul(2);
            }
            soin = soin_potentiel;
        }
        // Dégâts sur l'adversaire, soin sur soi (vol de vie du spécial).
        if qui == 'a' {
            *pv_b = pv_b.saturating_sub(degats);
            if soin > 0 {
                *pv_a = (*pv_a + soin).min(cfg.vie_max);
            }
        } else {
            *pv_a = pv_a.saturating_sub(degats);
            if soin > 0 {
                *pv_b = (*pv_b + soin).min(cfg.vie_max);
            }
        }

        inner.action_seq += 1;
        *derniere = Some(ActionCoup {
            seq: inner.action_seq,
            acteur: qui,
            coup: coup.to_string(),
            degats,
            soin,
            touche,
            critique,
        });
        *pv_a == 0 || *pv_b == 0
    }

    /// Passe à la phase Fin (bannière) : cooldowns posés sur les deux
    /// participants humains, timer de retour à Idle, frame + event.
    /// No-op si la phase n'est plus Combat (timer périmé vs course réelle).
    fn terminer(&self, gagnant: Option<char>, raison: &str) {
        let gen = {
            let mut inner = self.inner.lock().unwrap();
            let (ja, jb, pv_a, pv_b) = match &inner.phase {
                Phase::Combat {
                    ja, jb, pv_a, pv_b, ..
                } => (ja.clone(), jb.clone(), *pv_a, *pv_b),
                _ => return,
            };
            let now = Instant::now();
            // Un arrêt de mod ou une fin AFK ne pénalise pas les joueurs
            // (décision 2026-10-27) — seuls ko/abandon/temps posent le cooldown.
            if raison != "stop" && raison != "afk" {
                if !ja.bot {
                    inner.cooldowns.insert(ja.cle.clone(), now);
                }
                if !jb.bot {
                    inner.cooldowns.insert(jb.cle.clone(), now);
                }
            }
            inner.phase = Phase::Fin {
                ja,
                jb,
                pv_a,
                pv_b,
                gagnant,
                raison: raison.to_string(),
            };
            inner.gen += 1;
            inner.gen
        };
        self.spawn_timer(Duration::from_millis(FIN_AFFICHAGE_MS), gen, Minuteur::Fin);
        self.emit_state();
        self.emit_update();
    }

    // ===== Timers =====

    fn spawn_timer(&self, duree: Duration, gen: u64, min: Minuteur) {
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(duree).await;
            state.on_timer(gen, min);
        });
    }

    /// Réveil d'un timer : le `gen` capturé au spawn doit être le gen
    /// courant (toute mutation intermédiaire rend le timer obsolète).
    fn on_timer(&self, gen: u64, min: Minuteur) {
        match min {
            Minuteur::Lobby => {
                let cfg = self.config.lock().unwrap().clone();
                let mut inner = self.inner.lock().unwrap();
                if inner.gen != gen || !matches!(inner.phase, Phase::Lobby { .. }) {
                    return;
                }
                let texte = match &inner.phase {
                    Phase::Lobby { defiant, .. } => format!(
                        "⌛ Défi de {} expiré — !{} pour relancer",
                        defiant.pseudo, cfg.commande_fight
                    ),
                    _ => String::new(),
                };
                inner.phase = Phase::Idle;
                inner.gen += 1;
                pousser_info(&mut inner, "warn", texte.clone(), INFO_DUREE_MS);
                drop(inner);
                self.dire_chat(&cfg, texte);
                self.emit_state();
                self.emit_update();
            }
            Minuteur::Tour => {
                // Timeout de tour : le joueur actif "passe" (coup 0 dégât,
                // visible côté overlay comme action manquée) puis tour
                // suivant — la partie n'attend jamais un AFK. Après
                // PASSES_AFK passes consécutifs de CHAQUE joueur, le match
                // est mort : fin AFK immédiate, sans cooldown.
                let cfg = self.config.lock().unwrap().clone();
                let afk = {
                    let mut inner = self.inner.lock().unwrap();
                    if inner.gen != gen {
                        return;
                    }
                    let tour = match &inner.phase {
                        Phase::Combat { tour, .. } => *tour,
                        _ => return,
                    };
                    let _ = self.resoudre(&mut inner, &cfg, tour, "passe");
                    if let Phase::Combat {
                        passes_a, passes_b, ..
                    } = &mut inner.phase
                    {
                        if tour == 'a' {
                            *passes_a += 1;
                        } else {
                            *passes_b += 1;
                        }
                    }
                    let (pa, pb) = match &inner.phase {
                        Phase::Combat {
                            passes_a, passes_b, ..
                        } => (*passes_a, *passes_b),
                        _ => (0, 0),
                    };
                    let afk = afk_atteint(pa, pb);
                    if afk {
                        pousser_info(
                            &mut inner,
                            "warn",
                            "⏹ Match abandonné — inactivité".to_string(),
                            INFO_DUREE_MS,
                        );
                    } else {
                        avancer_tour(&mut inner, &cfg);
                    }
                    inner.gen += 1;
                    afk
                };
                if afk {
                    self.dire_chat(&cfg, "⏹ Match abandonné — inactivité".to_string());
                    self.terminer(None, "afk");
                } else {
                    self.apres_action(&cfg);
                }
            }
            Minuteur::Match(fin_match) => {
                // Deadline match : garde par IDENTITÉ de l'Instant (le gen
                // bouge à chaque action — la fin de match, non).
                let (pv_a, pv_b) = {
                    let inner = self.inner.lock().unwrap();
                    match &inner.phase {
                        Phase::Combat {
                            pv_a,
                            pv_b,
                            fin_match: f,
                            ..
                        } if *f == fin_match => (*pv_a, *pv_b),
                        _ => return,
                    }
                };
                let gagnant = if pv_a > pv_b {
                    Some('a')
                } else if pv_b > pv_a {
                    Some('b')
                } else {
                    None
                };
                self.terminer(gagnant, "temps");
            }
            Minuteur::Fin => {
                let mut inner = self.inner.lock().unwrap();
                if inner.gen != gen || !matches!(inner.phase, Phase::Fin { .. }) {
                    return;
                }
                inner.phase = Phase::Idle;
                inner.gen += 1;
                drop(inner);
                self.emit_state();
                self.emit_update();
            }
            Minuteur::RappelTour => {
                let cfg = self.config.lock().unwrap().clone();
                if !cfg.rappel_tour {
                    return;
                }
                let pseudo = {
                    let inner = self.inner.lock().unwrap();
                    if inner.gen != gen {
                        return;
                    }
                    match &inner.phase {
                        Phase::Combat { ja, jb, tour, .. } => {
                            let j = if *tour == 'a' { ja } else { jb };
                            if j.bot || j.plateforme == "local" {
                                return;
                            }
                            j.pseudo.clone()
                        }
                        _ => return,
                    }
                };
                self.dire_chat(&cfg, format!("⏳ {} : 5 s pour jouer !", pseudo));
            }
            Minuteur::Bot => {
                let cfg = self.config.lock().unwrap().clone();
                let mut inner = self.inner.lock().unwrap();
                if inner.gen != gen {
                    return;
                }
                let (qui, pv_bot, special_utilise_bot) = match &inner.phase {
                    Phase::Combat {
                        ja,
                        jb,
                        tour,
                        pv_a,
                        pv_b,
                        special_a,
                        special_b,
                        ..
                    } => {
                        let (q, pv, su) = if ja.bot {
                            ('a', *pv_a, *special_a)
                        } else if jb.bot {
                            ('b', *pv_b, *special_b)
                        } else {
                            return; // pas de bot dans ce combat
                        };
                        if *tour != q {
                            return;
                        }
                        (q, pv, su)
                    }
                    _ => return,
                };
                // Stratégie bot : spécial (vol de vie) si PV bas et dispo,
                // sinon pondéré rapide 65 % / lourd 35 %.
                let seuil = cfg.vie_max * BOT_SEUIL_SPECIAL_PCT / 100;
                let coup = if !special_utilise_bot
                    && pv_bot <= seuil
                    && rng_pct(&mut inner.rng, 60)
                {
                    "special"
                } else if rng_pct(&mut inner.rng, 65) {
                    "rapide"
                } else {
                    "lourd"
                };
                let ko = self.resoudre(&mut inner, &cfg, qui, coup);
                if !ko {
                    avancer_tour(&mut inner, &cfg);
                }
                inner.gen += 1;
                drop(inner);
                if ko {
                    let gagnant = self.gagnant_ko();
                    let texte = format!(
                        "🏆 {} gagne le combat !",
                        self.pseudo_gagnant(gagnant)
                    );
                    {
                        let mut inner = self.inner.lock().unwrap();
                        pousser_info(&mut inner, "ok", texte.clone(), INFO_DUREE_MS);
                    }
                    self.dire_chat(&cfg, texte);
                    self.terminer(Some(gagnant), "ko");
                } else {
                    self.apres_action(&cfg);
                }
            }
        }
    }

    // ===== Test manuel / arrêt dashboard =====

    /// Bouton « Tester » : lance un combat factice Streamy vs BOT (pipeline
    /// réel — les coups du BOT sont joués par le même code que !fight bot).
    pub fn tester(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        {
            let inner = self.inner.lock().unwrap();
            if !matches!(inner.phase, Phase::Idle) {
                return Err("Un combat ou un défi est déjà en cours".to_string());
            }
        }
        let ja = Joueur {
            cle: "local:streamy".to_string(),
            pseudo: "Streamy".to_string(),
            plateforme: "local".to_string(),
            emoji: "🦊".to_string(),
            bot: false,
        };
        let jb = Joueur {
            cle: "bot:bot".to_string(),
            pseudo: "BOT".to_string(),
            plateforme: "bot".to_string(),
            emoji: EMOJI_BOT.to_string(),
            bot: true,
        };
        {
            let mut inner = self.inner.lock().unwrap();
            pousser_info(
                &mut inner,
                "ok",
                "⚔ Combat de test — Streamy vs BOT".to_string(),
                INFO_DUREE_MS,
            );
        }
        self.demarrer_combat(&cfg, ja, jb);
        Ok(())
    }

    /// Bouton « Forcer l'arrêt » dashboard : lobby → Idle, combat → fin
    /// "stop", fin → Idle direct.
    pub fn stop(&self) {
        enum Sortie {
            Rien,
            Idle,
            FinStop,
        }
        let sortie = {
            let mut inner = self.inner.lock().unwrap();
            match &inner.phase {
                Phase::Idle => Sortie::Rien,
                Phase::Lobby { .. } => {
                    inner.phase = Phase::Idle;
                    inner.gen += 1;
                    Sortie::Idle
                }
                Phase::Combat { .. } => Sortie::FinStop,
                Phase::Fin { .. } => {
                    inner.phase = Phase::Idle;
                    inner.gen += 1;
                    Sortie::Idle
                }
            }
        };
        match sortie {
            Sortie::Rien => {}
            Sortie::Idle => {
                self.emit_state();
                self.emit_update();
            }
            Sortie::FinStop => self.terminer(None, "stop"),
        }
    }

    // ===== Config =====

    /// Config courante (lecture seule) — utilisée par `!cmd` pour lister
    /// les commandes viewer actives.
    pub fn config(&self) -> FightConfig {
        self.config.lock().unwrap().clone()
    }

    /// État complet pour l'UI dashboard : config + partie courante +
    /// cooldowns actifs (snapshot — l'UI remplace, pas de diff).
    pub fn etat(&self) -> serde_json::Value {
        let cfg = self.config.lock().unwrap().clone();
        let inner = self.inner.lock().unwrap();
        let now = Instant::now();
        let mut cooldowns: Vec<(String, u64)> = inner
            .cooldowns
            .iter()
            .filter_map(|(k, t)| {
                let reste = cfg
                    .cooldown_combat_s
                    .saturating_sub(now.duration_since(*t).as_secs());
                (reste > 0).then(|| (k.clone(), reste))
            })
            .collect();
        cooldowns.sort_by_key(|c| std::cmp::Reverse(c.1));
        let cooldowns_json: Vec<serde_json::Value> = cooldowns
            .into_iter()
            .map(|(joueur, restant_s)| serde_json::json!({ "joueur": joueur, "restant_s": restant_s }))
            .collect();
        serde_json::json!({
            "config": cfg,
            "partie": self.combat_json(&inner, &cfg),
            "cooldowns": cooldowns_json,
        })
    }

    /// Sauvegarde la config : noms normalisés (trim, sans "!", lowercase,
    /// non vides, distincts) + refus de collision avec les commandes chat
    /// personnalisées ET les commandes musique (les hooks déclencheraient
    /// en double — même règle que musique.rs).
    pub fn set_config(&self, mut cfg: FightConfig) -> Result<FightConfig, String> {
        for champ in [
            &mut cfg.commande_fight,
            &mut cfg.commande_accept,
            &mut cfg.commande_refuse,
            &mut cfg.commande_a,
            &mut cfg.commande_b,
            &mut cfg.commande_c,
            &mut cfg.commande_ff,
            &mut cfg.commande_stop,
        ] {
            *champ = valider_nom(champ.as_str())?;
        }
        // Les 8 noms doivent être distincts (sinon !x déclencherait deux
        // actions — la 1re gagne silencieusement dans on_message).
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
        // Collision avec les commandes chat personnalisées.
        if let Some(commandes) = self.app.try_state::<crate::commandes::CommandesState>() {
            for exist in commandes.etat() {
                if noms.contains(&exist.commande) {
                    return Err(format!(
                        "« !{} » est déjà une commande chat — choisissez un autre nom",
                        exist.commande
                    ));
                }
            }
        }
        // Collision avec les commandes musique (!rs, !skip, !retirer, !file).
        if let Some(m) = self.app.try_state::<crate::musique::MusiqueState>() {
            for r in m.noms_reserves() {
                if noms.contains(&r) {
                    return Err(format!(
                        "« !{} » est réservé à la musique — choisissez un autre nom",
                        r
                    ));
                }
            }
        }
        // Collision avec la commande cadres (!cadres — couleur du cadre).
        if let Some(c) = self.app.try_state::<crate::cadres_cmd::CadresChatState>() {
            for r in c.noms_reserves() {
                if noms.contains(&r) {
                    return Err(format!(
                        "« !{} » est réservé aux cadres — choisissez un autre nom",
                        r
                    ));
                }
            }
        }
        cfg.vie_max = cfg.vie_max.clamp(10, 1000);
        cfg.duree_lobby_s = cfg.duree_lobby_s.clamp(5, 120);
        cfg.duree_tour_s = cfg.duree_tour_s.clamp(5, 60);
        cfg.duree_match_s = cfg.duree_match_s.clamp(30, 900);
        cfg.cooldown_combat_s = cfg.cooldown_combat_s.min(3600);
        *self.config.lock().unwrap() = cfg.clone();
        self.save_config()?;
        self.emit_update();
        Ok(cfg)
    }

    /// Noms de commandes du module réservés (sans "!") — utilisés par
    /// commandes.rs pour refuser les collisions avec les commandes chat.
    pub fn noms_reserves(&self) -> Vec<String> {
        self.config.lock().unwrap().noms_reserves()
    }

    /// Accusé chat (None si reponses_chat désactivé — même règle que musique).
    fn reponse(&self, cfg: &FightConfig, texte: String) -> Option<String> {
        cfg.reponses_chat.then_some(texte)
    }

    /// Émet `fight:update` vers le dashboard (état complet).
    pub fn emit_update(&self) {
        let _ = self.app.emit("fight:update", self.etat());
    }

    // ===== WS (diffusion) =====

    /// Re-émet la frame `fight-state` courante (resync à la (re)connexion
    /// WS — reload OBS en plein combat). Appelée par server.rs.
    pub fn reemit(&self) {
        self.emit_state();
    }

    /// Émet `fight-state` sur le WS :4321 — snapshot COMPLET de la partie
    /// (la diffusion est un dumb renderer ; un resync = même code path).
    fn emit_state(&self) {
        let cfg = self.config.lock().unwrap().clone();
        let frame = {
            let inner = self.inner.lock().unwrap();
            serde_json::json!({
                "type": "fight-state",
                "combat": self.combat_json(&inner, &cfg),
            })
        };
        let _ = self.chat_tx.send(frame.to_string());
    }

    /// Construit le payload `combat` de la frame (forme selon la phase).
    /// `inner` et `cfg` sont passés en paramètres — appelants gardent
    /// l'ordre de verrouillage config PUIS inner, jamais l'inverse.
    fn combat_json(&self, inner: &Inner, cfg: &FightConfig) -> serde_json::Value {
        let now = Instant::now();
        match &inner.phase {
            Phase::Idle => serde_json::json!({
                "phase": "idle",
                "info": info_json(&inner.derniere_info),
                "anims_reduites": cfg.animations_reduites,
            }),
            Phase::Lobby {
                defiant,
                cible_lower,
                ouvert,
                fin_lobby,
            } => serde_json::json!({
                "phase": "lobby",
                "defiant": joueur_json(defiant, 0, 0, false),
                "cible": cible_lower,
                "ouvert": ouvert,
                "reste_lobby_ms": fin_lobby.saturating_duration_since(now).as_millis() as u64,
                "duree_tour_s": cfg.duree_tour_s,
                "duree_match_s": cfg.duree_match_s,
                "info": info_json(&inner.derniere_info),
                "anims_reduites": cfg.animations_reduites,
                // Noms des commandes dans la frame : l'overlay est la SEULE
                // source d'info hors Twitch (pas de réponse chat ailleurs).
                "cmds": {
                    "fight": cfg.commande_fight.as_str(),
                    "accept": cfg.commande_accept.as_str(),
                    "refuse": cfg.commande_refuse.as_str(),
                },
            }),
            Phase::Combat {
                ja,
                jb,
                pv_a,
                pv_b,
                tour,
                special_a,
                special_b,
                fin_tour,
                fin_match,
                derniere,
                ..
            } => serde_json::json!({
                "phase": "combat",
                "ja": joueur_json(ja, *pv_a, cfg.vie_max, *special_a),
                "jb": joueur_json(jb, *pv_b, cfg.vie_max, *special_b),
                "tour": tour.to_string(),
                "reste_tour_ms": fin_tour.saturating_duration_since(now).as_millis() as u64,
                "duree_tour_ms": cfg.duree_tour_s * 1000,
                "reste_match_s": fin_match.saturating_duration_since(now).as_secs(),
                "derniere": derniere_json(derniere),
                "info": info_json(&inner.derniere_info),
                "anims_reduites": cfg.animations_reduites,
                // Idem lobby : les hints des coups s'affichent sous le timer.
                "cmds": {
                    "fight": cfg.commande_fight.as_str(),
                    "a": cfg.commande_a.as_str(),
                    "b": cfg.commande_b.as_str(),
                    "c": cfg.commande_c.as_str(),
                    "ff": cfg.commande_ff.as_str(),
                },
            }),
            Phase::Fin {
                ja,
                jb,
                pv_a,
                pv_b,
                gagnant,
                raison,
                ..
            } => serde_json::json!({
                "phase": "fin",
                "ja": joueur_json(ja, *pv_a, cfg.vie_max, false),
                "jb": joueur_json(jb, *pv_b, cfg.vie_max, false),
                "gagnant": gagnant.map(|g| g.to_string()),
                "raison": raison,
                "info": info_json(&inner.derniere_info),
                "anims_reduites": cfg.animations_reduites,
            }),
        }
    }

    // ===== Persistance =====

    fn save_config(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let path = dir.join("fight_config.json");
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize fight_config.json: {}", e))?;
        fs::write(&path, json).map_err(|e| format!("Écrire fight_config.json: {}", e))?;
        Ok(())
    }
}

// ===== Helpers hors impl =====

/// Spécial déjà consommé par ce joueur ?
fn special_utilise(inner: &Inner, qui: char) -> bool {
    match &inner.phase {
        Phase::Combat {
            special_a,
            special_b,
            ..
        } => {
            if qui == 'a' {
                *special_a
            } else {
                *special_b
            }
        }
        _ => true,
    }
}

/// Cooldown restant (secondes) pour une clé joueur — None si libre.
fn cooldown_restant(inner: &Inner, cle: &str, cooldown_s: u64) -> Option<u64> {
    if cooldown_s == 0 {
        return None;
    }
    let last = inner.cooldowns.get(cle)?;
    let reste = cooldown_s.saturating_sub(last.elapsed().as_secs());
    (reste > 0).then_some(reste)
}

/// Emoji aléatoire du pool — `autre` évite un doublon avec l'adversaire.
fn emoji_aleatoire(rng: &mut u64, autre: Option<&str>) -> String {
    let idx = rng_plage(rng, 0, (EMOJIS_POOL.len() - 1) as u32) as usize;
    let e = EMOJIS_POOL[idx];
    if Some(e) == autre {
        // Retire une fois : décalage déterministe dans le pool.
        return EMOJIS_POOL[(idx + 1) % EMOJIS_POOL.len()].to_string();
    }
    e.to_string()
}

/// Valide un emoji saisi : une séquence courte, non ASCII, sans mot/URL.
/// Heuristique volontairement simple (pas de crate de segmentation) :
/// 16 codepoints max couvrent les familles ZWJ, drapeaux et tons de peau.
fn emoji_valide(tok: &str) -> bool {
    let t = tok.trim();
    if t.is_empty() || t.chars().count() > 16 {
        return false;
    }
    if t.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if t.chars().any(|c| c.is_whitespace() || c == '@' || c == '/' || c == '\\') {
        return false;
    }
    // Un token 100 % ASCII (« !!! », « 123 ») n'est pas un emoji.
    !t.is_ascii()
}

/// true si le token ressemble à un nom d'emote Twitch/BTTV/7TV/FFZ.
fn est_nom_emote(tok: &str) -> bool {
    let t = tok.trim();
    !t.is_empty()
        && t.chars().count() <= 32
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Mémorise l'emoji d'un viewer (session, cap simple).
fn memoriser_emoji(inner: &mut Inner, cle: &str, emoji: &str) {
    if inner.derniers_emojis.len() >= EMOJIS_MEMO_MAX {
        inner.derniers_emojis.clear();
    }
    inner
        .derniers_emojis
        .insert(cle.to_string(), emoji.to_string());
}

/// Choisit l'emoji : explicite validé → mémoire de session → aléatoire.
/// Retourne (emoji, avertissement éventuel à afficher en toast).
fn emoji_pour(
    inner: &mut Inner,
    cle: &str,
    explicite: Option<String>,
    autre: Option<&str>,
) -> (String, Option<String>) {
    let mut avert = None;
    if let Some(tok) = explicite {
        let tok = tok.trim().to_string();
        if emoji_valide(&tok) {
            memoriser_emoji(inner, cle, &tok);
            return (tok, None);
        }
        avert = Some(if est_nom_emote(&tok) {
            format!(
                "✗ « {} » est un nom d'emote, pas un emoji — un emoji aléatoire a été choisi",
                tok
            )
        } else {
            format!(
                "✗ « {} » n'est pas un emoji valide — un emoji aléatoire a été choisi",
                tok
            )
        });
    }
    let memo = inner.derniers_emojis.get(cle).cloned();
    let emoji = match memo {
        Some(e) if emoji_valide(&e) && Some(e.as_str()) != autre => e,
        _ => emoji_aleatoire(&mut inner.rng, autre),
    };
    memoriser_emoji(inner, cle, &emoji);
    (emoji, avert)
}

/// Normalise et valide un nom de commande (sans "!") : lowercase, 20 chars
/// max, lettres/chiffres/_/- uniquement (évite les noms qui cassent l'overlay).
fn valider_nom(brut: &str) -> Result<String, String> {
    let nom = brut.trim().trim_start_matches('!').to_lowercase();
    if nom.is_empty() {
        return Err("Nom de commande vide".into());
    }
    if nom.chars().count() > COMMANDE_MAX_CHARS {
        return Err(format!(
            "Nom de commande trop long ({} caractères max)",
            COMMANDE_MAX_CHARS
        ));
    }
    if !nom
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Nom de commande invalide (lettres, chiffres, _ et - uniquement)".into());
    }
    Ok(nom)
}

/// Tronque une chaîne à `max` caractères (ellipse incluse).
fn tronquer(s: String, max: usize) -> String {
    if s.chars().count() > max {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    } else {
        s
    }
}

/// Aide compacte pour le chat Twitch (une ligne, ≤ 480 chars).
fn aide_chat(cfg: &FightConfig) -> String {
    format!(
        "⚔ ARÈNE — !{f} · !{f} @pseudo · !{f} bot · !{ac} 🐲 | !{a} rapide 8-12 (90%) · !{b} lourd 18-26 (55%) · !{c} spécial vol de vie 1× | {t} s/tour · {m} min max · !{ff} abandon · !{f} aide · !{cmd}",
        f = cfg.commande_fight,
        ac = cfg.commande_accept,
        a = cfg.commande_a,
        b = cfg.commande_b,
        c = cfg.commande_c,
        ff = cfg.commande_ff,
        t = cfg.duree_tour_s,
        m = (cfg.duree_match_s / 60).max(1),
        cmd = crate::commandes::CMD_LISTE,
    )
}

/// Carte d'aide complète (overlay, hors combat).
fn aide_overlay(cfg: &FightConfig) -> String {
    format!(
        "⚔ ARÈNE D'EMOJIS\n!{f} · !{f} @pseudo · !{f} bot · !{ac} 🐲\n!{a} rapide 8-12 (90 %) · !{b} lourd 18-26 (55 %) · !{c} spécial vol de vie (1×)\n{t} s/tour · {m} min max · !{ff} abandon · !{f} liste pour les emojis\n!{cmd} : toutes les commandes actives",
        f = cfg.commande_fight,
        ac = cfg.commande_accept,
        a = cfg.commande_a,
        b = cfg.commande_b,
        c = cfg.commande_c,
        ff = cfg.commande_ff,
        t = cfg.duree_tour_s,
        m = (cfg.duree_match_s / 60).max(1),
        cmd = crate::commandes::CMD_LISTE,
    )
}

/// Carte d'aide compacte (overlay, pendant un combat — ne masque pas l'arène).
fn aide_overlay_compact(cfg: &FightConfig) -> String {
    format!(
        "⚔ !{a} rapide · !{b} lourd · !{c} spécial (1×)\n{t} s/tour · !{ff} abandon",
        a = cfg.commande_a,
        b = cfg.commande_b,
        c = cfg.commande_c,
        ff = cfg.commande_ff,
        t = cfg.duree_tour_s,
    )
}

/// Fin AFK : les deux joueurs ont passé PASSES_AFK tours consécutifs.
fn afk_atteint(passes_a: u32, passes_b: u32) -> bool {
    passes_a >= PASSES_AFK && passes_b >= PASSES_AFK
}

/// Pousse un message d'info (toast/carte) — `inner` déjà verrouillé.
fn pousser_info(inner: &mut Inner, kind: &str, texte: String, duree_ms: u64) {
    inner.info_seq += 1;
    inner.derniere_info = Some(Info {
        seq: inner.info_seq,
        kind: kind.to_string(),
        texte,
        duree_ms,
        instant: Instant::now(),
    });
}

/// Throttle global des réponses de statut (`!fight` pendant un lobby/combat).
fn statut_autorise(inner: &mut Inner) -> bool {
    let now = Instant::now();
    match inner.dernier_statut {
        Some(t) if now.duration_since(t).as_secs() < STATUT_SPAM_S => false,
        _ => {
            inner.dernier_statut = Some(now);
            true
        }
    }
}

/// Payload `info` de la frame — `age_ms` permet au client d'ignorer un
/// message périmé après un resync/reload OBS.
fn info_json(info: &Option<Info>) -> serde_json::Value {
    match info {
        Some(i) => serde_json::json!({
            "seq": i.seq,
            "kind": i.kind,
            "texte": i.texte,
            "duree_ms": i.duree_ms,
            "age_ms": i.instant.elapsed().as_millis() as u64,
        }),
        None => serde_json::Value::Null,
    }
}

/// Passe au tour de l'adversaire + nouvelle deadline de tour.
/// ⚠ `inner` verrouillé par l'appelant.
fn avancer_tour(inner: &mut Inner, cfg: &FightConfig) {
    if let Phase::Combat {
        tour, fin_tour, ..
    } = &mut inner.phase
    {
        *tour = if *tour == 'a' { 'b' } else { 'a' };
        *fin_tour = Instant::now() + Duration::from_secs(cfg.duree_tour_s);
    }
}

/// Payload joueur pour la frame WS.
fn joueur_json(j: &Joueur, pv: u32, pv_max: u32, special_utilise: bool) -> serde_json::Value {
    serde_json::json!({
        "pseudo": j.pseudo,
        "plateforme": j.plateforme,
        "emoji": j.emoji,
        "bot": j.bot,
        "pv": pv,
        "pv_max": pv_max,
        "special_utilise": special_utilise,
    })
}

/// Payload de la dernière action (None = combat qui vient de démarrer).
fn derniere_json(d: &Option<ActionCoup>) -> serde_json::Value {
    match d {
        Some(a) => serde_json::json!({
            "seq": a.seq,
            "acteur": a.acteur.to_string(),
            "coup": a.coup,
            "degats": a.degats,
            "soin": a.soin,
            "touche": a.touche,
            "critique": a.critique,
        }),
        None => serde_json::Value::Null,
    }
}

/// Charge `fight_config.json` (absent → défaut).
fn load_file(app: &AppHandle) -> Result<FightConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("fight_config.json");
    if !path.exists() {
        return Ok(FightConfig::default());
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("Lire fight_config.json: {}", e))?;
    let cfg: FightConfig =
        serde_json::from_str(&content).map_err(|e| format!("Parse fight_config.json: {}", e))?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_valide_accepte_les_sequences() {
        assert!(emoji_valide("🐲"));
        assert!(emoji_valide("👨‍👩‍👧"));
        assert!(emoji_valide("🇫🇷"));
        assert!(emoji_valide("1️⃣"));
    }

    #[test]
    fn emoji_valide_refuse_les_mots_et_urls() {
        assert!(!emoji_valide("Kappa"));
        assert!(!emoji_valide("hello🐲"));
        assert!(!emoji_valide("http://x"));
        assert!(!emoji_valide(""));
        assert!(!emoji_valide("!!!"));
        assert!(!emoji_valide(&"🐲".repeat(17)));
    }

    #[test]
    fn valider_nom_borne_et_filtre() {
        assert_eq!(valider_nom("  !Fight ").unwrap(), "fight");
        assert!(valider_nom("").is_err());
        assert!(valider_nom("a".repeat(21).as_str()).is_err());
        assert!(valider_nom("fight!").is_err());
        assert!(valider_nom("🐲").is_err());
    }

    #[test]
    fn aide_utilise_les_noms_courants() {
        let cfg = FightConfig::default();
        let texte = aide_chat(&cfg);
        assert!(texte.contains("!fight"));
        assert!(texte.contains("!accept"));
        assert!(texte.contains("!a"));
    }

    #[test]
    fn afk_apres_trois_passes() {
        assert!(!afk_atteint(2, 2));
        assert!(afk_atteint(3, 3));
        assert!(!afk_atteint(3, 2));
    }
}
