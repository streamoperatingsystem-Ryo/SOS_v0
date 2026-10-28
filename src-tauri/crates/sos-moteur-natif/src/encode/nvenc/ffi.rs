//! FFI minimale du SDK NVENC (`nvEncodeAPI64.dll`), module du crate depuis le
//! lot 1.6 (auparavant inclus par les exemples via `#[path]`).
//!
//! Source des structures : en-tête officiel `nvEncodeAPI.h` de
//! `FFmpeg/nv-codec-headers` (API 13.1). Chaque structure ne déclare que les
//! champs utilisés, suivis d'une queue de remplissage **surdimensionnée** :
//! le SDK écrit au plus `sizeof` de SA structure, donc une queue plus grande
//! est sans effet, alors qu'une queue plus petite provoquerait un
//! débordement.
//!
//! ⚠️ Couche brute : les fonctions prennent des handles opaques et ne sont
//! pas `unsafe` (elles le sont de fait). L'API sûre est `EncodeurNvenc`.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
//!
//! ⚠️ Les offsets des champs que le SDK lit **doivent** correspondre au
//! layout C. Ils sont vérifiés par [`verifier_layout`], appelée au démarrage :
//! un décalage est signalé immédiatement au lieu de produire un encodage faux.

#![allow(dead_code)]

use std::ffi::c_void;
use std::ptr::null_mut;

use windows::core::GUID;

// --- Chargement de la DLL (sans la feature Win32_System_LibraryLoader) -----

extern "system" {
    fn LoadLibraryW(nom: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, nom: *const u8) -> *mut c_void;
}

// --- Versions --------------------------------------------------------------

pub const NVENCAPI_MAJOR_VERSION: u32 = 13;
pub const NVENCAPI_MINOR_VERSION: u32 = 1;

/// `NVENCAPI_VERSION` de l'en-tête (majeur | mineur << 24).
pub const fn nvencapi_version() -> u32 {
    NVENCAPI_MAJOR_VERSION | (NVENCAPI_MINOR_VERSION << 24)
}

/// `NVENCAPI_STRUCT_VERSION(ver)` pour une API donnée.
pub const fn struct_version(api: u32, ver: u32) -> u32 {
    api | (ver << 16) | (0x7 << 28)
}

pub const VER_CONFIG: u32 = 9;
pub const VER_PRESET_CONFIG: u32 = 5;
pub const VER_INITIALIZE: u32 = 7;
pub const VER_PIC: u32 = 7;
pub const VER_LOCK_BITSTREAM: u32 = 2;
pub const VER_MAP_INPUT: u32 = 4;
pub const VER_REGISTER_RESOURCE: u32 = 5;
pub const VER_CREATE_BITSTREAM: u32 = 1;
pub const VER_SESSION: u32 = 1;

// --- Constantes ------------------------------------------------------------

pub const DEVICE_TYPE_DIRECTX: u32 = 0x0;
pub const INPUT_RESOURCE_TYPE_DIRECTX: u32 = 0x0;
pub const BUFFER_USAGE_NONE: u32 = 0x0;
pub const BUFFER_FORMAT_NV12: u32 = 0x1;
pub const TUNING_ULTRA_LOW_LATENCY: u32 = 3;
pub const PIC_STRUCT_FRAME: u32 = 0x01;
pub const PIC_TYPE_IDR: u32 = 0x03;
pub const RC_MODE_CONSTQP: u32 = 0x0;
/// ⚠️ 0x1 = **VBR** dans l'enum 13.x (`CONSTQP 0x0 / VBR 0x1 / CBR 0x2`) ;
/// les modes CBR_LOWDELAY/CBR_HQ/VBR_HQ n'existent plus.
pub const RC_MODE_CBR: u32 = 0x2;
pub const MEMORY_HEAP_DEFAULT: u32 = 0x0;

