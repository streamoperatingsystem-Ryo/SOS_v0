//! Chemin audio **temps réel** — les échantillons sont consommés *pendant*
//! la session, pas seulement muxés à l'arrêt.
//!
//! Contrairement à `piste.rs` (PCM → fichier, remux à l'arrêt), ici chaque
//! source pousse dans un [`AnneauAudio`] borné (drop-oldest). La boucle
//! vidéo (`moteur.rs`) produit les frames 48 kHz **dues** via
//! [`MixeurLocal`] et les écrit en `s16le` dans le pipe audio du sidecar
//! FFmpeg — interleaving A/V en direct, réutilisable tel quel par un futur
//! sink RTMP/SRT (le muxeur porte alors les PTS, plan §12.2).
//!
//! `SourceLive` écrit **aussi** le PCM `s16le` historique : `muxer_audio`
//! reste le repli (live non créé) et le réparateur (live cassé en cours —
//! il remappe `0:v` + les PCM sur le fichier finalisé).

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::capture;
use super::piste::Piste;
use super::{ErreurAudio, Sens};

/// Format cible unique du mixage live : 48 kHz stéréo (entrée `-f s16le` de
/// FFmpeg, AAC 48 kHz en sortie — même contrat que le post-mux).
pub const TAUX_CIBLE: u32 = 48_000;
pub const CANAUX_CIBLE: u16 = 2;

/// Frames 48 kHz produites au maximum par appel de [`MixeurLocal::produire`]
/// (~100 ms). Borne le travail d'un rattrapage après une stalle de boucle.
const BLOC_MAX: u64 = TAUX_CIBLE as u64 / 10;

/// Anneau d'échantillons **f32 entrelacés** entre un producteur (thread
/// WASAPI, lecteur de média) et le mixage. Borné à ~1 s : un consommateur en
/// retard fait perdre les plus vieux échantillons (drop-oldest, jamais de
/// blocage — invariant « file bornée »).
///
/// `pos` est la position de lecture **fractionnaire** en frames source :
/// [`mixer_dans`](AnneauAudio::mixer_dans) resample linéairement vers 48 kHz
/// stéréo et consomme ensuite ce qui a été dépassé.
pub struct AnneauAudio {
    /// Taux source ; 0 = non configuré → l'anneau rend du silence.
    taux: u32,
    canaux: u16,
    tampon: VecDeque<f32>,
    /// Position de lecture fractionnaire, en frames source.
    pos: f64,
    /// Capacité en échantillons (≈ 1 s de source).
    cap: usize,
}

impl AnneauAudio {
    /// Anneau muet : tant que `configurer` n'a pas reçu le format réel, il
    /// produit du silence (source pas encore ouverte — ou en échec).
    pub fn nouveau() -> Self {
        Self {
            taux: 0,
            canaux: 0,
            tampon: VecDeque::new(),
            pos: 0.0,
            cap: 0,
        }
    }

    /// Anneau de format connu (flux déjà fixe — ex. média en 48 kHz stéréo).
    pub fn avec_format(taux: u32, canaux: u16) -> Self {
        let mut anneau = Self::nouveau();
        anneau.configurer(taux, canaux);
        anneau
    }

    /// Fixe le format source : ~1 s de capacité, repart de zéro.
    pub fn configurer(&mut self, taux: u32, canaux: u16) {
        self.taux = taux;
        self.canaux = canaux;
        self.cap = taux as usize * canaux as usize;
        self.purger();
    }

    /// Oublie tout le contenu (pré-roll WASAPI, seek d'un média).
    pub fn purger(&mut self) {
        self.tampon.clear();
        self.pos = 0.0;
    }

    /// Pousse des échantillons entrelacés. Au-delà de la capacité, les plus
    /// vieux sont perdus — et `pos` (comptée depuis la tête) est recalée
    /// pour rester cohérente avec ce qui reste.
    pub fn pousser(&mut self, echantillons: &[f32]) {
        let canaux = self.canaux as usize;
        if self.taux == 0 || canaux == 0 || self.cap == 0 {
            return;
        }
        self.tampon.extend(echantillons.iter().copied());
        if self.tampon.len() > self.cap {
            let exces = self.tampon.len() - self.cap;
            // Arrondi à la frame : ne jamais désentrelacer le flux.
            let frames = exces.div_ceil(canaux);
            self.tampon.drain(..frames * canaux);
            self.pos = (self.pos - frames as f64).max(0.0);
        }
    }

