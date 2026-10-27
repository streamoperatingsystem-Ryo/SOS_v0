//! Énumération des moniteurs via DXGI.
//!
//! Pourquoi DXGI et pas `EnumDisplayMonitors` : on obtient dans la même passe
//! le `HMONITOR` (nécessaire à WGC), la géométrie du bureau virtuel et
//! **l'adaptateur qui possède l'écran** — indispensable sur un portable à
//! double GPU (le device D3D11 doit être créé sur cet adaptateur).
//!
//! Les sorties sont **dédupliquées par `HMONITOR`** : sur une machine hybride,
//! les deux GPU peuvent exposer le même écran.

use super::{ErreurCapture, Moniteur};

/// Énumère les moniteurs attachés au bureau, dédupliqués par `HMONITOR`.
///
/// Ordre : adaptateur puis sortie, tel que DXGI les expose.
#[cfg(windows)]
pub fn enumerer_moniteurs() -> Result<Vec<Moniteur>, ErreurCapture> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ERROR_NOT_FOUND,
    };

    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
        .map_err(|e| ErreurCapture::Enumeration(e.message()))?;

    let mut moniteurs: Vec<Moniteur> = Vec::new();
    let mut adaptateur_index = 0u32;

    loop {
        let adaptateur = match unsafe { factory.EnumAdapters1(adaptateur_index) } {
            Ok(a) => a,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => return Err(ErreurCapture::Enumeration(e.message())),
        };

        let mut sortie_index = 0u32;
        loop {
            let sortie = match unsafe { adaptateur.EnumOutputs(sortie_index) } {
                Ok(s) => s,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                // Une sortie en erreur ne doit pas invalider les autres.
                Err(_) => break,
            };

            let desc = match unsafe { sortie.GetDesc() } {
                Ok(d) => d,
                Err(_) => {
                    sortie_index += 1;
                    continue;
                }
            };

            // Écran non attaché au bureau (sortie fantôme) : inutilisable.
            if !desc.AttachedToDesktop.as_bool() {
                sortie_index += 1;
                continue;
            }

            let hmon = desc.Monitor.0 as isize;
            if moniteurs.iter().any(|m| m.hmon == hmon) {
                sortie_index += 1;
                continue;
            }

            let rect = desc.DesktopCoordinates;
            let largeur = (rect.right - rect.left).max(0) as u32;
            let hauteur = (rect.bottom - rect.top).max(0) as u32;
            if largeur == 0 || hauteur == 0 {
                sortie_index += 1;
                continue;
            }

            let primaire = rect.left <= 0
                && rect.top <= 0
                && rect.right > 0
                && rect.bottom > 0;

            moniteurs.push(Moniteur {
                index: moniteurs.len() as u32,
                adaptateur_index,
                nom: utf16z(&desc.DeviceName),
                x: rect.left,
                y: rect.top,
                largeur,
                hauteur,
                primaire,
                hmon,
            });

            sortie_index += 1;
        }

        adaptateur_index += 1;
    }

    if moniteurs.is_empty() {
        return Err(ErreurCapture::AucunMoniteur);
    }

    Ok(moniteurs)
}

/// Stub hors Windows.
#[cfg(not(windows))]
pub fn enumerer_moniteurs() -> Result<Vec<Moniteur>, ErreurCapture> {
    Err(ErreurCapture::NonSupporte)
}

#[cfg(windows)]
fn utf16z(desc: &[u16; 32]) -> String {
    let len = desc.iter().position(|&c| c == 0).unwrap_or(desc.len());
    String::from_utf16_lossy(&desc[..len])
}
