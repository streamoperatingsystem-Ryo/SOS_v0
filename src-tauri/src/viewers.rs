//! Compteur de viewers unifié — affiché dans le widget chat en diffusion.
//!
//! UNE seule tâche pour toutes les plateformes (pas de boucle par plateforme) :
//! tick toutes les 15s, chaque plateforme connectée est interrogée selon son
//! propre rythme — 60s si en live, 180s si hors-ligne (back-off : inutile de
//! frapper l'API pour un stream arrêté). Max ~3 GET/min au total.
//!
//! - Twitch  : Helix GET /streams (stream_viewers)
//! - YouTube : Data API liveStreamingDetails.concurrentViewers (live_viewers)
//! - Kick    : GET api/v2/channels/<slug> → livestream.viewer_count
//! - TikTok  : RIEN ici — le count est poussé par le WS Webcast (RoomUserSeq),
//!   forwardé vers chat_tx dans tiktok_chat.rs. Zéro polling.
//!
//! Publication : `{"type":"viewers","plateforme":p,"count":n|null}` sur chat_tx
//! (diffusion :4321) + app.emit("viewers:update"). Envoyé seulement quand le
//! chiffre change (dedup) ou qu'une plateforme passe hors-ligne / déconnectée.
//!
//! + Communauté (objectifs de stream) : compteurs réels poussés par frames
//!   `{"type":"communaute","plateforme":p,...}` — même cadence, même dedup :
//!   - Twitch  : followers (1 GET first=1→total), subs + sub_points (1 GET)
//!   - YouTube : abonnes (channels.list, 1 GET — members = N/A, scope absent)
//!   - Kick    : followers (mutualisé dans le GET channel des viewers)
//!   - TikTok  : rien ici (viewers via RoomUserSeq dans tiktok_chat.rs)
//! + Détection « objectif atteint » : les widgets `goal` de la scène courante
//!   sont vérifiés à chaque mesure fraîche (viewers ET communauté) → frame
//!   `goal-atteint` au crossing ≥ cible, réarmée quand le compteur redescend.
//! + Creator Goals Twitch (widgets goal LIÉS par goalId) : GET /goals au
//!   tick twitch → frames `{"type":"goal-twitch",id,goal_type,description,
//!   actuel,cible,atteint,termine}` dédupliquées par goal ; disparus du GET =
//!   terminés → termine:true. Temps réel : EventSub channel.goal.* appelle
//!   `on_goal_event` (mêmes frames + atteinte). Scope channel:read:goals.

use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

/// Granularité de la boucle (pas le rythme d'appel API — voir LIVE_EVERY).
const TICK: Duration = Duration::from_secs(15);
/// Intervalle de fetch par plateforme EN LIVE.
const LIVE_EVERY: Duration = Duration::from_secs(60);
/// Intervalle de fetch par plateforme HORS-LIGNE ou en erreur.
const OFFLINE_EVERY: Duration = Duration::from_secs(180);

/// État par plateforme : prochain fetch autorisé + dernier count publié.
struct Plat {
    prochain: Instant,
    dernier: Option<u32>,
}

impl Plat {
    fn new(now: Instant) -> Self {
        Plat {
            prochain: now, // fetch dès le prochain tick (connexion fraîche)
            dernier: None,
        }
    }
}

