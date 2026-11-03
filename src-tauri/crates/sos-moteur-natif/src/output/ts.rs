//! Muxeur **MPEG-TS** minimal — un seul flux vidéo, **PTS par frame**.
//!
//! Pourquoi : un flux H.264/HEVC élémentaire (Annex B) ne transporte aucun
//! horodatage. Le seul moyen de donner à FFmpeg les **vrais** PTS produits
//! par le moteur (`PaquetEncode.pts_us`, horloge monotone) sans écrire un
//! muxeur MP4 complet est de les envelopper dans un conteneur qui les porte :
//! MPEG-TS met le PTS dans l'en-tête PES, et FFmpeg le démuxe nativement
//! (`-f mpegts -i pipe:0 -c copy`).
//!
//! ⚠️ **Ne pas revenir à `-use_wallclock_as_timestamps`** : ce mode
//! estampille chaque paquet à son **arrivée** dans le démultiplexeur, donc
//! quantifiée par les rafales de lecture du pipe et par la file d'écriture —
//! une session réelle (46,79 fps mesurés, 824 ticks en retard) produisait une
//! image qui se fige puis saute.
//!
//! Limites assumées :
//! - **Un seul flux** (vidéo) : l'audio est muxé à l'arrêt (`muxer_audio`).
//! - **AV1 refusé** (`Codec::Av1` → `None`) : l'AV1 en TS exige
//!   `stream_type 0x06` + descriptor de registration, non vérifié ici — le
//!   sink garde alors le chemin brut historique.
//! - PTS = DTS : l'encodeur ne produit pas de B-frames (§11.3).
//! - Débordement du compteur 33 bits (~26,5 h) : modulo explicite.

use sos_moteur_api::Codec;

use crate::encode::PaquetEncode;

/// Taille d'un paquet de transport MPEG-TS.
const TS: usize = 188;
/// PID du flux vidéo (0x100, convention ffmpeg/OBS).
const PID_VIDEO: u16 = 0x0100;
/// PID de la table PMT (hors plage des PID réservés).
const PID_PMT: u16 = 0x0FFF;
/// PID de la table PAT (fixé par la norme).
const PID_PAT: u16 = 0x0000;
/// Premier PTS : 1 s à 90 kHz. Un PTS initial à 0 fait dériver des DTS
/// négatifs dans le muxeur MP4 (il recale, mais l'amorce est sale).
const DECALAGE_PTS: u64 = 90_000;
/// Réémission PAT/PMT : ~1,25 s à 48 fps. Un décodeur qui se branche en cours
/// de flux doit retrouver les tables sans attendre la fin.
const PERIODE_PSI: u32 = 60;

/// Enveloppe des paquets encodés en paquets TS de 188 octets.
pub struct MuxeurTs {
    codec: Codec,
    cc_pat: u8,
    cc_pmt: u8,
    cc_video: u8,
    /// Nombre de paquets vidéo enveloppés — cadence les PAT/PMT.
    paquets: u32,
    /// Dernier PTS émis (90 kHz, **avant** modulo) : garantit la monotonie.
    dernier_pts: Option<u64>,
    /// Nombre de PTS forcés pour rester strictement croissants. Cas
    /// impossible en pratique (horloge monotone) : compté, jamais silencieux.
    pts_corriges: u32,
}

impl MuxeurTs {
    /// `None` = codec non supporté en TS (AV1) : l'appelant garde le chemin
    /// brut historique.
    pub fn nouveau(codec: Codec) -> Option<Self> {
        match codec {
            Codec::H264 | Codec::Hevc => Some(Self {
                codec,
                cc_pat: 0,
                cc_pmt: 0,
                cc_video: 0,
                paquets: 0,
                dernier_pts: None,
                pts_corriges: 0,
            }),
            Codec::Av1 => None,
        }
    }

    /// PTS forcés pour rester strictement croissants (diagnostic / tests).
    pub fn pts_corriges(&self) -> u32 {
        self.pts_corriges
    }

    /// `stream_type` du PMT (ISO/IEC 13818-1).
    fn stream_type(&self) -> u8 {
        match self.codec {
            Codec::H264 => 0x1B,
            Codec::Hevc => 0x24,
            // Jamais atteint : `nouveau` refuse AV1.
            Codec::Av1 => 0x06,
        }
    }

