//! StreamOS — moteur natif de capture, composition et encodage (sans OBS).
//!
//! Contrat technique : `docs/features/moteur-natif.md`.
//! Plan, phases et lots : `PLAN-MOTEUR-NATIF.md`.
//!
//! Règles de dépendance (inviolables) :
//! - ce crate ne dépend **jamais** de Tauri, de `src/` ni du domaine ;
//! - il ne connaît que `sos-scene`, `sos-moteur-api` et `windows`.
//!
//! État au LOT 0.2a : socle + énumération des adaptateurs. Rien d'autre.

pub mod gpu;

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