    /// Mixe jusqu'à `n` frames **48 kHz stéréo** dans `dest` (accumulation,
    /// gain appliqué) — resampling linéaire. Rend le nombre de frames
    /// réellement produites ; le reste de `dest` garde sa valeur (silence).
    pub fn mixer_dans(&mut self, n: usize, dest: &mut [f32], gain: f32) -> usize {
        let canaux = self.canaux as usize;
        if self.taux == 0 || canaux == 0 || gain == 0.0 {
            return 0;
        }
        let frames_dispo = self.tampon.len() / canaux;
        let ratio = self.taux as f64 / TAUX_CIBLE as f64;
        let cible = CANAUX_CIBLE as usize;
        let mut produites = 0;
        for (i, case) in dest.chunks_exact_mut(cible).take(n).enumerate() {
            let i0 = self.pos as usize;
            if i0 >= frames_dispo {
                break; // sous-alimentation : silence pour la suite
            }
            let frac = (self.pos - i0 as f64) as f32;
            // Dernière frame disponible : interpolée vers elle-même.
            let i1 = (i0 + 1).min(frames_dispo - 1);
            let (a, b) = (i0 * canaux, i1 * canaux);
            for (c, sortie) in case.iter_mut().enumerate() {
                // Mono dupliqué ; > 2 canaux : les deux premiers.
                let sc = if canaux == 1 { 0 } else { c.min(canaux - 1) };
                *sortie +=
                    (self.tampon[a + sc] * (1.0 - frac) + self.tampon[b + sc] * frac) * gain;
            }
            self.pos += ratio;
            produites = i + 1;
        }
        // Consomme les frames source entièrement dépassées.
        let consommer = (self.pos as usize).min(frames_dispo);
        if consommer > 0 {
            self.tampon.drain(..consommer * canaux);
            self.pos -= consommer as f64;
        }
        produites
    }
}

/// Une entrée du mixage : l'anneau d'une source + son gain (millièmes,
/// 1000 = unité — même convention qu'`AudioCfg`).
#[derive(Clone)]
pub struct EntreeMix {
    pub anneau: Arc<Mutex<AnneauAudio>>,
    pub gain_milliemes: u32,
}

/// Source WASAPI « live » : capture en thread dédié, double sortie —
/// **anneau** pour le mixage temps réel **et** PCM `s16le` en fichier (le
/// post-mux reste le repli/réparateur, voir l'en-tête du module).
///
/// Même structure que `piste::CapturePiste` : `attendre_demarrage` pour
/// l'alignement A/V (buffer WASAPI de 200 ms), `arreter` rend la `Piste`.
pub struct SourceLive {
    arret: Arc<AtomicBool>,
    fil: Option<JoinHandle<Result<Piste, ErreurAudio>>>,
    /// Signale le **premier paquet réellement capturé**.
    demarrage: Option<Receiver<()>>,
    anneau: Arc<Mutex<AnneauAudio>>,
}

