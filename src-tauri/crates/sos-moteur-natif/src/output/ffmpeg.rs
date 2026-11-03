//! Sink conteneurisé via le **sidecar FFmpeg** : bitstream → fMP4 / MKV
//! (fichier) ou → FLV / MPEG-TS (flux live RTMP / SRT).
//!
//! Le sidecar ne reçoit que le bitstream (`-c copy`), jamais de frame brute.
//! ⚠️ Un flux H.264/HEVC élémentaire ne transporte aucun horodatage. Le
//! défaut est donc d'**envelopper chaque paquet en MPEG-TS** avec le vrai
//! `PaquetEncode.pts_us` (`output/ts.rs`) : FFmpeg reçoit `-f mpegts` et
//! remuxe en fMP4/MKV sans rien inventer. La grille est tenue en plus par le
//! moteur, qui **réémet** la dernière frame quand un tick est manqué
//! (`moteur.rs`, compteur `frames_dupliquees`).
//!
//! ⚠️ **Ne pas revenir à `-use_wallclock_as_timestamps 1` par défaut** : les
//! PTS y sont les **temps d'arrivée** dans le démultiplexeur, quantifiés par
//! les rafales de lecture du pipe et par la file d'écriture — une session
//! réelle (46,79 fps mesurés, 824 ticks en retard) produisait une image qui
//! se fige puis saute. Le mode est conservé sous `SOS_PTS=1` pour l'A/B.
//!
//! ⚠️ **Le réhorodatage ne doit pas s'ajouter aux PTS réels** : il étirerait
//! une seconde fois (mesuré le 2026-11-03 : 11 460 frames / 448,9 s = 25,53
//! fps, cible 50 → ×1,96 → lecture au ralenti). `rehorodater` ne s'applique
//! donc **qu'au** mode `SOS_CFR=1`, qui ne duplique pas et dont la durée
//! dérive réellement.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

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
use super::ts::MuxeurTs;
use crate::audio::live::{
    chemin_pipe_audio, chemin_pipe_audio_ffmpeg, nom_pipe_audio, CANAUX_CIBLE, TAUX_CIBLE,
};
use crate::encode::ffmpeg::chemin_sidecar;
use crate::encode::PaquetEncode;
use sos_moteur_api::{Codec, ErreurMoteur};

/// Mode d'horodatage de la vidéo — **un seul point de vérité**, partagé par
/// le sink (arguments FFmpeg) et par le moteur (rattrapage CFR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Horodatage {
    /// Défaut : le vrai `pts_us` de chaque frame, enveloppé en **MPEG-TS**
    /// (`output/ts.rs`). Le moteur réémet en plus la dernière frame pour
    /// chaque tick manqué → grille uniforme **et** PTS fidèles.
    Ts,
    /// `SOS_TS=0` — A/B : bitstream brut + `-framerate` (grille CFR déduite
    /// du nombre de frames), avec rattrapage. Utile si le TS se comporte mal
    /// chez un utilisateur.
    CfrDuplique,
    /// `SOS_CFR=1` — A/B historique : CFR **sans** rattrapage, durée rétablie
    /// à l'arrêt par [`rehorodater`] (`setts`, étirement uniforme).
    Cfr,
    /// `SOS_PTS=1` — A/B : PTS = temps d'arrivée réels
    /// (`-use_wallclock_as_timestamps 1`). C'est ce mode qui produisait la
    /// saccade (PTS quantifiés par les rafales de lecture du pipe).
    Reel,
}

/// Mode courant, lu dans l'environnement à chaque appel (pas d'état figé :
/// l'A/B doit pouvoir changer sans relancer le moteur).
pub fn horodatage() -> Horodatage {
    if std::env::var_os("SOS_CFR").is_some() {
        Horodatage::Cfr
    } else if std::env::var_os("SOS_PTS").is_some() {
        Horodatage::Reel
    } else if std::env::var_os("SOS_TS").is_some_and(|v| v == "0") {
        Horodatage::CfrDuplique
    } else {
        Horodatage::Ts
    }
}

/// Le moteur doit-il réémettre les frames manquées ? Oui partout **sauf** en
/// `SOS_CFR=1` (le réhorodatage rétablit la durée, dupliquer l'étirerait) et
/// en `SOS_PTS=1` (on mesure le comportement d'avant).
pub fn rattrapage_cfr() -> bool {
    matches!(horodatage(), Horodatage::Ts | Horodatage::CfrDuplique)
}

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

