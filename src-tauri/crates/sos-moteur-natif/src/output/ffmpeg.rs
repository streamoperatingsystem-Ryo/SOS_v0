//! Sink conteneurisé via le **sidecar FFmpeg** : bitstream → fMP4 / MKV
//! (fichier) ou → FLV / MPEG-TS (flux live RTMP / SRT).
//!
//! Le sidecar ne reçoit que le bitstream (`-c copy`), jamais de frame brute.
//! ⚠️ Un flux H.264 élémentaire ne transporte pas d'horodatage : le timing
//! vient de `-framerate` (valeur passée à la création). Comme les paquets
//! sont écrits en temps réel à la cadence de l'encodeur, les PTS synthétisés
//! coïncident avec l'horloge murale — suffisant pour muxer FLV/TS ; la
//! dérive d'un paquet manquant se borne à sa durée.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::thread::JoinHandle;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_BUSY, ERROR_NO_DATA, ERROR_PIPE_LISTENING, ERROR_PIPE_NOT_CONNECTED, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_OUTBOUND,
};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, SetNamedPipeHandleState, PIPE_NOWAIT, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};

use super::{Conteneur, ErreurSortie, Sink};
use crate::audio::live::{
    chemin_pipe_audio, chemin_pipe_audio_ffmpeg, nom_pipe_audio, CANAUX_CIBLE, TAUX_CIBLE,
};
use crate::encode::ffmpeg::chemin_sidecar;
use crate::encode::PaquetEncode;
use sos_moteur_api::{Codec, ErreurMoteur};

/// Taille maximale de stderr conservée par sink réseau (queue de tampon).
const STDERR_MAX: usize = 16 * 1024;

/// Muxer réseau d'une sortie live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatReseau {
    /// FLV — RTMP et variantes (`rtmp://`, `rtmps://`).
    Flv,
    /// MPEG-TS — SRT (`srt://`, mode caller par défaut du côté ffmpeg).
    Mpegts,
}

impl FormatReseau {
    /// Argument `-f` de FFmpeg.
    fn muxer(self) -> &'static str {
        match self {
            FormatReseau::Flv => "flv",
            FormatReseau::Mpegts => "mpegts",
        }
    }
}

/// Destination du flux conteneurisé (interne aux constructeurs).
enum Sortie {
    /// fMP4 / MKV local.
    Fichier { chemin: std::path::PathBuf, conteneur: Conteneur },
    /// Flux réseau — l'URL embarque déjà la clé si elle est fournie ;
    /// `cle` est la clé seule, masquée en plus de l'URL (ffmpeg peut
    /// citer une forme tronquée de l'URL où la clé resterait lisible).
    Reseau {
        format: FormatReseau,
        url: String,
        cle: Option<String>,
    },
}

/// Démultiplexeur du bitstream élémentaire sur stdin.
fn nom_demuxer(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "h264",
        Codec::Hevc => "hevc",
        Codec::Av1 => "av1",
    }
}

/// Muxeur piloté par un processus FFmpeg (stdin = bitstream H.264).
pub struct FfmpegSink {
    enfant: Child,
    /// Stdin direct — `None` dès que le fil d'écriture le possède.
    entree: Option<ChildStdin>,
    /// Fil d'écriture du bitstream : la composition **ne bloque jamais** sur
    /// le stdin (un FFmpeg qui ne draine plus devient une dégradation
    /// explicite, jamais un gel de la boucle).
    ecrivain: Option<(SyncSender<Vec<u8>>, JoinHandle<()>)>,
    octets: u64,
    paquets: u64,
    ferme: bool,
    /// Queue de stderr drainée par un fil dédié — **sinks réseau
    /// uniquement** : stderr n'y est plus hérité, car ffmpeg cite l'URL
    /// (qui embarque la clé de flux) dans ses messages d'erreur.
    stderr: Option<Arc<Mutex<Vec<u8>>>>,
    /// Chaînes à masquer dans toute erreur remontée (URL complète, donc la
    /// clé qu'elle contient). Vide pour un sink fichier.
    secrets: Vec<String>,
}

/// Délai maximal d'attente d'une écriture avant de considérer la sortie
/// **figée** (FFmpeg ne draine plus). Généreux : un pic d'encodage de 2 s se
/// résorbe, au-delà la sortie est perdue de toute façon.
const DELAI_ECRITURE: std::time::Duration = std::time::Duration::from_secs(3);

