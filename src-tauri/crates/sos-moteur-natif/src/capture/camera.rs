//! Session de capture d'une caméra via Media Foundation.
//!
//! Différence structurelle avec WGC : MF ne fournit pas de texture GPU —
//! le Source Reader sort des buffers CPU RGB32. Le fil moteur ne doit donc
//! **jamais** lire les échantillons (un `ReadSample` bloquant gèlerait la
//! composition) : un **thread dédié** lit et dépose chaque frame dans un
//! **slot à une place** — une frame neuve remplace l'ancienne non consommée
//! (dernière-frame, jamais de file). L'upload GPU est fait par le
//! consommateur : `copier_dans` → `UpdateSubresource` sur le thread de
//! composition, comme la copie WGC. Aucun accès D3D11 depuis le fil de
//! capture → aucune race sur le contexte immédiat.
//!
//! L'appareil est choisi par son nom convivial (`cameraDeviceId` persisté
//! dans le widget) ; `None`, nom inconnu ou device unique → **premier**
//! device énuméré — la caméra par défaut, comme le dashboard.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{ID3D11Resource, ID3D11Texture2D};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFMediaSource, IMFSample, IMFSourceReader,
    MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromMediaSource,
    MFEnumDeviceSources, MFMediaType_Video, MFVideoFormat_RGB32,
    MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::core::GUID;

use super::wgc::FrameBgra;
use super::ErreurCapture;
use crate::gpu::ContexteGpu;

/// Une caméra énumérée par Media Foundation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraInfo {
    /// Nom convivial (`MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME`) — valeur
    /// persistée par `cameraDeviceId`.
    pub nom: String,
    /// Lien symbolique du device (identité stable — diagnostic).
    pub lien: String,
}

/// Énumère les caméras (sources vidéo MF), dans l'ordre du système.
///
/// Une machine sans caméra rend une liste vide — pas une erreur : le
/// refus a lieu au `demarrer`, qui sait dire « aucune caméra ».
pub fn enumerer_cameras() -> Result<Vec<CameraInfo>, ErreurCapture> {
    Ok(enum_devices()?
        .into_iter()
        .map(|(info, _)| info)
        .collect())
}

/// Énumère les caméras **avec leur activate** (création de source).
fn enum_devices() -> Result<Vec<(CameraInfo, IMFActivate)>, ErreurCapture> {
    crate::encode::detect::initialiser();
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1)
            .map_err(|e| ErreurCapture::Camera(format!("attributs : {}", e.message())))?;
        let attrs = attrs
            .ok_or_else(|| ErreurCapture::Camera("attributs nuls".to_string()))?;
        attrs
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|e| ErreurCapture::Camera(format!("attributs : {}", e.message())))?;

        let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut nombre = 0u32;
        MFEnumDeviceSources(&attrs, &mut activates, &mut nombre)
            .map_err(|e| ErreurCapture::Camera(format!("MFEnumDeviceSources : {}", e.message())))?;

        let mut devices = Vec::new();
        if !activates.is_null() && nombre > 0 {
            let slice = std::slice::from_raw_parts(activates, nombre as usize);
            for activate in slice.iter().flatten() {
                devices.push((
                    CameraInfo {
                        nom: attribut_chaine(activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME),
                        lien: attribut_chaine(
                            activate,
                            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                        ),
                    },
                    activate.clone(),
                ));
            }
            CoTaskMemFree(Some(activates as *const _));
        }
        Ok(devices)
    }
}

/// Attribut chaîne d'un activate (même motif que `nom_convivial` de
/// `detect.rs`) ; absent → `"(sans nom)"`, jamais de chaîne inventée.
fn attribut_chaine(activate: &IMFActivate, cle: &GUID) -> String {
    let mut buf = [0u16; 256];
    let mut len = 0u32;
    let ok = unsafe { activate.GetString(cle, &mut buf, Some(&mut len)) }.is_ok();
    if !ok || len == 0 {
        return "(sans nom)".to_string();
    }
    let len = (len as usize).min(buf.len());
    String::from_utf16_lossy(&buf[..len])
        .trim_end_matches('\0')
        .to_string()
}

/// Session de capture caméra — fil MF + slot dernière-frame.
///
/// Le fil lit les échantillons RGB32 et remplace le contenu du slot ;
/// `copier_dans` téléverse la dernière frame reçue dans la texture de
/// destination. Tout est libéré au `Drop` (drapeau d'arrêt + join : un
/// `ReadSample` bloquant rend le join borné par la cadence du capteur).
pub struct CaptureCamera {
    contexte: Arc<ContexteGpu>,
    derniere: Arc<Mutex<Option<FrameBgra>>>,
    arret: Arc<AtomicBool>,
    fil: Mutex<Option<JoinHandle<()>>>,
    /// Taille de la frame négociée (constante pour la session).
    taille: (u32, u32),
}

impl CaptureCamera {
    /// Démarre la capture sur son propre contexte GPU (adaptateur par
    /// défaut) — commodité de harnais.
    pub fn demarrer(device: Option<&str>) -> Result<Self, ErreurCapture> {
        let contexte = Arc::new(
            ContexteGpu::creer(None).map_err(|e| ErreurCapture::Gpu(e.to_string()))?,
        );
        Self::demarrer_avec(device, contexte)
    }

