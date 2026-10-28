//! Spike **2.3** — compositeur GPU : sources → canvas NV12 → NVENC.
//!
//! Deux moniteurs + une fenêtre (premier plan) sont copiés chacun dans une
//! texture BGRA persistante (copie GPU, la texture WGC n'est jamais
//! conservée), composés en un seul `VideoProcessorBlt` (crop + échelle +
//! conversion NV12), puis encodés en H.264 zéro-copie.
//!
//! ```text
//! cargo run --release -p sos-moteur-natif --example compose_encode -- \
//!     [sortie.h264] [frames] [fps] [kbps]
//! ```

#[path = "nvenc/ffi.rs"]
mod ffi;

use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
use windows::core::Interface;

use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::capture::wgc::CaptureWgc;
use sos_moteur_natif::compositor::{Calque, Compositeur};
use sos_moteur_natif::gpu::{ContexteGpu, DescTexture, Format};

use ffi::{struct_version, InitializeParams, Sdk, GUID_CODEC_H264, GUID_PRESET_P4};

struct Source {
    nom: String,
    session: CaptureWgc,
    /// Copie GPU persistante de la dernière frame (BGRA).
    cible: ID3D11Texture2D,
    taille: (u32, u32),
    capturees: u32,
    prete: bool,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let sortie = args.next().unwrap_or_else(|| "compose-zc.h264".to_string());
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(300);
    let fps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(60);
    let kbps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(6000);

    println!("=== Spike 2.3 — compositeur GPU + NVENC ===");
    ffi::verifier_layout();

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
    let principal = moniteurs.iter().find(|m| m.primaire).unwrap_or(&moniteurs[0]);
    let largeur = principal.largeur & !1;
    let hauteur = principal.hauteur & !1;
    println!("Canvas : {largeur}x{hauteur} (moniteur {})", principal.index);

