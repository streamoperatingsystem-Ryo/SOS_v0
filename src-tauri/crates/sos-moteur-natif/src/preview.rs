//! Aperçu basse résolution — canvas NV12 → BGRA réduit, lu par le dashboard.
//!
//! Chemin : video processor (NV12 → BGRA, échelle) → texture staging → Map.
//! 10 fps suffisent (plan §2.7) ; ~0,5 Mo par frame à 480×270.

use std::fmt;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{BOOL, RECT};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DeviceContext, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice,
    ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_RATIONAL, DXGI_SAMPLE_DESC};
use windows::core::Interface;

use crate::gpu::{ContexteGpu, DescTexture, Format};

/// Erreurs d'aperçu. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurApercu {
    Gpu(String),
}

impl fmt::Display for ErreurApercu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurApercu::Gpu(m) => write!(f, "GPU : {m}"),
        }
    }
}

impl std::error::Error for ErreurApercu {}

/// Aperçu réduit d'une source NV12.
pub struct Apercu {
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    enumerateur: ID3D11VideoProcessorEnumerator,
    processeur: ID3D11VideoProcessor,
    context: ID3D11DeviceContext,
    sortie: ID3D11Texture2D,
    staging: ID3D11Texture2D,
    vue_sortie: ID3D11VideoProcessorOutputView,
    source: (u32, u32),
    largeur: u32,
    hauteur: u32,
    tampon: Vec<u8>,
}