/// Muxeur piloté par un processus FFmpeg (stdin = bitstream ou MPEG-TS).
pub struct FfmpegSink {
    enfant: Child,
    /// Stdin direct — `None` dès que le fil d'écriture le possède.
    entree: Option<ChildStdin>,
    /// Enveloppe MPEG-TS des paquets — `None` = bitstream brut (A/B
    /// `SOS_TS=0`, `SOS_CFR=1`, `SOS_PTS=1`, ou codec refusé en TS comme
    /// AV1). Voir [`Horodatage`].
    muxeur: Option<MuxeurTs>,
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

/// Plafond de la file d'écriture **audio**, en octets : 120 s de PCM 48 kHz
/// stéréo s16 (~23 Mio). FFmpeg draine ce pipe par rafales séparées de
/// plusieurs secondes ; au-delà du plafond le bloc le plus ancien est jeté et
/// **compté** (jamais silencieux).
const FILE_AUDIO_MAX: usize = TAUX_CIBLE as usize * CANAUX_CIBLE as usize * 2 * 120;

/// Attente entre deux tentatives d'écriture sur un pipe plein (`PIPE_NOWAIT`).
/// 2 ms : assez court pour ne pas retarder le drain, assez long pour ne pas
/// brûler un cœur quand FFmpeg retient l'audio.
const ATTENTE_PIPE: std::time::Duration = std::time::Duration::from_millis(2);

/// Délai maximal de vidage **après fermeture** avant d'abandonner un bloc
/// (FFmpeg figé) : l'arrêt d'un enregistrement ne doit jamais se figer.
/// 2 s : un drain sain vide sa file en quelques dizaines de ms ; au-delà
/// c'est un FFmpeg qui retient l'audio — le post-mux répare alors la piste.
/// Mesuré sur une session réelle : la piste live est systématiquement
/// incomplète, donc attendre 5 s ne sauvait rien et coûtait 3 s de plus à
/// chaque arrêt.
const DELAI_VIDAGE: std::time::Duration = std::time::Duration::from_secs(2);

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

        let mode = horodatage();
        // Le TS n'est utilisé que si le codec est supporté (AV1 → `None`,
        // chemin brut historique). `mode == Ts` et `muxeur.is_none()` ne
        // peuvent pas diverger : les deux dérivent de cette seule valeur.
        let muxeur = if mode == Horodatage::Ts {
            MuxeurTs::nouveau(codec)
        } else {
            None
        };
        let ts = muxeur.is_some();

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
        ];
        if ts {
            // Entrée TS : les PTS viennent de nos en-têtes PES, `-framerate`
            // n'a plus de sens. Fenêtre d'analyse **bornée** : sans elle
            // `avformat_find_stream_info` peut lire plusieurs secondes avant
            // d'écrire le premier octet. Elle reste largement suffisante —
            // PAT/PMT et SPS/PPS arrivent dans les premiers Ko.
            args.extend([
                "-f".into(),
                "mpegts".into(),
                "-analyzeduration".into(),
                "1000000".into(),
                "-probesize".into(),
                "1000000".into(),
            ]);
        } else {
            args.extend([
                "-f".into(),
                nom_demuxer(codec).to_string(),
                "-framerate".into(),
                fps.to_string(),
            ]);
        }
        // PTS réels = temps d'arrivée : **A/B uniquement** (`SOS_PTS=1`).
        // **Fichiers seulement** : un flux RTMP/SRT garde la CFR exigée par
        // les plateformes.
        if mode == Horodatage::Reel && matches!(cible, Sortie::Fichier { .. }) {
            args.push("-use_wallclock_as_timestamps".into());
            args.push("1".into());
        }
        args.extend(["-i".into(), "pipe:0".into()]);
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
            muxeur,
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
        // Enveloppe MPEG-TS (PTS par frame) ou bitstream brut selon le mode.
        // Le PTS du paquet n'est **jamais** perdu en mode TS : c'est la
        // raison d'être de `output/ts.rs`.
        let data = match self.muxeur.as_mut() {
            Some(muxeur) => muxeur.envelopper(paquet),
            None => paquet.data.clone(),
        };
        let taille = data.len();
        if let Some((tx, _)) = self.ecrivain.as_ref() {
            // File bornée + échéance : jamais de blocage indéfini de la
            // composition (voir `demarrer_ecrivain`). `send_timeout` est
            // instable → `try_send` + attente courte bornée.
            let debut = std::time::Instant::now();
            let mut bloc = Some(data);
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
                .write_all(&data)
                .map_err(|e| self.erreur_processus(format!("ecriture : {e}")))?;
        }
        self.octets += taille as u64;
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
/// `HANDLE` vit dans le fil d'écriture (transmis en `usize`, il n'est pas
/// `Send`) : `PipeAudio` ne garde que la file partagée, il est `Send`.
pub struct PipeAudio {
    /// État partagé avec le fil d'écriture (file bornée, compteurs, arrêt).
    partage: Arc<(Mutex<FileAudio>, Condvar)>,
    /// `\\.\pipe\sos-audio-<pid>-<n>` — affiché dans les erreurs.
    nom: String,
    fil: Option<JoinHandle<()>>,
}

