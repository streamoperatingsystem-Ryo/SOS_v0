//! Détection du matériel graphique.
//!
//! Première brique du moteur : savoir **sur quel adaptateur** on va capturer,
//! composer et encoder. C'est aussi la source de vérité du diagnostic
//! « Moteur » (pilote graphique, VRAM, LUID) — cf.
//! `docs/features/moteur-natif.md` §16.
//!
//! Aucune valeur inventée : un adaptateur non énumérable produit une erreur,
//! jamais une entrée vide.

use std::fmt;

/// Un adaptateur DXGI tel que le voit le moteur.
///
/// `luid` est l'identité stable de la carte (elle ne change pas quand les
/// écrans sont branchés/débranchés) : c'est la clé d'agrégation des compteurs
/// PDH `pid_<PID>_luid_<LUID>_…` et de la persistance `moteur.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adaptateur {
    /// Index d'énumération DXGI (0 = premier adaptateur).
    pub index: u32,
    /// `VendorId` DXGI (`0x10DE` NVIDIA, `0x1002`/`0x1022` AMD, `0x8086` Intel…).
    pub vendor_id: u32,
    /// Nom lisible du constructeur.
    pub vendor_nom: &'static str,
    /// Nom commercial de la carte (ex. « NVIDIA GeForce RTX 4070 »).
    pub description: String,
    /// Mémoire vidéo dédiée, en Mio.
    pub vram_mio: u64,
    /// LUID 64 bits (`HighPart << 32 | LowPart`).
    pub luid: u64,
    /// `true` si DXGI signale un adaptateur logiciel (WARP, rendu logiciel).
    pub logiciel: bool,
}

impl Adaptateur {
    /// Un adaptateur logiciel ne peut pas encoder en matériel : le moteur
    /// doit le savoir avant de le choisir.
    pub fn materiel(&self) -> bool {
        !self.logiciel
    }
}

/// Erreurs de la couche GPU. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurGpu {
    /// DXGI a refusé l'opération (message natif conservé tel quel).
    Dxgi(String),
    /// Plateforme non supportée (le moteur est Windows-only en v1).
    NonSupporte,
}

impl fmt::Display for ErreurGpu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurGpu::Dxgi(m) => write!(f, "DXGI : {m}"),
            ErreurGpu::NonSupporte => write!(f, "plateforme non supportée"),
        }
    }
}

impl std::error::Error for ErreurGpu {}

/// Nom lisible d'un `VendorId` DXGI. Ne devine jamais : inconnu = « Inconnu ».
pub fn vendor_nom(vendor_id: u32) -> &'static str {
    match vendor_id {
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        0x1414 => "Microsoft",
        0x13B5 => "ARM",
        0x5143 => "Qualcomm",
        _ => "Inconnu",
    }
}

/// Énumère les adaptateurs DXGI, dans l'ordre du système.
///
/// `DXGI_ERROR_NOT_FOUND` termine la liste (cas normal) ; toute autre erreur
/// sur le premier adaptateur remonte en [`ErreurGpu::Dxgi`].
#[cfg(windows)]
pub fn enumerer_adaptateurs() -> Result<Vec<Adaptateur>, ErreurGpu> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|e| ErreurGpu::Dxgi(e.message()))?;

    let mut adaptateurs = Vec::new();
    let mut index = 0u32;

    loop {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(a) => a,
            // Fin de liste : c'est le cas normal, pas une erreur.
            Err(_) => break,
        };

        let desc = unsafe { adapter.GetDesc1() }.map_err(|e| ErreurGpu::Dxgi(e.message()))?;

        adaptateurs.push(Adaptateur {
            index,
            vendor_id: desc.VendorId,
            vendor_nom: vendor_nom(desc.VendorId),
            description: utf16z(&desc.Description),
            vram_mio: (desc.DedicatedVideoMemory as u64) / (1024 * 1024),
            luid: luid_vers_u64(desc.AdapterLuid),
            logiciel: desc.Flags & windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32
                != 0,
        });

        index += 1;
    }

    Ok(adaptateurs)
}

/// Stub hors Windows : le moteur natif est Windows-only en v1.
#[cfg(not(windows))]
pub fn enumerer_adaptateurs() -> Result<Vec<Adaptateur>, ErreurGpu> {
    Err(ErreurGpu::NonSupporte)
}

/// Convertit une chaîne UTF-16 terminée par zéro (description DXGI).
#[cfg(windows)]
fn utf16z(desc: &[u16; 128]) -> String {
    let len = desc.iter().position(|&c| c == 0).unwrap_or(desc.len());
    String::from_utf16_lossy(&desc[..len])
}

/// Recompose un LUID 64 bits à partir de ses deux moitiés Win32.
#[cfg(windows)]
fn luid_vers_u64(luid: windows::Win32::Foundation::LUID) -> u64 {
    ((luid.HighPart as u32 as u64) << 32) | luid.LowPart as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendors_connus() {
        assert_eq!(vendor_nom(0x10DE), "NVIDIA");
        assert_eq!(vendor_nom(0x1002), "AMD");
        assert_eq!(vendor_nom(0x8086), "Intel");
        assert_eq!(vendor_nom(0x1414), "Microsoft");
    }

    #[test]
    fn vendor_inconnu_ne_devine_pas() {
        assert_eq!(vendor_nom(0xDEAD), "Inconnu");
    }

    #[test]
    fn adaptateur_logiciel_n_est_pas_materiel() {
        let a = Adaptateur {
            index: 0,
            vendor_id: 0x1414,
            vendor_nom: "Microsoft",
            description: "Microsoft Basic Render Driver".into(),
            vram_mio: 0,
            luid: 0,
            logiciel: true,
        };
        assert!(!a.materiel());
    }
}
