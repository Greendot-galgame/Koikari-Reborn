use crate::events::Database;
use crate::kag::{self, Kind, Token};
use crate::model::*;
use crate::tjs;
use crate::xp3::{self, Assets, Store};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, String>;

pub struct Engine {
    pub assets: Arc<Store>,
    pub state: State,
    programs: BTreeMap<String, Vec<Token>>,
    tables: BTreeMap<String, Value>,
    dimensions: BTreeMap<String, (u32, u32)>,
    names: BTreeMap<String, String>,
    checkpoints: VecDeque<State>,
    database: Database,
    replay_scenes: Vec<String>,
    transition_speeds: BTreeMap<String, u64>,
}

fn merge_progress(target: &mut Progress, source: &Progress) {
    // Current globals are authoritative even when an older value is larger/truthy.
    let mut current = source.clone();
    crate::save::merge_progress(&mut current, target);
    *target = current;
}
fn sync_globals(state: &mut State) {
    for (key, value) in &state.progress.variables {
        if key.starts_with("sf.") {
            state.flow.variables.insert(key.clone(), value.clone());
        }
    }
}

fn static_value(source: &str) -> Result<Value> {
    let text = source.trim();
    if text.starts_with('[') {
        if !text.ends_with(']') {
            return Err("unfinished metadata array".into());
        }
        let mut depth = 0;
        let mut start = 1;
        let mut values = Vec::new();
        for (i, c) in text.char_indices().skip(1) {
            if c == '[' {
                depth += 1;
            }
            if c == ']' && depth > 0 {
                depth -= 1;
            } else if (c == ',' && depth == 0) || i == text.len() - 1 {
                let part = text[start..i].trim();
                if !part.is_empty() {
                    values.push(static_value(part)?);
                }
                start = i + 1;
            }
        }
        return Ok(Value::Array(values));
    }
    let mut shift = text.split("<<");
    let mut value: i64 = shift
        .next()
        .ok_or("empty metadata value")?
        .trim()
        .parse()
        .map_err(|_| "nonliteral metadata value")?;
    if let Some(bits) = shift.next() {
        let bits: u32 = bits.trim().parse().map_err(|_| "bad shift")?;
        value = value.checked_shl(bits).ok_or("shift overflow")?;
    }
    if shift.next().is_some() {
        return Err("multiple metadata shifts".into());
    }
    Ok(Value::from(value))
}

fn value_at(tables: &BTreeMap<String, Value>, key: &str, indices: &[u32]) -> Result<f32> {
    let mut value = tables
        .get(key)
        .ok_or_else(|| format!("missing sprite metadata: {key}"))?;
    for index in indices {
        value = value
            .get(*index as usize)
            .ok_or_else(|| format!("sprite metadata index missing: {key}{indices:?}"))?;
    }
    value
        .as_i64()
        .map(|n| n as f32)
        .ok_or_else(|| format!("nonnumeric sprite metadata: {key}"))
}

