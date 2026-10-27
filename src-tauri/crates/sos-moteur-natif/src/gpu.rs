//! Détection du matériel graphique.
//!
//! Première brique du moteur : savoir **sur quel adaptateur** on va capturer,
//! composer et encoder. C'est aussi la source de vérité du diagnostic
//! « Moteur » (pilote graphique, VRAM, LUID) — cf.
//! `docs/features/moteur-natif.md` §16.
//!
//! Aucune valeur inventée : un adaptateur non énumérable produit une erreur,
//! jamais une entrée vide.

use std::fmt;

#[cfg(windows)]
use windows::core::Interface;

/// Un adaptateur DXGI tel que le voit le moteur.
///
/// `luid` est l'identité stable de la carte (elle ne change pas quand les
/// écrans sont branchés/débranchés) : c'est la clé d'agrégation des compteurs
/// PDH `pid_<PID>_luid_<LUID>_…` et de la persistance `moteur.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adaptateur {
    /// Index d'énumération DXGI (0 = premier adaptateur).
    pub index: u32,
    /// `VendorId` DXGI (`0x10DE` NVIDIA, `0x1002`/`0x1022` AMD, `0x8086` Intel…).
    pub vendor_id: u32,
    /// Nom lisible du constructeur.
    pub vendor_nom: &'static str,
    /// Nom commercial de la carte (ex. « NVIDIA GeForce RTX 4070 »).
    pub description: String,
    /// Mémoire vidéo dédiée, en Mio.
    pub vram_mio: u64,
    /// LUID 64 bits (`HighPart << 32 | LowPart`).
    pub luid: u64,
    /// `true` si DXGI signale un adaptateur logiciel (WARP, rendu logiciel).
    pub logiciel: bool,
}

impl Adaptateur {
    /// Un adaptateur logiciel ne peut pas encoder en matériel : le moteur
    /// doit le savoir avant de le choisir.
    pub fn materiel(&self) -> bool {
        !self.logiciel
    }
}

/// Erreurs de la couche GPU. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurGpu {
    /// DXGI a refusé l'opération (message natif conservé tel quel).
    Dxgi(String),
    /// Plateforme non supportée (le moteur est Windows-only en v1).
    NonSupporte,
}

impl fmt::Display for ErreurGpu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurGpu::Dxgi(m) => write!(f, "DXGI : {m}"),
            ErreurGpu::NonSupporte => write!(f, "plateforme non supportée"),
        }
    }
}

impl std::error::Error for ErreurGpu {}

/// Format de texture du moteur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// BGRA8 4:4:4 — sortie de WGC et des renderers.
    Bgra8,
    /// NV12 semi-planar 4:2:0 — entrée de l'encodeur.
    Nv12,
}

impl Format {
    /// Format DXGI correspondant.
    #[cfg(windows)]
    pub fn dxgi(self) -> windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT {
        use windows::Win32::Graphics::Dxgi::Common::{
            DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12,
        };
        match self {
            Format::Bgra8 => DXGI_FORMAT_B8G8R8A8_UNORM,
            Format::Nv12 => DXGI_FORMAT_NV12,
        }
    }

    /// `true` si le format est semi-planar (plan Y puis plan UV entrelacé).
    pub fn semi_planar(self) -> bool {
        matches!(self, Format::Nv12)
    }

    /// Taille d'une texture en octets (Y + UV pour NV12).
    pub fn taille_octets(self, largeur: u32, hauteur: u32) -> u64 {
        let pixels = largeur as u64 * hauteur as u64;
        match self {
            Format::Bgra8 => pixels * 4,
            Format::Nv12 => pixels + pixels / 2,
        }
    }
}

/// Description d'une texture du moteur (format + dimensions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescTexture {
    pub format: Format,
    pub largeur: u32,
    pub hauteur: u32,
}

impl DescTexture {
    /// Valide les dimensions : non nulles, et paires pour NV12 (4:2:0).
    pub fn new(format: Format, largeur: u32, hauteur: u32) -> Result<Self, ErreurGpu> {
        if largeur == 0 || hauteur == 0 {
            return Err(ErreurGpu::Dxgi("dimensions nulles".into()));
        }
        if format.semi_planar() && (largeur % 2 != 0 || hauteur % 2 != 0) {
            return Err(ErreurGpu::Dxgi(format!(
                "NV12 exige des dimensions paires : {largeur}x{hauteur}"
            )));
        }
        Ok(Self {
            format,
            largeur,
            hauteur,
        })
    }

    /// Taille de la texture en octets.
    pub fn taille_octets(&self) -> u64 {
        self.format.taille_octets(self.largeur, self.hauteur)
    }
}

/// Nom lisible d'un `VendorId` DXGI. Ne devine jamais : inconnu = « Inconnu ».
pub fn vendor_nom(vendor_id: u32) -> &'static str {
    match vendor_id {
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        0x1414 => "Microsoft",
        0x13B5 => "ARM",
        0x5143 => "Qualcomm",
        _ => "Inconnu",
    }
}

