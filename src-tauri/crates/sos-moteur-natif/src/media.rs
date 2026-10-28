//! Médias statiques du moteur — décodage WIC (PNG/JPG/BMP) → texture BGRA.
//!
//! Tranche P3 n°1 : fond de scène + widgets média, sans animation ni texte.
//! Les vidéos (Media Foundation Source Reader) et le rendu de texte arrivent
//! dans les tranches suivantes — ne pas élargir ce module sans plan.

use std::ffi::c_void;
use std::fmt;
use std::path::{Path, PathBuf};

use windows::core::{HSTRING, Interface};
use windows::Win32::Foundation::{RECT, GENERIC_READ};
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
use windows::Win32::Media::MediaFoundation::{
    IMFSourceReader, IMFSample, MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL,
    MFMediaType_Video, MFVideoFormat_RGB32, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM,
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

/// Comment le média remplit son rect (contrat `mediaFit`/`bgFit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Étirement au rect (historique, aucune déformation évitée).
    Etirer,
    /// Contenu entier, centré (letterbox).
    Ajuster,
    /// Rect entièrement couvert, crop centré (cover).
    Remplir,
}

impl Fit {
    pub fn depuis_nom(nom: &str) -> Self {
        match nom {
            "ajuster" => Fit::Ajuster,
            "remplir" => Fit::Remplir,
            _ => Fit::Etirer,
        }
    }
}

/// Un média statique à composer : chemin résolu + destination + ordre z.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaCfg {
    pub chemin: PathBuf,
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
    pub fit: Fit,
}

/// Média décodé, prêt pour le compositeur.
pub struct MediaChargee {
    pub texture: ID3D11Texture2D,
    pub taille: (u32, u32),
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
    pub fit: Fit,
    video: Option<VideoLecteur>,
}

struct VideoLecteur {
    reader: IMFSourceReader,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    /// Frame lue mais **pas encore due** (PTS postérieur au temps écoulé).
    /// Le Source Reader ne sait pas remettre un échantillon : on le garde.
    en_attente: Option<(IMFSample, i64)>,
}

impl MediaChargee {
    pub fn est_video(&self) -> bool {
        self.video.is_some()
    }

    /// Avance la vidéo jusqu'à la frame **due** à `temps_us`.
    ///
    /// Une frame lue dont le PTS est postérieur au temps écoulé est conservée
    /// jusqu'à son échéance (`en_attente`) : sans cette attente, chaque appel
    /// du moteur afficherait une frame et toute vidéo serait jouée à la
    /// cadence du moteur — ×2 pour une source 30 fps, ×2,5 pour du 24 fps
    /// (mesuré 2026-10-28 : frame 61 à t = 1 s d'un compteur 24 fps).
    /// Sans effet pour une image. Fin de flux = dernière image figée.
    pub fn avancer(&mut self, temps_us: i64) -> Result<(), ErreurMedia> {
        let Some(video) = self.video.as_mut() else {
            return Ok(());
        };
        loop {
            let en_attente = video.en_attente.take();
            let (sample, pts_us) = match en_attente {
                Some(en_attente) => en_attente,
                None => {
                    let mut flags = 0u32;
                    let mut sample: Option<IMFSample> = None;
                    unsafe {
                        video.reader.ReadSample(
                            MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                            0,
                            None,
                            Some(&mut flags),
                            None,
                            Some(&mut sample),
                        )
                    }
                    .map_err(|e| {
                        ErreurMedia::Decodage(format!("ReadSample : {}", e.message()))
                    })?;
                    if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                        return Ok(());
                    }
                    let Some(sample) = sample else {
                        return Ok(());
                    };
                    let pts_us = unsafe { sample.GetSampleTime() }.unwrap_or(0) / 10;
                    (sample, pts_us)
                }
            };
            // Frame future : on la garde pour un prochain appel.
            if pts_us > temps_us {
                video.en_attente = Some((sample, pts_us));
                return Ok(());
            }
            let buffer = unsafe { sample.ConvertToContiguousBuffer() }
                .map_err(|e| ErreurMedia::Decodage(format!("buffer : {}", e.message())))?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut longueur = 0u32;
            unsafe { buffer.Lock(&mut ptr, None, Some(&mut longueur)) }
                .map_err(|e| ErreurMedia::Decodage(format!("Lock : {}", e.message())))?;
            let ressource: windows::Win32::Graphics::Direct3D11::ID3D11Resource = self
                .texture
                .cast()
                .map_err(|e| ErreurMedia::Gpu(format!("texture → ressource : {}", e.message())))?;
            unsafe {
                video.context.UpdateSubresource(
                    Some(&ressource),
                    0,
                    None,
                    ptr as *const c_void,
                    self.taille.0 * 4,
                    0,
                )
            };
            unsafe { buffer.Unlock() }
                .map_err(|e| ErreurMedia::Decodage(format!("Unlock : {}", e.message())))?;
        }
    }
}