pub const GUID_CODEC_H264: GUID = GUID {
    data1: 0x6bc8_2762,
    data2: 0x4e63,
    data3: 0x4ca4,
    data4: [0xaa, 0x85, 0x1e, 0x50, 0xf3, 0x21, 0xf6, 0xbf],
};
pub const GUID_PRESET_P4: GUID = GUID {
    data1: 0x90a7_b826,
    data2: 0xdf06,
    data3: 0x4862,
    data4: [0xb9, 0xd2, 0xcd, 0x6d, 0x73, 0xa0, 0x86, 0x81],
};

// --- Structures ------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Qp {
    pub inter_p: u32,
    pub inter_b: u32,
    pub intra: u32,
}

/// `NV_ENC_RC_PARAMS` — 128 octets en C. Sa taille exacte est **critique** :
/// elle détermine l'offset de l'union de codec dans `NV_ENC_CONFIG`.
#[repr(C)]
pub struct RcParams {
    pub version: u32,
    pub rate_control_mode: u32,
    pub const_qp: Qp,
    pub average_bit_rate: u32,
    pub max_bit_rate: u32,
    pub vbv_buffer_size: u32,
    pub vbv_initial_delay: u32,
    pub bitfields: u32,
    pub min_qp: Qp,
    pub max_qp: Qp,
    pub initial_rc_qp: Qp,
    pub temporal_layer_idx_mask: u32,
    pub temporal_layer_qp: [u8; 8],
    pub target_quality: u8,
    pub target_quality_lsb: u8,
    pub lookahead_depth: u16,
    pub low_delay_key_frame_scale: u8,
    pub y_dc_qp_index_offset: i8,
    pub u_dc_qp_index_offset: i8,
    pub v_dc_qp_index_offset: i8,
    pub qp_map_mode: u32,
    pub multi_pass: u32,
    pub alpha_layer_bitrate_ratio: u32,
    pub cb_qp_index_offset: i8,
    pub cr_qp_index_offset: i8,
    pub reserved2: u16,
    pub lookahead_level: u32,
    pub view_bitrate_ratios: [u8; 7],
    pub reserved3: u8,
    pub reserved1: u32,
}

/// `NV_ENC_CONFIG` — `encode_codec_config` est une union (≈1,7 Ko) suivie de
/// réservés : on la remplace par une queue large.
#[repr(C)]
pub struct Config {
    pub version: u32,
    pub profile_guid: GUID,
    pub gop_length: u32,
    pub frame_interval_p: i32,
    pub mono_chrome_encoding: u32,
    pub frame_field_mode: u32,
    pub mv_precision: u32,
    pub rc_params: RcParams,
    pub encode_codec_config: [u64; 1024],
}

/// `NV_ENC_PRESET_CONFIG` — ce que remplit `nvEncGetEncodePresetConfigEx`.
/// ⚠️ Ce n'est PAS un `NV_ENC_CONFIG` : le champ `presetCfg` est imbriqué.
#[repr(C)]
pub struct PresetConfig {
    pub version: u32,
    pub preset_cfg: Config,
    pub reste: [u64; 512],
}

/// `NVENC_EXTERNAL_ME_HINT_COUNTS_PER_BLOCKTYPE` — **16 octets** en C : un
/// mot de bitfields (numCandsPerBlk…/numCandsPerSb/reserved) puis
/// `reserved1[3]`. Ces 12 octets réservés placent `tuningInfo` à l'offset
/// 136 ; les prendre pour un `[u32; 2]` (8 octets) est la cause du refus
/// « Presets P1-P7 … tuningInfo » (2026-10-27).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MeHintCountsPerBlock {
    pub bitfields: u32,
    pub reserved1: [u32; 3],
}

