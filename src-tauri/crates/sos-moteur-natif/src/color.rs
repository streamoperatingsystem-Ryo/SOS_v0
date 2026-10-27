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

    // Plan Y — BT.709 plage limitée, arithmétique entière (pas de f32).
    // Y = ((47R + 157G + 16B + 128) >> 8) + 16
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let b = pixels[i] as i32;
            let g = pixels[i + 1] as i32;
            let r = pixels[i + 2] as i32;
            let yv = ((47 * r + 157 * g + 16 * b + 128) >> 8) + 16;
            nv12[y * w + x] = yv.clamp(16, 235) as u8;
        }
    }

    // Plan UV (sous-échantillonnage 2×2, moyennes entières).
    let base_uv = taille_y;
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let mut sr = 0i32;
            let mut sg = 0i32;
            let mut sb = 0i32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = ((y + dy) * w + (x + dx)) * 4;
                    sb += pixels[i] as i32;
                    sg += pixels[i + 1] as i32;
                    sr += pixels[i + 2] as i32;
                }
            }
            // Moyenne des 4 pixels (division par 4), puis conversion.
            let r = sr >> 2;
            let g = sg >> 2;
            let b = sb >> 2;
            let u = (((-26 * r - 87 * g + 112 * b + 128) >> 8) + 128).clamp(16, 240);
            let v = (((112 * r - 102 * g - 10 * b + 128) >> 8) + 128).clamp(16, 240);
            let index = base_uv + (y / 2) * w + x;
            nv12[index] = u as u8;
            nv12[index + 1] = v as u8;
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