/// File d'écriture du pipe audio — partagée entre la composition (dépôt) et
/// le fil dédié (écriture).
struct FileAudio {
    blocs: VecDeque<Vec<u8>>,
    octets: usize,
    ferme: bool,
    /// Horodatage de la fermeture : borne **globale** du vidage final.
    ferme_depuis: Option<std::time::Instant>,
    ecrits: u64,
    sautes: u64,
    erreur: Option<String>,
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
        // un tampon plein rend la main immédiatement : le fil d'écriture
        // **réessaie** (voir `ecrire_bloc`), la composition ne bloque jamais.
        let mode = PIPE_NOWAIT;
        if let Err(e) = unsafe { SetNamedPipeHandleState(handle, Some(&mode), None, None) } {
            // Non fatal : on garde le mode bloquant (comportement d'avant).
            log::warn!("pipe {chemin} : mode non bloquant refusé ({e})");
        }
        let partage = Arc::new((
            Mutex::new(FileAudio {
                blocs: VecDeque::new(),
                octets: 0,
                ferme: false,
                ferme_depuis: None,
                ecrits: 0,
                sautes: 0,
                erreur: None,
            }),
            Condvar::new(),
        ));
        let fil = {
            let partage = Arc::clone(&partage);
            // `HANDLE` n'est pas `Send` : on le transporte en `usize` et on le
            // reconstruit dans le fil, seul propriétaire du handle.
            let brut = handle.0 as usize;
            match std::thread::Builder::new()
                .name("sos-pipe-audio".to_string())
                .spawn(move || ecrire_file(brut, partage))
            {
                Ok(fil) => fil,
                Err(e) => {
                    unsafe {
                        let _ = CloseHandle(handle);
                    }
                    return Err(ErreurSortie::Processus(format!("fil audio {chemin} : {e}")));
                }
            }
        };
        Ok(Self {
            partage,
            nom: chemin.to_string(),
            fil: Some(fil),
        })
    }

    /// Dépose un bloc PCM dans la file du fil d'écriture — **jamais** bloquant
    /// et **jamais** perdu sous le plafond (`FILE_AUDIO_MAX`).
    ///
    /// ⚠️ FFmpeg draine ce pipe **par rafales** : mesuré, il lit ~1,2 s puis
    /// n'y touche plus pendant plusieurs secondes (le muxeur retient les
    /// paquets). L'ancien contrat « tampon plein → bloc sauté » réduisait donc
    /// la piste audio à la taille du tampon du pipe (1,55 s pour 320 s de
    /// vidéo, mesuré sur `StreamOS-2026-11-02_05-05-19.mp4`). Le bloc est
    /// maintenant **retenu** et écrit quand FFmpeg le veut.
    ///
    /// Une erreur réelle du pipe (client fermé) est mémorisée puis rendue au
    /// prochain dépôt : `moteur.rs` retire alors le pipe et le post-mux
    /// répare la piste.
    pub fn ecrire(&mut self, octets: &[u8]) -> Result<(), ErreurSortie> {
        if octets.is_empty() {
            return Ok(());
        }
        let (verrou, cond) = &*self.partage;
        let mut file = verrou.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = file.erreur.as_ref() {
            return Err(ErreurSortie::Processus(format!(
                "pipe audio {} : {e}",
                self.nom
            )));
        }
        // Borne mémoire : au-delà, le bloc le plus ancien est jeté et compté.
        while file.octets + octets.len() > FILE_AUDIO_MAX {
            match file.blocs.pop_front() {
                Some(vieux) => {
                    file.octets -= vieux.len();
                    file.sautes += 1;
                }
                None => break,
            }
        }
        file.blocs.push_back(octets.to_vec());
        file.octets += octets.len();
        cond.notify_one();
        Ok(())
    }

    /// Ferme le pipe (EOF audio) **et rend le bilan** de la session.
    ///
    /// Le moteur décide sur ces chiffres si la piste live est complète ou si
    /// le post-mux doit réparer (`muxer_audio`). Idempotent : `Drop` ne refait
    /// rien après.
    pub fn fermer(&mut self) -> BilanAudio {
        {
            let (verrou, cond) = &*self.partage;
            let mut file = verrou.lock().unwrap_or_else(|e| e.into_inner());
            if !file.ferme {
                file.ferme = true;
                file.ferme_depuis = Some(std::time::Instant::now());
            }
            cond.notify_all();
        }
        if let Some(fil) = self.fil.take() {
            let _ = fil.join();
        }
        let file = self
            .partage
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        BilanAudio {
            octets_ecrits: file.ecrits,
            blocs_sautes: file.sautes,
        }
    }
}

