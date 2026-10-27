//! Session de capture d'un moniteur **ou d'une fenêtre** via Windows Graphics
//! Capture.
//!
//! Le device D3D11 est **partagé** avec l'encodeur (`gpu::ContexteGpu`) :
//! c'est la condition du zéro-copie. `prochaine_texture` rend la texture GPU
//! telle quelle ; `prochaine_frame` ne sert qu'à la vérification (une copie
//! CPU explicite, jamais dans le chemin de production).
//!
//! Réglages (plan §7.3) : frame pool **free-threaded** de 4 buffers, curseur
//! capturé, aucune exigence MTA.

use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use super::{ErreurCapture, Moniteur};
use crate::gpu::ContexteGpu;

/// Une frame capturée, en BGRA8, lignes de haut en bas (vérification seule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameBgra {
    pub largeur: u32,
    pub hauteur: u32,
    pub pixels: Vec<u8>,
}

impl FrameBgra {
    /// Taille attendue en octets (4 octets par pixel).
    pub fn taille_attendue(&self) -> usize {
        self.largeur as usize * self.hauteur as usize * 4
    }
}

/// Session de capture WGC — moniteur ou fenêtre.
///
/// La session partage le `ContexteGpu` fourni ; tout est libéré au `Drop`.
pub struct CaptureWgc {
    contexte: Arc<ContexteGpu>,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    taille: (u32, u32),
}

/// Alias historique : le cas d'usage principal reste le moniteur.
pub type CaptureMoniteur = CaptureWgc;

impl CaptureWgc {
    /// Démarre la capture en créant son propre contexte GPU sur l'adaptateur
    /// du moniteur.
    pub fn demarrer(moniteur: &Moniteur) -> Result<Self, ErreurCapture> {
        let contexte = Arc::new(
            ContexteGpu::creer(Some(moniteur.adaptateur_index))
                .map_err(|e| ErreurCapture::Gpu(e.to_string()))?,
        );
        Self::demarrer_avec(moniteur, contexte)
    }

    /// Démarre la capture sur un contexte GPU existant (partagé avec
    /// l'encodeur → zéro-copie).
    pub fn demarrer_avec(
        moniteur: &Moniteur,
        contexte: Arc<ContexteGpu>,
    ) -> Result<Self, ErreurCapture> {
        // WinRT en MTA. RPC_E_CHANGED_MODE (déjà initialisé en STA) est
        // toléré : WGC free-threaded n'exige pas la MTA.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| ErreurCapture::Wgc(e.message()))?;

        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForMonitor(HMONITOR(moniteur.hmon as *mut _)) }
                .map_err(|e| ErreurCapture::Wgc(e.message()))?;

