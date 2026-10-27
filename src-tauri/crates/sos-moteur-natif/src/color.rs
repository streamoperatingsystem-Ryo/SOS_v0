//! Conversions de format couleur.
//!
//! ⚠️ **Spike uniquement** (lot 0.2c) : cette conversion BGRA→NV12 est faite
//! en **CPU**. Le lot 0.2d la remplacera par `ID3D11VideoProcessor` (GPU,
//! zéro-copie) avant toute mesure de performance P0 — sinon le spike serait
//! pénalisé par un coût qui n'existera pas en production.
//!
//! Espace colorimétrique : BT.709, plage **limitée** (16-235 / 16-240), ce
//! qu'attendent les encodeurs matériels.

use crate::encode::ErreurEncodage;

/// Convertit une frame BGRA8 (lignes de haut en bas) en NV12.
///
/// Le NV12 produit est : plan Y (W×H) puis plan UV entrelacé (W/2 × H/2).
/// Largeur et hauteur doivent être **paires** (exigence des encodeurs).
pub fn bgra_vers_nv12(
    pixels: &[u8],
    largeur: u32,
    hauteur: u32,
) -> Result<Vec<u8>, ErreurEncodage> {
    if largeur == 0 || hauteur == 0 {
        return Err(ErreurEncodage::Conversion("dimensions nulles".into()));
    }
    if largeur % 2 != 0 || hauteur % 2 != 0 {
        return Err(ErreurEncodage::Conversion(format!(
            "dimensions impaires {largeur}×{hauteur} (NV12 exige des valeurs paires)"
        )));
    }

    let attendu = largeur as usize * hauteur as usize * 4;
    if pixels.len() != attendu {
        return Err(ErreurEncodage::Conversion(format!(
            "BGRA {} octets, {} attendus",
            pixels.len(),
            attendu
        )));
    }

    let w = largeur as usize;
    let h = hauteur as usize;
    let taille_y = w * h;
    let taille_uv = w * h / 2;
    let mut nv12 = vec![0u8; taille_y + taille_uv];

    // Plan Y
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let b = pixels[i] as f32 / 255.0;
            let g = pixels[i + 1] as f32 / 255.0;
            let r = pixels[i + 2] as f32 / 255.0;

            let y_prime = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            nv12[y * w + x] = (16.0 + 219.0 * y_prime).round().clamp(16.0, 235.0) as u8;
        }
    }

    // Plan UV (sous-échantillonnage 2×2)
    let base_uv = taille_y;
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let mut somme_u = 0.0f32;
            let mut somme_v = 0.0f32;

            for dy in 0..2 {
                for dx in 0..2 {
                    let i = ((y + dy) * w + (x + dx)) * 4;
                    let b = pixels[i] as f32 / 255.0;
                    let g = pixels[i + 1] as f32 / 255.0;
                    let r = pixels[i + 2] as f32 / 255.0;

                    let y_prime = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    somme_u += (b - y_prime) / 1.8556;
                    somme_v += (r - y_prime) / 1.5748;
                }
            }

            let u = (128.0 + 224.0 * (somme_u / 4.0)).round().clamp(16.0, 240.0) as u8;
            let v = (128.0 + 224.0 * (somme_v / 4.0)).round().clamp(16.0, 240.0) as u8;

            let index = base_uv + (y / 2) * w + x;
            nv12[index] = u;
            nv12[index + 1] = v;
        }
    }

    Ok(nv12)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noir_donne_y16_uv128() {
        let pixels = vec![0u8; 4 * 4 * 4]; // 4×4 BGRA noir
        let nv12 = bgra_vers_nv12(&pixels, 4, 4).unwrap();
        assert_eq!(nv12.len(), 4 * 4 * 3 / 2);
        assert!(nv12[..16].iter().all(|&y| y == 16));
        assert!(nv12[16..].iter().all(|&c| c == 128));
    }

    #[test]
    fn blanc_donne_y235() {
        let mut pixels = vec![0u8; 4 * 4 * 4];
        for px in pixels.chunks_exact_mut(4) {
            px[0] = 255;
            px[1] = 255;
            px[2] = 255;
            px[3] = 255;
        }
        let nv12 = bgra_vers_nv12(&pixels, 4, 4).unwrap();
        assert!(nv12[..16].iter().all(|&y| y == 235));
    }

    #[test]
    fn dimensions_impaires_refusees() {
        let pixels = vec![0u8; 3 * 3 * 4];
        assert!(bgra_vers_nv12(&pixels, 3, 3).is_err());
    }

    #[test]
    fn taille_incoherente_refusee() {
        let pixels = vec![0u8; 10];
        assert!(bgra_vers_nv12(&pixels, 4, 4).is_err());
    }
}
