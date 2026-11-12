use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const WIDTH: f32 = 2560.0;
pub const HEIGHT: f32 = 1440.0;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Choice {
    pub id: usize,
    pub text: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FlowState {
    pub revision: u32,
    pub variables: BTreeMap<String, serde_json::Value>,
    pub event_id: Option<usize>,
    pub selection_id: Option<usize>,
    pub choices: Vec<Choice>,
    pub wait_ms: u64,
    pub wait_skippable: bool,
    pub ending: Option<String>,
    pub replay: bool,
    pub visited: BTreeSet<String>,
    pub route: Option<usize>,
    pub conditions: Vec<Conditional>,
    pub macros: BTreeMap<String, MacroDefinition>,
    pub macro_stack: Vec<MacroFrame>,
    pub call_conditions: Vec<Vec<Conditional>>,
    pub selected: BTreeMap<usize, usize>,
    pub video_sequence: u64,
    pub pending_video: Option<String>,
    pub page_open: bool,
    pub scene_replay: bool,
    pub replay_return: Option<Box<State>>,
    /// Resolved native macro continuation, resumed before the next script token.
    pub staging: Vec<VisualCommand>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Conditional {
    pub parent: bool,
    pub active: bool,
    pub taken: bool,
    pub otherwise: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MacroDefinition {
    pub script: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MacroFrame {
    pub cursor: Cursor,
    pub params: BTreeMap<String, String>,
    pub conditions: Vec<Conditional>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Progress {
    pub events: BTreeSet<String>,
    pub cg: BTreeSet<String>,
    pub endings: BTreeSet<String>,
    pub variables: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VisualCommand {
    pub name: String,
    pub args: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VisualState {
    pub sequence: u64,
    pub elapsed_ms: u64,
    pub duration_ms: u64,
    pub previous: Vec<Layer>,
    pub commands: Vec<VisualCommand>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Video {
    pub asset: String,
    pub sequence: u64,
    pub skippable: bool,
    pub looping: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Credits {
    pub text: String,
    pub images: Vec<String>,
    #[serde(default)]
    pub commands: Vec<VisualCommand>,
    pub duration_ms: u64,
    pub elapsed_ms: u64,
    pub skippable: bool,
    pub left: u32,
    pub width: u32,
    pub start_disp: u32,
    pub end_disp: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct GalleryEntry {
    pub id: String,
    pub title: String,
    pub asset: String,
    pub unlocked: bool,
}

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
    #[serde(default)]
    pub missing: bool,
    pub sequence: u64,
    pub looping: bool,
    #[serde(default)]
    pub volume: Option<f32>,
    #[serde(default)]
    pub fade_ms: u64,
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
    #[serde(default)]
    pub visual: VisualState,
    #[serde(default)]
    pub video: Option<Video>,
    #[serde(default)]
    pub credits: Option<Credits>,
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
            visual: VisualState::default(),
            video: None,
            credits: None,
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
    pub flow: FlowState,
    #[serde(default)]
    pub progress: Progress,
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
            flow: FlowState::default(),
            progress: Progress::default(),
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
    fn full_runtime_contract_round_trips_pending_input_and_media() {
        let mut state = State::default();
        state
            .flow
            .variables
            .insert("f.route".into(), serde_json::json!(2));
        state.flow.choices.push(Choice {
            id: 3,
            text: "Synthetic choice".into(),
            enabled: true,
        });
        state.flow.wait_ms = 200;
        state.scene.video = Some(Video {
            asset: "test.mp4".into(),
            sequence: 3,
            skippable: true,
            looping: false,
        });
        state.scene.visual.duration_ms = 300;
        state.scene.visual.elapsed_ms = 125;
        state.progress.endings.insert("synthetic-ending".into());
        let bytes = serde_json::to_vec(&state).unwrap();
        assert_eq!(serde_json::from_slice::<State>(&bytes).unwrap(), state);
    }
    #[test]
    fn legacy_state_without_new_contract_fields_uses_safe_defaults() {
        let state = State::default();
        let mut value = serde_json::to_value(&state).unwrap();
        for key in ["flow", "progress"] {
            value.as_object_mut().unwrap().remove(key);
        }
        for key in ["visual", "video"] {
            value["scene"].as_object_mut().unwrap().remove(key);
        }
        for channel in ["bgm", "voice", "se"] {
            for key in ["volume", "fade_ms"] {
                value["scene"][channel].as_object_mut().unwrap().remove(key);
            }
        }
        assert_eq!(serde_json::from_value::<State>(value).unwrap(), state);
    }
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
