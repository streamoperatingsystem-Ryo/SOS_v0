//! Spike **0.2g-β** — encodage H.264 **zéro-copie** via le SDK NVENC.
//!
//! Capture WGC → texture BGRA → conversion NV12 **sur GPU** (video processor)
//! → `nvEncRegisterResource` sur **cette texture** → `nvEncEncodePicture`.
//! Aucune frame brute ne traverse la mémoire système : ni readback BGRA, ni
//! conversion CPU, ni pipe.
//!
//! ```text
//! cargo run --release -p sos-moteur-natif --example nvenc_encode -- \
//!     [moniteur] [sortie.h264] [frames] [fps] [kbps]
//! ```
//!
//! ⚠️ La texture NV12 est **persistante** (créée une fois, enregistrée une
//! fois) et réécrite à chaque frame par le video processor : les images
//! encodées sont vivantes. La mesure P0-a sur ce chemin rejoue le protocole
//! de `docs/features/moteur-natif/mesures-p0.md` (lot 0.2g-γ).

use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::Interface;

use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::capture::wgc::CaptureMoniteur;
use sos_moteur_natif::color_gpu::ConvertisseurNv12;
use sos_moteur_natif::gpu::ContexteGpu;

use sos_moteur_natif::encode::nvenc::ffi::{self, struct_version, InitializeParams, Sdk, GUID_CODEC_H264, GUID_PRESET_P4};

