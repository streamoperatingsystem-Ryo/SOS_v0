//! Validation audio WASAPI — capture N secondes → WAV 16 bits + RMS.
//!
//! ```text
//! cargo run --release -p sos-moteur-natif --example audio_capture -- \
//!     [sortie.wav] [secondes] [sortie|entree]
//! ```

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use sos_moteur_natif::audio::{Sens, SourceAudio};

fn main() {
    let mut args = std::env::args().skip(1);
    let chemin = args.next().unwrap_or_else(|| "audio.wav".to_string());
    let secondes: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(3);
    let sens = match args.next().as_deref() {
        Some("entree") | Some("micro") => Sens::Entree,
        _ => Sens::Sortie,
    };

    println!("=== Audio WASAPI ({sens:?}) ===");

    let ouverture = match sens {
        Sens::Sortie => SourceAudio::sortie(),
        Sens::Entree => SourceAudio::entree(),
    };
    let mut source = match ouverture {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Ouverture impossible : {e}");
            return;
        }
    };

    let fmt = source.format();
    println!(
        "Format : {} Hz, {} canaux, {}",
        fmt.taux,
        fmt.canaux,
        if fmt.flottant { "f32" } else { "pcm16" }
    );

    let debut = Instant::now();
    let mut echantillons: Vec<f32> = Vec::new();
    let mut paquets = 0u32;
    while debut.elapsed() < Duration::from_secs(secondes) {
        match source.lire(&mut echantillons) {
            Ok(0) => {}
            Ok(_) => paquets += 1,
            Err(e) => {
                eprintln!("Lecture : {e}");
                break;
            }
        }
    }
    source.arreter();

    let canaux = fmt.canaux.max(1) as usize;
    let frames = echantillons.len() / canaux;
    let mut somme = vec![0.0f64; canaux];
    let mut pic = vec![0.0f32; canaux];
    for (i, &s) in echantillons.iter().enumerate() {
        let c = i % canaux;
        somme[c] += f64::from(s) * f64::from(s);
        if s.abs() > pic[c] {
            pic[c] = s.abs();
        }
    }

    println!(
        "{frames} frames en {} ms ({paquets} paquets)",
        debut.elapsed().as_millis()
    );
    for c in 0..canaux {
        let rms = if frames > 0 {
            (somme[c] / frames as f64).sqrt()
        } else {
            0.0
        };
        println!(
            "  canal {c} : RMS {:.4} ({:.1} dBFS), pic {:.3}",
            rms,
            20.0 * rms.max(1e-9).log10(),
            pic[c]
        );
    }

    match ecrire_wav(Path::new(&chemin), &echantillons, fmt.taux, fmt.canaux) {
        Ok(()) => println!("WAV : {chemin}"),
        Err(e) => eprintln!("WAV impossible : {e}"),
    }
}

/// WAV PCM 16 bits, en-tête RIFF standard.
fn ecrire_wav(chemin: &Path, echantillons: &[f32], taux: u32, canaux: u16) -> std::io::Result<()> {
    let mut donnees: Vec<u8> = Vec::with_capacity(echantillons.len() * 2);
    for &s in echantillons {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        donnees.extend_from_slice(&v.to_le_bytes());
    }

    let octets = donnees.len() as u32;
    let bloc = canaux as u32 * 2;

    let mut fichier = File::create(chemin)?;
    fichier.write_all(b"RIFF")?;
    fichier.write_all(&(36 + octets).to_le_bytes())?;
    fichier.write_all(b"WAVEfmt ")?;
    fichier.write_all(&16u32.to_le_bytes())?;
    fichier.write_all(&1u16.to_le_bytes())?;
    fichier.write_all(&canaux.to_le_bytes())?;
    fichier.write_all(&taux.to_le_bytes())?;
    fichier.write_all(&(taux * bloc).to_le_bytes())?;
    fichier.write_all(&(bloc as u16).to_le_bytes())?;
    fichier.write_all(&16u16.to_le_bytes())?;
    fichier.write_all(b"data")?;
    fichier.write_all(&octets.to_le_bytes())?;
    fichier.write_all(&donnees)?;
    Ok(())
}
