//! Écrivain ZIP minimal — entrées « stored » (méthode 0, sans compression).
//!
//! Utilisé par l'export « tout en .zip » des scènes (scenes::exporter_tout_zip).
//! Les médias exportés sont déjà des formats compressés (jpg/png/mp4/webp) et
//! les JSON sont minuscules : le deflate n'y gagnerait rien. Zéro dépendance —
//! même esprit que l'encodeur PNG maison de mask_png.rs.
//!
//! Format écrit : local file headers + central directory + EOCD, méthode 0,
//! noms UTF-8 (flag général 0x800 — accents des noms de scènes OK), séparateur
//! `/` dans les chemins (exigé par la spec APPNOTE).
//! Limites ZIP classiques gardées : entrée ≤ 4 Gio, archive ≤ 4 Gio,
//! ≤ 65535 entrées. Pas de data descriptor : le CRC est calculé avant le
//! header (ajouter_fichier lit le fichier deux fois — disque local, négligeable).
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

// ===== CRC32 IEEE (polynôme réfléchi 0xEDB88320) =====

fn crc32_table() -> &'static [u32; 256] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    })
}

/// CRC32 d'un flux (lecture par blocs de 64 Ko — pas de chargement en RAM).
fn crc32_stream(r: &mut impl Read) -> io::Result<u32> {
    let table = crc32_table();
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for &b in &buf[..n] {
            crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
    }
    Ok(crc ^ 0xFFFF_FFFF)
}

// ===== Date/heure DOS (champs mtime/mdate des entrées) =====

fn dos_datetime() -> (u16, u16) {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let annee = (now.year().clamp(1980, 2107) as u16) - 1980;
    let date = (annee << 9) | ((now.month() as u16) << 5) | now.day() as u16;
    let time =
        ((now.hour() as u16) << 11) | ((now.minute() as u16) << 5) | (now.second() as u16 / 2);
    (time, date)
}

// ===== Écrivain =====

/// Métadonnées d'une entrée écrite, conservées pour le central directory.
struct EntreeCentrale {
    nom: Vec<u8>,
    crc32: u32,
    taille: u32,
    offset: u32,
    dos_time: u16,
    dos_date: u16,
}

/// Écrivain ZIP append-only : local headers en flux, central directory +
/// EOCD au `terminer`. `out` reste append-only (pas de Seek exigé —
/// BufWriter<File> suffit pour l'export disque).
pub struct ZipEcrivain<W: Write> {
    out: W,
    entrees: Vec<EntreeCentrale>,
    offset: u64,
    dos_time: u16,
    dos_date: u16,
}

fn err<S: Into<String>>(msg: S) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}

impl<W: Write> ZipEcrivain<W> {
    pub fn new(out: W) -> Self {
        let (dos_time, dos_date) = dos_datetime();
        Self {
            out,
            entrees: Vec::new(),
            offset: 0,
            dos_time,
            dos_date,
        }
    }

    fn ecrire(&mut self, buf: &[u8]) -> io::Result<()> {
        self.out.write_all(buf)?;
        self.offset += buf.len() as u64;
        Ok(())
    }

    fn ecrire_u16(&mut self, v: u16) -> io::Result<()> {
        self.ecrire(&v.to_le_bytes())
    }

    fn ecrire_u32(&mut self, v: u32) -> io::Result<()> {
        self.ecrire(&v.to_le_bytes())
    }