/// Bilan d'une session audio live (voir [`PipeAudio::fermer`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BilanAudio {
    /// Octets PCM réellement remis à FFmpeg.
    pub octets_ecrits: u64,
    /// Blocs abandonnés (file saturée, ou FFmpeg muet après la fermeture).
    pub blocs_sautes: u64,
}

/// Corps du fil d'écriture : vide la file vers le pipe. La composition ne
/// touche plus jamais `WriteFile` — un FFmpeg qui retient l'audio fait grandir
/// la file (bornée), jamais la latence de composition.
fn ecrire_file(brut: usize, partage: Arc<(Mutex<FileAudio>, Condvar)>) {
    let handle = HANDLE(brut as *mut core::ffi::c_void);
    let (verrou, cond) = &*partage;
    loop {
        let bloc = {
            let mut file = verrou.lock().unwrap_or_else(|e| e.into_inner());
            while file.blocs.is_empty() && !file.ferme {
                file = cond.wait(file).unwrap_or_else(|e| e.into_inner());
            }
            match file.blocs.pop_front() {
                Some(bloc) => {
                    file.octets -= bloc.len();
                    bloc
                }
                // Fermé et vidé : plus rien à écrire.
                None => break,
            }
        };
        match ecrire_bloc(handle, &bloc, verrou) {
            Ok(ecrits) => {
                let mut file = verrou.lock().unwrap_or_else(|e| e.into_inner());
                file.ecrits += ecrits as u64;
                if ecrits < bloc.len() {
                    file.sautes += 1;
                }
            }
            Err(e) => {
                let mut file = verrou.lock().unwrap_or_else(|e| e.into_inner());
                file.erreur = Some(e);
                file.blocs.clear();
                file.octets = 0;
                break;
            }
        }
    }
    // Fermer le handle = EOF audio pour FFmpeg.
    unsafe {
        let _ = CloseHandle(handle);
    }
    let file = verrou.lock().unwrap_or_else(|e| e.into_inner());
    if file.sautes > 0 {
        log::warn!(
            "pipe audio : {} octet(s) écrits, {} bloc(s) abandonné(s) (file saturée ou FFmpeg muet)",
            file.ecrits,
            file.sautes
        );
    }
}