/// Calcule la fenêtre source et la destination selon le fit.
///
/// `rect` est borné au canvas ; `taille` = dimensions natives du média.
/// `Etirer` garde toute l'image et le rect ; `Ajuster` réduit la destination
/// au ratio du média (centrée) ; `Remplir` garde le rect et croppe la source.
pub fn geometrie(
    taille: (u32, u32),
    rect: (f64, f64, f64, f64),
    canvas: (u32, u32),
    fit: Fit,
) -> (RECT, RECT) {
    let (mw, mh) = (taille.0.max(1) as f64, taille.1.max(1) as f64);
    let (cw, ch) = (canvas.0 as f64, canvas.1 as f64);
    let x = rect.0.clamp(0.0, cw);
    let y = rect.1.clamp(0.0, ch);
    let w = rect.2.max(1.0).min((cw - x).max(1.0));
    let h = rect.3.max(1.0).min((ch - y).max(1.0));

    let source_pleine = RECT {
        left: 0,
        top: 0,
        right: taille.0 as i32,
        bottom: taille.1 as i32,
    };
    let destination = RECT {
        left: x as i32,
        top: y as i32,
        right: (x + w) as i32,
        bottom: (y + h) as i32,
    };

    match fit {
        Fit::Etirer => (source_pleine, destination),
        Fit::Ajuster => {
            let echelle = (w / mw).min(h / mh);
            let (dw, dh) = (mw * echelle, mh * echelle);
            // Arrondi : évite une asymétrie de 1 px du letterbox.
            let dx = (x + (w - dw) / 2.0).round();
            let dy = (y + (h - dh) / 2.0).round();
            (
                source_pleine,
                RECT {
                    left: dx as i32,
                    top: dy as i32,
                    right: (dx + dw).round() as i32,
                    bottom: (dy + dh).round() as i32,
                },
            )
        }
        Fit::Remplir => {
            let ratio_destination = w / h;
            let ratio_media = mw / mh;
            let (sw, sh) = if ratio_media > ratio_destination {
                (mh * ratio_destination, mh)
            } else {
                (mw, mw / ratio_destination)
            };
            let (sx, sy) = ((mw - sw) / 2.0, (mh - sh) / 2.0);
            (
                RECT {
                    left: sx as i32,
                    top: sy as i32,
                    right: (sx + sw) as i32,
                    bottom: (sy + sh) as i32,
                },
                destination,
            )
        }
    }
}

/// Charge une image ou une vidéo selon l'extension.
pub fn charger(contexte: &ContexteGpu, cfg: &MediaCfg) -> Result<MediaChargee, ErreurMedia> {
    if !cfg.chemin.is_file() {
        return Err(ErreurMedia::Fichier(format!(
            "introuvable : {}",
            cfg.chemin.display()
        )));
    }
    if est_video(&cfg.chemin) {
        charger_video(contexte, cfg)
    } else {
        charger_image(contexte, cfg)
    }
}

/// Extensions vidéo prises en charge par le Source Reader.
pub fn est_video(chemin: &Path) -> bool {
    matches!(
        chemin
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "mp4" | "m4v" | "mov" | "avi" | "mkv" | "webm"
    )
}

fn charger_image(contexte: &ContexteGpu, cfg: &MediaCfg) -> Result<MediaChargee, ErreurMedia> {
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
        fit: cfg.fit,
        video: None,
    })
}