/// Énumère les adaptateurs DXGI, dans l'ordre du système.
///
/// `DXGI_ERROR_NOT_FOUND` termine la liste (cas normal) ; toute autre erreur
/// sur le premier adaptateur remonte en [`ErreurGpu::Dxgi`].
#[cfg(windows)]
pub fn enumerer_adaptateurs() -> Result<Vec<Adaptateur>, ErreurGpu> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|e| ErreurGpu::Dxgi(e.message()))?;

    let mut adaptateurs = Vec::new();
    let mut index = 0u32;

    loop {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(a) => a,
            // Fin de liste : c'est le cas normal, pas une erreur.
            Err(_) => break,
        };

        let desc = unsafe { adapter.GetDesc1() }.map_err(|e| ErreurGpu::Dxgi(e.message()))?;

        adaptateurs.push(Adaptateur {
            index,
            vendor_id: desc.VendorId,
            vendor_nom: vendor_nom(desc.VendorId),
            description: utf16z(&desc.Description),
            vram_mio: (desc.DedicatedVideoMemory as u64) / (1024 * 1024),
            luid: luid_vers_u64(desc.AdapterLuid),
            logiciel: desc.Flags & windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32
                != 0,
        });

        index += 1;
    }

    Ok(adaptateurs)
}

/// Stub hors Windows : le moteur natif est Windows-only en v1.
#[cfg(not(windows))]
pub fn enumerer_adaptateurs() -> Result<Vec<Adaptateur>, ErreurGpu> {
    Err(ErreurGpu::NonSupporte)
}

/// Ouvre un adaptateur DXGI par index.
///
/// Sert à créer le device D3D11 **sur l'adaptateur qui possède l'écran
/// capturé** : sur un portable à double GPU, un device créé sur l'autre
/// adaptateur impose une copie inter-GPU (cf. plan §6.3).
#[cfg(windows)]
pub fn ouvrir_adaptateur(
    index: u32,
) -> Result<windows::Win32::Graphics::Dxgi::IDXGIAdapter1, ErreurGpu> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|e| ErreurGpu::Dxgi(e.message()))?;
    unsafe { factory.EnumAdapters1(index) }.map_err(|e| ErreurGpu::Dxgi(e.message()))
}

/// Contexte GPU partagé : **un seul** device pour la capture, la composition
/// et l'encodage. C'est la condition du zéro-copie (plan §11).
///
/// Le device est créé sur l'adaptateur demandé (ou l'adaptateur par défaut)
/// avec le support BGRA exigé par WGC.
#[cfg(windows)]
pub struct ContexteGpu {
    device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    adaptateur_index: Option<u32>,
}