/// Démarre la tâche viewers (durée de vie = app). Zéro coût si aucune
/// plateforme n'est connectée : la boucle dort 15s et ne fait aucun fetch.
pub fn demarrer(app: AppHandle, chat_tx: broadcast::Sender<String>) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut plats: HashMap<&'static str, Plat> = HashMap::new();
        // Dernières communautés publiées par plateforme (dedup par tuple).
        let mut commus: HashMap<&'static str, Communaute> = HashMap::new();
        let mut interval = tokio::time::interval(TICK);
        loop {
            interval.tick().await;
            let now = Instant::now();

            // --- Twitch (Helix /streams + communauté followers/subs) ---
            if let Some((uid, access)) = twitch_creds(&app) {
                let p = plats.entry("twitch").or_insert_with(|| Plat::new(now));
                if now >= p.prochain {
                    match crate::twitch_helix::stream_viewers(&uid, &access).await {
                        Ok(v) => {
                            p.prochain = now + if v.is_some() { LIVE_EVERY } else { OFFLINE_EVERY };
                            publier(&app, &chat_tx, "twitch", v, &mut p.dernier);
                            if let Some(n) = v {
                                verifier_objectifs(&app, &chat_tx, "twitch", &[("viewers", n as u64)]);
                            }
                        }
                        Err(e) => {
                            eprintln!("[Viewers] twitch err: {:?}", e);
                            p.prochain = now + LIVE_EVERY;
                        }
                    }
                    // Communauté (objectifs) : 2 GET light, non-fatal chacun.
                    let mut c = Communaute::default();
                    match crate::twitch_helix::followers_total(&uid, &access).await {
                        Ok(v) => c.followers = Some(v),
                        Err(e) => eprintln!("[Viewers] twitch followers err: {:?}", e),
                    }
                    match crate::twitch_helix::subs_totaux(&uid, &access).await {
                        Ok((s, pts)) => {
                            c.subs = Some(s);
                            c.sub_points = Some(pts);
                        }
                        Err(e) => eprintln!("[Viewers] twitch subs err: {:?}", e),
                    }
                    publier_communaute(&app, &chat_tx, "twitch", &c, &mut commus);
                    verifier_objectifs(&app, &chat_tx, "twitch", &metriques_commu(&c));
                    // Creator Goals déclarés (widgets liés par goalId) : 1 GET,
                    // non-fatal. 403 = vieux token sans channel:read:goals →
                    // need_reauth (même mécanique que les sections En direct).
                    match crate::twitch_helix::creator_goals(&uid, &access).await {
                        Ok(goals) => {
                            publier_goals(&app, &chat_tx, &goals);
                            verifier_objectifs_goals(&app, &chat_tx, &goals);
                        }
                        Err(crate::twitch_helix::HelixError::NeedReauth) => {
                            let _ = app.emit("twitch:need_reauth", ());
                        }
                        Err(e) => eprintln!("[Viewers] twitch goals err: {:?}", e),
                    }
                }
            } else {
                deconnecte(&app, &chat_tx, &mut plats, "twitch");
            }

            // --- YouTube (liveStreamingDetails + channel.subscriberCount) ---
            if let Some((cid, access)) = youtube_creds(&app) {
                let p = plats.entry("youtube").or_insert_with(|| Plat::new(now));
                if now >= p.prochain {
                    match crate::youtube_data::live_viewers(&access, &cid).await {
                        Ok(v) => {
                            p.prochain = now + if v.is_some() { LIVE_EVERY } else { OFFLINE_EVERY };
                            publier(&app, &chat_tx, "youtube", v, &mut p.dernier);
                            if let Some(n) = v {
                                verifier_objectifs(&app, &chat_tx, "youtube", &[("viewers", n as u64)]);
                            }
                        }
                        Err(e) => {
                            eprintln!("[Viewers] youtube err: {}", e);
                            p.prochain = now + OFFLINE_EVERY;
                        }
                    }
                    // Communauté : abonnés chaîne (1 GET). Members = N/A
                    // (scope channel-memberships.creator absent du Device Flow).
                    match crate::youtube_data::channel_info(&access).await {
                        Ok(info) => {
                            let c = Communaute { abonnes: Some(info.subscriber_count), ..Default::default() };
                            publier_communaute(&app, &chat_tx, "youtube", &c, &mut commus);
                            verifier_objectifs(&app, &chat_tx, "youtube", &metriques_commu(&c));
                        }
                        Err(e) => eprintln!("[Viewers] youtube channel err: {}", e),
                    }
                }
            } else {
                deconnecte(&app, &chat_tx, &mut plats, "youtube");
            }

            // --- Kick (api/v2/channels → viewers + followers, UN SEUL GET) ---
            if let Some(slug) = kick_creds(&app) {
                let p = plats.entry("kick").or_insert_with(|| Plat::new(now));
                if now >= p.prochain {
                    match crate::kick::fetch_channel_stats(&slug).await {
                        Ok((v, f)) => {
                            p.prochain = now + if v.is_some() { LIVE_EVERY } else { OFFLINE_EVERY };
                            publier(&app, &chat_tx, "kick", v, &mut p.dernier);
                            if let Some(n) = v {
                                verifier_objectifs(&app, &chat_tx, "kick", &[("viewers", n as u64)]);
                            }
                            let c = Communaute { followers: f, ..Default::default() };
                            publier_communaute(&app, &chat_tx, "kick", &c, &mut commus);
                            verifier_objectifs(&app, &chat_tx, "kick", &metriques_commu(&c));
                        }
                        Err(e) => {
                            eprintln!("[Viewers] kick err: {}", e);
                            p.prochain = now + OFFLINE_EVERY;
                        }
                    }
                }
            } else {
                deconnecte(&app, &chat_tx, &mut plats, "kick");
            }
        }
    })
}

