//! Audio — capture WASAPI (loopback / micro), mixage et **mesure de niveau**,
//! sans dépendance tierce.
//!
//! Liaisons `windows` directes : `IMMDeviceEnumerator` → `IAudioClient` →
//! `IAudioCaptureClient`. Le format effectif est celui du mixage du
//! périphérique (f32 48 kHz stéréo en pratique) ; PCM 16 bits accepté aussi.
//!
//! Les niveaux (peak / RMS / peak-hold / écrêtage) sont calculés **dans le
//! thread de capture**, sur les échantillons bruts (pré-fader) : la lecture
//! à 20 Hz côté UI ne peut donc manquer aucun pic.

pub mod capture;
/// Chemin temps réel : anneaux bornés + mixage dans la boucle (live.rs).
pub mod live;
pub mod piste;

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Plancher d'affichage en dBFS — évite `-inf` dans le JSON.
pub const PLANCHER_DB: f32 = -96.0;
/// Seuil d'écrêtage (pleine échelle moins 0,01 dB).
const SEUIL_ECRETAGE: f32 = 0.999;
/// Décroissance du peak, en dB/s (PPM broadcast ≈ 20 dB/s).
const DECROISSANCE_DB_S: f32 = 20.0;
/// Durée du peak-hold, en secondes.
const HOLD_S: f32 = 1.5;
/// Constante de temps du RMS (lissage exponentiel), en secondes.
const RMS_TAU_S: f32 = 0.3;

/// Sens de capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sens {
    /// Loopback d'un périphérique de sortie (= « desktop audio »).
    Sortie,
    /// Périphérique d'entrée (micro).
    Entree,
}

/// Format effectif d'une source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatAudio {
    pub taux: u32,
    pub canaux: u16,
    /// `true` = f32, `false` = PCM 16 bits.
    pub flottant: bool,
}

/// Erreurs audio. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurAudio {
    /// Ouverture du périphérique / du client WASAPI.
    Init(String),
    /// Lecture d'un paquet.
    Lecture(String),
    /// Format de mixage non supporté.
    Format(String),
}

impl fmt::Display for ErreurAudio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurAudio::Init(m) => write!(f, "init : {m}"),
            ErreurAudio::Lecture(m) => write!(f, "lecture : {m}"),
            ErreurAudio::Format(m) => write!(f, "format : {m}"),
        }
    }
}

impl std::error::Error for ErreurAudio {}

/// Niveau d'un canal, en dBFS (pic, RMS lissé, peak-hold).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanalNiveau {
    pub peak_db: f32,
    pub rms_db: f32,
    pub hold_db: f32,
}

impl CanalNiveau {
    pub const SILENCE: Self = Self {
        peak_db: PLANCHER_DB,
        rms_db: PLANCHER_DB,
        hold_db: PLANCHER_DB,
    };
}

/// Niveau stéréo d'une source (ou du bus programme).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NiveauAudio {
    pub gauche: CanalNiveau,
    pub droite: CanalNiveau,
    /// Écrêtage **latché** : reste vrai jusqu'à `reinitialiser()`.
    pub ecretage: bool,
}

impl NiveauAudio {
    pub const SILENCE: Self = Self {
        gauche: CanalNiveau::SILENCE,
        droite: CanalNiveau::SILENCE,
        ecretage: false,
    };

    /// Pic le plus fort des deux canaux — valeur d'accessibilité (`aria-valuenow`).
    pub fn peak_max_db(&self) -> f32 {
        self.gauche.peak_db.max(self.droite.peak_db)
    }
}

/// Linéaire → dBFS borné au plancher.
fn db(lin: f32) -> f32 {
    if lin <= 0.0 || !lin.is_finite() {
        PLANCHER_DB
    } else {
        (20.0 * lin.log10()).max(PLANCHER_DB)
    }
}

fn lire_bits(cellule: &AtomicU32) -> f32 {
    f32::from_bits(cellule.load(Ordering::Relaxed))
}

fn ecrire_bits(cellule: &AtomicU32, valeur: f32) {
    cellule.store(valeur.to_bits(), Ordering::Relaxed);
}

/// Compteurs de niveau partagés — **atomics uniquement**, aucun verrou dans
/// le chemin chaud. Le producteur (thread de capture ou mixeur) écrit, l'UI
/// lit à sa cadence.
pub struct NiveauxPartages {
    peak: [AtomicU32; 2],
    rms: [AtomicU32; 2],
    hold: [AtomicU32; 2],
    /// Frames restantes avant que le peak-hold ne commence à décroître.
    hold_restant: [AtomicU32; 2],
    ecretage: AtomicU32,
}

impl Default for NiveauxPartages {
    fn default() -> Self {
        Self::nouveau()
    }
}