/// Profondeur de la file d'écriture, en paquets (~0,13 s à 60 fps).
const FILE_ECRITURE: usize = 8;

impl FfmpegSink {
    /// Démarre le sidecar. `ffmpeg` à `None` → résolution par `chemin_sidecar`.
    pub fn nouveau(
        ffmpeg: Option<&Path>,
        sortie: &Path,
        conteneur: Conteneur,
        fps: u32,
    ) -> Result<Self, ErreurSortie> {
        Self::nouveau_codec(ffmpeg, sortie, conteneur, Codec::H264, fps)
    }

    /// [`FfmpegSink::nouveau`] avec le codec réel : le démultiplexeur du
    /// bitstream stdin suit le codec encodé (`h264` / `hevc` / `av1`).
    pub fn nouveau_codec(
        ffmpeg: Option<&Path>,
        sortie: &Path,
        conteneur: Conteneur,
        codec: Codec,
        fps: u32,
    ) -> Result<Self, ErreurSortie> {
        let cible = Sortie::Fichier {
            chemin: sortie.to_path_buf(),
            conteneur,
        };
        Self::lancer(ffmpeg, &cible, codec, fps, None)
    }

    /// Démarre le sidecar avec **l'audio en direct** : le mix 48 kHz stéréo
    /// est alimenté pendant l'enregistrement via un pipe nommé Windows
    /// (pattern « audioNamedPipe » du plan §12.2), muxé en AAC par FFmpeg —
    /// aucun post-mux nécessaire quand ce chemin tient.
    ///
    /// Rend le sink **et** le `PipeAudio` : l'écrivain vit dans la boucle
    /// vidéo (`moteur.rs`), qui y pousse le mix `s16le`. Fermer le pipe =
    /// EOF audio pour FFmpeg.
    pub fn nouveau_avec_audio(
        ffmpeg: Option<&Path>,
        sortie: &Path,
        conteneur: Conteneur,
        fps: u32,
        index: usize,
    ) -> Result<(Self, PipeAudio), ErreurSortie> {
        Self::nouveau_avec_audio_codec(ffmpeg, sortie, conteneur, Codec::H264, fps, index)
    }

    /// [`FfmpegSink::nouveau_avec_audio`] avec le codec réel.
    pub fn nouveau_avec_audio_codec(
        ffmpeg: Option<&Path>,
        sortie: &Path,
        conteneur: Conteneur,
        codec: Codec,
        fps: u32,
        index: usize,
    ) -> Result<(Self, PipeAudio), ErreurSortie> {
        let cible = Sortie::Fichier {
            chemin: sortie.to_path_buf(),
            conteneur,
        };
        Self::avec_audio(ffmpeg, &cible, codec, fps, index)
    }

    /// Flux live **vidéo seule** vers une URL réseau : `-f flv` (RTMP) ou
    /// `-f mpegts` (SRT). L'URL embarque déjà la clé de flux si elle est
    /// fournie — elle n'est **jamais journalisée** (stderr capturé et
    /// masqué, contrairement au sink fichier qui l'hérite). `cle` = la clé
    /// seule, masquée en plus de l'URL complète.
    pub fn nouveau_reseau(
        ffmpeg: Option<&Path>,
        url: &str,
        cle: Option<&str>,
        format: FormatReseau,
        codec: Codec,
        fps: u32,
    ) -> Result<Self, ErreurSortie> {
        let cible = Sortie::Reseau {
            format,
            url: url.to_string(),
            cle: cle.map(str::to_string),
        };
        Self::lancer(ffmpeg, &cible, codec, fps, None)
    }

    /// Flux live **vidéo + audio** : même pipe nommé que l'enregistrement
    /// (`nouveau_avec_audio`), muxé en AAC dans le FLV/TS.
    pub fn nouveau_reseau_avec_audio(
        ffmpeg: Option<&Path>,
        url: &str,
        cle: Option<&str>,
        format: FormatReseau,
        codec: Codec,
        fps: u32,
        index: usize,
    ) -> Result<(Self, PipeAudio), ErreurSortie> {
        let cible = Sortie::Reseau {
            format,
            url: url.to_string(),
            cle: cle.map(str::to_string),
        };
        Self::avec_audio(ffmpeg, &cible, codec, fps, index)
    }

