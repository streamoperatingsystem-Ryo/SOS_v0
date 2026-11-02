//! Implémentation WASAPI en liaisons directes (Windows uniquement).
//!
//! Lecture par **polling** (pas d'événement) : `GetNextPacketSize` toutes les
//! 3 ms au plus, plafonné à 150 ms d'attente. Coût négligeable pour un
//! enregistreur, et aucune dépendance à `Win32_Security`/`Threading`.
//!
//! Deux services en plus de la capture :
//! - **énumération** des périphériques actifs (nom convivial + ID stable) ;
//! - **mesure de niveau** pré-fader, calculée sur les échantillons bruts.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::GUID;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, EDataFlow, IAudioCaptureClient, IAudioClient, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE_ACTIVE, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

use super::{ErreurAudio, FormatAudio, NiveauxPartages, Sens};

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// GUID `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`.
const GUID_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00AA00389B71);
/// 200 ms de tampon WASAPI (unités de 100 ns).
const HNS_BUFFER: i64 = 2_000_000;
const ATTENTE: Duration = Duration::from_millis(150);
const PAS_SOMMEIL: Duration = Duration::from_millis(3);

/// Un périphérique audio actif, tel que présenté dans le mixeur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheriqueAudio {
    /// ID `IMMDevice` — stable tant que le périphérique est installé.
    pub id: String,
    /// Nom convivial (« Haut-parleurs (Realtek…) »).
    pub nom: String,
    /// Périphérique par défaut du système pour ce sens.
    pub defaut: bool,
}

/// Périphérique réellement ouvert (avec repli explicite éventuel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppareilOuvert {
    pub id: String,
    pub nom: String,
    /// `true` = l'ID demandé n'existait plus, le défaut a été utilisé.
    pub repli: bool,
}

/// Client de capture WASAPI ouvert.
pub struct Capteur {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    canaux: u16,
    taux: u32,
    flottant: bool,
    niveaux: Arc<NiveauxPartages>,
}

impl Capteur {
    pub fn format(&self) -> FormatAudio {
        FormatAudio {
            taux: self.taux,
            canaux: self.canaux,
            flottant: self.flottant,
        }
    }

    /// Compteurs de niveau de cette source (pré-fader).
    pub fn niveaux(&self) -> Arc<NiveauxPartages> {
        Arc::clone(&self.niveaux)
    }