    /// Écrit le local file header d'une entrée et l'enregistre pour le
    /// central directory. Les données doivent suivre immédiatement.
    fn header_local(&mut self, nom: &str, crc32: u32, taille: u32) -> io::Result<()> {
        let nom_b = nom.as_bytes();
        if nom_b.len() > u16::MAX as usize {
            return Err(err(format!("Nom d'entrée ZIP trop long : {}", nom)));
        }
        if self.entrees.len() >= u16::MAX as usize {
            return Err(err("Plus de 65535 entrées : limite ZIP classique"));
        }
        if self.offset + taille as u64 > u32::MAX as u64 {
            return Err(err("Archive > 4 Gio : limite ZIP classique"));
        }
        let offset = self.offset as u32;
        self.ecrire_u32(0x0403_4B50)?; // signature local header
        self.ecrire_u16(20)?; // version needed (2.0)
        self.ecrire_u16(0x0800)?; // flag : noms UTF-8
        self.ecrire_u16(0)?; // méthode 0 = stored
        self.ecrire_u16(self.dos_time)?;
        self.ecrire_u16(self.dos_date)?;
        self.ecrire_u32(crc32)?;
        self.ecrire_u32(taille)?; // taille compressée = taille (stored)
        self.ecrire_u32(taille)?;
        self.ecrire_u16(nom_b.len() as u16)?;
        self.ecrire_u16(0)?; // pas de champ extra
        self.ecrire(nom_b)?;
        self.entrees.push(EntreeCentrale {
            nom: nom_b.to_vec(),
            crc32,
            taille,
            offset,
            dos_time: self.dos_time,
            dos_date: self.dos_date,
        });
        Ok(())
    }

    /// Ajoute une entrée depuis des octets en mémoire (JSON des scènes).
    pub fn ajouter_octets(&mut self, nom: &str, data: &[u8]) -> io::Result<()> {
        if data.len() as u64 > u32::MAX as u64 {
            return Err(err(format!("Entrée > 4 Gio : {}", nom)));
        }
        let crc = crc32_stream(&mut &data[..])?;
        self.header_local(nom, crc, data.len() as u32)?;
        self.ecrire(data)
    }

    /// Ajoute une entrée depuis un fichier disque. Le fichier est lu deux
    /// fois (CRC puis copie en flux) — disque local, négligeable, et ça
    /// évite de charger les médias en RAM ou d'exiger Seek sur `out`.
    ///
    /// `Ok(false)` = fichier illisible AVANT l'écriture du header : rien
    /// n'a été écrit, l'archive reste valide (l'appelant peut sauter le
    /// fichier). `Err` = échec PENDANT l'écriture : l'offset du header
    /// annoncé ne correspondrait plus aux données — l'archive serait
    /// corrompue, l'appelant doit abandonner.
    pub fn ajouter_fichier(&mut self, nom: &str, src: &Path) -> io::Result<bool> {
        let mut f = match File::open(src) {
            Ok(f) => f,
            Err(_) => return Ok(false),
        };
        let crc = match crc32_stream(&mut f) {
            Ok(c) => c,
            Err(_) => return Ok(false),
        };
        let taille = f.metadata()?.len();
        if taille > u32::MAX as u64 {
            return Err(err(format!("Fichier > 4 Gio : {}", src.display())));
        }
        f.seek(SeekFrom::Start(0))?;
        self.header_local(nom, crc, taille as u32)?;
        let n = io::copy(&mut f, &mut self.out)?;
        self.offset += n;
        Ok(true)
    }