    /// Enveloppe un paquet encodé : PAT/PMT si dus, puis un PES avec son PTS.
    pub fn envelopper(&mut self, paquet: &PaquetEncode) -> Vec<u8> {
        let mut sortie = Vec::with_capacity(TS * 8);
        if self.paquets % PERIODE_PSI == 0 {
            self.ecrire_pat(&mut sortie);
            self.ecrire_pmt(&mut sortie);
        }
        let pts = self.pts(paquet.pts_us);
        let pes = pes_video(&paquet.data, pts);
        let mut reste: &[u8] = &pes;
        let mut premier = true;
        while !reste.is_empty() {
            // Le premier paquet porte le PCR : 8 octets d'en-tête
            // d'adaptation en moins dans la charge utile.
            let capacite = if premier { TS - 4 - 8 } else { TS - 4 };
            let n = reste.len().min(capacite);
            let (bloc, suite) = reste.split_at(n);
            ecrire_paquet(
                &mut sortie,
                PID_VIDEO,
                &mut self.cc_video,
                premier,
                premier.then_some(pts),
                bloc,
            );
            reste = suite;
            premier = false;
        }
        self.paquets = self.paquets.wrapping_add(1);
        sortie
    }

    /// µs → 90 kHz, décalé, strictement croissant, modulo 2³³.
    fn pts(&mut self, pts_us: i64) -> u64 {
        // Arrondi (et non troncature) : l'erreur reste sous un demi-tick
        // (5,5 µs) par frame, sans accumulation — chaque PTS est calculé
        // depuis un temps absolu.
        let brut = DECALAGE_PTS + (pts_us.max(0) as u64 * 9 + 50) / 100;
        let pts = match self.dernier_pts {
            // Deux frames au même PTS feraient jeter la seconde par le muxeur
            // MP4 : on force un tick d'écart.
            Some(d) if brut <= d => {
                self.pts_corriges = self.pts_corriges.saturating_add(1);
                d + 1
            }
            _ => brut,
        };
        self.dernier_pts = Some(pts);
        pts % (1 << 33)
    }

    fn ecrire_pat(&mut self, sortie: &mut Vec<u8>) {
        let mut section = vec![
            0x00, // table_id
            0xB0, 0x0D, // syntax + section_length = 13
            0x00, 0x01, // transport_stream_id
            0xC1, // version 0, current_next 1
            0x00, 0x00, // section_number, last_section_number
            0x00, 0x01, // program_number
            0xE0 | (PID_PMT >> 8) as u8,
            (PID_PMT & 0xFF) as u8,
        ];
        let crc = crc32_mpeg(&section);
        section.extend_from_slice(&crc.to_be_bytes());
        ecrire_section(sortie, PID_PAT, &mut self.cc_pat, &section);
    }

    fn ecrire_pmt(&mut self, sortie: &mut Vec<u8>) {
        let mut section = vec![
            0x02, // table_id
            0xB0, 0x12, // syntax + section_length = 18
            0x00, 0x01, // program_number
            0xC1,
            0x00, 0x00,
            0xE0 | (PID_VIDEO >> 8) as u8, // PCR_PID
            (PID_VIDEO & 0xFF) as u8,
            0xF0, 0x00, // program_info_length = 0
            self.stream_type(),
            0xE0 | (PID_VIDEO >> 8) as u8,
            (PID_VIDEO & 0xFF) as u8,
            0xF0, 0x00, // ES_info_length = 0
        ];
        let crc = crc32_mpeg(&section);
        section.extend_from_slice(&crc.to_be_bytes());
        ecrire_section(sortie, PID_PMT, &mut self.cc_pmt, &section);
    }
}

/// Une section PSI dans un paquet TS (`pointer_field` à 0, bourrage compris).
fn ecrire_section(sortie: &mut Vec<u8>, pid: u16, cc: &mut u8, section: &[u8]) {
    let mut charge = Vec::with_capacity(1 + section.len());
    charge.push(0x00);
    charge.extend_from_slice(section);
    ecrire_paquet(sortie, pid, cc, true, None, &charge);
}

