//! Mesure de la passe BGRA → NV12 (voie A du texte, décision 2026-10-28).
//!
//! La voie A ajoute une passe plein écran : les calques sont composés dans un
//! canvas **BGRA**, le texte est dessiné par Direct2D dessus, puis une
//! conversion BGRA → NV12 alimente la texture que NVENC a enregistrée une
//! fois. Ce harnais mesure le coût de cette passe **avant** d'écrire du code
//! de texte (protocole : mesure d'abord).
//!
//! Usage :
//! `cargo run --release --example passe_nv12 -- [largeur] [hauteur] [itérations]`
//!
//! Mesure : horloge murale sur un lot de `n` opérations, terminée par une
//! synchronisation GPU (copie en texture staging + `Map`). La surcharge de la
//! synchronisation est mesurée à vide et retranchée.
//!
//! ⚠️ **Ne pas utiliser `D3D11_QUERY_TIMESTAMP` ici** : les `VideoProcessorBlt`
//! s'exécutent sur le moteur vidéo, distinct du pipeline 3D ; les requêtes de
//! timestamp rendent alors deux valeurs identiques (delta 0, vérifié sur
//! GTX 1070 Ti). Mesure mesurée fausse, pas de bug à chercher côté pilote.

use std::time::Instant;

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};

use sos_moteur_natif::color_gpu::ConvertisseurNv12;
use sos_moteur_natif::compositor::{Calque, Compositeur};
use sos_moteur_natif::gpu::{ContexteGpu, DescTexture, Format};

fn main() {
    let mut args = std::env::args().skip(1);
    let largeur: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1360);
    let hauteur: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(768);
    let iterations: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(200);

    let contexte = match ContexteGpu::creer(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("GPU indisponible : {e}");
            return;
        }
    };
    println!("passe BGRA → NV12 : {largeur}×{hauteur}, {iterations} itérations");

    let desc_bgra = DescTexture::new(Format::Bgra8, largeur, hauteur).expect("desc BGRA");
    let desc_nv12 = DescTexture::new(Format::Nv12, largeur, hauteur).expect("desc NV12");
    let canvas = contexte.creer_texture(&desc_bgra).expect("canvas BGRA");
    let nv12 = contexte.creer_texture(&desc_nv12).expect("cible NV12");

    // Gris moyen : contrôle de la conversion (Y attendu ≈ 126 en BT.709).
    remplir(&contexte, &canvas, largeur, hauteur, [128, 128, 128, 255]);

    let convertisseur = match ConvertisseurNv12::nouveau(&contexte, largeur, hauteur, 60) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("convertisseur : {e}");
            return;
        }
    };
    if let Err(e) = convertisseur.convertir_vers(&canvas, &nv12) {
        eprintln!("conversion : {e}");
        return;
    }
    let staging = match creer_staging(&contexte, largeur, hauteur) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("staging : {e}");
            return;
        }
    };
    match lire_y(&contexte, &staging, &nv12) {
        Ok(y) => println!("contrôle conversion : gris 128 → Y = {y} (attendu ≈ 126)"),
        Err(e) => println!("contrôle conversion impossible : {e}"),
    }

    // Référence : composition actuelle (2 calques → NV12).
    let source_a = contexte.creer_texture(&desc_bgra).expect("source A");
    let source_b = contexte.creer_texture(&desc_bgra).expect("source B");
    remplir(&contexte, &source_a, largeur, hauteur, [40, 20, 200, 255]);
    remplir(&contexte, &source_b, largeur, hauteur, [200, 120, 20, 255]);
    let mut compositeur = Compositeur::nouveau(&contexte, largeur, hauteur, 60).expect("compositeur");
    let plein = RECT {
        left: 0,
        top: 0,
        right: largeur as i32,
        bottom: hauteur as i32,
    };
    let moitie = RECT {
        left: 0,
        top: 0,
        right: (largeur / 2) as i32,
        bottom: (hauteur / 2) as i32,
    };
    let calques = [
        Calque {
            texture: &source_a,
            source: plein,
            destination: plein,
        },
        Calque {
            texture: &source_b,
            source: moitie,
            destination: moitie,
        },
    ];

    let ctx = contexte.context();
    let synchroniser = || {
        unsafe {
            ctx.CopyResource(&staging, &nv12);
            let mut mappe = D3D11_MAPPED_SUBRESOURCE::default();
            if ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mappe))
                .is_ok()
            {
                ctx.Unmap(&staging, 0);
            }
        }
    };

    // Échauffement (vues, upload pilote, caches).
    for _ in 0..10 {
        let _ = compositeur.composer(&calques);
        let _ = convertisseur.convertir_vers(&canvas, &nv12);
    }
    synchroniser();

    let surcharge = chrono(0, &synchroniser);

    mesure(
        "composition 2 calques → NV12 (actuel)",
        iterations,
        surcharge,
        &synchroniser,
        |_| {
            let _ = compositeur.composer(&calques);
        },
    );
    mesure(
        "conversion BGRA → NV12 (passe ajoutée)",
        iterations,
        surcharge,
        &synchroniser,
        |_| {
            let _ = convertisseur.convertir_vers(&canvas, &nv12);
        },
    );
    mesure(
        "composition + conversion (voie A)",
        iterations,
        surcharge,
        &synchroniser,
        |_| {
            let _ = compositeur.composer(&calques);
            let _ = convertisseur.convertir_vers(&canvas, &nv12);
        },
    );
    println!("budget 16,7 ms @60 fps — la passe ajoutée doit rester marginale");
}