    /// Écrit le central directory + EOCD et flush. Consomme l'écrivain.
    pub fn terminer(mut self) -> io::Result<()> {
        let cd_offset = self.offset;
        let entrees = std::mem::take(&mut self.entrees);
        for e in &entrees {
            self.ecrire_u32(0x0201_4B50)?; // signature central directory
            self.ecrire_u16(20)?; // version made by
            self.ecrire_u16(20)?; // version needed
            self.ecrire_u16(0x0800)?; // flag UTF-8
            self.ecrire_u16(0)?; // stored
            self.ecrire_u16(e.dos_time)?;
            self.ecrire_u16(e.dos_date)?;
            self.ecrire_u32(e.crc32)?;
            self.ecrire_u32(e.taille)?;
            self.ecrire_u32(e.taille)?;
            self.ecrire_u16(e.nom.len() as u16)?;
            self.ecrire_u16(0)?; // extra
            self.ecrire_u16(0)?; // commentaire
            self.ecrire_u16(0)?; // disque de départ
            self.ecrire_u16(0)?; // attributs internes
            self.ecrire_u32(0)?; // attributs externes
            self.ecrire_u32(e.offset)?;
            self.ecrire(&e.nom)?;
        }
        let cd_taille = self.offset - cd_offset;
        if cd_offset > u32::MAX as u64 || cd_taille > u32::MAX as u64 {
            return Err(err("Central directory > 4 Gio : limite ZIP classique"));
        }
        self.ecrire_u32(0x0605_4B50)?; // signature EOCD
        self.ecrire_u16(0)?; // numéro de disque
        self.ecrire_u16(0)?; // disque du central directory
        self.ecrire_u16(entrees.len() as u16)?; // entrées sur ce disque
        self.ecrire_u16(entrees.len() as u16)?; // entrées au total
        self.ecrire_u32(cd_taille as u32)?;
        self.ecrire_u32(cd_offset as u32)?;
        self.ecrire_u16(0)?; // pas de commentaire
        self.out.flush()
    }
}

// ===== Lecteur (import « tout en .zip ») =====
//
// Parse du central directory — c'est lui qui porte les vraies tailles/CRC,
// donc les zips étrangers écrits avec data descriptor (header local à 0)
// passent aussi. Méthodes supportées : 0 (stored — nos exports) et 8
// (deflate raw via flate2 — zips compressés par Windows/7-Zip).
// Gardes : multi-disque et Zip64 refusés (nos archives n'y arrivent
// jamais), noms sanitizés anti zip-slip (`..`, absolu, `:`, `\`→`/`),
// décompression bornée à la taille déclarée + 1 (anti bombe), CRC vérifié.

/// Entrée du central directory d'une archive ouverte.
struct EntreeLue {
    nom: String,
    methode: u16,
    crc32: u32,
    taille_compressee: u32,
    taille: u32,
    offset_local: u32,
}

/// Lecteur d'une archive ZIP : `ouvrir` parse le central directory une fois,
/// `entrees` liste les chemins, `lire_bytes` décompresse une entrée.
/// Le fichier reste ouvert pour les lectures à la demande.
pub struct ZipLecteur {
    f: File,
    entrees: Vec<EntreeLue>,
}

/// Taille max d'une entrée lue en mémoire (les médias font ≤ 80 Mo —
/// 512 Mo laisse une large marge contre un usize mensonger).
const MAX_ENTREE_LUE: u64 = 512 * 1024 * 1024;

/// Un nom d'entrée est acceptable s'il est relatif, sans `..`, sans `\`
/// (normalisé en `/` avant le test), sans `:` (drive Windows) et non vide.
/// Retourne le nom normalisé ou None si malveillant/invalide.
fn normaliser_nom_entree(nom: &str) -> Option<String> {
    let n = nom.replace('\\', "/");
    if n.is_empty() || n.starts_with('/') || n.contains(':') {
        return None;
    }
    if n.split('/').any(|seg| seg.is_empty() || seg == "..") {
        return None;
    }
    Some(n)
}

fn lire_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn lire_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

