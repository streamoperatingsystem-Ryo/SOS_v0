//! Diagnostic **0.2f-α** — pourquoi les MFT matériels renvoient E_UNEXPECTED.
//!
//! Trois variantes sur le **même** encodeur, dans l'ordre :
//!
//! - **A. texture NV12 avec `BIND_RENDER_TARGET`** (sortie directe du video
//!   processor, `ConvertisseurNv12::convertir`). C'est la seule variante
//!   cohérente avec l'entrée déclarée par `EncodeurMft`, qui annonce
//!   `MF_SA_D3D11_BINDFLAGS = D3D11_BIND_RENDER_TARGET`.
//! - **B. texture NV12 sans bind flags** (`convertir_pour_encodeur`), ce que
//!   fait `encode_gpu.rs` — incohérente avec l'entrée déclarée.
//! - **C. mémoire NV12** (sample `MFCreateMemoryBuffer`), témoin : le rapport
//!   antérieur indique qu'elle échoue aussi.
//!
//! Aucune modification du crate : tout passe par l'API publique.
//!
//! ```text
//! cargo run --release -p sos-moteur-natif --example mft_diag -- [moniteur] [frames] [fps] [kbps]
//! ```
//!
//! Lecture du résultat :
//! - **A passe, B échoue** → cause = incohérence de bind flags ; correctif
//!   possible sans changer d'API (fournir la texture RT, ou déclarer 0).
//! - **A et B échouent, C échoue** → le MFT matériel refuse les deux sources ;
//!   relancer avec `SOS_MOTEUR_DEBUG=1` (trace des types/événements) et
//!   `SOS_MFT_NO_D3D=1` (sans gestionnaire D3D11) pour isoler.
//! - **A et B passent** → le blocage n'était pas le bind flag : le rapport
//!   antérieur était périmé.

use std::sync::Arc;
use std::time::Duration;

use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    eAVEncH264VLevel4_1, eAVEncH264VProfile_High, IMFActivate, IMFTransform, MFCreateMediaType,
    MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MPEG2_LEVEL, MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE, MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC,
    MF_TRANSFORM_ASYNC_UNLOCK,
};

use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::capture::wgc::CaptureMoniteur;
use sos_moteur_natif::color_gpu::ConvertisseurNv12;
use sos_moteur_natif::encode::detect::lister_tous;
use sos_moteur_natif::encode::mft::EncodeurMft;
use sos_moteur_natif::encode::PaquetEncode;
use sos_moteur_natif::encode::Codec;
use sos_moteur_natif::gpu::ContexteGpu;

/// Paramètres communs aux trois essais.
struct Params<'a> {
    activate: &'a IMFActivate,
    contexte: &'a ContexteGpu,
    largeur: u32,
    hauteur: u32,
    fps: u32,
    kbps: u32,
    frames: u32,
}

/// Résultat d'un essai.
struct Resultat {
    paquets: usize,
    octets: usize,
    asynchrone: bool,
}

type Essai = Result<Resultat, String>;

fn preparer(p: &Params) -> Result<EncodeurMft, String> {
    EncodeurMft::nouveau_avec(
        p.activate,
        Codec::H264,
        p.largeur,
        p.hauteur,
        p.fps,
        p.kbps,
        Some(p.contexte),
    )
    .map_err(|e| format!("initialisation : {e}"))
}

fn accumuler(liste: Vec<PaquetEncode>, r: &mut Resultat) {
    for paquet in liste {
        r.paquets += 1;
        r.octets += paquet.data.len();
    }
}

/// Encode `frames` images depuis une texture NV12.
fn essai_texture(p: &Params, texture: &ID3D11Texture2D) -> Essai {
    let mut enc = preparer(p)?;
    let mut r = Resultat {
        paquets: 0,
        octets: 0,
        asynchrone: enc.asynchrone(),
    };

    for i in 0..p.frames {
        let pts = (i as i64 * 1_000_000) / p.fps as i64;
        let liste = enc
            .encoder_texture(texture, pts)
            .map_err(|e| format!("frame {i} : {e}"))?;
        accumuler(liste, &mut r);
    }
    if let Ok(liste) = enc.terminer() {
        accumuler(liste, &mut r);
    }

    Ok(r)
}

