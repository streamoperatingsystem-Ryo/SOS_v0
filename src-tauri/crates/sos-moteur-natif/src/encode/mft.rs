//! Encodeur Media Foundation (MFT) — matériel **et** logiciel.
//!
//! Deux modèles de pilotage coexistent :
//! - **asynchrone** (MFT matériels NVENC/QSV/AMF) : `MF_TRANSFORM_ASYNC_UNLOCK`
//!   puis événements `METransformNeedInput` / `METransformHaveOutput` ;
//! - **synchrone** (MFT logiciel Microsoft) : `ProcessInput` + `ProcessOutput`.
//!
//! Le mode est choisi d'après l'attribut `MF_TRANSFORM_ASYNC` de l'encodeur —
//! jamais d'après son nom.
//!
//! La conversion BGRA→NV12 est encore en CPU (`crate::color`) pour ce spike ;
//! le lot 0.2d la remplacera par le video processor GPU et fournira le
//! gestionnaire D3D11 aux MFT matériels.

use std::mem::ManuallyDrop;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::IDXGIAdapter;
use windows::Win32::Media::MediaFoundation::{
    eAVEncCommonRateControlMode_CBR, IMFActivate, IMFAttributes, IMFDXGIDeviceManager,
    IMFMediaBuffer, IMFMediaEvent, IMFMediaEventGenerator, IMFMediaType, IMFSample, IMFTransform,
    MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS, METransformDrainComplete, METransformHaveOutput,
    METransformNeedInput, MFMediaType_Video, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MF_EVENT_FLAG_NO_WAIT, MF_E_NO_EVENTS_AVAILABLE,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_LOW_LATENCY, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE,
    MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, CODECAPI_AVEncCommonQuality,
    CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVGOPSize,
};

use super::detect;
use super::{Codec, ErreurEncodage, PaquetEncode};

/// Délai maximal d'attente d'un événement MFT (garde-fou anti-blocage).
const DELAI_EVENEMENT: Duration = Duration::from_secs(2);

/// Encodeur MFT prêt à recevoir des frames NV12.
pub struct EncodeurMft {
    transform: IMFTransform,
    events: Option<IMFMediaEventGenerator>,
    asynchrone: bool,
    largeur: u32,
    hauteur: u32,
    duree_us: i64,
    taille_nv12: usize,
    /// Taille/particularités du flux de sortie (sample fourni ou non).
    info_sortie: windows::Win32::Media::MediaFoundation::MFT_OUTPUT_STREAM_INFO,
    /// Device D3D11 fourni aux MFT « D3D11-aware » (gardé vivant).
    _device: Option<ID3D11Device>,
    /// Gestionnaire DXGI associé (gardé vivant).
    _manager: Option<IMFDXGIDeviceManager>,
    /// Frame en attente d'un événement `NeedInput` (mode asynchrone).
    en_attente: Option<(Vec<u8>, i64)>,
}

impl EncodeurMft {
    /// Active et configure un encodeur.
    ///
    /// `largeur`/`hauteur` sont ramenées à des valeurs paires (exigence NV12).
    pub fn nouveau(
        activate: &IMFActivate,
        codec: Codec,
        largeur: u32,
        hauteur: u32,
        fps: u32,
        debit_kbps: u32,
    ) -> Result<Self, ErreurEncodage> {
        detect::initialiser();

        let largeur = largeur & !1;
        let hauteur = hauteur & !1;
        if largeur == 0 || hauteur == 0 || fps == 0 {
            return Err(ErreurEncodage::Conversion(
                "dimensions ou fps invalides".into(),
            ));
        }

        let transform: IMFTransform = unsafe { activate.ActivateObject() }
            .map_err(|e| ErreurEncodage::Mft(format!("ActivateObject : {}", e.message())))?;

        let attrs: IMFAttributes = unsafe { transform.GetAttributes() }
            .map_err(|e| ErreurEncodage::Mft(format!("GetAttributes : {}", e.message())))?;

        // Détection du modèle : attribut MF_TRANSFORM_ASYNC, jamais le nom.
        let asynchrone = unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }
            .map(|v| v != 0)
            .unwrap_or(false);