impl ZipLecteur {
    /// Ouvre une archive et parse son central directory. Les entrées aux
    /// noms invalides (zip-slip) sont ignorées (log warn) — le reste de
    /// l'archive reste exploitable.
    pub fn ouvrir(path: &Path) -> io::Result<Self> {
        let mut f = File::open(path)?;
        let len = f.metadata()?.len();
        if len < 22 {
            return Err(err("Fichier trop petit pour être un ZIP"));
        }

        // 1. EOCD : dans les 22 + 65535 derniers octets, signature cherchée
        //    en arrière ; le candidat est validé par sa longueur de
        //    commentaire (un commentaire peut contenir les octets de la
        //    signature).
        let queue = len.min(22 + 65535) as usize;
        f.seek(SeekFrom::End(-(queue as i64)))?;
        let mut buf = vec![0u8; queue];
        f.read_exact(&mut buf)?;
        let mut eocd_pos = None;
        for pos in (0..=buf.len() - 22).rev() {
            if buf[pos..pos + 4] == [0x50, 0x4B, 0x05, 0x06] {
                let comment_len = lire_u16(&buf, pos + 20) as usize;
                if pos + 22 + comment_len == buf.len() {
                    eocd_pos = Some(pos);
                    break;
                }
            }
        }
        let pos = eocd_pos.ok_or_else(|| err("Signature EOCD introuvable : pas un ZIP ?"))?;
        let e = &buf[pos..];
        let disque = lire_u16(e, 4);
        let disque_cd = lire_u16(e, 6);
        let nb_disque = lire_u16(e, 8);
        let nb = lire_u16(e, 10);
        if disque != 0 || disque_cd != 0 || nb_disque != nb {
            return Err(err("Archive multi-disque non supportée"));
        }
        let cd_taille = lire_u32(e, 12);
        let cd_offset = lire_u32(e, 16);
        if nb == 0xFFFF || cd_taille == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
            return Err(err("Zip64 non supporté"));
        }