impl Apercu {
    /// `source` = taille du canvas NV12, `apercu` = taille réduite (BGRA).
    pub fn nouveau(
        contexte: &ContexteGpu,
        source: (u32, u32),
        apercu: (u32, u32),
    ) -> Result<Self, ErreurApercu> {
        let desc_source = DescTexture::new(Format::Nv12, source.0, source.1)
            .map_err(|e| ErreurApercu::Gpu(e.to_string()))?;
        let desc_apercu = DescTexture::new(Format::Bgra8, apercu.0, apercu.1)
            .map_err(|e| ErreurApercu::Gpu(e.to_string()))?;

        let video_device: ID3D11VideoDevice = contexte
            .device()
            .cast()
            .map_err(|e| ErreurApercu::Gpu(format!("ID3D11VideoDevice : {}", e.message())))?;
        let video_context: ID3D11VideoContext = contexte
            .context()
            .cast()
            .map_err(|e| ErreurApercu::Gpu(format!("ID3D11VideoContext : {}", e.message())))?;

        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            InputWidth: desc_source.largeur,
            InputHeight: desc_source.hauteur,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: 10,
                Denominator: 1,
            },
            OutputWidth: desc_apercu.largeur,
            OutputHeight: desc_apercu.hauteur,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };

        let enumerateur = unsafe { video_device.CreateVideoProcessorEnumerator(&content) }
            .map_err(|e| {
                ErreurApercu::Gpu(format!("CreateVideoProcessorEnumerator : {}", e.message()))
            })?;
        let processeur = unsafe { video_device.CreateVideoProcessor(&enumerateur, 0) }
            .map_err(|e| ErreurApercu::Gpu(format!("CreateVideoProcessor : {}", e.message())))?;

        let sortie = contexte
            .creer_texture(&desc_apercu)
            .map_err(|e| ErreurApercu::Gpu(e.to_string()))?;

        let desc_sortie = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut vue_sortie: Option<ID3D11VideoProcessorOutputView> = None;
        unsafe {
            video_device.CreateVideoProcessorOutputView(
                &sortie,
                &enumerateur,
                &desc_sortie,
                Some(&mut vue_sortie),
            )
        }
        .map_err(|e| {
            ErreurApercu::Gpu(format!(
                "CreateVideoProcessorOutputView : {}",
                e.message()
            ))
        })?;
        let vue_sortie =
            vue_sortie.ok_or_else(|| ErreurApercu::Gpu("vue de sortie nulle".into()))?;

        let desc_staging = D3D11_TEXTURE2D_DESC {
            Width: desc_apercu.largeur,
            Height: desc_apercu.hauteur,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe {
            contexte
                .device()
                .CreateTexture2D(&desc_staging, None, Some(&mut staging))
        }
        .map_err(|e| ErreurApercu::Gpu(format!("CreateTexture2D staging : {}", e.message())))?;
        let staging = staging.ok_or_else(|| ErreurApercu::Gpu("texture staging nulle".into()))?;

        let tampon = vec![0u8; (desc_apercu.largeur * desc_apercu.hauteur * 4) as usize];

        Ok(Self {
            video_device,
            video_context,
            enumerateur,
            processeur,
            context: contexte.context().clone(),
            sortie,
            staging,
            vue_sortie,
            source: (desc_source.largeur, desc_source.hauteur),
            largeur: desc_apercu.largeur,
            hauteur: desc_apercu.hauteur,
            tampon,
        })
    }

    /// Convertit la source NV12 en BGRA réduit et rend les pixels (BGRA serré).
    pub fn lire(&mut self, source: &ID3D11Texture2D) -> Result<&[u8], ErreurApercu> {
        let desc_entree = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut vue_entree: Option<ID3D11VideoProcessorInputView> = None;
        unsafe {
            self.video_device.CreateVideoProcessorInputView(
                source,
                &self.enumerateur,
                &desc_entree,
                Some(&mut vue_entree),
            )
        }
        .map_err(|e| {
            ErreurApercu::Gpu(format!(
                "CreateVideoProcessorInputView : {}",
                e.message()
            ))
        })?;
        let vue_entree =
            vue_entree.ok_or_else(|| ErreurApercu::Gpu("vue d'entrée nulle".into()))?;

        let rect_src = RECT {
            left: 0,
            top: 0,
            right: self.source.0 as i32,
            bottom: self.source.1 as i32,
        };
        let rect_dst = RECT {
            left: 0,
            top: 0,
            right: self.largeur as i32,
            bottom: self.hauteur as i32,
        };
        unsafe {
            self.video_context.VideoProcessorSetStreamSourceRect(
                &self.processeur,
                0,
                true,
                Some(&rect_src),
            );
            self.video_context.VideoProcessorSetStreamDestRect(
                &self.processeur,
                0,
                true,
                Some(&rect_dst),
            );
            self.video_context
                .VideoProcessorSetStreamAlpha(&self.processeur, 0, true, 1.0);
        }

        let mut flux = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: BOOL(1),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(vue_entree)),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };

        let resultat = unsafe {
            self.video_context
                .VideoProcessorBlt(&self.processeur, &self.vue_sortie, 0, std::slice::from_ref(&flux))
        };
        unsafe { ManuallyDrop::drop(&mut flux.pInputSurface) };
        resultat
            .map_err(|e| ErreurApercu::Gpu(format!("VideoProcessorBlt : {}", e.message())))?;

        unsafe { self.context.CopyResource(&self.staging, &self.sortie) };

        let mut mappee = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mappee))
        }
        .map_err(|e| ErreurApercu::Gpu(format!("Map staging : {}", e.message())))?;

        let ligne = self.largeur as usize * 4;
        unsafe {
            for y in 0..self.hauteur as usize {
                let src = (mappee.pData as *const u8).add(y * mappee.RowPitch as usize);
                let dst = self.tampon.as_mut_ptr().add(y * ligne);
                std::ptr::copy_nonoverlapping(src, dst, ligne);
            }
            self.context.Unmap(&self.staging, 0);
        }

        Ok(&self.tampon)
    }

    pub fn taille(&self) -> (u32, u32) {
        (self.largeur, self.hauteur)
    }

    pub fn octets(&self) -> usize {
        self.tampon.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn apercu_lit_une_frame() {
        let Ok(contexte) = ContexteGpu::creer(None) else {
            eprintln!("apercu : pas de device D3D11, test ignore");
            return;
        };
        let desc = DescTexture::new(Format::Nv12, 640, 360).expect("desc");
        let source = contexte.creer_texture(&desc).expect("texture source");
        let mut apercu = Apercu::nouveau(&contexte, (640, 360), (320, 180)).expect("apercu");
        let pixels = apercu.lire(&source).expect("lire");
        assert_eq!(pixels.len(), 320 * 180 * 4);
        assert_eq!(apercu.taille(), (320, 180));
        assert_eq!(apercu.octets(), 320 * 180 * 4);
    }
}
