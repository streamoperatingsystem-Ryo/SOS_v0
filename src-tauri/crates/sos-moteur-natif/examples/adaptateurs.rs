//! Vérification matérielle du moteur natif.
//!
//! Affiche les adaptateurs DXGI détectés. Sert au LOT 0 (matrice matérielle)
//! et au diagnostic « Moteur » : aucune valeur inventée, tout vient de DXGI.
//!
//! ```text
//! cargo run -p sos-moteur-natif --example adaptateurs
//! ```

use sos_moteur_natif::gpu::{enumerer_adaptateurs, ErreurGpu};

fn main() {
    println!("sos-moteur-natif {}", sos_moteur_natif::version());

    match enumerer_adaptateurs() {
        Ok(adaptateurs) if adaptateurs.is_empty() => {
            eprintln!("Aucun adaptateur DXGI détecté.");
            std::process::exit(2);
        }
        Ok(adaptateurs) => {
            println!("{} adaptateur(s) détecté(s) :", adaptateurs.len());
            println!();
            println!(
                "{:<3} {:<10} {:<38} {:>8} {:<18} {}",
                "#", "Vendeur", "Carte", "VRAM", "LUID", "Type"
            );
            println!("{}", "-".repeat(92));
            for a in &adaptateurs {
                println!(
                    "{:<3} {:<10} {:<38} {:>6} Mio {:#018x} {}",
                    a.index,
                    a.vendor_nom,
                    tronquer(&a.description, 38),
                    a.vram_mio,
                    a.luid,
                    if a.materiel() { "matériel" } else { "logiciel" }
                );
            }
            println!();

            let materiels = adaptateurs.iter().filter(|a| a.materiel()).count();
            if materiels == 0 {
                eprintln!(
                    "⚠️  Aucun adaptateur matériel : l'encodage sera logiciel (dégradé)."
                );
            }
        }
        Err(ErreurGpu::NonSupporte) => {
            eprintln!("Plateforme non supportée (moteur natif = Windows x64).");
            std::process::exit(3);
        }
        Err(e) => {
            eprintln!("Échec de l'énumération DXGI : {e}");
            std::process::exit(1);
        }
    }
}

/// Tronque proprement une description trop longue pour l'affichage tabulaire.
fn tronquer(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
