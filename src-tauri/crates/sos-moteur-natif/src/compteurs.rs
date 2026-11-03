//! Compteurs du moteur — atomics, lecture **sans blocage** (plan §9.3).
//!
//! Le moteur écrit, le diagnostic lit : aucun verrou dans le chemin chaud
//! (sauf la raison de dégradation, écrite uniquement lors d'une décision).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// Compteurs chauds du moteur.
pub struct CompteursMoteur {
    frames_capturees: AtomicU64,
    frames_composites: AtomicU64,
    frames_perdues: AtomicU64,
    /// Frames réémises pour combler un tick manqué (rattrapage CFR, voir
    /// `moteur.rs`) : elles ne sont **pas** des compositions — c'est la même
    /// texture encodée une seconde fois.
    frames_dupliquees: AtomicU64,
    file_encode: AtomicU32,
    kbps: AtomicU32,
    latence_ms: AtomicU32,
    /// fps **mesuré** × 1000 (`u32::MAX` = jamais mesuré). Le diagnostic ne
    /// doit jamais confondre le fps demandé et le fps tenu.
    fps_reel_milliemes: AtomicU32,
    /// `(largeur << 32) | hauteur`.
    resolution: AtomicU64,
    raison: Mutex<Option<String>>,
}

/// Photographie cohérente des compteurs (lue par le diagnostic).
#[derive(Debug, Clone, PartialEq)]
pub struct Instantane {
    pub frames_capturees: u64,
    pub frames_composites: u64,
    pub frames_perdues: u64,
    /// Frames réémises par le rattrapage CFR (comptées à part des pertes :
    /// une frame dupliquée **tient sa place** dans la timeline).
    pub frames_dupliquees: u64,
    pub file_encode: u32,
    pub kbps: u32,
    pub latence_ms: u32,
    /// fps réellement produit (moyenne depuis le début), `None` tant que rien
    /// n'est mesuré — jamais un 0 inventé.
    pub fps_reel: Option<f64>,
    pub resolution: (u32, u32),
    pub raison_degradation: Option<String>,
}

impl Default for CompteursMoteur {
    /// `fps_reel` part à `u32::MAX` = **jamais mesuré** : un 0 serait un fps
    /// inventé (contrat §16.7).
    fn default() -> Self {
        Self {
            frames_capturees: AtomicU64::new(0),
            frames_composites: AtomicU64::new(0),
            frames_perdues: AtomicU64::new(0),
            frames_dupliquees: AtomicU64::new(0),
            file_encode: AtomicU32::new(0),
            kbps: AtomicU32::new(0),
            latence_ms: AtomicU32::new(0),
            fps_reel_milliemes: AtomicU32::new(u32::MAX),
            resolution: AtomicU64::new(0),
            raison: Mutex::new(None),
        }
    }
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

    /// Compte `n` frames perdues d'un coup — un retard de 240 ms à 60 fps
    /// coûte ~14 frames, pas une : sans ça le compteur reste flatteur.
    pub fn perdue_n(&self, n: u64) {
        self.frames_perdues.fetch_add(n.max(1), Ordering::Relaxed);
    }

    pub fn perdue(&self) {
        self.perdue_n(1);
    }

    /// Compte une frame **réémise** pour tenir la grille CFR (voir
    /// `moteur.rs`). Distincte de [`CompteursMoteur::perdue`] : la frame
    /// existe dans le fichier, elle n'est pas un trou.
    pub fn dupliquee(&self) {
        self.frames_dupliquees.fetch_add(1, Ordering::Relaxed);
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

    /// Publie le fps **mesuré** (jamais le fps demandé : c'est la règle du
    /// diagnostic honnête, plan §16.7).
    pub fn maj_fps_reel(&self, fps: f64) {
        let milliemes = (fps.max(0.0) * 1000.0).min(f64::from(u32::MAX - 1)) as u32;
        self.fps_reel_milliemes.store(milliemes, Ordering::Relaxed);
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
        let fps = self.fps_reel_milliemes.load(Ordering::Relaxed);
        Instantane {
            frames_capturees: self.frames_capturees.load(Ordering::Relaxed),
            frames_composites: self.frames_composites.load(Ordering::Relaxed),
            frames_perdues: self.frames_perdues.load(Ordering::Relaxed),
            frames_dupliquees: self.frames_dupliquees.load(Ordering::Relaxed),
            file_encode: self.file_encode.load(Ordering::Relaxed),
            kbps: self.kbps.load(Ordering::Relaxed),
            latence_ms: self.latence_ms.load(Ordering::Relaxed),
            fps_reel: (fps != u32::MAX).then(|| f64::from(fps) / 1000.0),
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
        compteurs.dupliquee();
        compteurs.maj_file_encode(3);
        compteurs.maj_kbps(6000);
        compteurs.maj_latence(42);
        compteurs.maj_resolution(1360, 768);

        let i = compteurs.instantane();
        assert_eq!(i.frames_capturees, 2);
        assert_eq!(i.frames_composites, 1);
        assert_eq!(i.frames_perdues, 1);
        // Une frame dupliquée n'est pas une perte : les deux compteurs sont
        // indépendants (le diagnostic ne doit pas les confondre).
        assert_eq!(i.frames_dupliquees, 1);
        assert_eq!(i.file_encode, 3);
        assert_eq!(i.kbps, 6000);
        assert_eq!(i.latence_ms, 42);
        assert_eq!(i.resolution, (1360, 768));
        assert_eq!(i.raison_degradation, None);
        // fps jamais mesuré = `None` (jamais un 0 inventé).
        assert_eq!(i.fps_reel, None);
    }

    #[test]
    fn fps_reel_mesure_puis_publie() {
        let compteurs = CompteursMoteur::nouveau();
        compteurs.maj_fps_reel(48.596);
        let i = compteurs.instantane();
        let fps = i.fps_reel.expect("fps mesuré");
        assert!((fps - 48.596).abs() < 0.001, "{fps}");
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