    /// Factorisation des variantes « avec audio » : crée l'instance de pipe
    /// nommée **avant** le spawn (le `CreateFile` du client l'exige), puis
    /// lance le muxeur. Fermer le pipe = EOF audio pour FFmpeg.
    fn avec_audio(
        ffmpeg: Option<&Path>,
        cible: &Sortie,
        codec: Codec,
        fps: u32,
        index: usize,
    ) -> Result<(Self, PipeAudio), ErreurSortie> {
        let nom = nom_pipe_audio(index);
        // Deux orthographes du même pipe : `\\.\` exigée par
        // `CreateNamedPipeW`, `//./` acceptée par le protocole `file` de
        // FFmpeg (le backslash y est refusé — cf. `chemin_pipe_audio_ffmpeg`).
        let serveur = chemin_pipe_audio(&nom);
        let client = chemin_pipe_audio_ffmpeg(&nom);
        let pipe = PipeAudio::creer(&serveur)?;
        let sink = Self::lancer(ffmpeg, cible, codec, fps, Some(&client))?;
        Ok((sink, pipe))
    }

    /// Construction commune. `pipe_audio` = chemin client `//./pipe/…`
    /// d'une instance `PipeAudio` **déjà créée** (sinon le `CreateFile` de
    /// FFmpeg échouerait — le client exige l'instance en écoute).
    fn lancer(
        ffmpeg: Option<&Path>,
        cible: &Sortie,
        codec: Codec,
        fps: u32,
        pipe_audio: Option<&str>,
    ) -> Result<Self, ErreurSortie> {
        if fps == 0 {
            return Err(ErreurSortie::Processus("fps nul".into()));
        }
        let binaire = match ffmpeg {
            Some(p) => p.to_path_buf(),
            None => chemin_sidecar().map_err(|e| ErreurSortie::Processus(e.to_string()))?,
        };

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-f".into(),
            nom_demuxer(codec).to_string(),
            "-framerate".into(),
            fps.to_string(),
            "-i".into(),
            "pipe:0".into(),
        ];
        match pipe_audio {
            // Interleaving live : PCM brut → AAC ; le mapping est explicite
            // (vidéo = entrée 0, audio = entrée 1). Jamais `-shortest` ici :
            // une fin prématurée du pipe couperait la vidéo.
            Some(pipe) => args.extend([
                "-f".into(),
                "s16le".into(),
                "-ar".into(),
                TAUX_CIBLE.to_string(),
                "-ac".into(),
                CANAUX_CIBLE.to_string(),
                // ⚠️ **Fenêtre d'analyse supprimée** (gel n° 2, §75) : le
                // format est entièrement spécifié par `-f/-ar/-ac`, donc
                // `avformat_find_stream_info` n'a rien à deviner. Sans cette
                // option il lit ~4,3 s de PCM (819 200 o, défaut 5 s) **au
                // rythme réel et sans lire stdin** — le tampon du stdin vidéo
                // (~74 Ko) déborde, la composition se bloque et n'alimente
                // plus le pipe audio : interblocage définitif.
                "-analyzeduration".into(),
                "1".into(),
                "-probesize".into(),
                "32".into(),
                "-i".into(),
                pipe.to_string(),
                "-map".into(),
                "0:v".into(),
                "-map".into(),
                "1:a".into(),
                "-c:v".into(),
                "copy".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "192k".into(),
            ]),
            None => args.extend(["-c".into(), "copy".into()]),
        }
        let mut secrets = Vec::new();
        match cible {
            Sortie::Fichier { chemin, conteneur } => {
                match conteneur {
                    Conteneur::Mp4 => args.extend([
                        "-movflags".into(),
                        "+frag_keyframe+empty_moov+default_base_moof".into(),
                        "-f".into(),
                        "mp4".into(),
                    ]),
                    Conteneur::Mkv => args.extend(["-f".into(), "matroska".into()]),
                }
                args.push(chemin.to_string_lossy().to_string());
            }
            Sortie::Reseau { format, url, cle } => {
                args.extend(["-f".into(), format.muxer().to_string(), url.clone()]);
                secrets.push(url.clone());
                if let Some(c) = cle {
                    secrets.push(c.clone());
                }
            }
        }

