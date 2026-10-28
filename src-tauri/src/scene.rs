//! Ré-export des contrats de scène — la source de vérité est `sos-scene`.
//!
//! Le module reste privé à l'app (`mod scene;` dans `lib.rs`) : tous les
//! chemins `crate::scene::…` existants continuent de résoudre, et le mode OBS
//! n'a pas à connaître le découpage en crates.

pub use sos_scene::*;
