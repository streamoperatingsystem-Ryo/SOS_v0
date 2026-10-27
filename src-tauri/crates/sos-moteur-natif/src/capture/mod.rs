//! Capture de sources.
//!
//! LOT 2.2 : **moniteurs et fenêtres** (WGC). La caméra et les médias
//! arrivent aux lots suivants — ne pas élargir ce module sans mettre à jour
//! le plan.
//!
//! Invariant du moteur : une source = **une** session refcountée, partagée
//! par tous ses consommateurs (widgets, fond, aperçu, enregistrement).

pub mod enumeration;
pub mod wgc;

use std::fmt;

/// Un moniteur tel que le voit le moteur.
///
/// `hmon` est conservé sous forme d'`isize` (l'identité native du moniteur)
/// pour que la structure reste `Send + Sync` et sérialisable plus tard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moniteur {
    /// Index dans la liste dédupliquée du moteur.
    pub index: u32,
    /// Index DXGI de l'adaptateur qui expose ce moniteur.
    pub adaptateur_index: u32,
    /// Nom natif (`\\.\DISPLAY1`).
    pub nom: String,
    /// Position et taille dans le bureau virtuel, en pixels physiques.
    pub x: i32,
    pub y: i32,
    pub largeur: u32,
    pub hauteur: u32,
    /// `true` si le moniteur contient l'origine du bureau virtuel (0,0).
    pub primaire: bool,
    /// `HMONITOR` natif, opaque, nécessaire à WGC.
    pub hmon: isize,
}

impl Moniteur {
    /// Surface en pixels — sert au budget mémoire et au diagnostic.
    pub fn pixels(&self) -> u64 {
        self.largeur as u64 * self.hauteur as u64
    }
}

/// Erreurs de capture. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurCapture {
    /// Énumération impossible (DXGI).
    Enumeration(String),
    /// Création ou lecture de la session WGC.
    Wgc(String),
    /// Création du device / de la texture GPU.
    Gpu(String),
    /// Aucun moniteur exploitable.
    AucunMoniteur,
    /// Plateforme non supportée.
    NonSupporte,
}

impl fmt::Display for ErreurCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurCapture::Enumeration(m) => write!(f, "énumération : {m}"),
            ErreurCapture::Wgc(m) => write!(f, "WGC : {m}"),
            ErreurCapture::Gpu(m) => write!(f, "GPU : {m}"),
            ErreurCapture::AucunMoniteur => write!(f, "aucun moniteur"),
            ErreurCapture::NonSupporte => write!(f, "plateforme non supportée"),
        }
    }
}

impl std::error::Error for ErreurCapture {}