#[cfg(windows)]
impl ContexteGpu {
    /// Crée le device D3D11 (FL 11_0, BGRA).
    pub fn creer(adaptateur_index: Option<u32>) -> Result<Self, ErreurGpu> {
        use windows::Win32::Graphics::Direct3D::{
            D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0,
        };
        use windows::Win32::Graphics::Direct3D11::{
            D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG,
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
        };
        use windows::Win32::Graphics::Dxgi::IDXGIAdapter;

        let adaptateur: Option<IDXGIAdapter> = match adaptateur_index {
            Some(i) => {
                let a1 = ouvrir_adaptateur(i)?;
                Some(a1.cast().map_err(|e| ErreurGpu::Dxgi(e.message()))?)
            }
            None => None,
        };

        // Règle D3D11 : adaptateur explicite ⇒ D3D_DRIVER_TYPE_UNKNOWN.
        let driver_type = if adaptateur.is_some() {
            D3D_DRIVER_TYPE_UNKNOWN
        } else {
            D3D_DRIVER_TYPE_HARDWARE
        };

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        unsafe {
            D3D11CreateDevice(
                adaptateur.as_ref(),
                driver_type,
                None::<&windows::Win32::Foundation::HMODULE>,
                D3D11_CREATE_DEVICE_FLAG(
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT.0 | D3D11_CREATE_DEVICE_VIDEO_SUPPORT.0,
                ),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(|e| ErreurGpu::Dxgi(e.message()))?;

        Ok(Self {
            device: device.ok_or_else(|| ErreurGpu::Dxgi("device D3D11 nul".into()))?,
            context: context.ok_or_else(|| ErreurGpu::Dxgi("contexte D3D11 nul".into()))?,
            adaptateur_index,
        })
    }

    pub fn device(&self) -> &windows::Win32::Graphics::Direct3D11::ID3D11Device {
        &self.device
    }

    pub fn context(&self) -> &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext {
        &self.context
    }

    pub fn adaptateur_index(&self) -> Option<u32> {
        self.adaptateur_index
    }

    /// Crée une texture `DEFAULT` selon la description.
    ///
    /// Aucun accès CPU (chemin zéro-copie) ; bind `RENDER_TARGET`, qui sert
    /// de cible au video processor comme au compositeur.
    pub fn creer_texture(
        &self,
        desc: &DescTexture,
    ) -> Result<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D, ErreurGpu> {
        use windows::Win32::Graphics::Direct3D11::{
            ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        };
        use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

        let desc_d3d = D3D11_TEXTURE2D_DESC {
            Width: desc.largeur,
            Height: desc.hauteur,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.format.dxgi(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };

        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe { self.device.CreateTexture2D(&desc_d3d, None, Some(&mut texture)) }
            .map_err(|e| ErreurGpu::Dxgi(e.message()))?;
        texture.ok_or_else(|| ErreurGpu::Dxgi("texture nulle".into()))
    }
}

/// Pool de textures de même description, servi en round-robin.
///
/// Aucune synchronisation GPU : l'appelant garantit qu'une texture n'est
/// plus utilisée quand elle revient dans la rotation.
#[cfg(windows)]
pub struct PoolTextures {
    desc: DescTexture,
    textures: Vec<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>,
    suivant: usize,
}

#[cfg(windows)]
impl PoolTextures {
    /// Crée `nombre` textures identiques (au moins 1).
    pub fn nouveau(
        contexte: &ContexteGpu,
        desc: DescTexture,
        nombre: usize,
    ) -> Result<Self, ErreurGpu> {
        if nombre == 0 {
            return Err(ErreurGpu::Dxgi("pool vide".into()));
        }
        let mut textures = Vec::with_capacity(nombre);
        for _ in 0..nombre {
            textures.push(contexte.creer_texture(&desc)?);
        }
        Ok(Self {
            desc,
            textures,
            suivant: 0,
        })
    }

    /// Rend la texture suivante (rotation circulaire).
    pub fn suivante(&mut self) -> &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D {
        let index = self.suivant;
        self.suivant = (self.suivant + 1) % self.textures.len();
        &self.textures[index]
    }

    pub fn desc(&self) -> DescTexture {
        self.desc
    }

    pub fn len(&self) -> usize {
        self.textures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
}

/// Convertit une chaîne UTF-16 terminée par zéro (description DXGI).
#[cfg(windows)]
fn utf16z(desc: &[u16; 128]) -> String {
    let len = desc.iter().position(|&c| c == 0).unwrap_or(desc.len());
    String::from_utf16_lossy(&desc[..len])
}

/// Recompose un LUID 64 bits à partir de ses deux moitiés Win32.
#[cfg(windows)]
fn luid_vers_u64(luid: windows::Win32::Foundation::LUID) -> u64 {
    ((luid.HighPart as u32 as u64) << 32) | luid.LowPart as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendors_connus() {
        assert_eq!(vendor_nom(0x10DE), "NVIDIA");
        assert_eq!(vendor_nom(0x1002), "AMD");
        assert_eq!(vendor_nom(0x8086), "Intel");
        assert_eq!(vendor_nom(0x1414), "Microsoft");
    }

    #[test]
    fn vendor_inconnu_ne_devine_pas() {
        assert_eq!(vendor_nom(0xDEAD), "Inconnu");
    }

    #[test]
    fn adaptateur_logiciel_n_est_pas_materiel() {
        let a = Adaptateur {
            index: 0,
            vendor_id: 0x1414,
            vendor_nom: "Microsoft",
            description: "Microsoft Basic Render Driver".into(),
            vram_mio: 0,
            luid: 0,
            logiciel: true,
        };
        assert!(!a.materiel());
    }

    #[test]
    fn formats_dxgi_et_tailles() {
        assert_eq!(Format::Bgra8.taille_octets(1360, 768), 1360 * 768 * 4);
        assert_eq!(Format::Nv12.taille_octets(1360, 768), 1360 * 768 * 3 / 2);
        assert!(Format::Nv12.semi_planar());
        assert!(!Format::Bgra8.semi_planar());
    }

    #[test]
    fn desc_refuse_les_dimensions_invalides() {
        assert!(DescTexture::new(Format::Bgra8, 0, 768).is_err());
        assert!(DescTexture::new(Format::Nv12, 1361, 768).is_err());
        assert!(DescTexture::new(Format::Nv12, 1360, 768).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn pool_alterne_et_se_libere() {
        let Ok(contexte) = ContexteGpu::creer(None) else {
            eprintln!("pool : pas de device D3D11, test ignore");
            return;
        };
        let desc = DescTexture::new(Format::Nv12, 320, 240).expect("desc valide");
        let mut pool = PoolTextures::nouveau(&contexte, desc, 2).expect("pool");
        assert_eq!(pool.len(), 2);
        let a = pool.suivante().as_raw() as isize;
        let b = pool.suivante().as_raw() as isize;
        let c = pool.suivante().as_raw() as isize;
        assert_ne!(a, b);
        assert_eq!(a, c);
    }
}
