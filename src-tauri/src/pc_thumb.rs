//! Miniatures du sélecteur de capture (ObsSourceDialog) — Windows Graphics
//! Capture (WGC), la MÊME API qu'OBS utilise pour window_capture /
//! monitor_capture : la vignette montre ce qu'OBS captera réellement.
//!
//! - Fenêtre/jeu : CreateForWindow(HWND) → frame pool D3D11 free-threaded →
//!   1re frame (timeout 1,5 s) → staging texture CPU → BGRA → downscale box
//!   ≤ 256 px → PNG → data URL. Fenêtre minimisée / contenu protégé (DRM) /
//!   GPU exclusif → échec WGC → fallback icône de fenêtre (WM_GETICON ou
//!   icône de classe → PNG 48×48) : une carte a TOUJOURS une image.
//! - Écran : CreateForMonitor(HMONITOR) → même pipeline.
//! - Caméra : pas de miniature (conflit d'accès exclusif dshow avec OBS).
//!
//! Le device D3D11 + l'IDirect3DDevice WinRT sont créés UNE fois (OnceLock)
//! — les vignettes arrivent en rafale (un invoke par carte de la grille).
//! WGC_LOCK sérialise les sessions : les invokes concurrents se succèdent
//! plutôt que d'empiler des frame pools parallèles.
//!
//! Windows-only ; ailleurs les fonctions retournent None (le frontend
//! affiche l'emoji générique de la carte).