        // 2. Central directory.
        f.seek(SeekFrom::Start(cd_offset as u64))?;
        let mut cd = vec![0u8; cd_taille as usize];
        f.read_exact(&mut cd)?;
        let mut entrees = Vec::with_capacity(nb as usize);
        let mut cur = 0usize;
        for _ in 0..nb {
            if cur + 46 > cd.len() || cd[cur..cur + 4] != [0x50, 0x4B, 0x01, 0x02] {
                return Err(err("Central directory tronqué ou corrompu"));
            }
            let nom_len = lire_u16(&cd, cur + 28) as usize;
            let extra_len = lire_u16(&cd, cur + 30) as usize;
            let comment_len = lire_u16(&cd, cur + 32) as usize;
            let fin_nom = cur + 46 + nom_len;
            if fin_nom > cd.len() {
                return Err(err("Central directory tronqué (nom)"));
            }
            let nom_brut = String::from_utf8_lossy(&cd[cur + 46..fin_nom]).into_owned();
            match normaliser_nom_entree(&nom_brut) {
                Some(nom) => entrees.push(EntreeLue {
                    nom,
                    methode: lire_u16(&cd, cur + 10),
                    crc32: lire_u32(&cd, cur + 16),
                    taille_compressee: lire_u32(&cd, cur + 20),
                    taille: lire_u32(&cd, cur + 24),
                    offset_local: lire_u32(&cd, cur + 42),
                }),
                None => log::warn!("ZIP : entrée au nom invalide ignorée : {}", nom_brut),
            }
            cur = fin_nom + extra_len + comment_len;
        }
        Ok(Self { f, entrees })
    }

    /// Itère les chemins des entrées (séparateur `/`, déjà normalisés).
    pub fn entrees(&self) -> impl Iterator<Item = &str> {
        self.entrees.iter().map(|e| e.nom.as_str())
    }

    /// Lit + décompresse une entrée par son chemin. Méthode 0 → octets tels
    /// quels ; méthode 8 → inflate raw borné à `taille + 1` (anti bombe).
    /// Vérifie taille et CRC32 déclarés — une entrée corrompue → Err.
    pub fn lire_bytes(&mut self, nom: &str) -> io::Result<Vec<u8>> {
        let idx = self
            .entrees
            .iter()
            .position(|e| e.nom == nom)
            .ok_or_else(|| err(format!("Entrée absente de l'archive : {}", nom)))?;
        let (methode, crc_attendu, csize, usize_attendu, offset_local) = {
            let e = &self.entrees[idx];
            (e.methode, e.crc32, e.taille_compressee, e.taille, e.offset_local)
        };
        if usize_attendu as u64 > MAX_ENTREE_LUE || csize as u64 > MAX_ENTREE_LUE {
            return Err(err(format!("Entrée trop grosse : {}", nom)));
        }

        // Header local : 30 octets fixes + nom + extra → début des données.
        self.f.seek(SeekFrom::Start(offset_local as u64))?;
        let mut lh = [0u8; 30];
        self.f.read_exact(&mut lh)?;
        if lh[0..4] != [0x50, 0x4B, 0x03, 0x04] {
            return Err(err(format!("Header local corrompu : {}", nom)));
        }
        let debut = offset_local as u64 + 30
            + lire_u16(&lh, 26) as u64
            + lire_u16(&lh, 28) as u64;
        self.f.seek(SeekFrom::Start(debut))?;
        let mut comp = vec![0u8; csize as usize];
        self.f.read_exact(&mut comp)?;

        let data = match methode {
            0 => comp,
            8 => {
                let mut out = Vec::with_capacity(usize_attendu as usize);
                let n = flate2::read::DeflateDecoder::new(&comp[..])
                    .take(usize_attendu as u64 + 1)
                    .read_to_end(&mut out)?;
                if n > usize_attendu as usize {
                    return Err(err(format!(
                        "Entrée plus grosse que déclarée après inflate : {}",
                        nom
                    )));
                }
                out
            }
            m => {
                return Err(err(format!(
                    "Méthode de compression non supportée ({}) : {}",
                    m, nom
                )))
            }
        };
        if data.len() as u64 != usize_attendu as u64 {
            return Err(err(format!("Taille décompressée incohérente : {}", nom)));
        }
        if crc32_stream(&mut &data[..])? != crc_attendu {
            return Err(err(format!("CRC32 invalide : {}", nom)));
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_vecteur_connu() {
        // Vecteur de référence : CRC32("123456789") = 0xCBF43926.
        let crc = crc32_stream(&mut &b"123456789"[..]).unwrap();
        assert_eq!(crc, 0xCBF4_3926);
    }

    #[test]
    fn zip_stored_structure() {
        let mut buf = Vec::new();
        {
            let mut zip = ZipEcrivain::new(&mut buf);
            zip.ajouter_octets("scenes/index.json", b"[]").unwrap();
            zip.ajouter_octets("medias/a.png", &[1u8, 2, 3]).unwrap();
            zip.terminer().unwrap();
        }
        // Signature local header en tête.
        assert_eq!(&buf[0..4], &[0x50, 0x4B, 0x03, 0x04]);
        // Signatures central directory + EOCD présentes.
        assert!(buf
            .windows(4)
            .any(|w| w == [0x50, 0x4B, 0x01, 0x02]));
        assert!(buf
            .windows(4)
            .any(|w| w == [0x50, 0x4B, 0x05, 0x06]));
        // Noms UTF-8 visibles en clair (stored = données lisibles).
        assert!(buf.windows(17).any(|w| w == b"scenes/index.json"));
        // EOCD : 2 entrées.
        let eocd = buf
            .windows(4)
            .position(|w| w == [0x50, 0x4B, 0x05, 0x06])
            .unwrap();
        let nb = u16::from_le_bytes([buf[eocd + 10], buf[eocd + 11]]);
        assert_eq!(nb, 2);
    }

    /// Écrit un vrai .zip sur disque pour validation par un parseur externe
    /// (Expand-Archive / zipfile) — ignoré par défaut, lancé à la demande :
    /// `cargo test zip_store::tests::zip_fichier_reel -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn zip_fichier_reel() {
        let path = std::env::temp_dir().join("sos_zip_test.zip");
        {
            let f = File::create(&path).unwrap();
            let mut zip = ZipEcrivain::new(io::BufWriter::new(f));
            zip.ajouter_octets("scenes/index.json", "[{\"id\":\"a1\",\"nom\":\"Défaut\"}]".as_bytes())
                .unwrap();
            zip.ajouter_octets("scenes/a1.json", "{\"widgets\":[]}".as_bytes())
                .unwrap();
            zip.ajouter_octets("packs/Défaut/scene.json", "{\"widgets\":[],\"nom\":\"Défaut\"}".as_bytes())
                .unwrap();
            zip.ajouter_octets("medias/musique/piste.mp3", &[0xFF, 0xFB, 0x90, 0x00])
                .unwrap();
            zip.terminer().unwrap();
        }
        eprintln!("ZIP_TEST={}", path.display());
        assert!(path.exists());
    }

    /// Round-trip : ce que ZipEcrivain écrit, ZipLecteur le relit à
    /// l'identique (nom UTF-8 + octets).
    #[test]
    fn lecteur_roundtrip_stored() {
        let path = std::env::temp_dir().join("sos_zip_rt.zip");
        {
            let f = File::create(&path).unwrap();
            let mut zip = ZipEcrivain::new(io::BufWriter::new(f));
            zip.ajouter_octets("scenes/index.json", "[1]".as_bytes()).unwrap();
            zip.ajouter_octets("packs/Défaut/scene.json", "{}".as_bytes()).unwrap();
            zip.terminer().unwrap();
        }
        let mut l = ZipLecteur::ouvrir(&path).unwrap();
        let noms: Vec<String> = l.entrees().map(|s| s.to_string()).collect();
        assert_eq!(noms, vec!["scenes/index.json", "packs/Défaut/scene.json"]);
        assert_eq!(l.lire_bytes("scenes/index.json").unwrap(), b"[1]");
        assert!(l.lire_bytes("inexistant").is_err());
        let _ = std::fs::remove_file(&path);
    }

    /// Zip méthode 8 forgé à la main (deflate raw via flate2) : le lecteur
    /// doit le lire comme les zips compressés par Windows/7-Zip.
    #[test]
    fn lecteur_zip_deflate() {
        use io::Write as _;
        let data = b"contenu deflate repete repete repete";
        let comp = {
            let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        };
        let crc = crc32_stream(&mut &data[..]).unwrap();
        let nom = b"a.txt";

        let mut buf = Vec::new();
        // Local header méthode 8.
        buf.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        buf.extend_from_slice(&20u16.to_le_bytes());
        buf.extend_from_slice(&0x0800u16.to_le_bytes());
        buf.extend_from_slice(&8u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&crc.to_le_bytes());
        buf.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(nom.len() as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(nom);
        let offset_cd = buf.len() + comp.len();
        buf.extend_from_slice(&comp);
        // Central directory.
        buf.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        buf.extend_from_slice(&20u16.to_le_bytes());
        buf.extend_from_slice(&20u16.to_le_bytes());
        buf.extend_from_slice(&0x0800u16.to_le_bytes());
        buf.extend_from_slice(&8u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&crc.to_le_bytes());
        buf.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(nom.len() as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(nom);
        let cd_taille = buf.len() - offset_cd;
        // EOCD.
        buf.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&(cd_taille as u32).to_le_bytes());
        buf.extend_from_slice(&(offset_cd as u32).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());

        let path = std::env::temp_dir().join("sos_zip_deflate.zip");
        std::fs::write(&path, &buf).unwrap();
        let mut l = ZipLecteur::ouvrir(&path).unwrap();
        assert_eq!(l.lire_bytes("a.txt").unwrap(), data);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn normaliser_nom_entree_rejette_zip_slip() {
        assert!(normaliser_nom_entree("medias/a.png").is_some());
        assert_eq!(normaliser_nom_entree("a\\b.txt").as_deref(), Some("a/b.txt"));
        assert!(normaliser_nom_entree("../evil").is_none());
        assert!(normaliser_nom_entree("a/../b").is_none());
        assert!(normaliser_nom_entree("/etc/passwd").is_none());
        assert!(normaliser_nom_entree("C:/win").is_none());
        assert!(normaliser_nom_entree("").is_none());
        assert!(normaliser_nom_entree("a//b").is_none());
    }
}
