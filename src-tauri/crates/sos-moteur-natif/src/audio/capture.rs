//! Implémentation WASAPI en liaisons directes (Windows uniquement).
//!
//! Lecture par **polling** (pas d'événement) : `GetNextPacketSize` toutes les
//! 3 ms au plus, plafonné à 150 ms d'attente. Coût négligeable pour un
//! enregistreur, et aucune dépendance à `Win32_Security`/`Threading`.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};

use super::{ErreurAudio, FormatAudio, Sens};

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// GUID `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`.
const GUID_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00AA00389B71);
/// 200 ms de tampon WASAPI (unités de 100 ns).
const HNS_BUFFER: i64 = 2_000_000;
const ATTENTE: Duration = Duration::from_millis(150);
const PAS_SOMMEIL: Duration = Duration::from_millis(3);

/// Client de capture WASAPI ouvert.
pub struct Capteur {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    canaux: u16,
    taux: u32,
    flottant: bool,
}

impl Capteur {
    pub fn format(&self) -> FormatAudio {
        FormatAudio {
            taux: self.taux,
            canaux: self.canaux,
            flottant: self.flottant,
        }
    }

    /// Attend un paquet (≤ 150 ms) puis draine tout ce qui est disponible.
    pub fn lire(&mut self, sortie: &mut Vec<f32>) -> Result<usize, ErreurAudio> {
        let debut = Instant::now();
        loop {
            let lus = self.drainer(sortie)?;
            if lus > 0 {
                return Ok(lus);
            }
            if debut.elapsed() >= ATTENTE {
                return Ok(0);
            }
            std::thread::sleep(PAS_SOMMEIL);
        }
    }

    fn drainer(&mut self, sortie: &mut Vec<f32>) -> Result<usize, ErreurAudio> {
        let mut total = 0usize;
        loop {
            let taille = unsafe {
                self.capture
                    .GetNextPacketSize()
                    .map_err(|e| ErreurAudio::Lecture(format!("GetNextPacketSize : {}", e.message())))?
            };
            if taille == 0 {
                return Ok(total);
            }

            let mut donnees: *mut u8 = null_mut();
            let mut frames = 0u32;
            let mut drapeaux = 0u32;
            unsafe {
                self.capture
                    .GetBuffer(&mut donnees, &mut frames, &mut drapeaux, None, None)
                    .map_err(|e| ErreurAudio::Lecture(format!("GetBuffer : {}", e.message())))?;
            }

            let echantillons = frames as usize * self.canaux as usize;
            if drapeaux & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                sortie.resize(sortie.len() + echantillons, 0.0);
            } else if self.flottant {
                let s = unsafe { std::slice::from_raw_parts(donnees as *const f32, echantillons) };
                sortie.extend_from_slice(s);
            } else {
                let s = unsafe { std::slice::from_raw_parts(donnees as *const i16, echantillons) };
                sortie.extend(s.iter().map(|&v| v as f32 / 32768.0));
            }

            unsafe {
                self.capture.ReleaseBuffer(frames).map_err(|e| {
                    ErreurAudio::Lecture(format!("ReleaseBuffer : {}", e.message()))
                })?;
            }
            total += echantillons;
        }
    }

    pub fn arreter(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

impl Drop for Capteur {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

/// Ouvre le périphérique par défaut dans le sens demandé.
///
/// `Sortie` = loopback (flag `AUDCLNT_STREAMFLAGS_LOOPBACK` sur le device de
/// rendu) ; `Entree` = capture directe du micro.
pub fn ouvrir(sens: Sens) -> Result<Capteur, ErreurAudio> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerateur: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| ErreurAudio::Init(format!("MMDeviceEnumerator : {}", e.message())))?;
        let flux = if sens == Sens::Sortie { eRender } else { eCapture };
        let device = enumerateur
            .GetDefaultAudioEndpoint(flux, eConsole)
            .map_err(|e| ErreurAudio::Init(format!("GetDefaultAudioEndpoint : {}", e.message())))?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| ErreurAudio::Init(format!("Activate IAudioClient : {}", e.message())))?;

        let brut = client
            .GetMixFormat()
            .map_err(|e| ErreurAudio::Init(format!("GetMixFormat : {}", e.message())))?;
        let fmt: WAVEFORMATEX = *brut;
        let flottant = match fmt.wFormatTag {
            WAVE_FORMAT_IEEE_FLOAT => true,
            WAVE_FORMAT_PCM => false,
            WAVE_FORMAT_EXTENSIBLE => {
                // `WAVEFORMATEXTENSIBLE` est packed(1) : lecture non alignée.
                let ext = brut as *const WAVEFORMATEXTENSIBLE;
                let sous_format = std::ptr::addr_of!((*ext).SubFormat).read_unaligned();
                sous_format == GUID_FLOAT
            }
            autre => {
                CoTaskMemFree(Some(brut as *const c_void));
                return Err(ErreurAudio::Format(format!("format {autre} non supporte")));
            }
        };
        let bits = fmt.wBitsPerSample;
        let bits_attendus = if flottant { 32 } else { 16 };
        if bits != bits_attendus {
            CoTaskMemFree(Some(brut as *const c_void));
            return Err(ErreurAudio::Format(format!(
                "{bits} bits, {bits_attendus} attendus"
            )));
        }

        let mut drapeaux = 0u32;
        if sens == Sens::Sortie {
            drapeaux |= AUDCLNT_STREAMFLAGS_LOOPBACK;
        }
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, drapeaux, HNS_BUFFER, 0, brut, None);
        CoTaskMemFree(Some(brut as *const c_void));
        init.map_err(|e| ErreurAudio::Init(format!("Initialize : {}", e.message())))?;

        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|e| ErreurAudio::Init(format!("GetService : {}", e.message())))?;
        client
            .Start()
            .map_err(|e| ErreurAudio::Init(format!("Start : {}", e.message())))?;

        Ok(Capteur {
            client,
            capture,
            canaux: fmt.nChannels,
            taux: fmt.nSamplesPerSec,
            flottant,
        })
    }
}
