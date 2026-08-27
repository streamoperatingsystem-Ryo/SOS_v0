use serde::{Deserialize, Serialize};

const DEFAULT_CANVAS_W: u32 = 1920;
const DEFAULT_CANVAS_H: u32 = 1080;

fn default_canvas_w() -> u32 {
    DEFAULT_CANVAS_W
}

fn default_canvas_h() -> u32 {
    DEFAULT_CANVAS_H
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
}

/// Scène unique (source de vérité). canvasW/canvasH = résolution OBS lue
/// au connect (fallback 1920×1080 si GetVideoSettings échoue ou config absente).
/// Les widgets existants ne sont JAMAIS rescalés quand ces dims changent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub widgets: Vec<Widget>,
    #[serde(default = "default_canvas_w")]
    pub canvasW: u32,
    #[serde(default = "default_canvas_h")]
    pub canvasH: u32,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            widgets: Vec::new(),
            canvasW: DEFAULT_CANVAS_W,
            canvasH: DEFAULT_CANVAS_H,
        }
    }
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }
}
