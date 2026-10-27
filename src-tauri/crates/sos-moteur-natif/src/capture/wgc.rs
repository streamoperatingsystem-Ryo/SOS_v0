//! Session de capture d'un moniteur via Windows Graphics Capture.
//!
//! LOT 0.2b : **une frame à la demande**, en GPU, avec une seule copie CPU
//! explicite pour la vérification (`prochaine_frame`). La boucle temps réel
//! du moteur consommera la texture GPU directement — la copie CPU de ce lot
//! ne doit **pas** être réutilisée dans le chemin de production.
//!
//! Réglages (plan §7.3) : frame pool **free-threaded** de 4 buffers, curseur
//! capturé, aucune exigence MTA.

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIAdapter1, IDXGIDevice};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use super::{ErreurCapture, Moniteur};

/// Une frame capturée, en BGRA8, lignes de haut en bas.
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

/// Session de capture d'un moniteur.
///
/// La session est propriétaire du device, du pool et de la session WGC :
/// tout est libéré au `Drop` (le moteur gardera la session vivante tant que
/// le refcount est > 0).
pub struct CaptureMoniteur {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    taille: (u32, u32),
}

impl CaptureMoniteur {
    /// Démarre la capture d'un moniteur.
    ///
    /// Le device D3D11 est créé **sur l'adaptateur du moniteur** : sur une
    /// machine hybride, cela évite une copie inter-GPU (plan §6.3).
    pub fn demarrer(moniteur: &Moniteur) -> Result<Self, ErreurCapture> {
        // WinRT en MTA. RPC_E_CHANGED_MODE (déjà initialisé en STA) est
        // toléré : WGC free-threaded n'exige pas la MTA.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

        let adaptateur: IDXGIAdapter1 = crate::gpu::ouvrir_adaptateur(moniteur.adaptateur_index)
            .map_err(|e| ErreurCapture::Gpu(e.to_string()))?;

        let (device, context) = creer_device(Some(&adaptateur))?;

        let dxgi_device: IDXGIDevice = device
            .cast()
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;
        let winrt_device: IDirect3DDevice = inspectable
            .cast()
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;

        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| ErreurCapture::Wgc(e.message()))?;

        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForMonitor(HMONITOR(moniteur.hmon as *mut _)) }
                .map_err(|e| ErreurCapture::Wgc(e.message()))?;

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
            device,
            context,
            pool,
            session,
            taille: (taille.Width.max(0) as u32, taille.Height.max(0) as u32),
        })
    }

    /// Taille de la source, en pixels.
    pub fn taille(&self) -> (u32, u32) {
        self.taille
    }

    /// Attend la prochaine frame et la rapatrie en mémoire CPU (BGRA8).
    ///
    /// ⚠️ Usage **vérification uniquement** (exemple / tests). Le chemin de
    /// production consommera la texture GPU sans copie CPU.
    pub fn prochaine_frame(&self, timeout: Duration) -> Result<FrameBgra, ErreurCapture> {
        let debut = Instant::now();
        loop {
            match self.pool.TryGetNextFrame() {
                Ok(frame) => return self.lire_frame(&frame),
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
        unsafe { self.device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;
        let staging = staging.ok_or_else(|| ErreurCapture::Gpu("texture staging nulle".into()))?;

        unsafe { self.context.CopyResource(&staging, &texture) };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
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
            self.context.Unmap(&staging, 0);
        }

        Ok(FrameBgra {
            largeur,
            hauteur,
            pixels,
        })
    }
}

impl Drop for CaptureMoniteur {
    fn drop(&mut self) {
        // Ordre : session puis pool. Les erreurs de fermeture sont ignorées
        // (l'objet est de toute façon détruit).
        let _ = &self.session;
        let _ = &self.pool;
        let _ = &self.context;
        let _ = &self.device;
    }
}

/// Crée un device D3D11 BGRA (obligatoire pour WGC), sur l'adaptateur donné
/// ou sur l'adaptateur par défaut.
fn creer_device(
    adaptateur: Option<&IDXGIAdapter1>,
) -> Result<(ID3D11Device, ID3D11DeviceContext), ErreurCapture> {
    let adaptateur_dxgi: Option<IDXGIAdapter> = match adaptateur {
        Some(a) => Some(a.cast().map_err(|e| ErreurCapture::Gpu(e.message()))?),
        None => None,
    };

    // Règle D3D11 : avec un adaptateur explicite, le driver type DOIT être
    // UNKNOWN (sinon E_INVALIDARG). Sans adaptateur, on demande HARDWARE.
    let driver_type = if adaptateur_dxgi.is_some() {
        D3D_DRIVER_TYPE_UNKNOWN
    } else {
        D3D_DRIVER_TYPE_HARDWARE
    };

    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut niveau = D3D_FEATURE_LEVEL_11_0;

    unsafe {
        D3D11CreateDevice(
            adaptateur_dxgi.as_ref(),
            driver_type,
            None::<&windows::Win32::Foundation::HMODULE>,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut niveau),
            Some(&mut context),
        )
    }
    .map_err(|e| ErreurCapture::Gpu(e.message()))?;

    let device = device.ok_or_else(|| ErreurCapture::Gpu("device D3D11 nul".into()))?;
    let context = context.ok_or_else(|| ErreurCapture::Gpu("contexte D3D11 nul".into()))?;
    Ok((device, context))
}
