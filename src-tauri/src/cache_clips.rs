//! Préchargement disque des clips de la présentation streamer — lot E6c.
//!
//! Le moteur natif **ne connaît pas le réseau** (invariant §13.8) et l'URL MP4
//! d'un clip Twitch est **signée** (`…-1080.mp4?sig=…&token=…`) : elle expire
//! côté Twitch et n'est lisible que par un client HTTP. C'est donc l'app qui
//! télécharge, et le moteur ne lit qu'un **fichier local** — même contrat que
//! les images (`cache_images.rs`, lot G).
//!
//! Décision utilisateur du 2026-11-01 : **préchargement disque** (et non
//! « vignette + titre sans lecture »).
//!
//! Chemin : `<app_data>\medias\clips\<fnv16(url)>.mp4` — le nom est dérivé de
//! l'URL (`cache_images::empreinte`, FNV-1a 64, **une seule**
//! implémentation : le crate recalcule le même nom avec
//! `sos_moteur_natif::chat::empreinte_url`).
//!
//! Politique : téléchargement en **tâche de fond** au clic « jouer un clip »
//! (le bouton du dashboard ne bloque pas), écriture **atomique** (temporaire +
//! rename : le moteur ne lit jamais un fichier partiel), purge des plus
//! anciens au-delà de [`FICHIERS_MAX`] fichiers.
//!
//! Contrat vie privée (`AGENTS.md`, encart « Clips de la présentation ») : GET
//! `https` vers `clips-media-assets2.twitch.tv` uniquement, aucune auth, aucune
//! donnée utilisateur envoyée, User-Agent générique, taille bornée.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

/// Taille maximale d'un clip téléchargé (un clip 1080p de 30 s ≈ 10-30 Mio).
const TAILLE_MAX: u64 = 256 * 1024 * 1024;
/// Nombre de fichiers conservés (les plus anciens sont purgés).
const FICHIERS_MAX: usize = 8;

/// Chemin du fichier local d'un clip (`base` = dossier de données de l'app).
///
/// ⚠️ **Miroir** du chemin calculé par le crate
/// (`texte::presentation::chemin_clip`) : même empreinte (`empreinte` ↔
/// `chat::empreinte_url`), même sous-dossier, même extension.
pub fn chemin(base: &Path, url: &str) -> PathBuf {
    base.join("medias")
        .join("clips")
        .join(format!("{}.mp4", crate::cache_images::empreinte(url)))
}

/// Client HTTP partagé (un seul pour tous les clips).
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("StreamOS/0.2")
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default()
    })
}

/// Télécharge un clip en tâche de fond (no-op si déjà présent).
///
/// **Jamais bloquant** : la frame `presentation-clip-play` part immédiatement,
/// le moteur re-tente de lire le fichier toutes les 500 ms et le lecteur
/// affiche le voile + le titre en attendant (équivalent d'un `<video>` en
/// buffering).
pub fn precharger(app: &tauri::AppHandle, url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    let Ok(base) = crate::config::data_dir(app) else {
        return;
    };
    let chemin = chemin(&base, url);
    if chemin.exists() {
        return;
    }
    let dossier = match chemin.parent() {
        Some(d) => d.to_path_buf(),
        None => return,
    };
    if std::fs::create_dir_all(&dossier).is_err() {
        return;
    }
    purger(&dossier);
    let client = client().clone();
    let url = url.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = telecharger(&client, &url, &chemin).await {
            log::debug!("cache clips : {url} ignoré ({e})");
        }
    });
}

/// Purge les plus anciens clips quand le dossier dépasse [`FICHIERS_MAX`] —
/// un clip pèse plusieurs dizaines de Mio, le disque doit rester borné.
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
    if fichiers.len() < FICHIERS_MAX {
        return;
    }
    fichiers.sort_by_key(|(date, _)| *date);
    let a_retirer = fichiers.len() + 1 - FICHIERS_MAX;
    for (_, chemin) in fichiers.into_iter().take(a_retirer) {
        let _ = std::fs::remove_file(chemin);
    }
}

/// GET → fichier, écriture **atomique** (temporaire + rename).
async fn telecharger(client: &reqwest::Client, url: &str, chemin: &Path) -> Result<(), String> {
    let reponse = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !reponse.status().is_success() {
        return Err(format!("HTTP {}", reponse.status()));
    }
    if reponse.content_length().unwrap_or(0) > TAILLE_MAX {
        return Err("clip trop gros".to_string());
    }
    let octets = reponse.bytes().await.map_err(|e| e.to_string())?;
    if octets.len() as u64 > TAILLE_MAX {
        return Err("clip trop gros".to_string());
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
    fn chemin_sous_medias_clips_et_miroir_du_crate() {
        let base = Path::new("C:/data");
        let url = "https://clips-media-assets2.twitch.tv/AT-cm%7Cabc-1080.mp4?sig=x";
        let attendu = base
            .join("medias")
            .join("clips")
            .join(format!("{}.mp4", crate::cache_images::empreinte(url)));
        assert_eq!(chemin(base, url), attendu);
        // Le crate recalcule le même nom (`chat::empreinte_url`) : vecteur
        // verrouillé des deux côtés par le test d'`empreinte`.
        assert_eq!(
            crate::cache_images::empreinte(""),
            sos_moteur_natif::chat::empreinte_url(""),
            "FNV-1a : même empreinte des deux côtés"
        );
    }

    #[test]
    fn purge_borne_le_disque() {
        let dossier =
            std::env::temp_dir().join(format!("sos-clips-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(&dossier).expect("dossier de test");
        for i in 0..FICHIERS_MAX + 4 {
            std::fs::write(dossier.join(format!("{i:016x}.mp4")), b"x").expect("écriture");
        }
        purger(&dossier);
        let restants = std::fs::read_dir(&dossier).expect("lecture").count();
        assert!(restants < FICHIERS_MAX, "les plus anciens sont partis");
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
