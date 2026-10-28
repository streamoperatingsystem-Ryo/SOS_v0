//! Mixage audio — addition de sources f32 entrelacées, gain par source.
//!
//! Utilisé pour le mix loopback + micro. Les longueurs différentes sont
//! complétées par du silence ; le résultat est borné à [-1, 1] pour ne jamais
//! dépasser dans l'encodeur.

/// Mélange des entrées (échantillons f32 **entrelacés**) avec un gain par
/// source. La sortie a la longueur de l'entrée la plus longue.
pub fn mixer(entrees: &[(&[f32], f32)]) -> Vec<f32> {
    let longueur = entrees.iter().map(|(e, _)| e.len()).max().unwrap_or(0);
    let mut sortie = vec![0.0f32; longueur];
    for (echantillons, gain) in entrees {
        for (i, &valeur) in echantillons.iter().enumerate() {
            sortie[i] += valeur * gain;
        }
    }
    for valeur in &mut sortie {
        *valeur = valeur.clamp(-1.0, 1.0);
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proche(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn additionne_deux_sources() {
        let sortie = mixer(&[(&[0.1, 0.2], 1.0), (&[0.3, 0.4], 1.0)]);
        assert_eq!(sortie.len(), 2);
        assert!(proche(sortie[0], 0.4));
        assert!(proche(sortie[1], 0.6));
    }

    #[test]
    fn applique_le_gain() {
        let sortie = mixer(&[(&[1.0, -0.5], 0.5)]);
        assert!(proche(sortie[0], 0.5));
        assert!(proche(sortie[1], -0.25));
    }

    #[test]
    fn complete_les_sources_courtes() {
        let sortie = mixer(&[(&[0.2, 0.2, 0.2], 1.0), (&[0.1], 1.0)]);
        assert_eq!(sortie.len(), 3);
        assert!(proche(sortie[0], 0.3));
        assert!(proche(sortie[1], 0.2));
        assert!(proche(sortie[2], 0.2));
    }

    #[test]
    fn sature_au_lieu_de_depasser() {
        let sortie = mixer(&[(&[0.8, -0.8], 1.0), (&[0.8, -0.8], 1.0)]);
        assert!(proche(sortie[0], 1.0));
        assert!(proche(sortie[1], -1.0));
    }

    #[test]
    fn vide_donne_vide() {
        assert!(mixer(&[]).is_empty());
        assert!(mixer(&[(&[], 1.0)]).is_empty());
    }
}