#[repr(C)]
pub struct InitializeParams {
    pub version: u32,
    pub encode_guid: GUID,
    pub preset_guid: GUID,
    pub encode_width: u32,
    pub encode_height: u32,
    pub dar_width: u32,
    pub dar_height: u32,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub enable_encode_async: u32,
    pub enable_ptd: u32,
    pub bitfields: u32,
    pub priv_data_size: u32,
    pub reserved: u32,
    pub priv_data: *mut c_void,
    pub encode_config: *mut Config,
    pub max_encode_width: u32,
    pub max_encode_height: u32,
    pub max_me_hint_counts_per_block: [MeHintCountsPerBlock; 2],
    pub tuning_info: u32,
    pub buffer_format: u32,
    pub num_state_buffers: u32,
    pub output_stats_level: u32,
    pub reste: [u64; 512],
}

#[repr(C)]
pub struct PicParams {
    pub version: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub input_pitch: u32,
    pub encode_pic_flags: u32,
    pub frame_idx: u32,
    pub input_time_stamp: u64,
    pub input_duration: u64,
    pub input_buffer: *mut c_void,
    pub output_bitstream: *mut c_void,
    pub completion_event: *mut c_void,
    pub buffer_fmt: u32,
    pub picture_struct: u32,
    pub picture_type: u32,
    pub reste: [u64; 512],
}

#[repr(C)]
pub struct RegisterResource {
    pub version: u32,
    pub resource_type: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub sub_resource_index: u32,
    pub resource_to_register: *mut c_void,
    pub registered_resource: *mut c_void,
    pub buffer_format: u32,
    pub buffer_usage: u32,
    pub input_fence_point: *mut c_void,
    pub chroma_offset: [u32; 2],
    pub chroma_offset_in: [u32; 2],
    pub reste: [u64; 256],
}

#[repr(C)]
pub struct MapInputResource {
    pub version: u32,
    pub sub_resource_index: u32,
    pub input_resource: *mut c_void,
    pub registered_resource: *mut c_void,
    pub mapped_resource: *mut c_void,
    pub mapped_buffer_fmt: u32,
    pub reste: [u64; 256],
}

#[repr(C)]
pub struct CreateBitstreamBuffer {
    pub version: u32,
    pub size: u32,
    pub memory_heap: u32,
    pub reserved: u32,
    pub bitstream_buffer: *mut c_void,
    pub bitstream_buffer_ptr: *mut c_void,
    pub reste: [u64; 128],
}

#[repr(C)]
pub struct LockBitstream {
    pub version: u32,
    pub bitfields: u32,
    pub output_bitstream: *mut c_void,
    pub slice_offsets: *mut u32,
    pub frame_idx: u32,
    pub hw_encode_status: u32,
    pub num_slices: u32,
    pub bitstream_size_in_bytes: u32,
    pub output_time_stamp: u64,
    pub output_duration: u64,
    pub bitstream_buffer_ptr: *mut c_void,
    pub picture_type: u32,
    pub picture_struct: u32,
    pub frame_avg_qp: u32,
    pub frame_satd: u32,
    pub ltr_frame_idx: u32,
    pub ltr_frame_bitmap: u32,
    pub temporal_id: u32,
    pub intra_mb_count: u32,
    pub inter_mb_count: u32,
    pub average_mvx: i32,
    pub average_mvy: i32,
    pub alpha_layer_size_in_bytes: u32,
    pub output_stats_ptr_size: u32,
    pub reserved: u32,
    pub output_stats_ptr: *mut c_void,
    pub frame_idx_display: u32,
    pub reste: [u64; 256],
}

#[repr(C)]
pub struct SessionParams {
    pub version: u32,
    pub device_type: u32,
    pub device: *mut c_void,
    pub reserved: *mut c_void,
    pub api_version: u32,
    pub reserved1: [u32; 253],
    pub reserved2: [*mut c_void; 64],
}

// --- Liste de fonctions ----------------------------------------------------

type FnGetMaxVersion = unsafe extern "system" fn(*mut u32) -> i32;
type FnCreateInstance = unsafe extern "system" fn(*mut FunctionList) -> i32;
type FnOpenSessionEx = unsafe extern "system" fn(*mut SessionParams, *mut *mut c_void) -> i32;
type FnGetGuidCount = unsafe extern "system" fn(*mut c_void, *mut u32) -> i32;
type FnGetGuids = unsafe extern "system" fn(*mut c_void, *mut GUID, u32, *mut u32) -> i32;
type FnGetPresetConfigEx =
    unsafe extern "system" fn(*mut c_void, GUID, GUID, u32, *mut PresetConfig) -> i32;
