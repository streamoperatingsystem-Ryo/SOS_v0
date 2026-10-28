//! Backend **NVENC direct** (SDK, zéro-copie) — promu du spike P0-a.
//!
//! Le SDK est chargé dynamiquement (`nvEncodeAPI64.dll`) : absent sur AMD /
//! Intel, `nouveau` échoue proprement et l'orchestrateur se replie sur MFT.
//! La texture NV12 du compositeur est enregistrée **une fois** puis réécrite
//! à chaque frame : aucune frame brute ne traverse la mémoire système.
//!
//! Contrat : `docs/features/moteur-natif.md` §11.3 (pas de B-frames, IDR à la
//! première frame et à chaque GOP).

pub mod ffi;

use std::ffi::c_void;
use std::ptr::null_mut;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

use crate::encode::{Codec, ErreurEncodage, PaquetEncode};
use crate::gpu::ContexteGpu;

use ffi::{struct_version, Image, InitializeParams, Sdk, GUID_CODEC_H264, GUID_PRESET_P4};

/// Image P (non-clé). L'IDR est `ffi::PIC_TYPE_IDR`.
const PIC_TYPE_P: u32 = 0x02;

/// Encodeur NVENC prêt à consommer une texture NV12 persistante.
pub struct EncodeurNvenc {
    sdk: Sdk,
    encoder: *mut c_void,
    ressource: *mut c_void,
    entree: *mut c_void,
    bitstream: *mut c_void,
    largeur: u32,
    hauteur: u32,
    index: u32,
    gop: u32,
}

impl EncodeurNvenc {
    /// `true` si le SDK NVENC est chargeable sur cette machine.
    pub fn disponible() -> bool {
        Sdk::charger().is_ok()
    }

    /// Ouvre une session sur le device D3D11 de la capture (zéro-copie).
    pub fn nouveau(
        contexte: &ContexteGpu,
        codec: Codec,
        largeur: u32,
        hauteur: u32,
        fps: u32,
        kbps: u32,
    ) -> Result<Self, ErreurEncodage> {
        if codec != Codec::H264 {
            return Err(ErreurEncodage::NonSupporte);
        }
        let largeur = largeur & !1;
        let hauteur = hauteur & !1;
        if largeur == 0 || hauteur == 0 || fps == 0 {
            return Err(ErreurEncodage::Conversion(
                "dimensions ou fps invalides".into(),
            ));
        }

        let sdk = Sdk::charger().map_err(ErreurEncodage::Nvenc)?;
        let encoder = sdk
            .ouvrir_session(contexte.device().as_raw())
            .map_err(ErreurEncodage::Nvenc)?;

        // Construit tôt : tout `?` ultérieur démonte proprement via `Drop`.
        let mut e = Self {
            sdk,
            encoder,
            ressource: null_mut(),
            entree: null_mut(),
            bitstream: null_mut(),
            largeur,
            hauteur,
            index: 0,
            gop: fps.saturating_mul(2).max(1),
        };

        let guids = e.sdk.codecs(encoder).map_err(ErreurEncodage::Nvenc)?;
        if !guids.contains(&GUID_CODEC_H264) {
            return Err(ErreurEncodage::AucunEncodeur(
                "NVENC sans H.264".to_string(),
            ));
        }

        let mut preset = e
            .sdk
            .config_preset(
                encoder,
                GUID_CODEC_H264,
                GUID_PRESET_P4,
                ffi::TUNING_ULTRA_LOW_LATENCY,
            )
            .map_err(ErreurEncodage::Nvenc)?;
        {
            let config = &mut preset.preset_cfg;
            config.gop_length = e.gop;
            config.frame_interval_p = 1;
            config.rc_params.rate_control_mode = ffi::RC_MODE_CBR;
            config.rc_params.average_bit_rate = kbps.saturating_mul(1000);
            config.rc_params.max_bit_rate = kbps.saturating_mul(1000);
            config.rc_params.vbv_buffer_size = kbps.saturating_mul(1000);
        }

        let mut params: InitializeParams = unsafe { std::mem::zeroed() };
        params.version = struct_version(e.sdk.api_version, ffi::VER_INITIALIZE) | (1u32 << 31);
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
        e.sdk
            .initialiser(encoder, &mut params)
            .map_err(ErreurEncodage::Nvenc)?;

        e.bitstream = e
            .sdk
            .creer_bitstream(encoder, 4 * 1024 * 1024)
            .map_err(ErreurEncodage::Nvenc)?;
        Ok(e)
    }

