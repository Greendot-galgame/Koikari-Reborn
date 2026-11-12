//! Deterministic visual timelines. The renderer never invents a wall clock.
//! Geometry in Scene.layers is stable; completed displacement commits once.
use crate::model::{Layer, Scene, VisualCommand, HEIGHT, WIDTH};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const RUNTIME: &str = "__visual_runtime_v1";
const MAX_TIME: u64 = 3_600_000;
const MAX_TRACKS: usize = 128;
const MAX_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Unhandled,
    Continue,
    Wait(u64),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Point {
    value: f32,
    ms: u64,
    accel: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Path(Vec<Point>);
impl Path {
    fn duration(&self) -> u64 {
        self.0.iter().fold(0u64, |sum, p| sum.saturating_add(p.ms))
    }
    fn end(&self) -> f32 {
        self.0.last().map_or(0.0, |p| p.value)
    }
    fn sample(&self, time: u64, loops: u32) -> f32 {
        let duration = self.duration();
        if duration == 0 {
            return self.end();
        }
        if loops > 0 && time >= duration.saturating_mul(loops as u64) {
            return self.end() * loops as f32;
        }
        let cycle = time / duration;
        let mut time = time % duration;
        let mut previous = 0.0;
        for p in &self.0 {
            if time < p.ms {
                return cycle as f32 * self.end()
                    + previous
                    + (p.value - previous) * easing(time as f32 / p.ms as f32, p.accel);
            }
            time = time.saturating_sub(p.ms);
            previous = p.value;
        }
        cycle as f32 * self.end() + previous
    }
}

/// The actual motion plugin uses linear -1/0/1, power acceleration above 1,
/// and the complement power curve below -1.
pub fn easing(progress: f32, accel: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    if accel > 1.0 {
        p.powf(accel)
    } else if accel < -1.0 {
        1.0 - (1.0 - p).powf(-accel)
    } else {
        p
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Motion {
    x: Path,
    y: Path,
    loops_x: u32,
    loops_y: u32,
}
impl Default for Motion {
    fn default() -> Self {
        Self {
            x: Path::default(),
            y: Path::default(),
            loops_x: 1,
            loops_y: 1,
        }
    }
}
impl Motion {
    fn duration(&self) -> u64 {
        let axis = |p: &Path, n: u32| {
            if n == 0 && p.duration() > 0 {
                u64::MAX
            } else {
                p.duration().saturating_mul(n as u64)
            }
        };
        axis(&self.x, self.loops_x).max(axis(&self.y, self.loops_y))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Track {
    kind: String,
    id: String,
    group: Option<u32>,
    start: u64,
    motion: Motion,
    opacity: Path,
    zoom: Path,
}
impl Track {
    fn duration(&self) -> u64 {
        self.motion
            .duration()
            .max(self.opacity.duration())
            .max(self.zoom.duration())
    }
}

/// Source image processing order: grayscale, gamma/floor/ceiling, color blend.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Style {
    pub grayscale: bool,
    pub gamma: [f32; 3],
    pub floor: [f32; 3],
    pub ceiling: [f32; 3],
    pub tint: [f32; 4],
}
impl Default for Style {
    fn default() -> Self {
        Self {
            grayscale: false,
            gamma: [1.0; 3],
            floor: [0.0; 3],
            ceiling: [1.0; 3],
            tint: [0.0; 4],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Transition {
    start: u64,
    duration: u64,
    kind: u32,
    rule: String,
    vague: f32,
    #[serde(default = "unit_zoom")]
    zoom1: f32,
    #[serde(default = "unit_zoom")]
    zoom2: f32,
}
fn unit_zoom() -> f32 {
    1.0
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Runtime {
    tracks: Vec<Track>,
    definitions: BTreeMap<String, Motion>,
    styles: BTreeMap<u32, Style>,
    previous_styles: BTreeMap<u32, Style>,
    transition: Option<Transition>,
    captured: bool,
    atlas_starts: BTreeMap<String, u64>,
    atlas_clips: BTreeMap<String, [u32; 4]>,
    atlas_paused: BTreeMap<String, u64>,
}
fn read(scene: &Scene) -> Result<Runtime, String> {
    if scene.visual.commands.len() > 1 {
        return Err("unexpected visual runtime commands".into());
    }
    if let Some(c) = scene.visual.commands.iter().find(|c| c.name == RUNTIME) {
        let data = c.args.get("data").ok_or("missing visual runtime payload")?;
        if data.len() > MAX_BYTES {
            return Err("visual runtime payload limit".into());
        }
        let r: Runtime =
            serde_json::from_str(data).map_err(|_| "invalid visual runtime payload")?;
        check_runtime(&r, scene.visual.elapsed_ms)?;
        Ok(r)
    } else if !scene.visual.commands.is_empty() {
        Err("unknown visual runtime version".into())
    } else {
        Ok(Runtime::default())
    }
}
fn check_runtime(r: &Runtime, elapsed: u64) -> Result<(), String> {
    if r.tracks.len() > MAX_TRACKS
        || r.definitions.len() > 128
        || [
            r.styles.len(),
            r.previous_styles.len(),
            r.atlas_starts.len(),
            r.atlas_clips.len(),
            r.atlas_paused.len(),
        ]
        .into_iter()
        .any(|n| n > 256)
    {
        return Err("visual runtime collection limit".into());
    }
    let paths = r
        .tracks
        .iter()
        .flat_map(|t| [&t.motion.x, &t.motion.y, &t.opacity, &t.zoom])
        .chain(r.definitions.values().flat_map(|m| [&m.x, &m.y]));
    for p in paths {
        if p.0.len() > 256
            || p.duration() > MAX_TIME
            || p.0.iter().any(|v| {
                v.ms > MAX_TIME
                    || !v.value.is_finite()
                    || v.value.abs() > 1.0e7
                    || !v.accel.is_finite()
                    || v.accel.abs() > 16.0
            })
        {
            return Err("invalid visual path".into());
        }
    }
    for m in r
        .tracks
        .iter()
        .map(|t| &t.motion)
        .chain(r.definitions.values())
    {
        if m.loops_x > 10_000 || m.loops_y > 10_000 {
            return Err("visual loop limit".into());
        }
    }
    if r.tracks.iter().any(|t| {
        t.start > elapsed
            || t.id.len() > 256
            || !matches!(t.kind.as_str(), "move" | "camera" | "motion" | "quake")
    }) || r.definitions.keys().any(|k| k.len() > 256)
    {
        return Err("invalid visual track identity or clock".into());
    }
    for s in r.styles.values().chain(r.previous_styles.values()) {
        if s.gamma
            .iter()
            .any(|n| !n.is_finite() || !(0.01..=16.0).contains(n))
            || s.floor
                .iter()
                .chain(&s.ceiling)
                .chain(&s.tint)
                .any(|n| !n.is_finite() || !(0.0..=1.0).contains(n))
        {
            return Err("invalid visual style".into());
        }
    }
    if r.transition.as_ref().is_some_and(|t| {
        t.start > elapsed
            || t.duration > MAX_TIME
            || t.kind > 3
            || [t.zoom1, t.zoom2]
                .iter()
                .any(|v| !v.is_finite() || !(0.01..=16.0).contains(v))
            || !t.vague.is_finite()
            || !(0.0..=1.0).contains(&t.vague)
            || t.rule.len() > 4096
            || (t.kind == 1 && t.rule.is_empty())
    }) || (r.captured && r.transition.is_some())
    {
        return Err("invalid visual transition".into());
    }
    if r.atlas_starts.iter().any(|(k, t)| {
        k.len() > 4096
            || *t > elapsed
            || r.atlas_paused.contains_key(k)
            || !r.atlas_clips.contains_key(k)
    }) || r
        .atlas_paused
        .keys()
        .any(|k| k.len() > 4096 || !r.atlas_clips.contains_key(k))
        || r.atlas_clips.iter().any(|(k, c)| {
            k.len() > 4096
                || c[0] > 32768
                || c[1] > 32768
                || c[2] == 0
                || c[3] == 0
                || c[2] > 8192
                || c[3] > 8192
        })
    {
        return Err("invalid visual atlas state".into());
    }
    Ok(())
}
fn write(scene: &mut Scene, r: &Runtime) -> Result<(), String> {
    check_runtime(r, scene.visual.elapsed_ms)?;
    let data = serde_json::to_string(r).map_err(|e| e.to_string())?;
    if data.len() > MAX_BYTES {
        return Err("visual runtime payload limit".into());
    }
    scene.visual.commands = vec![VisualCommand {
        name: RUNTIME.into(),
        args: BTreeMap::from([("data".into(), data)]),
    }];
    scene.visual.duration_ms = r
        .tracks
        .iter()
        .map(|t| t.start.saturating_add(t.duration()))
        .chain(
            r.transition
                .iter()
                .map(|t| t.start.saturating_add(t.duration)),
        )
        .max()
        .unwrap_or(scene.visual.elapsed_ms);
    Ok(())
}
/// Required at save-validation boundaries; tick/layers are infallible by contract.
pub fn validate(scene: &Scene) -> Result<(), String> {
    read(scene).map(|_| ())
}
fn number(args: &BTreeMap<String, String>, key: &str, default: f32) -> Result<f32, String> {
    let Some(s) = args.get(key) else {
        return Ok(default);
    };
    let n: f32 = s
        .parse()
        .map_err(|_| format!("visual {key} must be a resolved number"))?;
    if !n.is_finite() || n.abs() > 1.0e7 {
        return Err(format!("visual {key} out of range"));
    }
    Ok(n)
}
fn time(args: &BTreeMap<String, String>, default: u64) -> Result<u64, String> {
    let n = number(args, "time", default as f32)?;
    if !(0.0..=MAX_TIME as f32).contains(&n) {
        return Err("visual duration out of range".into());
    }
    Ok(n as u64)
}
fn boolean(args: &BTreeMap<String, String>, key: &str, default: bool) -> Result<bool, String> {
    match args.get(key).map(String::as_str) {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
        _ => Err(format!("visual {key} must be a resolved boolean")),
    }
}
fn group(args: &BTreeMap<String, String>, default: Option<u32>) -> Result<Option<u32>, String> {
    let Some(s) = args
        .get("lay")
        .or_else(|| args.get("layer"))
        .or_else(|| args.get("ch"))
    else {
        return Ok(default);
    };
    if s == "all" {
        return Ok(None);
    }
    if s == "base" {
        return Ok(Some(0));
    }
    let n: u32 = s
        .parse()
        .map_err(|_| "visual layer must be a resolved numeric group")?;
    if n > 1024 {
        return Err("visual layer limit".into());
    }
    Ok(Some(n))
}
fn check_keys(args: &BTreeMap<String, String>, allowed: &[&str]) -> Result<(), String> {
    for k in args.keys() {
        if !allowed.contains(&k.as_str()) && !["cond", "canskip", "page"].contains(&k.as_str()) {
            return Err(format!("unsupported visual parameter: {k}"));
        }
    }
    if args.get("page").is_some_and(|p| p != "fore" && p != "both") {
        return Err("visual back-page operation requires scenario staging".into());
    }
    Ok(())
}
fn tuples(text: &str) -> Result<Vec<Vec<&str>>, String> {
    let mut s = text.trim();
    let mut result = Vec::new();
    while !s.is_empty() {
        s = s
            .strip_prefix('(')
            .ok_or("expected parenthesized visual path")?;
        let end = s.find(')').ok_or("unfinished visual path")?;
        result.push(s[..end].split(',').map(str::trim).collect());
        if result.len() > 256 {
            return Err("visual path point limit".into());
        }
        s = s[end + 1..].trim();
        if !s.is_empty() {
            s = s
                .strip_prefix(',')
                .ok_or("invalid visual path separator")?
                .trim();
        }
    }
    if result.is_empty() {
        return Err("empty visual path".into());
    }
    Ok(result)
}
fn scalar(s: &str) -> Result<f32, String> {
    let n: f32 = s.parse().map_err(|_| "invalid visual path number")?;
    if !n.is_finite() || n.abs() > 1.0e7 {
        return Err("visual path number limit".into());
    }
    Ok(n)
}
fn path(text: &str) -> Result<Path, String> {
    let mut r = Path::default();
    let mut prev = Point::default();
    for v in tuples(text)? {
        if v.len() != 3 {
            return Err("motion path requires (value,milliseconds,acceleration)".into());
        }
        let value = if v[0].is_empty() {
            prev.value
        } else if let Some(v) = v[0].strip_prefix('r') {
            prev.value + scalar(v)?
        } else {
            scalar(v[0])?
        };
        let ms = if v[1].is_empty() {
            prev.ms as f32
        } else {
            scalar(v[1])?
        };
        let accel = if v[2].is_empty() {
            prev.accel
        } else {
            scalar(v[2])?
        };
        if !(0.0..=MAX_TIME as f32).contains(&ms) || accel.abs() > 16.0 {
            return Err("visual path timing/easing limit".into());
        }
        prev = Point {
            value,
            ms: ms as u64,
            accel,
        };
        r.0.push(prev.clone());
    }
    if r.duration() > MAX_TIME {
        return Err("visual path duration limit".into());
    }
    Ok(r)
}
fn single(value: f32, ms: u64, accel: f32) -> Path {
    Path(vec![Point { value, ms, accel }])
}
fn loop_count(args: &BTreeMap<String, String>, key: &str, default: u32) -> Result<u32, String> {
    let n = number(args, key, default as f32)?;
    if !(0.0..=10_000.0).contains(&n) || n.fract() != 0.0 {
        return Err("invalid visual loop count".into());
    }
    Ok(n as u32)
}

// Semantic motion presets, not executable source macros. u moves down; dt up.
fn preset(id: &str) -> Option<Motion> {
    let (x, y, loops) = match id {
        "l" | "左移動" => ("(-50,200,2)", "", 1),
        "r" | "右移動" => ("(50,200,2)", "", 1),
        "ll" | "左左移動" => ("(-40,200,2)", "", 1),
        "rr" | "右右移動" => ("(40,200,2)", "", 1),
        "u" | "下移動" => ("", "(50,200,2)", 1),
        "uu" | "下ゆっくり移動" => ("", "(30,200,2)", 1),
        "dt" | "上移動" => ("", "(-70,200,2)", 1),
        "ft_l" | "左移動f" => ("(-300,240,2)", "", 1),
        "ft_r" | "右移動f" => ("(300,240,2)", "", 1),
        "ft_ll" | "左移動ff" => ("(-400,260,2)", "", 1),
        "ft_rr" | "右移動ff" => ("(400,260,2)", "", 1),
        "st_l" | "左移動s" => ("(-280,200,2)", "", 1),
        "st_r" | "右移動s" => ("(280,200,2)", "", 1),
        "st_ly" | "左移動sy" => ("(-280,400,2)", "", 1),
        "st_ry" | "右移動sy" => ("(280,400,2)", "", 1),
        "j" | "ジャンプ" => ("", "(-20,130,0),(0,130,0)", 1),
        "jj" | "ジャンプ2" => ("", "(-16,150,1),(0,150,1)", 2),
        "uz" | "うずうず" => ("", "(-10,400,0),(0,400,1)", 2),
        "un" | "うん" => ("", "(10,200,0),(0,200,1)", 1),
        "un2" | "うんうん" => ("", "(10,200,0),(0,200,1)", 2),
        "re" | "礼" => ("", "(20,300,1),(0,300,-1)", 1),
        "gt" | "ぐったり" => ("(8,300,0),(0,300,1),(-8,300,1),(0,300,1)", "", 1),
        "oc" | "落ち着きがない" => (
            "(80,200,1),(80,200,0),(-80,200,1),(-80,200,0),(0,200,1)",
            "(0,200,0),(-20,100,0),(0,100,0),(0,200,0),(-20,100,0),(0,100,0)",
            1,
        ),
        "t" | "ツッコミ" => ("", "(-24,90,2),(0,90,-2)", 1),
        "tt" | "ツッコミタッチ" => ("(40,80,1),(0,80,-2)", "", 1),
        "ttl" | "ツッコミタッチ左" => ("(-40,80,1),(0,80,-2)", "", 1),
        "gk" | "ぎくっ" => ("(20,45,-1),(0,45,1),(-10,45,-1),(0,45,1)", "", 1),
        "yu" | "ゆらゆら" => ("(15,170,-2),(0,170,2),(-15,170,-2),(0,170,2)", "", 1),
        "yue" | "ゆらゆら永" => ("(15,170,-2),(0,170,2),(-15,170,-2),(0,170,2)", "", 0),
        "yuyu" | "ゆらゆらゆっくり" => {
            ("(15,300,-2),(0,300,2),(-15,300,-2),(0,300,2)", "", 1)
        }
        "pp" | "ぷるぷる" => ("(10,20,-2),(0,20,2),(-10,20,-2),(0,20,2)", "", 2),
        "ppe" | "ぷるぷる永" => ("(10,20,-2),(0,20,2),(-10,20,-2),(0,20,2)", "", 0),
        "ppy" | "ぷるぷるたて" => ("", "(10,20,-2),(0,20,2),(-10,20,-2),(0,20,2)", 2),
        "ppye" | "ぷるぷるたて永" => ("", "(10,20,-2),(0,20,2),(-10,20,-2),(0,20,2)", 0),
        "sk" | "深呼吸" => ("", "(24,800,-2),(0,800,1)", 1),
        "skk" | "深呼吸2" => ("", "(24,800,-2),(0,800,1)", 2),
        "qk" | "振動" => (
            "(5,10,-1),(-5,20,1),(5,20,-1),(-5,20,1),(0,10,-1)",
            "(5,10,-1),(5,20,1),(-5,20,-1),(-5,20,-1),(0,10,1)",
            3,
        ),
        "qk_e" | "振動永" => (
            "(5,10,-1),(-5,20,1),(5,20,-1),(-5,20,1),(0,10,-1)",
            "(5,10,-1),(5,20,1),(-5,20,-1),(-5,20,-1),(0,10,1)",
            0,
        ),
        "dl" | "ドン引き左" => ("(-20,100,0),(-20,160,0)", "", 2),
        "dr" | "ドン引き右" => ("(20,100,0),(20,160,0)", "", 2),
        _ => return None,
    };
    Some(Motion {
        x: if x.is_empty() {
            Path::default()
        } else {
            path(x).ok()?
        },
        y: if y.is_empty() {
            Path::default()
        } else {
            path(y).ok()?
        },
        loops_x: loops,
        loops_y: loops,
    })
}

fn apply(layers: &mut [Layer], track: &Track, elapsed: u64) {
    let t = elapsed.saturating_sub(track.start);
    let x = track.motion.x.sample(t, track.motion.loops_x);
    let y = track.motion.y.sample(t, track.motion.loops_y);
    let opacity = track.opacity.sample(t, 1);
    let zoom = 1.0 + track.zoom.sample(t, 1);
    for l in layers
        .iter_mut()
        .filter(|l| track.group.is_none_or(|g| l.group == g))
    {
        if track.kind == "camera" {
            l.x = (l.x - WIDTH * 0.5) * zoom + WIDTH * 0.5 + x;
            l.y = (l.y - HEIGHT * 0.5) * zoom + HEIGHT * 0.5 + y;
            l.width *= zoom;
            l.height *= zoom;
        } else {
            l.x += x;
            l.y += y;
        }
        l.opacity = (l.opacity + opacity).clamp(0.0, 1.0);
    }
}
fn evaluated(scene: &Scene, r: &Runtime) -> Vec<Layer> {
    let mut v = scene.layers.clone();
    for t in &r.tracks {
        apply(&mut v, t, scene.visual.elapsed_ms);
    }
    v
}
fn remove_tracks(
    scene: &mut Scene,
    r: &mut Runtime,
    group: Option<u32>,
    selector: (&str, Option<&str>),
    last: bool,
    end: bool,
) {
    let (kind, id) = selector;
    r.tracks.retain(|t| {
        let matches = (group.is_none() || t.group == group)
            && (kind.is_empty() || t.kind == kind)
            && id.is_none_or(|id| t.id == id);
        if matches && last && t.kind != "quake" {
            if end && t.duration() == u64::MAX {
                // The source plugin's final value multiplies displacement by
                // its loop count, including zero for an infinite axis.
                let mut stopped = t.clone();
                stopped.motion.x = single(t.motion.x.end() * t.motion.loops_x as f32, 0, 0.0);
                stopped.motion.y = single(t.motion.y.end() * t.motion.loops_y as f32, 0, 0.0);
                stopped.opacity = single(t.opacity.end(), 0, 0.0);
                stopped.zoom = single(t.zoom.end(), 0, 0.0);
                apply(&mut scene.layers, &stopped, t.start);
            } else {
                let elapsed = if end {
                    t.start.saturating_add(t.duration())
                } else {
                    scene.visual.elapsed_ms
                };
                apply(&mut scene.layers, t, elapsed);
            }
        }
        !matches
    });
}
fn remaining(
    scene: &Scene,
    r: &Runtime,
    group: Option<u32>,
    selector: (&str, Option<&str>),
) -> Result<u64, String> {
    let (kind, id) = selector;
    let end = r
        .tracks
        .iter()
        .filter(|t| {
            (group.is_none() || t.group == group)
                && (kind.is_empty() || t.kind == kind)
                && id.is_none_or(|id| t.id == id)
        })
        .map(|t| t.start.saturating_add(t.duration()))
        .max()
        .unwrap_or(scene.visual.elapsed_ms);
    if end == u64::MAX {
        return Err("cannot wait for an infinite visual loop".into());
    }
    Ok(end.saturating_sub(scene.visual.elapsed_ms))
}

fn image_style(args: &BTreeMap<String, String>) -> Result<Style, String> {
    let mut s = Style::default();
    match number(args, "type", 0.0)? as i32 {
        0 => (),
        1 => s.grayscale = true,
        2 => {
            s.grayscale = true;
            s.gamma = [2.1, 1.5, 1.0];
        }
        3 => {
            s.floor = [1.0; 3];
            s.ceiling = [0.0; 3];
        }
        4 => {
            s.floor = [40.0 / 255.0, 0.0, 0.0];
            s.ceiling = [240.0 / 255.0; 3];
        }
        5 => {
            s.ceiling = [210.0 / 255.0; 3];
            s.tint = [0.0, 11.0 / 255.0, 115.0 / 255.0, 50.0 / 255.0];
        }
        _ => return Err("unsupported visual image type".into()),
    }
    s.grayscale = boolean(args, "grayscale", s.grayscale)?;
    for (i, c) in ["r", "g", "b"].iter().enumerate() {
        s.gamma[i] = number(args, &format!("{c}gamma"), s.gamma[i])?;
        s.floor[i] = number(args, &format!("{c}floor"), s.floor[i] * 255.0)? / 255.0;
        s.ceiling[i] = number(args, &format!("{c}ceil"), s.ceiling[i] * 255.0)? / 255.0;
        if !(0.01..=16.0).contains(&s.gamma[i])
            || !(0.0..=1.0).contains(&s.floor[i])
            || !(0.0..=1.0).contains(&s.ceiling[i])
        {
            return Err("visual image adjustment out of range".into());
        }
    }
    if let Some(c) = args.get("mcolor") {
        let c = if let Some(h) = c.strip_prefix("0x") {
            u32::from_str_radix(h, 16)
        } else {
            c.parse()
        }
        .map_err(|_| "invalid visual tint color")?;
        s.tint = [
            ((c >> 16) & 255) as f32 / 255.0,
            ((c >> 8) & 255) as f32 / 255.0,
            (c & 255) as f32 / 255.0,
            0.0,
        ];
    }
    s.tint[3] = number(args, "mopacity", s.tint[3] * 255.0)? / 255.0;
    if !(0.0..=1.0).contains(&s.tint[3]) {
        return Err("visual tint opacity out of range".into());
    }
    Ok(s)
}

/// Execute atomically. For asset-changing directives the scenario loader must
/// capture with ts, change destination layers, then call te/trans/whiteout.
pub fn command(
    scene: &mut Scene,
    name: &str,
    args: &BTreeMap<String, String>,
) -> Result<Outcome, String> {
    let mut next = scene.clone();
    let mut r = read(scene)?;
    let result = execute(&mut next, &mut r, name, args)?;
    if result != Outcome::Unhandled {
        next.visual.sequence = next.visual.sequence.wrapping_add(1);
        write(&mut next, &r)?;
        *scene = next;
    }
    Ok(result)
}
fn execute(
    scene: &mut Scene,
    r: &mut Runtime,
    name: &str,
    args: &BTreeMap<String, String>,
) -> Result<Outcome, String> {
    let now = scene.visual.elapsed_ms;
    let mut wait = false;
    let mut wait_ms = 0;
    match name {
        "ts" | "trans_macro_s" | "backlay" => {
            check_keys(args, &["layer"])?;
            if args.contains_key("layer") {
                return Err("per-layer backlay requires scenario staging".into());
            }
            if r.transition.is_some() {
                return Err("capture during active transition: wait or stoptrans first".into());
            }
            if r.captured {
                return Ok(Outcome::Continue);
            }
            scene.visual.previous = evaluated(scene, r);
            r.previous_styles = r.styles.clone();
            r.captured = true;
        }
        "te" | "trans" | "trans_macro_e" | "whiteout" => {
            check_keys(
                args,
                &[
                    "time", "method", "rule", "vague", "wait", "bg", "zoom1", "zoom2",
                ],
            )?;
            if !r.captured {
                return Err("transition requires before-layers captured by ts/backlay".into());
            }
            let method = args
                .get("method")
                .map(String::as_str)
                .unwrap_or("crossfade");
            let kind = if name == "whiteout" {
                2
            } else {
                match method {
                    "crossfade" => 0,
                    "universal" => 1,
                    "zoomfade" => 3,
                    _ => return Err(format!("unsupported transition method: {method}")),
                }
            };
            let duration = if name == "whiteout" {
                1200
            } else {
                time(
                    args,
                    if name == "trans" || name == "trans_macro_e" {
                        1
                    } else {
                        200
                    },
                )?
            };
            let rule = args.get("rule").cloned().unwrap_or_default();
            if kind == 1 && rule.is_empty() {
                return Err("universal transition requires a rule texture".into());
            }
            let vague = number(args, "vague", 64.0)?;
            if !(0.0..=255.0).contains(&vague) {
                return Err("transition vague out of range".into());
            }
            r.transition = Some(Transition {
                start: now,
                duration,
                kind,
                rule,
                vague: vague / 255.0,
                zoom1: number(args, "zoom1", 100.0)? / 100.0,
                zoom2: number(args, "zoom2", 100.0)? / 100.0,
            });
            r.captured = false;
            wait = boolean(args, "wait", name != "trans")?;
            wait_ms = duration;
        }
        "wt" => {
            check_keys(args, &[])?;
            wait = true;
            wait_ms = r.transition.as_ref().map_or(0, |t| {
                t.start.saturating_add(t.duration).saturating_sub(now)
            });
        }
        "stoptrans" => {
            check_keys(args, &[])?;
            if r.transition.take().is_some() {
                scene.visual.previous.clear();
                r.captured = false;
            }
        }
        "t_move" | "motion_start" | "motion_start_onetime" | "motion_define" => {
            if name != "motion_define" && r.tracks.iter().any(|t| t.kind == "camera") {
                return Err("camera transform must complete before starting a layer motion".into());
            }
            let aliases: Vec<_> = args
                .keys()
                .filter(|k| preset(k).is_some())
                .cloned()
                .collect();
            if aliases.len() > 1 {
                return Err("multiple motion aliases".into());
            }
            let alias = aliases.first();
            let id = args
                .get("id")
                .or_else(|| args.get("name"))
                .cloned()
                .or_else(|| alias.cloned())
                .ok_or("motion requires recognized alias or id/name")?;
            let mut allowed = vec![
                "lay", "layer", "id", "name", "wait", "locatex", "locatey", "loop", "loopx",
                "loopy",
            ];
            if let Some(a) = alias {
                allowed.push(a);
            }
            check_keys(args, &allowed)?;
            let mut motion = if args.contains_key("locatex") || args.contains_key("locatey") {
                Motion {
                    x: args
                        .get("locatex")
                        .map(|v| path(v))
                        .transpose()?
                        .unwrap_or_default(),
                    y: args
                        .get("locatey")
                        .map(|v| path(v))
                        .transpose()?
                        .unwrap_or_default(),
                    ..Motion::default()
                }
            } else {
                r.definitions
                    .get(&id)
                    .cloned()
                    .or_else(|| preset(&id))
                    .ok_or_else(|| format!("unsupported motion id: {id}"))?
            };
            let loops = loop_count(args, "loop", motion.loops_x)?;
            motion.loops_x = loop_count(args, "loopx", loops)?;
            motion.loops_y = loop_count(
                args,
                "loopy",
                args.get("loop").map_or(motion.loops_y, |_| loops),
            )?;
            if name == "motion_define" {
                if r.definitions.len() >= 128 {
                    return Err("motion definition limit".into());
                }
                r.definitions.insert(id, motion);
            } else {
                let group = group(args, Some(0))?;
                let duration = motion.duration();
                wait = boolean(args, "wait", false)?;
                if wait && duration == u64::MAX {
                    return Err("cannot wait for an infinite motion".into());
                }
                wait_ms = duration;
                if r.tracks.len() >= MAX_TRACKS {
                    return Err("visual track limit".into());
                }
                r.tracks.push(Track {
                    kind: "motion".into(),
                    id,
                    group,
                    start: now,
                    motion,
                    opacity: Path::default(),
                    zoom: Path::default(),
                });
            }
        }
        "motion_undefine" => {
            check_keys(args, &["name"])?;
            r.definitions
                .remove(args.get("name").ok_or("missing motion name")?);
        }
        "move" | "camera" => {
            if (name == "camera" && !r.tracks.is_empty())
                || r.tracks.iter().any(|t| t.kind == "camera")
            {
                return Err("camera transform must be completed before another transform".into());
            }
            check_keys(
                args,
                &[
                    "layer", "lay", "time", "path", "left", "top", "x", "y", "opacity", "accel",
                    "wait", "zoom",
                ],
            )?;
            let group = group(args, if name == "camera" { None } else { Some(0) })?;
            remove_tracks(scene, r, group, (name, None), true, false);
            let base = scene
                .layers
                .iter()
                .find(|l| group.is_none_or(|g| l.group == g))
                .ok_or("move target layer is absent")?;
            let duration = time(args, 0)?;
            let accel = number(args, "accel", 0.0)?;
            if accel.abs() > 16.0 {
                return Err("visual acceleration limit".into());
            }
            let (base_x, base_y, base_opacity) = if name == "camera" {
                (0.0, 0.0, 1.0)
            } else {
                (base.x, base.y, base.opacity)
            };
            let (mut x, mut y, mut opacity) = (Path::default(), Path::default(), Path::default());
            if let Some(p) = args.get("path") {
                let points = tuples(p)?;
                if duration.saturating_mul(points.len() as u64) > MAX_TIME {
                    return Err("move path duration limit".into());
                }
                for v in points {
                    if v.len() != 3 {
                        return Err("move path requires (left,top,opacity)".into());
                    }
                    x.0.push(Point {
                        value: scalar(v[0])? - base_x,
                        ms: duration,
                        accel,
                    });
                    y.0.push(Point {
                        value: scalar(v[1])? - base_y,
                        ms: duration,
                        accel,
                    });
                    opacity.0.push(Point {
                        value: scalar(v[2])?.clamp(0.0, 255.0) / 255.0 - base_opacity,
                        ms: duration,
                        accel,
                    });
                }
            } else {
                x = single(
                    number(args, "left", number(args, "x", base_x)?)? - base_x,
                    duration,
                    accel,
                );
                y = single(
                    number(args, "top", number(args, "y", base_y)?)? - base_y,
                    duration,
                    accel,
                );
                opacity = single(
                    number(args, "opacity", base_opacity * 255.0)?.clamp(0.0, 255.0) / 255.0
                        - base_opacity,
                    duration,
                    accel,
                );
            }
            let zoom = number(args, "zoom", 1.0)?;
            if name != "camera" && args.contains_key("zoom") {
                return Err("zoom requires camera command".into());
            }
            if !(0.01..=16.0).contains(&zoom) {
                return Err("camera zoom limit".into());
            }
            wait = boolean(args, "wait", false)?;
            let t = Track {
                kind: name.into(),
                id: String::new(),
                group,
                start: now,
                motion: Motion {
                    x,
                    y,
                    ..Motion::default()
                },
                opacity,
                zoom: single(zoom - 1.0, duration, accel),
            };
            wait_ms = t.duration();
            if r.tracks.len() >= MAX_TRACKS {
                return Err("visual track limit".into());
            }
            r.tracks.push(t);
        }
        "wm" | "motion_wait" | "wq" => {
            check_keys(args, &["lay", "layer", "name"])?;
            wait = true;
            wait_ms = remaining(
                scene,
                r,
                group(args, None)?,
                (
                    if name == "wq" {
                        "quake"
                    } else if name == "wm" {
                        "move"
                    } else {
                        "motion"
                    },
                    args.get("name").map(String::as_str),
                ),
            )?;
        }
        "stopmove" | "motion_stop" | "stopquake" => {
            check_keys(args, &["lay", "layer", "name", "lastpos"])?;
            remove_tracks(
                scene,
                r,
                group(args, None)?,
                (
                    match name {
                        "stopmove" => "move",
                        "stopquake" => "quake",
                        _ => "motion",
                    },
                    args.get("name").map(String::as_str),
                ),
                boolean(args, "lastpos", true)?,
                true,
            );
        }
        "quake" => {
            check_keys(args, &["time", "hmax", "vmax", "wait"])?;
            let duration = time(args, 300)?;
            let h = number(args, "hmax", 10.0)?;
            let v = number(args, "vmax", 10.0)?;
            if !(0.0..=1024.0).contains(&h) || !(0.0..=1024.0).contains(&v) {
                return Err("quake amplitude limit".into());
            }
            remove_tracks(scene, r, None, ("quake", None), false, false);
            // Reproducible shake, ending at zero without committing displacement.
            let (mut x, mut y) = (Path::default(), Path::default());
            let steps = duration.div_ceil(30).max(1);
            if steps > 256 {
                return Err("quake duration exceeds supported path budget".into());
            }
            for i in 0..steps {
                let ms = if i + 1 == steps {
                    duration - i * 30
                } else {
                    30
                };
                x.0.push(Point {
                    value: if i + 1 == steps {
                        0.0
                    } else if i % 2 == 0 {
                        h
                    } else {
                        -h
                    },
                    ms,
                    accel: 0.0,
                });
                y.0.push(Point {
                    value: if i + 1 == steps {
                        0.0
                    } else if i % 3 == 0 {
                        v
                    } else {
                        -v
                    },
                    ms,
                    accel: 0.0,
                });
            }
            if r.tracks.len() >= MAX_TRACKS {
                return Err("visual track limit".into());
            }
            r.tracks.push(Track {
                kind: "quake".into(),
                id: String::new(),
                group: None,
                start: now,
                motion: Motion {
                    x,
                    y,
                    ..Motion::default()
                },
                opacity: Path::default(),
                zoom: Path::default(),
            });
            wait = boolean(args, "wait", false)?;
            wait_ms = duration;
        }
        "visual_style" => {
            check_keys(
                args,
                &[
                    "layer",
                    "lay",
                    "ch",
                    "type",
                    "grayscale",
                    "rgamma",
                    "ggamma",
                    "bgamma",
                    "rfloor",
                    "gfloor",
                    "bfloor",
                    "rceil",
                    "gceil",
                    "bceil",
                    "mcolor",
                    "mopacity",
                    "opacity",
                    "visible",
                ],
            )?;
            let selected = group(args, Some(0))?;
            let s = image_style(args)?;
            for l in &mut scene.layers {
                if selected.is_none_or(|g| l.group == g) {
                    r.styles.insert(l.group, s.clone());
                    if args.contains_key("opacity") {
                        l.opacity = (number(args, "opacity", 255.0)? / 255.0).clamp(0.0, 1.0);
                    }
                    if !boolean(args, "visible", true)? {
                        l.opacity = 0.0;
                    }
                }
            }
        }
        "atlas_start" | "atlas_stop" => {
            check_keys(
                args,
                &["asset", "clipleft", "cliptop", "clipwidth", "clipheight"],
            )?;
            let asset = args.get("asset").ok_or("atlas command requires asset")?;
            if !scene.layers.iter().any(|l| &l.asset == asset) {
                return Err("atlas asset is not in scene".into());
            }
            if name == "atlas_start" {
                if !args.contains_key("clipwidth") || !args.contains_key("clipheight") {
                    return Err("atlas_start requires explicit clipwidth and clipheight".into());
                }
                r.atlas_starts.insert(asset.clone(), now);
                r.atlas_paused.remove(asset);
                let mut clip = [0, 0, WIDTH as u32, HEIGHT as u32];
                for (i, k) in ["clipleft", "cliptop", "clipwidth", "clipheight"]
                    .iter()
                    .enumerate()
                {
                    let n = number(args, k, clip[i] as f32)?;
                    if !(0.0..=8192.0).contains(&n) || n.fract() != 0.0 {
                        return Err("atlas initial clip limit".into());
                    }
                    clip[i] = n as u32;
                }
                r.atlas_clips.insert(asset.clone(), clip);
            } else if let Some(start) = r.atlas_starts.remove(asset) {
                r.atlas_paused
                    .insert(asset.clone(), now.saturating_sub(start));
            } else if !r.atlas_paused.contains_key(asset) {
                return Err("atlas_stop requires a started atlas".into());
            }
        }
        _ => return Ok(Outcome::Unhandled),
    }
    Ok(if wait && wait_ms > 0 {
        Outcome::Wait(wait_ms)
    } else {
        Outcome::Continue
    })
}

pub fn tick(scene: &mut Scene, milliseconds: u64) {
    // Keep the clock running while idle, for continuous ASD animation.
    scene.visual.elapsed_ms = scene.visual.elapsed_ms.saturating_add(milliseconds);
    let Ok(mut r) = read(scene) else { return };
    let now = scene.visual.elapsed_ms;
    r.tracks.retain(|t| {
        if t.duration() != u64::MAX && now.saturating_sub(t.start) >= t.duration() {
            if t.kind != "quake" {
                apply(&mut scene.layers, t, t.start.saturating_add(t.duration()));
            }
            false
        } else {
            true
        }
    });
    if r.transition
        .as_ref()
        .is_some_and(|t| now.saturating_sub(t.start) >= t.duration)
    {
        r.transition = None;
        scene.visual.previous.clear();
        r.previous_styles.clear();
    }
    let _ = write(scene, &r);
}
/// Evaluated destination layers; full-scene transition composition is separate
/// because fading each sprite separately exposes the face/body overlaps.
pub fn layers(scene: &Scene) -> Vec<Layer> {
    read(scene)
        .map(|r| evaluated(scene, &r))
        .unwrap_or_default()
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) struct Frame {
    pub current: Vec<Layer>,
    pub previous: Vec<Layer>,
    pub styles: BTreeMap<u32, Style>,
    pub previous_styles: BTreeMap<u32, Style>,
    pub kind: u32,
    pub progress: f32,
    pub rule: String,
    pub vague: f32,
    pub zoom: [f32; 2],
    pub atlas_times: BTreeMap<String, u64>,
    pub atlas_clips: BTreeMap<String, [u32; 4]>,
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) fn frame(scene: &Scene) -> Result<Frame, String> {
    let r = read(scene)?;
    let current = evaluated(scene, &r);
    let mut f = Frame {
        current,
        previous: Vec::new(),
        styles: r.styles,
        previous_styles: r.previous_styles,
        kind: 0,
        progress: 1.0,
        rule: String::new(),
        vague: 0.0,
        zoom: [1.0; 2],
        atlas_times: r
            .atlas_starts
            .into_iter()
            .map(|(k, v)| (k, scene.visual.elapsed_ms.saturating_sub(v)))
            .chain(r.atlas_paused)
            .collect(),
        atlas_clips: r.atlas_clips,
    };
    if let Some(t) = r.transition {
        f.progress = if t.duration == 0 {
            1.0
        } else {
            (scene.visual.elapsed_ms.saturating_sub(t.start) as f32 / t.duration as f32)
                .clamp(0.0, 1.0)
        };
        f.kind = t.kind;
        f.rule = t.rule;
        f.vague = t.vague;
        f.zoom = [
            1.0 + (t.zoom1 - 1.0) * f.progress,
            t.zoom2 + (1.0 - t.zoom2) * f.progress,
        ];
        if f.progress < 1.0 {
            f.previous = scene.visual.previous.clone();
        }
    }
    Ok(f)
}

/// Bounded ASD wait/clip/jump subset. Clip offsets are resource pixels; never
/// infer a scale factor or frame count by dividing the sheet width.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtlasAnimation {
    pub frames: Vec<AtlasFrame>,
    pub duration_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtlasFrame {
    pub clip: [u32; 4],
    pub duration_ms: u64,
}
impl AtlasAnimation {
    /// Message-layer cell animation. Geometry comes from copy sw/sh and the
    /// base image, independently of the cell sheet's dimensions.
    pub fn parse_message_cell(source: &str, base: [u32; 2]) -> Result<Self, String> {
        use crate::kag::Kind;
        if source.len() > 65536 || base.contains(&0) {
            return Err("message ASD bounds".into());
        }
        let mut input: std::collections::VecDeque<_> = crate::kag::parse(source)?.into();
        let mut definitions = BTreeMap::<String, Vec<crate::kag::Token>>::new();
        let mut clip = [0, 0, base[0], base[1]];
        let mut frames = Vec::new();
        let mut label = None;
        let mut duration_ms = 0;
        for _ in 0..4096 {
            let token = input.pop_front().ok_or("message ASD needs loop jump")?;
            match token.kind {
                Kind::Label(name) if label.is_none() => label = Some(name),
                Kind::Tag { name, args } => match name.as_str() {
                    "loadcell" | "loop" if label.is_none() && args.is_empty() => (),
                    "macro" if label.is_none() => {
                        check_keys(&args, &["name"])?;
                        let name = args.get("name").ok_or("message ASD macro name")?.clone();
                        let mut body = Vec::new();
                        loop {
                            let token =
                                input.pop_front().ok_or("message ASD unterminated macro")?;
                            if matches!(&token.kind, Kind::Tag { name, .. } if name == "endmacro") {
                                break;
                            }
                            body.push(token);
                            if body.len() > 64 {
                                return Err("message ASD macro limit".into());
                            }
                        }
                        if definitions.insert(name, body).is_some() || definitions.len() > 16 {
                            return Err("message ASD duplicate/limit".into());
                        }
                    }
                    "copy" => {
                        check_keys(&args, &["dx", "dy", "sx", "sy", "sw", "sh"])?;
                        if number(&args, "dx", 0.0)? != 0.0 || number(&args, "dy", 0.0)? != 0.0 {
                            return Err("message ASD offset unsupported".into());
                        }
                        for (i, key) in ["sx", "sy", "sw", "sh"].iter().enumerate() {
                            let n = number(&args, key, clip[i] as f32)?;
                            if !(0.0..=32768.0).contains(&n) || n.fract() != 0.0 {
                                return Err("message ASD invalid copy".into());
                            }
                            clip[i] = n as u32;
                        }
                        if clip[2..] != base {
                            return Err("message ASD base/cell size mismatch".into());
                        }
                    }
                    "wait" => {
                        check_keys(&args, &["time"])?;
                        let ms = time(&args, 0)?;
                        if ms == 0 {
                            return Err("message ASD zero wait".into());
                        }
                        frames.push(AtlasFrame {
                            clip,
                            duration_ms: ms,
                        });
                        duration_ms += ms;
                        if frames.len() > 256 || duration_ms > MAX_TIME {
                            return Err("message ASD timeline limit".into());
                        }
                    }
                    "jump"
                        if args.get("target").map(|s| s.trim_start_matches('*'))
                            == label.as_deref() =>
                    {
                        check_keys(&args, &["target"])?;
                        if frames.is_empty() {
                            return Err("message ASD empty".into());
                        }
                        return Ok(Self {
                            frames,
                            duration_ms,
                        });
                    }
                    _ if definitions.contains_key(&name) => {
                        let mut body = definitions[&name].clone();
                        for token in &mut body {
                            if let Kind::Tag { args: fields, .. } = &mut token.kind {
                                for value in fields.values_mut() {
                                    if let Some(key) = value.strip_prefix('%') {
                                        *value = args
                                            .get(key)
                                            .ok_or("message ASD missing macro argument")?
                                            .clone();
                                    }
                                }
                            }
                        }
                        for token in body.into_iter().rev() {
                            input.push_front(token);
                        }
                    }
                    _ => return Err(format!("unsupported message ASD command: {name}")),
                },
                Kind::Text(text) if text.trim().is_empty() => (),
                _ => return Err("unsupported message ASD structure".into()),
            }
        }
        Err("message ASD expansion budget".into())
    }
    pub fn parse(source: &str, initial_clip: [u32; 4]) -> Result<Self, String> {
        if source.len() > 65_536 {
            return Err("ASD source limit".into());
        }
        let mut clip = initial_clip;
        let mut frames = Vec::new();
        let mut duration_ms = 0u64;
        let mut first_label = None;
        let mut jumped = false;
        for token in crate::kag::parse(source)? {
            match token.kind {
                crate::kag::Kind::Label(label) => {
                    if first_label.is_none() {
                        first_label = Some(label);
                    }
                }
                crate::kag::Kind::Text(s) if s.trim().is_empty() => (),
                crate::kag::Kind::Tag { name, args } => {
                    if jumped {
                        return Err("ASD data after loop jump".into());
                    }
                    match name.as_str() {
                        "wait" => {
                            check_keys(&args, &["time"])?;
                            let ms = time(&args, 0)?;
                            if ms == 0 {
                                return Err("zero ASD wait".into());
                            }
                            frames.push(AtlasFrame {
                                clip,
                                duration_ms: ms,
                            });
                            duration_ms = duration_ms.saturating_add(ms);
                        }
                        "clip" => {
                            check_keys(&args, &["left", "top", "width", "height"])?;
                            for (i, k) in ["left", "top", "width", "height"].iter().enumerate() {
                                let v = number(&args, k, clip[i] as f32)?;
                                if !(0.0..=32768.0).contains(&v) || v.fract() != 0.0 {
                                    return Err("invalid ASD clip".into());
                                }
                                clip[i] = v as u32;
                            }
                        }
                        "jump" => {
                            check_keys(&args, &["target"])?;
                            if args.get("target").map(|s| s.trim_start_matches('*'))
                                != first_label.as_deref()
                            {
                                return Err("ASD supports only a loop to its first label".into());
                            }
                            jumped = true;
                        }
                        _ => return Err(format!("unsupported ASD command: {name}")),
                    }
                }
                _ => return Err("unsupported ASD text".into()),
            }
            if frames.len() > 256 || duration_ms > MAX_TIME {
                return Err("ASD timeline limit".into());
            }
        }
        if !jumped || frames.is_empty() {
            return Err("ASD requires finite waits and a loop jump".into());
        }
        Ok(Self {
            frames,
            duration_ms,
        })
    }
    pub fn clip_at(&self, elapsed_ms: u64) -> [u32; 4] {
        let mut t = elapsed_ms % self.duration_ms;
        for f in &self.frames {
            if t < f.duration_ms {
                return f.clip;
            }
            t -= f.duration_ms;
        }
        self.frames[0].clip
    }
    pub fn validate_dimensions(&self, width: u32, height: u32) -> Result<(), String> {
        for f in &self.frames {
            let [x, y, w, h] = f.clip;
            if w == 0
                || h == 0
                || w > 8192
                || h > 8192
                || x.checked_add(w).is_none_or(|v| v > width)
                || y.checked_add(h).is_none_or(|v| v > height)
            {
                return Err("ASD clip outside image or GPU frame limit".into());
            }
        }
        Ok(())
    }
}
