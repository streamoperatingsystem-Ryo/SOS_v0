//! Télémétrie multiplateforme — collecte passive, normalisée, horodatée.
//!
//! Consommateur PASSIF des flux existants : listeners Tauri sur les events
//! `*:connecte` / `*:deconnecte` + subscription du canal `chat_tx` (whitelist
//! fermée de types de frames). AUCUN module émetteur n'est modifié — la
//! télémétrie ne peut ni ralentir ni casser le chat / la diffusion.
//!
//! Persistance : JSONL append-only `telemetrie/events.jsonl` (thread writer
//! dédié, flush 500 ms ou 64 lignes). En mémoire : compteurs par kind /
//! plateforme, sessions courantes et anneau des derniers événements.
//!
//! LOT 1 : collecte (connexions, chat, viewers, plays) + `telemetrie_etat` /
//! `telemetrie_derniers`. LOT 2 : enrichissement titre/jeu + stats.

pub mod analyse;
pub mod collecte;
pub mod commands;
pub mod modele;
pub mod stockage;

use modele::EventTelemetrie;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use tauri::AppHandle;
use tokio::sync::broadcast;

/// Capacité de l'anneau des derniers événements en mémoire (telemetrie_derniers).
const DERNIERS_CAP: usize = 500;

/// État partagé du module (tauri::State). `tx` = canal non borné vers le
/// writer JSONL — `send` ne bloque jamais le producteur.
#[derive(Clone)]
pub struct TelemetrieState {
    inner: Arc<Mutex<TelemetrieInner>>,
    tx: mpsc::Sender<String>,
    fichier: Arc<PathBuf>,
}

/// Contexte du live en cours d'une plateforme — posé par les events
/// `stream-info` (viewers.rs, émis au changement titre/jeu), retiré à
/// `en_live:false` ou à la déconnexion. Sert à estampiller
/// `titre_stream`/`jeu`/`jeu_id` sur tous les events de la plateforme.
struct ContexteLive {
    titre: String,
    jeu: String,
    jeu_id: String,
}

#[derive(Default)]
struct TelemetrieInner {
    /// plateforme → session_id courante (posée à `*:connecte`, retirée à
    /// `*:deconnecte`). Fenêtre connexion → déconnexion = une session.
    sessions: HashMap<String, String>,
    /// plateforme → contexte live courant (stream-info).
    contextes: HashMap<String, ContexteLive>,
    par_kind: HashMap<String, u64>,
    par_plateforme: HashMap<String, u64>,
    total: u64,
    dernier_ts: Option<String>,
    derniers: VecDeque<EventTelemetrie>,
}