fn main() {
    let mut args = std::env::args().skip(1);
    let index_moniteur: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let sortie = args
        .next()
        .unwrap_or_else(|| "nvenc-zero-copie.h264".to_string());
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(60);
    let fps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(60);
    let kbps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(6000);

    println!("=== Spike 0.2g-beta — NVENC zero-copie ===");
    ffi::verifier_layout();
    println!("Layouts C verifies (offsets et tailles)");

    let sdk = match Sdk::charger() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SDK NVENC : {e}");
            return;
        }
    };
    println!("SDK charge (API 0x{:08x})", sdk.api_version);

    let moniteurs = match enumerer_moniteurs() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Enumeration impossible : {e}");
            return;
        }
    };
    let moniteur = match moniteurs.iter().find(|m| m.index == index_moniteur) {
        Some(m) => m,
        None => {
            eprintln!("Moniteur {index_moniteur} introuvable.");
            return;
        }
    };

    let contexte = match ContexteGpu::creer(Some(moniteur.adaptateur_index)) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Contexte GPU impossible : {e}");
            return;
        }
    };
    let capture = match CaptureMoniteur::demarrer_avec(moniteur, contexte.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Capture impossible : {e}");
            return;
        }
    };
    let (largeur, hauteur) = capture.taille();
    let largeur = largeur & !1;
    let hauteur = hauteur & !1;
    println!("Capture : {largeur}x{hauteur}");

    let encoder = match sdk.ouvrir_session(contexte.device().as_raw()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    println!("Session NVENC ouverte sur le device D3D11 de capture");

    match sdk.codecs(encoder) {
        Ok(guids) => {
            let h264 = guids.contains(&GUID_CODEC_H264);
            println!(
                "Codecs : {} — H.264 {}",
                guids.len(),
                if h264 { "present" } else { "ABSENT" }
            );
            if !h264 {
                eprintln!("H.264 absent : arret.");
                sdk.detruire(encoder);
                return;
            }
        }
        Err(e) => {
            eprintln!("{e}");
            sdk.detruire(encoder);
            return;
        }
    }

    let mut preset =
        match sdk.config_preset(encoder, GUID_CODEC_H264, GUID_PRESET_P4, ffi::TUNING_ULTRA_LOW_LATENCY)
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                sdk.detruire(encoder);
                return;
            }
        };
    {
        let config = &mut preset.preset_cfg;
        config.gop_length = fps.saturating_mul(2);
        config.frame_interval_p = 1;
        config.rc_params.rate_control_mode = ffi::RC_MODE_CBR;
        config.rc_params.average_bit_rate = kbps.saturating_mul(1000);
        config.rc_params.max_bit_rate = kbps.saturating_mul(1000);
        config.rc_params.vbv_buffer_size = kbps.saturating_mul(1000);
    }
    println!("Preset P4 / ultra-low-latency, CBR {kbps} kbps");

    let mut params: InitializeParams = unsafe { std::mem::zeroed() };
    params.version = struct_version(sdk.api_version, ffi::VER_INITIALIZE) | (1u32 << 31);
    params.encode_guid = GUID_CODEC_H264;
    params.preset_guid = GUID_PRESET_P4;
    params.encode_width = largeur;
    params.encode_height = hauteur;
    params.dar_width = largeur;
    params.dar_height = hauteur;
    params.frame_rate_num = fps;
    params.frame_rate_den = 1;
    params.enable_encode_async = 0;
    params.enable_ptd = 1;
    params.encode_config = &mut preset.preset_cfg;
    params.max_encode_width = largeur;
    params.max_encode_height = hauteur;
    params.tuning_info = ffi::TUNING_ULTRA_LOW_LATENCY;
    params.buffer_format = ffi::BUFFER_FORMAT_NV12;

    if let Err(e) = sdk.initialiser(encoder, &mut params) {
        eprintln!("{e}");
        sdk.detruire(encoder);
        return;
    }
    println!("Encodeur initialise");

    let convertisseur = match ConvertisseurNv12::nouveau(&contexte, largeur, hauteur, fps) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Video processor impossible : {e}");
            sdk.detruire(encoder);
            return;
        }
    };
    let texture_nv12 = match convertisseur.creer_cible() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Texture NV12 impossible : {e}");
            sdk.detruire(encoder);
            return;
        }
    };
    println!("Texture NV12 persistante creee (reecrite a chaque frame)");

    let ressource = match sdk.enregistrer_texture(encoder, texture_nv12.as_raw(), largeur, hauteur) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            sdk.detruire(encoder);
            return;
        }
    };
    let entree = match sdk.mapper(encoder, ressource) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            sdk.demapper(encoder, ressource);
            sdk.desenregistrer(encoder, ressource);
            sdk.detruire(encoder);
            return;
        }
    };
    println!("Texture enregistree et mappee (aucune copie CPU)");

    let bitstream = match sdk.creer_bitstream(encoder, 4 * 1024 * 1024) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            sdk.demapper(encoder, entree);
            sdk.desenregistrer(encoder, ressource);
            sdk.detruire(encoder);
            return;
        }
    };

    let mut fichier = match std::fs::File::create(&sortie) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Creation {sortie} impossible : {e}");
            return;
        }
    };

    let debut = Instant::now();
    let intervalle = Duration::from_secs_f64(1.0 / fps as f64);
    let mut octets = 0usize;
    let mut paquets = 0usize;
    let mut capturees = 0u32;
    let mut sans_maj = 0u32;
    let mut en_retard = 0u32;
    let mut erreur = None;

    for i in 0..frames {
        // Cadence : la boucle tourne à `fps`, indépendamment de WGC.
        let echeance = debut + intervalle * i;
        let maintenant = Instant::now();
        if maintenant < echeance {
            std::thread::sleep(echeance - maintenant);
        } else if i > 0 {
            en_retard += 1;
        }

        // WGC ne délivre des frames que sur changement d'écran : tentative
        // non bloquante. Sans frame, on ré-encode le contenu précédent —
        // compteur honnête, jamais un drop silencieux.
        match capture.prochaine_texture(Duration::ZERO) {
            Ok(texture) => match convertisseur.convertir_vers(&texture, &texture_nv12) {
                Ok(()) => capturees += 1,
                Err(e) => {
                    erreur = Some(format!("frame {i} : conversion : {e}"));
                    break;
                }
            },
            Err(_) => sans_maj += 1,
        }

        let pts = (i as i64 * 1_000_000) / fps as i64;
        match sdk.encoder_image(
            encoder,
            ffi::Image {
                entree,
                sortie: bitstream,
                largeur,
                hauteur,
                index: i,
                pts_us: pts,
            },
        ) {
            Ok(donnees) => {
                if donnees.is_empty() {
                    continue;
                }
                octets += donnees.len();
                paquets += 1;
                if let Err(e) = fichier.write_all(&donnees) {
                    erreur = Some(format!("ecriture : {e}"));
                    break;
                }
            }
            Err(e) => {
                erreur = Some(format!("frame {i} : {e}"));
                break;
            }
        }
    }
    let duree = debut.elapsed();
    let _ = fichier.flush();

    sdk.demapper(encoder, entree);
    sdk.desenregistrer(encoder, ressource);
    sdk.liberer_bitstream(encoder, bitstream);
    sdk.detruire(encoder);

    match erreur {
        Some(e) => eprintln!("ECHEC — {e}"),
        None => {
            println!(
                "\n{} frames en {} ms ({:.1} fps effectif)",
                frames,
                duree.as_millis(),
                frames as f64 / duree.as_secs_f64().max(0.001)
            );
            println!(
                "{paquets} paquets, {octets} octets → {sortie} ({:.0} kbps)",
                (octets as f64 * 8.0) / (frames as f64 / fps as f64).max(0.001) / 1000.0
            );
            println!(
                "Capture : {capturees}/{frames} frames — {sans_maj} sans mise a jour, {en_retard} en retard"
            );
            println!("Verifier : ffmpeg -i {sortie} -f null -");
        }
    }
}
