//! Audio — capture WASAPI (loopback sortie + micro), **sans dépendance tierce**.
//!
//! Liaisons `windows` directes : `IMMDeviceEnumerator` → `IAudioClient` →
//! `IAudioCaptureClient`. Le format effectif est celui du mixage du
//! périphérique (f32 48 kHz stéréo en pratique) ; PCM 16 bits accepté aussi.

pub mod capture;

use std::fmt;

/// Sens de capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sens {
    /// Loopback du périphérique de sortie par défaut (= « desktop audio »).
    Sortie,
    /// Périphérique d'entrée par défaut (micro).
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

/// Source audio ouverte (loopback ou micro).
pub struct SourceAudio {
    interne: capture::Capteur,
}

impl SourceAudio {
    /// Loopback du périphérique de sortie par défaut.
    pub fn sortie() -> Result<Self, ErreurAudio> {
        Ok(Self {
            interne: capture::ouvrir(Sens::Sortie)?,
        })
    }

    /// Périphérique d'entrée par défaut (micro).
    pub fn entree() -> Result<Self, ErreurAudio> {
        Ok(Self {
            interne: capture::ouvrir(Sens::Entree)?,
        })
    }

    /// Attend (≤ 150 ms) puis ajoute les échantillons **f32 entrelacés**.
    /// Rend le nombre d'échantillons ajoutés (0 = silence / timeout).
    pub fn lire(&mut self, sortie: &mut Vec<f32>) -> Result<usize, ErreurAudio> {
        self.interne.lire(sortie)
    }

    pub fn format(&self) -> FormatAudio {
        self.interne.format()
    }

    pub fn arreter(&mut self) {
        self.interne.arreter();
    }
}