    let contexte = match ContexteGpu::creer(Some(principal.adaptateur_index)) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Contexte GPU impossible : {e}");
            return;
        }
    };

    let mut sources: Vec<Source> = Vec::new();
    for moniteur in moniteurs.iter().take(2) {
        match CaptureWgc::demarrer_avec(moniteur, contexte.clone()) {
            Ok(session) => ajouter_source(
                &mut sources,
                &contexte,
                format!("moniteur {}", moniteur.index),
                session,
            ),
            Err(e) => eprintln!("  moniteur {} : {e}", moniteur.index),
        }
    }
    if let Some((hwnd, nom)) = trouver_fenetre() {
        match CaptureWgc::demarrer_fenetre_avec(hwnd, contexte.clone()) {
            Ok(session) => ajouter_source(
                &mut sources,
                &contexte,
                format!("fenetre « {nom} »"),
                session,
            ),
            Err(e) => eprintln!("  fenetre « {nom} » : {e}"),
        }
    }
    if sources.is_empty() {
        eprintln!("Aucune source : arret.");
        return;
    }

    let mut compositeur = match Compositeur::nouveau(&contexte, largeur, hauteur, fps) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Compositeur impossible : {e}");
            return;
        }
    };
    println!("Compositeur : {} flux max", compositeur.max_flux());

    let encoder = match sdk.ouvrir_session(contexte.device().as_raw()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    println!("Session NVENC ouverte sur le device D3D11");

    match sdk.codecs(encoder) {
        Ok(guids) => {
            if !guids.contains(&GUID_CODEC_H264) {
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

    let mut preset = match sdk.config_preset(
        encoder,
        GUID_CODEC_H264,
        GUID_PRESET_P4,
        ffi::TUNING_ULTRA_LOW_LATENCY,
    ) {
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
    params.encode_config = &mut preset.preset_cfg;
    params.max_encode_width = largeur;
    params.max_encode_height = hauteur;
    params.tuning_info = ffi::TUNING_ULTRA_LOW_LATENCY;

    if let Err(e) = sdk.initialiser(encoder, &mut params) {
        eprintln!("{e}");
        sdk.detruire(encoder);
        return;
    }
    println!("Encodeur initialise");

    let ressource = match sdk.enregistrer_texture(
        encoder,
        compositeur.sortie().as_raw(),
        largeur,
        hauteur,
    ) {
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
            sdk.desenregistrer(encoder, ressource);
            sdk.detruire(encoder);
            return;
        }
    };
    println!("Texture de sortie enregistree et mappee (aucune copie CPU)");

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

    let mut fichier = match File::create(&sortie) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Creation {sortie} impossible : {e}");
            return;
        }
    };

    // Calques : 0 = plein canvas, 1 = tiers bas-droit, 2 = tiers haut-droit.
    let tiers_l = largeur - largeur / 3;
    let tiers_h = hauteur - hauteur / 3;
    let dest: [RECT; 3] = [
        RECT {
            left: 0,
            top: 0,
            right: largeur as i32,
            bottom: hauteur as i32,
        },
        RECT {
            left: tiers_l as i32,
            top: tiers_h as i32,
            right: largeur as i32,
            bottom: hauteur as i32,
        },
        RECT {
            left: tiers_l as i32,
            top: 0,
            right: largeur as i32,
            bottom: (hauteur / 3) as i32,
        },
    ];

    let intervalle = Duration::from_secs_f64(1.0 / fps as f64);
    let debut = Instant::now();
    let mut octets = 0usize;
    let mut paquets = 0usize;
    let mut sans_calque = 0u32;
    let mut en_retard = 0u32;
    let mut erreur: Option<String> = None;

    for i in 0..frames {
        let echeance = debut + intervalle * i;
        let maintenant = Instant::now();
        if maintenant < echeance {
            std::thread::sleep(echeance - maintenant);
        } else if i > 0 {
            en_retard += 1;
        }

        // Copie GPU de la dernière frame de chaque source (la texture WGC
        // n'est jamais conservée au-delà de cet appel).
        for s in sources.iter_mut() {
            if let Ok(t) = s.session.prochaine_texture(Duration::ZERO) {
                unsafe { contexte.context().CopyResource(&s.cible, &t) };
                s.capturees += 1;
                s.prete = true;
            }
        }

        let calques: Vec<Calque> = sources
            .iter()
            .enumerate()
            .filter(|(_, s)| s.prete)
            .map(|(idx, s)| {
                let (l, h) = s.taille;
                Calque {
                    texture: &s.cible,
                    source: RECT {
                        left: 0,
                        top: 0,
                        right: l as i32,
                        bottom: h as i32,
                    },
                    destination: dest[idx.min(2)],
                }
            })
            .collect();

        if calques.is_empty() {
            sans_calque += 1;
            continue;
        }

        if let Err(e) = compositeur.composer(&calques) {
            erreur = Some(format!("frame {i} : composition : {e}"));
            break;
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
                "\n{frames} frames en {} ms ({:.1} fps effectif)",
                duree.as_millis(),
                frames as f64 / duree.as_secs_f64().max(0.001)
            );
            println!(
                "{paquets} paquets, {octets} octets → {sortie} ({:.0} kbps)",
                (octets as f64 * 8.0) / (frames as f64 / fps as f64).max(0.001) / 1000.0
            );
            for s in &sources {
                println!("  {} : {} frames capturees", s.nom, s.capturees);
            }
            println!("  {sans_calque} frames sans calque, {en_retard} en retard");
            println!("Verifier : ffmpeg -i {sortie} -f null -");
        }
    }
}

fn ajouter_source(
    sources: &mut Vec<Source>,
    contexte: &ContexteGpu,
    nom: String,
    session: CaptureWgc,
) {
    let (l, h) = session.taille();
    let desc = match DescTexture::new(Format::Bgra8, l, h) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  {nom} : {e}");
            return;
        }
    };
    match contexte.creer_texture(&desc) {
        Ok(cible) => {
            println!("  {nom} : {l}x{h}");
            sources.push(Source {
                nom,
                session,
                cible,
                taille: (l, h),
                capturees: 0,
                prete: false,
            });
        }
        Err(e) => eprintln!("  {nom} : {e}"),
    }
}

/// Fenêtre cible : la fenêtre au premier plan.
fn trouver_fenetre() -> Option<(isize, String)> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    Some((hwnd.0 as isize, titre_fenetre(hwnd)))
}

fn titre_fenetre(hwnd: HWND) -> String {
    let mut tampon = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut tampon) };
    String::from_utf16_lossy(&tampon[..n.max(0) as usize])
}