/// Envoie le count sur chat_tx (diffusion) + emit frontend — seulement si
/// la valeur a changé depuis la dernière publication.
fn publier(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    plat: &'static str,
    count: Option<u32>,
    dernier: &mut Option<u32>,
) {
    if *dernier == count {
        return;
    }
    *dernier = count;
    let msg = json!({"type": "viewers", "plateforme": plat, "count": count}).to_string();
    let _ = chat_tx.send(msg);
    let _ = app.emit("viewers:update", json!({"plateforme": plat, "count": count}));
}

/// Plateforme déconnectée : publie None une fois (efface la pastille) puis
/// retire l'entrée — le prochain `Plat::new` au retour fera un fetch immédiat.
fn deconnecte(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    plats: &mut HashMap<&'static str, Plat>,
    nom: &'static str,
) {
    if let Some(p) = plats.remove(nom) {
        if p.dernier.is_some() {
            let msg = json!({"type": "viewers", "plateforme": nom, "count": null}).to_string();
            let _ = chat_tx.send(msg);
            let _ = app.emit("viewers:update", json!({"plateforme": nom, "count": null}));
        }
    }
}

/// (user_id, access) si Twitch connecté. Un lock par statement, un mutex
/// différent à chaque fois (règle mutex non réentrant).
fn twitch_creds(app: &AppHandle) -> Option<(String, String)> {
    let st = app.try_state::<crate::TwitchState>()?;
    let connected = *st.connected.lock().ok()?;
    if !connected {
        return None;
    }
    let uid = st.user_id.lock().ok()?.clone()?;
    let access = st.access.lock().ok()?.clone()?;
    Some((uid, access))
}

/// (channel_id, access) si YouTube connecté.
fn youtube_creds(app: &AppHandle) -> Option<(String, String)> {
    let st = app.try_state::<crate::YoutubeState>()?;
    let connected = *st.connected.lock().ok()?;
    if !connected {
        return None;
    }
    let cid = st.channel_id.lock().ok()?.clone()?;
    let access = st.access.lock().ok()?.clone()?;
    Some((cid, access))
}

/// slug du canal si Kick connecté.
fn kick_creds(app: &AppHandle) -> Option<String> {
    let st = app.try_state::<crate::KickState>()?;
    let connected = *st.connected.lock().ok()?;
    if !connected {
        return None;
    }
    let slug = st.slug.lock().ok()?.clone();
    slug
}

// =============================================================================
// COMMUNAUTÉ + OBJECTIFS (frames "communaute" + détection "goal-atteint")
// =============================================================================

/// Compteurs communauté d'une plateforme. Champs None = non disponibles
/// pour cette plateforme (absents de la frame — le widget affiche « N/A »).
#[derive(Clone, Default, PartialEq)]
struct Communaute {
    followers: Option<u64>,
    subs: Option<u64>,
    sub_points: Option<u64>,
    abonnes: Option<u64>,
}