        if asynchrone {
            unsafe { attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
                .map_err(|e| ErreurEncodage::Mft(format!("AsyncUnlock : {}", e.message())))?;
        }

        // MFT « D3D11-aware » (NVENC/QSV/AMF) : sans gestionnaire de device,
        // ProcessOutput échoue en E_FAIL. Le device est créé sur l'adaptateur
        // par défaut ; le lot 0.2d le remplacera par celui de la capture.
        let d3d11_aware = unsafe { attrs.GetUINT32(&MF_SA_D3D11_AWARE) }
            .map(|v| v != 0)
            .unwrap_or(false);

        let (device, manager) = if d3d11_aware {
            let device = creer_device_d3d11()?;
            let mut token = 0u32;
            let mut manager: Option<IMFDXGIDeviceManager> = None;
            unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager) }.map_err(|e| {
                ErreurEncodage::Mft(format!("MFCreateDXGIDeviceManager : {}", e.message()))
            })?;
            let manager = manager.ok_or_else(|| {
                ErreurEncodage::Mft("gestionnaire DXGI nul".to_string())
            })?;
            unsafe { manager.ResetDevice(&device, token) }.map_err(|e| {
                ErreurEncodage::Mft(format!("ResetDevice : {}", e.message()))
            })?;
            unsafe {
                transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
            }
            .map_err(|e| {
                ErreurEncodage::Mft(format!("SetD3DManager : {}", e.message()))
            })?;
            (Some(device), Some(manager))
        } else {
            (None, None)
        };

        // --- Type de SORTIE (d'abord : recommandation Media Foundation) ---
        let sortie = unsafe { MFCreateMediaType() }
            .map_err(|e| ErreurEncodage::Mft(format!("MFCreateMediaType : {}", e.message())))?;
        definir_guid(&sortie, &MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        definir_guid(&sortie, &MF_MT_SUBTYPE, &detect::subtype(codec))?;
        definir_u32(&sortie, &MF_MT_AVG_BITRATE, debit_kbps.saturating_mul(1000))?;
        definir_u32(
            &sortie,
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_Progressive.0 as u32,
        )?;
        definir_u64(&sortie, &MF_MT_FRAME_SIZE, pack2(largeur, hauteur))?;
        definir_u64(&sortie, &MF_MT_FRAME_RATE, pack2(fps, 1))?;
        if codec == Codec::H264 {
            definir_u32(&sortie, &MF_MT_MPEG2_PROFILE, detect::profil_h264())?;
        }
        unsafe { transform.SetOutputType(0, &sortie, 0) }.map_err(|e| {
            ErreurEncodage::Mft(format!("SetOutputType {} : {}", codec.nom(), e.message()))
        })?;

        // --- Type d'ENTRÉE : NV12 ---
        let entree = unsafe { MFCreateMediaType() }
            .map_err(|e| ErreurEncodage::Mft(format!("MFCreateMediaType : {}", e.message())))?;
        definir_guid(&entree, &MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        definir_guid(&entree, &MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        definir_u64(&entree, &MF_MT_FRAME_SIZE, pack2(largeur, hauteur))?;
        definir_u64(&entree, &MF_MT_FRAME_RATE, pack2(fps, 1))?;
        definir_u32(
            &entree,
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_Progressive.0 as u32,
        )?;
        unsafe { transform.SetInputType(0, &entree, 0) }
            .map_err(|e| ErreurEncodage::Mft(format!("SetInputType NV12 : {}", e.message())))?;

        // --- Réglages de codec (best effort : certains pilotes les ignorent) ---
        let _ = unsafe {
            attrs.SetUINT32(
                &CODECAPI_AVEncCommonRateControlMode,
                eAVEncCommonRateControlMode_CBR.0 as u32,
            )
        };
        let _ = unsafe { attrs.SetUINT32(&CODECAPI_AVEncMPVGOPSize, fps.saturating_mul(2)) };
        let _ = unsafe { attrs.SetUINT32(&CODECAPI_AVEncCommonQuality, 70) };
        let _ = unsafe { attrs.SetUINT32(&MF_LOW_LATENCY, 1) };

        // Particularités du flux de sortie : certains MFT (logiciels) exigent
        // que le CLIENT fournisse le sample de sortie.
        let info_sortie = unsafe { transform.GetOutputStreamInfo(0) }
            .map_err(|e| ErreurEncodage::Mft(format!("GetOutputStreamInfo : {}", e.message())))?;

        let events = if asynchrone {            let gen: IMFMediaEventGenerator = transform.cast().map_err(|e| {
                ErreurEncodage::Mft(format!("IMFMediaEventGenerator : {}", e.message()))
            })?;
            unsafe { transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) }
                .map_err(|e| ErreurEncodage::Mft(format!("Flush : {}", e.message())))?;
            unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) }
                .map_err(|e| ErreurEncodage::Mft(format!("BeginStreaming : {}", e.message())))?;
            unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0) }
                .map_err(|e| ErreurEncodage::Mft(format!("StartOfStream : {}", e.message())))?;
            Some(gen)
        } else {
            None
        };

        Ok(Self {
            transform,
            events,
            asynchrone,
            largeur,
            hauteur,
            duree_us: 1_000_000 / fps as i64,
            taille_nv12: (largeur as usize * hauteur as usize * 3) / 2,
            info_sortie,
            _device: device,
            _manager: manager,
            en_attente: None,
        })
    }

    pub fn largeur(&self) -> u32 {
        self.largeur
    }

    pub fn hauteur(&self) -> u32 {
        self.hauteur
    }

    pub fn asynchrone(&self) -> bool {
        self.asynchrone
    }

    /// Taille attendue d'une frame NV12 (Y + UV entrelacé).
    pub fn taille_frame(&self) -> usize {
        self.taille_nv12
    }

    /// Encode une frame NV12 et renvoie les paquets produits.
    pub fn encoder(
        &mut self,
        nv12: &[u8],
        pts_us: i64,
    ) -> Result<Vec<PaquetEncode>, ErreurEncodage> {
        if nv12.len() != self.taille_nv12 {
            return Err(ErreurEncodage::Conversion(format!(
                "frame {} octets, {} attendus",
                nv12.len(),
                self.taille_nv12
            )));
        }

        if self.asynchrone {
            if self.en_attente.is_some() {
                return Err(ErreurEncodage::Mft(
                    "une frame est déjà en attente de l'encodeur".into(),
                ));
            }
            self.en_attente = Some((nv12.to_vec(), pts_us));
            return self.pomper(false);
        }

        // Mode synchrone : vider d'abord, puis envoyer.
        let _ = self.lire_sorties()?;
        self.envoyer(nv12, pts_us)?;
        self.lire_sorties()
    }

    /// Signale la fin de flux et récupère les derniers paquets.
    pub fn terminer(&mut self) -> Result<Vec<PaquetEncode>, ErreurEncodage> {
        unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0) }
            .map_err(|e| ErreurEncodage::Mft(format!("Drain : {}", e.message())))?;

        if self.asynchrone {
            self.pomper(true)
        } else {
            self.lire_sorties()
        }
    }

    /// Pompe les événements MFT (mode asynchrone uniquement).
    fn pomper(&mut self, drain: bool) -> Result<Vec<PaquetEncode>, ErreurEncodage> {
        let mut paquets = Vec::new();
        let mut envoye = false;
        let deadline = Instant::now() + DELAI_EVENEMENT;

        loop {
            let flags = if envoye && !paquets.is_empty() {
                MF_EVENT_FLAG_NO_WAIT
            } else {
                MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0)
            };

            let evenement: IMFMediaEvent = match unsafe { self.events().GetEvent(flags) } {
                Ok(e) => e,
                Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => {
                    if envoye && !paquets.is_empty() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(e) => {
                    return Err(ErreurEncodage::Mft(format!(
                        "GetEvent : {}",
                        e.message()
                    )))
                }
            };

            let type_ev = unsafe { evenement.GetType() }
                .map_err(|e| ErreurEncodage::Mft(format!("GetType : {}", e.message())))?;

            if type_ev == METransformNeedInput.0 as u32 {
                match self.en_attente.take() {
                    Some((data, pts)) => {
                        self.envoyer(&data, pts)?;
                        envoye = true;
                    }
                    None => break,
                }
            } else if type_ev == METransformHaveOutput.0 as u32 {
                paquets.extend(self.lire_sorties()?);
            } else if type_ev == METransformDrainComplete.0 as u32 {
                break;
            }

            if drain && paquets.is_empty() && Instant::now() >= deadline {
                break;
            }
        }

        Ok(paquets)
    }

    fn events(&self) -> &IMFMediaEventGenerator {
        self.events
            .as_ref()
            .expect("pomper() n'est appelé qu'en mode asynchrone")
    }

    fn envoyer(&mut self, nv12: &[u8], pts_us: i64) -> Result<(), ErreurEncodage> {
        let sample = unsafe { MFCreateSample() }
            .map_err(|e| ErreurEncodage::Mft(format!("MFCreateSample : {}", e.message())))?;
        let buffer = unsafe { MFCreateMemoryBuffer(self.taille_nv12 as u32) }
            .map_err(|e| ErreurEncodage::Mft(format!("MFCreateMemoryBuffer : {}", e.message())))?;

        let mut ptr: *mut u8 = null_mut();
        unsafe { buffer.Lock(&mut ptr, None, None) }
            .map_err(|e| ErreurEncodage::Mft(format!("Lock : {}", e.message())))?;
        unsafe {
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), ptr, self.taille_nv12);
        }
        unsafe { buffer.Unlock() }
            .map_err(|e| ErreurEncodage::Mft(format!("Unlock : {}", e.message())))?;
        unsafe { buffer.SetCurrentLength(self.taille_nv12 as u32) }
            .map_err(|e| ErreurEncodage::Mft(format!("SetCurrentLength : {}", e.message())))?;

        unsafe { sample.AddBuffer(&buffer) }
            .map_err(|e| ErreurEncodage::Mft(format!("AddBuffer : {}", e.message())))?;
        unsafe { sample.SetSampleTime(pts_us) }
            .map_err(|e| ErreurEncodage::Mft(format!("SetSampleTime : {}", e.message())))?;
        unsafe { sample.SetSampleDuration(self.duree_us) }
            .map_err(|e| ErreurEncodage::Mft(format!("SetSampleDuration : {}", e.message())))?;

        unsafe { self.transform.ProcessInput(0, &sample, 0) }
            .map_err(|e| ErreurEncodage::Mft(format!("ProcessInput : {}", e.message())))
    }

    fn lire_sorties(&mut self) -> Result<Vec<PaquetEncode>, ErreurEncodage> {
        let mut paquets = Vec::new();

        loop {
            // Certains MFT (logiciels) n'allouent pas le sample de sortie :
            // c'est au client de le fournir.
            let fournit_samples =
                self.info_sortie.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
            let sample_client = if fournit_samples {
                None
            } else {
                let sample = unsafe { MFCreateSample() }
                    .map_err(|e| ErreurEncodage::Mft(format!("MFCreateSample sortie : {}", e.message())))?;
                let taille = self.info_sortie.cbSize.max(4 * 1024 * 1024);
                let buffer = unsafe { MFCreateMemoryBuffer(taille) }.map_err(|e| {
                    ErreurEncodage::Mft(format!("MFCreateMemoryBuffer sortie : {}", e.message()))
                })?;
                unsafe { sample.AddBuffer(&buffer) }.map_err(|e| {
                    ErreurEncodage::Mft(format!("AddBuffer sortie : {}", e.message()))
                })?;
                Some(sample)
            };

            let mut sortie = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(sample_client),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut statut = 0u32;

            let resultat = unsafe {
                self.transform
                    .ProcessOutput(0, std::slice::from_mut(&mut sortie), &mut statut)
            };

            match resultat {
                Ok(()) => {
                    let sample = unsafe { ManuallyDrop::take(&mut sortie.pSample) };
                    let events = unsafe { ManuallyDrop::take(&mut sortie.pEvents) };
                    drop(events);

                    match sample {
                        Some(s) => paquets.push(lire_sample(&s)?),
                        None => break,
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                Err(e) => {
                    return Err(ErreurEncodage::Mft(format!(
                        "ProcessOutput : {}",
                        e.message()
                    )))
                }
            }
        }

        Ok(paquets)
    }
}

fn definir_guid(
    type_media: &IMFMediaType,
    cle: &windows::core::GUID,
    valeur: &windows::core::GUID,
) -> Result<(), ErreurEncodage> {
    unsafe { type_media.SetGUID(cle, valeur) }
        .map_err(|e| ErreurEncodage::Mft(format!("SetGUID : {}", e.message())))
}

fn definir_u32(
    type_media: &IMFMediaType,
    cle: &windows::core::GUID,
    valeur: u32,
) -> Result<(), ErreurEncodage> {
    unsafe { type_media.SetUINT32(cle, valeur) }
        .map_err(|e| ErreurEncodage::Mft(format!("SetUINT32 : {}", e.message())))
}

fn definir_u64(
    type_media: &IMFMediaType,
    cle: &windows::core::GUID,
    valeur: u64,
) -> Result<(), ErreurEncodage> {
    unsafe { type_media.SetUINT64(cle, valeur) }
        .map_err(|e| ErreurEncodage::Mft(format!("SetUINT64 : {}", e.message())))
}

/// Empaquette deux u32 en un u64 (convention MF_MT_FRAME_SIZE / _RATE).
fn pack2(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | b as u64
}

/// Crée un device D3D11 BGRA (FL 11_0) sur l'adaptateur par défaut.
///
/// Sert uniquement à alimenter les MFT « D3D11-aware ». Le lot 0.2d
/// réutilisera le device de la capture pour rester zéro-copie.
fn creer_device_d3d11() -> Result<ID3D11Device, ErreurEncodage> {
    let mut device: Option<ID3D11Device> = None;
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            D3D_DRIVER_TYPE_HARDWARE,
            None::<&windows::Win32::Foundation::HMODULE>,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    }
    .map_err(|e| ErreurEncodage::Gpu(format!("D3D11CreateDevice : {}", e.message())))?;

    device.ok_or_else(|| ErreurEncodage::Gpu("device D3D11 nul".to_string()))
}