impl SourceLive {
    /// Démarre la capture (`nom` sert au fichier temporaire et au thread).
    ///
    /// Rend immédiatement : une erreur d'ouverture WASAPI ne survient que
    /// dans le thread — elle remontera par `attendre_demarrage` (délai
    /// écoulé) puis `arreter`, comme pour `piste::demarrer`.
    pub fn demarrer(sens: Sens, gain_milliemes: u32, nom: &str) -> Result<Self, ErreurAudio> {
        let chemin = std::env::temp_dir().join(format!(
            "sos-audio-{}-{}.pcm",
            std::process::id(),
            nom
        ));
        let anneau = Arc::new(Mutex::new(AnneauAudio::nouveau()));
        let anneau_fil = Arc::clone(&anneau);
        let arret = Arc::new(AtomicBool::new(false));
        let drapeau = Arc::clone(&arret);
        let chemin_fil = chemin.clone();
        let (tx, rx) = mpsc::channel::<()>();

        let fil = std::thread::Builder::new()
            .name(format!("sos-audio-{nom}"))
            .spawn(move || -> Result<Piste, ErreurAudio> {
                let mut capteur = capture::ouvrir(sens)?;
                let format = capteur.format();
                anneau_fil
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .configurer(format.taux, format.canaux);
                let fichier = File::create(&chemin_fil).map_err(|e| {
                    ErreurAudio::Lecture(format!("création {} : {e}", chemin_fil.display()))
                })?;
                let mut sortie = BufWriter::with_capacity(64 * 1024, fichier);
                let mut tampon: Vec<f32> = Vec::with_capacity(8192);
                let mut octets: Vec<u8> = Vec::with_capacity(8192 * 2);
                let mut echantillons: u64 = 0;

                while !drapeau.load(Ordering::Relaxed) {
                    tampon.clear();
                    let lus = capteur.lire(&mut tampon)?;
                    if lus == 0 {
                        continue;
                    }
                    anneau_fil
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .pousser(&tampon);
                    octets.clear();
                    octets.reserve(lus * 2);
                    for valeur in &tampon {
                        let pcm = (valeur.clamp(-1.0, 1.0) * 32767.0) as i16;
                        octets.extend_from_slice(&pcm.to_le_bytes());
                    }
                    sortie.write_all(&octets).map_err(|e| {
                        ErreurAudio::Lecture(format!("écriture {} : {e}", chemin_fil.display()))
                    })?;
                    if echantillons == 0 {
                        let _ = tx.send(());
                    }
                    echantillons += lus as u64;
                }
                sortie.flush().map_err(|e| {
                    ErreurAudio::Lecture(format!("flush {} : {e}", chemin_fil.display()))
                })?;
                capteur.arreter();
                let duree_s = if format.taux > 0 && format.canaux > 0 {
                    echantillons as f64 / (format.taux as f64 * format.canaux as f64)
                } else {
                    0.0
                };
                Ok(Piste {
                    chemin: chemin_fil,
                    taux: format.taux,
                    canaux: format.canaux,
                    gain_milliemes,
                    duree_s,
                })
            })
            .map_err(|e| ErreurAudio::Init(format!("thread audio : {e}")))?;

        Ok(Self {
            arret,
            fil: Some(fil),
            demarrage: Some(rx),
            anneau,
        })
    }

    /// Anneau de la source — à enregistrer dans les entrées du mixeur.
    pub fn anneau(&self) -> Arc<Mutex<AnneauAudio>> {
        Arc::clone(&self.anneau)
    }

    /// Attend le premier paquet audio (≤ `delai`) — alignement A/V,
    /// identique à `CapturePiste::attendre_demarrage`.
    pub fn attendre_demarrage(&mut self, delai: Duration) -> bool {
        match self.demarrage.take() {
            Some(rx) => matches!(rx.recv_timeout(delai), Ok(())),
            None => true,
        }
    }

    /// Arrête la capture et attend le thread. Rend la `Piste` écrite — le
    /// post-mux peut réparer un chemin live cassé.
    pub fn arreter(mut self) -> Result<Piste, ErreurAudio> {
        self.arret.store(true, Ordering::Relaxed);
        match self.fil.take() {
            Some(fil) => fil.join().unwrap_or_else(|_| {
                Err(ErreurAudio::Lecture("thread audio interrompu".to_string()))
            }),
            None => Err(ErreurAudio::Lecture("piste déjà arrêtée".to_string())),
        }
    }
}

/// Mixeur temps réel tournant **dans la boucle vidéo** (pas de thread
/// supplémentaire) : à chaque frame, produit les frames 48 kHz dues depuis
/// [`demarrer`](MixeurLocal::demarrer) et les rend en `s16le` prêt à écrire
/// dans le pipe audio du sidecar.
///
/// Une entrée sous-alimentée contribue silence (rien n'est retranché au
/// compteur) : les horodatages restent calés sur l'horloge murale, jamais
/// sur le contenu.
pub struct MixeurLocal {
    debut: Instant,
    /// Frames 48 kHz déjà produites depuis `demarrer` — c'est l'horloge du
    /// flux : la position dans le fichier = `emises / 48000` s.
    emises: u64,
    entrees: Vec<EntreeMix>,
    tampon: Vec<f32>,
    octets: Vec<u8>,
}