impl NiveauxPartages {
    pub fn nouveau() -> Self {
        Self {
            peak: [AtomicU32::new(0), AtomicU32::new(0)],
            rms: [AtomicU32::new(0), AtomicU32::new(0)],
            hold: [AtomicU32::new(0), AtomicU32::new(0)],
            hold_restant: [AtomicU32::new(0), AtomicU32::new(0)],
            ecretage: AtomicU32::new(0),
        }
    }

    /// Met à jour les compteurs avec un bloc **f32 entrelacé** au taux
    /// `taux` (Hz) et `canaux` canaux. Mono = dupliqué sur les deux canaux ;
    /// au-delà de 2 canaux, seuls les deux premiers sont mesurés.
    pub fn maj(&self, echantillons: &[f32], canaux: u16, taux: u32) {
        if canaux == 0 || taux == 0 {
            return;
        }
        let n = canaux as usize;
        let frames = echantillons.len() / n;
        if frames == 0 {
            return;
        }
        let dt = frames as f32 / taux as f32;
        let mut peak = [0f32; 2];
        let mut somme = [0f64; 2];
        for (i, &valeur) in echantillons.iter().enumerate() {
            let canal = if n == 1 { 0 } else { (i % n).min(1) };
            let absolu = valeur.abs();
            if absolu > peak[canal] {
                peak[canal] = absolu;
            }
            somme[canal] += (valeur as f64) * (valeur as f64);
        }
        if n == 1 {
            peak[1] = peak[0];
            somme[1] = somme[0];
        }
        let facteur = 10f32.powf(-DECROISSANCE_DB_S * dt / 20.0);
        let alpha = (-dt / RMS_TAU_S).exp();
        let frames_f = frames as f64;
        for canal in 0..2 {
            let ancien_peak = lire_bits(&self.peak[canal]);
            let nouveau_peak = peak[canal].max(ancien_peak * facteur);
            ecrire_bits(&self.peak[canal], nouveau_peak);

            let ancien_rms = lire_bits(&self.rms[canal]);
            let rms_bloc = (somme[canal] / frames_f).sqrt() as f32;
            let nouveau_rms =
                (alpha * ancien_rms * ancien_rms + (1.0 - alpha) * rms_bloc * rms_bloc).sqrt();
            ecrire_bits(&self.rms[canal], nouveau_rms);

            let ancien_hold = lire_bits(&self.hold[canal]);
            if peak[canal] >= ancien_hold {
                ecrire_bits(&self.hold[canal], peak[canal]);
                self.hold_restant[canal]
                    .store((HOLD_S * taux as f32) as u32, Ordering::Relaxed);
            } else {
                let restant = self.hold_restant[canal].load(Ordering::Relaxed);
                let restant = restant.saturating_sub(frames as u32);
                self.hold_restant[canal].store(restant, Ordering::Relaxed);
                if restant == 0 {
                    ecrire_bits(&self.hold[canal], ancien_hold * facteur);
                }
            }

            if peak[canal] >= SEUIL_ECRETAGE {
                self.ecretage.store(1, Ordering::Relaxed);
            }
        }
    }

    /// Applique une décroissance temporelle **sans nouvel échantillon**
    /// (périphérique silencieux ou paquet manquant) : le vu-mètre ne gèle
    /// jamais sur un pic ancien.
    pub fn decroitre(&self, dt_s: f32, taux: u32) {
        if dt_s <= 0.0 || taux == 0 {
            return;
        }
        let facteur = 10f32.powf(-DECROISSANCE_DB_S * dt_s / 20.0);
        let alpha = (-dt_s / RMS_TAU_S).exp();
        let frames = (dt_s * taux as f32) as u32;
        for canal in 0..2 {
            ecrire_bits(&self.peak[canal], lire_bits(&self.peak[canal]) * facteur);
            ecrire_bits(&self.rms[canal], lire_bits(&self.rms[canal]) * alpha.sqrt());
            let ancien_hold = lire_bits(&self.hold[canal]);
            let restant = self.hold_restant[canal]
                .load(Ordering::Relaxed)
                .saturating_sub(frames);
            self.hold_restant[canal].store(restant, Ordering::Relaxed);
            if restant == 0 {
                ecrire_bits(&self.hold[canal], ancien_hold * facteur);
            }
        }
    }

    /// Photographie des niveaux (dBFS).
    pub fn lire(&self) -> NiveauAudio {        let canal = |c: usize| CanalNiveau {
            peak_db: db(lire_bits(&self.peak[c])),
            rms_db: db(lire_bits(&self.rms[c])),
            hold_db: db(lire_bits(&self.hold[c])),
        };
        NiveauAudio {
            gauche: canal(0),
            droite: canal(1),
            ecretage: self.ecretage.load(Ordering::Relaxed) != 0,
        }
    }

    /// Remet tout à zéro (démarrage de session, bouton « effacer l'écrêtage »).
    pub fn reinitialiser(&self) {
        for canal in 0..2 {
            ecrire_bits(&self.peak[canal], 0.0);
            ecrire_bits(&self.rms[canal], 0.0);
            ecrire_bits(&self.hold[canal], 0.0);
            self.hold_restant[canal].store(0, Ordering::Relaxed);
        }
        self.ecretage_reset();
    }