    /// Attend un paquet (≤ 150 ms) puis draine tout ce qui est disponible.
    pub fn lire(&mut self, sortie: &mut Vec<f32>) -> Result<usize, ErreurAudio> {
        let debut = Instant::now();
        loop {
            let lus = self.drainer(sortie)?;
            if lus > 0 {
                // Les `lus` derniers échantillons sont les nouveaux.
                let depuis = sortie.len() - lus;
                self.niveaux.maj(&sortie[depuis..], self.canaux, self.taux);
                return Ok(lus);
            }
            if debut.elapsed() >= ATTENTE {
                // Périphérique muet : le vu-mètre décroît, il ne gèle pas.
                self.niveaux.decroitre(debut.elapsed().as_secs_f32(), self.taux);
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

/// Sens → flux WASAPI.
fn flux(sens: Sens) -> EDataFlow {
    if sens == Sens::Sortie {
        eRender
    } else {
        eCapture
    }
}

/// ID `IMMDevice` (chaîne à libérer par `CoTaskMemFree`).
fn id_de(device: &IMMDevice) -> Option<String> {
    unsafe {
        let pointeur = device.GetId().ok()?;
        let texte = pointeur.to_string().ok();
        CoTaskMemFree(Some(pointeur.0 as *const c_void));
        texte
    }
}

/// Nom convivial (`PKEY_Device_FriendlyName`) — `None` si le store refuse.
fn nom_convivial(device: &IMMDevice) -> Option<String> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let valeur = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let nom = valeur.to_string();
        (!nom.trim().is_empty()).then_some(nom)
    }
}

/// ID du périphérique par défaut pour ce flux (s'il existe).
fn defaut_id(enumerateur: &IMMDeviceEnumerator, flux: EDataFlow) -> Option<String> {
    unsafe {
        enumerateur
            .GetDefaultAudioEndpoint(flux, eConsole)
            .ok()
            .and_then(|device| id_de(&device))
    }
}

/// Cherche un périphérique actif par ID.
fn trouver(
    enumerateur: &IMMDeviceEnumerator,
    flux: EDataFlow,
    id: &str,
) -> Result<Option<IMMDevice>, ErreurAudio> {
    unsafe {
        let collection = enumerateur
            .EnumAudioEndpoints(flux, DEVICE_STATE_ACTIVE)
            .map_err(|e| ErreurAudio::Init(format!("EnumAudioEndpoints : {}", e.message())))?;
        let total = collection
            .GetCount()
            .map_err(|e| ErreurAudio::Init(format!("GetCount : {}", e.message())))?;
        for index in 0..total {
            let Ok(device) = collection.Item(index) else {
                continue;
            };
            if id_de(&device).as_deref() == Some(id) {
                return Ok(Some(device));
            }
        }
        Ok(None)
    }
}

/// Liste les périphériques **actifs** d'un sens : défaut en tête, puis
/// ordre alphabétique (insensible à la casse).
pub fn lister(sens: Sens) -> Result<Vec<PeripheriqueAudio>, ErreurAudio> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerateur: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| ErreurAudio::Init(format!("MMDeviceEnumerator : {}", e.message())))?;
        let flux = flux(sens);
        let defaut = defaut_id(&enumerateur, flux);
        let collection = enumerateur
            .EnumAudioEndpoints(flux, DEVICE_STATE_ACTIVE)
            .map_err(|e| ErreurAudio::Init(format!("EnumAudioEndpoints : {}", e.message())))?;
        let total = collection
            .GetCount()
            .map_err(|e| ErreurAudio::Init(format!("GetCount : {}", e.message())))?;
        let mut liste = Vec::with_capacity(total as usize);
        for index in 0..total {
            let Ok(device) = collection.Item(index) else {
                continue;
            };
            let Some(id) = id_de(&device) else {
                continue;
            };
            let nom = nom_convivial(&device).unwrap_or_else(|| id.clone());
            liste.push(PeripheriqueAudio {
                defaut: defaut.as_deref() == Some(id.as_str()),
                id,
                nom,
            });
        }
        liste.sort_by(|a, b| {
            b.defaut
                .cmp(&a.defaut)
                .then_with(|| a.nom.to_lowercase().cmp(&b.nom.to_lowercase()))
        });
        Ok(liste)
    }
}

/// Ouvre un périphérique précis (`None` = défaut) dans le sens demandé.
///
/// Un ID inconnu (périphérique débranché) **replie sur le défaut** et le
/// signale (`AppareilOuvert::repli`) : l'enregistrement continue, l'UI peut
/// afficher l'avertissement au lieu d'une piste muette silencieuse.
pub fn ouvrir(
    sens: Sens,
    peripherique: Option<&str>,
    niveaux: Arc<NiveauxPartages>,
) -> Result<(Capteur, AppareilOuvert), ErreurAudio> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerateur: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| ErreurAudio::Init(format!("MMDeviceEnumerator : {}", e.message())))?;
        let flux = flux(sens);
        let mut repli = false;
        let device = match peripherique.map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => match trouver(&enumerateur, flux, id)? {
                Some(device) => device,
                None => {
                    repli = true;
                    enumerateur
                        .GetDefaultAudioEndpoint(flux, eConsole)
                        .map_err(|e| {
                            ErreurAudio::Init(format!(
                                "périphérique « {id} » introuvable et défaut indisponible : {}",
                                e.message()
                            ))
                        })?
                }
            },
            None => enumerateur
                .GetDefaultAudioEndpoint(flux, eConsole)
                .map_err(|e| {
                    ErreurAudio::Init(format!("GetDefaultAudioEndpoint : {}", e.message()))
                })?,
        };
        let id = id_de(&device).unwrap_or_default();
        let nom = nom_convivial(&device).unwrap_or_else(|| id.clone());

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

        Ok((
            Capteur {
                client,
                capture,
                canaux: fmt.nChannels,
                taux: fmt.nSamplesPerSec,
                flottant,
                niveaux,
            },
            AppareilOuvert { id, nom, repli },
        ))
    }
}