impl Engine {
    pub fn choose(&mut self, id: usize) -> Result<()> {
        if self.state.title
            || self.state.finished
            || self.state.blocked.is_some()
            || self.state.scene.video.is_some()
            || self.state.flow.wait_ms > 0
        {
            return Err("choice is not currently available".into());
        }
        let selection = self.state.flow.selection_id.ok_or("no pending selection")?;
        if !self
            .state
            .flow
            .choices
            .iter()
            .any(|c| c.id == id && c.enabled)
        {
            return Err("invalid or disabled choice ID".into());
        }
        let row = self
            .database
            .selections
            .get(selection)
            .ok_or("invalid selection state")?
            .clone();
        let args = BTreeMap::from([
            ("storage".into(), row.script.clone()),
            ("target".into(), format!("*SLP_{selection}_{id}")),
        ]);
        let cursor = self.jump_cursor(&args)?;
        self.checkpoint();
        self.register_input()?;
        self.state.flow.selected.insert(selection, id);
        self.state
            .flow
            .variables
            .insert("f.EV_NowSelectNum".into(), Value::from(selection));
        self.state.flow.variables.insert(
            "f.EV_SelectDirectJump".into(),
            if row.destinations[id].is_empty() {
                Value::from(-1)
            } else {
                Value::from(row.destinations[id].clone())
            },
        );
        if !self.state.flow.replay {
            self.state
                .progress
                .variables
                .insert(format!("selection:{selection}:{id}"), Value::Bool(true));
        }
        self.state.flow.choices.clear();
        self.state.cursor = cursor;
        self.run()
    }
    pub fn tick(&mut self, milliseconds: u64) -> Result<bool> {
        if self.state.blocked.is_some() {
            return Err(self.state.blocked.clone().unwrap());
        }
        if self.state.title || self.state.finished {
            return Ok(false);
        }
        let scene_before = self.state.scene.clone();
        let wait_before = self.state.flow.wait_ms;
        let before = self.state.scene.visual.elapsed_ms;
        crate::animation::tick(&mut self.state.scene, milliseconds);
        let mut changed = before != self.state.scene.visual.elapsed_ms;
        if let Some(credits) = &mut self.state.scene.credits {
            let previous = credits.elapsed_ms;
            credits.elapsed_ms = credits
                .elapsed_ms
                .saturating_add(milliseconds)
                .min(credits.duration_ms);
            changed |= credits.elapsed_ms != previous;
        }
        if self.state.flow.wait_ms > 0
            && self.state.scene.video.is_none()
            && self.state.flow.choices.is_empty()
        {
            self.state.flow.wait_ms = self.state.flow.wait_ms.saturating_sub(milliseconds);
            if self.state.flow.wait_ms == 0 {
                self.run()?;
                changed = true;
            }
        }
        Ok(changed || self.state.scene != scene_before || self.state.flow.wait_ms != wait_before)
    }
    pub fn finish_video(&mut self, sequence: u64) -> Result<()> {
        if self.state.blocked.is_some() {
            return Err("blocked video continuation".into());
        }
        let video = self.state.scene.video.as_ref().ok_or("no active video")?;
        if video.sequence != sequence || video.looping {
            return Err("stale or looping video completion".into());
        }
        self.state.scene.video = None;
        self.run()
    }
    pub fn gallery(&self) -> Result<Vec<GalleryEntry>> {
        let mut entries = BTreeMap::new();
        for asset in self.assets.names() {
            let base = asset.rsplit('/').next().unwrap_or(&asset);
            if base.starts_with("ev") && (base.ends_with(".webp") || base.ends_with(".png")) {
                let group = base.split('_').next().unwrap_or(base);
                if group.len() < 3 || !group[2..].chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                entries
                    .entry(group.to_owned())
                    .and_modify(|entry: &mut GalleryEntry| {
                        if !self
                            .state
                            .progress
                            .cg
                            .contains(&format!("asset:{}", entry.asset))
                            && self.state.progress.cg.contains(&format!("asset:{asset}"))
                        {
                            entry.asset = asset.clone();
                        }
                    })
                    .or_insert_with(|| GalleryEntry {
                        id: group.into(),
                        title: group.into(),
                        asset: asset.clone(),
                        unlocked: self.state.progress.cg.contains(group),
                    });
            }
        }
        Ok(entries.into_values().collect())
    }
    pub fn replay_list(&self) -> Result<Vec<GalleryEntry>> {
        let mut entries: Vec<_> = self
            .database
            .events
            .iter()
            .map(|e| GalleryEntry {
                id: format!("event:{}", e.script),
                title: e.title.clone(),
                asset: e.script.clone(),
                unlocked: self.state.progress.events.contains(&e.script),
            })
            .collect();
        for script in &self.replay_scenes {
            entries.push(GalleryEntry {
                id: format!("scene:{script}"),
                title: self.database.events
                    [self.database.index(script).ok_or("unknown replay event")?]
                .title
                .clone(),
                asset: script.clone(),
                unlocked: self
                    .state
                    .progress
                    .events
                    .contains(&format!("scene:{script}")),
            });
        }
        Ok(entries)
    }
    pub fn start_replay(&mut self, id: &str) -> Result<()> {
        if self.state.flow.replay {
            return Err("nested replay is not allowed".into());
        }
        let entry = self
            .replay_list()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or("unknown replay ID")?;
        if !entry.unlocked {
            return Err("replay is locked".into());
        }
        let index = self
            .database
            .index(&entry.asset)
            .ok_or("unknown replay event")?;
        let mut next = State {
            title: false,
            progress: self.state.progress.clone(),
            ..State::default()
        };
        next.flow.revision = 1;
        next.flow.replay = true;
        next.flow.scene_replay = id.starts_with("scene:");
        next.flow.video_sequence = self.state.flow.video_sequence;
        next.flow.replay_return = Some(Box::new(self.state.clone()));
        let old = std::mem::replace(&mut self.state, next);
        if let Err(e) = self.enter_event(index).and_then(|_| self.advance()) {
            self.state = old;
            return Err(e);
        }
        self.checkpoints.clear();
        Ok(())
    }
    pub fn open(assets: Arc<Store>) -> Result<Self> {
        let metadata = assets
            .text("Init_VariableSTAND.ks")
            .map_err(|e| e.to_string())?;
        let mut tables = BTreeMap::new();
        for line in metadata.lines() {
            let Some((left, right)) = line.split_once('=') else {
                continue;
            };
            let left: String = left.chars().filter(|c| !c.is_whitespace()).collect();
            if [
                "STI_AX[",
                "STI_AY[",
                "STI_AH[",
                "STI_HAllPos[",
                "STI_FCAX[",
                "STI_FCAY[",
                "STI_NoCheekNum[",
            ]
            .iter()
            .any(|key| left.starts_with(key))
            {
                if let Ok(value) = static_value(right.split(';').next().unwrap_or_default()) {
                    tables.insert(left, value);
                }
            }
        }
        let config = assets
            .text("Init_VariableADV.ks")
            .map_err(|e| e.to_string())?;
        let mut transition_speeds = BTreeMap::new();
        for line in config.lines() {
            if let Some(rest) = line.trim().strip_prefix("tf.adv_EFSpeed[\"") {
                if let Some((key, value)) = rest.split_once("\"]") {
                    if let Some(value) = value.trim().strip_prefix('=') {
                        if let Ok(ms) = value.split(';').next().unwrap_or("").trim().parse::<u64>()
                        {
                            if ms > 86400000 {
                                return Err("invalid transition configuration".into());
                            }
                            transition_speeds.insert(key.into(), ms);
                        }
                    }
                }
            }
        }
        let mapping = config
            .split("tf.adv_NameMappings =")
            .nth(1)
            .ok_or("name mappings missing")?;
        let mut names = BTreeMap::new();
        for line in mapping
            .lines()
            .take_while(|line| !line.trim_start().starts_with("];"))
        {
            if let Some((key, value)) = line.trim().split_once("=>") {
                if let (Ok(key), Ok(value)) = (
                    serde_json::from_str::<String>(key.trim()),
                    serde_json::from_str::<String>(value.trim().trim_end_matches(',')),
                ) {
                    names.insert(key, value);
                }
            }
        }
        let database = Database::parse(&assets.text("DB_EV.ks").map_err(|e| e.to_string())?)?;
        let mut replay_scenes = Vec::new();
        if assets.find("DB_EX.ks", &[]).is_ok() {
            let extra = assets.text("DB_EX.ks").map_err(|e| e.to_string())?;
            let scenes = tjs::table(&extra, "lf.EX_SCENEInfo")?;
            for row in scenes.as_array().ok_or("invalid replay table")? {
                let script = row
                    .get(2)
                    .and_then(Value::as_str)
                    .ok_or("invalid replay event")?;
                if database.index(script).is_none() {
                    return Err("replay references unknown event".into());
                }
                replay_scenes.push(script.to_owned());
            }
        }
        let mut engine = Self {
            assets,
            state: State::default(),
            programs: BTreeMap::new(),
            tables,
            dimensions: BTreeMap::new(),
            names,
            checkpoints: VecDeque::new(),
            database,
            replay_scenes,
            transition_speeds,
        };
        engine.title()?;
        Ok(engine)
    }
    pub fn title(&mut self) -> Result<()> {
        self.checkpoints.clear();
        let progress = self.state.progress.clone();
        let sequence = self.state.flow.video_sequence;
        self.state = State {
            progress,
            ..State::default()
        };
        self.state.flow.video_sequence = sequence;
        self.background("TI_base_0")?;
        Ok(())
    }
    pub fn start(&mut self) -> Result<()> {
        self.checkpoints.clear();
        let progress = self.state.progress.clone();
        let sequence = self.state.flow.video_sequence;
        self.state = State {
            title: false,
            progress,
            ..State::default()
        };
        self.state.flow.revision = 1;
        self.state.flow.video_sequence = sequence;
        self.enter_event(0)?;
        self.advance()
    }
    fn program(&mut self, name: &str) -> Result<&Vec<Token>> {
        if !self.programs.contains_key(name) {
            let source = self.assets.text(name).map_err(|e| e.to_string())?;
            self.programs.insert(
                name.to_owned(),
                kag::parse(&source).map_err(|e| format!("{name}:{e}"))?,
            );
        }
        Ok(&self.programs[name])
    }
    pub fn validate_state(&mut self, state: &State) -> Result<()> {
        if state.flow.revision > 1 {
            return Err("save requires a newer scenario runtime".into());
        }
        if state.flow.variables.len() > 4096
            || state.flow.conditions.len() > 64
            || state.flow.macro_stack.len() > 64
            || state.flow.macros.len() > 256
            || state.flow.call_conditions.len() > 64
            || state.flow.selected.len() > 256
            || state.flow.choices.len() > 32
            || state.flow.wait_ms > 86400000
            || state.flow.visited.len() > 2048
            || state.flow.staging.len() > 128
            || state.progress.events.len() > 8192
            || state.progress.cg.len() > 8192
            || state.progress.endings.len() > 256
            || state.progress.variables.len() > 8192
        {
            return Err("saved scenario state exceeds limits".into());
        }
        if serde_json::to_vec(&state.flow.variables)
            .map_err(|e| e.to_string())?
            .len()
            > 2 * 1024 * 1024
            || serde_json::to_vec(&state.progress.variables)
                .map_err(|e| e.to_string())?
                .len()
                > 2 * 1024 * 1024
        {
            return Err("saved variable size limit".into());
        }
        if state.flow.replay != state.flow.replay_return.is_some()
            || state
                .flow
                .replay_return
                .as_ref()
                .is_some_and(|s| s.flow.replay || s.flow.replay_return.is_some())
        {
            return Err("invalid replay return state".into());
        }
        if let Some(previous) = &state.flow.replay_return {
            if !previous.title {
                self.validate_state(previous)?;
            } else if !previous.cursor.script.is_empty() || !previous.stack.is_empty() {
                return Err("invalid replay title return".into());
            }
        }
        if state.flow.revision == 1 {
            let event = state
                .flow
                .event_id
                .and_then(|id| self.database.events.get(id))
                .ok_or("saved event ID missing")?;
            if state.flow.route != Some(event.route) {
                return Err("saved route does not match event".into());
            }
            if state.flow.call_conditions.len() != state.stack.len() {
                return Err("saved call/control stacks disagree".into());
            }
            if state.finished
                && (!event.terminal
                    || state.flow.ending.as_deref() != Some(&format!("route:{}", event.route)))
            {
                return Err("finished save has no authentic ending".into());
            }
        }
        for (selection, id) in &state.flow.selected {
            if !self
                .database
                .selections
                .get(*selection)
                .is_some_and(|s| *id < s.text.len())
            {
                return Err("saved selected branch out of bounds".into());
            }
        }
        if !state.flow.choices.is_empty() {
            let id = state
                .flow
                .selection_id
                .ok_or("pending choice group missing")?;
            let selection = self
                .database
                .selections
                .get(id)
                .ok_or("pending choice group invalid")?;
            let expected: Vec<_> = selection
                .text
                .iter()
                .enumerate()
                .map(|(id, text)| Choice {
                    id,
                    text: text.clone(),
                    enabled: true,
                })
                .collect();
            if state.flow.choices != expected
                || self.database.index(&state.cursor.script)
                    != self.database.index(&selection.script)
            {
                return Err("saved choices do not match the active database group".into());
            }
            if state.scene.video.is_some() || state.flow.wait_ms > 0 {
                return Err("choice overlaps a timed/media wait".into());
            }
        }
        for frame in &state.flow.macro_stack {
            if frame.params.len() > 128 || frame.conditions.len() > 64 {
                return Err("saved macro frame size limit".into());
            }
        }
        for def in state.flow.macros.values() {
            let p = self.program(&def.script)?;
            if def.start == 0
                || def.start > def.end
                || !matches!(p.get(def.start-1).map(|t|&t.kind),Some(Kind::Tag{name,..}) if name.eq_ignore_ascii_case("macro"))
                || !matches!(p.get(def.end).map(|t|&t.kind),Some(Kind::Tag{name,..}) if name.eq_ignore_ascii_case("endmacro"))
            {
                return Err("invalid saved macro definition".into());
            }
        }
        if let Some(video) = &state.scene.video {
            if video.sequence != state.flow.video_sequence {
                return Err("saved video sequence mismatch".into());
            }
            self.assets
                .find(&video.asset, &["mp4"])
                .map_err(|e| e.to_string())?;
        }
        if let Some(asset) = &state.flow.pending_video {
            self.assets
                .find(asset, &["mp4"])
                .map_err(|e| e.to_string())?;
        }
        if let Some(credits) = &state.scene.credits {
            if credits.text.len() > 1024 * 1024
                || credits.images.len() > 32
                || credits.commands.len() > 16384
                || credits.duration_ms > 86400000
                || credits.elapsed_ms > credits.duration_ms
                || credits.left > WIDTH as u32
                || credits.width > WIDTH as u32
                || credits.start_disp > HEIGHT as u32
                || credits.end_disp > HEIGHT as u32
            {
                return Err("invalid saved credits".into());
            }
            if serde_json::to_vec(&credits.commands)
                .map_err(|e| e.to_string())?
                .len()
                > 2 * 1024 * 1024
            {
                return Err("saved credits command size limit".into());
            }
            for image in &credits.images {
                self.assets
                    .find(image, &["webp", "png"])
                    .map_err(|e| e.to_string())?;
            }
        }
        if state.visual_revision > 1 {
            return Err("save requires a newer visual runtime".into());
        }
        if let Some(interlude) = &state.interlude {
            self.assets
                .find(&interlude.background, &["webp", "png"])
                .map_err(|e| e.to_string())?;
            if !interlude.bgm.is_empty() && interlude.bgm != "stop" {
                self.assets
                    .find(&interlude.bgm, &["opus", "ogg"])
                    .map_err(|e| e.to_string())?;
            }
        }
        if state.title || state.blocked.is_some() || state.cursor.script.is_empty() {
            return Err("save does not contain a resumable story".into());
        }
        if state.history.len() > 300
            || state.stack.len() > 64
            || state.scene.layers.len() > 128
            || state.warnings.len() > 64
            || state.sprites.len() > 32
        {
            return Err("save state exceeds limits".into());
        }
        for cursor in std::iter::once(&state.cursor)
            .chain(state.stack.iter())
            .chain(state.flow.macro_stack.iter().map(|f| &f.cursor))
        {
            if cursor.script.is_empty() && state.title {
                continue;
            }
            if cursor.token > self.program(&cursor.script)?.len() {
                return Err("save program position out of bounds".into());
            }
        }
        if state.scene.visual.previous.len() > 128 || state.scene.visual.commands.len() > 4096 {
            return Err("invalid saved animation bounds".into());
        }
        crate::animation::validate(&state.scene)?;
        if state.flow.call_conditions.iter().any(|c| c.len() > 64) {
            return Err("saved call condition limit".into());
        }
        for layer in state
            .scene
            .layers
            .iter()
            .chain(state.scene.visual.previous.iter())
        {
            if ![layer.x, layer.y, layer.width, layer.height, layer.opacity]
                .iter()
                .all(|n| n.is_finite())
                || layer.width < 0.0
                || layer.height < 0.0
                || !(0.0..=1.0).contains(&layer.opacity)
            {
                return Err("invalid saved layer".into());
            }
            self.assets
                .find(&layer.asset, &[])
                .map_err(|e| e.to_string())?;
        }
        for (channel, audio) in [&state.scene.bgm, &state.scene.voice, &state.scene.se]
            .into_iter()
            .enumerate()
        {
            if audio
                .volume
                .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
                || audio.fade_ms > 86400000
            {
                return Err("invalid saved audio settings".into());
            }
            if !audio.asset.is_empty() {
                match self.assets.find(&audio.asset, &["opus", "ogg"]) {
                    Err(xp3::Error::Missing(_)) if audio.missing && channel == 2 => (),
                    Ok(_) if !audio.missing => (),
                    Ok(_) => return Err("saved missing audio request now resolves".into()),
                    Err(e) => return Err(e.to_string()),
                }
            } else if audio.missing {
                return Err("missing audio request has no identifier".into());
            }
        }
        Ok(())
    }
    pub fn previous_pages(&self) -> impl Iterator<Item = &State> {
        self.checkpoints.iter()
    }
    /// Restore committed pages, including the last page while its successor is open.
    pub fn restore_checkpoints(&mut self, states: Vec<State>) -> Result<()> {
        if states.len() > 100 {
            return Err("rollback window exceeds limit".into());
        }
        let mut previous = None;
        for state in &states {
            self.validate_state(state)?;
            if state.title
                || state.pages > self.state.pages
                || (state.pages == self.state.pages
                    && (!self.state.flow.page_open || state.flow.page_open))
                || previous.is_some_and(|p| state.pages <= p)
            {
                return Err("invalid rollback page ordering".into());
            }
            previous = Some(state.pages);
        }
        self.checkpoints = states.into();
        Ok(())
    }
    pub fn rewind(&mut self, page: u64) -> Result<()> {
        let index = self
            .checkpoints
            .iter()
            .position(|state| state.pages == page)
            .ok_or("page is outside the rollback window")?;
        let mut state = self.checkpoints[index].clone();
        merge_progress(&mut state.progress, &self.state.progress);
        sync_globals(&mut state);
        self.checkpoints.truncate(index);
        self.state = state;
        Ok(())
    }
    pub fn restore(&mut self, state: State) -> Result<()> {
        self.validate_state(&state)?;
        let mut state = state;
        let legacy = state.flow.revision == 0 || state.visual_revision == 0;
        if !legacy {
            if self.state != state {
                if let Some(index) = self.checkpoints.iter().position(|s| s == &state) {
                    self.checkpoints.truncate(index);
                } else {
                    self.checkpoints.clear();
                }
            }
            merge_progress(&mut state.progress, &self.state.progress);
            self.state = state;
            return Ok(());
        }
        let mut replay = Self::open(self.assets.clone())?;
        replay.dimensions.clone_from(&self.dimensions);
        replay.programs.clone_from(&self.programs);
        replay.state.progress.clone_from(&state.progress);
        merge_progress(&mut replay.state.progress, &self.state.progress);
        replay.start()?;
        for _ in 0..100000 {
            if replay.state.cursor == state.cursor
                && replay.state.stack == state.stack
                && replay.state.interlude.is_some() == state.interlude.is_some()
            {
                let progress = self.state.progress.clone();
                merge_progress(&mut replay.state.progress, &progress);
                sync_globals(&mut replay.state);
                self.checkpoints = replay.checkpoints;
                self.state = replay.state;
                return Ok(());
            }
            if state.finished && state.flow.event_id.is_none() {
                let previous = self.database.index(&state.cursor.script);
                if previous == Some(0) && replay.state.flow.event_id == Some(1) {
                    merge_progress(&mut replay.state.progress, &self.state.progress);
                    sync_globals(&mut replay.state);
                    *self = replay;
                    return Ok(());
                }
            }
            if replay.state.finished {
                break;
            }
            let result = if !replay.state.flow.choices.is_empty() {
                let selection = replay
                    .state
                    .flow
                    .selection_id
                    .ok_or("selection ID missing")?;
                if let Some(id) = state.flow.selected.get(&selection) {
                    replay.choose(*id)
                } else {
                    break;
                }
            } else if let Some(video) = &replay.state.scene.video {
                replay.finish_video(video.sequence)
            } else if replay.state.flow.wait_ms > 0 {
                replay.tick(replay.state.flow.wait_ms).map(|_| ())
            } else {
                replay.advance()
            };
            if result.is_err() {
                break;
            }
        }
        Err("legacy save cannot be migrated without inventing branch state".into())
    }
    pub fn advance(&mut self) -> Result<()> {
        if self.state.title || self.state.finished {
            return Ok(());
        }
        if let Some(error) = &self.state.blocked {
            return Err(error.clone());
        }
        if !self.state.flow.choices.is_empty() {
            return Err("a selection requires choose(id)".into());
        }
        if self.state.scene.video.is_some() {
            return Err("video requires sequence-checked completion".into());
        }
        if self.state.flow.wait_ms > 0 {
            self.register_input()?;
            if !self.state.flow.wait_skippable {
                return Ok(());
            }
            crate::animation::tick(&mut self.state.scene, self.state.flow.wait_ms);
            self.state.flow.wait_ms = 0;
            self.skip_ending()?;
            return self.run();
        }
        if !self.state.flow.page_open {
            self.checkpoint();
            self.state.scene.text.clear();
            self.state.scene.speaker.clear();
            self.state.scene.voice.asset.clear();
            self.state.scene.voice.sequence = self.state.scene.voice.sequence.saturating_add(1);
            self.state.flow.page_open = true;
        }
        self.register_input()?;
        if let Some(interlude) = self.state.interlude.take() {
            self.clear_sprites();
            self.background(&interlude.background)?;
            self.audio(&interlude.bgm, "bgm", true)?;
            self.state.scene.message_visible = !tjs::truth(
                self.state
                    .flow
                    .variables
                    .get("tf.EyeCatchEnd")
                    .unwrap_or(&Value::Null),
            );
        }
        self.run()
    }
    fn checkpoint(&mut self) {
        if self.state.pages > 0 {
            self.checkpoints.push_back(self.state.clone());
            if self.checkpoints.len() > 100 {
                self.checkpoints.pop_front();
            }
        }
    }
    fn run(&mut self) -> Result<()> {
        if !self.state.flow.replay {
            sync_globals(&mut self.state);
        }
        for _ in 0..10000 {
            if !self.state.flow.staging.is_empty() {
                let command = self.state.flow.staging.remove(0);
                match self.command(&command.name, &command.args) {
                    Ok(true) => return Ok(()),
                    Ok(false) => continue,
                    Err(error) => {
                        let message = format!(
                            "{}:token {}: staged [{}]: {error}",
                            self.state.cursor.script, self.state.cursor.token, command.name
                        );
                        self.state.blocked = Some(message.clone());
                        return Err(message);
                    }
                }
            }
            let script = self.state.cursor.script.clone();
            let index = self.state.cursor.token;
            let token = self.program(&script)?.get(index).cloned();
            let Some(token) = token else {
                let message = format!("{script}:EOF: unexpected end without SC_EndProcess/return");
                self.state.blocked = Some(message.clone());
                return Err(message);
            };
            self.state
                .flow
                .visited
                .insert(script.replace('\\', "/").to_lowercase());
            self.state.cursor.token += 1;
            let replay_before = self.state.flow.replay;
            let result = match &token.kind {
                Kind::Text(text) => {
                    if self.active() {
                        self.state.scene.text.push_str(text);
                    }
                    Ok(false)
                }
                Kind::Label(_) => Ok(false),
                Kind::Script(source) => {
                    if self.active() {
                        self.execute(source).map(|_| false)
                    } else {
                        Ok(false)
                    }
                }
                Kind::Tag { name, args } => self.dispatch(name, args),
            };
            match result {
                Ok(true) => {
                    if replay_before && !self.state.flow.replay {
                        return Ok(());
                    }
                    if !self.state.flow.page_open || !self.state.flow.choices.is_empty() {
                        self.state.pages = self.state.pages.saturating_add(1);
                    }
                    if !self.state.flow.page_open && !self.state.scene.text.is_empty() {
                        self.state.history.push(format!(
                            "{}\n{}",
                            self.state.scene.speaker, self.state.scene.text
                        ));
                        if self.state.history.len() > 300 {
                            self.state.history.remove(0);
                        }
                    }
                    return Ok(());
                }
                Ok(false) => (),
                Err(error) => {
                    let message = format!("{script}:{}: {error}", token.line);
                    self.state.blocked = Some(message.clone());
                    return Err(message);
                }
            }
        }
        let message = format!(
            "{}:token {}: script instruction budget exceeded",
            self.state.cursor.script, self.state.cursor.token
        );
        self.state.blocked = Some(message.clone());
        Err(message)
    }
    fn active(&self) -> bool {
        self.state.flow.conditions.last().is_none_or(|c| c.active)
    }
    fn register_input(&mut self) -> Result<()> {
        let count = self
            .state
            .flow
            .variables
            .get("kag.clickCount")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("input sequence overflow")?;
        self.state
            .flow
            .variables
            .insert("kag.clickCount".into(), Value::from(count));
        Ok(())
    }
    fn skip_ending(&mut self) -> Result<()> {
        let variables = &self.state.flow.variables;
        if tjs::truth(variables.get("sf.ED_Cancel").unwrap_or(&Value::Null))
            && variables
                .get("tf.ClickCount")
                .and_then(Value::as_u64)
                .zip(variables.get("kag.clickCount").and_then(Value::as_u64))
                .is_some_and(|(before, now)| now > before)
            && self.state.scene.credits.is_some()
        {
            self.jump(
                &BTreeMap::from([
                    ("storage".into(), "endroll.ks".into()),
                    ("target".into(), "*endroll_end".into()),
                ]),
                false,
            )?;
        }
        Ok(())
    }
    fn execute(&mut self, source: &str) -> Result<()> {
        tjs::execute(source, &mut self.state.flow.variables)?;
        if !self.state.flow.replay {
            for (key, value) in &self.state.flow.variables {
                if key.starts_with("sf.") {
                    self.state
                        .progress
                        .variables
                        .insert(key.clone(), value.clone());
                }
            }
        }
        Ok(())
    }
    fn enter_event(&mut self, index: usize) -> Result<()> {
        let event = self
            .database
            .events
            .get(index)
            .ok_or("event index out of range")?
            .clone();
        let script = self
            .assets
            .find(&event.script, &["ks"])
            .map_err(|e| e.to_string())?;
        self.state.cursor = Cursor { script, token: 0 };
        self.state.flow.event_id = Some(index);
        self.state.flow.route = Some(event.route);
        self.state.flow.conditions.clear();
        self.state
            .flow
            .variables
            .insert("f.EV_NowEventNum".into(), Value::from(index));
        self.state
            .flow
            .variables
            .insert("tf.EV_DirectJump".into(), Value::from(-1));
        self.state
            .flow
            .variables
            .insert("tf.EV_SelectNowJump".into(), Value::Bool(false));
        self.state.flow.variables.insert(
            "tf.SCENE_Enabled".into(),
            Value::Bool(self.state.flow.scene_replay),
        );
        if !self.state.flow.replay {
            self.state.progress.events.insert(event.script);
        }
        sync_globals(&mut self.state);
        Ok(())
    }
    fn complete_event(&mut self, args: &BTreeMap<String, String>) -> Result<()> {
        if !self.state.stack.is_empty() || !self.state.flow.macro_stack.is_empty() {
            return Err("event completion inside unresolved call/macro".into());
        }
        if self.state.flow.replay {
            return self.end_replay();
        }
        let current = self.state.flow.event_id.ok_or("event not initialized")?;
        let direct = args.get("storage").cloned().or_else(|| {
            self.state
                .flow
                .variables
                .get("tf.EV_DirectJump")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
        let next = if let Some(direct) = direct {
            Some(
                self.database
                    .index(&direct)
                    .ok_or("unknown direct event destination")?,
            )
        } else {
            self.database.next(current)?
        };
        if let Some(index) = next {
            self.enter_event(index)?;
        } else {
            let event = &self.database.events[current];
            let ending = format!("route:{}", event.route);
            self.state.flow.ending = Some(ending.clone());
            self.state.progress.endings.insert(ending);
            for key in ["sf.ADV_EX_Enabled", "sf.ADV_EV_Enabled"] {
                self.state
                    .progress
                    .variables
                    .insert(key.into(), Value::Bool(true));
                self.state
                    .flow
                    .variables
                    .insert(key.into(), Value::Bool(true));
            }
            self.state.finished = true;
        }
        Ok(())
    }
    fn end_replay(&mut self) -> Result<()> {
        let previous = self
            .state
            .flow
            .replay_return
            .take()
            .ok_or("replay return state missing")?;
        let progress = self.state.progress.clone();
        self.state = *previous;
        merge_progress(&mut self.state.progress, &progress);
        Ok(())
    }
    fn dispatch(&mut self, name: &str, args: &BTreeMap<String, String>) -> Result<bool> {
        let lower = name.to_lowercase();
        if matches!(lower.as_str(), "if" | "elsif" | "else" | "endif") {
            match lower.as_str() {
                "if" => {
                    if self.state.flow.conditions.len() >= 64 {
                        return Err("conditional nesting limit".into());
                    }
                    let parent = self.active();
                    let active = parent
                        && tjs::truth(&tjs::eval(
                            args.get("exp").ok_or("if expression missing")?,
                            &self.state.flow.variables,
                        )?);
                    self.state.flow.conditions.push(Conditional {
                        parent,
                        active,
                        taken: active,
                        otherwise: false,
                    });
                }
                "endif" => {
                    self.state.flow.conditions.pop().ok_or("endif without if")?;
                }
                _ => {
                    let c = self
                        .state
                        .flow
                        .conditions
                        .last()
                        .ok_or("branch without if")?
                        .clone();
                    if c.otherwise {
                        return Err("branch after else".into());
                    }
                    let active = c.parent
                        && !c.taken
                        && (lower == "else"
                            || tjs::truth(&tjs::eval(
                                args.get("exp").ok_or("elsif expression missing")?,
                                &self.state.flow.variables,
                            )?));
                    let c = self.state.flow.conditions.last_mut().unwrap();
                    c.active = active;
                    c.taken |= active;
                    c.otherwise = lower == "else";
                }
            }
            return Ok(false);
        }
        if lower == "macro" {
            let start = self.state.cursor.token;
            let script = self.state.cursor.script.clone();
            let program = self.program(&script)?;
            let end=(start..program.len()).find(|i|matches!(&program[*i].kind,Kind::Tag{name,..} if name.eq_ignore_ascii_case("endmacro"))).ok_or("macro end missing")?;
            if self.active() {
                if self.state.flow.macros.len() >= 256 {
                    return Err("macro count limit".into());
                }
                let name = args.get("name").ok_or("macro name missing")?;
                self.state
                    .flow
                    .macros
                    .insert(name.to_lowercase(), MacroDefinition { script, start, end });
            }
            self.state.cursor.token = end + 1;
            return Ok(false);
        }
        if !self.active() {
            return Ok(false);
        }
        if let Some(cond) = args.get("cond") {
            if !tjs::truth(&tjs::eval(cond, &self.state.flow.variables)?) {
                return Ok(false);
            }
        }
        let mut resolved = BTreeMap::new();
        for (key, value) in args {
            if key == "cond" {
                continue;
            }
            if key == "*" {
                if let Some(frame) = self.state.flow.macro_stack.last() {
                    resolved.extend(frame.params.clone());
                }
                continue;
            }
            // Video volume is owned by the native audio settings, not the old Windows mixer.
            if lower == "video" && key == "volume" && value == "&kag.se[BufMO[0]].volume2\\1000" {
                continue;
            }
            let value = if key == "exp" {
                value.clone()
            } else if let Some(exp) = value.strip_prefix('&') {
                tjs::string(&tjs::eval(exp, &self.state.flow.variables)?)
            } else if let Some(param) = value.strip_prefix('%') {
                let (param, default) = param.split_once('|').unwrap_or((param, ""));
                self.state
                    .flow
                    .macro_stack
                    .last()
                    .and_then(|f| f.params.get(param))
                    .map(String::as_str)
                    .unwrap_or(default)
                    .into()
            } else {
                value.clone()
            };
            resolved.insert(key.clone(), value);
        }
        if lower == "endmacro" {
            let frame = self
                .state
                .flow
                .macro_stack
                .pop()
                .ok_or("endmacro without macro call")?;
            self.state.cursor = frame.cursor;
            self.state.flow.conditions = frame.conditions;
            self.refresh_params();
            return Ok(false);
        }
        if let Some(def) = self.state.flow.macros.get(&lower).cloned() {
            if self.state.flow.macro_stack.len() >= 64 {
                return Err("macro stack limit".into());
            }
            self.state.flow.macro_stack.push(MacroFrame {
                cursor: self.state.cursor.clone(),
                params: resolved,
                conditions: std::mem::take(&mut self.state.flow.conditions),
            });
            self.state.cursor = Cursor {
                script: def.script,
                token: def.start,
            };
            self.refresh_params();
            return Ok(false);
        }
        self.command(name, &resolved)
    }
    fn refresh_params(&mut self) {
        self.state
            .flow
            .variables
            .retain(|key, _| !key.starts_with("mp."));
        if let Some(frame) = self.state.flow.macro_stack.last() {
            for (key, value) in &frame.params {
                self.state
                    .flow
                    .variables
                    .insert(format!("mp.{key}"), Value::String(value.clone()));
            }
        }
    }
    fn background(&mut self, name: &str) -> Result<()> {
        let asset = self
            .assets
            .find(name, &["webp", "png"])
            .map_err(|e| e.to_string())?;
        if !self.state.flow.replay {
            let base = asset.rsplit('/').next().unwrap_or(&asset);
            let group = base.split('_').next().unwrap_or(base);
            if group.starts_with("ev")
                && group.len() > 2
                && group[2..].chars().all(|c| c.is_ascii_digit())
            {
                self.state.progress.cg.insert(group.into());
                self.state.progress.cg.insert(format!("asset:{asset}"));
            }
        }
        self.state.scene.layers.retain(|layer| layer.group != 0);
        self.state.scene.layers.insert(
            0,
            Layer {
                asset,
                x: 0.0,
                y: 0.0,
                width: WIDTH,
                height: HEIGHT,
                opacity: 1.0,
                group: 0,
            },
        );
        Ok(())
    }
    fn audio(&mut self, name: &str, channel: &str, looping: bool) -> Result<()> {
        let mut missing = false;
        let asset = if name.is_empty() || name == "stop" {
            String::new()
        } else {
            match self.assets.find(name, &["opus", "ogg"]) {
                Ok(asset) => asset,
                Err(xp3::Error::Missing(_)) if channel == "se" => {
                    // Original PlaySE reports absent effects and returns. Keep
                    // the unresolved request visible to native per-clip recovery.
                    missing = true;
                    name.into()
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        let audio = match channel {
            "bgm" => &mut self.state.scene.bgm,
            "voice" => &mut self.state.scene.voice,
            _ => &mut self.state.scene.se,
        };
        audio.asset = asset;
        audio.missing = missing;
        audio.sequence = audio
            .sequence
            .checked_add(1)
            .ok_or("audio sequence overflow")?;
        audio.looping = looping;
        Ok(())
    }
    fn size(&mut self, asset: &str) -> Result<(u32, u32)> {
        if let Some(size) = self.dimensions.get(asset) {
            return Ok(*size);
        }
        let bytes = self.assets.bytes(asset).map_err(|e| e.to_string())?;
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let size = reader.into_dimensions().map_err(|e| e.to_string())?;
        self.dimensions.insert(asset.into(), size);
        Ok(size)
    }
    fn sprite(&mut self, args: &BTreeMap<String, String>) -> Result<()> {
        let number = args
            .get("ch")
            .ok_or("stc missing ch")?
            .parse::<u32>()
            .map_err(|_| "invalid sprite character")?;
        let previous = self.state.sprites.get(&number).cloned();
        let mut sprite = Sprite {
            size: 2,
            dress: 1,
            expression: "111".into(),
            position: "pc".into(),
            parts: 0,
        };
        if let Some(value) = args.get("f") {
            sprite.expression = value.clone();
        }
        if let Some(value) = args.get("p") {
            sprite.parts = value.parse().map_err(|_| "bad sprite parts")?;
        }
        for key in args.keys() {
            if let Some(n) = key.strip_prefix("dr") {
                if let Ok(n) = n.parse() {
                    sprite.dress = n;
                }
            }
            if let Some(n) = key.strip_prefix('s') {
                if let Ok(n) = n.parse() {
                    sprite.size = n;
                }
            }
            if [
                "pc", "pl", "pr", "pll", "prr", "plll", "prrr", "p4ll", "p4l", "p4r", "p4rr",
                "p5ll", "p5l", "p5c", "p5r", "p5rr", "p6lll", "p6ll", "p6l", "p6r", "p6rr",
                "p6rrr", "p0",
            ]
            .contains(&key.as_str())
            {
                sprite.position = key.clone();
            }
        }
        if let Some(value) = args.get("dr") {
            sprite.dress = value.parse().map_err(|_| "bad dress")?;
        }
        if let Some(value) = args.get("size") {
            sprite.size = value.parse().map_err(|_| "bad size")?;
        }
        if args.contains_key("fc") || args.get("fcenabled").is_some_and(|v| v == "true") {
            sprite.position = "fc".into();
            sprite.size = 3;
        }
        if sprite.size > 4 || sprite.expression.len() < 3 || !sprite.expression.is_ascii() {
            return Err("unsupported sprite expression/size".into());
        }
        let direction: u32 = sprite.expression[1..2]
            .parse()
            .map_err(|_| "bad direction")?;
        let face = &sprite.expression[2..];
        let cheek = &sprite.expression[..1];
        let suffix = format!("{number}_s{}_{}_", sprite.size, direction);
        let base = self
            .assets
            .find(&format!("stb{suffix}{}", sprite.dress), &["webp"])
            .map_err(|e| e.to_string())?;
        let (width, _) = self.size(&base)?;
        let (mut x, mut y) = if sprite.position == "fc" {
            (
                -180.0 + value_at(&self.tables, &format!("STI_FCAX[{number}]"), &[direction])?,
                960.0 + value_at(&self.tables, &format!("STI_FCAY[{number}]"), &[direction])?,
            )
        } else {
            let center = match sprite.position.as_str() {
                "pc" | "p0" | "p5c" => 0.5,
                "pl" => 0.25,
                "pr" => 0.75,
                "pll" => 0.125,
                "prr" => 0.875,
                "plll" => 0.1,
                "prrr" | "p4rr" => 0.8,
                "p4ll" => 0.2,
                "p4l" => 0.4,
                "p4r" => 0.6,
                "p5ll" => 1.0 / 6.0,
                "p5l" => 2.0 / 6.0,
                "p5r" => 4.0 / 6.0,
                "p5rr" => 5.0 / 6.0,
                "p6lll" => 1.0 / 7.0,
                "p6ll" => 2.0 / 7.0,
                "p6l" => 3.0 / 7.0,
                "p6r" => 4.0 / 7.0,
                "p6rr" => 5.0 / 7.0,
                "p6rrr" => 6.0 / 7.0,
                _ => return Err(format!("unsupported sprite position {}", sprite.position)),
            };
            let pad = match sprite.size {
                0 => 20.0,
                1 => 30.0,
                2 => 50.0,
                3 => 0.0,
                _ => -50.0,
            };
            let side = match sprite.position.as_str() {
                "pl" | "pll" | "p4rr" | "p6rr" => pad,
                "pr" | "prr" | "p4ll" | "p6ll" => -pad,
                "p6lll" => -2.0 * pad,
                "p6rrr" => 2.0 * pad,
                _ => 0.0,
            };
            (
                WIDTH * center - (width / 2) as f32
                    + side
                    + value_at(
                        &self.tables,
                        &format!("STI_AX[{number}]"),
                        &[sprite.size, direction],
                    )?,
                value_at(&self.tables, &format!("STI_AH[{number}]"), &[sprite.size])?
                    + value_at(
                        &self.tables,
                        &format!("STI_AY[{number}]"),
                        &[sprite.size, direction],
                    )?
                    + value_at(&self.tables, &format!("STI_HAllPos[{}]", sprite.size), &[])?,
            )
        };
        if sprite.position == "p0" && previous.is_some() {
            if let Some(layer) = self.state.scene.layers.iter().find(|l| l.group == number) {
                x = layer.x;
                y = layer.y;
            }
        }
        for (key, pos) in [("dx", &mut x), ("dy", &mut y)] {
            if let Some(value) = args.get(key) {
                *pos += value
                    .parse::<f32>()
                    .map_err(|_| "unsupported sprite offset")?;
            }
        }
        let sprites = &self.state.sprites;
        self.state.scene.layers.retain(|layer| {
            layer.group != number
                && (sprite.position != "fc"
                    || !sprites
                        .get(&layer.group)
                        .is_some_and(|s| s.position == "fc"))
        });
        if sprite.position == "fc" {
            self.state.sprites.retain(|_, s| s.position != "fc");
        }
        let mut images = vec![
            base,
            self.assets
                .find(&format!("st{suffix}{face}"), &["webp"])
                .map_err(|e| e.to_string())?,
        ];
        let cheek_override = self
            .tables
            .get(&format!("STI_NoCheekNum[{number}][{direction}][{face}]"))
            .and_then(Value::as_i64)
            .map(|n| n.to_string());
        let cheek = cheek_override.as_deref().unwrap_or(cheek);
        if cheek != "0" {
            if let Ok(asset) = self.assets.find(&format!("stc{suffix}{cheek}"), &["webp"]) {
                images.push(asset);
            }
        }
        for parts in [
            sprite.parts.to_string(),
            args.get("p2").cloned().unwrap_or_default(),
        ] {
            if !parts.is_empty() && parts != "0" {
                if let Ok(asset) = self.assets.find(&format!("stp{suffix}{parts}"), &["webp"]) {
                    if !images.contains(&asset) {
                        images.push(asset);
                    }
                }
            }
        }
        for asset in images {
            let (width, height) = self.size(&asset)?;
            self.state.scene.layers.push(Layer {
                asset,
                x,
                y,
                width: width as f32,
                height: height as f32,
                opacity: 1.0,
                group: number,
            });
        }
        self.state.scene.layers.sort_by_key(|layer| layer.group);
        self.state.sprites.insert(number, sprite);
        Ok(())
    }
    fn jump_cursor(&mut self, args: &BTreeMap<String, String>) -> Result<Cursor> {
        let script = if let Some(storage) = args.get("storage") {
            if storage.starts_with('&') {
                return Err("dynamic TJS jump is not supported".into());
            }
            self.assets
                .find(storage, &["ks"])
                .map_err(|e| e.to_string())?
        } else {
            self.state.cursor.script.clone()
        };
        let position = if let Some(target) = args.get("target") {
            let label = target.trim_start_matches('*');
            self.program(&script)?
                .iter()
                .position(|token| matches!(&token.kind,Kind::Label(name) if name==label))
                .ok_or_else(|| format!("label missing: {label}"))?
                + 1
        } else {
            0
        };
        Ok(Cursor {
            script,
            token: position,
        })
    }
    fn jump(&mut self, args: &BTreeMap<String, String>, call: bool) -> Result<()> {
        let cursor = self.jump_cursor(args)?;
        if call {
            if self.state.stack.len() >= 64 {
                return Err("call stack limit".into());
            }
            self.state.stack.push(self.state.cursor.clone());
            self.state
                .flow
                .call_conditions
                .push(std::mem::take(&mut self.state.flow.conditions));
        } else {
            self.state.flow.conditions.clear();
        }
        self.state.cursor = cursor;
        Ok(())
    }
    fn clear_sprites(&mut self) {
        self.state.scene.layers.retain(|l| l.group == 0);
        self.state.sprites.clear();
    }
    fn command(&mut self, name: &str, args: &BTreeMap<String, String>) -> Result<bool> {
        let arg = |key: &str| args.get(key).map(String::as_str).unwrap_or("");
        match name.to_lowercase().as_str() {
            "c" | "p" | "l" => {
                self.state.flow.page_open = false;
                return Ok(true);
            }
            ">>" => self.state.scene.text.push_str(if arg("tx").is_empty() {
                "「"
            } else {
                arg("tx")
            }),
            "<<" => self.state.scene.text.push_str(if arg("tx").is_empty() {
                "」"
            } else {
                arg("tx")
            }),
            "m_bljump_linejump" | "indent" | "endindent" => (),
            "r" => self.state.scene.text.push('\n'),
            "er" | "cm" | "ct" => self.state.scene.text.clear(),
            "emb" => self.state.scene.text.push_str(&tjs::string(&tjs::eval(
                arg("exp"),
                &self.state.flow.variables,
            )?)),
            "eval" => self.execute(arg("exp"))?,
            "sc_startprocess" => {
                self.clear_sprites();
                self.state.scene.layers.clear();
                self.background(arg("bg"))?;
                self.audio(arg("bgm"), "bgm", true)?;
                self.state.scene.message_visible = flag(args, "ms_show", true)?;
                self.state
                    .flow
                    .variables
                    .insert("tf.EyeCatchEnd".into(), Value::Bool(false));
                self.state
                    .flow
                    .variables
                    .insert("f.HSceneCheck".into(), Value::Bool(false));
            }
            "stc" => {
                self.capture_visual()?;
                self.sprite(args)?;
                let group = arg("ch");
                self.image_style(args, group)?;
                return self.finish_visual(args, "standtrans");
            }
            "bgset" | "bgset_withoutblack" | "bgset_interval" => {
                let mut stages = Vec::new();
                let time = duration(
                    args,
                    "time",
                    *self.transition_speeds.get("sctrans").unwrap_or(&0),
                )?;
                let transition = |bg: &str| VisualCommand {
                    name: "__scene_target".into(),
                    args: BTreeMap::from([
                        ("bg".into(), bg.into()),
                        ("time".into(), time.to_string()),
                    ]),
                };
                if name.eq_ignore_ascii_case("bgset") && time > 0 {
                    stages.push(transition(if arg("type") == "white" {
                        "bgwhite"
                    } else {
                        "bgblack"
                    }));
                    stages.push(VisualCommand {
                        name: "wait".into(),
                        args: BTreeMap::from([("time".into(), "50".into())]),
                    });
                }
                if name.eq_ignore_ascii_case("bgset_interval") && !arg("bg1").is_empty() {
                    stages.push(transition(arg("bg1")));
                    stages.push(VisualCommand {
                        name: "wait".into(),
                        args: BTreeMap::from([("time".into(), "700".into())]),
                    });
                }
                let mut target = transition(if arg("bg").is_empty() {
                    arg("bg2")
                } else {
                    arg("bg")
                });
                for key in [
                    "ef", "method", "rule", "vague", "wait", "canskip", "zoom1", "zoom2",
                ] {
                    if let Some(value) = args.get(key) {
                        target.args.insert(key.into(), value.clone());
                    }
                }
                stages.push(target);
                if !arg("bgm").is_empty() {
                    self.audio(arg("bgm"), "bgm", true)?;
                }
                if !arg("se").is_empty() {
                    self.audio(&format!("se{}", arg("se")), "se", false)?;
                }
                self.state.scene.message_visible = true;
                self.state.flow.variables.insert(
                    "f.MS_BaseImageType".into(),
                    Value::String(if arg("ms").is_empty() { "0" } else { arg("ms") }.into()),
                );
                self.state.flow.staging.splice(0..0, stages);
            }
            "__scene_target" => {
                self.capture_visual()?;
                self.clear_sprites();
                self.state.scene.layers.retain(|l| l.group != 25);
                self.background(arg("bg"))?;
                self.image_style(args, "0")?;
                if !arg("ef").is_empty() {
                    self.stage_image(arg("ef"), "25", args)?;
                }
                return self.finish_visual(args, "sctrans");
            }
            "bg_image" => {
                self.capture_visual()?;
                self.background(arg("num"))?;
                self.image_style(args, "0")?;
                return self.finish_visual(
                    args,
                    if arg("num").to_lowercase().starts_with("ev") {
                        "evtrans"
                    } else {
                        "bgtrans"
                    },
                );
            }
            "hh" | "free_lay_reset" => {
                self.capture_visual()?;
                self.clear_sprites();
                return self.finish_visual(args, "standtrans");
            }
            "bgm_fp" | "bgm_p" | "bgm_fx" => {
                self.audio(arg("st"), "bgm", flag(args, "loop", true)?)?;
                self.state.scene.bgm.fade_ms = duration(
                    args,
                    "time",
                    if name.eq_ignore_ascii_case("bgm_fp") {
                        30
                    } else if name.eq_ignore_ascii_case("bgm_fx") {
                        500
                    } else {
                        0
                    },
                )?;
                if !arg("volume").is_empty() {
                    self.state.scene.bgm.volume = Some(
                        arg("volume")
                            .parse::<f32>()
                            .map_err(|_| "invalid BGM volume")?
                            / 100.0,
                    );
                }
            }
            "bgm_fe" | "bgm_stop" | "stopbgm" => {
                self.audio("", "bgm", false)?;
                self.state.scene.bgm.fade_ms = duration(
                    args,
                    "time",
                    if name.eq_ignore_ascii_case("bgm_fe") {
                        500
                    } else {
                        0
                    },
                )?;
            }
            "se" => {
                if flag(args, "stop", false)? {
                    self.audio("", "se", false)?;
                    let stop = arg("stop").parse::<u64>().unwrap_or(0);
                    self.state.scene.se.fade_ms = if stop > 1 {
                        duration(args, "stop", 0)?
                    } else {
                        0
                    };
                } else if !arg("play").is_empty() {
                    self.audio(arg("play"), "se", flag(args, "loop", false)?)?;
                    self.state.scene.se.fade_ms = duration(args, "time", 0)?;
                }
            }
            "voiceallstop" | "stopvoice" => self.audio("", "voice", false)?,
            "ms_hide" => {
                if !flag(args, "standhide", false)? {
                    self.clear_sprites();
                }
                self.state.scene.message_visible = false;
            }
            "ms_show" => self.state.scene.message_visible = true,
            "ms_c" => {
                self.state.flow.variables.insert(
                    "f.MS_BaseImageType".into(),
                    Value::String(arg("num").into()),
                );
            }
            "ms_frameopacityset" => {
                self.state.flow.variables.insert(
                    "f.MS_OpacityForce".into(),
                    Value::Bool(flag(args, "hvisible", false)?),
                );
            }
            "eyecatch" => {
                let end = flag(args, "end", false)?;
                if !end && arg("bg").is_empty() {
                    return Err("eyecatch continuation background missing".into());
                }
                let background = self
                    .assets
                    .find(if end { "bgblack" } else { arg("bg") }, &["webp", "png"])
                    .map_err(|e| e.to_string())?;
                if !arg("bgm").is_empty() && arg("bgm") != "stop" {
                    self.assets
                        .find(arg("bgm"), &["opus", "ogg"])
                        .map_err(|e| e.to_string())?;
                }
                self.clear_sprites();
                self.background(if arg("sp").is_empty() {
                    "eyecatch_Base"
                } else {
                    arg("sp")
                })?;
                self.audio("", "bgm", false)?;
                self.audio("", "voice", false)?;
                self.audio("", "se", false)?;
                self.state.scene.message_visible = arg("sp").is_empty();
                if arg("sp").is_empty() {
                    if args.contains_key("ch") && arg("nostand") != "true" {
                        self.sprite(args)?;
                    }
                    self.state.scene.text = format!("{}\n{}", arg("text"), arg("text2"));
                    if !arg("voice").is_empty() {
                        self.audio(arg("voice"), "voice", false)?;
                    }
                }
                self.state.interlude = Some(Interlude {
                    background,
                    bgm: if end {
                        String::new()
                    } else {
                        arg("bgm").into()
                    },
                });
                self.state
                    .flow
                    .variables
                    .insert("tf.EyeCatchEnd".into(), Value::Bool(end));
                self.state.flow.page_open = false;
                return Ok(true);
            }
            "wait" => {
                self.state.flow.wait_ms = duration(args, "time", 0)?;
                self.state.flow.wait_skippable = flag(args, "canskip", true)?;
                return Ok(self.state.flow.wait_ms > 0);
            }
            "ts" => {
                self.capture_visual()?;
                self.state
                    .flow
                    .variables
                    .insert("tf.trans_check".into(), Value::Bool(true));
            }
            "te" => {
                self.state
                    .flow
                    .variables
                    .insert("tf.trans_check".into(), Value::Bool(false));
                return self.finish_visual(args, "standtrans");
            }
            "whiteout" => {
                self.capture_visual()?;
                self.background(arg("bg"))?;
                return self.visual(
                    "whiteout",
                    &BTreeMap::from([("bg".into(), arg("bg").into())]),
                    None,
                );
            }
            "ef_image" => {
                self.capture_visual()?;
                self.stage_image(arg("num"), "25", args)?;
                return self.finish_visual(args, "bgtrans");
            }
            "ef_hide" => {
                self.capture_visual()?;
                self.state.scene.layers.retain(|l| l.group != 25);
                return self.finish_visual(args, "bgtrans");
            }
            "image" => {
                self.stage_image(
                    arg("storage"),
                    if arg("layer").is_empty() {
                        "0"
                    } else {
                        arg("layer")
                    },
                    args,
                )?;
            }
            "layopt" => {
                if let Some(message) = arg("layer").strip_prefix("message") {
                    let number: u32 = message
                        .parse()
                        .map_err(|_| "invalid native message layer")?;
                    let visible = flag(args, "visible", true)?;
                    match number {
                        0 => self.state.scene.message_visible = visible,
                        7 if !visible => self.state.scene.credits = None,
                        _ if !visible => (), // Explicitly hide an unused native message plane.
                        _ => return Err("unsupported visible native message plane".into()),
                    }
                    return Ok(false);
                }
                let group = if arg("layer") == "base" {
                    "0"
                } else {
                    arg("layer")
                };
                self.image_style(args, group)?;
                for layer in &mut self.state.scene.layers {
                    if layer.group.to_string() == group {
                        if let Some(x) = args.get("left") {
                            layer.x = x.parse().map_err(|_| "bad layer x")?;
                        }
                        if let Some(y) = args.get("top") {
                            layer.y = y.parse().map_err(|_| "bad layer y")?;
                        }
                    }
                }
            }
            "t_move" | "quake" | "trans" | "wt" | "move" | "wm" => {
                return self.visual(name, args, None)
            }
            "achievement" => (),
            "jump" => self.jump(args, false)?,
            "call" => self.jump(args, true)?,
            "return" => {
                self.state.cursor = self.state.stack.pop().ok_or("return with empty stack")?;
                self.state.flow.conditions = self
                    .state
                    .flow
                    .call_conditions
                    .pop()
                    .ok_or("missing call control frame")?;
            }
            "sc_endprocess" => {
                let was_replay = self.state.flow.replay;
                self.complete_event(args)?;
                if was_replay && !self.state.flow.replay {
                    return Ok(true);
                }
                return Ok(self.state.finished);
            }
            "sc_scenestartprocess" => {
                self.state
                    .flow
                    .variables
                    .insert("f.HSceneCheck".into(), Value::Bool(true));
                if !self.state.flow.replay {
                    let index = self.state.flow.event_id.ok_or("scene event missing")?;
                    self.state
                        .progress
                        .events
                        .insert(format!("scene:{}", self.database.events[index].script));
                }
            }
            "sc_sceneend" => {
                self.state
                    .flow
                    .variables
                    .insert("f.HSceneCheck".into(), Value::Bool(false));
                if self.state.flow.scene_replay {
                    self.end_replay()?;
                    return Ok(true);
                }
            }
            "sl_startselect" => {
                let id = arg("num")
                    .parse::<usize>()
                    .map_err(|_| "invalid selection ID")?;
                let row = self
                    .database
                    .selections
                    .get(id)
                    .ok_or("selection not in database")?
                    .clone();
                if self.database.index(&self.state.cursor.script)
                    != self.database.index(&row.script)
                {
                    return Err("selection invoked from the wrong event".into());
                }
                for id in 0..row.text.len() {
                    self.jump_cursor(&BTreeMap::from([(
                        "target".into(),
                        format!("*SLP_{}_{id}", arg("num")),
                    )]))?;
                }
                self.state.flow.selection_id = Some(id);
                self.state.flow.choices = row
                    .text
                    .into_iter()
                    .enumerate()
                    .map(|(id, text)| Choice {
                        id,
                        text,
                        enabled: true,
                    })
                    .collect();
                self.state
                    .flow
                    .variables
                    .insert("f.EV_ShowSelect".into(), Value::Bool(true));
            }
            "sl_endselect" => {
                let selection = self
                    .state
                    .flow
                    .selection_id
                    .ok_or("selection end without selection")?;
                if !self.state.flow.choices.is_empty()
                    || !self.state.flow.selected.contains_key(&selection)
                {
                    return Err("unselected branch cannot execute".into());
                }
                self.state
                    .flow
                    .variables
                    .insert("f.EV_ShowSelect".into(), Value::Bool(false));
                self.state.scene.text.clear();
            }
            "sl_join" => {
                let id = self
                    .state
                    .flow
                    .selection_id
                    .ok_or("join without selected group")?;
                if !self.state.flow.selected.contains_key(&id) {
                    return Err("join without chosen branch".into());
                }
                let row = self
                    .database
                    .selections
                    .get(id)
                    .ok_or("invalid join selection")?;
                let args = BTreeMap::from([
                    ("storage".into(), row.script.clone()),
                    ("target".into(), format!("*ENDSLP_{id}")),
                ]);
                self.jump(&args, false)?;
            }
            "s" => {
                if !self.state.flow.choices.is_empty() {
                    return Ok(true);
                }
                return Err("halt without a pending choice or authentic ending".into());
            }
            "openvideo" => {
                self.state.flow.pending_video = Some(
                    self.assets
                        .find(arg("storage"), &["mp4"])
                        .map_err(|e| e.to_string())?,
                );
            }
            "videolayer" | "preparevideo" | "wp" | "clearvideolayer" => (),
            "video" => {
                if flag(args, "loop", false)? {
                    return Err("unsupported looping scenario video".into());
                }
            }
            "playvideo" => {
                let asset = self
                    .state
                    .flow
                    .pending_video
                    .take()
                    .ok_or("playvideo without openvideo")?;
                self.state.flow.video_sequence = self
                    .state
                    .flow
                    .video_sequence
                    .checked_add(1)
                    .ok_or("video sequence overflow")?;
                self.state.scene.video = Some(Video {
                    asset,
                    sequence: self.state.flow.video_sequence,
                    skippable: false,
                    looping: false,
                });
            }
            "wv" => {
                let video = self
                    .state
                    .scene
                    .video
                    .as_mut()
                    .ok_or("video wait without active video")?;
                video.skippable = flag(args, "canskip", false)?;
                return Ok(true);
            }
            "stopvideo" => {
                self.state.scene.video = None;
            }
            "freeimage" => {
                let group = arg("layer")
                    .parse::<u32>()
                    .map_err(|_| "invalid freeimage layer")?;
                self.state.scene.layers.retain(|l| l.group != group);
                self.state.sprites.remove(&group);
                return self.native_visual(name, args, Vec::new(), false);
            }
            "clickskip" | "rclickapply" => (),
            "staffroll_init" => {
                self.state.scene.credits = Some(Credits {
                    left: duration(args, "left", 0)?
                        .try_into()
                        .map_err(|_| "credits left overflow")?,
                    width: duration(args, "width", WIDTH as u64)?
                        .try_into()
                        .map_err(|_| "credits width overflow")?,
                    ..Credits::default()
                });
            }
            "staffroll_set" => self.load_credits(arg("storage"))?,
            "staffroll_start" => {
                let credits = self
                    .state
                    .scene
                    .credits
                    .as_mut()
                    .ok_or("credits not initialized")?;
                credits.duration_ms = duration(args, "time", 115000)?;
                credits.elapsed_ms = 0;
                credits.start_disp = duration(args, "startdisp", 0)?
                    .try_into()
                    .map_err(|_| "credits start overflow")?;
                credits.end_disp = duration(args, "enddisp", 0)?
                    .try_into()
                    .map_err(|_| "credits end overflow")?;
                credits.skippable = tjs::truth(
                    self.state
                        .flow
                        .variables
                        .get("sf.ED_Cancel")
                        .unwrap_or(&Value::Null),
                );
            }
            "staffroll_wait" => {
                let credits = self
                    .state
                    .scene
                    .credits
                    .as_ref()
                    .ok_or("credits not initialized")?;
                self.state.flow.wait_ms = credits.duration_ms.saturating_sub(credits.elapsed_ms);
                self.state.flow.wait_skippable = flag(args, "canskip", false)?;
                return Ok(self.state.flow.wait_ms > 0);
            }
            "staffroll_end" => {
                self.state.scene.credits = None;
            }
            "ed_clickskip" => self.skip_ending()?,
            "end_image" => {
                let character = self
                    .state
                    .flow
                    .variables
                    .get("tf.ED_CharaNum")
                    .and_then(Value::as_u64)
                    .filter(|n| (1..=4).contains(n))
                    .ok_or("invalid ending character")?;
                let name = format!("ED_{character}_{}", arg("num"));
                self.capture_visual()?;
                if self.assets.find(&name, &["webp", "png"]).is_ok() {
                    self.background(&name)?;
                }
                self.finish_visual(
                    &BTreeMap::from([("time".into(), duration(args, "time", 3000)?.to_string())]),
                    "evtrans",
                )?;
                let total = self
                    .state
                    .flow
                    .variables
                    .get("tf.ED_AllTime")
                    .and_then(Value::as_u64)
                    .unwrap_or(115000);
                self.state.flow.wait_ms = duration(args, "time", 3000)?
                    .checked_add((total / 8).saturating_sub(3000))
                    .ok_or("ending duration overflow")?;
                self.state.flow.wait_skippable = tjs::truth(
                    self.state
                        .flow
                        .variables
                        .get("sf.ED_Cancel")
                        .unwrap_or(&Value::Null),
                );
                return Ok(true);
            }
            _ if name
                .strip_prefix('h')
                .is_some_and(|v| v.parse::<u32>().is_ok()) =>
            {
                let group = name[1..].parse::<u32>().unwrap();
                let before = self.state.scene.layers.clone();
                self.state.scene.layers.retain(|l| l.group != group);
                self.state.sprites.remove(&group);
                return self.native_visual(name, args, before, false);
            }
            "地" | "ネーム表示" | "ネーム表示_ボイス" | "複数" => {
                match name {
                    "地" => self.state.scene.speaker.clear(),
                    "ネーム表示" | "ネーム表示_ボイス" => {
                        self.state.scene.speaker = arg("name").to_owned()
                    }
                    "複数" => self.state.scene.speaker = arg("name").to_owned(),
                    _ => unreachable!(),
                }
                if !arg("vo").is_empty() {
                    self.audio(arg("vo"), "voice", false)?;
                }
            }
            _ if self.names.contains_key(name) => {
                self.state.scene.speaker = self.names[name].clone();
                if !arg("vo").is_empty() {
                    self.audio(arg("vo"), "voice", false)?;
                }
            }
            _ => return self.visual(name, args, None),
        }
        Ok(false)
    }
    fn visual(
        &mut self,
        name: &str,
        args: &BTreeMap<String, String>,
        before: Option<Vec<Layer>>,
    ) -> Result<bool> {
        if let Some(previous) = before {
            self.state.scene.visual.previous = previous;
        }
        match crate::animation::command(&mut self.state.scene, name, args)? {
            crate::animation::Outcome::Unhandled => Err(format!(
                "unsupported command [{name}] — execution stopped, not skipped"
            )),
            crate::animation::Outcome::Continue => Ok(false),
            crate::animation::Outcome::Wait(ms) => {
                if ms > 86400000 {
                    return Err("animation wait exceeds limit".into());
                }
                self.state.flow.wait_ms = ms;
                self.state.flow.wait_skippable = flag(args, "canskip", true)?;
                Ok(ms > 0)
            }
        }
    }
    fn capture_visual(&mut self) -> Result<()> {
        if !self.visual_batch() {
            self.visual("stoptrans", &BTreeMap::new(), None)?;
            self.visual("ts", &BTreeMap::new(), None)?;
        }
        Ok(())
    }
    fn visual_batch(&self) -> bool {
        tjs::truth(
            self.state
                .flow
                .variables
                .get("tf.trans_check")
                .unwrap_or(&Value::Null),
        )
    }
    fn finish_visual(&mut self, args: &BTreeMap<String, String>, speed: &str) -> Result<bool> {
        if self.visual_batch() {
            return Ok(false);
        }
        let mut normalized: BTreeMap<_, _> = args
            .iter()
            .filter(|(key, _)| {
                ["time", "method", "vague", "wait", "zoom1", "zoom2"].contains(&key.as_str())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        normalized
            .entry("time".into())
            .or_insert_with(|| self.transition_speeds.get(speed).unwrap_or(&0).to_string());
        if normalized.get("method").is_some_and(|s| s == "universal") {
            let rule = args.get("rule").map(String::as_str).unwrap_or("tg_du");
            normalized.insert(
                "rule".into(),
                self.assets
                    .find(rule, &["webp", "png"])
                    .map_err(|e| e.to_string())?,
            );
        }
        let waiting = self.visual("te", &normalized, None)?;
        self.state.flow.wait_skippable = flag(args, "canskip", true)?;
        Ok(waiting)
    }
    fn image_style(&mut self, args: &BTreeMap<String, String>, group: &str) -> Result<()> {
        let mut normalized: BTreeMap<_, _> = args
            .iter()
            .filter(|(k, _)| {
                [
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
                ]
                .contains(&k.as_str())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        normalized.insert("layer".into(), group.into());
        self.visual("visual_style", &normalized, None)?;
        Ok(())
    }
    fn stage_image(
        &mut self,
        name: &str,
        group: &str,
        args: &BTreeMap<String, String>,
    ) -> Result<()> {
        let group: u32 = if group == "base" {
            0
        } else {
            group.parse().map_err(|_| "invalid image layer")?
        };
        let asset = self
            .assets
            .find(name, &["webp", "png"])
            .map_err(|e| e.to_string())?;
        let (width, height) = self.size(&asset)?;
        self.state.scene.layers.retain(|l| l.group != group);
        let number = |key: &str, default: f32| -> Result<f32> {
            args.get(key).map_or(Ok(default), |v| {
                v.parse::<f32>().map_err(|_| format!("invalid image {key}"))
            })
        };
        let w = number("clipwidth", width as f32)?;
        let h = number("clipheight", height as f32)?;
        self.state.scene.layers.push(Layer {
            asset: asset.clone(),
            x: number("left", 0.0)?,
            y: number("top", 0.0)?,
            width: w,
            height: h,
            opacity: 1.0,
            group,
        });
        self.state.scene.layers.sort_by_key(|l| l.group);
        self.image_style(args, &group.to_string())?;
        let sidecar = format!(
            "{}.asd",
            asset.rsplit_once('.').ok_or("image extension missing")?.0
        );
        if let Ok(asd) = self.assets.find(&sidecar, &["asd"]) {
            if width > 8192 && !args.contains_key("clipwidth") {
                return Err("wide animated image requires verified clip geometry".into());
            }
            let clip = [
                number("clipleft", 0.0)? as u32,
                number("cliptop", 0.0)? as u32,
                w as u32,
                h as u32,
            ];
            crate::animation::AtlasAnimation::parse(
                &self.assets.text(&asd).map_err(|e| e.to_string())?,
                clip,
            )?
            .validate_dimensions(width, height)?;
            let mut atlas = BTreeMap::from([("asset".into(), asset)]);
            for (key, value) in ["clipleft", "cliptop", "clipwidth", "clipheight"]
                .into_iter()
                .zip(clip)
            {
                atlas.insert(key.into(), value.to_string());
            }
            self.visual("atlas_start", &atlas, None)?;
        }
        Ok(())
    }
    fn native_visual(
        &mut self,
        name: &str,
        args: &BTreeMap<String, String>,
        before: Vec<Layer>,
        required: bool,
    ) -> Result<bool> {
        // Older callers already staged their target. Recreate a real capture
        // from their before geometry, retaining the active target separately.
        let target = self.state.scene.layers.clone();
        self.state.scene.layers = before;
        self.capture_visual()?;
        self.state.scene.layers = target;
        let _ = (name, required);
        self.finish_visual(args, "standtrans")
    }
    fn load_credits(&mut self, storage: &str) -> Result<()> {
        let storage = self
            .assets
            .find(storage, &["ks"])
            .map_err(|e| e.to_string())?;
        let tokens = kag::parse(&self.assets.text(&storage).map_err(|e| e.to_string())?)?;
        self.state
            .flow
            .visited
            .insert(storage.replace('\\', "/").to_lowercase());
        let mut credits = self.state.scene.credits.take().unwrap_or_default();
        credits.text.clear();
        credits.images.clear();
        credits.commands.clear();
        let mut active = true;
        let mut conditional = false;
        for token in tokens {
            match token.kind {
                Kind::Text(text) if active => {
                    credits.commands.push(VisualCommand {
                        name: "text".into(),
                        args: BTreeMap::from([("text".into(), text.clone())]),
                    });
                    credits.text.push_str(&text);
                }
                Kind::Label(_) => (),
                Kind::Tag { name, args } => match name.to_lowercase().as_str() {
                    "if" if args.get("exp").is_some_and(|s| s == "bSimpChineseEnabled")
                        && !conditional =>
                    {
                        conditional = true;
                        active = true;
                    }
                    "else" if conditional => active = !active,
                    "endif" if conditional => {
                        conditional = false;
                        active = true;
                    }
                    "r" if active => {
                        credits.text.push('\n');
                        credits.commands.push(VisualCommand { name, args });
                    }
                    "graph" if active => {
                        let asset = self
                            .assets
                            .find(
                                args.get("storage").ok_or("credits image missing")?,
                                &["webp", "png"],
                            )
                            .map_err(|e| e.to_string())?;
                        let mut args = args;
                        let (width, height) = self.size(&asset)?;
                        args.insert("width".into(), width.to_string());
                        args.insert("height".into(), height.to_string());
                        args.insert("asset".into(), asset.clone());
                        credits.images.push(asset);
                        credits.commands.push(VisualCommand { name, args });
                    }
                    "font" | "style" | "resetstyle" if active => {
                        credits.commands.push(VisualCommand { name, args })
                    }
                    "font" | "style" | "nowait" | "endnowait" | "resetstyle" | "return" | "r"
                    | "graph" => (),
                    _ => {
                        return Err(format!(
                            "{storage}:{}: unsupported credits command [{name}]",
                            token.line
                        ))
                    }
                },
                Kind::Text(_) => (),
                Kind::Script(_) => {
                    return Err(format!(
                        "{storage}:{}: unsupported credits script",
                        token.line
                    ))
                }
            }
        }
        if conditional {
            return Err("unterminated credits condition".into());
        }
        self.state.scene.credits = Some(credits);
        Ok(())
    }
}

fn flag(args: &BTreeMap<String, String>, key: &str, default: bool) -> Result<bool> {
    match args.get(key).map(String::as_str).unwrap_or("") {
        "" => Ok(default),
        "true" => Ok(true),
        "false" => Ok(false),
        value => value
            .parse::<i64>()
            .map(|n| n != 0)
            .map_err(|_| format!("invalid {key} flag")),
    }
}
fn duration(args: &BTreeMap<String, String>, key: &str, default: u64) -> Result<u64> {
    args.get(key)
        .map(|s| {
            s.parse::<u64>()
                .map_err(|_| format!("invalid {key} duration"))
        })
        .unwrap_or(Ok(default))
        .and_then(|n| {
            if n <= 86400000 {
                Ok(n)
            } else {
                Err("duration limit".into())
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_sprite_tables_are_evaluated_without_tjs() {
        assert_eq!(
            static_value("[[0 << 1, -25 << 1], [0, 4]]").unwrap(),
            serde_json::json!([[0, -50], [0, 4]])
        );
        assert!(static_value("System.exit()").is_err());
    }
}