    /// Efface uniquement le latch d'écrêtage (les niveaux continuent).
    pub fn ecretage_reset(&self) {
        self.ecretage.store(0, Ordering::Relaxed);
    }
}

/// Source audio ouverte (loopback ou micro) — wrapper simple pour les
/// harnais (`examples/audio_capture.rs`).
pub struct SourceAudio {
    interne: capture::Capteur,
    niveaux: Arc<NiveauxPartages>,
}

impl SourceAudio {
    /// Loopback du périphérique de sortie par défaut.
    pub fn sortie() -> Result<Self, ErreurAudio> {
        Self::sur(Sens::Sortie, None)
    }

    /// Périphérique d'entrée par défaut (micro).
    pub fn entree() -> Result<Self, ErreurAudio> {
        Self::sur(Sens::Entree, None)
    }

    /// Source sur un périphérique précis (`None` = défaut système).
    pub fn sur(sens: Sens, peripherique: Option<&str>) -> Result<Self, ErreurAudio> {
        let niveaux = Arc::new(NiveauxPartages::nouveau());
        let (interne, _) = capture::ouvrir(sens, peripherique, Arc::clone(&niveaux))?;
        Ok(Self { interne, niveaux })
    }

    /// Attend (≤ 150 ms) puis ajoute les échantillons **f32 entrelacés**.
    /// Rend le nombre d'échantillons ajoutés (0 = silence / timeout).
    pub fn lire(&mut self, sortie: &mut Vec<f32>) -> Result<usize, ErreurAudio> {
        self.interne.lire(sortie)
    }

    pub fn format(&self) -> FormatAudio {
        self.interne.format()
    }

    /// Compteurs de niveau de cette source (pré-fader).
    pub fn niveaux(&self) -> Arc<NiveauxPartages> {
        Arc::clone(&self.niveaux)
    }

    pub fn arreter(&mut self) {
        self.interne.arreter();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pic_rms_et_hold() {
        let niveaux = NiveauxPartages::nouveau();
        // 10 ms à 48 kHz : gauche 0,5 (−6,02 dB), droite 0,25 (−12,04 dB).
        let bloc: Vec<f32> = (0..480).flat_map(|_| [0.5f32, -0.25]).collect();
        // 2 s de signal : le RMS lissé (τ = 0,3 s) a convergé.
        for _ in 0..200 {
            niveaux.maj(&bloc, 2, 48_000);
        }
        let l = niveaux.lire();
        assert!((l.gauche.peak_db - (-6.0206)).abs() < 0.1, "{:?}", l.gauche);
        assert!((l.droite.peak_db - (-12.0412)).abs() < 0.1, "{:?}", l.droite);
        assert!((l.gauche.rms_db - (-6.0206)).abs() < 0.1, "{:?}", l.gauche);
        assert!((l.gauche.hold_db - (-6.0206)).abs() < 0.1);

        // Silence : le pic décroît, le hold tient (1,5 s pas écoulées).
        let silence = vec![0.0f32; 480 * 2];
        niveaux.maj(&silence, 2, 48_000);
        let apres = niveaux.lire();
        assert!(apres.gauche.peak_db < l.gauche.peak_db);
        assert!((apres.gauche.hold_db - l.gauche.hold_db).abs() < 0.01);
    }

    #[test]
    fn ecretage_latche_et_reset() {
        let niveaux = NiveauxPartages::nouveau();
        niveaux.maj(&[1.0f32; 8], 2, 48_000);
        assert!(niveaux.lire().ecretage);
        // Le latch survit à un bloc silencieux.
        niveaux.maj(&[0.0f32; 8], 2, 48_000);
        assert!(niveaux.lire().ecretage);
        niveaux.ecretage_reset();
        assert!(!niveaux.lire().ecretage);
        niveaux.reinitialiser();
        assert_eq!(niveaux.lire().gauche.peak_db, PLANCHER_DB);
    }

    #[test]
    fn mono_duplique_sur_les_deux_canaux() {
        let niveaux = NiveauxPartages::nouveau();
        niveaux.maj(&[0.5f32; 480], 1, 48_000);
        let l = niveaux.lire();
        assert!((l.gauche.peak_db - l.droite.peak_db).abs() < 0.001);
        assert!((l.gauche.rms_db - l.droite.rms_db).abs() < 0.001);
    }

    #[test]
    fn bloc_vide_ou_format_nul_est_ignore() {
        let niveaux = NiveauxPartages::nouveau();
        niveaux.maj(&[], 2, 48_000);
        niveaux.maj(&[1.0, 1.0], 0, 48_000);
        niveaux.maj(&[1.0, 1.0], 2, 0);
        assert_eq!(niveaux.lire(), NiveauAudio::SILENCE);
    }
}