/// Ouvre une vidéo via Media Foundation Source Reader (sortie RGB32).
fn charger_video(contexte: &ContexteGpu, cfg: &MediaCfg) -> Result<MediaChargee, ErreurMedia> {
    crate::encode::detect::initialiser();
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    unsafe {
        let mut attrs: Option<windows::Win32::Media::MediaFoundation::IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 2)
            .map_err(|e| ErreurMedia::Decodage(format!("attributs : {}", e.message())))?;
        let attrs = attrs.ok_or_else(|| ErreurMedia::Decodage("attributs nuls".to_string()))?;
        attrs
            .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
            .map_err(|e| ErreurMedia::Decodage(format!("attributs : {}", e.message())))?;

        let reader = MFCreateSourceReaderFromURL(
            &HSTRING::from(cfg.chemin.as_os_str()),
            &attrs,
        )
        .map_err(|e| {
            ErreurMedia::Decodage(format!("ouverture {} : {}", cfg.chemin.display(), e.message()))
        })?;

        let type_sortie = MFCreateMediaType()
            .map_err(|e| ErreurMedia::Decodage(format!("type : {}", e.message())))?;
        type_sortie
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|e| ErreurMedia::Decodage(format!("major : {}", e.message())))?;
        type_sortie
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(|e| ErreurMedia::Decodage(format!("subtype : {}", e.message())))?;
        reader
            .SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &type_sortie)
            .map_err(|e| ErreurMedia::Decodage(format!("type RGB32 : {}", e.message())))?;

        let courant = reader
            .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
            .map_err(|e| ErreurMedia::Decodage(format!("type courant : {}", e.message())))?;
        let pack = courant
            .GetUINT64(&MF_MT_FRAME_SIZE)
            .map_err(|e| ErreurMedia::Decodage(format!("taille vidéo : {}", e.message())))?;
        let largeur = (pack >> 32) as u32;
        let hauteur = pack as u32;
        if largeur == 0 || hauteur == 0 {
            return Err(ErreurMedia::Decodage("vidéo de taille nulle".to_string()));
        }

        let texture = creer_texture_video(contexte, largeur, hauteur)?;
        let mut media = MediaChargee {
            texture,
            taille: (largeur, hauteur),
            rect: cfg.rect,
            z: cfg.z,
            fit: cfg.fit,
            video: Some(VideoLecteur {
                reader,
                context: contexte.context().clone(),
                en_attente: None,
            }),
        };
        media.avancer(0)?;
        Ok(media)
    }
}

fn creer_texture_video(
    contexte: &ContexteGpu,
    largeur: u32,
    hauteur: u32,
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
    let mut texture: Option<ID3D11Texture2D> = None;
    unsafe { contexte.device().CreateTexture2D(&desc, None, Some(&mut texture)) }
        .map_err(|e| ErreurMedia::Gpu(format!("CreateTexture2D vidéo : {}", e.message())))?;
    texture.ok_or_else(|| ErreurMedia::Gpu("texture vidéo nulle".into()))
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
            fit: Fit::Etirer,
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

    #[test]
    fn extensions_video_reconnues() {
        assert!(est_video(Path::new("a.MP4")));
        assert!(est_video(Path::new("b.webm")));
        assert!(est_video(Path::new("c.mkv")));
        assert!(!est_video(Path::new("d.png")));
        assert!(!est_video(Path::new("e")));
    }

    #[test]
    fn fit_etirer_garde_tout() {
        let (source, dest) = geometrie((1920, 1080), (0.0, 0.0, 100.0, 100.0), (100, 100), Fit::Etirer);
        assert_eq!((source.left, source.top, source.right, source.bottom), (0, 0, 1920, 1080));
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (0, 0, 100, 100));
    }

    #[test]
    fn fit_ajuster_letterbox_centre() {
        // 16:9 dans un carré : 100×56 centré (22 px en haut et en bas).
        let (source, dest) = geometrie((1920, 1080), (0.0, 0.0, 100.0, 100.0), (100, 100), Fit::Ajuster);
        assert_eq!((source.left, source.top, source.right, source.bottom), (0, 0, 1920, 1080));
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (0, 22, 100, 78));
    }

    #[test]
    fn fit_remplir_crop_centre() {
        // 16:9 dans un carré : crop 1080×1080 centré horizontalement.
        let (source, dest) = geometrie((1920, 1080), (0.0, 0.0, 100.0, 100.0), (100, 100), Fit::Remplir);
        assert_eq!((source.left, source.top, source.right, source.bottom), (420, 0, 1500, 1080));
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (0, 0, 100, 100));
    }

    #[test]
    fn fit_est_borne_au_canvas() {
        let (_, dest) = geometrie((100, 100), (50.0, 50.0, 500.0, 500.0), (100, 100), Fit::Etirer);
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (50, 50, 100, 100));
    }
}