        // Réseau : stderr n'est **jamais hérité** — ffmpeg y cite l'URL (donc
        // la clé). Le pipe est drainé par un fil qui ne garde que la queue
        // de tampon, relue (masquée) pour les erreurs remontées.
        let stderr_partage = match cible {
            Sortie::Reseau { .. } => Some(Arc::new(Mutex::new(Vec::new()))),
            _ => None,
        };
        let mut enfant = Command::new(&binaire)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(match &stderr_partage {
                Some(_) => Stdio::piped(),
                None => Stdio::inherit(),
            })
            .spawn()
            .map_err(|e| {
                ErreurSortie::Processus(format!("lancement de {} : {e}", binaire.display()))
            })?;
        let entree = enfant.stdin.take();
        if let (Some(partage), Some(mut sortie_err)) = (&stderr_partage, enfant.stderr.take()) {
            let partage = Arc::clone(partage);
            let _ = std::thread::Builder::new()
                .name("sos-ffmpeg-stderr".to_string())
                .spawn(move || {
                    let mut tampon = [0u8; 4096];
                    loop {
                        match sortie_err.read(&mut tampon) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                let mut v =
                                    partage.lock().unwrap_or_else(|e| e.into_inner());
                                v.extend_from_slice(&tampon[..n]);
                                if v.len() > STDERR_MAX {
                                    let moitie = v.len() - STDERR_MAX / 2;
                                    v.drain(..moitie);
                                }
                            }
                        }
                    }
                });
        }

        let mut sink = Self {
            enfant,
            entree,
            ecrivain: None,
            octets: 0,
            paquets: 0,
            ferme: false,
            stderr: stderr_partage,
            secrets,
        };
        sink.demarrer_ecrivain();
        Ok(sink)
    }

    /// Démarre le fil qui possède le stdin : la boucle de composition ne fait
    /// plus qu'empiler des paquets dans une file **bornée**.
    ///
    /// ⚠️ **Pourquoi** (§75) : un `WriteFile` sur le stdin de FFmpeg bloque
    /// dès que FFmpeg cesse de le drainer (il attend alors de l'audio) — la
    /// composition, bloquée, ne produit plus de vidéo : interblocage à deux
    /// flux. Mesuré : `pousser` bloquait 1,5 s, puis définitivement.
    fn demarrer_ecrivain(&mut self) {
        let Some(mut entree) = self.entree.take() else {
            return;
        };
        let (tx, rx) = sync_channel::<Vec<u8>>(FILE_ECRITURE);
        let fil = std::thread::Builder::new()
            .name("sos-ffmpeg-ecrivain".to_string())
            .spawn(move || {
                // Sort de la boucle si le pipe casse ; `flush` + drop =
                // EOF pour FFmpeg.
                while let Ok(bloc) = rx.recv() {
                    if entree.write_all(&bloc).is_err() {
                        break;
                    }
                }
                let _ = entree.flush();
            });
        match fil {
            Ok(fil) => self.ecrivain = Some((tx, fil)),
            Err(e) => {
                // Fil impossible (OOM) : on garde l'écriture directe — le
                // risque de blocage revient, mais la sortie fonctionne.
                log::warn!("sink : fil d'écriture indisponible ({e})");
                self.ecrivain = None;
            }
        }
    }

    /// Ferme la file et attend que le fil ait tout écrit (EOF pour FFmpeg).
    fn arreter_ecrivain(&mut self) {
        if let Some((tx, fil)) = self.ecrivain.take() {
            drop(tx);
            let _ = fil.join();
        }
        drop(self.entree.take());
    }

    pub fn octets(&self) -> u64 {
        self.octets
    }

    pub fn paquets(&self) -> u64 {
        self.paquets
    }

    /// Queue de stderr lue par le fil draineur, avec les secrets masqués.
    /// `None` pour un sink fichier (stderr hérité, rien à masquer).
    fn queue_stderr(&self) -> Option<String> {
        self.stderr.as_ref().map(|partage| {
            let brut = partage.lock().unwrap_or_else(|e| e.into_inner()).clone();
            self.masquer(&String::from_utf8_lossy(&brut))
        })
    }

    /// Remplace chaque secret (URL embarquant la clé) par « <sortie> ».
    fn masquer(&self, texte: &str) -> String {
        let mut t = texte.to_string();
        for secret in &self.secrets {
            if !secret.is_empty() {
                t = t.replace(secret.as_str(), "<sortie>");
            }
        }
        t
    }

    /// Complète une erreur de processus par la queue de stderr masquée.
    fn erreur_processus(&self, message: String) -> ErreurMoteur {
        let detail = self
            .queue_stderr()
            .map(|s| s.trim().lines().last().unwrap_or("").trim().to_string())
            .filter(|s| !s.is_empty());
        let complet = match detail {
            Some(d) => format!("{message} — {d}"),
            None => message,
        };
        ErreurMoteur::from(ErreurSortie::Processus(complet))
    }

    /// Arrêt **brutal** (annulation, test de robustesse) : tue le processus.
    ///
    /// Le conteneur reste lisible jusqu'au dernier fragment complet.
    pub fn couper(&mut self) -> Result<(), ErreurSortie> {
        self.ferme = true;
        // Le fil peut être bloqué sur un FFmpeg figé : on tue d'abord, il
        // sortira sur l'erreur d'écriture.
        if let Some((tx, _)) = self.ecrivain.take() {
            drop(tx);
        }
        drop(self.entree.take());
        self.enfant
            .kill()
            .map_err(|e| ErreurSortie::Processus(format!("kill : {e}")))?;
        let _ = self.enfant.wait();
        Ok(())
    }
}