type FnInitializeEncoder = unsafe extern "system" fn(*mut c_void, *mut InitializeParams) -> i32;
type FnRegisterResource = unsafe extern "system" fn(*mut c_void, *mut RegisterResource) -> i32;
type FnUnregisterResource = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
type FnMapInputResource = unsafe extern "system" fn(*mut c_void, *mut MapInputResource) -> i32;
type FnUnmapInputResource = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
type FnCreateBitstream = unsafe extern "system" fn(*mut c_void, *mut CreateBitstreamBuffer) -> i32;
type FnDestroyBitstream = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
type FnEncodePicture = unsafe extern "system" fn(*mut c_void, *mut PicParams) -> i32;
type FnLockBitstream = unsafe extern "system" fn(*mut c_void, *mut LockBitstream) -> i32;
type FnUnlockBitstream = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
type FnDestroyEncoder = unsafe extern "system" fn(*mut c_void) -> i32;
type FnGetLastError = unsafe extern "system" fn(*mut c_void) -> *const i8;

/// Une image à encoder : entrée mappée, bitstream de sortie, horodatage.
#[derive(Clone, Copy)]
pub struct Image {
    pub entree: *mut c_void,
    pub sortie: *mut c_void,
    pub largeur: u32,
    pub hauteur: u32,
    pub index: u32,
    pub pts_us: i64,
}

/// `NV_ENCODE_API_FUNCTION_LIST` — 43 pointeurs + `reserved2[275]`.
#[repr(C)]
pub struct FunctionList {
    pub version: u32,
    pub reserved: u32,
    pub open_encode_session: *mut c_void,
    pub get_encode_guid_count: *mut c_void,
    pub get_encode_profile_guid_count: *mut c_void,
    pub get_encode_profile_guids: *mut c_void,
    pub get_encode_guids: *mut c_void,
    pub get_input_format_count: *mut c_void,
    pub get_input_formats: *mut c_void,
    pub get_encode_caps: *mut c_void,
    pub get_encode_preset_count: *mut c_void,
    pub get_encode_preset_guids: *mut c_void,
    pub get_encode_preset_config: *mut c_void,
    pub initialize_encoder: *mut c_void,
    pub create_input_buffer: *mut c_void,
    pub destroy_input_buffer: *mut c_void,
    pub create_bitstream_buffer: *mut c_void,
    pub destroy_bitstream_buffer: *mut c_void,
    pub encode_picture: *mut c_void,
    pub lock_bitstream: *mut c_void,
    pub unlock_bitstream: *mut c_void,
    pub lock_input_buffer: *mut c_void,
    pub unlock_input_buffer: *mut c_void,
    pub get_encode_stats: *mut c_void,
    pub get_sequence_params: *mut c_void,
    pub register_async_event: *mut c_void,
    pub unregister_async_event: *mut c_void,
    pub map_input_resource: *mut c_void,
    pub unmap_input_resource: *mut c_void,
    pub destroy_encoder: *mut c_void,
    pub invalidate_ref_frames: *mut c_void,
    pub open_encode_session_ex: *mut c_void,
    pub register_resource: *mut c_void,
    pub unregister_resource: *mut c_void,
    pub reconfigure_encoder: *mut c_void,
    pub reserved1: *mut c_void,
    pub create_mv_buffer: *mut c_void,
    pub destroy_mv_buffer: *mut c_void,
    pub run_motion_estimation_only: *mut c_void,
    pub get_last_error_string: *mut c_void,
    pub set_io_cuda_streams: *mut c_void,
    pub get_encode_preset_config_ex: *mut c_void,
    pub get_sequence_param_ex: *mut c_void,
    pub restore_encoder_state: *mut c_void,
    pub lookahead_picture: *mut c_void,
    pub reserved2: [*mut c_void; 275],
}

