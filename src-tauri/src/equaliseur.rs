/// Module « Équaliseur » — spectre audio système → overlay néon+bulles en diffusion.
///
/// Le spectre n'est PAS mesuré dans OBS (obs-websocket n'expose aucun niveau
/// audio) : il vient de la capture WASAPI Windows —
///   - LOOPBACK du périphérique de SORTIE par défaut (= ce qu'entend le
///     streamer, c'est-à-dire la « Desktop Audio » capturée par OBS) ;
///   - \+ périphérique d'ENTRÉE par défaut (la voix micro — absente du
///     loopback sauf monitoring OBS activé). Option `eqMicro` du module.
///
/// Les deux spectres sont fusionnés par bande (max).
///
/// Flux :
///   scene.modules.equaliseur.actif → sync() (appelée par scenes::push_snapshot,
///   point unique des mutations de scène, + sync initial au boot dans lib.rs)
///   → threads capture (un par device) → FFT 2048 → 24 bandes log normalisées
///   → frame `{"type":"eq-spectrum","bandes":[24 floats 0..1]}` sur chat_tx
///   → serveur :4321 → diffusion.html (couche #eq-root sous les widgets).
///
/// Zéro thread quand le module est inactif (invariant input_viewer.rs) :
/// sync() démarre/arrête/reconfigure la capture. Hors Windows : stubs no-op.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use tokio::sync::broadcast;

use crate::scene::ModuleHud;

/// Nombre de bandes publiées — répartition logarithmique sur le spectre audible.
const NB_BANDES: usize = 24;
/// Taille FFT (fenêtre d'analyse) — 2048 @ 48 kHz ≈ 23 Hz/bin.
const FFT_N: usize = 2048;
/// Cadence de publication des frames eq-spectrum (~30 Hz — droppable).
const PUBLICATION_MS: u128 = 33;
/// Fenêtre spectrale analysée (Hz).
const F_MIN: f32 = 32.0;
const F_MAX: f32 = 16000.0;
/// Plancher dynamique : -55 dBFS → norm. 0 ; 0 dBFS → norm. 1.
const DB_MIN: f32 = -55.0;
/// Timeout d'attente d'un paquet audio (ms) — cadence aussi le check cancel.
const WAIT_MS: u32 = 150;
/// Taux d'échantillonnage demandé (autoconvert WASAPI si le device diffère).
const SAMPLE_RATE: usize = 48000;
/// Canaux demandés (stéréo f32 → mixé en mono).
const CANAUX: usize = 2;

/// État du module équaliseur — champ de `ScenesState` (pas app.manage : l'état
/// vit avec le canal, comme les autres états de push le long de chat_tx).
#[derive(Clone)]
pub struct EqualiseurState {
    inner: Arc<Mutex<Inner>>,
    chat_tx: broadcast::Sender<String>,
}

impl EqualiseurState {
    pub fn new(chat_tx: broadcast::Sender<String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            chat_tx,
        }
    }

    /// Réconcilie la capture avec la config du module (extraite de la scène) :
    /// démarre si `actif`, arrête sinon, redémarre si `eqMicro` a changé.
    /// `None` = module jamais configuré → micro ON (défaut « tout »).
    /// Appelée par push_snapshot — no-op si rien n'a changé.
    pub fn sync(&self, module: Option<&ModuleHud>) {
        let voulu = match module {
            Some(m) if m.actif => Some(m.eqMicro != Some(false)),
            _ => None,
        };
        let mut g = self.inner.lock().unwrap();
        if voulu == g.voulu {
            return;
        }
        // Drop du moteur → cancel (les threads sortent seuls, non joints —
        // jamais de blocage dans le chemin de commit).
        g.moteur = None;
        if let Some(micro) = voulu {
            g.moteur = Some(Moteur::demarrer(&self.chat_tx, micro));
        }
        g.voulu = voulu;
    }
}

