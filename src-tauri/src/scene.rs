use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Scene {
    pub widgets: Vec<Widget>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }
}
