//! Analyse d'audience — reconstruction des sessions live depuis
//! `telemetrie/events.jsonl` (onglet « Audience » de la modale Diagnostic).
//!
//! Pipeline : parse JSONL (lignes cassées ignorées — writer crash-safe) →
//! groupe par `session_id` → intervalles « en live » (carry-forward du count
//! viewers, bornés par `stream-info en_live:false` / déconnexion / fin des
//! données) → rééchantillonnage grille 60 s → segments jeu → détection de
//! pics (baseline = médiane glissante des 15 points) → attribution de cause
//! → agrégats par jeu/titre/créneau.
//!
//! Tout le calcul est en fonctions pures sur &[EventTelemetrie] — la lecture
//! du fichier n'arrive qu'en entrée (`lire_events`) : testable sans IO.
//! Limites assumées : résolution 60 s (cadence Helix figée — un pic < 1-2 min
//! peut être invisible) et viewer_count Twitch approximatif.

use crate::telemetrie::modele::EventTelemetrie;
use chrono::{Datelike, Timelike};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

/// Pas de la grille de rééchantillonnage (1 pt/min — cadence fetch Helix).
const PAS_MS: i64 = 60_000;
/// Fenêtre de la baseline : médiane des N derniers points (de l'historique
/// disponible — 0 point → baseline = v(t), aucune détection possible).
const BASELINE_PTS: usize = 15;
/// Durée minimale d'un live pour entrer dans les stats (anti-bruit).
const MIN_SESSION_MS: i64 = 300_000;
/// Fenêtre d'attribution de cause autour du début du pic (ms).
const FENETRE_AVANT_MS: i64 = 300_000;
const FENETRE_APRES_MS: i64 = 120_000;
/// Un pic ouvrant ≤ 3 min après le début du live → cause "debut_live".
const DEBUT_LIVE_MS: i64 = 180_000;
/// Types comptés comme « effort communauté » (≥3 dans la fenêtre → cause).
const TYPES_COMMUNAUTE: &[&str] = &["bits", "sub", "resub", "subgift", "superchat", "gift"];

// ============================================================================
// STRUCTS PUBLIQUES (Serialize — contrat des commandes audience_*)
// ============================================================================

/// Un point de la série viewers rééchantillonnée (grille PAS_MS).
#[derive(Debug, Clone, Serialize)]
pub struct Point {
    /// Timestamp epoch ms.
    pub ts: i64,
    pub count: u32,
}

/// Plage de la série où le jeu (jeu_id) est constant.
#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub debut: i64,
    pub fin: i64,
    pub jeu: String,
    pub jeu_id: String,
    pub titre: String,
    pub moyenne: f64,
    pub pic: u32,
}

/// Pic de viewers détecté + cause attribuée.
#[derive(Debug, Clone, Serialize)]
pub struct Pic {
    /// Id de l'intervalle live (`session_id` ou `session_id#n`).
    pub session_id: String,
    pub t_debut: i64,
    pub t_pic: i64,
    pub t_fin: i64,
    /// Baseline à l'ouverture du pic (médiane des 15 points précédents).
    pub v_avant: u32,
    pub v_pic: u32,
    pub delta: i64,
    pub delta_pct: f64,
    /// Jeu/titre au sommet du pic.
    pub jeu: String,
    pub jeu_id: String,
    pub titre: String,
    /// "raid" | "contenu" | "communaute" | "debut_live" | "organique".
    pub cause: String,
    /// Pseudo à l'origine de la cause (raid) — vide sinon.
    pub cause_pseudo: String,
    /// Chiffre de la cause (viewers du raid / nb d'events communauté) — 0 sinon.
    pub cause_nb: i64,
    /// "certaine" | "probable" | "faible".
    pub confiance: String,
    /// Messages chat/min dans la fenêtre d'attribution (corroboration).
    pub chat_par_min: f64,
}

/// Event de la timeline d'une session (marqueur sur le graphe).
#[derive(Debug, Clone, Serialize)]
pub struct Evt {
    pub ts: i64,
    /// type_alerte pour les events, ou "stream-info".
    pub genre: String,
    /// Pseudo (events) ou jeu (stream-info) — affichage direct.
    pub detail: String,
    /// Chiffre utile (nb_viewers raid, nb_bits, nb_objet) — 0 sinon.
    pub nb: i64,
}