#[derive(Default)]
struct Inner {
    moteur: Option<Moteur>,
    /// Config courante appliquée : Some(micro) = capture démarrée, None =
    /// arrêtée. Sert au dedup de sync().
    voulu: Option<bool>,
}

/// État partagé entre les threads de capture.
struct Partage {
    cancel: AtomicBool,
    /// Dernières bandes du micro — lues/fusionnées par le thread sortie.
    bandes_micro: Mutex<[f32; NB_BANDES]>,
}

/// Les threads de capture en cours. Drop → cancel ; les JoinHandles sont
/// conservés pour la durée de vie puis détachés (les threads sortent en
/// ~WAIT_MS max sur le prochain wait_for_event).
struct Moteur {
    partage: Arc<Partage>,
    _handles: Vec<JoinHandle<()>>,
}

impl Drop for Moteur {
    fn drop(&mut self) {
        self.partage.cancel.store(true, Ordering::Relaxed);
    }
}

impl Moteur {
    fn demarrer(chat_tx: &broadcast::Sender<String>, micro: bool) -> Self {
        let partage = Arc::new(Partage {
            cancel: AtomicBool::new(false),
            bandes_micro: Mutex::new([0.0; NB_BANDES]),
        });
        let mut handles = Vec::with_capacity(2);
        {
            let p = partage.clone();
            let tx = chat_tx.clone();
            handles.push(thread::spawn(move || capture::boucle_sortie(p, tx)));
        }
        if micro {
            let p = partage.clone();
            handles.push(thread::spawn(move || capture::boucle_micro(p)));
        }
        log::info!("[Equaliseur] capture démarrée (micro: {})", micro);
        Self {
            partage,
            _handles: handles,
        }
    }
}

// =============================================================================
// Capture WASAPI — Windows uniquement (loopback + capture directe).
// =============================================================================