    /// Démarre la capture sur un contexte GPU existant (partagé avec la
    /// composition et l'encodeur → la texture cible vit sur le même device).
    ///
    /// `device` = nom convivial (`cameraDeviceId`) ; `None` ou inconnu →
    /// premier device énuméré.
    ///
    /// **Toute** la chaîne MF vit sur le fil de capture (`IMFSourceReader`
    /// n'est pas `Send`) : énumération, activation, négociation du type et
    /// lecture. `demarrer_avec` attend la taille négociée — nécessaire pour
    /// créer la texture cible — via un channel borné, puis rend la main.
    pub fn demarrer_avec(
        device: Option<&str>,
        contexte: Arc<ContexteGpu>,
    ) -> Result<Self, ErreurCapture> {
        let (tx, rx) = mpsc::channel();
        let derniere = Arc::new(Mutex::new(None));
        let arret = Arc::new(AtomicBool::new(false));
        let fil = std::thread::Builder::new()
            .name("sos-camera".to_string())
            .spawn({
                let derniere = Arc::clone(&derniere);
                let arret = Arc::clone(&arret);
                let device = device.map(str::to_string);
                move || fil_camera(device, tx, derniere, arret)
            })
            .map_err(|e| ErreurCapture::Camera(format!("thread : {e}")))?;

        // La négociation d'une caméra USB peut prendre plusieurs secondes ;
        // au-delà de 10 s le fil est arrêté proprement plutôt qu'attendu.
        let taille = match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(taille)) => taille,
            Ok(Err(m)) => {
                arret.store(true, Ordering::Relaxed);
                let _ = fil.join();
                return Err(ErreurCapture::Camera(m));
            }
            Err(_) => {
                arret.store(true, Ordering::Relaxed);
                let _ = fil.join();
                return Err(ErreurCapture::Camera(
                    "initialisation : délai dépassé".to_string(),
                ));
            }
        };

        Ok(Self {
            contexte,
            derniere,
            arret,
            fil: Mutex::new(Some(fil)),
            taille,
        })
    }

    /// Taille de la source, en pixels.
    pub fn taille(&self) -> (u32, u32) {
        self.taille
    }

    /// Contexte GPU partagé.
    pub fn contexte(&self) -> &Arc<ContexteGpu> {
        &self.contexte
    }

    /// Téléverse la dernière frame reçue dans `destination`
    /// (`UpdateSubresource`, thread appelant — jamais le fil de capture).
    ///
    /// `Ok(true)` = une frame neuve a été copiée ; `Ok(false)` = rien de
    /// neuf, la texture garde son contenu (la caméra à 30 fps cadence moins
    /// vite que la composition : la plupart des frames composent l'image
    /// précédente, comme WGC qui répète sa dernière texture).
    pub fn copier_dans(&self, destination: &ID3D11Texture2D) -> Result<bool, ErreurCapture> {
        let frame = self
            .derniere
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let Some(frame) = frame else {
            return Ok(false);
        };
        if (frame.largeur, frame.hauteur) != self.taille
            || frame.pixels.len() != frame.taille_attendue()
        {
            // Taille renégociée en cours de route : frame ignorée — la
            // texture cible est figée pour la session.
            return Ok(false);
        }
        let ressource: ID3D11Resource = destination
            .cast()
            .map_err(|e| ErreurCapture::Gpu(e.message()))?;
        unsafe {
            self.contexte.context().UpdateSubresource(
                Some(&ressource),
                0,
                None,
                frame.pixels.as_ptr() as *const _,
                frame.largeur * 4,
                0,
            )
        };
        Ok(true)
    }
}

impl Drop for CaptureCamera {
    fn drop(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
        if let Some(fil) = self.fil.lock().unwrap_or_else(|e| e.into_inner()).take() {
            // Borné par la cadence du capteur (un ReadSample en vol).
            let _ = fil.join();
        }
    }
}

/// Corps du fil de capture : prépare la chaîne MF **ici** (les objets COM
/// ne traversent pas les threads), annonce la taille négociée puis lit —
/// la dernière frame remplace l'ancienne non consommée (son tampon est
/// recyclé : zéro allocation en régime).
///
/// Sortie silencieuse : une source morte se constate par l'absence de
/// frames neuves (`copier_dans` → `Ok(false)`), jamais par une panique.
fn fil_camera(
    device: Option<String>,
    tx: mpsc::Sender<Result<(u32, u32), String>>,
    derniere: Arc<Mutex<Option<FrameBgra>>>,
    arret: Arc<AtomicBool>,
) {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    match preparer(device.as_deref()) {
        Ok((reader, taille, pas)) => {
            if tx.send(Ok(taille)).is_err() {
                return;
            }
            boucle_capture(reader, taille, pas, derniere, arret);
        }
        Err(e) => {
            let _ = tx.send(Err(e.to_string()));
        }
    }
}