impl MixeurLocal {
    /// Mixeur sans entrée (horloge démarrée à la construction).
    pub fn nouveau() -> Self {
        Self {
            debut: Instant::now(),
            emises: 0,
            entrees: Vec::new(),
            tampon: Vec::new(),
            octets: Vec::new(),
        }
    }

    /// Entrées du mix — les anneaux média y sont rafraîchis à chaque
    /// `appliquer_scene` (une vidéo ajoutée à chaud rejoint le mix).
    pub fn entrees(&mut self) -> &mut Vec<EntreeMix> {
        &mut self.entrees
    }

    /// Recale le zéro du flux **et** purge toutes les entrées : le pré-roll
    /// WASAPI (buffer de 200 ms rempli pendant l'alignement) est jeté —
    /// l'échantillon t = 0 du pipe correspond à la frame t = 0 de la vidéo.
    pub fn demarrer(&mut self) {
        for entree in &self.entrees {
            entree
                .anneau
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .purger();
        }
        self.debut = Instant::now();
        self.emises = 0;
    }

    /// Produit les frames dues et les rend en **s16le** (48 kHz stéréo).
    /// `&[]` = rien de neuf à écrire.
    pub fn produire(&mut self) -> &[u8] {
        let cible = (self.debut.elapsed().as_secs_f64() * TAUX_CIBLE as f64) as u64;
        self.produire_jusqu(cible)
    }

    /// Produit les frames jusqu'à `cible` (extraite pour la testabilité —
    /// `produire` la calcule sur l'horloge murale).
    fn produire_jusqu(&mut self, cible: u64) -> &[u8] {
        let n = cible.saturating_sub(self.emises).min(BLOC_MAX) as usize;
        if n == 0 {
            return &[];
        }
        let cible_canaux = CANAUX_CIBLE as usize;
        self.tampon.clear();
        self.tampon.resize(n * cible_canaux, 0.0);
        for entree in &self.entrees {
            let gain = entree.gain_milliemes as f32 / 1000.0;
            entree
                .anneau
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .mixer_dans(n, &mut self.tampon, gain);
        }
        self.octets.clear();
        self.octets.reserve(n * cible_canaux * 2);
        for valeur in &self.tampon {
            let pcm = (valeur.clamp(-1.0, 1.0) * 32767.0) as i16;
            self.octets.extend_from_slice(&pcm.to_le_bytes());
        }
        self.emises += n as u64;
        &self.octets
    }
}

/// Nom du pipe nommé Windows pour une sortie (`n` = index du sink).
/// Pattern « audioNamedPipe » du plan §12.2.
pub fn nom_pipe_audio(n: usize) -> String {
    format!("sos-audio-{}-{n}", std::process::id())
}

