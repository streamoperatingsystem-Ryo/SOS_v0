//! Vérification du **lot 2.2** — sessions de capture simultanées.
//!
//! Deux moniteurs (ou le seul disponible) + une fenêtre, en même temps.
//!
//! ```text
//! cargo run --release -p sos-moteur-natif --example capture_multi -- \
//!     [fragment_de_titre | secondes] [secondes]
//! ```

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextW, IsWindowVisible,
};

use sos_moteur_natif::capture::enumeration::enumerer_moniteurs;
use sos_moteur_natif::capture::wgc::{CaptureMoniteur, CaptureWgc};

fn main() {
    let mut args = std::env::args().skip(1);
    // Premier argument : un nombre = durée (fenêtre au premier plan) ;
    // sinon un fragment de titre, suivi éventuellement de la durée.
    let (filtre, secondes) = match args.next() {
        Some(premier) => match premier.parse::<u64>() {
            Ok(duree) => (None, duree),
            Err(_) => (
                Some(premier),
                args.next().and_then(|v| v.parse().ok()).unwrap_or(5),
            ),
        },
        None => (None, 5),
    };

    println!("=== Lot 2.2 — captures simultanees ===");

    let moniteurs = match enumerer_moniteurs() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Enumeration impossible : {e}");
            return;
        }
    };
    println!("Moniteurs detectes : {}", moniteurs.len());

    let mut sessions: Vec<(String, CaptureWgc)> = Vec::new();

    for moniteur in moniteurs.iter().take(2) {
        match CaptureMoniteur::demarrer(moniteur) {
            Ok(c) => {
                let (l, h) = c.taille();
                println!("  moniteur {} ({}) : {l}x{h}", moniteur.index, moniteur.nom);
                sessions.push((format!("moniteur {}", moniteur.index), c));
            }
            Err(e) => eprintln!("  moniteur {} : {e}", moniteur.index),
        }
    }
    if moniteurs.len() < 2 {
        println!(
            "  (un seul moniteur : le cas « 2 moniteurs » n'est pas demontrable sur cette machine)"
        );
    }

    match trouver_fenetre(filtre.as_deref()) {
        Some((hwnd, nom)) => match CaptureWgc::demarrer_fenetre(hwnd) {
            Ok(c) => {
                let (l, h) = c.taille();
                println!("  fenetre « {nom} » : {l}x{h}");
                sessions.push((format!("fenetre « {nom} »"), c));
            }
            Err(e) => eprintln!("  fenetre « {nom} » : {e}"),
        },
        None => eprintln!("  aucune fenetre trouvee"),
    }

    if sessions.is_empty() {
        eprintln!("Aucune session ouverte : arret.");
        return;
    }

    let debut = Instant::now();
    let mut compteurs = vec![0u32; sessions.len()];
    let mut tailles = vec![(0u32, 0u32); sessions.len()];

    while debut.elapsed() < Duration::from_secs(secondes) {
        for (i, (_, session)) in sessions.iter().enumerate() {
            if let Ok(texture) = session.prochaine_texture(Duration::ZERO) {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                unsafe { texture.GetDesc(&mut desc) };
                compteurs[i] += 1;
                tailles[i] = (desc.Width, desc.Height);
            }
        }
        std::thread::sleep(Duration::from_millis(8));
    }

    println!("\n{secondes} s — frames par session :");
    for (i, (nom, _)) in sessions.iter().enumerate() {
        let (l, h) = tailles[i];
        println!("  {nom} : {} frames ({l}x{h})", compteurs[i]);
    }
}

/// Fenêtre cible : premier titre contenant `filtre`, sinon premier plan.
fn trouver_fenetre(filtre: Option<&str>) -> Option<(isize, String)> {
    match filtre {
        Some(filtre) => {
            let mut recherche = Recherche {
                filtre: filtre.to_lowercase(),
                trouve: None,
            };
            unsafe {
                let _ = EnumWindows(
                    Some(rappel_enumeration),
                    LPARAM(&mut recherche as *mut Recherche as isize),
                );
            }
            recherche.trouve
        }
        None => {
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0.is_null() {
                return None;
            }
            Some((hwnd.0 as isize, titre_fenetre(hwnd)))
        }
    }
}

struct Recherche {
    filtre: String,
    trouve: Option<(isize, String)>,
}

unsafe extern "system" fn rappel_enumeration(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let recherche = &mut *(lparam.0 as *mut Recherche);
    if recherche.trouve.is_some() {
        return BOOL(0);
    }
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    let nom = titre_fenetre(hwnd);
    if nom.to_lowercase().contains(&recherche.filtre) {
        recherche.trouve = Some((hwnd.0 as isize, nom));
        return BOOL(0);
    }
    BOOL(1)
}

fn titre_fenetre(hwnd: HWND) -> String {
    let mut tampon = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut tampon) };
    String::from_utf16_lossy(&tampon[..n.max(0) as usize])
}