/// Temps (ms) d'un lot de `n` opérations + synchronisation, surcharge déduite.
fn chrono(n: u32, synchroniser: &impl Fn()) -> f64 {
    let debut = Instant::now();
    for _ in 0..n {}
    synchroniser();
    debut.elapsed().as_secs_f64() * 1000.0
}

/// Cinq lots, on garde le meilleur (le plus stable).
fn mesure(
    nom: &str,
    n: u32,
    surcharge: f64,
    synchroniser: &impl Fn(),
    mut travail: impl FnMut(u32),
) {
    let mut meilleur = f64::MAX;
    for _ in 0..5 {
        let debut = Instant::now();
        for i in 0..n {
            travail(i);
        }
        synchroniser();
        let total = debut.elapsed().as_secs_f64() * 1000.0 - surcharge;
        if total >= 0.0 {
            meilleur = meilleur.min(total / n as f64);
        }
    }
    if meilleur == f64::MAX {
        println!("{nom} : mesure impossible");
    } else {
        println!("{nom:<40} {meilleur:6.3} ms/op");
    }
}

/// Remplit une texture BGRA par `UpdateSubresource`.
fn remplir(
    contexte: &ContexteGpu,
    texture: &ID3D11Texture2D,
    largeur: u32,
    hauteur: u32,
    bgra: [u8; 4],
) {
    let mut pixels = vec![0u8; (largeur * hauteur * 4) as usize];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&bgra);
    }
    unsafe {
        contexte.context().UpdateSubresource(
            texture,
            0,
            None,
            pixels.as_ptr() as *const std::ffi::c_void,
            largeur * 4,
            0,
        )
    };
}

fn creer_staging(
    contexte: &ContexteGpu,
    largeur: u32,
    hauteur: u32,
) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: largeur,
        Height: hauteur,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut staging: Option<ID3D11Texture2D> = None;
    unsafe { contexte.device().CreateTexture2D(&desc, None, Some(&mut staging)) }
        .map_err(|e| format!("CreateTexture2D : {}", e.message()))?;
    staging.ok_or_else(|| "staging nulle".to_string())
}

/// Lit le plan Y d'une texture NV12 (copie en staging puis `Map`).
fn lire_y(
    contexte: &ContexteGpu,
    staging: &ID3D11Texture2D,
    nv12: &ID3D11Texture2D,
) -> Result<u8, String> {
    let ctx: &ID3D11DeviceContext = contexte.context();
    unsafe {
        ctx.CopyResource(staging, nv12);
        let mut mappe = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mappe))
            .map_err(|e| format!("Map : {}", e.message()))?;
        let y = *(mappe.pData as *const u8).add(mappe.RowPitch as usize * 10 + 10);
        ctx.Unmap(staging, 0);
        Ok(y)
    }
}
