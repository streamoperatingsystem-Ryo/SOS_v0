//! Masque alpha PNG pour le filtre OBS « Image mask/Blend » (mask_filter,
//! mode mask_alpha_filter.effect) : découpe les sources SOS-Caméra /
//! SOS-Caméra-Fond à la forme du cadre SVG du widget caméra.
//!
//! Deux briques pures (std seul, aucune I/O, aucune dépendance) :
//! - `rasteriser_polygone` : polygone fermé (points en pixels de la source)
//!   → bitmap de couverture alpha 0-255 anti-aliasé (scanline à franchissements
//!   triés, règle even-odd — même sémantique que le fill() du canvas 2D de
//!   diffusion.html, qui découpe le trou à la même forme).
//! - `encoder_png_rgba` : couverture → PNG RGBA (blanc + alpha), deflate
//!   « stored » (non compressé) : l'encodeur tient en quelques dizaines de
//!   lignes sans crate, et le masque reste petit (alpha binaire lissé).
//!
//! Fail-open : entrée invalide (moins de 3 points, coordonnée non finie)
//! → couverture PLEINE (caméra rectangulaire = comportement sans masque),
//! jamais une couverture vide qui rendrait la caméra invisible.

/// Rastérise un polygone fermé en bitmap de couverture alpha (w×h octets,
/// row-major). Anti-aliasing par ligne : la scanline coupe à yc = y + 0.5,
/// les franchissements d'arêtes triés forment des spans even-odd [x0, x1],
/// la couverture d'un pixel = recouvrement fractionnaire du span avec
/// [px, px+1] (le filtre OBS échantillonne le masque en Linear : les bords
/// lissés restent lisses après la mise à l'échelle de l'item).
pub fn rasteriser_polygone(points: &[(f64, f64)], w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 {
        return vec![255u8; w * h];
    }
    let invalide = points.len() < 3
        || !points.iter().all(|(x, y)| x.is_finite() && y.is_finite());
    if invalide {
        return vec![255u8; w * h];
    }

    let mut couverture = vec![0u8; w * h];
    let mut ligne = vec![0f64; w]; // accumulateur de couverture de la ligne
    let mut xs: Vec<f64> = Vec::with_capacity(points.len());
    for y in 0..h {
        for v in ligne.iter_mut() {
            *v = 0.0;
        }
        let yc = y as f64 + 0.5;
        // Franchissements de la scanline par les arêtes (horizontales ignorées).
        xs.clear();
        for i in 0..points.len() {
            let (x0, y0) = points[i];
            let (x1, y1) = points[(i + 1) % points.len()];
            let (yh, yb) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            if yc < yh || yc >= yb || y0 == y1 {
                continue;
            }
            let t = (yc - y0) / (y1 - y0);
            xs.push(x0 + t * (x1 - x0));
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Spans even-odd : paires consécutives de franchissements triés.
        let mut k = 0;
        while k + 1 < xs.len() {
            let (xa, xb) = (xs[k], xs[k + 1]);
            k += 2;
            // Clip du span au bitmap, puis couverture fractionnaire par pixel.
            let xa = xa.max(0.0);
            let xb = xb.min(w as f64);
            if xb <= xa {
                continue;
            }
            let debut = xa.floor() as usize;
            let fin = xb.ceil().min(w as f64) as usize;
            for (i, v) in ligne[debut..fin].iter_mut().enumerate() {
                let px = debut + i;
                let borne_g = (px as f64).max(xa);
                let borne_d = (px + 1) as f64;
                *v += (borne_d.min(xb) - borne_g).clamp(0.0, 1.0);
            }
        }
        for x in 0..w {
            couverture[y * w + x] = (ligne[x] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    couverture
}

/// CRC-32 PNG (polynôme 0xEDB88320) de `donnees`, calcul bit à bit (les
/// données concernées font quelques Mo au pire — pas besoin de table).
fn crc32(donnees: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &octet in donnees {
        crc ^= octet as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Adler-32 (sommes mod 65521) — contrôle de fin du flux zlib.
fn adler32(donnees: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &octet in donnees {
        a = (a + octet as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Écrit un chunk PNG : [longueur u32 BE][type][donnees][crc32 u32 BE].
fn chunk_png(sortie: &mut Vec<u8>, type_: &[u8; 4], donnees: &[u8]) {
    sortie.extend_from_slice(&(donnees.len() as u32).to_be_bytes());
    sortie.extend_from_slice(type_);
    sortie.extend_from_slice(donnees);
    let mut entree_crc = Vec::with_capacity(4 + donnees.len());
    entree_crc.extend_from_slice(type_);
    entree_crc.extend_from_slice(donnees);
    sortie.extend_from_slice(&crc32(&entree_crc).to_be_bytes());
}

/// Assemble le PNG final : signature + IHDR (w×h, RGBA profondeur 8,
/// non entrelacé) + IDAT (zlib « stored » de `brut` = scanlines déjà
/// filtrées : octet filtre 0 + pixels par ligne) + IEND. Partagé par
/// encoder_png_rgba (masque caméra) et encoder_png_pixels (miniatures).
fn assembler_png(brut: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut png = Vec::with_capacity(64 + brut.len());
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    // IHDR : dims, profondeur 8, couleur 6 (RGBA), deflate, filtre 0, entrelacement 0.
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk_png(&mut png, b"IHDR", &ihdr);

    // zlib « stored » : blocs bruts + Adler-32 final (big-endian).
    let mut zlib = Vec::with_capacity(brut.len() + (brut.len() / 65535 + 1) * 5 + 8);
    zlib.extend_from_slice(&[0x78, 0x01]);
    let mut reste: &[u8] = brut;
    loop {
        let taille = reste.len().min(65535);
        let dernier = taille == reste.len();
        zlib.push(if dernier { 1 } else { 0 });
        zlib.extend_from_slice(&(taille as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(taille as u16)).to_le_bytes());
        zlib.extend_from_slice(&reste[..taille]);
        reste = &reste[taille..];
        if dernier {
            break;
        }
    }
    zlib.extend_from_slice(&adler32(brut).to_be_bytes());
    chunk_png(&mut png, b"IDAT", &zlib);
    chunk_png(&mut png, b"IEND", &[]);
    png
}

/// Encode une couverture alpha en PNG RGBA (blanc + alpha, profondeur 8,
/// non entrelacé). Chaque scanline = octet filtre 0 + w×4 octets. Le flux
/// zlib est en deflate « stored » : en-tête 0x78 0x01, blocs ≤ 65535 octets
/// (LEN u16 LE + NLEN = !LEN), Adler-32 des données brutes en fin.
pub fn encoder_png_rgba(couverture: &[u8], w: u32, h: u32) -> Vec<u8> {
    debug_assert_eq!(couverture.len(), w as usize * h as usize);
    let w = w as usize;

    // Données brutes : scanlines filtrées (filtre 0 = aucune prédiction).
    let mut brut = Vec::with_capacity((w * 4 + 1) * h as usize);
    for y in 0..h as usize {
        brut.push(0);
        for x in 0..w {
            brut.extend_from_slice(&[255, 255, 255, couverture[y * w + x]]);
        }
    }
    assembler_png(&brut, w as u32, h)
}

/// Encode des pixels RGBA bruts (w×h×4 octets, row-major) en PNG — même
/// conteneur deflate « stored » qu'encoder_png_rgba. Utilisé par les
/// miniatures WGC du sélecteur de capture (pc_thumb) : la vignette reste
/// modeste (≤ 256 px de large) donc l'absence de compression est ok.
pub fn encoder_png_pixels(pixels: &[u8], w: u32, h: u32) -> Vec<u8> {
    debug_assert_eq!(pixels.len(), w as usize * h as usize * 4);
    let stride = w as usize * 4;
    let mut brut = Vec::with_capacity((stride + 1) * h as usize);
    for y in 0..h as usize {
        brut.push(0);
        brut.extend_from_slice(&pixels[y * stride..(y + 1) * stride]);
    }
    assembler_png(&brut, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_plein() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)];
        let cov = rasteriser_polygone(&pts, 10, 6);
        assert_eq!(cov.len(), 60);
        assert!(cov.iter().all(|&a| a == 255), "rectangle plein attendu");
    }

    #[test]
    fn polygone_hors_cadre() {
        let pts = vec![(-50.0, -50.0), (-40.0, -50.0), (-40.0, -40.0), (-50.0, -40.0)];
        let cov = rasteriser_polygone(&pts, 10, 6);
        assert!(cov.iter().all(|&a| a == 0), "polygone hors cadre → tout vide");
    }

    #[test]
    fn triangle_bord_lisse() {
        // Triangle rectangle : coin intérieur plein, coin opposé vide, et un
        // pixel de bord diagonal partiellement couvert (anti-aliasing).
        let pts = vec![(0.0, 0.0), (20.0, 0.0), (0.0, 20.0)];
        let cov = rasteriser_polygone(&pts, 20, 20);
        assert_eq!(cov[0], 255, "coin haut-gauche intérieur");
        assert_eq!(cov[19 * 20 + 19], 0, "coin bas-droit extérieur");
        // Ligne y=4.5 : l'hypoténuse (x+y=20) coupe à x=15.5 → pixel 15
        // couvert à moitié ([15,16]∩[0,15.5]), pixel 16 vide.
        let bord = cov[4 * 20 + 15] as u32;
        assert!(bord > 0 && bord < 255, "pixel de bord diagonal partiel: {}", bord);
        assert_eq!(cov[4 * 20 + 16], 0, "pixel 16 au-delà de l'hypoténuse");
    }

    #[test]
    fn entree_invalide_fail_open() {
        assert!(rasteriser_polygone(&[], 4, 4).iter().all(|&a| a == 255));
        let nan = vec![(f64::NAN, 0.0), (1.0, 0.0), (1.0, 1.0)];
        assert!(rasteriser_polygone(&nan, 4, 4).iter().all(|&a| a == 255));
    }

    #[test]
    fn png_structure_complete() {
        let cov = vec![255u8, 128, 0, 255, 0, 0]; // 3×2
        let png = encoder_png_rgba(&cov, 3, 2);
        // Signature.
        assert_eq!(&png[0..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // IHDR : dims 3×2, profondeur 8, couleur RGBA (6).
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &3u32.to_be_bytes());
        assert_eq!(&png[20..24], &2u32.to_be_bytes());
        assert_eq!(png[24], 8);
        assert_eq!(png[25], 6);
        // IEND en fin de flux.
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");

        // Décodage des blocs « stored » : les scanlines doivent reproduire
        // filtre 0 + RGBA blanc/alpha, et l'Adler-32 doit couvrir le brut.
        let mut idat = Vec::new();
        let mut i = 8;
        while i + 12 <= png.len() {
            let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
            let typ = &png[i + 4..i + 8];
            if typ == b"IDAT" {
                idat.extend_from_slice(&png[i + 8..i + 8 + len]);
            }
            i += 12 + len;
        }
        assert_eq!(&idat[0..2], &[0x78, 0x01], "en-tête zlib");
        let mut brut = Vec::new();
        let mut p = 2;
        loop {
            let dernier = idat[p] & 1 == 1;
            let len = u16::from_le_bytes([idat[p + 1], idat[p + 2]]) as usize;
            let nlen = u16::from_le_bytes([idat[p + 3], idat[p + 4]]);
            assert_eq!(!(len as u16), nlen, "NLEN = !LEN");
            brut.extend_from_slice(&idat[p + 5..p + 5 + len]);
            p += 5 + len;
            if dernier {
                break;
            }
        }
        assert_eq!(brut.len(), 2 * (1 + 3 * 4), "2 scanlines de 1+12 octets");
        let attendu: Vec<u8> = vec![
            0, 255, 255, 255, 255, 255, 255, 255, 128, 255, 255, 255, 0,
            0, 255, 255, 255, 255, 255, 255, 255, 0, 255, 255, 255, 0,
        ];
        assert_eq!(brut, attendu);
        let adler = u32::from_be_bytes([idat[p], idat[p + 1], idat[p + 2], idat[p + 3]]);
        assert_eq!(adler, adler32(&brut), "Adler-32 du flux");
    }
}