/// Écrit un bloc entier, en réessayant tant que le tampon est plein.
///
/// `PIPE_NOWAIT` rend la main au lieu d'attendre (voir `creer`) : sur tampon
/// plein (`n == 0`) ou client pas encore connecté (`ERROR_PIPE_LISTENING`),
/// on **réessaie** après `ATTENTE_PIPE` — le bloc n'est plus jeté.
/// ⚠️ Comparer des `HRESULT` (`HRESULT_FROM_WIN32`), jamais le code Win32 nu :
/// `0x8007_00E7` ≠ `231`.
///
/// Seule exception, après la fermeture : au-delà de `DELAI_VIDAGE` le bloc est
/// abandonné (compté par l'appelant) pour que l'arrêt ne se fige jamais sur un
/// FFmpeg mort. La borne est **globale** (horodatée à la fermeture), pas par
/// bloc — sinon un FFmpeg muet ferait attendre 10 s par bloc.
fn ecrire_bloc(
    handle: HANDLE,
    bloc: &[u8],
    verrou: &Mutex<FileAudio>,
) -> Result<usize, String> {
    let mut ecrits = 0usize;
    while ecrits < bloc.len() {
        let mut n = 0u32;
        let bloque = match unsafe { WriteFile(handle, Some(&bloc[ecrits..]), Some(&mut n), None) } {
            Ok(()) => {
                if n == 0 {
                    true
                } else {
                    ecrits += n as usize;
                    false
                }
            }
            Err(e) => {
                if [
                    ERROR_PIPE_LISTENING,
                    ERROR_PIPE_NOT_CONNECTED,
                    ERROR_NO_DATA,
                    ERROR_BUSY,
                ]
                .iter()
                .any(|code| e.code() == windows::core::HRESULT::from_win32(code.0))
                {
                    true
                } else {
                    return Err(e.message().to_string());
                }
            }
        };
        if !bloque {
            continue;
        }
        let fermeture = verrou
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ferme_depuis;
        if fermeture.is_some_and(|t| t.elapsed() >= DELAI_VIDAGE) {
            break;
        }
        std::thread::sleep(ATTENTE_PIPE);
    }
    Ok(ecrits)
}