impl Sink for FfmpegSink {
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurMoteur> {
        if let Some((tx, _)) = self.ecrivain.as_ref() {
            // File bornée + échéance : jamais de blocage indéfini de la
            // composition (voir `demarrer_ecrivain`). `send_timeout` est
            // instable → `try_send` + attente courte bornée.
            let debut = std::time::Instant::now();
            let mut bloc = Some(paquet.data.clone());
            while let Some(data) = bloc.take() {
                match tx.try_send(data) {
                    Ok(()) => break,
                    Err(TrySendError::Full(data)) => {
                        if debut.elapsed() >= DELAI_ECRITURE {
                            return Err(self.erreur_processus(
                                "ecriture : FFmpeg ne draine plus (sortie figee)".to_string(),
                            ));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        bloc = Some(data);
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        return Err(self
                            .erreur_processus("ecriture : fil d'ecriture arrete".to_string()))
                    }
                }
            }
        } else {
            let entree = self
                .entree
                .as_mut()
                .ok_or_else(|| ErreurSortie::Processus("stdin deja ferme".into()))
                .map_err(ErreurMoteur::from)?;
            entree
                .write_all(&paquet.data)
                .map_err(|e| self.erreur_processus(format!("ecriture : {e}")))?;
        }
        self.octets += paquet.data.len() as u64;
        self.paquets += 1;
        Ok(())
    }

    fn fermer(&mut self) -> Result<(), ErreurMoteur> {
        self.ferme = true;
        self.arreter_ecrivain();
        let statut = self
            .enfant
            .wait()
            .map_err(|e| self.erreur_processus(format!("attente : {e}")))?;
        if !statut.success() {
            return Err(self.erreur_processus(format!("ffmpeg : statut {:?}", statut.code())));
        }
        Ok(())
    }
}

impl Drop for FfmpegSink {
    fn drop(&mut self) {
        if self.ferme {
            return;
        }
        // Best effort : on coupe la file puis on **tue** — `wait()` seul
        // pourrait attendre un FFmpeg figé (le chemin propre est `fermer`).
        if let Some((tx, _)) = self.ecrivain.take() {
            drop(tx);
        }
        drop(self.entree.take());
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
    }
}

/// Extrémité **serveur** du pipe nommé audio (`PIPE_ACCESS_OUTBOUND`) :
/// la boucle y écrit le mix `s16le` 48 kHz stéréo, FFmpeg le lit côté
/// client en ouvrant `\\.\pipe\…` comme un simple fichier (sa `CreateFile`
/// se connecte à l'instance en écoute, créée **avant** le spawn).
///
/// Aucun réseau : un pipe nommé est un canal kernel local, conforme au
/// contrat vie privée — et un futur sink RTMP/SRT réutilise le même
/// transport (le muxeur y porte les PTS).
///
/// `HANDLE` est un entier : `PipeAudio` est naturellement `Send`.
pub struct PipeAudio {
    handle: HANDLE,
    /// `\\.\pipe\sos-audio-<pid>-<n>` — affiché dans les erreurs.
    nom: String,
}

impl PipeAudio {
    /// Crée l'instance nommée en écoute (1 client max, refus des clients
    /// distants). Doit précéder le spawn FFmpeg : le `CreateFile` du client
    /// échoue immédiatement s'il n'y a aucune instance.
    fn creer(chemin: &str) -> Result<Self, ErreurSortie> {
        let large: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(large.as_ptr()),
                PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,           // une seule instance — un seul client FFmpeg
                256 * 1024,  // tampon sortie (~85 ms de PCM 48 kHz stéréo)
                1024,        // tampon entrée inutilisé (outbound)
                0,
                None,
            )
        };
        if handle.is_invalid() {
            return Err(ErreurSortie::Processus(format!(
                "pipe {chemin} : {}",
                windows::core::Error::from_win32().message()
            )));
        }
        // ⚠️ **Écriture non bloquante** (gel de composition, §67) : en mode
        // `PIPE_WAIT`, un `WriteFile` sur ce pipe attend que FFmpeg le draine.
        // FFmpeg cesse de le lire quand il attend de la vidéo — et la
        // composition, bloquée ici, ne lui en envoie plus : interblocage
        // définitif (reproduit : 389 composites puis gel). En `PIPE_NOWAIT`,
        // un tampon plein rend la main immédiatement et le bloc est **sauté**
        // (trou audio borné) — jamais le flux entier.
        let mode = PIPE_NOWAIT;
        if let Err(e) = unsafe { SetNamedPipeHandleState(handle, Some(&mode), None, None) } {
            // Non fatal : on garde le mode bloquant (comportement d'avant).
            log::warn!("pipe {chemin} : mode non bloquant refusé ({e})");
        }
        Ok(Self {
            handle,
            nom: chemin.to_string(),
        })
    }

    /// Écrit un bloc PCM entier. Tant que FFmpeg n'a pas fait son
    /// `CreateFile` (les ~ms entre le spawn et l'ouverture de l'entrée 1),
    /// `WriteFile` reçoit `ERROR_PIPE_LISTENING` : le bloc est sauté —
    /// trou bref au pire, jamais d'attente ni de casse de la voie live.
    ///
    /// Le pipe est en `PIPE_NOWAIT` (voir `creer`) : si FFmpeg stagne en
    /// lecture, le tampon plein rend la main et le bloc est **sauté** —
    /// `ERROR_PIPE_NOT_CONNECTED` / `ERROR_NO_DATA` / `ERROR_BUSY` ou une
    /// écriture nulle. C'est un trou audio borné, jamais un gel de la
    /// composition (le flux vidéo continue d'alimenter FFmpeg, qui reprend
    /// la lecture de l'audio dès qu'il peut).
    /// Pas de `ConnectNamedPipe` : il **bloquerait** tant qu'aucun client
    /// n'est là, et la connexion est faite côté client par FFmpeg.
    pub fn ecrire(&mut self, octets: &[u8]) -> Result<(), ErreurSortie> {
        let mut ecrits = 0usize;
        while ecrits < octets.len() {
            let mut n = 0u32;
            if let Err(e) = unsafe {
                WriteFile(
                    self.handle,
                    Some(&octets[ecrits..]),
                    Some(&mut n),
                    None,
                )
            } {
                // Pas de client / tampon plein : bloc sauté, jamais d'attente.
                // ⚠️ Comparer des `HRESULT` (`HRESULT_FROM_WIN32`), jamais le
                // code Win32 nu : `0x8007_00E7` ≠ `231`.
                if [
                    ERROR_PIPE_LISTENING,
                    ERROR_PIPE_NOT_CONNECTED,
                    ERROR_NO_DATA,
                    ERROR_BUSY,
                ]
                .iter()
                .any(|code| e.code() == windows::core::HRESULT::from_win32(code.0))
                {
                    return Ok(());
                }
                return Err(ErreurSortie::Processus(format!(
                    "pipe audio {} : {}",
                    self.nom,
                    e.message()
                )));
            }
            if n == 0 {
                // `PIPE_NOWAIT` sans place : rien écrit, bloc sauté.
                return Ok(());
            }
            ecrits += n as usize;
        }
        Ok(())
    }
}