/// Vérifie que les offsets Rust correspondent au layout C de l'en-tête.
/// Panique avec le détail si ce n'est pas le cas : mieux vaut un arrêt net
/// qu'un encodage silencieusement faux.
pub fn verifier_layout() {
    use std::mem::{offset_of, size_of};

    assert_eq!(size_of::<RcParams>(), 128, "NV_ENC_RC_PARAMS doit faire 128 octets");
    assert_eq!(offset_of!(PresetConfig, preset_cfg), 8, "PresetConfig.presetCfg");
    assert_eq!(offset_of!(Config, rc_params), 40, "Config.rcParams");
    assert_eq!(
        offset_of!(Config, encode_codec_config),
        168,
        "Config.encodeCodecConfig (union alignée sur 8)"
    );
    assert_eq!(offset_of!(InitializeParams, encode_guid), 4);
    assert_eq!(offset_of!(InitializeParams, preset_guid), 20);
    assert_eq!(offset_of!(InitializeParams, encode_width), 36);
    assert_eq!(offset_of!(InitializeParams, frame_rate_num), 52);
    assert_eq!(offset_of!(InitializeParams, encode_config), 88);
    assert_eq!(offset_of!(InitializeParams, max_me_hint_counts_per_block), 104);
    assert_eq!(
        size_of::<MeHintCountsPerBlock>(),
        16,
        "ME_HINT_COUNTS_PER_BLOCKTYPE = 16 octets"
    );
    assert_eq!(
        offset_of!(InitializeParams, tuning_info),
        136,
        "tuningInfo apres maxMEHintCountsPerBlock[2] (32 octets)"
    );
    assert_eq!(offset_of!(InitializeParams, buffer_format), 140);
    assert_eq!(offset_of!(PicParams, input_time_stamp), 24);
    assert_eq!(offset_of!(PicParams, input_buffer), 40);
    assert_eq!(offset_of!(PicParams, output_bitstream), 48);
    assert_eq!(offset_of!(PicParams, buffer_fmt), 64);
    assert_eq!(offset_of!(PicParams, picture_type), 72);
    assert_eq!(offset_of!(RegisterResource, resource_to_register), 24);
    assert_eq!(offset_of!(RegisterResource, registered_resource), 32);
    assert_eq!(offset_of!(MapInputResource, mapped_resource), 24);
    assert_eq!(offset_of!(CreateBitstreamBuffer, bitstream_buffer), 16);
    assert_eq!(offset_of!(LockBitstream, bitstream_size_in_bytes), 36);
    assert_eq!(offset_of!(LockBitstream, bitstream_buffer_ptr), 56);
    assert_eq!(size_of::<SessionParams>(), 1552, "SessionParams doit faire 1552 octets");
}

// --- Instance --------------------------------------------------------------

/// SDK chargé et fonction listée. Garde la DLL vivante pour toute la session.
pub struct Sdk {
    pub api_version: u32,
    pub liste: FunctionList,
}