/// Énumération → activation → reader RGB32 → taille et stride négociés.
///
/// Entièrement sur le fil de capture : `IMFActivate`/`IMFMediaSource`/
/// `IMFSourceReader` ne quittent jamais ce thread.
fn preparer(
    device: Option<&str>,
) -> Result<(IMFSourceReader, (u32, u32), i32), ErreurCapture> {
    crate::encode::detect::initialiser();

    let devices = enum_devices()?;
    let choix = device
        .and_then(|nom| devices.iter().find(|(info, _)| info.nom == nom))
        .or_else(|| devices.first())
        .ok_or_else(|| ErreurCapture::Camera("aucune caméra".to_string()))?;

    let source: IMFMediaSource = unsafe { choix.1.ActivateObject() }
        .map_err(|e| ErreurCapture::Camera(format!("activation : {}", e.message())))?;

    // Le processeur vidéo avancé convertit tout format capteur
    // (NV12/YUY2/MJPEG) en RGB32 — même attribut que `media.rs`.
    let mut attrs: Option<IMFAttributes> = None;
    unsafe { MFCreateAttributes(&mut attrs, 1) }
        .map_err(|e| ErreurCapture::Camera(format!("attributs : {}", e.message())))?;
    let attrs = attrs.ok_or_else(|| ErreurCapture::Camera("attributs nuls".to_string()))?;
    unsafe { attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1) }
        .map_err(|e| ErreurCapture::Camera(format!("attributs : {}", e.message())))?;

    let reader = unsafe { MFCreateSourceReaderFromMediaSource(&source, &attrs) }
        .map_err(|e| ErreurCapture::Camera(format!("source reader : {}", e.message())))?;

    let type_sortie = unsafe { MFCreateMediaType() }
        .map_err(|e| ErreurCapture::Camera(format!("type : {}", e.message())))?;
    unsafe { type_sortie.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }
        .map_err(|e| ErreurCapture::Camera(format!("major : {}", e.message())))?;
    unsafe { type_sortie.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32) }
        .map_err(|e| ErreurCapture::Camera(format!("subtype : {}", e.message())))?;
    unsafe {
        reader.SetCurrentMediaType(
            MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
            None,
            &type_sortie,
        )
    }
    .map_err(|e| ErreurCapture::Camera(format!("type RGB32 : {}", e.message())))?;

    let courant =
        unsafe { reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32) }
            .map_err(|e| ErreurCapture::Camera(format!("type courant : {}", e.message())))?;
    let pack = unsafe { courant.GetUINT64(&MF_MT_FRAME_SIZE) }
        .map_err(|e| ErreurCapture::Camera(format!("taille : {}", e.message())))?;
    let (largeur, hauteur) = ((pack >> 32) as u32, pack as u32);
    if largeur == 0 || hauteur == 0 {
        return Err(ErreurCapture::Camera("frame de taille nulle".to_string()));
    }

    // RGB32 non compressé peut sortir **bas-haut** (stride négatif,
    // convention BMP). L'attribut porte un entier signé dans un UINT32.
    let pas = unsafe { courant.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
        .map(|v| v as i32)
        .unwrap_or(largeur as i32 * 4);

    Ok((reader, (largeur, hauteur), pas))
}

/// Boucle de lecture : dépose la dernière frame BGRA dans le slot.
fn boucle_capture(
    reader: IMFSourceReader,
    taille: (u32, u32),
    pas: i32,
    derniere: Arc<Mutex<Option<FrameBgra>>>,
    arret: Arc<AtomicBool>,
) {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let ligne = taille.0 as usize * 4;
    let attendu = ligne * taille.1 as usize;
    let pitch = pas.unsigned_abs() as usize;
    while !arret.load(Ordering::Relaxed) {
        let mut flags = 0u32;
        let mut sample: Option<IMFSample> = None;
        if unsafe {
            reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                None,
                Some(&mut sample),
            )
        }
        .is_err()
            || flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0
        {
            break;
        }
        let Some(sample) = sample else {
            continue;
        };
        let Ok(buffer) = (unsafe { sample.ConvertToContiguousBuffer() }) else {
            continue;
        };
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut longueur = 0u32;
        if unsafe { buffer.Lock(&mut ptr, None, Some(&mut longueur)) }.is_err() {
            continue;
        }
        // Le tampon non consommé est repris pour réécriture (recyclage).
        let mut pixels = derniere
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|f| f.pixels)
            .unwrap_or_default();
        pixels.clear();
        if (longueur as usize) >= pitch * taille.1 as usize && pitch >= ligne {
            unsafe {
                for y in 0..taille.1 as usize {
                    // Stride négatif = image bas-haut : on retourne.
                    let sy = if pas < 0 { taille.1 as usize - 1 - y } else { y };
                    let src = std::slice::from_raw_parts(ptr.add(sy * pitch), ligne);
                    pixels.extend_from_slice(src);
                }
            }
            if pixels.len() == attendu {
                *derniere.lock().unwrap_or_else(|e| e.into_inner()) = Some(FrameBgra {
                    largeur: taille.0,
                    hauteur: taille.1,
                    pixels,
                });
            }
        }
        let _ = unsafe { buffer.Unlock() };
    }
}
