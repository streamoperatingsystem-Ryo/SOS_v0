//! Médias statiques du moteur — décodage WIC (PNG/JPG/BMP) → texture BGRA.
//!
//! Tranche P3 n°1 : fond de scène + widgets média, sans animation ni texte.
//! Les vidéos (Media Foundation Source Reader) et le rendu de texte arrivent
//! dans les tranches suivantes — ne pas élargir ce module sans plan.

use std::ffi::c_void;
use std::fmt;
use std::path::{Path, PathBuf};

use windows::core::HSTRING;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Imaging::{
    IWICImagingFactory, CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::gpu::ContexteGpu;

/// Erreurs de média. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurMedia {
    Fichier(String),
    Decodage(String),
    Gpu(String),
}

impl fmt::Display for ErreurMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurMedia::Fichier(m) => write!(f, "fichier : {m}"),
            ErreurMedia::Decodage(m) => write!(f, "décodage : {m}"),
            ErreurMedia::Gpu(m) => write!(f, "GPU : {m}"),
        }
    }
}

impl std::error::Error for ErreurMedia {}

/// Un média statique à composer : chemin résolu + destination + ordre z.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaCfg {
    pub chemin: PathBuf,
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
}

/// Média décodé, prêt pour le compositeur.
pub struct MediaChargee {
    pub texture: ID3D11Texture2D,
    pub taille: (u32, u32),
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
}

/// Décode une image en texture BGRA (usage `DEFAULT`, bind render target,
/// compatible vue d'entrée du video processor).
pub fn charger(contexte: &ContexteGpu, cfg: &MediaCfg) -> Result<MediaChargee, ErreurMedia> {
    if !cfg.chemin.is_file() {
        return Err(ErreurMedia::Fichier(format!(
            "introuvable : {}",
            cfg.chemin.display()
        )));
    }
    let (largeur, hauteur, pixels) = decoder_bgra(&cfg.chemin)?;
    if largeur == 0 || hauteur == 0 {
        return Err(ErreurMedia::Decodage("image vide".to_string()));
    }
    let texture = televerser(contexte, largeur, hauteur, &pixels)?;
    Ok(MediaChargee {
        texture,
        taille: (largeur, hauteur),
        rect: cfg.rect,
        z: cfg.z,
    })
}

/// Décode un fichier image en BGRA8 linéaire (ligne par ligne, sans padding).
fn decoder_bgra(chemin: &Path) -> Result<(u32, u32, Vec<u8>), ErreurMedia> {
    // WIC exige COM sur le fil appelant. Le fil moteur est à nous : MTA.
    // S_FALSE (déjà initialisé) et RPC_E_CHANGED_MODE (déjà STA) sont
    // tolérés — WIC fonctionne dans les deux cas.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| ErreurMedia::Decodage(format!("WIC factory : {}", e.message())))?;
        let nom = HSTRING::from(chemin.as_os_str());
        let decodeur = factory
            .CreateDecoderFromFilename(&nom, None, GENERIC_READ, WICDecodeMetadataCacheOnLoad)
            .map_err(|e| {
                ErreurMedia::Decodage(format!("ouverture {} : {}", chemin.display(), e.message()))
            })?;
        let cadre = decodeur
            .GetFrame(0)
            .map_err(|e| ErreurMedia::Decodage(format!("frame 0 : {}", e.message())))?;
        let convertisseur = factory
            .CreateFormatConverter()
            .map_err(|e| ErreurMedia::Decodage(format!("convertisseur : {}", e.message())))?;
        convertisseur
            .Initialize(
                &cadre,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| ErreurMedia::Decodage(format!("conversion BGRA : {}", e.message())))?;

        let mut largeur = 0u32;
        let mut hauteur = 0u32;
        convertisseur
            .GetSize(&mut largeur, &mut hauteur)
            .map_err(|e| ErreurMedia::Decodage(format!("taille : {}", e.message())))?;
        let pas = largeur
            .checked_mul(4)
            .ok_or_else(|| ErreurMedia::Decodage("largeur débordante".to_string()))?;
        let taille = (pas as usize)
            .checked_mul(hauteur as usize)
            .ok_or_else(|| ErreurMedia::Decodage("image trop grande".to_string()))?;
        let mut pixels = vec![0u8; taille];
        convertisseur
            .CopyPixels(std::ptr::null(), pas, &mut pixels)
            .map_err(|e| ErreurMedia::Decodage(format!("pixels : {}", e.message())))?;
        Ok((largeur, hauteur, pixels))
    }
}

/// Crée la texture et téléverse les pixels (une seule fois).
fn televerser(
    contexte: &ContexteGpu,
    largeur: u32,
    hauteur: u32,
    pixels: &[u8],
) -> Result<ID3D11Texture2D, ErreurMedia> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: largeur,
        Height: hauteur,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let donnees = D3D11_SUBRESOURCE_DATA {
        pSysMem: pixels.as_ptr() as *const c_void,
        SysMemPitch: largeur * 4,
        SysMemSlicePitch: 0,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    unsafe { contexte.device().CreateTexture2D(&desc, Some(&donnees), Some(&mut texture)) }
        .map_err(|e| ErreurMedia::Gpu(format!("CreateTexture2D : {}", e.message())))?;
    texture.ok_or_else(|| ErreurMedia::Gpu("texture nulle".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichier_absent_refuse_sans_gpu() {
        let cfg = MediaCfg {
            chemin: PathBuf::from("C:/inexistant/sos-test.png"),
            rect: (0.0, 0.0, 10.0, 10.0),
            z: 0,
        };
        // Pas de contexte GPU nécessaire : l'erreur vient du fichier.
        let contexte = match ContexteGpu::creer(None) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert!(matches!(charger(&contexte, &cfg), Err(ErreurMedia::Fichier(_))));
    }

    #[test]
    fn erreur_affichee() {
        let e = ErreurMedia::Decodage("test".to_string());
        assert!(e.to_string().contains("test"));
    }
}
