//! Piste audio d'un enregistrement : capture WASAPI dans un **thread dédié**,
//! écrite en PCM brut (`s16le`) dans un fichier temporaire.
//!
//! Pourquoi un thread : `Capteur::lire` attend un paquet jusqu'à 150 ms
//! (`capture.rs`). L'appeler depuis la boucle vidéo (16,7 ms à 60 fps)
//! bloquerait la composition. La boucle vidéo ne fait donc que démarrer et
//! arrêter les pistes.
//!
//! Pourquoi du PCM brut et pas un WAV : le muxage est fait par FFmpeg, à qui
//! on donne le format (`-f s16le -ar … -ac …`). Écrire un en-tête WAV
//! demanderait de revenir en arrière sur la taille à la fermeture, pour un
//! gain nul.
//!
//! Gains : **jamais** appliqués ici — ils partent en `volume=` dans la
//! commande de muxage. Le PCM reste à l'unité, donc une seule source de
//! vérité pour le mixage (FFmpeg).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;
use std::thread::JoinHandle;

use super::capture;
use super::{ErreurAudio, Sens};

/// Une piste capturée, prête à être muxée.
#[derive(Debug, Clone, PartialEq)]
pub struct Piste {
    pub chemin: PathBuf,
    pub taux: u32,
    pub canaux: u16,
    /// Gain en millièmes (1000 = unité) — appliqué au muxage.
    pub gain_milliemes: u32,
    /// Durée réellement écrite (diagnostic, dérive A/V).
    pub duree_s: f64,
}

/// Capture en cours d'une source.
pub struct CapturePiste {
    arret: Arc<AtomicBool>,
    fil: Option<JoinHandle<Result<Piste, ErreurAudio>>>,
    /// Signale le **premier paquet réellement écrit**.
    demarrage: Option<Receiver<()>>,
}

impl CapturePiste {
    /// Attend le premier paquet audio (≤ `delai`).
    ///
    /// Sert à **aligner** les flux : la boucle vidéo ne doit démarrer qu'une
    /// fois que le son produit des échantillons. Sinon le buffer WASAPI
    /// (200 ms) décale toute la piste par rapport à la première frame —
    /// mesuré : 2,83 s d'audio pour 3 s de vidéo.
    pub fn attendre_demarrage(&mut self, delai: Duration) -> bool {
        match self.demarrage.take() {
            // `Disconnected` = le thread s'est terminé (périphérique refusé) :
            // inutile d'attendre, l'erreur remontera à `arreter`.
            Some(rx) => matches!(rx.recv_timeout(delai), Ok(())),
            None => true,
        }
    }

    /// Arrête la capture et attend le thread (≤ 150 ms, le temps d'un paquet).
    pub fn arreter(mut self) -> Result<Piste, ErreurAudio> {
        self.arret.store(true, Ordering::Relaxed);
        match self.fil.take() {
            Some(fil) => fil.join().unwrap_or_else(|_| {
                Err(ErreurAudio::Lecture("thread audio interrompu".to_string()))
            }),
            None => Err(ErreurAudio::Lecture("piste déjà arrêtée".to_string())),
        }
    }
}

/// Démarre la capture d'une source (`nom` sert au fichier temporaire).
///
/// Rend une erreur si le périphérique ne s'ouvre pas : l'appelant **dégrade**
/// (enregistrement muet sur cette source) plutôt que d'échouer — un micro
/// occupé ne doit pas annuler un enregistrement.
pub fn demarrer(sens: Sens, gain_milliemes: u32, nom: &str) -> Result<CapturePiste, ErreurAudio> {
    let chemin = std::env::temp_dir().join(format!(
        "sos-audio-{}-{}.pcm",
        std::process::id(),
        nom
    ));

    // L'ouverture reste **dans le thread** : `Capteur` porte des interfaces
    // COM non `Send` (`IUnknown` → `NonNull<c_void>`), et on ne veut pas
    // d'`unsafe impl Send` sur un objet d'appartement. L'alignement A/V est
    // obtenu autrement : l'appelant attend le premier paquet
    // (`attendre_demarrage`) avant de lancer la boucle vidéo.
    let arret = Arc::new(AtomicBool::new(false));
    let drapeau = Arc::clone(&arret);
    let chemin_fil = chemin.clone();
    let (tx, rx) = mpsc::channel::<()>();

    let fil = std::thread::Builder::new()
        .name(format!("sos-audio-{nom}"))
        .spawn(move || -> Result<Piste, ErreurAudio> {
            let mut capteur = capture::ouvrir(sens)?;
            let format = capteur.format();
            let fichier = File::create(&chemin_fil).map_err(|e| {
                ErreurAudio::Lecture(format!("création {} : {e}", chemin_fil.display()))
            })?;
            let mut sortie = BufWriter::with_capacity(64 * 1024, fichier);
            let mut tampon: Vec<f32> = Vec::with_capacity(8192);
            let mut octets: Vec<u8> = Vec::with_capacity(8192 * 2);
            let mut echantillons: u64 = 0;

            while !drapeau.load(Ordering::Relaxed) {
                tampon.clear();
                let lus = capteur.lire(&mut tampon)?;
                if lus == 0 {
                    continue;
                }
                octets.clear();
                octets.reserve(lus * 2);
                for valeur in &tampon {
                    let pcm = (valeur.clamp(-1.0, 1.0) * 32767.0) as i16;
                    octets.extend_from_slice(&pcm.to_le_bytes());
                }
                sortie.write_all(&octets).map_err(|e| {
                    ErreurAudio::Lecture(format!("écriture {} : {e}", chemin_fil.display()))
                })?;
                if echantillons == 0 {
                    let _ = tx.send(());
                }
                echantillons += lus as u64;
            }
            sortie
                .flush()
                .map_err(|e| ErreurAudio::Lecture(format!("flush {} : {e}", chemin_fil.display())))?;
            capteur.arreter();
            let duree_s = if format.taux > 0 && format.canaux > 0 {
                echantillons as f64 / (format.taux as f64 * format.canaux as f64)
            } else {
                0.0
            };
            Ok(Piste {
                chemin: chemin_fil,
                taux: format.taux,
                canaux: format.canaux,
                gain_milliemes,
                duree_s,
            })
        })
        .map_err(|e| ErreurAudio::Init(format!("thread audio : {e}")))?;

    Ok(CapturePiste {
        arret,
        fil: Some(fil),
        demarrage: Some(rx),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sans périphérique audio, `demarrer` doit **échouer proprement** (jamais
    /// paniquer) : c'est ce qui permet à un enregistrement de continuer muet.
    #[test]
    fn demarrage_sans_peripherique_ne_panique_pas() {
        match demarrer(Sens::Entree, 1000, "test-sans-device") {
            Ok(capture) => {
                // Un device existe : on arrête tout de suite, la piste doit
                // être cohérente (durée ≥ 0, format non nul).
                let piste = capture.arreter().expect("piste");
                assert!(piste.taux > 0 && piste.canaux > 0);
                assert!(piste.duree_s >= 0.0);
                let _ = std::fs::remove_file(&piste.chemin);
            }
            Err(_) => { /* attendu sans micro : rien à vérifier */ }
        }
    }
}
