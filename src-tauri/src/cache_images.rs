//! Cache disque des images du chat (avatars + emotes) — lot G (2026-11-01).
//!
//! Le moteur natif **ne connaît pas le réseau** (invariant §13.8 : il ne
//! dépend ni de Tauri ni du domaine). C'est donc l'app qui télécharge, et le
//! moteur ne lit qu'un **fichier local** — exactement le chemin déjà utilisé
//! pour les alertes (`media::decoder_pbgra`, WIC + alpha).
//!
//! Le nom du fichier est **dérivé de l'URL** (FNV-1a 64 bits → 16 hex) :
//! aucune modification de la frame `chat` n'est nécessaire, le crate
//! recalcule le même nom (`sos_moteur_natif::chat::empreinte_url`, miroir
//! verrouillé des deux côtés par un vecteur de test commun).
//!
//! Chemin : `<app_data>\medias\cache\<empreinte>` (sans extension — WIC
//! détecte le format au contenu, vérifié sur PNG/GIF/WebP).
//!
//! Politique (décision utilisateur 2026-11-01) : **à la volée**. Au premier
//! affichage l'image manque (le chat affiche le texte de l'emote / pas
//! d'avatar), le téléchargement part en tâche de fond, les occurrences
//! suivantes sont locales. Aucune rafale au démarrage, aucun blocage du chat.
//!
//! Contrat vie privée : les URLs viennent des APIs officielles (Helix,
//! YouTube, TikTok) et des APIs publiques d'emotes (BTTV/7TV/FFZ) — jamais
//! d'une saisie utilisateur. GET `https` uniquement, taille bornée, aucune
//! donnée envoyée (pas d'auth, User-Agent générique).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// Taille maximale d'une image téléchargée (une emote fait ~10-60 Kio).
const TAILLE_MAX: u64 = 2 * 1024 * 1024;
/// Nombre de fichiers au-delà duquel les plus anciens sont purgés au boot.
const FICHIERS_MAX: usize = 1024;

static CACHE: OnceLock<CacheImages> = OnceLock::new();

/// État du cache (client HTTP partagé + téléchargements en cours).
struct CacheImages {
    /// Dossier de données de l'app (`<base>\medias\cache\` = le cache).
    base: PathBuf,
    client: reqwest::Client,
    /// URLs en cours de téléchargement — évite N GET pour une emote spammée.
    en_cours: Mutex<HashSet<String>>,
}

impl CacheImages {
    fn nouveau(base: PathBuf) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("StreamOS/0.2")
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            base,
            client,
            en_cours: Mutex::new(HashSet::new()),
        }
    }
}

/// FNV-1a 64 bits → 16 hex minuscules.
///
/// ⚠️ **Miroir exact** de `sos_moteur_natif::chat::empreinte_url` : les deux
/// implémentations doivent produire la même chaîne pour la même URL, sinon le
/// moteur ne retrouve jamais le fichier écrit ici. Verrouillé par le même
/// vecteur de test des deux côtés (`3d695665d84ffc9e` pour l'URL 7TV).
pub fn empreinte(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for octet in url.as_bytes() {
        hash ^= *octet as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Chemin du fichier de cache d'une URL (`base` = dossier de données de l'app).
pub fn chemin(base: &Path, url: &str) -> PathBuf {
    base.join("medias").join("cache").join(empreinte(url))
}

/// Cache initialisé au premier usage (dossier créé, orphelins purgés).
fn cache(app: &tauri::AppHandle) -> Option<&'static CacheImages> {
    CACHE.get_or_init(|| {
        let base = crate::config::data_dir(app).unwrap_or_default();
        if !base.as_os_str().is_empty() {
            let dossier = base.join("medias").join("cache");
            let _ = std::fs::create_dir_all(&dossier);
            purger(&dossier);
        }
        CacheImages::nouveau(base)
    });
    let cache = CACHE.get()?;
    (!cache.base.as_os_str().is_empty()).then_some(cache)
}