    /// Enregistre la texture NV12 persistante (une seule fois).
    pub fn enregistrer(&mut self, texture: &ID3D11Texture2D) -> Result<(), ErreurEncodage> {
        if !self.ressource.is_null() {
            return Ok(());
        }
        let ressource = self
            .sdk
            .enregistrer_texture(self.encoder, texture.as_raw(), self.largeur, self.hauteur)
            .map_err(ErreurEncodage::Nvenc)?;
        let entree = match self.sdk.mapper(self.encoder, ressource) {
            Ok(e) => e,
            Err(m) => {
                self.sdk.desenregistrer(self.encoder, ressource);
                return Err(ErreurEncodage::Nvenc(m));
            }
        };
        self.ressource = ressource;
        self.entree = entree;
        Ok(())
    }

    /// Encode la texture enregistrée. IDR à la première frame et chaque GOP.
    pub fn encoder_texture(
        &mut self,
        texture: &ID3D11Texture2D,
        pts_us: i64,
    ) -> Result<Vec<PaquetEncode>, ErreurEncodage> {
        self.enregistrer(texture)?;
        let index = self.index;
        self.index = self.index.wrapping_add(1);
        let cle = index == 0 || index % self.gop == 0;
        let image = Image {
            entree: self.entree,
            sortie: self.bitstream,
            largeur: self.largeur,
            hauteur: self.hauteur,
            index,
            pts_us,
        };
        let type_pic = if cle { ffi::PIC_TYPE_IDR } else { PIC_TYPE_P };
        let data = self
            .sdk
            .encoder_image_avec(self.encoder, image, type_pic)
            .map_err(ErreurEncodage::Nvenc)?;
        if data.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![PaquetEncode { data, pts_us, cle }])
        }
    }

    /// NVENC synchrone émet chaque frame dans `encoder_texture` : rien à
    /// vider en fin de flux.
    pub fn terminer(&mut self) -> Result<Vec<PaquetEncode>, ErreurEncodage> {
        Ok(Vec::new())
    }
}

impl Drop for EncodeurNvenc {
    fn drop(&mut self) {
        if !self.entree.is_null() {
            self.sdk.demapper(self.encoder, self.entree);
        }
        if !self.ressource.is_null() {
            self.sdk.desenregistrer(self.encoder, self.ressource);
        }
        if !self.bitstream.is_null() {
            self.sdk.liberer_bitstream(self.encoder, self.bitstream);
        }
        if !self.encoder.is_null() {
            self.sdk.detruire(self.encoder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_les_codecs_non_h264_sans_charger_le_sdk() {
        let contexte = match ContexteGpu::creer(None) {
            Ok(c) => c,
            Err(_) => return, // pas de GPU dans l'environnement de test
        };
        assert!(matches!(
            EncodeurNvenc::nouveau(&contexte, Codec::Hevc, 1280, 720, 60, 6000),
            Err(ErreurEncodage::NonSupporte)
        ));
    }

    #[test]
    fn refuse_les_dimensions_nulles() {
        let contexte = match ContexteGpu::creer(None) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert!(matches!(
            EncodeurNvenc::nouveau(&contexte, Codec::H264, 0, 720, 60, 6000),
            Err(ErreurEncodage::Conversion(_))
        ));
    }
}