impl Drop for PipeAudio {
    fn drop(&mut self) {
        // Fermer le handle = EOF audio pour FFmpeg.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Ajoute les pistes audio à un fichier **déjà finalisé** (muxage en deux temps).
///
/// Le son est capturé en PCM brut pendant l'enregistrement (threads WASAPI,
/// `audio::piste`), puis FFmpeg rééchantillonne, applique les gains et mixe —
/// aucun code de resampling côté Rust.
///
/// Choix assumé (2026-10-28) : pas de muxage en direct. Une seconde entrée
/// FFmpeg exigerait un pipe nommé Windows ; le live (RTMP) est de toute façon
/// refusé par le moteur et demandera un muxeur portant les PTS.
///
/// Le fichier d'origine est remplacé **atomiquement** : mux vers un fichier
/// temporaire, bascule, suppression du temporaire. Si la bascule échoue, le
/// fichier d'origine est restauré.
pub fn muxer_audio(
    chemin: &Path,
    pistes: &[crate::audio::piste::Piste],
) -> Result<(), ErreurSortie> {
    if pistes.is_empty() {
        return Ok(());
    }
    let binaire = chemin_sidecar().map_err(|e| ErreurSortie::Processus(e.to_string()))?;
    let extension = chemin
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "mp4".to_string());
    let nom = chemin
        .file_stem()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "sortie".to_string());
    let dossier = chemin.parent().unwrap_or_else(|| Path::new("."));
    let temporaire = dossier.join(format!("{nom}.audio.{extension}"));
    let sauvegarde = dossier.join(format!("{nom}.sans-audio.{extension}"));

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        chemin.to_string_lossy().to_string(),
    ];
    for piste in pistes {
        args.extend([
            "-f".into(),
            "s16le".into(),
            "-ar".into(),
            piste.taux.to_string(),
            "-ac".into(),
            piste.canaux.to_string(),
            "-i".into(),
            piste.chemin.to_string_lossy().to_string(),
        ]);
    }

    // Gains et mixage : `volume` par source puis `amix`. Une seule source ne
    // passe pas par `amix` (inutile et source de variantes de comportement).
    let mut filtre = String::new();
    for (i, piste) in pistes.iter().enumerate() {
        if !filtre.is_empty() {
            filtre.push(';');
        }
        filtre.push_str(&format!(
            "[{}:a]volume={:.3}[a{i}]",
            i + 1,
            piste.gain_milliemes as f32 / 1000.0
        ));
    }
    // `apad` : une piste plus courte que la vidéo est complétée par du
    // silence. Sans lui, `-shortest` couperait le fichier à la longueur de
    // l'audio (mesuré : 2,83 s de vidéo pour 3 s demandées).
    if pistes.len() == 1 {
        filtre.push_str(";[a0]apad[a]");
    } else {
        filtre.push(';');
        for i in 0..pistes.len() {
            filtre.push_str(&format!("[a{i}]"));
        }
        filtre.push_str(&format!(
            "amix=inputs={}:duration=longest:normalize=0[am];[am]apad[a]",
            pistes.len()
        ));
    }

    args.extend([
        "-filter_complex".into(),
        filtre,
        "-map".into(),
        "0:v".into(),
        "-map".into(),
        "[a]".into(),
        "-c:v".into(),
        "copy".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "192k".into(),
        // La vidéo fait foi : si la capture audio déborde (elle démarre un
        // peu avant la première frame), le fichier garde la durée vidéo.
        "-shortest".into(),
        temporaire.to_string_lossy().to_string(),
    ]);

    let sortie = Command::new(&binaire)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| ErreurSortie::Processus(format!("muxage : {e}")))?;
    if !sortie.status.success() {
        let _ = std::fs::remove_file(&temporaire);
        let detail = String::from_utf8_lossy(&sortie.stderr);
        return Err(ErreurSortie::Processus(format!(
            "muxage audio : statut {:?} — {}",
            sortie.status.code(),
            detail.lines().last().unwrap_or("").trim()
        )));
    }

    std::fs::rename(chemin, &sauvegarde)
        .map_err(|e| ErreurSortie::Processus(format!("sauvegarde : {e}")))?;
    match std::fs::rename(&temporaire, chemin) {
        Ok(()) => {
            let _ = std::fs::remove_file(&sauvegarde);
            Ok(())
        }
        Err(e) => {
            // Restaure l'original plutôt que de laisser l'utilisateur sans
            // fichier.
            let _ = std::fs::rename(&sauvegarde, chemin);
            let _ = std::fs::remove_file(&temporaire);
            Err(ErreurSortie::Processus(format!("bascule : {e}")))
        }
    }
}