impl Sdk {
    pub fn charger() -> Result<Self, String> {
        let nom: Vec<u16> = "nvEncodeAPI64.dll\0".encode_utf16().collect();
        let lib = unsafe { LoadLibraryW(nom.as_ptr()) };
        if lib.is_null() {
            return Err("nvEncodeAPI64.dll introuvable".into());
        }

        let p_max = unsafe { GetProcAddress(lib, c"NvEncodeAPIGetMaxSupportedVersion".as_ptr().cast()) };
        if p_max.is_null() {
            return Err("NvEncodeAPIGetMaxSupportedVersion absent".into());
        }
        let get_max: FnGetMaxVersion = unsafe { std::mem::transmute(p_max) };
        let mut version_max = 0u32;
        if unsafe { get_max(&mut version_max) } != 0 {
            return Err("NvEncodeAPIGetMaxSupportedVersion a échoué".into());
        }
        // Renvoie `majeur << 4 | mineur`, pas le format NVENCAPI_VERSION.
        let majeur = (version_max >> 4) & 0xff;
        let mineur = version_max & 0x0f;
        let api_version = majeur | (mineur << 24);

        let p_create = unsafe { GetProcAddress(lib, c"NvEncodeAPICreateInstance".as_ptr().cast()) };
        if p_create.is_null() {
            return Err("NvEncodeAPICreateInstance absent".into());
        }
        let create: FnCreateInstance = unsafe { std::mem::transmute(p_create) };

        let mut liste: FunctionList = unsafe { std::mem::zeroed() };
        liste.version = struct_version(api_version, 2);
        if unsafe { create(&mut liste) } != 0 {
            return Err("NvEncodeAPICreateInstance a échoué".into());
        }

        Ok(Self { api_version, liste })
    }

    fn ptr(&self, champ: *mut c_void, nom: &str) -> Result<*mut c_void, String> {
        if champ.is_null() {
            Err(format!("fonction NVENC absente : {nom}"))
        } else {
            Ok(champ)
        }
    }

    pub fn ouvrir_session(&self, device: *mut c_void) -> Result<*mut c_void, String> {
        let f: FnOpenSessionEx = unsafe {
            std::mem::transmute(self.ptr(self.liste.open_encode_session_ex, "OpenEncodeSessionEx")?)
        };
        let mut params: SessionParams = unsafe { std::mem::zeroed() };
        params.version = struct_version(self.api_version, VER_SESSION);
        params.device_type = DEVICE_TYPE_DIRECTX;
        params.device = device;
        params.api_version = self.api_version;
        let mut encoder: *mut c_void = null_mut();
        let st = unsafe { f(&mut params, &mut encoder) };
        if st != 0 || encoder.is_null() {
            return Err(format!("nvEncOpenEncodeSessionEx : statut {st}"));
        }
        Ok(encoder)
    }

    pub fn codecs(&self, encoder: *mut c_void) -> Result<Vec<GUID>, String> {
        let compter: FnGetGuidCount = unsafe {
            std::mem::transmute(self.ptr(self.liste.get_encode_guid_count, "GetEncodeGUIDCount")?)
        };
        let lister: FnGetGuids = unsafe {
            std::mem::transmute(self.ptr(self.liste.get_encode_guids, "GetEncodeGUIDs")?)
        };
        let mut nombre = 0u32;
        if unsafe { compter(encoder, &mut nombre) } != 0 {
            return Err("nvEncGetEncodeGUIDCount a échoué".into());
        }
        let mut guids = vec![GUID::from_u128(0); nombre as usize];
        let mut obtenus = nombre;
        if unsafe { lister(encoder, guids.as_mut_ptr(), nombre, &mut obtenus) } != 0 {
            return Err("nvEncGetEncodeGUIDs a échoué".into());
        }
        guids.truncate(obtenus as usize);
        Ok(guids)
    }

    pub fn config_preset(
        &self,
        encoder: *mut c_void,
        codec: GUID,
        preset: GUID,
        tuning: u32,
    ) -> Result<PresetConfig, String> {
        let f: FnGetPresetConfigEx = unsafe {
            std::mem::transmute(self.ptr(
                self.liste.get_encode_preset_config_ex,
                "GetEncodePresetConfigEx",
            )?)
        };
        let mut preset_config: PresetConfig = unsafe { std::mem::zeroed() };
        preset_config.version = struct_version(self.api_version, VER_PRESET_CONFIG) | (1u32 << 31);
        preset_config.preset_cfg.version =
            struct_version(self.api_version, VER_CONFIG) | (1u32 << 31);
        let st = unsafe { f(encoder, codec, preset, tuning, &mut preset_config) };
        if st != 0 {
            return Err(format!(
                "nvEncGetEncodePresetConfigEx : statut {st} — {}",
                self.derniere_erreur(encoder)
            ));
        }
        Ok(preset_config)
    }