/// Aplatit une Communaute en liste `(metrique, valeur)` pour verifier_objectifs
/// (les noms = valeurs possibles de `goalMetrique` côté widget).
fn metriques_commu(c: &Communaute) -> Vec<(&'static str, u64)> {
    let mut m = Vec::new();
    if let Some(v) = c.followers {
        m.push(("followers", v));
    }
    if let Some(v) = c.subs {
        m.push(("subs", v));
    }
    if let Some(v) = c.sub_points {
        m.push(("sub_points", v));
    }
    if let Some(v) = c.abonnes {
        m.push(("abonnes", v));
    }
    m
}

/// Pousse la frame `{"type":"communaute",...}` (dedup par tuple complet) +
/// emit `communaute:update` pour le dashboard.
fn publier_communaute(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    plat: &'static str,
    c: &Communaute,
    derniers: &mut HashMap<&'static str, Communaute>,
) {
    if derniers.get(plat) == Some(c) {
        return;
    }
    derniers.insert(plat, c.clone());
    let mut m = serde_json::Map::new();
    m.insert("type".into(), json!("communaute"));
    m.insert("plateforme".into(), json!(plat));
    if let Some(v) = c.followers {
        m.insert("followers".into(), json!(v));
    }
    if let Some(v) = c.subs {
        m.insert("subs".into(), json!(v));
    }
    if let Some(v) = c.sub_points {
        m.insert("sub_points".into(), json!(v));
    }
    if let Some(v) = c.abonnes {
        m.insert("abonnes".into(), json!(v));
    }
    let val = serde_json::Value::Object(m);
    let _ = chat_tx.send(val.to_string());
    let _ = app.emit("communaute:update", &val);
}

/// Ids des widgets "goal" ayant déjà célébré (réarmés quand le compteur
/// redescend sous la cible — unfollow/sub expiré : les objectifs Twitch
/// redescendent aussi, doc officielle Creator Goals).
fn objectifs_atteints() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Décision d'atteinte d'un widget goal — fonction pure testable :
/// true = émettre goal-atteint (crossing ≥ cible, une fois) ; la présence
/// dans `atteints` est gérée ici (retirée sous la cible → réarmement).
fn evaluer_objectif(atteints: &mut HashSet<String>, widget_id: &str, valeur: u64, cible: u64) -> bool {
    if valeur >= cible {
        atteints.insert(widget_id.to_string())
    } else {
        atteints.remove(widget_id);
        false
    }
}

/// Vérifie les widgets `goal` de la scène courante contre les mesures
/// fraîches `(metrique, valeur)` d'une plateforme. Au crossing ≥ cible →
/// frame `{"type":"goal-atteint",...}` sur chat_tx + `app.emit` (une fois
/// par widget ; réarmé sous la cible). Partagé par le tick (twitch/youtube/
/// kick) ET par tiktok_chat (RoomUserSeq) — détection unique quel que soit
/// le chemin de la donnée.
pub fn verifier_objectifs(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    plat: &str,
    metriques: &[(&str, u64)],
) {
    if metriques.is_empty() {
        return;
    }
    let Some(state) = app.try_state::<crate::scenes::ScenesState>() else {
        return;
    };
    let scene = state.scene.lock().unwrap().clone();
    let mut atteints = objectifs_atteints().lock().unwrap();
    // Nettoyage : ids de widgets goal supprimés → sortie du set.
    let ids_goal: HashSet<&str> = scene
        .widgets
        .iter()
        .filter(|w| w.widget_type == "goal")
        .map(|w| w.id.as_str())
        .collect();
    atteints.retain(|id| ids_goal.contains(id.as_str()));
    for w in &scene.widgets {
        if w.widget_type != "goal" {
            continue;
        }
        // Widget LIÉ à un Creator Goal Twitch : sa valeur vient des frames
        // goal-twitch (verifier_objectifs_goals), pas des métriques mesurées.
        if w.goalId.is_some() {
            continue;
        }
        let (Some(p), Some(m), Some(cible)) = (
            w.goalPlateforme.as_deref(),
            w.goalMetrique.as_deref(),
            w.goalCible,
        ) else {
            continue;
        };
        if p != plat {
            continue;
        }
        let Some(&(_, valeur)) = metriques.iter().find(|(nom, _)| *nom == m) else {
            continue;
        };
        if evaluer_objectif(&mut atteints, &w.id, valeur, cible) {
            let msg = json!({
                "type": "goal-atteint",
                "widget_id": w.id,
                "plateforme": plat,
                "metrique": m,
                "count": valeur,
                "cible": cible,
            });
            let _ = chat_tx.send(msg.to_string());
            let _ = app.emit("goal-atteint", &msg);
        }
    }
}