fn lire_sample(sample: &IMFSample) -> Result<PaquetEncode, ErreurEncodage> {
    let buffer: IMFMediaBuffer = unsafe { sample.ConvertToContiguousBuffer() }
        .map_err(|e| ErreurEncodage::Mft(format!("ConvertToContiguousBuffer : {}", e.message())))?;

    let mut ptr: *mut u8 = null_mut();
    let mut longueur = 0u32;
    unsafe { buffer.Lock(&mut ptr, None, Some(&mut longueur)) }
        .map_err(|e| ErreurEncodage::Mft(format!("Lock sortie : {}", e.message())))?;

    let data = unsafe { std::slice::from_raw_parts(ptr, longueur as usize) }.to_vec();
    unsafe { buffer.Unlock() }
        .map_err(|e| ErreurEncodage::Mft(format!("Unlock sortie : {}", e.message())))?;

    let pts_us = unsafe { sample.GetSampleTime() }.unwrap_or(0);
    let cle = contient_idr(&data);

    Ok(PaquetEncode { data, pts_us, cle })
}

/// Détecte une image-clé (NAL type 5) dans un flux Annex B.
fn contient_idr(data: &[u8]) -> bool {
    let mut i = 0;
    while i + 4 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            if data[i + 3] & 0x1F == 5 {
                return true;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    false
}