    pub fn initialiser(
        &self,
        encoder: *mut c_void,
        params: &mut InitializeParams,
    ) -> Result<(), String> {
        let f: FnInitializeEncoder = unsafe {
            std::mem::transmute(self.ptr(self.liste.initialize_encoder, "InitializeEncoder")?)
        };
        let st = unsafe { f(encoder, params) };
        if st != 0 {
            return Err(format!(
                "nvEncInitializeEncoder : statut {st} — {}",
                self.derniere_erreur(encoder)
            ));
        }
        Ok(())
    }

    pub fn enregistrer_texture(
        &self,
        encoder: *mut c_void,
        texture: *mut c_void,
        largeur: u32,
        hauteur: u32,
    ) -> Result<*mut c_void, String> {
        let f: FnRegisterResource = unsafe {
            std::mem::transmute(self.ptr(self.liste.register_resource, "RegisterResource")?)
        };
        let mut p: RegisterResource = unsafe { std::mem::zeroed() };
        p.version = struct_version(self.api_version, VER_REGISTER_RESOURCE);
        p.resource_type = INPUT_RESOURCE_TYPE_DIRECTX;
        p.width = largeur;
        p.height = hauteur;
        p.pitch = 0;
        p.sub_resource_index = 0;
        p.resource_to_register = texture;
        p.buffer_format = BUFFER_FORMAT_NV12;
        p.buffer_usage = BUFFER_USAGE_NONE;
        let st = unsafe { f(encoder, &mut p) };
        if st != 0 || p.registered_resource.is_null() {
            return Err(format!("nvEncRegisterResource : statut {st}"));
        }
        Ok(p.registered_resource)
    }

    pub fn mapper(&self, encoder: *mut c_void, ressource: *mut c_void) -> Result<*mut c_void, String> {
        let f: FnMapInputResource = unsafe {
            std::mem::transmute(self.ptr(self.liste.map_input_resource, "MapInputResource")?)
        };
        let mut p: MapInputResource = unsafe { std::mem::zeroed() };
        p.version = struct_version(self.api_version, VER_MAP_INPUT);
        p.registered_resource = ressource;
        let st = unsafe { f(encoder, &mut p) };
        if st != 0 || p.mapped_resource.is_null() {
            return Err(format!("nvEncMapInputResource : statut {st}"));
        }
        Ok(p.mapped_resource)
    }

    pub fn creer_bitstream(&self, encoder: *mut c_void, taille: u32) -> Result<*mut c_void, String> {
        let f: FnCreateBitstream = unsafe {
            std::mem::transmute(self.ptr(
                self.liste.create_bitstream_buffer,
                "CreateBitstreamBuffer",
            )?)
        };
        let mut p: CreateBitstreamBuffer = unsafe { std::mem::zeroed() };
        p.version = struct_version(self.api_version, VER_CREATE_BITSTREAM);
        p.size = taille;
        p.memory_heap = MEMORY_HEAP_DEFAULT;
        let st = unsafe { f(encoder, &mut p) };
        if st != 0 || p.bitstream_buffer.is_null() {
            return Err(format!("nvEncCreateBitstreamBuffer : statut {st}"));
        }
        Ok(p.bitstream_buffer)
    }

    /// Encode une image et rend les octets du bitstream.
    pub fn encoder_image(
        &self,
        encoder: *mut c_void,
        image: Image,
    ) -> Result<Vec<u8>, String> {
        self.encoder_image_avec(encoder, image, PIC_TYPE_IDR)
    }