/// Écrit un paquet TS de 188 octets (en-tête + adaptation éventuelle + charge).
fn ecrire_paquet(
    sortie: &mut Vec<u8>,
    pid: u16,
    cc: &mut u8,
    debut: bool,
    pcr: Option<u64>,
    charge: &[u8],
) {
    debug_assert!(charge.len() <= TS - 4, "charge {} o", charge.len());
    let mut paquet = Vec::with_capacity(TS);
    paquet.push(0x47);
    let pusi = if debut { 0x40 } else { 0x00 };
    paquet.push(pusi | ((pid >> 8) as u8 & 0x1F));
    paquet.push((pid & 0xFF) as u8);

    let libre = TS - 4 - charge.len();
    // Champ d'adaptation : nécessaire pour le PCR, et pour le bourrage dès
    // que la charge utile ne remplit pas le paquet — sinon le décodeur lit
    // les octets suivants comme du payload.
    let af = pcr.is_some() || libre > 0;
    let afc: u8 = if af { 0x30 } else { 0x10 };
    paquet.push(afc | (*cc & 0x0F));
    *cc = (*cc + 1) & 0x0F;

    if af {
        let longueur = libre.saturating_sub(1);
        let mut corps = Vec::new();
        if longueur > 0 {
            match pcr {
                Some(p) => {
                    debug_assert!(longueur >= 7, "PCR dans {longueur} octet(s)");
                    corps.push(0x10); // PCR_flag
                    corps.extend_from_slice(&pcr_octets(p));
                }
                None => corps.push(0x00),
            }
            while corps.len() < longueur {
                corps.push(0xFF);
            }
        }
        paquet.push(longueur as u8);
        paquet.extend_from_slice(&corps);
    }
    paquet.extend_from_slice(charge);
    debug_assert_eq!(paquet.len(), TS);
    sortie.extend_from_slice(&paquet);
}

/// PES vidéo (stream_id 0xE0) avec PTS seul — pas de B-frames, donc PTS = DTS.
fn pes_video(data: &[u8], pts: u64) -> Vec<u8> {
    let mut pes = Vec::with_capacity(14 + data.len());
    pes.extend_from_slice(&[0x00, 0x00, 0x01, 0xE0]);
    // Longueur 0 = non spécifiée, autorisé pour la vidéo en TS.
    pes.extend_from_slice(&[0x00, 0x00]);
    pes.extend_from_slice(&[0x80, 0x80, 0x05]);
    pes.extend_from_slice(&pts_octets(pts));
    pes.extend_from_slice(data);
    pes
}

/// PTS sur 5 octets (préfixe `0010`, marqueurs à 1).
fn pts_octets(pts: u64) -> [u8; 5] {
    [
        (0x20 | ((pts >> 30) & 0x07) << 1) as u8 | 0x01,
        ((pts >> 22) & 0xFF) as u8,
        ((((pts >> 15) & 0x7F) << 1) as u8) | 0x01,
        ((pts >> 7) & 0xFF) as u8,
        (((pts & 0x7F) << 1) as u8) | 0x01,
    ]
}

/// PCR sur 6 octets (base 33 bits à 90 kHz, extension nulle).
fn pcr_octets(base: u64) -> [u8; 6] {
    let base = base & ((1 << 33) - 1);
    [
        (base >> 25) as u8,
        (base >> 17) as u8,
        (base >> 9) as u8,
        (base >> 1) as u8,
        (((base & 1) as u8) << 7) | 0x7E,
        0x00,
    ]
}