        Self::demarrer_item(item, contexte)
    }

    /// Démarre la capture d'une **fenêtre** (HWND natif) sur son propre
    /// contexte GPU (adaptateur par défaut).
    ///
    /// Une fenêtre réduite ne produit pas d'image : WGC exige une fenêtre
    /// visible à l'écran.
    pub fn demarrer_fenetre(hwnd: isize) -> Result<Self, ErreurCapture> {
        let contexte = Arc::new(
            ContexteGpu::creer(None).map_err(|e| ErreurCapture::Gpu(e.to_string()))?,
        );
        Self::demarrer_fenetre_avec(hwnd, contexte)
    }

    /// Démarre la capture d'une fenêtre sur un contexte GPU existant.
    pub fn demarrer_fenetre_avec(
        hwnd: isize,
        contexte: Arc<ContexteGpu>,
    ) -> Result<Self, ErreurCapture> {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| ErreurCapture::Wgc(e.message()))?;

        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForWindow(HWND(hwnd as *mut _)) }
                .map_err(|e| ErreurCapture::Wgc(e.message()))?;

        Self::demarrer_item(item, contexte)
    }

    /// Tronc commun moniteur/fenêtre : device WinRT, pool, session.
    fn demarrer_item(
        item: GraphicsCaptureItem,
        contexte: Arc<ContexteGpu>,
    ) -> Result<Self, ErreurCapture> {
        let dxgi_device: IDXGIDevice = contexte
            .device()
            .cast()
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;
        let winrt_device: IDirect3DDevice = inspectable
            .cast()
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;

        let taille = item.Size().map_err(|e| ErreurCapture::Wgc(e.message()))?;

        // 4 buffers : leçon de l'archive (2 provoque des drops quand le
        // compositeur système a une irrégularité).
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            4,
            taille,
        )
        .map_err(|e| ErreurCapture::Wgc(e.message()))?;

        let session = pool
            .CreateCaptureSession(&item)
            .map_err(|e| ErreurCapture::Wgc(e.message()))?;

        let _ = session.SetIsCursorCaptureEnabled(true);
        session
            .StartCapture()
            .map_err(|e| ErreurCapture::Wgc(e.message()))?;

        Ok(Self {
            contexte,
            pool,
            session,
            taille: (taille.Width.max(0) as u32, taille.Height.max(0) as u32),
        })
    }

    /// Taille de la source, en pixels.
    pub fn taille(&self) -> (u32, u32) {
        self.taille
    }

    /// Contexte GPU partagé (device/context à réutiliser pour l'encodage).
    pub fn contexte(&self) -> &Arc<ContexteGpu> {
        &self.contexte
    }

    /// Attend la prochaine frame et rend sa **texture GPU** (zéro-copie).
    ///
    /// La texture appartient au frame pool : elle doit être consommée
    /// immédiatement (conversion/encodage), pas conservée.
    pub fn prochaine_texture(
        &self,
        timeout: Duration,
    ) -> Result<ID3D11Texture2D, ErreurCapture> {
        let frame = self.attendre_frame(timeout)?;
        let surface = frame
            .Surface()
            .map_err(|e| ErreurCapture::Wgc(e.message()))?;
        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|e| ErreurCapture::Wgc(e.message()))?;
        unsafe { access.GetInterface() }.map_err(|e| ErreurCapture::Gpu(e.message()))
    }

    /// Attend la prochaine frame et la rapatrie en mémoire CPU (BGRA8).
    ///
    /// ⚠️ Usage **vérification uniquement** (exemple / tests).
    pub fn prochaine_frame(&self, timeout: Duration) -> Result<FrameBgra, ErreurCapture> {
        let frame = self.attendre_frame(timeout)?;
        self.lire_frame(&frame)
    }

    fn attendre_frame(
        &self,
        timeout: Duration,
    ) -> Result<Direct3D11CaptureFrame, ErreurCapture> {
        let debut = Instant::now();
        loop {
            match self.pool.TryGetNextFrame() {
                Ok(frame) => return Ok(frame),
                Err(e) => {
                    if debut.elapsed() >= timeout {
                        return Err(ErreurCapture::Wgc(format!(
                            "aucune frame en {} ms : {}",
                            timeout.as_millis(),
                            e.message()
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(8));
                }
            }
        }
    }

    fn lire_frame(&self, frame: &Direct3D11CaptureFrame) -> Result<FrameBgra, ErreurCapture> {
        let surface = frame
            .Surface()
            .map_err(|e| ErreurCapture::Wgc(e.message()))?;
        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|e| ErreurCapture::Wgc(e.message()))?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface() }
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        let staging_desc = D3D11_TEXTURE2D_DESC {
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
        unsafe {
            self.contexte
                .device()
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
        }
        .map_err(|e| ErreurCapture::Gpu(e.message()))?;
        let staging = staging.ok_or_else(|| ErreurCapture::Gpu("texture staging nulle".into()))?;

        unsafe {
            self.contexte
                .context()
                .CopyResource(&staging, &texture)
        };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.contexte
                .context()
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|e| ErreurCapture::Gpu(e.message()))?;

        let largeur = desc.Width;
        let hauteur = desc.Height;
        let ligne_octets = largeur as usize * 4;
        let mut pixels = vec![0u8; ligne_octets * hauteur as usize];

        unsafe {
            for y in 0..hauteur as usize {
                let src = (mapped.pData as *const u8).add(y * mapped.RowPitch as usize);
                let dst = pixels.as_mut_ptr().add(y * ligne_octets);
                std::ptr::copy_nonoverlapping(src, dst, ligne_octets);
            }
            self.contexte.context().Unmap(&staging, 0);
        }

        Ok(FrameBgra {
            largeur,
            hauteur,
            pixels,
        })
    }
}

impl Drop for CaptureWgc {
    fn drop(&mut self) {
        // Ordre : session puis pool. Les erreurs de fermeture sont ignorées.
        let _ = &self.session;
        let _ = &self.pool;
    }
}