    /// Variante avec type d'image explicite : IDR seulement à la première
    /// frame et à chaque GOP, P sinon. Forcer IDR à chaque frame gaspille le
    /// débit (défaut du spike corrigé au lot 1.6).
    pub fn encoder_image_avec(
        &self,
        encoder: *mut c_void,
        image: Image,
        picture_type: u32,
    ) -> Result<Vec<u8>, String> {
        let Image { entree, sortie, largeur, hauteur, index, pts_us } = image;
        let encoder_f: FnEncodePicture = unsafe {
            std::mem::transmute(self.ptr(self.liste.encode_picture, "EncodePicture")?)
        };
        let verrouiller: FnLockBitstream = unsafe {
            std::mem::transmute(self.ptr(self.liste.lock_bitstream, "LockBitstream")?)
        };
        let deverrouiller: FnUnlockBitstream = unsafe {
            std::mem::transmute(self.ptr(self.liste.unlock_bitstream, "UnlockBitstream")?)
        };

        let mut p: PicParams = unsafe { std::mem::zeroed() };
        p.version = struct_version(self.api_version, VER_PIC) | (1u32 << 31);
        p.input_width = largeur;
        p.input_height = hauteur;
        p.input_pitch = 0;
        p.frame_idx = index;
        p.input_time_stamp = pts_us as u64;
        p.input_duration = 1;
        p.input_buffer = entree;
        p.output_bitstream = sortie;
        p.buffer_fmt = BUFFER_FORMAT_NV12;
        p.picture_struct = PIC_STRUCT_FRAME;
        p.picture_type = picture_type;
        let st = unsafe { encoder_f(encoder, &mut p) };
        if st != 0 {
            return Err(format!(
                "nvEncEncodePicture : statut {st} — {}",
                self.derniere_erreur(encoder)
            ));
        }

        let mut l: LockBitstream = unsafe { std::mem::zeroed() };
        l.version = struct_version(self.api_version, VER_LOCK_BITSTREAM) | (1u32 << 31);
        l.output_bitstream = sortie;
        let st = unsafe { verrouiller(encoder, &mut l) };
        if st != 0 {
            return Err(format!(
                "nvEncLockBitstream : statut {st} — {}",
                self.derniere_erreur(encoder)
            ));
        }

        let taille = l.bitstream_size_in_bytes as usize;
        let donnees = if l.bitstream_buffer_ptr.is_null() || taille == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(l.bitstream_buffer_ptr as *const u8, taille) }.to_vec()
        };
        unsafe { deverrouiller(encoder, sortie) };

        Ok(donnees)
    }

    pub fn derniere_erreur(&self, encoder: *mut c_void) -> String {
        if encoder.is_null() || self.liste.get_last_error_string.is_null() {
            return String::new();
        }
        let f: FnGetLastError = unsafe { std::mem::transmute(self.liste.get_last_error_string) };
        let ptr = unsafe { f(encoder) };
        if ptr.is_null() {
            return String::new();
        }
        unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn detruire(&self, encoder: *mut c_void) {
        if self.liste.destroy_encoder.is_null() || encoder.is_null() {
            return;
        }
        let f: FnDestroyEncoder = unsafe { std::mem::transmute(self.liste.destroy_encoder) };
        unsafe { f(encoder) };
    }

    pub fn liberer_bitstream(&self, encoder: *mut c_void, bitstream: *mut c_void) {
        if self.liste.destroy_bitstream_buffer.is_null() || bitstream.is_null() {
            return;
        }
        let f: FnDestroyBitstream =
            unsafe { std::mem::transmute(self.liste.destroy_bitstream_buffer) };
        unsafe { f(encoder, bitstream) };
    }

    pub fn demapper(&self, encoder: *mut c_void, entree: *mut c_void) {
        if self.liste.unmap_input_resource.is_null() || entree.is_null() {
            return;
        }
        let f: FnUnmapInputResource =
            unsafe { std::mem::transmute(self.liste.unmap_input_resource) };
        unsafe { f(encoder, entree) };
    }

    pub fn desenregistrer(&self, encoder: *mut c_void, ressource: *mut c_void) {
        if self.liste.unregister_resource.is_null() || ressource.is_null() {
            return;
        }
        let f: FnUnregisterResource =
            unsafe { std::mem::transmute(self.liste.unregister_resource) };
        unsafe { f(encoder, ressource) };
    }
}