#[cfg(windows)]
mod capture {
    use super::*;
    use rustfft::num_complex::Complex32;
    use rustfft::{Fft, FftPlanner};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};
    use wasapi::*;

    /// Thread sortie (loopback render) — c'est aussi l'éditeur des frames
    /// eq-spectrum : il fusionne son spectre avec celui du micro (slot
    /// partagé, max par bande) et publie à ~30 Hz.
    /// Échec d'init = log + sortie immédiate (le module reste à plat, jamais
    /// de panic dans le chemin de commit).
    pub fn boucle_sortie(partage: Arc<Partage>, chat_tx: broadcast::Sender<String>) {
        let _ = initialize_mta();
        let mut capteur = match ouvrir(&Direction::Render) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Equaliseur] init loopback sortie impossible: {}", e);
                return;
            }
        };
        let mut spectre = Spectre::new();
        let mut envoi = Instant::now() - Duration::from_millis(PUBLICATION_MS as u64);
        // Une frame plate est envoyée une fois pour resetter le visuel, puis
        // les frames identiques sont sautées (silence = zéro trafic).
        let mut dernier_plat = false;
        while !partage.cancel.load(Ordering::Relaxed) {
            if !capteur.pomper(&mut spectre) {
                eprintln!("[Equaliseur] lecture loopback échouée — thread arrêté");
                break;
            }
            spectre.analyser();
            if envoi.elapsed().as_millis() < PUBLICATION_MS {
                continue;
            }
            envoi = Instant::now();
            let micro = *partage.bandes_micro.lock().unwrap();
            let mut bandes = spectre.bandes;
            for i in 0..NB_BANDES {
                if micro[i] > bandes[i] {
                    bandes[i] = micro[i];
                }
            }
            let plat = bandes.iter().all(|&b| b <= 0.005);
            if plat && dernier_plat {
                continue;
            }
            dernier_plat = plat;
            let msg = json!({ "type": "eq-spectrum", "bandes": bandes });
            let _ = chat_tx.send(msg.to_string());
        }
    }

    /// Thread micro — capture directe du périphérique d'entrée par défaut,
    /// écrit son spectre dans le slot partagé (lu par boucle_sortie).
    pub fn boucle_micro(partage: Arc<Partage>) {
        let _ = initialize_mta();
        let mut capteur = match ouvrir(&Direction::Capture) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Equaliseur] init capture micro impossible: {}", e);
                return;
            }
        };
        let mut spectre = Spectre::new();
        while !partage.cancel.load(Ordering::Relaxed) {
            if !capteur.pomper(&mut spectre) {
                eprintln!("[Equaliseur] lecture micro échouée — thread arrêté");
                break;
            }
            spectre.analyser();
            *partage.bandes_micro.lock().unwrap() = spectre.bandes;
        }
    }

    /// Ouvre un client de capture WASAPI en mode événementiel partagé.
    /// `Direction::Render` = device de sortie → la crate pose le flag loopback
    /// automatiquement (capture « ce qui sort ») ; `Direction::Capture` =
    /// périphérique d'entrée (micro). Format demandé : f32 stéréo 48 kHz,
    /// autoconvert si le device diffère.
    fn ouvrir(direction: &Direction) -> Result<Capteur, WasapiError> {
        let enumerateur = DeviceEnumerator::new()?;
        let device = enumerateur.get_default_device(direction)?;
        let mut client = device.get_iaudioclient()?;
        let fmt = WaveFormat::new(32, 32, &SampleType::Float, SAMPLE_RATE, CANAUX, None);
        let (_def, min_time) = client.get_device_period()?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_time.max(1),
        };
        client.initialize_client(&fmt, &Direction::Capture, &mode)?;
        let h_event = client.set_get_eventhandle()?;
        let capture = client.get_audiocaptureclient()?;
        client.start_stream()?;
        Ok(Capteur {
            blockalign: fmt.get_blockalign() as usize,
            octets: VecDeque::with_capacity(64 * 1024),
            capture,
            h_event,
            client,
        })
    }

    struct Capteur {
        /// Conservé pour stop_stream() à la sortie (Drop).
        client: AudioClient,
        capture: AudioCaptureClient,
        h_event: Handle,
        /// Octets bruts en attente de conversion (f32 stéréo interleave).
        octets: VecDeque<u8>,
        blockalign: usize,
    }

    impl Drop for Capteur {
        fn drop(&mut self) {
            let _ = self.client.stop_stream();
        }
    }

    impl Capteur {
        /// Attend un paquet (≤ WAIT_MS) puis draine le buffer dans `spectre`.
        /// Timeout = silence (injecté en zéros pour faire retomber les bandes).
        /// Retourne false = erreur de lecture fatale → le thread doit sortir.
        fn pomper(&mut self, spectre: &mut Spectre) -> bool {
            if self.h_event.wait_for_event(WAIT_MS).is_err() {
                spectre.pousser_zeros(FFT_N);
                return true;
            }
            // Drainer tous les paquets disponibles (plusieurs peuvent être
            // arrivés entre deux événements).
            loop {
                let avant = self.octets.len();
                let info = match self.capture.read_from_device_to_deque(&mut self.octets) {
                    Ok(i) => i,
                    Err(e) => {
                        eprintln!("[Equaliseur] read_from_device: {}", e);
                        return false;
                    }
                };
                let ajoutes = self.octets.len() - avant;
                if ajoutes == 0 {
                    break;
                }
                if info.flags.silent {
                    // Paquet silencieux : les octets sont indéfinis → zéros.
                    let frames = ajoutes / self.blockalign;
                    for _ in 0..ajoutes {
                        self.octets.pop_back();
                    }
                    spectre.pousser_zeros(frames);
                } else {
                    while self.octets.len() >= self.blockalign {
                        let l = lire_f32_le(&mut self.octets);
                        let r = lire_f32_le(&mut self.octets);
                        spectre.echantillons.push_back((l + r) * 0.5);
                    }
                }
            }
            true
        }
    }

    fn lire_f32_le(octets: &mut VecDeque<u8>) -> f32 {
        let mut b = [0u8; 4];
        for x in &mut b {
            *x = octets.pop_front().unwrap_or(0);
        }
        f32::from_le_bytes(b)
    }

    /// Accumulateur d'échantillons mono + analyse FFT → bandes normalisées
    /// [0..1] (dB mappé sur DB_MIN..0, max de magnitude par bande).
    struct Spectre {
        /// Échantillons mono récents (recouvrement 50 % entre analyses).
        echantillons: VecDeque<f32>,
        tampon: Vec<Complex32>,
        /// Fenêtre de Hann pré-calculée.
        fenetre: Vec<f32>,
        fft: Arc<dyn Fft<f32>>,
        /// Plages de bins FFT par bande (répartition log, pré-calculées).
        bords: Vec<(usize, usize)>,
        bandes: [f32; NB_BANDES],
    }

    impl Spectre {
        fn new() -> Self {
            let fft = FftPlanner::new().plan_fft_forward(FFT_N);
            let fenetre = (0..FFT_N)
                .map(|i| {
                    0.5 * (1.0
                        - (2.0 * std::f32::consts::PI * i as f32 / (FFT_N - 1) as f32).cos())
                })
                .collect();
            let bin_hz = SAMPLE_RATE as f32 / FFT_N as f32;
            let ratio = (F_MAX / F_MIN).powf(1.0 / NB_BANDES as f32);
            let mut bords = Vec::with_capacity(NB_BANDES);
            for i in 0..NB_BANDES {
                let b0 = ((F_MIN * ratio.powi(i as i32)) / bin_hz).floor() as usize;
                let b1 = (((F_MIN * ratio.powi(i as i32 + 1)) / bin_hz).ceil() as usize)
                    .max(b0 + 1)
                    .min(FFT_N / 2);
                bords.push((b0.min(FFT_N / 2 - 1), b1));
            }
            Self {
                echantillons: VecDeque::with_capacity(FFT_N * 2),
                tampon: vec![Complex32::ZERO; FFT_N],
                fenetre,
                fft,
                bords,
                bandes: [0.0; NB_BANDES],
            }
        }

        /// Injecte n échantillons de silence (zéros) — retombée du spectre
        /// quand le device n'émet plus rien.
        fn pousser_zeros(&mut self, n: usize) {
            self.echantillons.extend(std::iter::repeat(0.0).take(n));
        }

        /// Analyse les FFT_N derniers échantillons (recouvrement 50 %) et met
        /// à jour `bandes`. No-op tant que le buffer n'est pas rempli.
        fn analyser(&mut self) {
            if self.echantillons.len() < FFT_N {
                return;
            }
            let debut = self.echantillons.len() - FFT_N;
            for (i, &s) in self.echantillons.iter().skip(debut).enumerate() {
                self.tampon[i] = Complex32::new(s * self.fenetre[i], 0.0);
            }
            self.fft.process(&mut self.tampon);
            // Pic théorique ≈ N/2 × gain Hann 0,5 = N×0,25 pour un sinus plein
            // échelle → normalisation amplitude avant passage en dB.
            let norm = FFT_N as f32 * 0.25;
            for i in 0..NB_BANDES {
                let (b0, b1) = self.bords[i];
                let mut mag = 0.0f32;
                for c in &self.tampon[b0..b1] {
                    let m = c.norm();
                    if m > mag {
                        mag = m;
                    }
                }
                let db = 20.0 * ((mag / norm) + 1e-9).log10();
                self.bandes[i] = ((db - DB_MIN) / (-DB_MIN)).clamp(0.0, 1.0);
            }
            // Recouvrement 50 % : garder les derniers FFT_N/2 échantillons.
            while self.echantillons.len() > FFT_N / 2 {
                self.echantillons.pop_front();
            }
        }
    }
}

/// Stubs non-Windows : pas de WASAPI → threads no-op, le module n'émet rien.
#[cfg(not(windows))]
mod capture {
    use super::*;

    pub fn boucle_sortie(_partage: Arc<Partage>, _chat_tx: broadcast::Sender<String>) {}
    pub fn boucle_micro(_partage: Arc<Partage>) {}
}
