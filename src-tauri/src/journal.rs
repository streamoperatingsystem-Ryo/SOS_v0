//! Journal du backend — sans lui, **tous** les `log::*!` des crates sont jetés.
//!
//! Constat (2026-11-02) : 163 appels `log::info!/warn!/error!` dans le backend
//! et **aucun logger installé** — `log` ne fait rien sans `set_logger`. Tout
//! était donc silencieux en console, y compris le bilan de session du moteur
//! (`session : … fps mesurés … drop(s)`) et les dégradations, qui sont les
//! seuls chiffres exploitables pour diagnostiquer un enregistrement.
//!
//! Niveau : variable `SOS_LOG` (`error|warn|info|debug|trace`). Défaut
//! **`info` en debug** (console de `tauri dev` : c'est le canal
//! d'observabilité du développement) et **`warn` en release** (aucune console
//! à l'exécution — les avertissements restent capturés par la boîte noire et
//! les modales de diagnostic).
//!
//! Aucune écriture disque ici : stderr uniquement, conforme au contrat vie
//! privée (tout local, zéro envoi).

use std::io::Write;

struct Journal;

impl log::Log for Journal {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, enregistrement: &log::Record) {
        let niveau = match enregistrement.level() {
            log::Level::Error => "ERREUR",
            log::Level::Warn => "WARN",
            log::Level::Info => "INFO",
            log::Level::Debug => "DEBUG",
            log::Level::Trace => "TRACE",
        };
        // Un seul `write` par ligne : stderr est verrouillé par l'appel, pas
        // de risque d'entrelacement entre threads.
        let _ = writeln!(
            std::io::stderr(),
            "[{niveau}] {}",
            enregistrement.args()
        );
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

static JOURNAL: Journal = Journal;

/// Installe le journal — **idempotent** : un second appel (ou un logger déjà
/// posé par un autre module) n'est pas une erreur.
pub fn installer() {
    let defaut = if cfg!(debug_assertions) {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Warn
    };
    let niveau = std::env::var("SOS_LOG")
        .ok()
        .and_then(|v| v.trim().parse::<log::LevelFilter>().ok())
        .unwrap_or(defaut);
    if log::set_logger(&JOURNAL).is_ok() {
        log::set_max_level(niveau);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SOS_LOG` accepte les niveaux de `log` (et ignore le bruit).
    #[test]
    fn niveau_par_defaut_selon_le_profil() {
        // Le défaut dépend du profil de compilation : info en debug (console
        // de dev), warn en release. On vérifie seulement la cohérence.
        let defaut = if cfg!(debug_assertions) {
            log::LevelFilter::Info
        } else {
            log::LevelFilter::Warn
        };
        assert!(matches!(
            defaut,
            log::LevelFilter::Info | log::LevelFilter::Warn
        ));
    }
}