#[cfg(windows)]
mod imp {
    use crate::mask_png::encoder_png_pixels;
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use std::sync::{mpsc, Mutex, OnceLock};
    use std::time::Duration;
    use windows::core::{factory, Interface};
    use windows::Foundation::TypedEventHandler;
    use windows::Graphics::Capture::{
        Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
    };
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, S_OK, WPARAM};
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11Resource, ID3D11Texture2D,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
        D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HBRUSH,
        HDC, HGDIOBJ, HMONITOR,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows::Win32::UI::WindowsAndMessaging::{
        DrawIconEx, GetClassLongPtrW, IsWindow, SendMessageTimeoutW, DI_NORMAL,
        GCLP_HICON, GCLP_HICONSM, HICON, ICON_BIG, ICON_SMALL, ICON_SMALL2,
        SMTO_ABORTIFHUNG, WM_GETICON,
    };

    /// Timeout d'attente de la 1re frame WGC (fenêtre qui ne produit rien :
    /// minimisée, protégée — on bascule sur l'icône plutôt que bloquer).
    const FRAME_TIMEOUT: Duration = Duration::from_millis(1500);
    /// Largeur max de la vignette PNG (aspect conservé, downscale box).
    const MAX_W: usize = 256;
    /// Côté de la vignette icône (fallback).
    const ICO: i32 = 48;

    /// Device D3D11 + IDirect3DDevice WinRT — réutilisés entre les appels
    /// (une vingtaine de vignettes par ouverture du dialog). Send/Sync
    /// manuels : ces objets COM/WinRT sont free-threaded (agiles) — WGC_LOCK
    /// sérialise de toute façon chaque capture complète.
    struct Gpu {
        d3d: ID3D11Device,
        wrt: IDirect3DDevice,
    }
    unsafe impl Send for Gpu {}
    unsafe impl Sync for Gpu {}
    static GPU: OnceLock<Option<Gpu>> = OnceLock::new();
    /// Une seule session WGC à la fois (les invokes des cartes se sérialisent
    /// ici plutôt que d'empiler des frame pools concurrents).
    static WGC_LOCK: Mutex<()> = Mutex::new(());

    /// COM en MTA sur le thread appelant — les commandes Tauri tournent sur
    /// des threads du pool, non initialisés. RAII : RoUninitialize via
    /// CoUninitialize SEULEMENT si on est l'initialiseur (S_OK ; S_FALSE =
    /// déjà init → on ne touche pas).
    struct ComMta;
    impl ComMta {
        fn init() -> Option<Self> {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            (hr == S_OK).then_some(ComMta)
        }
    }
    impl Drop for ComMta {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    fn gpu() -> Option<&'static Gpu> {
        GPU.get_or_init(|| creer_gpu().ok()).as_ref()
    }

    /// D3D11 hardware (BGRA requis par WGC), fallback WARP logiciel si pas
    /// de GPU (VM, RDP sans GPU, driver cassé).
    fn creer_gpu() -> windows::core::Result<Gpu> {
        unsafe {
            let mut d3d = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )
            .or_else(|_| {
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_WARP,
                    None,
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut d3d),
                    None,
                    None,
                )
            })?;
            let d3d = d3d.ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?;
            let wrt: IDirect3DDevice = inspectable.cast()?;
            Ok(Gpu { d3d, wrt })
        }
    }

    /// Session WGC one-shot : frame pool free-threaded (pas d'exigence
    /// d'apartement MTA), 1re frame capturée → pixels BGRA ou None.
    fn frame_wgc(item: &GraphicsCaptureItem, gpu: &Gpu) -> Option<Vec<u8>> {
        let size = item.Size().ok()?;
        if size.Width <= 0 || size.Height <= 0 {
            return None;
        }
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &gpu.wrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )
        .ok()?;
        let session = pool.CreateCaptureSession(item).ok()?;
        // Non-fatal : ces setters exigent Windows 10 2004+ / Windows 11 —
        // sur un OS plus ancien ils échouent et la vignette garde curseur/bordure.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);

        let (tx, rx) = mpsc::channel::<Direct3D11CaptureFrame>();
        let handler = TypedEventHandler::new(
            move |p: &Option<Direct3D11CaptureFramePool>, _| {
                if let Some(p) = p {
                    if let Ok(f) = p.TryGetNextFrame() {
                        let _ = tx.send(f);
                    }
                }
                Ok(())
            },
        );
        pool.FrameArrived(&handler).ok()?;
        session.StartCapture().ok()?;
        let frame = rx.recv_timeout(FRAME_TIMEOUT).ok();
        let _ = session.Close();
        let _ = pool.Close();
        frame.and_then(|f| frame_vers_bgra(&f, gpu))
    }

    /// Frame WGC → staging texture CPU → pixels BGRA (row-major, w×h×4).
    fn frame_vers_bgra(frame: &Direct3D11CaptureFrame, gpu: &Gpu) -> Option<Vec<u8>> {
        let surface = frame.Surface().ok()?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast().ok()?;
        let tex: ID3D11Texture2D = unsafe { access.GetInterface() }.ok()?;

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        let (w, h) = (desc.Width as usize, desc.Height as usize);
        if w == 0 || h == 0 {
            return None;
        }

        let sdesc = D3D11_TEXTURE2D_DESC {
            Width: desc.Width,
            Height: desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: desc.SampleDesc,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe { gpu.d3d.CreateTexture2D(&sdesc, None, Some(&mut staging)) }.ok()?;
        let staging = staging?;

        let ctx = unsafe { gpu.d3d.GetImmediateContext() }.ok()?;
        let dst: ID3D11Resource = staging.cast().ok()?;
        let src: ID3D11Resource = tex.cast().ok()?;
        unsafe {
            ctx.CopyResource(&dst, &src);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&dst, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).ok()?;
            let pitch = mapped.RowPitch as usize;
            let mut bgra = vec![0u8; w * h * 4];
            if !mapped.pData.is_null() {
                let base = mapped.pData as *const u8;
                for y in 0..h {
                    std::ptr::copy_nonoverlapping(
                        base.add(y * pitch),
                        bgra[y * w * 4..].as_mut_ptr(),
                        (w * 4).min(pitch),
                    );
                }
            }
            ctx.Unmap(&dst, 0);
            Some(bgra)
        }
    }

    /// BGRA → RGBA + downscale box (moyenne des pixels sources du bloc
    /// destination) — ≤ MAX_W de large, aspect conservé.
    fn miniature_rgba(bgra: &[u8], w: usize, h: usize) -> (Vec<u8>, u32, u32) {
        let (nw, nh) = if w > MAX_W {
            (MAX_W, ((h * MAX_W / w).max(1)))
        } else {
            (w, h)
        };
        let mut rgba = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            let (y0, y1) = (y * h / nh, (((y + 1) * h / nh).max(y * h / nh + 1)).min(h));
            for x in 0..nw {
                let (x0, x1) = (x * w / nw, (((x + 1) * w / nw).max(x * w / nw + 1)).min(w));
                let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
                for row in bgra[y0 * w * 4..y1 * w * 4].chunks_exact(w * 4) {
                    for px in row[x0 * 4..x1 * 4].chunks_exact(4) {
                        b += px[0] as u64;
                        g += px[1] as u64;
                        r += px[2] as u64;
                        a += px[3] as u64;
                        n += 1;
                    }
                }
                let o = (y * nw + x) * 4;
                rgba[o] = (r / n) as u8;
                rgba[o + 1] = (g / n) as u8;
                rgba[o + 2] = (b / n) as u8;
                rgba[o + 3] = (a / n) as u8;
            }
        }
        (rgba, nw as u32, nh as u32)
    }

    /// Item WGC → vignette PNG (data URL) ou None.
    fn item_vers_data_url(item: GraphicsCaptureItem) -> Option<String> {
        let gpu = gpu()?;
        let _lock = WGC_LOCK.lock().ok()?;
        let bgra = frame_wgc(&item, gpu)?;
        let desc_w = item.Size().ok()?.Width as usize;
        let desc_h = item.Size().ok()?.Height as usize;
        let (rgba, w, h) = miniature_rgba(&bgra, desc_w, desc_h);
        Some(format!(
            "data:image/png;base64,{}",
            B64.encode(encoder_png_pixels(&rgba, w, h))
        ))
    }

    /// Miniature d'une fenêtre : frame WGC → PNG data URL ; si la fenêtre est
    /// incapturable (minimisée, protégée, disparue) → icône de la fenêtre.
    /// None seulement si même l'icône échoue (le front affiche l'emoji).
    pub fn window_thumbnail(hwnd: isize) -> Option<String> {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        let _com = ComMta::init();
        // Factory/CreateForWindow en échec → None → fallback icône (pas de
        // `?` ici : un échec WGC ne doit pas priver la carte de son icône).
        let png = if unsafe { IsWindow(hwnd) }.as_bool() {
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .ok()
                .and_then(|interop| {
                    unsafe { interop.CreateForWindow::<HWND, GraphicsCaptureItem>(hwnd) }.ok()
                })
                .and_then(item_vers_data_url)
        } else {
            None
        };
        png.or_else(|| icone_fenetre(hwnd).map(|p| format!("data:image/png;base64,{}", B64.encode(p))))
    }

    /// Miniature d'un écran : frame WGC du moniteur → PNG data URL.
    /// Pas de fallback icône : un HMONITOR invalide → None (emoji côté front).
    pub fn monitor_thumbnail(hmon: isize) -> Option<String> {
        let hmon = HMONITOR(hmon as *mut core::ffi::c_void);
        let _com = ComMta::init();
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().ok()?;
        unsafe { interop.CreateForMonitor::<HMONITOR, GraphicsCaptureItem>(hmon) }
            .ok()
            .and_then(item_vers_data_url)
    }

    /// Icône de la fenêtre rendue dans un DIB 32 bpp top-down ICO×ICO → PNG.
    /// WM_GETICON (petit puis grand) sinon icône de la classe fenêtre — marche
    /// même pour les processus protégés (pas d'accès process requis).
    fn icone_fenetre(hwnd: HWND) -> Option<Vec<u8>> {
        unsafe {
            let mut hicon = HICON::default();
            for taille in [ICON_SMALL2, ICON_BIG, ICON_SMALL] {
                let mut res: usize = 0;
                let _ = SendMessageTimeoutW(
                    hwnd,
                    WM_GETICON,
                    WPARAM(taille as usize),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    100,
                    Some(&mut res),
                );
                if res != 0 {
                    hicon = HICON(res as *mut core::ffi::c_void);
                    break;
                }
            }
            if hicon.0.is_null() {
                for gcl in [GCLP_HICONSM, GCLP_HICON] {
                    let p = GetClassLongPtrW(hwnd, gcl);
                    if p != 0 {
                        hicon = HICON(p as *mut core::ffi::c_void);
                        break;
                    }
                }
            }
            if hicon.0.is_null() {
                return None;
            }

            // DIB 32 bpp top-down : CreateCompatibleDC(null) = DC compatible
            // écran — pas besoin de GetDC/ReleaseDC.
            let mem = CreateCompatibleDC(HDC::default());
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: ICO,
                    biHeight: -ICO, // négatif = lignes top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bmp: HBITMAP =
                CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let old = SelectObject(mem, HGDIOBJ(bmp.0));
            let _ = DrawIconEx(mem, 0, 0, hicon, ICO, ICO, 0, HBRUSH::default(), DI_NORMAL);

            let mut px = vec![0u8; (ICO * ICO * 4) as usize];
            if !bits.is_null() {
                std::ptr::copy_nonoverlapping(bits as *const u8, px.as_mut_ptr(), px.len());
            }
            SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem);

            // BGRA → RGBA. Les icônes sans couche alpha (8 bpp historiques)
            // laissent alpha=0 partout → opaque là où il y a de la couleur.
            let sans_alpha = px.chunks_exact(4).all(|p| p[3] == 0);
            for p in px.chunks_exact_mut(4) {
                p.swap(0, 2);
                if sans_alpha {
                    p[3] = if p[0] | p[1] | p[2] != 0 { 255 } else { 0 };
                }
            }
            Some(encoder_png_pixels(&px, ICO as u32, ICO as u32))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn window_thumbnail(_hwnd: isize) -> Option<String> {
        None
    }
    pub fn monitor_thumbnail(_hmon: isize) -> Option<String> {
        None
    }
}

pub use imp::*;
