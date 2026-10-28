//! Compteurs du moteur — atomics, lecture **sans blocage** (plan §9.3).
//!
//! Le moteur écrit, le diagnostic lit : aucun verrou dans le chemin chaud
//! (sauf la raison de dégradation, écrite uniquement lors d'une décision).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// Compteurs chauds du moteur.
#[derive(Default)]
pub struct CompteursMoteur {
    frames_capturees: AtomicU64,
    frames_composites: AtomicU64,
    frames_perdues: AtomicU64,
    file_encode: AtomicU32,
    kbps: AtomicU32,
    latence_ms: AtomicU32,
    /// `(largeur << 32) | hauteur`.
    resolution: AtomicU64,
    raison: Mutex<Option<String>>,
}

/// Photographie cohérente des compteurs (lue par le diagnostic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instantane {
    pub frames_capturees: u64,
    pub frames_composites: u64,
    pub frames_perdues: u64,
    pub file_encode: u32,
    pub kbps: u32,
    pub latence_ms: u32,
    pub resolution: (u32, u32),
    pub raison_degradation: Option<String>,
}

impl CompteursMoteur {
    pub fn nouveau() -> Self {
        Self::default()
    }

    pub fn capture(&self) {
        self.frames_capturees.fetch_add(1, Ordering::Relaxed);
    }

    pub fn composite(&self) {
        self.frames_composites.fetch_add(1, Ordering::Relaxed);
    }

    pub fn perdue(&self) {
        self.frames_perdues.fetch_add(1, Ordering::Relaxed);
    }

    pub fn maj_file_encode(&self, profondeur: u32) {
        self.file_encode.store(profondeur, Ordering::Relaxed);
    }

    pub fn maj_kbps(&self, kbps: u32) {
        self.kbps.store(kbps, Ordering::Relaxed);
    }

    pub fn maj_latence(&self, millisecondes: u32) {
        self.latence_ms.store(millisecondes, Ordering::Relaxed);
    }

    pub fn maj_resolution(&self, largeur: u32, hauteur: u32) {
        self.resolution
            .store(((largeur as u64) << 32) | hauteur as u64, Ordering::Relaxed);
    }

    /// Enregistre la raison d'une dégradation (jamais silencieuse).
    pub fn degrader(&self, raison: impl Into<String>) {
        if let Ok(mut garde) = self.raison.lock() {
            *garde = Some(raison.into());
        }
    }

    pub fn instantane(&self) -> Instantane {
        let packee = self.resolution.load(Ordering::Relaxed);
        Instantane {
            frames_capturees: self.frames_capturees.load(Ordering::Relaxed),
            frames_composites: self.frames_composites.load(Ordering::Relaxed),
            frames_perdues: self.frames_perdues.load(Ordering::Relaxed),
            file_encode: self.file_encode.load(Ordering::Relaxed),
            kbps: self.kbps.load(Ordering::Relaxed),
            latence_ms: self.latence_ms.load(Ordering::Relaxed),
            resolution: ((packee >> 32) as u32, packee as u32),
            raison_degradation: self.raison.lock().ok().and_then(|g| g.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremente_et_photographie() {
        let compteurs = CompteursMoteur::nouveau();
        compteurs.capture();
        compteurs.capture();
        compteurs.composite();
        compteurs.perdue();
        compteurs.maj_file_encode(3);
        compteurs.maj_kbps(6000);
        compteurs.maj_latence(42);
        compteurs.maj_resolution(1360, 768);

        let i = compteurs.instantane();
        assert_eq!(i.frames_capturees, 2);
        assert_eq!(i.frames_composites, 1);
        assert_eq!(i.frames_perdues, 1);
        assert_eq!(i.file_encode, 3);
        assert_eq!(i.kbps, 6000);
        assert_eq!(i.latence_ms, 42);
        assert_eq!(i.resolution, (1360, 768));
        assert_eq!(i.raison_degradation, None);
    }

    #[test]
    fn degradation_tracee() {
        let compteurs = CompteursMoteur::nouveau();
        compteurs.degrader("reduire fps 60->30 (drops)");
        assert_eq!(
            compteurs.instantane().raison_degradation.as_deref(),
            Some("reduire fps 60->30 (drops)")
        );
    }
}