// =============================================================================
// CREATOR GOALS TWITCH (widgets goal LIÉS par goalId — frame "goal-twitch")
// =============================================================================

/// Dernières valeurs publiées par goal id (dedup par tuple complet, comme
/// `commus` — partagé entre le poll du tick ET les events EventSub).
fn goals_publies() -> &'static Mutex<HashMap<String, crate::twitch_helix::CreatorGoal>> {
    static S: OnceLock<Mutex<HashMap<String, crate::twitch_helix::CreatorGoal>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Envoie UNE frame `{"type":"goal-twitch",...}` sur chat_tx + emit miroir
/// dashboard. Pas de dedup ici (fait par l'appelant).
fn envoyer_goal(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    g: &crate::twitch_helix::CreatorGoal,
) {
    let msg = json!({
        "type": "goal-twitch",
        "id": g.id,
        "goal_type": g.goal_type,
        "description": g.description,
        "actuel": g.actuel,
        "cible": g.cible,
        "atteint": g.atteint,
        "termine": g.termine,
    });
    let _ = chat_tx.send(msg.to_string());
    let _ = app.emit("goal-twitch", &msg);
}

/// Un goal frais (poll ou EventSub) : frame si le tuple a changé (dedup par
/// la map partagée) puis mémorisation.
fn publier_un_goal(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    g: &crate::twitch_helix::CreatorGoal,
) {
    {
        let mut map = goals_publies().lock().unwrap();
        if map.get(&g.id) == Some(g) {
            return;
        }
        map.insert(g.id.clone(), g.clone());
    }
    envoyer_goal(app, chat_tx, g);
}

/// Lot de goals issu du GET /goals (ne retourne que les actifs) : publish
/// des changés + frame `termine:true` pour ceux qui ont disparu (fin du
/// goal ratée par EventSub, ou suppression dans le Creator Dashboard).
fn publier_goals(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    goals: &[crate::twitch_helix::CreatorGoal],
) {
    let mut map = goals_publies().lock().unwrap();
    let actifs: HashSet<&str> = goals.iter().map(|g| g.id.as_str()).collect();
    let termines: Vec<crate::twitch_helix::CreatorGoal> = map
        .iter()
        .filter(|(id, _)| !actifs.contains(id.as_str()))
        .map(|(_, g)| g.clone())
        .collect();
    for g in &termines {
        map.remove(&g.id);
    }
    for g in goals {
        if map.get(&g.id) == Some(g) {
            continue;
        }
        map.insert(g.id.clone(), g.clone());
    }
    drop(map); // hors du lock : envois WS/emit
    for mut g in termines {
        g.termine = true;
        envoyer_goal(app, chat_tx, &g);
    }
    for g in goals {
        envoyer_goal(app, chat_tx, g);
    }
}

/// Entrée EventSub (twitch_eventsub.rs) : un event channel.goal.begin/
/// progress/end → parse → frame goal-twitch + atteinte des widgets liés.
/// `termine` = true pour channel.goal.end (is_achieved = verdict final de
/// Twitch ; null sur les events begin/progress → dérivé actuel >= cible).
/// Non-fatal : event malformé ignoré (un goal cassé ne coupe pas la socket).
pub fn on_goal_event(app: &AppHandle, termine: bool, event: &serde_json::Value) {
    let Some(chat_tx) = app
        .try_state::<crate::scenes::ScenesState>()
        .map(|s| s.chat_tx.clone())
    else {
        return;
    };
    let (Some(id), Some(goal_type), Some(actuel), Some(cible)) = (
        event["id"].as_str(),
        event["type"].as_str(),
        event["current_amount"].as_u64(),
        event["target_amount"].as_u64(),
    ) else {
        return;
    };
    let g = crate::twitch_helix::CreatorGoal {
        id: id.to_string(),
        goal_type: goal_type.to_string(),
        description: event["description"].as_str().unwrap_or_default().to_string(),
        actuel,
        cible,
        atteint: event["is_achieved"].as_bool().unwrap_or(actuel >= cible),
        termine,
    };
    publier_un_goal(app, &chat_tx, &g);
    verifier_objectifs_goals(app, &chat_tx, &[g]);
}