/// CRC32 MPEG-2 (poly 0x04C11DB7, init 0xFFFFFFFF, sans réflexion ni XOR
/// final) — obligatoire pour les sections PAT/PMT.
fn crc32_mpeg(donnees: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for octet in donnees {
        crc ^= u32::from(*octet) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paquet(pts_us: i64, taille: usize) -> PaquetEncode {
        PaquetEncode {
            data: (0..taille).map(|i| i as u8).collect(),
            pts_us,
            cle: true,
        }
    }

    fn pid(p: &[u8]) -> u16 {
        (u16::from(p[1] & 0x1F) << 8) | u16::from(p[2])
    }

    fn debut(p: &[u8]) -> bool {
        p[1] & 0x40 != 0
    }

    /// Charge utile d'un paquet TS (après en-tête et champ d'adaptation).
    fn charge(p: &[u8]) -> &[u8] {
        let afc = (p[3] >> 4) & 0x03;
        match afc {
            0b11 => {
                let longueur = p[4] as usize;
                &p[5 + longueur..]
            }
            0b01 => &p[4..],
            _ => &[],
        }
    }

    /// Tous les paquets de 188 octets, tous synchronisés.
    fn decouper(ts: &[u8]) -> Vec<&[u8]> {
        assert_eq!(ts.len() % TS, 0, "longueur {}", ts.len());
        let paquets: Vec<&[u8]> = ts.chunks(TS).collect();
        for p in &paquets {
            assert_eq!(p[0], 0x47, "octet de synchronisation");
        }
        paquets
    }

    #[test]
    fn pat_et_pmt_en_tete_puis_video() {
        let mut muxeur = MuxeurTs::nouveau(Codec::H264).expect("h264");
        let ts = muxeur.envelopper(&paquet(0, 1_000));
        let paquets = decouper(&ts);
        assert_eq!(pid(paquets[0]), PID_PAT);
        assert_eq!(pid(paquets[1]), PID_PMT);
        assert!(paquets[2..].iter().all(|p| pid(p) == PID_VIDEO));
        assert!(debut(paquets[2]), "le premier paquet vidéo ouvre le PES");
    }

    #[test]
    fn pat_et_pmt_reeemis_periodiquement() {
        let mut muxeur = MuxeurTs::nouveau(Codec::H264).expect("h264");
        let mut ts = Vec::new();
        for i in 0..PERIODE_PSI {
            ts.extend(muxeur.envelopper(&paquet(i as i64 * 20_000, 300)));
        }
        let paquets = decouper(&ts);
        let pats = paquets.iter().filter(|p| pid(p) == PID_PAT).count();
        assert_eq!(pats, 1, "une seule PAT sur une période");
        ts.extend(muxeur.envelopper(&paquet(PERIODE_PSI as i64 * 20_000, 300)));
        let paquets = decouper(&ts);
        assert_eq!(paquets.iter().filter(|p| pid(p) == PID_PAT).count(), 2);
    }

    #[test]
    fn charge_utile_restituee_a_l_identique() {
        let mut muxeur = MuxeurTs::nouveau(Codec::Hevc).expect("hevc");
        let source = paquet(1_000_000, 5_000);
        let ts = muxeur.envelopper(&source);
        let pes: Vec<u8> = decouper(&ts)
            .iter()
            .filter(|p| pid(p) == PID_VIDEO)
            .flat_map(|p| charge(p).iter().copied())
            .collect();
        // En-tête PES : 4 (start code + stream_id) + 2 (longueur) + 3 + 5 (PTS).
        assert_eq!(&pes[..4], &[0x00, 0x00, 0x01, 0xE0]);
        assert_eq!(&pes[14..], &source.data[..]);
    }

    #[test]
    fn pts_encode_est_relu() {
        let mut muxeur = MuxeurTs::nouveau(Codec::H264).expect("h264");
        let pts_us = 2_000_000; // 2 s
        let ts = muxeur.envelopper(&paquet(pts_us, 100));
        let pes: Vec<u8> = decouper(&ts)
            .iter()
            .filter(|p| pid(p) == PID_VIDEO)
            .flat_map(|p| charge(p).iter().copied())
            .collect();
        let p = &pes[9..14];
        let pts = (u64::from(p[0] & 0x0E) << 29)
            | (u64::from(p[1]) << 22)
            | (u64::from(p[2] & 0xFE) << 14)
            | (u64::from(p[3]) << 7)
            | (u64::from(p[4]) >> 1);
        assert_eq!(pts, DECALAGE_PTS + 2_000_000 * 9 / 100);
    }

    #[test]
    fn pts_reste_strictement_croissant() {
        let mut muxeur = MuxeurTs::nouveau(Codec::H264).expect("h264");
        let _ = muxeur.envelopper(&paquet(1_000_000, 100));
        // Même instant : le second doit être décalé d'un tick, pas dupliqué.
        let _ = muxeur.envelopper(&paquet(1_000_000, 100));
        assert_eq!(muxeur.pts_corriges(), 1);
    }

    #[test]
    fn av1_refuse() {
        assert!(MuxeurTs::nouveau(Codec::Av1).is_none());
    }

    #[test]
    fn crc_mpeg_vecteur_de_controle() {
        // Valeur de contrôle officielle de CRC-32/MPEG-2 : « 123456789 »
        // doit donner 0x0376E6E7. C'est ce CRC que les décodeurs vérifient
        // sur les sections PAT/PMT — un mauvais polynôme rend le flux
        // illisible sans autre message.
        assert_eq!(crc32_mpeg(b"123456789"), 0x0376_E6E7);
    }
}
