//! Détection des encodeurs Media Foundation disponibles.
//!
//! Politique (plan §6.2) : on énumère d'abord les MFT **matériels**
//! (`MFT_ENUM_FLAG_HARDWARE`), puis, seulement s'il n'y en a aucun, les MFT
//! logiciels. Le diagnostic affiche le nom retenu et le caractère
//! matériel/logiciel — aucune valeur inventée.

use std::sync::Once;

use windows::Win32::Media::MediaFoundation::{
    eAVEncH264VProfile_High, IMFActivate, MFMediaType_Video, MFTEnumEx,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_FRIENDLY_NAME_Attribute, MFT_REGISTER_TYPE_INFO,
    MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_NV12, MFStartup,
    MF_VERSION,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::GUID;

use super::{Codec, ErreurEncodage};

static MF_INIT: Once = Once::new();

/// Démarre Media Foundation une seule fois par processus.
pub fn initialiser() {
    MF_INIT.call_once(|| {
        // L'échec est constaté plus tard, à la première opération MFT, avec
        // un message explicite — jamais de panic ici.
        let _ = unsafe { MFStartup(MF_VERSION, 0) };
    });
}

/// Un encodeur disponible sur cette machine.
pub struct EncodeurDispo {
    pub nom: String,
    pub materiel: bool,
    pub codec: Codec,
    /// Handle d'activation conservé pour créer l'encodeur.
    pub(crate) activate: IMFActivate,
}

impl EncodeurDispo {
    /// Handle d'activation Media Foundation (usage moteur).
    pub fn activate(&self) -> &IMFActivate {
        &self.activate
    }
}

impl std::fmt::Debug for EncodeurDispo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncodeurDispo")
            .field("nom", &self.nom)
            .field("materiel", &self.materiel)
            .field("codec", &self.codec)
            .finish_non_exhaustive()
    }
}

/// Sous-type Media Foundation d'un codec.
pub(crate) fn subtype(codec: Codec) -> GUID {
    match codec {
        Codec::H264 => MFVideoFormat_H264,
        Codec::Hevc => MFVideoFormat_HEVC,
        Codec::Av1 => MFVideoFormat_AV1,
    }
}

/// Profil H.264 visé (High = compatible streaming).
pub(crate) fn profil_h264() -> u32 {
    eAVEncH264VProfile_High.0 as u32
}

/// Liste les encodeurs disponibles pour un codec.
///
/// Matériels d'abord ; si la liste est vide, on retombe sur les logiciels.
pub fn lister(codec: Codec) -> Result<Vec<EncodeurDispo>, ErreurEncodage> {
    initialiser();

    let mut liste = enumerer(codec, true)?;
    if liste.is_empty() {
        liste = enumerer(codec, false)?;
    }
    Ok(liste)
}

/// Liste **tous** les encodeurs (matériels puis logiciels), sans doublon de
/// nom — utile au spike et au diagnostic (« matériel / logiciel »).
pub fn lister_tous(codec: Codec) -> Result<Vec<EncodeurDispo>, ErreurEncodage> {
    initialiser();

    let mut liste = enumerer(codec, true)?;
    liste.extend(enumerer(codec, false)?);
    Ok(dedupliquer(liste))
}

/// Supprime les doublons de nom (les MFT QSV apparaissent souvent 2×).
fn dedupliquer(liste: Vec<EncodeurDispo>) -> Vec<EncodeurDispo> {
    let mut out: Vec<EncodeurDispo> = Vec::new();
    for e in liste {
        if !out.iter().any(|x| x.nom == e.nom) {
            out.push(e);
        }
    }
    out
}

fn enumerer(codec: Codec, materiel: bool) -> Result<Vec<EncodeurDispo>, ErreurEncodage> {
    let entree = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let sortie = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype(codec),
    };

    let mut flags = MFT_ENUM_FLAG_SORTANDFILTER.0;
    if materiel {
        flags |= MFT_ENUM_FLAG_HARDWARE.0;
    }

    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut nombre = 0u32;

    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG(flags),
            Some(&entree),
            Some(&sortie),
            &mut activates,
            &mut nombre,
        )
    }
    .map_err(|e| ErreurEncodage::Mft(format!("MFTEnumEx : {}", e.message())))?;

    let mut resultat = Vec::with_capacity(nombre as usize);
    if !activates.is_null() && nombre > 0 {
        let slice = unsafe { std::slice::from_raw_parts(activates, nombre as usize) };
        for activate in slice.iter().flatten() {
            resultat.push(EncodeurDispo {
                nom: nom_convivial(activate),
                materiel,
                codec,
                activate: activate.clone(),
            });
        }
        unsafe { CoTaskMemFree(Some(activates as *const _)) };
    }

    Ok(resultat)
}

fn nom_convivial(activate: &IMFActivate) -> String {
    let mut buf = [0u16; 256];
    let mut len = 0u32;
    let ok = unsafe {
        activate.GetString(&MFT_FRIENDLY_NAME_Attribute, &mut buf, Some(&mut len))
    }
    .is_ok();

    if !ok || len == 0 {
        return "(sans nom)".to_string();
    }
    let len = (len as usize).min(buf.len());
    String::from_utf16_lossy(&buf[..len])
        .trim_end_matches('\0')
        .to_string()
}