/// Purge les plus anciens fichiers quand le cache dépasse [`FICHIERS_MAX`] —
/// l'ensemble des URLs vues (avatars qui changent, emotes de chaînes
/// visitées) croît sans borne ; le cache doit rester borné en disque.
fn purger(dossier: &Path) {
    let Ok(lecture) = std::fs::read_dir(dossier) else {
        return;
    };
    let mut fichiers: Vec<(SystemTime, PathBuf)> = lecture
        .flatten()
        .filter_map(|e| {
            let chemin = e.path();
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), chemin))
        })
        .collect();
    if fichiers.len() <= FICHIERS_MAX {
        return;
    }
    fichiers.sort_by_key(|(date, _)| *date);
    let a_retirer = fichiers.len() - FICHIERS_MAX / 4 * 3;
    for (_, chemin) in fichiers.into_iter().take(a_retirer) {
        let _ = std::fs::remove_file(chemin);
    }
}

/// Précharge une image en tâche de fond.
///
/// No-op si l'URL n'est pas `https`, si le fichier est déjà en cache ou si le
/// téléchargement est déjà en cours. **Jamais bloquant** : l'appelant émet le
/// message immédiatement (le premier rendu retombe sur le texte).
pub fn precharger(app: &tauri::AppHandle, url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    let Some(cache) = cache(app) else {
        return;
    };
    let chemin = chemin(&cache.base, url);
    if chemin.exists() {
        return;
    }
    {
        let mut en_cours = cache.en_cours.lock().unwrap_or_else(|e| e.into_inner());
        if !en_cours.insert(url.to_string()) {
            return;
        }
    }
    let client = cache.client.clone();
    let url = url.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = telecharger(&client, &url, &chemin).await {
            log::debug!("cache images : {url} ignorée ({e})");
        }
        if let Some(cache) = CACHE.get() {
            cache
                .en_cours
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&url);
        }
    });
}

/// Précharge une liste d'URLs (emotes d'un message).
pub fn precharger_urls<'a>(app: &tauri::AppHandle, urls: impl IntoIterator<Item = &'a str>) {
    for url in urls {
        precharger(app, url);
    }
}

/// GET → fichier, écriture **atomique** (temporaire + rename) : le moteur ne
/// lit jamais un fichier partiel.
async fn telecharger(client: &reqwest::Client, url: &str, chemin: &Path) -> Result<(), String> {
    let reponse = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !reponse.status().is_success() {
        return Err(format!("HTTP {}", reponse.status()));
    }
    if reponse.content_length().unwrap_or(0) > TAILLE_MAX {
        return Err("image trop grosse".to_string());
    }
    let octets = reponse.bytes().await.map_err(|e| e.to_string())?;
    if octets.len() as u64 > TAILLE_MAX {
        return Err("image trop grosse".to_string());
    }
    let temporaire = chemin.with_extension("part");
    tokio::fs::write(&temporaire, &octets)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&temporaire, chemin)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empreinte_miroir_du_crate_moteur() {
        // Vecteur commun avec `sos_moteur_natif::chat::empreinte_url` —
        // les deux implémentations DOIVENT produire ces valeurs, sinon le
        // moteur ne retrouve pas les fichiers écrits par l'app.
        assert_eq!(
            empreinte("https://cdn.7tv.app/emote/01FCY771D800007PQ2DF3GDTN6/2x.webp"),
            "3d695665d84ffc9e"
        );
        assert_eq!(
            empreinte("https://static-cdn.jtvnw.net/emoticons/v2/25/default/dark/2.0"),
            "0d8894bf3abf0e01"
        );
        assert_eq!(empreinte(""), "cbf29ce484222325", "offset FNV-1a");
    }

    #[test]
    fn chemin_sous_medias_cache() {
        let base = Path::new("C:/data");
        assert_eq!(
            chemin(base, "https://cdn.7tv.app/emote/x/2x.webp"),
            base.join("medias")
                .join("cache")
                .join(empreinte("https://cdn.7tv.app/emote/x/2x.webp"))
        );
    }

    #[test]
    fn purge_borne_le_disque() {
        let dossier = std::env::temp_dir().join(format!(
            "sos-cache-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(&dossier).expect("dossier de test");
        for i in 0..FICHIERS_MAX + 10 {
            std::fs::write(dossier.join(format!("{i:016x}")), b"x").expect("écriture");
        }
        purger(&dossier);
        let restants = std::fs::read_dir(&dossier).expect("lecture").count();
        assert_eq!(restants, FICHIERS_MAX / 4 * 3);
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
