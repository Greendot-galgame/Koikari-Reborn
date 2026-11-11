use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WIDTH: f32 = 2560.0;
pub const HEIGHT: f32 = 1440.0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Layer {
    pub asset: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub opacity: f32,
    pub group: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Audio {
    pub asset: String,
    pub sequence: u64,
    pub looping: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    pub layers: Vec<Layer>,
    pub speaker: String,
    pub text: String,
    pub message_visible: bool,
    pub bgm: Audio,
    pub voice: Audio,
    pub se: Audio,
}
impl Default for Scene {
    fn default() -> Self {
        Self {
            layers: Vec::new(),
            speaker: String::new(),
            text: String::new(),
            message_visible: true,
            bgm: Audio::default(),
            voice: Audio::default(),
            se: Audio::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Cursor {
    pub script: String,
    pub token: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Sprite {
    pub size: u32,
    pub dress: u32,
    pub expression: String,
    pub position: String,
    pub parts: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Interlude {
    pub background: String,
    pub bgm: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct State {
    #[serde(default)]
    pub visual_revision: u32,
    #[serde(default)]
    pub interlude: Option<Interlude>,
    pub scene: Scene,
    pub cursor: Cursor,
    pub stack: Vec<Cursor>,
    pub sprites: BTreeMap<u32, Sprite>,
    pub history: Vec<String>,
    pub warnings: Vec<String>,
    pub blocked: Option<String>,
    pub title: bool,
    pub finished: bool,
    pub pages: u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            visual_revision: 1,
            interlude: None,
            scene: Scene::default(),
            cursor: Cursor::default(),
            stack: Vec::new(),
            sprites: BTreeMap::new(),
            history: Vec::new(),
            warnings: Vec::new(),
            blocked: None,
            title: true,
            finished: false,
            pages: 0,
        }
    }
}

/// Returns a centered 16:9 content rectangle in the host's coordinate units.
pub fn content_rect(width: f32, height: f32) -> [f32; 4] {
    let scale = (width.max(0.0) / WIDTH).min(height.max(0.0) / HEIGHT);
    let w = WIDTH * scale;
    let h = HEIGHT * scale;
    [(width - w) / 2.0, (height - h) / 2.0, w, h]
}

pub fn content_point(x: f32, y: f32, width: f32, height: f32) -> Option<[f32; 2]> {
    let [left, top, w, h] = content_rect(width, height);
    if w <= 0.0 || h <= 0.0 || x < left || y < top || x > left + w || y > top + h {
        return None;
    }
    Some([(x - left) / w * WIDTH, (y - top) / h * HEIGHT])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ipad_letterbox() {
        assert_eq!(content_rect(1024.0, 768.0), [0.0, 96.0, 1024.0, 576.0]);
        assert_eq!(
            content_point(512.0, 384.0, 1024.0, 768.0),
            Some([1280.0, 720.0])
        );
        assert!(content_point(512.0, 0.0, 1024.0, 768.0).is_none());
    }
    #[test]
    fn zero_size_is_not_interactive() {
        assert!(content_point(0.0, 0.0, 0.0, 0.0).is_none());
    }
}