/// Encode `frames` images depuis un buffer NV12 en mémoire système.
fn essai_memoire(p: &Params, nv12: &[u8]) -> Essai {
    let mut enc = preparer(p)?;
    let mut r = Resultat {
        paquets: 0,
        octets: 0,
        asynchrone: enc.asynchrone(),
    };

    for i in 0..p.frames {
        let pts = (i as i64 * 1_000_000) / p.fps as i64;
        let liste = enc.encoder(nv12, pts).map_err(|e| format!("frame {i} : {e}"))?;
        accumuler(liste, &mut r);
    }
    if let Ok(liste) = enc.terminer() {
        accumuler(liste, &mut r);
    }

    Ok(r)
}

fn pack2(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | b as u64
}

/// Rejoue la négociation de types du crate pour lire les infos de flux
/// **après** configuration (avant, `GetOutputStreamInfo` échoue).
///
/// Rend `Some(fournit_ses_samples)` si l'introspection a pu aboutir.
fn introspecter(
    activate: &IMFActivate,
    largeur: u32,
    hauteur: u32,
    fps: u32,
    kbps: u32,
) -> Option<bool> {
    let transform: IMFTransform = match unsafe { activate.ActivateObject() } {
        Ok(t) => t,
        Err(e) => {
            println!("introspection : ActivateObject a échoué ({})", e.message());
            return None;
        }
    };
    let attrs = match unsafe { transform.GetAttributes() } {
        Ok(a) => a,
        Err(e) => {
            println!("introspection : GetAttributes a échoué ({})", e.message());
            return None;
        }
    };
    let asynchrone = unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0);
    let d3d_aware = unsafe { attrs.GetUINT32(&MF_SA_D3D11_AWARE) }.unwrap_or(0);
    println!("MF_TRANSFORM_ASYNC = {asynchrone}, MF_SA_D3D11_AWARE = {d3d_aware}");
    if asynchrone != 0 {
        let _ = unsafe { attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) };
    }

    let sortie = match unsafe { MFCreateMediaType() } {
        Ok(t) => t,
        Err(_) => return None,
    };
    let _ = unsafe { sortie.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) };
    let _ = unsafe { sortie.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264) };
    let _ = unsafe { sortie.SetUINT32(&MF_MT_AVG_BITRATE, kbps.saturating_mul(1000)) };
    let _ = unsafe {
        sortie.SetUINT32(
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_Progressive.0 as u32,
        )
    };
    let _ = unsafe { sortie.SetUINT64(&MF_MT_FRAME_SIZE, pack2(largeur, hauteur)) };
    let _ = unsafe { sortie.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1)) };
    let _ = unsafe { sortie.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32) };
    let _ = unsafe { sortie.SetUINT32(&MF_MT_MPEG2_LEVEL, eAVEncH264VLevel4_1.0 as u32) };
    if let Err(e) = unsafe { transform.SetOutputType(0, &sortie, 0) } {
        println!("introspection : SetOutputType a échoué ({})", e.message());
        return None;
    }

    let entree = match unsafe { MFCreateMediaType() } {
        Ok(t) => t,
        Err(_) => return None,
    };
    let _ = unsafe { entree.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) };
    let _ = unsafe { entree.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12) };
    let _ = unsafe { entree.SetUINT64(&MF_MT_FRAME_SIZE, pack2(largeur, hauteur)) };
    let _ = unsafe { entree.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1)) };
    let _ = unsafe {
        entree.SetUINT32(
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_Progressive.0 as u32,
        )
    };
    if let Err(e) = unsafe { transform.SetInputType(0, &entree, 0) } {
        println!("introspection : SetInputType a échoué ({})", e.message());
        return None;
    }

    match unsafe { transform.GetOutputStreamInfo(0) } {
        Ok(info) => {
            let fournit = info.dwFlags
                & windows::Win32::Media::MediaFoundation::MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                != 0;
            println!(
                "Sortie : flags={:#x} cbSize={} cbAlignment={} → fournit ses samples : {}",
                info.dwFlags, info.cbSize, info.cbAlignment, fournit
            );
            Some(fournit)
        }
        Err(e) => {
            println!("introspection : GetOutputStreamInfo a échoué ({})", e.message());
            None
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let index_moniteur: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(10);
    let fps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(60);
    let kbps: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(6000);
    // Index dans la liste des MFT (0 = premier) et pilotage direct optionnel.
    let index_encodeur: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    // Adaptateur sur lequel créer le device de l'encodeur (variante E).
    let adaptateur_encodeur: Option<u32> = args.next().and_then(|v| v.parse().ok());
    // Résolution imposée à la variante E (ex. 1280x720), pour les MFT qui
    // refusent la taille native de l'écran.
    let resolution_test: Option<(u32, u32)> = args.next().and_then(|v| {
        let mut it = v.split('x');
        let w = it.next()?.parse().ok()?;
        let h = it.next()?.parse().ok()?;
        Some((w, h))
    });

    let moniteurs = match enumerer_moniteurs() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Énumération impossible : {e}");
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
    println!("=== Diagnostic MFT 0.2f-alpha ===");
    println!("Capture : {largeur}x{hauteur}, {frames} frames, {fps} fps, {kbps} kbps");

    let texture_bgra = match capture.prochaine_texture(Duration::from_secs(5)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Aucune texture : {e}");
            return;
        }
    };
    let convertisseur = match ConvertisseurNv12::nouveau(&contexte, largeur, hauteur, fps) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Video processor impossible : {e}");
            return;
        }
    };
    let nv12_rt = match convertisseur.convertir(&texture_bgra) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Conversion GPU (RT) impossible : {e}");
            return;
        }
    };
    let nv12_plain = match convertisseur.convertir_pour_encodeur(&texture_bgra) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Conversion GPU (plain) impossible : {e}");
            return;
        }
    };
    let memoire = vec![0u8; (largeur as usize * hauteur as usize * 3) / 2];

    let encodeurs = match lister_tous(Codec::H264) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Détection impossible : {e}");
            return;
        }
    };
    println!("\nMFT H.264 disponibles :");
    for (i, e) in encodeurs.iter().enumerate() {
        println!(
            "  [{i}] {} [{}]",
            e.nom,
            if e.materiel { "matériel" } else { "logiciel" }
        );
    }
    let choisi = match encodeurs.get(index_encodeur) {
        Some(e) => e,
        None => {
            eprintln!("Encodeur {index_encodeur} absent de la liste.");
            return;
        }
    };
    println!(
        "\nEncodeur testé : {} [matériel]\nEntrée déclarée par le crate : NV12 + MF_SA_D3D11_BINDFLAGS = BIND_RENDER_TARGET\n",
        choisi.nom
    );

    // --- Introspection : ce que le MFT déclare réellement ---
    let fournit_samples = introspecter(choisi.activate(), largeur, hauteur, fps, kbps);
    println!();

    let p = Params {
        activate: choisi.activate(),
        contexte: &contexte,
        largeur,
        hauteur,
        fps,
        kbps,
        frames,
    };

    let variantes: [(&str, Essai); 4] = [
        (
            "A. texture NV12 avec BIND_RENDER_TARGET (cohérente)",
            essai_texture(&p, &nv12_rt),
        ),
        (
            "B. texture NV12 sans bind flags (encode_gpu.rs)",
            essai_texture(&p, &nv12_plain),
        ),
        ("C. mémoire NV12 (témoin)", essai_memoire(&p, &memoire)),
        (
            "D. mémoire NV12 sans gestionnaire D3D11 (SOS_MFT_NO_D3D)",
            {
                std::env::set_var("SOS_MFT_NO_D3D", "1");
                let r = essai_memoire(&p, &memoire);
                std::env::remove_var("SOS_MFT_NO_D3D");
                r
            },
        ),
    ];

    let mut a_ok = false;
    let mut b_ok = false;
    let mut c_ok = false;
    let mut d_ok = false;
    for (nom, resultat) in variantes {
        match resultat {
            Ok(r) => {
                println!(
                    "{nom} : OK — {} paquets, {} octets, modèle {}",
                    r.paquets,
                    r.octets,
                    if r.asynchrone { "asynchrone" } else { "synchrone" }
                );
                match nom.chars().next() {
                    Some('A') => a_ok = true,
                    Some('B') => b_ok = true,
                    Some('C') => c_ok = true,
                    Some('D') => d_ok = true,
                    _ => {}
                }
            }
            Err(e) => println!("{nom} : ÉCHEC — {e}"),
        }
    }

    // --- Variante E : device créé sur un AUTRE adaptateur (test croisé) ---
    // Permet de faire tourner le même code (EncodeurMft asynchrone) sur le MFT
    // Intel avec un device Intel, sans capture : entrée mémoire uniquement.
    let mut e_ok = false;
    if let Some(index_adapt) = adaptateur_encodeur {
        let (l_e, h_e) = resolution_test.unwrap_or((largeur, hauteur));
        match ContexteGpu::creer(Some(index_adapt)) {
            Ok(ctx) => {
                let memoire_e = vec![0u8; (l_e as usize * h_e as usize * 3) / 2];
                let p_e = Params {
                    activate: choisi.activate(),
                    contexte: &ctx,
                    largeur: l_e,
                    hauteur: h_e,
                    fps,
                    kbps,
                    frames,
                };
                match essai_memoire(&p_e, &memoire_e) {
                    Ok(r) => {
                        println!(
                            "E. mémoire NV12 {l_e}x{h_e} sur device de l'adaptateur {index_adapt} : OK — {} paquets, {} octets, modèle {}",
                            r.paquets,
                            r.octets,
                            if r.asynchrone { "asynchrone" } else { "synchrone" }
                        );
                        e_ok = true;
                    }
                    Err(e) => println!(
                        "E. mémoire NV12 {l_e}x{h_e} sur device de l'adaptateur {index_adapt} : ÉCHEC — {e}"
                    ),
                }
            }
            Err(e) => println!("E. device adaptateur {index_adapt} impossible : {e}"),
        }
    }

    println!("\n=== Conclusion ===");
    if e_ok && !c_ok {
        println!("Le MFT encode en asynchrone sur un autre adaptateur (variante E) :");
        println!("la boucle pomper()/lire_sorties() est CORRECTE, le refus vient du MFT");
        println!("visé (adaptateur ou pilote), pas de notre code.");
    } else if a_ok && !b_ok {
        println!("Cause isolée : incohérence de bind flags. Fournir la texture");
        println!("BIND_RENDER_TARGET à encoder_texture (ou déclarer 0 sur l'entrée).");
    } else if a_ok && b_ok {
        println!("Les deux textures passent : le blocage antérieur n'était pas le bind flag.");
    } else if !a_ok && !b_ok && d_ok {
        println!("Le chemin mémoire passe SANS gestionnaire D3D11 : le gestionnaire");
        println!("(ou son ordre d'installation) est la cause. La texture, elle, exige");
        println!("le gestionnaire : voir MFT_MESSAGE_SET_D3D_MANAGER et ResetDevice.");
    } else if !a_ok && !b_ok && !c_ok && !d_ok {
        println!("Le MFT refuse les quatre sources : le blocage n'est ni la texture ni");
        println!("le bind flag, mais le traitement de la SORTIE asynchrone.");
        if fournit_samples == Some(true) {
            println!("Le MFT fournit ses propres samples (0x100) : le chemin de sortie");
            println!("doit passer pSample = NULL, et ne jamais allouer de buffer client.");
        }
        println!("Test croisé indisponible sur cette machine : le MFT Intel refuse");
        println!("SetD3DManager avec le device de capture (NVIDIA) et SetOutputType avec");
        println!("un device Intel (même en 1280x720). Un pilotage direct hors du crate a");
        println!("été tenté : MF_E_NOTACCEPTING puis ACCESS_VIOLATION, donc non concluant.");
    } else {
        println!("Résultat mixte — voir les lignes ci-dessus, aucune conclusion automatique.");
    }
}
