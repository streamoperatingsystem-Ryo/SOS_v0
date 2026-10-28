//! Conversion BGRA → NV12 **sur GPU** (D3D11 Video Processor).
//!
//! Remplace la conversion CPU du spike : aucune frame ne repasse par la
//! mémoire système entre la capture et l'encodeur (plan §11, invariant 1).
//!
//! Le video processor gère la conversion colorimétrique (BT.709) et le
//! sous-échantillonnage 4:2:0 côté pilote.

use std::mem::ManuallyDrop;
use std::ptr::null_mut;

use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor,
    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC};
use windows::core::Interface;

use crate::encode::ErreurEncodage;
use crate::gpu::ContexteGpu;

/// Convertisseur BGRA → NV12 attaché à un contexte GPU.
pub struct ConvertisseurNv12 {
    device: ID3D11Device,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    enumerateur: ID3D11VideoProcessorEnumerator,
    processeur: ID3D11VideoProcessor,
    largeur: u32,
    hauteur: u32,
}

impl ConvertisseurNv12 {
    /// Prépare le video processor pour une taille et un fps donnés.
    pub fn nouveau(
        contexte: &ContexteGpu,
        largeur: u32,
        hauteur: u32,
        fps: u32,
    ) -> Result<Self, ErreurEncodage> {
        let largeur = largeur & !1;
        let hauteur = hauteur & !1;
        if largeur == 0 || hauteur == 0 || fps == 0 {
            return Err(ErreurEncodage::Conversion(
                "dimensions ou fps invalides".into(),
            ));
        }

        let video_device: ID3D11VideoDevice = contexte
            .device()
            .cast()
            .map_err(|e| ErreurEncodage::Gpu(format!("ID3D11VideoDevice : {}", e.message())))?;
        let video_context: ID3D11VideoContext = contexte
            .context()
            .cast()
            .map_err(|e| ErreurEncodage::Gpu(format!("ID3D11VideoContext : {}", e.message())))?;

        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: fps,
                Denominator: 1,
            },
            InputWidth: largeur,
            InputHeight: hauteur,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: fps,
                Denominator: 1,
            },
            OutputWidth: largeur,
            OutputHeight: hauteur,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };

        let enumerateur = unsafe { video_device.CreateVideoProcessorEnumerator(&desc) }.map_err(
            |e| ErreurEncodage::Gpu(format!("CreateVideoProcessorEnumerator : {}", e.message())),
        )?;

        let processeur = unsafe { video_device.CreateVideoProcessor(&enumerateur, 0) }
            .map_err(|e| ErreurEncodage::Gpu(format!("CreateVideoProcessor : {}", e.message())))?;

        Ok(Self {
            device: contexte.device().clone(),
            context: contexte.context().clone(),
            video_device,
            video_context,
            enumerateur,
            processeur,
            largeur,
            hauteur,
        })
    }

    pub fn largeur(&self) -> u32 {
        self.largeur
    }

    pub fn hauteur(&self) -> u32 {
        self.hauteur
    }

    /// Convertit une texture BGRA en texture NV12 (GPU uniquement).
    ///
    /// La texture NV12 est neuve à chaque appel ; le pool viendra avec le
    /// compositeur (lot 2.1).
    pub fn convertir(
        &self,
        bgra: &ID3D11Texture2D,
    ) -> Result<ID3D11Texture2D, ErreurEncodage> {
        let nv12 = self.creer_cible()?;
        self.convertir_vers(bgra, &nv12)?;
        Ok(nv12)
    }

    /// Crée une texture NV12 réutilisable comme cible du video processor.
    ///
    /// C'est la texture que le chemin zéro-copie enregistre **une fois**
    /// auprès de NVENC (lot 0.2g-γ) : elle est réécrite à chaque frame.
    pub fn creer_cible(&self) -> Result<ID3D11Texture2D, ErreurEncodage> {
        let desc_nv12 = D3D11_TEXTURE2D_DESC {
            Width: self.largeur,
            Height: self.hauteur,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };

        let mut nv12: Option<ID3D11Texture2D> = None;
        unsafe { self.device.CreateTexture2D(&desc_nv12, None, Some(&mut nv12)) }.map_err(
            |e| ErreurEncodage::Gpu(format!("CreateTexture2D NV12 : {}", e.message())),
        )?;
        nv12.ok_or_else(|| ErreurEncodage::Gpu("texture NV12 nulle".into()))
    }

    /// Convertit une texture BGRA **dans une texture NV12 existante**.
    ///
    /// Variante du chemin zéro-copie : la cible est enregistrée une fois
    /// auprès de l'encodeur puis réécrite à chaque frame. Sûr en mode
    /// synchrone (`enableEncodeAsync = 0`) : la frame est consommée par
    /// `nvEncLockBitstream` avant le `Blt` suivant.
    pub fn convertir_vers(
        &self,
        bgra: &ID3D11Texture2D,
        cible: &ID3D11Texture2D,
    ) -> Result<(), ErreurEncodage> {
        let desc_sortie = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };

        let mut vue_sortie: Option<ID3D11VideoProcessorOutputView> = None;
        unsafe {
            self.video_device.CreateVideoProcessorOutputView(
                cible,
                &self.enumerateur,
                &desc_sortie,
                Some(&mut vue_sortie),
            )
        }
        .map_err(|e| {
            ErreurEncodage::Gpu(format!(
                "CreateVideoProcessorOutputView : {}",
                e.message()
            ))
        })?;
        let vue_sortie =
            vue_sortie.ok_or_else(|| ErreurEncodage::Gpu("vue de sortie nulle".into()))?;

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
                bgra,
                &self.enumerateur,
                &desc_entree,
                Some(&mut vue_entree),
            )
        }
        .map_err(|e| {
            ErreurEncodage::Gpu(format!(
                "CreateVideoProcessorInputView : {}",
                e.message()
            ))
        })?;
        let vue_entree =
            vue_entree.ok_or_else(|| ErreurEncodage::Gpu("vue d'entrée nulle".into()))?;

        let mut flux = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: BOOL(1),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: null_mut(),
            pInputSurface: ManuallyDrop::new(Some(vue_entree)),
            ppFutureSurfaces: null_mut(),
            ppPastSurfacesRight: null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: null_mut(),
        };

        let resultat = unsafe {
            self.video_context.VideoProcessorBlt(
                &self.processeur,
                &vue_sortie,
                0,
                std::slice::from_ref(&flux),
            )
        };

        // Le flux n'est pas propriétaire : on libère explicitement la vue.
        unsafe { ManuallyDrop::drop(&mut flux.pInputSurface) };

        resultat
            .map_err(|e| ErreurEncodage::Gpu(format!("VideoProcessorBlt : {}", e.message())))?;

        Ok(())
    }

    /// Variante pour les encodeurs qui refusent une surface « render target » :
    /// copie GPU vers une texture NV12 sans bind flags.
    pub fn convertir_pour_encodeur(
        &self,
        bgra: &ID3D11Texture2D,
    ) -> Result<ID3D11Texture2D, ErreurEncodage> {
        let rendu = self.convertir(bgra)?;

        let desc = D3D11_TEXTURE2D_DESC {
            Width: self.largeur,
            Height: self.hauteur,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: 0,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };

        let mut cible: Option<ID3D11Texture2D> = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut cible)) }.map_err(|e| {
            ErreurEncodage::Gpu(format!("CreateTexture2D NV12 plain : {}", e.message()))
        })?;
        let cible = cible.ok_or_else(|| ErreurEncodage::Gpu("texture NV12 nulle".into()))?;

        unsafe { self.context.CopyResource(&cible, &rendu) };
        Ok(cible)
    }

    /// Lit une texture NV12 GPU vers un buffer NV12 linéaire (Y puis UV).
    ///
    /// C'est la seule copie mémoire du chemin FFmpeg : elle remplace la
    /// conversion par pixel (~20 ms/frame) par une copie (~1-2 ms).
    pub fn lire_texture_nv12(
        &self,
        texture: &ID3D11Texture2D,
    ) -> Result<Vec<u8>, ErreurEncodage> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        let desc_staging = D3D11_TEXTURE2D_DESC {
            Width: desc.Width,
            Height: desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
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
        unsafe { self.device.CreateTexture2D(&desc_staging, None, Some(&mut staging)) }.map_err(
            |e| ErreurEncodage::Gpu(format!("CreateTexture2D staging NV12 : {}", e.message())),
        )?;
        let staging =
            staging.ok_or_else(|| ErreurEncodage::Gpu("texture staging NV12 nulle".into()))?;

        unsafe { self.context.CopyResource(&staging, texture) };

        let largeur = desc.Width as usize;
        let hauteur = desc.Height as usize;
        let taille_y = largeur * hauteur;
        let mut nv12 = vec![0u8; taille_y + taille_y / 2];

        // Plan Y + plan UV dans le MÊME mapping : NV12 est une seule
        // sous-ressource D3D11. Mapper la sous-ressource 1 renvoie
        // E_INVALIDARG sur les pilotes réels (constaté 2026-10-28).
        let mut mappee = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mappee)) }.map_err(
            |e| ErreurEncodage::Gpu(format!("Map NV12 : {}", e.message())),
        )?;
        unsafe {
            let base = mappee.pData as *const u8;
            let pas = mappee.RowPitch as usize;
            for y in 0..hauteur {
                let src = base.add(y * pas);
                let dst = nv12.as_mut_ptr().add(y * largeur);
                std::ptr::copy_nonoverlapping(src, dst, largeur);
            }
            let uv = base.add(pas * hauteur);
            for y in 0..hauteur / 2 {
                let src = uv.add(y * pas);
                let dst = nv12.as_mut_ptr().add(taille_y + y * largeur);
                std::ptr::copy_nonoverlapping(src, dst, largeur);
            }
            self.context.Unmap(&staging, 0);
        }

        Ok(nv12)
    }
}
