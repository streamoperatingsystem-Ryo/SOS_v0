use serde::{Deserialize, Serialize};

const DEFAULT_CANVAS_W: u32 = 1920;
const DEFAULT_CANVAS_H: u32 = 1080;
const DEFAULT_MEDIA_FIT: &str = "ajuster";
const DEFAULT_KIND: &str = "image";
const DEFAULT_MEDIA_ZOOM: f64 = 1.0;
const DEFAULT_MEDIA_ROT: f64 = 0.0;
const DEFAULT_BG_FIT: &str = "remplir";
const DEFAULT_BG_KIND: &str = "image";
const DEFAULT_BG_ZOOM: f64 = 1.0;
const DEFAULT_BG_ROT: f64 = 0.0;

fn default_canvas_w() -> u32 {
    DEFAULT_CANVAS_W
}

fn default_canvas_h() -> u32 {
    DEFAULT_CANVAS_H
}

fn default_media_fit() -> String {
    DEFAULT_MEDIA_FIT.to_string()
}

fn default_kind() -> String {
    DEFAULT_KIND.to_string()
}

fn default_media_zoom() -> f64 {
    DEFAULT_MEDIA_ZOOM
}

fn default_media_rot() -> f64 {
    DEFAULT_MEDIA_ROT
}

fn default_bg_fit() -> String {
    DEFAULT_BG_FIT.to_string()
}

fn default_bg_kind() -> String {
    DEFAULT_BG_KIND.to_string()
}

fn default_bg_zoom() -> f64 {
    DEFAULT_BG_ZOOM
}

fn default_bg_rot() -> f64 {
    DEFAULT_BG_ROT
}

fn default_true() -> bool {
    true
}

fn default_zero_f64() -> f64 {
    0.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    #[serde(rename = "type")]
    pub widget_type: String,
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
    pub z: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    #[serde(default)]
    pub rotateX: f64,
    #[serde(default)]
    pub rotateY: f64,
    #[serde(default = "default_media_fit")]
    pub mediaFit: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default = "default_media_zoom")]
    pub mediaZoom: f64,
    #[serde(default = "default_media_rot")]
    pub mediaRot: f64,
    #[serde(default = "default_true")]
    pub mediaPaused: bool,
    #[serde(default = "default_zero_f64")]
    pub mediaTime: f64,
    #[serde(default)]
    pub trou: bool,
}

/// Scène unique (source de vérité). canvasW/canvasH = résolution OBS lue
/// au connect (fallback 1920×1080 si GetVideoSettings échoue ou config absente).
/// Les widgets existants ne sont JAMAIS rescalés quand ces dims changent.
/// Fond de scène : calque sous les widgets (bgMedia = chemin relatif medias/...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub widgets: Vec<Widget>,
    #[serde(default = "default_canvas_w")]
    pub canvasW: u32,
    #[serde(default = "default_canvas_h")]
    pub canvasH: u32,
    #[serde(default)]
    pub bgMedia: String,
    #[serde(default = "default_bg_kind")]
    pub bgKind: String,
    #[serde(default = "default_bg_fit")]
    pub bgFit: String,
    #[serde(default = "default_bg_zoom")]
    pub bgZoom: f64,
    #[serde(default = "default_bg_rot")]
    pub bgRot: f64,
    #[serde(default = "default_true")]
    pub bgPaused: bool,
    #[serde(default = "default_zero_f64")]
    pub bgTime: f64,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            widgets: Vec::new(),
            canvasW: DEFAULT_CANVAS_W,
            canvasH: DEFAULT_CANVAS_H,
            bgMedia: String::new(),
            bgKind: DEFAULT_BG_KIND.to_string(),
            bgFit: DEFAULT_BG_FIT.to_string(),
            bgZoom: DEFAULT_BG_ZOOM,
            bgRot: DEFAULT_BG_ROT,
            bgPaused: true,
            bgTime: 0.0,
        }
    }
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }
}