/// Widgets `goal` LIÉS (goalId présent) : l'atteinte se lit sur le goal
/// Twitch lui-même (is_achieved / actuel>=cible — les régressions réarment,
/// même règle que le mode manuel). Clé d'atteinte `widget_id:goal_id` :
/// relier un AUTRE goal réarme la célébration naturellement.
pub fn verifier_objectifs_goals(
    app: &AppHandle,
    chat_tx: &broadcast::Sender<String>,
    goals: &[crate::twitch_helix::CreatorGoal],
) {
    if goals.is_empty() {
        return;
    }
    let Some(state) = app.try_state::<crate::scenes::ScenesState>() else {
        return;
    };
    let scene = state.scene.lock().unwrap().clone();
    let mut atteints = objectifs_atteints().lock().unwrap();
    // Nettoyage : clés de widgets goal supprimés → sortie du set. La clé liée
    // est "widget_id:goal_id" → on teste le préfixe avant ':'.
    let ids_goal: HashSet<&str> = scene
        .widgets
        .iter()
        .filter(|w| w.widget_type == "goal")
        .map(|w| w.id.as_str())
        .collect();
    atteints.retain(|k| ids_goal.contains(k.split(':').next().unwrap_or(k.as_str())));
    for w in &scene.widgets {
        if w.widget_type != "goal" {
            continue;
        }
        let Some(gid) = w.goalId.as_deref() else {
            continue;
        };
        let Some(g) = goals.iter().find(|g| g.id == gid) else {
            continue;
        };
        let cle = format!("{}:{}", w.id, g.id);
        if evaluer_objectif(&mut atteints, &cle, g.actuel, g.cible) {
            let msg = json!({
                "type": "goal-atteint",
                "widget_id": w.id,
                "plateforme": "twitch",
                "metrique": g.goal_type,
                "count": g.actuel,
                "cible": g.cible,
            });
            let _ = chat_tx.send(msg.to_string());
            let _ = app.emit("goal-atteint", &msg);
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Crossing ≥ cible → une seule émission ; au-dessus = rien ;
    /// sous la cible → réarmement (unfollow/sub expiré, doc Creator Goals).
    #[test]
    fn test_evaluer_objectif_crossing_rearme() {
        let mut atteints = HashSet::new();
        // Sous la cible → rien.
        assert!(!evaluer_objectif(&mut atteints, "w1", 9, 10));
        // Crossing → émission une fois.
        assert!(evaluer_objectif(&mut atteints, "w1", 10, 10));
        // Resté au-dessus → pas de répétition.
        assert!(!evaluer_objectif(&mut atteints, "w1", 11, 10));
        // Redescendu sous la cible → réarmé (toujours rien tant que < cible).
        assert!(!evaluer_objectif(&mut atteints, "w1", 8, 10));
        // Nouveau crossing → ré-émission.
        assert!(evaluer_objectif(&mut atteints, "w1", 10, 10));
    }

    /// metriques_commu n'aplatit que les champs présents (None = absent).
    #[test]
    fn test_metriques_commu_aplatissement() {
        let c = Communaute {
            followers: Some(42),
            subs: None,
            sub_points: Some(7),
            abonnes: None,
        };
        let m = metriques_commu(&c);
        assert_eq!(m, vec![("followers", 42), ("sub_points", 7)]);
        assert!(metriques_commu(&Communaute::default()).is_empty());
    }
}