impl TelemetrieState {
    /// Crée l'état : résout `telemetrie/events.jsonl` + démarre le writer.
    /// Erreur uniquement si le dossier de données est inaccessible — l'app
    /// continue sans télémétrie dans ce cas (init non-fatal dans setup).
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let fichier = stockage::fichier_events(app)?;
        let tx = stockage::demarrer_writer(fichier.clone());
        Ok(Self {
            inner: Arc::new(Mutex::new(TelemetrieInner::default())),
            tx,
            fichier: Arc::new(fichier),
        })
    }

    /// Enregistre les listeners + la subscription chat_tx. Une fois, dans setup,
    /// AVANT les auto-resume pour capter leurs connexions.
    pub fn demarrer(&self, app: &AppHandle, chat_tx: broadcast::Sender<String>) {
        collecte::demarrer(app, self.clone(), chat_tx);
    }

    /// `*:connecte` → ouvre une session (uuid) + event connexion.
    fn connexion(&self, plateforme: &str) {
        let sid = uuid::Uuid::new_v4().to_string();
        self.inner
            .lock()
            .unwrap()
            .sessions
            .insert(plateforme.to_string(), sid.clone());
        self.record(EventTelemetrie {
            plateforme: plateforme.to_string(),
            kind: "connexion".into(),
            session_id: Some(sid),
            ..EventTelemetrie::nouveau()
        });
    }

    /// `*:deconnecte` → event deconnexion (avec la session fermée si connue).
    /// Coupe aussi le contexte live : plus de stream → plus d'estampillage.
    fn deconnexion(&self, plateforme: &str) {
        let sid = {
            let mut g = self.inner.lock().unwrap();
            g.contextes.remove(plateforme);
            g.sessions.remove(plateforme)
        };
        self.record(EventTelemetrie {
            plateforme: plateforme.to_string(),
            kind: "deconnexion".into(),
            session_id: sid,
            ..EventTelemetrie::nouveau()
        });
    }

    /// Dispatch d'une frame brute chat_tx (tâche subscriber de collecte.rs).
    /// Frames hors whitelist ignorées (bruit de synchro UI).
    fn on_frame(&self, raw: &str) {
        let frame: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(_) => return,
        };
        let t = frame
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let kind = match collecte::kind_depuis_frame(t) {
            Some(k) => k,
            None => return,
        };
        let plateforme = collecte::plateforme_frame(kind, &frame);
        let (pseudo, data) = collecte::extraire(kind, &frame);
        let (session_id, titre_stream, jeu, jeu_id) =
            self.preparer_event(&plateforme, kind, &data);
        self.record(EventTelemetrie {
            plateforme,
            kind: kind.to_string(),
            session_id,
            titre_stream,
            jeu,
            jeu_id,
            pseudo,
            data,
            ..EventTelemetrie::nouveau()
        });
    }

    /// `chat:event` (EventTwitch sérialisé, émis AVANT le filtre alertes) →
    /// kind "event". Conserve les chiffres que le kind "alerte" perd :
    /// nb_viewers des raids, nb_bits, montant, objet… Twitch-seul
    /// aujourd'hui (les autres plateformes n'émettent pas chat:event).
    fn on_chat_event(&self, payload: &str) {
        let evt: Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => return,
        };
        let plateforme = evt["plateforme"]
            .as_str()
            .unwrap_or("twitch")
            .to_string();
        let pseudo = evt["pseudo"].as_str().map(str::to_string);
        let data = serde_json::json!({
            "type_alerte": evt["type_alerte"],
            "nb_viewers": evt["nb_viewers"],
            "nb_bits": evt["nb_bits"],
            "montant": evt["montant"],
            "objet": evt["objet"],
            "nb_objet": evt["nb_objet"],
            "user_id": evt["user_id"],
            "niveau_sub": evt["niveau_sub"],
            "nb_mois": evt["nb_mois"],
        });
        let (session_id, titre_stream, jeu, jeu_id) =
            self.preparer_event(&plateforme, "event", &data);
        self.record(EventTelemetrie {
            plateforme,
            kind: "event".into(),
            session_id,
            titre_stream,
            jeu,
            jeu_id,
            pseudo,
            data,
            ..EventTelemetrie::nouveau()
        });
    }

    /// Session courante + contexte live de la plateforme, en UN seul lock
    /// (règle mutex non réentrant). Un event `stream-info` met d'abord le
    /// contexte à jour (en_live → insert, !en_live → remove) — la ligne
    /// stream-info elle-même porte ainsi son propre jeu/titre.
    /// Retourne (session_id, titre_stream, jeu, jeu_id) à poser sur l'event.
    fn preparer_event(
        &self,
        plateforme: &str,
        kind: &str,
        data: &Value,
    ) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
        let mut g = self.inner.lock().unwrap();
        if kind == "stream-info" {
            if data["en_live"].as_bool().unwrap_or(false) {
                g.contextes.insert(
                    plateforme.to_string(),
                    ContexteLive {
                        titre: data["titre"].as_str().unwrap_or_default().to_string(),
                        jeu: data["jeu"].as_str().unwrap_or_default().to_string(),
                        jeu_id: data["jeu_id"].as_str().unwrap_or_default().to_string(),
                    },
                );
            } else {
                g.contextes.remove(plateforme);
            }
        }
        let ctx = g.contextes.get(plateforme);
        (
            g.sessions.get(plateforme).cloned(),
            ctx.map(|c| c.titre.clone()),
            ctx.map(|c| c.jeu.clone()),
            ctx.map(|c| c.jeu_id.clone()),
        )
    }

    /// Index mémoire + push ligne au writer. Non bloquant côté producteur.
    fn record(&self, ev: EventTelemetrie) {
        let ligne = match serde_json::to_string(&ev) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[Telemetrie] ERR serialize event: {}", e);
                return;
            }
        };
        {
            let mut g = self.inner.lock().unwrap();
            *g.par_kind.entry(ev.kind.clone()).or_insert(0) += 1;
            *g.par_plateforme.entry(ev.plateforme.clone()).or_insert(0) += 1;
            g.total += 1;
            g.dernier_ts = Some(ev.ts.clone());
            if g.derniers.len() >= DERNIERS_CAP {
                g.derniers.pop_front();
            }
            g.derniers.push_back(ev);
        }
        let _ = self.tx.send(ligne);
    }

    /// Snapshot pour la commande `telemetrie_etat`.
    pub fn etat(&self) -> Value {
        let g = self.inner.lock().unwrap();
        serde_json::json!({
            "total_events": g.total,
            "par_kind": g.par_kind,
            "par_plateforme": g.par_plateforme,
            "sessions_actives": g.sessions,
            "dernier_ts": g.dernier_ts,
            "fichier": self.fichier.to_string_lossy(),
        })
    }

    /// Derniers événements, plus récent en premier, bornés à DERNIERS_CAP.
    pub fn derniers(&self, limit: usize) -> Vec<EventTelemetrie> {
        let g = self.inner.lock().unwrap();
        g.derniers.iter().rev().take(limit).cloned().collect()
    }
}
