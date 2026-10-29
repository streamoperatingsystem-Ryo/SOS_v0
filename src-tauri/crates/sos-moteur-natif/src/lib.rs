//! StreamOS — moteur natif de capture, composition et encodage (sans OBS).
//!
//! Contrat technique : `docs/features/moteur-natif.md`.
//! Plan, phases et lots : `PLAN-MOTEUR-NATIF.md`.
//!
//! Règles de dépendance (inviolables) :
//! - ce crate ne dépend **jamais** de Tauri, de `src/` ni du domaine ;
//! - il ne connaît que `sos-scene`, `sos-moteur-api` et `windows`.
//!
//! État (2026-10-29) : enregistrement MP4/MKV + aperçu natif fonctionnels.
//! Chemin de production (`moteur.rs`) : capture WGC (moniteurs/fenêtres) →
//! canvas BGRA composé **100 % Direct2D** (médias, textes, cadres, voiles,
//! chat, input viewer, morphs, plans à perspective) → conversion NV12 GPU →
//! encodeur (NVENC direct, repli MFT) → `FfmpegSink` → audio loopback/micro
//! muxé en AAC. Caméra MF native (`capture/camera.rs`). Absents assumés :
//! RTMP/SRT, fond vert (§24).
//!
//! Modules conservés pour les **harnais `examples/`** uniquement — jamais
//! appelés par le chemin de production : `compositor`, `color`, et parties
//! de `output/file.rs`, `encode/ffmpeg.rs`, `capture/wgc.rs` (voir leurs
//! en-têtes).

pub mod audio;
pub mod cadres;
pub mod capture;
pub mod chat;
pub mod clock;
pub mod color;
pub mod color_gpu;
pub mod compositor;
pub mod compteurs;
pub mod encode;
pub mod gouverneur;
pub mod gpu;
pub mod input;
pub mod media;
pub mod morph;
pub mod moteur;
pub mod output;
pub mod preview;
pub mod scene_sources;
pub mod texte;

/// Version du crate (affichée par le diagnostic, utile au support).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_non_vide() {
        assert!(!version().is_empty());
    }
}