/// Chemin d'ouverture côté client FFmpeg (`-i \\.\pipe\…`).
pub fn chemin_pipe_audio(nom: &str) -> String {
    format!(r"\\.\pipe\{nom}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Passthrough 48 kHz stéréo : `mixer_dans` rend la source à l'identique
    /// (ratio 1,0 → interpolation sans perte aux positions entières).
    #[test]
    fn anneau_48k_stereo_passthrough() {
        let mut anneau = AnneauAudio::avec_format(48_000, 2);
        let src: Vec<f32> = (0..480).flat_map(|i| [i as f32 / 480.0 - 0.5, 0.25]).collect();
        anneau.pousser(&src);
        let mut dest = vec![0.0f32; 480 * 2];
        assert_eq!(anneau.mixer_dans(480, &mut dest, 1.0), 480);
        for (a, b) in src.iter().zip(dest.iter()) {
            assert!((a - b).abs() < 1e-6, "passthrough : {a} vs {b}");
        }
        // Tout est consommé : un second appel rend 0 et laisse le silence.
        assert_eq!(anneau.mixer_dans(480, &mut dest, 1.0), 0);
    }

    /// Resampling 24 → 48 kHz : n frames source produisent ~2n frames de
    /// sortie, interpolées linéairement.
    #[test]
    fn anneau_resample_24k_vers_48k() {
        let mut anneau = AnneauAudio::avec_format(24_000, 1);
        // Rampe mono : 0, 1, 2, …, 99 (100 frames).
        let src: Vec<f32> = (0..100).map(|i| i as f32).collect();
        anneau.pousser(&src);
        let mut dest = vec![0.0f32; 200 * 2];
        let produites = anneau.mixer_dans(200, &mut dest, 1.0);
        // 100 frames source × 2 = 200 frames max, borné à la source : ~199.
        assert!(produites >= 198, "produites = {produites}");
        // Mono dupliqué sur les deux canaux, interpolation au milieu.
        assert!((dest[0] - 0.0).abs() < 1e-6);
        assert!((dest[2] - 0.5).abs() < 1e-4, "interp : {}", dest[2]);
        assert_eq!(dest[2], dest[3], "mono dupliqué");
    }

    /// Drop-oldest : plus d'1 s poussé → la capacité (1 s) tient, `pos`
    /// recalé, les plus vieux échantillons perdus.
    #[test]
    fn anneau_drop_oldest_et_alignement() {
        let mut anneau = AnneauAudio::avec_format(48_000, 2);
        let deux_secondes = vec![1.0f32; 48_000 * 2 * 2];
        anneau.pousser(&deux_secondes);
        assert_eq!(anneau.tampon.len(), 48_000 * 2);
        // La tête est le DÉBUT de la seconde seconde (le reste est perdu).
        let mut dest = vec![0.0f32; 2];
        anneau.mixer_dans(1, &mut dest, 1.0);
        assert_eq!(dest[0], 1.0);
    }

    /// Anneau non configuré (`taux` 0) : silence — ni push ni mix ne
    /// produisent quoi que ce soit (source jamais ouverte).
    #[test]
    fn anneau_non_configure_est_silence() {
        let mut anneau = AnneauAudio::nouveau();
        anneau.pousser(&[0.5; 100]);
        assert!(anneau.tampon.is_empty());
        let mut dest = vec![0.0f32; 20];
        assert_eq!(anneau.mixer_dans(10, &mut dest, 1.0), 0);
        assert!(dest.iter().all(|&v| v == 0.0));
    }

    /// Le mixeur émet en s16le, applique le gain, borne par `BLOC_MAX`, et
    /// ne rejoue jamais deux fois la même tranche.
    #[test]
    fn mixeur_gain_s16le_et_pas_de_rejeu() {
        let mut mixeur = MixeurLocal::nouveau();
        let anneau = Arc::new(Mutex::new(AnneauAudio::avec_format(48_000, 2)));
        // 480 frames à 1,0 → avec gain 500 ‰, s16le ≈ 16383.
        anneau
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pousser(&vec![1.0f32; 480 * 2]);
        mixeur.entrees().push(EntreeMix {
            anneau,
            gain_milliemes: 500,
        });
        let pcm = mixeur.produire_jusqu(480).to_vec();
        assert_eq!(pcm.len(), 480 * 2 * 2);
        let premier = i16::from_le_bytes([pcm[0], pcm[1]]);
        assert!((premier - 16383).abs() <= 1, "gain 500 ‰ : {premier}");
        // Un second appel à la même cible ne produit rien (pas de rejeu).
        assert!(mixeur.produire_jusqu(480).is_empty());
        // BLOC_MAX borne la production : une cible lointaine ne rend que
        // 4800 frames d'un coup — le reste au prochain passage.
        assert_eq!(mixeur.produire_jusqu(1_000_000).len() / 4, 4800);
    }

    /// `demarrer` recale l'horloge et purge les entrées : le pré-roll
    /// capturé avant le premier frame n'est jamais émis (alignement A/V).
    #[test]
    fn mixeur_demarrer_purge_le_preroll() {
        let mut mixeur = MixeurLocal::nouveau();
        let anneau = Arc::new(Mutex::new(AnneauAudio::avec_format(48_000, 2)));
        anneau
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pousser(&vec![0.9f32; 480 * 2]);
        mixeur.entrees().push(EntreeMix {
            anneau,
            gain_milliemes: 1000,
        });
        mixeur.demarrer();
        let pcm = mixeur.produire_jusqu(480);
        assert!(pcm.iter().all(|&b| b == 0), "pré-roll purgé → silence");
    }
}