impl Drop for PipeAudio {
    fn drop(&mut self) {
        // Signale la fin, laisse le fil vider la file, puis l'attend : c'est
        // lui qui ferme le handle (EOF audio pour FFmpeg), après le dernier
        // bloc — y compris ceux que FFmpeg n'a pas encore voulu lire.
        let _ = self.fermer();
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

/// Réhorodate la piste **vidéo** d'un fichier finalisé au fps réellement tenu.
///
/// ⚠️ Le moteur n'écrit aucun horodatage : FFmpeg synthétise les PTS depuis
/// `-framerate <fps cible>`. Un moteur qui produit 48,6 fps pendant 395 s
/// donne donc un fichier de 320 s — **accéléré ×1,23** (mesuré sur
/// `StreamOS-2026-11-02_05-05-19.mp4` : 19 199 frames, 320,01 s pour 395 s
/// réelles). `setts` réécrit les PTS sans réencodage : la durée redevient
/// celle de la session, l'audio est copié tel quel.
///
/// Écart < 0,5 % → aucun passage (le fichier est déjà juste). Un écart plus
/// grand laisse la **saccade de mouvement** : la grille devient uniforme, le
/// timing par frame est perdu — c'est le lot « vrais PTS » (option B) qui le
/// traitera.
///
/// ⚠️ **Réservé à `SOS_CFR=1`** : dans le mode par défaut, la grille CFR est
/// tenue par le rattrapage du moteur et la durée est déjà juste — réétirer
/// doublerait l'effet. En `SOS_PTS=1`, les PTS sont déjà les temps d'arrivée
/// réels (mesuré : fichier 25,53 fps étiré ×1,96 → ralenti).
pub fn rehorodater(
    chemin: &Path,
    fps_cible: u32,
    frames: u64,
    ecoule: std::time::Duration,
) -> Result<(), ErreurSortie> {
    if horodatage() != Horodatage::Cfr {
        return Ok(());
    }
    let Some((num, den)) = facteur_rehorodatage(fps_cible, frames, ecoule) else {
        return Ok(());
    };

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
    let temporaire = dossier.join(format!("{nom}.timing.{extension}"));
    let sauvegarde = dossier.join(format!("{nom}.avant-timing.{extension}"));

    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        chemin.to_string_lossy().to_string(),
        "-c".into(),
        "copy".into(),
        // Vidéo seulement : l'audio porte ses propres PTS (temps réel) et ne
        // doit surtout pas être étiré avec l'image.
        "-bsf:v".into(),
        format!("setts=ts=TS*{num}/{den}"),
        temporaire.to_string_lossy().to_string(),
    ];
    let sortie = Command::new(&binaire)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| ErreurSortie::Processus(format!("réhorodatage : {e}")))?;
    if !sortie.status.success() {
        let _ = std::fs::remove_file(&temporaire);
        let detail = String::from_utf8_lossy(&sortie.stderr);
        return Err(ErreurSortie::Processus(format!(
            "réhorodatage : statut {:?} — {}",
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

/// Facteur `(num, den)` du réhorodatage — `None` si l'écart au fps cible est
/// négligeable (< 0,5 %) ou si la mesure est inexploitable.
///
/// Ratio = (fps cible × écoulé) / frames, en fraction **entière réduite** :
/// les expressions `setts` sont évaluées en i64, on garde donc les nombres
/// petits.
fn facteur_rehorodatage(
    fps_cible: u32,
    frames: u64,
    ecoule: std::time::Duration,
) -> Option<(u128, u128)> {
    let millis = ecoule.as_millis();
    if frames == 0 || fps_cible == 0 || millis == 0 {
        return None;
    }
    let mut num = millis * u128::from(fps_cible);
    let mut den = u128::from(frames) * 1000;
    let (mut a, mut b) = (num, den);
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    let pgcd = a.max(1);
    num /= pgcd;
    den /= pgcd;
    if ((num as f64 / den as f64) - 1.0).abs() < 0.005 {
        return None;
    }
    Some((num, den))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COMPTEUR: AtomicU32 = AtomicU32::new(0);

    /// Le facteur du réhorodatage porte les chiffres **mesurés** du fichier
    /// utilisateur : 19 199 frames en 395 s à 60 fps de cible → ×1,2344.
    #[test]
    fn facteur_rehorodatage_suit_le_fps_mesure() {
        let (num, den) = facteur_rehorodatage(60, 19_199, std::time::Duration::from_secs(395))
            .expect("écart de 23 % → réhorodatage");
        let facteur = num as f64 / den as f64;
        assert!((facteur - 1.2344).abs() < 0.001, "facteur {facteur}");
        // Durée du fichier après réhorodatage = frames × (1/fps_cible) × facteur.
        let duree = 19_199.0 / 60.0 * facteur;
        assert!((duree - 395.0).abs() < 0.5, "durée {duree}");
    }

    /// Un moteur qui tient sa cible ne déclenche **aucun** passage.
    #[test]
    fn facteur_rehorodatage_ignore_un_ecart_negligeable() {
        assert_eq!(
            facteur_rehorodatage(60, 1199, std::time::Duration::from_millis(20_000)),
            None
        );
        assert_eq!(facteur_rehorodatage(60, 0, std::time::Duration::from_secs(1)), None);
        assert_eq!(facteur_rehorodatage(0, 60, std::time::Duration::from_secs(1)), None);
    }

    /// Un bloc déposé **avant** que le client ne lise n'est pas perdu : c'est
    /// exactement le défaut mesuré (piste audio réduite à la taille du tampon
    /// du pipe, 1,55 s pour 320 s de vidéo).
    #[test]
    fn pipe_audio_ne_perd_pas_les_blocs_retardes() {
        let n = COMPTEUR.fetch_add(1, Ordering::Relaxed);
        let nom = format!("sos-test-audio-{}-{n}", std::process::id());
        let chemin = format!(r"\\.\pipe\{nom}");
        let mut pipe = PipeAudio::creer(&chemin).expect("pipe");

        // Client « FFmpeg » : il ne se connecte qu'après 200 ms.
        let client = chemin.clone();
        let lecteur = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let mut fichier = std::fs::File::open(&client).expect("client");
            let mut recu = Vec::new();
            fichier.read_to_end(&mut recu).expect("lecture");
            recu
        });

        let bloc = vec![7u8; 19_200]; // 100 ms de PCM 48 kHz stéréo s16
        for _ in 0..5 {
            pipe.ecrire(&bloc).expect("dépôt");
        }
        // La fermeture vide la file puis rend EOF au client.
        drop(pipe);

        let recu = lecteur.join().expect("fil lecteur");
        assert_eq!(recu.len(), 5 * bloc.len());
        assert!(recu.iter().all(|o| *o == 7));
    }
}