/// Résumé d'une session live pour le sélecteur + les cartes.
#[derive(Debug, Clone, Serialize)]
pub struct SessionResume {
    /// Id de l'intervalle : `session_id` ou `session_id#n` si la connexion
    /// a connu plusieurs lives (offline → live → offline → live).
    pub session_id: String,
    pub debut: i64,
    pub fin: i64,
    pub duree_s: u64,
    pub pic: u32,
    pub moyenne: f64,
    /// Jeu occupant le plus de temps pendant le live.
    pub jeu_principal: String,
    pub nb_msgs_chat: u64,
    /// genre (type_alerte) → nombre d'events pendant le live.
    pub nb_events_par_type: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatJeu {
    pub jeu: String,
    pub jeu_id: String,
    pub temps_s: u64,
    pub moyenne: f64,
    pub pic_max: u32,
    /// Somme des viewers × minutes de présence (1 point grille = 1 min).
    pub viewer_minutes: f64,
    pub nb_segments: u32,
    pub nb_pics: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatTitre {
    pub titre: String,
    pub temps_s: u64,
    pub moyenne: f64,
    pub pic_max: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatCreneau {
    /// Jour de semaine local : 0 = lundi … 6 = dimanche.
    pub jour: u8,
    /// Heure locale 0-23.
    pub heure: u8,
    pub moyenne: f64,
    pub echantillons: u64,
}

/// Totaux des events pendant les lives de la période (kind "event" —
/// pré-filtre, donc vérité terrain indépendante du moteur d'alertes).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Totaux {
    pub raids: u64,
    pub raid_viewers: u64,
    pub subs: u64,
    pub resubs: u64,
    pub subgifts: u64,
    pub bits_total: u64,
    pub follows: u64,
    pub supers: u64,
    pub gifts: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Agregats {
    pub top_jeux: Vec<StatJeu>,
    pub top_titres: Vec<StatTitre>,
    pub top_creneaux: Vec<StatCreneau>,
    pub top_pics: Vec<Pic>,
    pub totaux: Totaux,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudienceSessions {
    pub periode_jours: u32,
    pub sessions: Vec<SessionResume>,
    pub agregats: Agregats,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionDetail {
    pub meta: SessionResume,
    pub serie: Vec<Point>,
    pub segments: Vec<Segment>,
    pub pics: Vec<Pic>,
    pub timeline: Vec<Evt>,
}

// ============================================================================
// ENTRÉES PUBLIQUES
// ============================================================================

/// Sessions live reconstruites + agrégats sur `depuis_jours` jours.
pub fn audience_sessions(
    fichier: &Path,
    plateforme: &str,
    depuis_jours: u32,
) -> Result<AudienceSessions, String> {
    let depuis_ms =
        chrono::Utc::now().timestamp_millis() - (depuis_jours as i64) * 86_400_000;
    let evs = lire_events(fichier, plateforme, Some(depuis_ms))?;
    let sessions = reconstruire(&evs);
    let gardes: Vec<&SessionConstruite> = sessions.iter().filter(|s| garder(s)).collect();
    let mut resumes: Vec<SessionResume> = gardes.iter().map(|s| resume(s)).collect();
    resumes.sort_by_key(|r| std::cmp::Reverse(r.debut));
    let agregats = agregats(&gardes);
    Ok(AudienceSessions {
        periode_jours: depuis_jours,
        sessions: resumes,
        agregats,
    })
}

/// Détail complet d'une session. `id` = session_id, suffixe `#n` quand la
/// connexion a plusieurs lives (le résumé porte l'id complet — le retrouver
/// par comparaison directe, pas par index).
pub fn audience_session(
    fichier: &Path,
    plateforme: &str,
    id: &str,
) -> Result<SessionDetail, String> {
    let evs = lire_events(fichier, plateforme, None)?;
    let sessions = reconstruire(&evs);
    let s = sessions
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "Session introuvable".to_string())?;
    Ok(detail(s))
}

// ============================================================================
// LECTURE + RECONSTRUCTION
// ============================================================================

/// Parse `ts` RFC3339 → epoch ms.
fn ts_ms(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Parse le JSONL ligne à ligne — lignes cassées ignorées (le writer peut
/// laisser une ligne partielle après un crash). Fichier absent = pas encore
/// de télémétrie → liste vide, pas une erreur.
fn lire_events(
    fichier: &Path,
    plateforme: &str,
    depuis_ms: Option<i64>,
) -> Result<Vec<EventTelemetrie>, String> {
    let f = match std::fs::File::open(fichier) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Ouvrir {}: {}", fichier.display(), e)),
    };
    let mut out = Vec::new();
    for ligne in std::io::BufReader::new(f).lines() {
        let Ok(l) = ligne else { continue };
        let Ok(ev) = serde_json::from_str::<EventTelemetrie>(&l) else {
            continue;
        };
        if ev.plateforme != plateforme {
            continue;
        }
        if let Some(seuil) = depuis_ms {
            if ts_ms(&ev.ts).map(|t| t < seuil).unwrap_or(true) {
                continue;
            }
        }
        out.push(ev);
    }
    Ok(out)
}

/// Un point viewers brut (events "viewers" — émis au changement seulement).
struct CountPt {
    ts: i64,
    count: u32,
    /// Contexte estampillé à l'enregistrement (peut être vide sur de
    /// l'ancien data pré-stream-info).
    jeu: String,
    jeu_id: String,
    titre: String,
}

/// Event stream-info d'un intervalle.
struct InfoLive {
    ts: i64,
    en_live: bool,
    titre: String,
    jeu: String,
    jeu_id: String,
    /// started_at Helix (début du live côté Twitch) — recale t0 si antérieur.
    debut_ms: Option<i64>,
}

/// Intervalle live en cours de reconstruction.
struct Intervalle {
    t0: i64,
    t1: i64,
    counts: Vec<CountPt>,
    infos: Vec<InfoLive>,
    events: Vec<Evt>,
    chat_ts: Vec<i64>,
}

/// Intervalle finalisé : série rééchantillonnée + pics calculés.
struct SessionConstruite {
    /// Id exposé : session_id, ou "session_id#n" si plusieurs lives.
    id: String,
    t0: i64,
    t1: i64,
    /// (ts, count, jeu, jeu_id, titre) — grille PAS_MS.
    serie: Vec<(i64, u32, String, String, String)>,
    infos: Vec<InfoLive>,
    events: Vec<Evt>,
    chat_ts: Vec<i64>,
    pics: Vec<Pic>,
}

/// Regroupe les events par session_id (ordre fichier conservé — append-only
/// chronologique) et reconstruit les intervalles live de chacune.
fn reconstruire(evs: &[EventTelemetrie]) -> Vec<SessionConstruite> {
    let mut ordre: Vec<String> = Vec::new();
    let mut par_session: HashMap<String, Vec<&EventTelemetrie>> = HashMap::new();
    for ev in evs {
        let Some(sid) = &ev.session_id else { continue };
        par_session
            .entry(sid.clone())
            .or_insert_with(|| {
                ordre.push(sid.clone());
                Vec::new()
            })
            .push(ev);
    }
    let mut out = Vec::new();
    for sid in &ordre {
        out.extend(construire_session(sid, &par_session[sid]));
    }
    out
}

/// Reconstruit les intervalles « en live » d'une connexion (session_id).
///
/// `live(t)` = dernier count viewers non-null ET dernier stream-info ≠
/// en_live:false (borne — jamais un prérequis : de l'ancien data sans
/// stream-info reste analysable). Intervalle fermé par viewers null,
/// stream-info en_live:false, déconnexion ou fin des données.
fn construire_session(sid: &str, evs: &[&EventTelemetrie]) -> Vec<SessionConstruite> {
    let mut cur_count: Option<u32> = None;
    let mut si_live: Option<bool> = None;
    let mut wip: Option<Intervalle> = None;
    let mut bornes: Vec<Intervalle> = Vec::new();

    for ev in evs {
        let Some(ts) = ts_ms(&ev.ts) else { continue };
        match ev.kind.as_str() {
            "viewers" => cur_count = ev.data["count"].as_u64().map(|v| v as u32),
            "stream-info" => si_live = Some(ev.data["en_live"].as_bool().unwrap_or(false)),
            "deconnexion" => cur_count = None,
            _ => {}
        }
        let live = cur_count.is_some() && si_live != Some(false);
        // Bascule AVANT accumulation : l'event qui ouvre est inclus dans
        // l'intervalle, celui qui ferme en est exclu.
        if live && wip.is_none() {
            wip = Some(Intervalle {
                t0: ts,
                t1: ts,
                counts: Vec::new(),
                infos: Vec::new(),
                events: Vec::new(),
                chat_ts: Vec::new(),
            });
        } else if !live && wip.is_some() {
            let mut it = wip.take().unwrap();
            it.t1 = ts;
            bornes.push(it);
        }
        if let Some(it) = &mut wip {
            it.t1 = ts;
            match ev.kind.as_str() {
                "viewers" => {
                    if let Some(c) = ev.data["count"].as_u64() {
                        it.counts.push(CountPt {
                            ts,
                            count: c as u32,
                            jeu: ev.jeu.clone().unwrap_or_default(),
                            jeu_id: ev.jeu_id.clone().unwrap_or_default(),
                            titre: ev.titre_stream.clone().unwrap_or_default(),
                        });
                    }
                }
                "stream-info" => it.infos.push(InfoLive {
                    ts,
                    en_live: ev.data["en_live"].as_bool().unwrap_or(false),
                    titre: ev.data["titre"].as_str().unwrap_or_default().to_string(),
                    jeu: ev.data["jeu"].as_str().unwrap_or_default().to_string(),
                    jeu_id: ev.data["jeu_id"].as_str().unwrap_or_default().to_string(),
                    debut_ms: ev.data["debut"].as_str().and_then(ts_ms),
                }),
                "chat" => it.chat_ts.push(ts),
                "event" => it.events.push(Evt {
                    ts,
                    genre: ev.data["type_alerte"].as_str().unwrap_or_default().to_string(),
                    detail: ev.pseudo.clone().unwrap_or_default(),
                    nb: extraire_nb(&ev.data),
                }),
                _ => {}
            }
        }
    }
    if let Some(it) = wip {
        // Session encore ouverte en fin de données → t1 = dernier ts vu
        // (déjà posé dans la boucle d'accumulation).
        bornes.push(it);
    }

    let n = bornes.len();
    bornes
        .into_iter()
        .enumerate()
        .map(|(i, it)| {
            let id = if n > 1 {
                format!("{}#{}", sid, i + 1)
            } else {
                sid.to_string()
            };
            finaliser(id, it)
        })
        .collect()
}

/// Chiffre utile d'un event : nb_viewers (raid) sinon nb_bits sinon nb_objet.
/// 0 = absent (les champs non pertinents sont à 0 dans EventTwitch).
fn extraire_nb(data: &Value) -> i64 {
    data["nb_viewers"]
        .as_i64()
        .filter(|&v| v != 0)
        .or_else(|| data["nb_bits"].as_i64().filter(|&v| v != 0))
        .or_else(|| data["nb_objet"].as_i64().filter(|&v| v != 0))
        .unwrap_or(0)
}

/// Rééchantillonne l'intervalle sur la grille PAS_MS (carry-forward : valeur
/// = dernier count connu ≤ g — les events viewers n'arrivent qu'au
/// changement) + contexte jeu par point + détection des pics.
fn finaliser(id: String, it: Intervalle) -> SessionConstruite {
    // Recale t0 sur started_at Helix si antérieur au 1er event (le 1er fetch
    // post-live arrive jusqu'à ~60 s après le vrai début du stream).
    let mut t0 = it.t0;
    if let Some(i) = it.infos.iter().find(|i| i.en_live) {
        if let Some(d) = i.debut_ms {
            if d < t0 {
                t0 = d;
            }
        }
    }

    let mut serie: Vec<(i64, u32, String, String, String)> = Vec::new();
    let mut idx = 0usize;
    let mut g = t0;
    while g <= it.t1 {
        while idx + 1 < it.counts.len() && it.counts[idx + 1].ts <= g {
            idx += 1;
        }
        if idx < it.counts.len() && it.counts[idx].ts <= g {
            let c = &it.counts[idx];
            let (jeu, jeu_id, titre) = contexte_a(&it, g, c);
            serie.push((g, c.count, jeu, jeu_id, titre));
        }
        g += PAS_MS;
    }

    let mut s = SessionConstruite {
        id,
        t0,
        t1: it.t1,
        serie,
        infos: it.infos,
        events: it.events,
        chat_ts: it.chat_ts,
        pics: Vec::new(),
    };
    s.pics = detecter_pics(&s);
    s
}

/// Jeu/titre d'un point : stamps de l'event viewers qui fournit le count
/// (remplis par l'estampillage stream-info), sinon dernier stream-info
/// en_live ≤ g (couvre l'ancien data partiellement stampillé).
fn contexte_a(it: &Intervalle, g: i64, c: &CountPt) -> (String, String, String) {
    if !c.jeu_id.is_empty() || !c.jeu.is_empty() {
        return (c.jeu.clone(), c.jeu_id.clone(), c.titre.clone());
    }
    let mut ctx = (String::new(), String::new(), String::new());
    for i in &it.infos {
        if i.ts > g {
            break;
        }
        if i.en_live {
            ctx = (i.jeu.clone(), i.jeu_id.clone(), i.titre.clone());
        }
    }
    ctx
}

/// Garde anti-bruit : < 2 points ou < MIN_SESSION_MS → hors stats.
fn garder(s: &SessionConstruite) -> bool {
    s.serie.len() >= 2 && s.t1 - s.t0 >= MIN_SESSION_MS
}

// ============================================================================
// DÉTECTION DE PICS + ATTRIBUTION
// ============================================================================

/// Médiane d'un échantillon (robuste aux pics — pas d'EMA qui traîne après
/// un raid).
fn mediane(hist: &[u32]) -> f64 {
    let mut v = hist.to_vec();
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2] as f64
    } else {
        (v[n / 2 - 1] as f64 + v[n / 2] as f64) / 2.0
    }
}

/// Pic en cours d'ouverture/suivi.
struct PicWip {
    t_debut: i64,
    /// Baseline à l'ouverture (f64 pour les seuils, arrondie à la sortie).
    v_avant: f64,
    v_pic: u32,
    t_pic: i64,
    jeu: String,
    jeu_id: String,
    titre: String,
    /// Points consécutifs sous le seuil de fermeture.
    sous: u32,
}

/// Détection : ouverture si v ≥ max(1.25×b, b+8) ≥2 points consécutifs ;
/// fermeture si v < 1.10×b pendant 2 points (hystérésis — un creux sous
/// 1.25×b mais ≥1.10×b ne ferme pas). Retenu si delta ≥ 10 ou ≥ 30 %.
fn detecter_pics(s: &SessionConstruite) -> Vec<Pic> {
    let mut pics = Vec::new();
    let mut ouvert: Option<PicWip> = None;
    let mut au_dessus = 0u32;
    let mut premier_ts = 0i64;
    let mut premier_base = 0.0f64;

    for (i, p) in s.serie.iter().enumerate() {
        let (ts, v, jeu, jeu_id, titre) = p;
        let debut = i.saturating_sub(BASELINE_PTS);
        let hist: Vec<u32> = s.serie[debut..i].iter().map(|q| q.1).collect();
        let b = if hist.is_empty() {
            *v as f64
        } else {
            mediane(&hist)
        };
        let vf = *v as f64;

        if let Some(w) = &mut ouvert {
            if *v > w.v_pic {
                w.v_pic = *v;
                w.t_pic = *ts;
                w.jeu = jeu.clone();
                w.jeu_id = jeu_id.clone();
                w.titre = titre.clone();
            }
            if vf < b * 1.10 {
                w.sous += 1;
                if w.sous >= 2 {
                    let w = ouvert.take().unwrap();
                    fermer_pic(s, w, *ts, &mut pics);
                }
            } else {
                w.sous = 0;
            }
            continue;
        }

        if vf >= (b * 1.25).max(b + 8.0) {
            if au_dessus == 0 {
                premier_ts = *ts;
                premier_base = b;
            }
            au_dessus += 1;
            if au_dessus >= 2 {
                ouvert = Some(PicWip {
                    t_debut: premier_ts,
                    v_avant: premier_base,
                    v_pic: *v,
                    t_pic: *ts,
                    jeu: jeu.clone(),
                    jeu_id: jeu_id.clone(),
                    titre: titre.clone(),
                    sous: 0,
                });
                au_dessus = 0;
            }
        } else {
            au_dessus = 0;
        }
    }
    // Pic encore ouvert en fin de série → fermé au dernier point.
    if let (Some(w), Some(last)) = (ouvert, s.serie.last()) {
        fermer_pic(s, w, last.0, &mut pics);
    }
    pics
}

/// Ferme un pic : filtre de retenue puis attribution de la cause.
fn fermer_pic(s: &SessionConstruite, w: PicWip, t_fin: i64, pics: &mut Vec<Pic>) {
    let v_avant = w.v_avant.round() as u32;
    let delta = w.v_pic as i64 - v_avant as i64;
    let delta_pct = if v_avant > 0 {
        delta as f64 / v_avant as f64 * 100.0
    } else {
        0.0
    };
    if delta < 10 && delta_pct < 30.0 {
        return;
    }
    let (cause, cause_pseudo, cause_nb, confiance, chat_par_min) =
        attribuer(s, w.t_debut, delta);
    pics.push(Pic {
        session_id: s.id.clone(),
        t_debut: w.t_debut,
        t_pic: w.t_pic,
        t_fin,
        v_avant,
        v_pic: w.v_pic,
        delta,
        delta_pct,
        jeu: w.jeu,
        jeu_id: w.jeu_id,
        titre: w.titre,
        cause,
        cause_pseudo,
        cause_nb,
        confiance,
        chat_par_min,
    });
}

/// Attribution de la cause — fenêtre [t_debut−300 s, t_debut+120 s], règles
/// ordonnées (la 1re qui matche gagne). Retourne
/// (cause, cause_pseudo, cause_nb, confiance, chat_par_min).
fn attribuer(
    s: &SessionConstruite,
    t_debut: i64,
    delta: i64,
) -> (String, String, i64, String, f64) {
    let w0 = t_debut - FENETRE_AVANT_MS;
    let w1 = t_debut + FENETRE_APRES_MS;
    let cpm = chat_par_min(s, w0, w1);

    // 1. Raid entrant — le chiffre nb_viewers corrobore la confiance.
    for e in &s.events {
        if e.ts < w0 || e.ts > w1 || e.genre != "raid" {
            continue;
        }
        let confiance = if e.nb as f64 >= 0.7 * delta as f64 {
            "certaine"
        } else {
            "probable"
        };
        return (
            "raid".into(),
            e.detail.clone(),
            e.nb,
            confiance.into(),
            cpm,
        );
    }
    // 2. Changement de contenu — stream-info en_live dans la fenêtre, hors
    // info d'ouverture du live (≈t0 — c'est le contexte initial, pas un
    // changement).
    for i in &s.infos {
        if i.ts < w0 || i.ts > w1 || !i.en_live || i.ts <= s.t0 + FENETRE_APRES_MS {
            continue;
        }
        return ("contenu".into(), String::new(), 0, "probable".into(), cpm);
    }
    // 3. Effort communauté : ≥3 events sub/bits/gift… dans la fenêtre.
    let nb_com = s
        .events
        .iter()
        .filter(|e| {
            e.ts >= w0 && e.ts <= w1 && TYPES_COMMUNAUTE.contains(&e.genre.as_str())
        })
        .count();
    if nb_com >= 3 {
        return (
            "communaute".into(),
            String::new(),
            nb_com as i64,
            "probable".into(),
            cpm,
        );
    }
    // 4. Début de live (afflux naturel des notifications « en ligne »).
    if t_debut - s.t0 <= DEBUT_LIVE_MS {
        return ("debut_live".into(), String::new(), 0, "probable".into(), cpm);
    }
    ("organique".into(), String::new(), 0, "faible".into(), cpm)
}

/// Messages chat/minute dans la fenêtre d'attribution.
fn chat_par_min(s: &SessionConstruite, w0: i64, w1: i64) -> f64 {
    let n = s.chat_ts.iter().filter(|&&t| t >= w0 && t <= w1).count() as f64;
    n / ((w1 - w0) as f64 / 60_000.0)
}

// ============================================================================
// SORTIES (resume / segments / timeline / agrégats)
// ============================================================================

/// Le live entre-t-il dans les stats ? (anti-bruit — voir `garder`).
fn resume(s: &SessionConstruite) -> SessionResume {
    let n = s.serie.len();
    let (somme, pic) = s
        .serie
        .iter()
        .fold((0u64, 0u32), |(sm, mx), p| (sm + p.1 as u64, mx.max(p.1)));
    // Jeu principal = jeu_id avec le plus de points (temps de présence).
    let mut par_jeu: HashMap<&str, u64> = HashMap::new();
    for p in &s.serie {
        *par_jeu.entry(p.2.as_str()).or_insert(0) += 1;
    }
    let jeu_principal = par_jeu
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(j, _)| j.to_string())
        .unwrap_or_default();
    let mut par_type: HashMap<String, u64> = HashMap::new();
    for e in &s.events {
        *par_type.entry(e.genre.clone()).or_insert(0) += 1;
    }
    SessionResume {
        session_id: s.id.clone(),
        debut: s.t0,
        fin: s.t1,
        duree_s: ((s.t1 - s.t0).max(0) / 1000) as u64,
        pic,
        moyenne: if n > 0 { somme as f64 / n as f64 } else { 0.0 },
        jeu_principal,
        nb_msgs_chat: s.chat_ts.len() as u64,
        nb_events_par_type: par_type,
    }
}

/// Découpe la série en segments à jeu_id constant.
fn segments(serie: &[(i64, u32, String, String, String)]) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    // Accumulateur interne (somme + n pour la moyenne).
    let mut somme = 0f64;
    let mut n = 0u64;
    for p in serie {
        let cont = out.last().map(|s| s.jeu_id == p.3).unwrap_or(false);
        if !cont {
            out.push(Segment {
                debut: p.0,
                fin: p.0 + PAS_MS,
                jeu: p.2.clone(),
                jeu_id: p.3.clone(),
                titre: p.4.clone(),
                moyenne: 0.0,
                pic: p.1,
            });
            somme = 0.0;
            n = 0;
        }
        somme += p.1 as f64;
        n += 1;
        let s = out.last_mut().unwrap();
        s.fin = p.0 + PAS_MS;
        s.pic = s.pic.max(p.1);
        s.moyenne = somme / n as f64;
    }
    out
}

/// Timeline affichable : events (type_alerte) + marqueurs stream-info.
fn timeline(s: &SessionConstruite) -> Vec<Evt> {
    let mut t: Vec<Evt> = s.events.clone();
    for i in &s.infos {
        t.push(Evt {
            ts: i.ts,
            genre: "stream-info".into(),
            detail: if i.en_live { i.jeu.clone() } else { String::new() },
            nb: 0,
        });
    }
    t.sort_by_key(|e| e.ts);
    t
}

/// Détail complet d'une session (graphe + timeline + pics).
fn detail(s: SessionConstruite) -> SessionDetail {
    SessionDetail {
        meta: resume(&s),
        serie: s.serie.iter().map(|p| Point { ts: p.0, count: p.1 }).collect(),
        segments: segments(&s.serie),
        timeline: timeline(&s),
        pics: s.pics,
    }
}

/// Agrégats sur toutes les sessions gardées de la période.
fn agregats(sessions: &[&SessionConstruite]) -> Agregats {
    // jeu_id → (jeu, points, somme, pic_max, segments, pics)
    let mut jeux: HashMap<String, (String, u64, u64, u32, u32, u32)> = HashMap::new();
    // titre → (points, somme, pic_max)
    let mut titres: HashMap<String, (u64, u64, u32)> = HashMap::new();
    // (jour, heure) locaux → (echantillons, somme)
    let mut creneaux: HashMap<(u8, u8), (u64, u64)> = HashMap::new();
    let mut totaux = Totaux::default();
    let mut all_pics: Vec<Pic> = Vec::new();

    for s in sessions {
        for seg in segments(&s.serie) {
            jeux.entry(seg.jeu_id.clone())
                .or_insert_with(|| (seg.jeu.clone(), 0, 0, 0, 0, 0))
                .4 += 1;
        }
        for p in &s.serie {
            let j = jeux
                .entry(p.3.clone())
                .or_insert_with(|| (p.2.clone(), 0, 0, 0, 0, 0));
            j.1 += 1;
            j.2 += p.1 as u64;
            j.3 = j.3.max(p.1);
            let t = titres.entry(p.4.clone()).or_insert((0, 0, 0));
            t.0 += 1;
            t.1 += p.1 as u64;
            t.2 = t.2.max(p.1);
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(p.0) {
                let lt = dt.with_timezone(&chrono::Local);
                let k = (lt.weekday().num_days_from_monday() as u8, lt.hour() as u8);
                let c = creneaux.entry(k).or_insert((0, 0));
                c.0 += 1;
                c.1 += p.1 as u64;
            }
        }
        for e in &s.events {
            match e.genre.as_str() {
                "raid" => {
                    totaux.raids += 1;
                    totaux.raid_viewers += e.nb.max(0) as u64;
                }
                "sub" => totaux.subs += 1,
                "resub" => totaux.resubs += 1,
                "subgift" => totaux.subgifts += 1,
                "bits" => totaux.bits_total += e.nb.max(0) as u64,
                "follow" => totaux.follows += 1,
                "superchat" => totaux.supers += 1,
                "gift" => totaux.gifts += 1,
                _ => {}
            }
        }
        for p in &s.pics {
            jeux.entry(p.jeu_id.clone())
                .or_insert_with(|| (p.jeu.clone(), 0, 0, 0, 0, 0))
                .5 += 1;
        }
        all_pics.extend(s.pics.iter().cloned());
    }

    let mut top_jeux: Vec<StatJeu> = jeux
        .into_iter()
        .map(|(jeu_id, (jeu, pts, somme, pic_max, nb_segments, nb_pics))| StatJeu {
            jeu,
            jeu_id,
            temps_s: pts * (PAS_MS as u64 / 1000),
            moyenne: if pts > 0 { somme as f64 / pts as f64 } else { 0.0 },
            pic_max,
            viewer_minutes: somme as f64,
            nb_segments,
            nb_pics,
        })
        .collect();
    top_jeux.sort_by(|a, b| b.viewer_minutes.total_cmp(&a.viewer_minutes));
    top_jeux.truncate(10);

    let mut top_titres: Vec<StatTitre> = titres
        .into_iter()
        .map(|(titre, (pts, somme, pic_max))| StatTitre {
            titre,
            temps_s: pts * (PAS_MS as u64 / 1000),
            moyenne: if pts > 0 { somme as f64 / pts as f64 } else { 0.0 },
            pic_max,
        })
        .collect();
    top_titres.sort_by(|a, b| b.moyenne.total_cmp(&a.moyenne));
    top_titres.truncate(10);

    let mut top_creneaux: Vec<StatCreneau> = creneaux
        .into_iter()
        .filter(|(_, (n, _))| *n >= 3)
        .map(|((jour, heure), (n, somme))| StatCreneau {
            jour,
            heure,
            moyenne: somme as f64 / n as f64,
            echantillons: n,
        })
        .collect();
    top_creneaux.sort_by(|a, b| b.moyenne.total_cmp(&a.moyenne));
    top_creneaux.truncate(8);

    all_pics.sort_by_key(|p| std::cmp::Reverse(p.delta));
    all_pics.truncate(10);

    Agregats {
        top_jeux,
        top_titres,
        top_creneaux,
        top_pics: all_pics,
        totaux,
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ts(t: i64) -> String {
        chrono::DateTime::from_timestamp_millis(t)
            .unwrap()
            .to_rfc3339()
    }

    fn ev(t: i64, sid: &str, kind: &str, data: Value) -> EventTelemetrie {
        EventTelemetrie {
            v: 1,
            ts: ts(t),
            plateforme: "twitch".into(),
            kind: kind.into(),
            session_id: Some(sid.into()),
            titre_stream: None,
            jeu: None,
            jeu_id: None,
            pseudo: None,
            data,
        }
    }

    fn ev_evt(t: i64, sid: &str, type_alerte: &str, pseudo: &str, data: Value) -> EventTelemetrie {
        let mut e = ev(t, sid, "event", {
            let mut d = data;
            d["type_alerte"] = json!(type_alerte);
            d
        });
        e.pseudo = Some(pseudo.into());
        e
    }

    /// Série viewers : (offset_s, count) — null = Option::None.
    fn evs_viewers(sid: &str, pts: &[(i64, Option<u32>)]) -> Vec<EventTelemetrie> {
        pts.iter()
            .map(|(o, c)| ev(o * 1000, sid, "viewers", json!({"count": c})))
            .collect()
    }

    #[test]
    fn carry_forward_et_bornes() {
        // Count émis au changement seulement → carry-forward sur la grille.
        let evs = evs_viewers(
            "s1",
            &[(0, Some(10)), (300, Some(20)), (600, None)],
        );
        let sessions = reconstruire(&evs);
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        // Grille 0..=600 step 60 → 11 points ; 0-4 → 10, 5-10 → 20.
        assert_eq!(s.serie.len(), 11);
        assert_eq!(s.serie[0].1, 10);
        assert_eq!(s.serie[4].1, 10);
        assert_eq!(s.serie[5].1, 20);
        assert_eq!(s.serie[10].1, 20);
    }

    #[test]
    fn session_trop_courte_ignoree() {
        let evs = evs_viewers("s1", &[(0, Some(10)), (60, None)]);
        let sessions = reconstruire(&evs);
        assert_eq!(sessions.len(), 1);
        assert!(!garder(&sessions[0])); // 60 s < 5 min
    }

    #[test]
    fn multi_lives_meme_connexion() {
        // Deux lives dans la même session_id → ids suffixés.
        let evs = evs_viewers(
            "s1",
            &[
                (0, Some(10)),
                (600, None),
                (900, Some(15)),
                (1500, None),
            ],
        );
        let sessions = reconstruire(&evs);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "s1#1");
        assert_eq!(sessions[1].id, "s1#2");
    }

    #[test]
    fn mediane_baseline() {
        assert_eq!(mediane(&[10; 15]), 10.0);
        assert_eq!(mediane(&[1, 2, 3, 4]), 2.5);
        // Robuste au pic : un 60 parmi des 10 ne bouge pas la médiane.
        assert_eq!(mediane(&[10, 10, 10, 10, 10, 10, 60]), 10.0);
    }

    #[test]
    fn pic_detecte_organique() {
        // 20 min à 10, pic à 60 pendant 5 min, retour à 10.
        let mut pts: Vec<(i64, Option<u32>)> = vec![(0, Some(10))];
        pts.push((20 * 60, Some(60)));
        pts.push((25 * 60, Some(10)));
        pts.push((35 * 60, None));
        let sessions = reconstruire(&evs_viewers("s1", &pts));
        let s = &sessions[0];
        assert_eq!(s.pics.len(), 1);
        let p = &s.pics[0];
        assert_eq!(p.v_avant, 10);
        assert_eq!(p.v_pic, 60);
        assert_eq!(p.delta, 50);
        assert_eq!(p.t_debut, 20 * 60_000);
        assert_eq!(p.cause, "organique");
        assert_eq!(p.confiance, "faible");
    }

    #[test]
    fn pic_ignore_bruit() {
        // +6 au-dessus de 1.25× mais < +8 → seuil ouverture = 10+8=18 →
        // 16 n'ouvre pas.
        let mut pts: Vec<(i64, Option<u32>)> = vec![(0, Some(10))];
        pts.push((20 * 60, Some(16)));
        pts.push((25 * 60, Some(10)));
        pts.push((35 * 60, None));
        let sessions = reconstruire(&evs_viewers("s1", &pts));
        assert_eq!(sessions[0].pics.len(), 0);
    }

    #[test]
    fn attribution_raid() {
        let mut evs = vec![ev(0, "s1", "viewers", json!({"count": 10}))];
        // Raid à 19:30 (dans la fenêtre t_debut−300s).
        evs.push(ev_evt(
            19 * 60_000 + 30_000,
            "s1",
            "raid",
            "Raideur",
            json!({"nb_viewers": 55}),
        ));
        evs.push(ev(20 * 60_000, "s1", "viewers", json!({"count": 60})));
        evs.push(ev(25 * 60_000, "s1", "viewers", json!({"count": 10})));
        evs.push(ev(35 * 60_000, "s1", "viewers", json!({"count": null})));
        let sessions = reconstruire(&evs);
        let p = &sessions[0].pics[0];
        assert_eq!(p.cause, "raid");
        assert_eq!(p.cause_pseudo, "Raideur");
        assert_eq!(p.cause_nb, 55);
        // 55 ≥ 0.7×50 → certaine.
        assert_eq!(p.confiance, "certaine");
    }

    #[test]
    fn attribution_contenu() {
        let mut evs = vec![ev(0, "s1", "viewers", json!({"count": 10}))];
        // Stream-info d'ouverture (≈t0) puis changement de jeu à 19 min.
        evs.push(ev(
            0,
            "s1",
            "stream-info",
            json!({"en_live": true, "titre": "T", "jeu": "A", "jeu_id": "1", "debut": "", "tags": []}),
        ));
        evs.push(ev(
            19 * 60_000,
            "s1",
            "stream-info",
            json!({"en_live": true, "titre": "T", "jeu": "B", "jeu_id": "2", "debut": "", "tags": []}),
        ));
        evs.push(ev(20 * 60_000, "s1", "viewers", json!({"count": 60})));
        evs.push(ev(25 * 60_000, "s1", "viewers", json!({"count": 10})));
        evs.push(ev(35 * 60_000, "s1", "viewers", json!({"count": null})));
        let sessions = reconstruire(&evs);
        let p = &sessions[0].pics[0];
        assert_eq!(p.cause, "contenu");
        assert_eq!(p.confiance, "probable");
    }

    #[test]
    fn attribution_debut_live() {
        // Pic 2 min après le début du live → debut_live.
        let evs = evs_viewers(
            "s1",
            &[(0, Some(10)), (120, Some(50)), (400, Some(10)), (600, None)],
        );
        let sessions = reconstruire(&evs);
        let p = &sessions[0].pics[0];
        assert_eq!(p.cause, "debut_live");
    }

    #[test]
    fn segments_jeu() {
        // Viewers stampillés jeu A puis jeu B → 2 segments.
        let mut e1 = ev(0, "s1", "viewers", json!({"count": 10}));
        e1.jeu_id = Some("1".into());
        e1.jeu = Some("A".into());
        e1.titre_stream = Some("T".into());
        let mut e2 = ev(300_000, "s1", "viewers", json!({"count": 20}));
        e2.jeu_id = Some("2".into());
        e2.jeu = Some("B".into());
        e2.titre_stream = Some("T".into());
        let e3 = ev(600_000, "s1", "viewers", json!({"count": null}));
        let sessions = reconstruire(&[e1, e2, e3]);
        let segs = segments(&sessions[0].serie);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].jeu, "A");
        assert_eq!(segs[0].moyenne, 10.0);
        assert_eq!(segs[1].jeu, "B");
        assert_eq!(segs[1].moyenne, 20.0);
    }

    /// Smoke test sur le vrai `events.jsonl` : lancer avec
    /// `SOS_JSONL_REEL=<chemin> cargo test smoke_jsonl_reel -- --nocapture`.
    /// Ignoré sans la variable — ne casse jamais la CI.
    #[test]
    fn smoke_jsonl_reel() {
        let Ok(chemin) = std::env::var("SOS_JSONL_REEL") else {
            return;
        };
        let f = Path::new(&chemin);
        let r = audience_sessions(f, "twitch", 90).expect("analyse sessions");
        eprintln!(
            "smoke: {} session(s), {} pics total",
            r.sessions.len(),
            r.agregats.top_pics.len()
        );
        for s in &r.sessions {
            eprintln!(
                "  {} | {} pts live, moy {:.1}, pic {}, jeu {:?}",
                s.session_id, s.duree_s, s.moyenne, s.pic, s.jeu_principal
            );
            let d = audience_session(f, "twitch", &s.session_id).expect("détail");
            eprintln!(
                "    serie {} pts, {} segments, {} pics, {} events",
                d.serie.len(),
                d.segments.len(),
                d.pics.len(),
                d.timeline.len()
            );
        }
    }
}
