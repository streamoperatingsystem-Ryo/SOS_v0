//! Stockage JSONL append-only — `telemetrie/events.jsonl`.
//!
//! Thread writer dédié : les producteurs poussent des lignes JSON déjà
//! sérialisées via un canal mpsc non borné (jamais bloquant). Le writer
//! flush périodiquement (500 ms ou 64 lignes) — crash-safe : au pire une
//! ligne partielle en fin de fichier, ignorée à la lecture.
//!
//! Rotation mensuelle non implémentée (LOT ultérieur si volume le justifie).

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tauri::AppHandle;

/// Intervalle de flush quand des lignes sont en attente.
const FLUSH_EVERY: Duration = Duration::from_millis(500);
/// Flush anticipé tous les N lignes écrites.
const FLUSH_LINES: usize = 64;

/// Chemin du fichier d'événements — crée `telemetrie/` si besoin.
pub fn fichier_events(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = crate::config::data_dir(app)?.join("telemetrie");
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("Créer telemetrie/: {}", e))?;
    }
    Ok(dir.join("events.jsonl"))
}

/// Démarre le thread writer. Retourne le canal de production de lignes.
/// Le thread meurt quand le Sender est droppé (arrêt de l'app).
/// Si le fichier ne peut pas être ouvert, le canal est vidé sans écrire
/// (la collecte mémoire continue — dégradé, pas de crash).
pub fn demarrer_writer(path: PathBuf) -> mpsc::Sender<String> {
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[Telemetrie] ERR open {}: {}", path.display(), e);
                while rx.recv().is_ok() {}
                return;
            }
        };
        let mut w = BufWriter::new(file);
        let mut pending = 0usize;
        loop {
            match rx.recv_timeout(FLUSH_EVERY) {
                Ok(ligne) => {
                    if writeln!(w, "{}", ligne).is_err() {
                        break;
                    }
                    pending += 1;
                    if pending >= FLUSH_LINES {
                        if w.flush().is_err() {
                            break;
                        }
                        pending = 0;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pending > 0 {
                        if w.flush().is_err() {
                            break;
                        }
                        pending = 0;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = w.flush();
                    break;
                }
            }
        }
    });
    tx
}
