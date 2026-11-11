use crate::kag::{self, Kind, Token};
use crate::model::*;
use crate::xp3::{Assets, Store};
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
        let mut engine = Self {
            assets,
            state: State::default(),
            programs: BTreeMap::new(),
            tables,
            dimensions: BTreeMap::new(),
            names,
            checkpoints: VecDeque::new(),
        };
        engine.title()?;
        Ok(engine)
    }
    pub fn title(&mut self) -> Result<()> {
        self.checkpoints.clear();
        self.state = State::default();
        self.background("TI_base_0")?;
        Ok(())
    }
    pub fn start(&mut self) -> Result<()> {
        self.checkpoints.clear();
        let database = self.assets.text("DB_EV.ks").map_err(|e| e.to_string())?;
        let entries = database
            .split("lf.EV_EventListInfo =")
            .nth(1)
            .ok_or("event database not found")?;
        let name = entries.split('"').nth(1).ok_or("first event missing")?;
        let script = self
            .assets
            .find(&format!("{name}.ks"), &[])
            .map_err(|e| e.to_string())?;
        self.state = State {
            title: false,
            ..State::default()
        };
        self.state.cursor.script = script;
        self.state.warnings.push("Opening preview: motion, transitions and eyecatches use static cuts; full TJS and route choices are not yet supported.".into());
        self.advance()
    }
    fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.state.warnings.contains(&message) && self.state.warnings.len() < 64 {
            self.state.warnings.push(message);
        }
    }
    fn program(&mut self, name: &str) -> Result<&Vec<Token>> {
        if !self.programs.contains_key(name) {
            let source = self.assets.text(name).map_err(|e| e.to_string())?;
            self.programs.insert(name.to_owned(), kag::parse(&source)?);
        }
        Ok(&self.programs[name])
    }
    pub fn validate_state(&mut self, state: &State) -> Result<()> {
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
        for cursor in std::iter::once(&state.cursor).chain(state.stack.iter()) {
            if cursor.script.is_empty() && state.title {
                continue;
            }
            if cursor.token > self.program(&cursor.script)?.len() {
                return Err("save program position out of bounds".into());
            }
        }
        for layer in &state.scene.layers {
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
        for audio in [&state.scene.bgm, &state.scene.voice, &state.scene.se] {
            if !audio.asset.is_empty() {
                self.assets
                    .find(&audio.asset, &["opus", "ogg"])
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    pub fn previous_pages(&self) -> impl Iterator<Item = &State> {
        self.checkpoints.iter()
    }
    pub fn rewind(&mut self, page: u64) -> Result<()> {
        let index = self
            .checkpoints
            .iter()
            .position(|state| state.pages == page)
            .ok_or("page is outside the rollback window")?;
        let state = self.checkpoints[index].clone();
        self.checkpoints.truncate(index);
        self.state = state;
        Ok(())
    }
    pub fn restore(&mut self, state: State) -> Result<()> {
        self.validate_state(&state)?;
        let mut replay = Self::open(self.assets.clone())?;
        replay.dimensions.clone_from(&self.dimensions);
        replay.programs.clone_from(&self.programs);
        replay.start()?;
        for _ in 0..10000 {
            if replay.state.cursor == state.cursor
                && replay.state.stack == state.stack
                && replay.state.interlude.is_some() == state.interlude.is_some()
                && replay.state.finished == state.finished
            {
                // Rebuild derived visuals and rollback checkpoints, including saves from before eyecatches were supported.
                *self = replay;
                return Ok(());
            }
            if replay.state.finished {
                break;
            }
            replay.advance()?;
        }
        Err("saved story position is not reachable in this preview".into())
    }
    pub fn advance(&mut self) -> Result<()> {
        if self.state.title || self.state.finished {
            return Ok(());
        }
        if let Some(error) = &self.state.blocked {
            return Err(error.clone());
        }
        if self.state.pages > 0 {
            self.checkpoints.push_back(self.state.clone());
            if self.checkpoints.len() > 100 {
                self.checkpoints.pop_front();
            }
        }
        if let Some(interlude) = self.state.interlude.take() {
            self.clear_sprites();
            self.background(&interlude.background)?;
            self.audio(&interlude.bgm, "bgm", true)?;
            self.state.scene.message_visible = true;
        }
        self.state.scene.text.clear();
        self.state.scene.speaker.clear();
        self.state.scene.voice.asset.clear();
        self.state.scene.voice.sequence += 1;
        for _ in 0..10000 {
            let script = self.state.cursor.script.clone();
            let index = self.state.cursor.token;
            let token = self.program(&script)?.get(index).cloned();
            let Some(token) = token else {
                self.state.finished = true;
                return Ok(());
            };
            self.state.cursor.token += 1;
            let result = match &token.kind {
                Kind::Text(text) => {
                    self.state.scene.text.push_str(text);
                    Ok(false)
                }
                Kind::Label(_) => Ok(false),
                Kind::Tag { name, args } => self.command(name, args),
            };
            match result {
                Ok(true) => {
                    self.state.pages += 1;
                    if !self.state.scene.text.is_empty() {
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
        let message = "script instruction budget exceeded".to_owned();
        self.state.blocked = Some(message.clone());
        Err(message)
    }
    fn background(&mut self, name: &str) -> Result<()> {
        let asset = self
            .assets
            .find(name, &["webp", "png"])
            .map_err(|e| e.to_string())?;
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
        let asset = if name.is_empty() || name == "stop" {
            String::new()
        } else {
            self.assets
                .find(name, &["opus", "ogg"])
                .map_err(|e| e.to_string())?
        };
        let audio = match channel {
            "bgm" => &mut self.state.scene.bgm,
            "voice" => &mut self.state.scene.voice,
            _ => &mut self.state.scene.se,
        };
        audio.asset = asset;
        audio.sequence += 1;
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
    fn jump(&mut self, args: &BTreeMap<String, String>, call: bool) -> Result<()> {
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
        if call {
            if self.state.stack.len() >= 64 {
                return Err("call stack limit".into());
            }
            self.state.stack.push(self.state.cursor.clone());
        }
        self.state.cursor = Cursor {
            script,
            token: position,
        };
        Ok(())
    }
    fn clear_sprites(&mut self) {
        self.state.scene.layers.retain(|l| l.group == 0);
        self.state.sprites.clear();
    }
    fn command(&mut self, name: &str, args: &BTreeMap<String, String>) -> Result<bool> {
        if args.contains_key("cond") {
            return Err(format!("conditional attribute needs TJS support: {name}"));
        }
        let arg = |key: &str| args.get(key).map(String::as_str).unwrap_or("");
        match name.to_lowercase().as_str() {
            "c" | "p" | "l" => return Ok(true),
            ">>" | "<<" | "m_bljump_linejump" => (),
            "r" => self.state.scene.text.push('\n'),
            "sc_startprocess" => {
                self.clear_sprites();
                self.state.scene.layers.clear();
                self.background(arg("bg"))?;
                self.audio(arg("bgm"), "bgm", true)?;
            }
            "stc" => self.sprite(args)?,
            "bgset" | "bgset_withoutblack" | "bgset_interval" => {
                self.clear_sprites();
                self.background(if arg("bg").is_empty() {
                    arg("bg2")
                } else {
                    arg("bg")
                })?;
                if !arg("bgm").is_empty() {
                    self.audio(arg("bgm"), "bgm", true)?;
                }
            }
            "bg_image" => self.background(arg("num"))?,
            "hh" | "free_lay_reset" => self.clear_sprites(),
            "bgm_fp" | "bgm_p" | "bgm_fx" => {
                self.audio(arg("st"), "bgm", arg("loop") != "false")?
            }
            "bgm_fe" | "bgm_stop" | "stopbgm" => self.audio("", "bgm", false)?,
            "se" => {
                if !arg("stop").is_empty() {
                    self.audio("", "se", false)?;
                } else if !arg("play").is_empty() {
                    self.audio(arg("play"), "se", arg("loop") == "true")?;
                }
            }
            "voiceallstop" | "stopvoice" => self.audio("", "voice", false)?,
            "ms_hide" => {
                if !args.contains_key("standhide") {
                    self.clear_sprites();
                }
                self.state.scene.message_visible = false;
            }
            "ms_show" => self.state.scene.message_visible = true,
            "ms_c" => {
                self.warn("Alternate message styles currently use one native dialogue panel.");
            }
            "eyecatch" => {
                if arg("sp").is_empty() || arg("bg").is_empty() {
                    return Err("eyecatch requires literal sp and bg images".into());
                }
                let background = self
                    .assets
                    .find(arg("bg"), &["webp", "png"])
                    .map_err(|e| e.to_string())?;
                if !arg("bgm").is_empty() && arg("bgm") != "stop" {
                    self.assets
                        .find(arg("bgm"), &["opus", "ogg"])
                        .map_err(|e| e.to_string())?;
                }
                self.clear_sprites();
                self.background(arg("sp"))?;
                self.audio("", "bgm", false)?;
                self.audio("", "voice", false)?;
                self.audio("", "se", false)?;
                self.state.scene.message_visible = false;
                self.state.interlude = Some(Interlude {
                    background,
                    bgm: arg("bgm").into(),
                });
                return Ok(true);
            }
            "ts" | "te" | "t_move" | "quake" | "wait" => {
                self.warn(format!("Visual/timing approximation: {name}"));
            }
            "achievement" => self.warn("Steam achievements are not integrated."),
            "jump" => self.jump(args, false)?,
            "call" => self.jump(args, true)?,
            "return" => {
                self.state.cursor = self.state.stack.pop().ok_or("return with empty stack")?
            }
            "sc_endprocess" => {
                self.state.finished = true;
                self.warn("Opening chapter complete. Later chapters and route choices are not enabled in this preview.");
                return Ok(true);
            }
            "s" => {
                self.state.finished = true;
                return Ok(true);
            }
            _ if name
                .strip_prefix('h')
                .is_some_and(|v| v.parse::<u32>().is_ok()) =>
            {
                let group = name[1..].parse::<u32>().unwrap();
                self.state.scene.layers.retain(|l| l.group != group);
                self.state.sprites.remove(&group);
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
            _ => {
                return Err(format!(
                    "unsupported command [{name}] — execution stopped, not skipped"
                ))
            }
        }
        Ok(false)
    }
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
