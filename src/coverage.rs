//! Static expectations and deterministic headless traversal; no dialogue in reports.
use crate::kag::{self, Kind};
use crate::model::{GalleryEntry, State};
use crate::story::Engine;
use crate::xp3::Store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Write};
use std::sync::Arc;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// Total driver operations, including restore and menu queries. Engine restore opens a fresh
    /// instance to isolate progress from alternative branches.
    pub max_steps: u64,
    pub max_states: usize,
    pub max_frontier: usize,
    pub max_snapshot_bytes: usize,
    pub max_frontier_bytes: usize,
    pub max_decisions: usize,
    pub tick_ms: u64,
    /// Exercise each observed alternative once, then continue canonical inputs.
    /// This is a target audit, explicitly not exhaustive path coverage.
    pub target_alternatives: bool,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
            max_states: 250_000,
            max_frontier: 2048,
            max_snapshot_bytes: 8 * 1024 * 1024,
            max_frontier_bytes: 128 * 1024 * 1024,
            max_decisions: 256,
            tick_ms: 1000,
            target_alternatives: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Site {
    pub script: String,
    pub token: usize,
    pub line: Option<usize>,
    pub tag: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Advance,
    Choose { id: usize },
    Tick { milliseconds: u64 },
    FinishVideo { sequence: u64 },
    NewGame,
    Replay { id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Decision {
    pub site: Site,
    pub state_sha256: String,
    pub action: Action,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Blocker {
    pub code: String,
    pub site: Site,
    pub state_sha256: String,
    /// Arbitrary engine errors can contain dialogue/expressions. Retain only their digest.
    pub error_sha256: Option<String>,
    pub operation: Option<Action>,
    pub origin: String,
    pub decisions: Vec<Decision>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Evidence {
    pub site: Site,
    pub state_sha256: String,
    pub origin: String,
    pub decisions: Vec<Decision>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Gap {
    pub item: String,
    pub reason: String,
    /// First observed blocker for the run; not an assertion of graph causality.
    pub first_observed_blocker: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Event {
    pub id: usize,
    pub script: String,
    pub route: usize,
    pub previous: Option<String>,
    pub next: Option<String>,
    pub terminal: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Edge {
    pub kind: String,
    pub from: String,
    pub to: String,
    pub selection: Option<usize>,
    pub choice: Option<usize>,
    pub site: Option<Site>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Selection {
    pub id: usize,
    pub script: String,
    pub choices: usize,
    pub sites: Vec<Site>,
    pub previous_rule: String,
    pub next_rules: Vec<String>,
    pub branch_sites: Vec<Site>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Route {
    pub id: usize,
    pub previous: Option<String>,
    pub continues: bool,
}

#[derive(Clone, Debug)]
struct Point {
    line: usize,
    tag: Option<String>,
    label: Option<String>,
    selection: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Audit {
    pub effective_scripts: usize,
    pub main_scripts: BTreeSet<String>,
    pub script_sha256: BTreeMap<String, String>,
    pub commands: BTreeMap<String, u64>,
    pub command_sites: BTreeMap<String, Vec<Site>>,
    pub events: Vec<Event>,
    pub selections: Vec<Selection>,
    pub routes: Vec<Route>,
    pub edges: Vec<Edge>,
    /// Terminal event script paths, independent of UI ending labels.
    pub expected_endings: BTreeSet<String>,
    pub replay_scripts: BTreeSet<String>,
    pub issues: Vec<String>,
    #[serde(skip)]
    programs: BTreeMap<String, Vec<Point>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Report {
    pub scripts: BTreeSet<String>,
    pub endings: BTreeSet<String>,
    pub choices: usize,
    pub pages: u64,
    pub failures: Vec<String>,
    pub unreachable: BTreeSet<String>,
    pub schema_version: u32,
    pub fingerprint: String,
    pub limits: Limits,
    pub audit: Audit,
    pub complete: bool,
    pub targets_complete: bool,
    pub exhausted: bool,
    pub budget_exhausted: bool,
    pub operations: u64,
    pub unique_states: usize,
    pub duplicate_states: u64,
    pub restored_states: u64,
    pub timers_completed: u64,
    pub videos_completed: u64,
    pub restarts: u64,
    pub replay_scripts: BTreeSet<String>,
    pub replay_endings: BTreeSet<String>,
    pub unlocked_replays: BTreeSet<String>,
    pub started_replays: BTreeSet<String>,
    pub completed_replays: BTreeSet<String>,
    pub locked_replays: BTreeSet<String>,
    pub unlocked_gallery: BTreeSet<String>,
    pub direct_probes: BTreeSet<String>,
    pub reached_terminal_scripts: BTreeSet<String>,
    pub unreachable_endings: BTreeSet<String>,
    pub unreachable_replays: BTreeSet<String>,
    pub untraversed_choices: BTreeSet<String>,
    pub blockers: Vec<Blocker>,
    pub first_blocker: Option<Blocker>,
    pub evidence: BTreeMap<String, Evidence>,
    pub gaps: Vec<Gap>,
    pub limitations: Vec<String>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn canonical(name: &str) -> String {
    name.replace('\\', "/").to_lowercase()
}
fn script_id(name: &str, mains: &BTreeSet<String>) -> Option<String> {
    let name = canonical(name);
    let name = if name.ends_with(".ks") {
        name
    } else {
        format!("{name}.ks")
    };
    if mains.contains(&name) {
        return Some(name);
    }
    let mut matches = mains
        .iter()
        .filter(|n| n.rsplit('/').next() == name.rsplit('/').next());
    let result = matches.next()?.clone();
    matches.next().is_none().then_some(result)
}

// Non-evaluating TJS data reader: no functions, assignments or flags execute.
#[derive(Debug)]
enum Data {
    Array(Vec<Data>),
    String(String),
    Number(i64),
    Symbol,
}
impl Data {
    fn array(&self) -> Option<&[Data]> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }
    fn string(&self) -> Option<&str> {
        if let Self::String(v) = self {
            Some(v)
        } else {
            None
        }
    }
    fn number(&self) -> Option<i64> {
        if let Self::Number(v) = self {
            Some(*v)
        } else {
            None
        }
    }
}
struct DataReader<'a> {
    source: &'a [u8],
    pos: usize,
    items: usize,
}
impl DataReader<'_> {
    fn space(&mut self) -> Result<()> {
        loop {
            while self
                .source
                .get(self.pos)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.pos += 1;
            }
            if self.source.get(self.pos..self.pos + 2) == Some(b"//") {
                while self.source.get(self.pos).is_some_and(|c| *c != b'\n') {
                    self.pos += 1;
                }
            } else if self.source.get(self.pos..self.pos + 2) == Some(b"/*") {
                self.pos += 2;
                while self.source.get(self.pos..self.pos + 2) != Some(b"*/") {
                    if self.pos >= self.source.len() {
                        return Err("unterminated_data_comment".into());
                    }
                    self.pos += 1;
                }
                self.pos += 2;
            } else {
                return Ok(());
            }
        }
    }
    fn value(&mut self, depth: usize) -> Result<Data> {
        self.space()?;
        self.items += 1;
        if depth > 32 || self.items > 100_000 {
            return Err("data_budget".into());
        }
        match self
            .source
            .get(self.pos)
            .copied()
            .ok_or("unfinished_data")?
        {
            b'[' => {
                self.pos += 1;
                let mut values = Vec::new();
                loop {
                    self.space()?;
                    if self.source.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        return Ok(Data::Array(values));
                    }
                    values.push(self.value(depth + 1)?);
                    self.space()?;
                    match self.source.get(self.pos) {
                        Some(b',') => self.pos += 1,
                        Some(b']') => (),
                        _ => return Err("bad_data_separator".into()),
                    }
                }
            }
            quote @ (b'\'' | b'"') => {
                self.pos += 1;
                let mut out = Vec::new();
                loop {
                    let c = *self.source.get(self.pos).ok_or("unfinished_data_string")?;
                    self.pos += 1;
                    if c == quote {
                        break;
                    }
                    if c == b'\\' {
                        let c = *self.source.get(self.pos).ok_or("unfinished_data_escape")?;
                        self.pos += 1;
                        out.push(match c {
                            b'n' => b'\n',
                            b'r' => b'\r',
                            b't' => b'\t',
                            _ => c,
                        });
                    } else {
                        out.push(c);
                    }
                }
                Ok(Data::String(
                    String::from_utf8(out).map_err(|_| "invalid_data_utf8")?,
                ))
            }
            _ => {
                let start = self.pos;
                while self
                    .source
                    .get(self.pos)
                    .is_some_and(|c| !c.is_ascii_whitespace() && !b",]".contains(c))
                {
                    self.pos += 1;
                }
                if start == self.pos {
                    return Err("bad_data_atom".into());
                }
                let atom = std::str::from_utf8(&self.source[start..self.pos])
                    .map_err(|_| "invalid_data_atom")?;
                if let Ok(n) = atom.parse() {
                    Ok(Data::Number(n))
                } else if atom == "tf.EX_Flag" {
                    Ok(Data::Symbol)
                } else {
                    Err("nonliteral_database_value".into())
                }
            }
        }
    }
}

fn table(source: &str, key: &str) -> Result<Data> {
    let bytes = source.as_bytes();
    let mut reader = DataReader {
        source: bytes,
        pos: 0,
        items: 0,
    };
    while reader.pos < bytes.len() {
        reader.space()?;
        let p = reader.pos;
        if bytes.get(p..p + key.len()) == Some(key.as_bytes())
            && (p == 0 || !bytes[p - 1].is_ascii_alphanumeric() && bytes[p - 1] != b'_')
        {
            reader.pos += key.len();
            reader.space()?;
            if bytes.get(reader.pos) == Some(&b'=') && bytes.get(reader.pos + 1) != Some(&b'=') {
                reader.pos += 1;
                return reader.value(0);
            }
        }
        reader.pos = p;
        if matches!(bytes.get(p), Some(b'\'' | b'"')) {
            reader.value(0)?;
        } else {
            reader.pos += 1;
        }
    }
    Err("database_assignment_missing".into())
}

/// Supply only winning overlay sources. This does not execute scripts or report attribute values.
pub fn audit_sources(sources: &BTreeMap<String, String>) -> Audit {
    let mut audit = Audit {
        effective_scripts: sources.len(),
        ..Audit::default()
    };
    if sources.len() > 4096
        || sources.values().map(|s| s.len() as u64).sum::<u64>() > 64 * 1024 * 1024
    {
        audit.issues.push("source_audit_budget".into());
        return audit;
    }
    let sources: BTreeMap<_, _> = sources.iter().map(|(n, s)| (canonical(n), s)).collect();
    if sources.len() != audit.effective_scripts {
        audit.issues.push("canonical_source_collision".into());
    }
    for (name, source) in &sources {
        audit
            .script_sha256
            .insert(name.clone(), digest(source.as_bytes()));
        if name.starts_with("scenario/main/") && name.ends_with(".ks") {
            audit.main_scripts.insert(name.clone());
        }
    }
    for name in &audit.main_scripts {
        match kag::parse(sources[name]) {
            Ok(tokens) => {
                for (index, token) in tokens.iter().enumerate() {
                    if let Kind::Tag { name: tag, .. } = &token.kind {
                        let tag = tag.to_lowercase();
                        *audit.commands.entry(tag.clone()).or_default() += 1;
                        let sites = audit.command_sites.entry(tag.clone()).or_default();
                        // All control sites; one sample for other tags bounds report size.
                        if sites.is_empty()
                            || tag.starts_with("sl_")
                            || tag.starts_with("sc_")
                            || matches!(tag.as_str(), "eval" | "if" | "call" | "jump")
                        {
                            sites.push(Site {
                                script: name.clone(),
                                token: index,
                                line: Some(token.line),
                                tag: Some(tag),
                            });
                        }
                    }
                }
                let points = tokens
                    .into_iter()
                    .map(|t| {
                        let (tag, label, selection) = match t.kind {
                            Kind::Tag { name, args } => {
                                let selection = if name.eq_ignore_ascii_case("SL_StartSelect") {
                                    args.get("num").and_then(|v| v.parse().ok())
                                } else {
                                    None
                                };
                                (Some(name.to_lowercase()), None, selection)
                            }
                            Kind::Label(label) => (None, Some(label), None),
                            Kind::Script(_) => (Some("iscript".into()), None, None),
                            Kind::Text(_) => (None, None, None),
                        };
                        Point {
                            line: t.line,
                            tag,
                            label,
                            selection,
                        }
                    })
                    .collect();
                audit.programs.insert(name.clone(), points);
            }
            Err(error) => audit
                .issues
                .push(format!("parse_failed:{name}:{}", digest(error.as_bytes()))),
        }
    }
    let find = |base: &str| -> Result<&str> {
        let values: Vec<_> = sources
            .iter()
            .filter(|(n, _)| n.rsplit('/').next() == Some(base))
            .map(|(_, s)| s.as_str())
            .collect();
        if values.is_empty() {
            Err(format!("missing_database:{base}"))
        } else if values.iter().any(|s| s != &values[0]) {
            Err(format!("ambiguous_database:{base}"))
        } else {
            Ok(values[0])
        }
    };
    let result = (|| -> Result<()> {
        let source = find("db_ev.ks")?;
        let data = table(source, "lf.EV_EventListInfo")?;
        for (id, row) in data
            .array()
            .ok_or("event_database_not_array")?
            .iter()
            .enumerate()
        {
            let row = row
                .array()
                .filter(|r| r.len() >= 5)
                .ok_or("event_row_shape")?;
            let script = script_id(
                row[0].string().ok_or("event_script_type")?,
                &audit.main_scripts,
            )
            .ok_or("event_script_missing")?;
            let route = row[1]
                .number()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("event_route_type")?;
            let terminal = row[3].string() == Some("End");
            let previous = match (row[2].string(), row[2].number()) {
                (Some("End"), _) => Some("End".into()),
                (Some(name), _) => {
                    Some(script_id(name, &audit.main_scripts).ok_or("event_previous_missing")?)
                }
                (_, Some(-1)) => None,
                _ => return Err("event_previous_semantics_unrecognized".into()),
            };
            if !terminal && row[3].number() != Some(-1) {
                return Err("event_next_semantics_unrecognized".into());
            }
            audit.events.push(Event {
                id,
                script: script.clone(),
                route,
                previous,
                next: None,
                terminal,
            });
            if terminal {
                audit.expected_endings.insert(script);
            }
        }
        for i in 0..audit.events.len() {
            if !audit.events[i].terminal {
                let next = audit
                    .events
                    .get(i + 1)
                    .ok_or("event_falls_off_database")?
                    .script
                    .clone();
                audit.events[i].next = Some(next.clone());
                audit.edges.push(Edge {
                    kind: "event_default_next".into(),
                    from: audit.events[i].script.clone(),
                    to: next,
                    ..Edge::default()
                });
            }
        }
        let data = table(source, "lf.EV_SelectListInfo")?;
        let selection_rows = data.array().ok_or("selection_database_not_array")?;
        for (id, row) in selection_rows.iter().enumerate() {
            let row = row
                .array()
                .filter(|r| r.len() >= 5)
                .ok_or("selection_row_shape")?;
            let script = script_id(
                row[0].string().ok_or("selection_script_type")?,
                &audit.main_scripts,
            )
            .ok_or("selection_script_missing")?;
            let targets = row[1].array().ok_or("selection_target_type")?;
            let next = row[3].array().ok_or("selection_next_type")?;
            let choices = row[4].array().ok_or("selection_choices_type")?.len();
            if targets.len() < choices || next.len() < choices || choices == 0 {
                return Err("selection_choice_shape".into());
            }
            let sites: Vec<_> = audit
                .programs
                .get(&script)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(token, t)| {
                    if t.tag.as_deref() == Some("sl_startselect") && t.selection == Some(id) {
                        return Some(Site {
                            script: script.clone(),
                            token,
                            line: Some(t.line),
                            tag: t.tag.clone(),
                        });
                    }
                    None
                })
                .collect();
            if sites.len() != 1 {
                audit
                    .issues
                    .push(format!("selection_site_count:{id}:{}", sites.len()));
            }
            let previous_rule = match (row[2].number(), row[2].string()) {
                (Some(-1), _) => "current".into(),
                (Some(n), _) if n >= 0 && (n as usize) < selection_rows.len() => {
                    format!("selection:{n}")
                }
                (_, Some("E")) => "end".into(),
                _ => return Err("selection_previous_semantics_unrecognized".into()),
            };
            let mut next_rules = Vec::new();
            let mut branch_sites = Vec::new();
            for choice in 0..choices {
                let label = format!("SLP_{id}_{choice}");
                let branch = audit
                    .programs
                    .get(&script)
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .find(|(_, t)| t.label.as_deref() == Some(label.as_str()));
                if let Some((token, t)) = branch {
                    let branch_site = Site {
                        script: script.clone(),
                        token,
                        line: Some(t.line),
                        tag: None,
                    };
                    branch_sites.push(branch_site.clone());
                    audit.edges.push(Edge {
                        kind: "selection_branch_label".into(),
                        from: format!("{script}#SL_{id}"),
                        to: format!("{script}#{label}"),
                        selection: Some(id),
                        choice: Some(choice),
                        site: Some(branch_site),
                    });
                } else {
                    audit
                        .issues
                        .push(format!("selection_branch_label_missing:{id}:{choice}"));
                }
                let target = targets[choice]
                    .string()
                    .ok_or("selection_target_not_string")?;
                let next = next[choice].string().ok_or("selection_next_not_string")?;
                if !next.is_empty() && next != "E" {
                    return Err("selection_next_semantics_unrecognized".into());
                }
                let to = if target.is_empty() {
                    format!("{script}#selection:{id}:choice:{choice}")
                } else {
                    script_id(target, &audit.main_scripts).ok_or("selection_destination_missing")?
                };
                audit.edges.push(Edge {
                    kind: if target.is_empty() {
                        "selection_local"
                    } else {
                        "selection_event_jump"
                    }
                    .into(),
                    from: script.clone(),
                    to,
                    selection: Some(id),
                    choice: Some(choice),
                    site: sites.first().cloned(),
                });
                let next_rule = if next == "E" {
                    "end".into()
                } else if id + 1 < selection_rows.len() {
                    format!("selection:{}", id + 1)
                } else {
                    "end".into()
                };
                audit.edges.push(Edge {
                    kind: "selection_next_navigation".into(),
                    from: format!("{script}#selection:{id}:choice:{choice}"),
                    to: next_rule.clone(),
                    selection: Some(id),
                    choice: Some(choice),
                    site: sites.first().cloned(),
                });
                next_rules.push(next_rule);
            }
            audit.selections.push(Selection {
                id,
                script,
                choices,
                sites,
                previous_rule,
                next_rules,
                branch_sites,
            });
        }
        let data = table(source, "lf.EV_RouteInfo")?;
        for (id, row) in data
            .array()
            .ok_or("route_database_not_array")?
            .iter()
            .enumerate()
        {
            let row = row
                .array()
                .filter(|r| r.len() >= 3)
                .ok_or("route_row_shape")?;
            // Column 1 is the predecessor of this route's first event, NOT its ending.
            let previous = match row[1].string().ok_or("route_previous_type")? {
                "End" => None,
                value => {
                    Some(script_id(value, &audit.main_scripts).ok_or("route_previous_missing")?)
                }
            };
            let continues = match row[2].string() {
                Some("Next") => true,
                Some("End") => false,
                _ => return Err("route_next_semantics_unrecognized".into()),
            };
            let last = audit
                .events
                .iter()
                .rfind(|e| e.route == id)
                .ok_or("route_has_no_events")?;
            if last.terminal == continues {
                audit.issues.push(format!("route_terminal_mismatch:{id}"));
            }
            audit.routes.push(Route {
                id,
                previous,
                continues,
            });
        }
        for i in 0..audit.events.len() {
            let previous = match audit.events[i].previous.as_deref() {
                Some("End") => None,
                Some(name) => {
                    Some(script_id(name, &audit.main_scripts).ok_or("event_previous_missing")?)
                }
                None => i.checked_sub(1).map(|j| audit.events[j].script.clone()),
            };
            audit.events[i].previous = previous.clone();
            if let Some(to) = previous {
                audit.edges.push(Edge {
                    kind: "event_previous_navigation".into(),
                    from: audit.events[i].script.clone(),
                    to,
                    ..Edge::default()
                });
            }
        }
        for (script, tokens) in &audit.programs {
            let mut current_branch = None;
            for (token, t) in tokens.iter().enumerate() {
                if let Some(label) = &t.label {
                    current_branch = label
                        .strip_prefix("SLP_")
                        .and_then(|tail| tail.split_once('_'))
                        .and_then(|(id, c)| {
                            Some((id.parse::<usize>().ok()?, c.parse::<usize>().ok()?))
                        });
                } else if t.tag.as_deref() == Some("sl_join") {
                    if let Some((id, choice)) = current_branch {
                        let target = format!("ENDSLP_{id}");
                        if !tokens
                            .iter()
                            .any(|t| t.label.as_deref() == Some(target.as_str()))
                        {
                            audit
                                .issues
                                .push(format!("selection_join_label_missing:{script}:{id}"));
                        }
                        audit.edges.push(Edge {
                            kind: "selection_join_label".into(),
                            from: format!("{script}#SLP_{id}_{choice}"),
                            to: format!("{script}#{target}"),
                            selection: Some(id),
                            choice: Some(choice),
                            site: Some(Site {
                                script: script.clone(),
                                token,
                                line: Some(t.line),
                                tag: t.tag.clone(),
                            }),
                        });
                    } else {
                        audit
                            .issues
                            .push(format!("selection_join_without_branch:{script}:{}", t.line));
                    }
                }
            }
        }
        let data = table(find("db_ex.ks")?, "lf.EX_SCENEInfo")?;
        for row in data.array().ok_or("replay_database_not_array")? {
            let row = row
                .array()
                .filter(|r| r.len() >= 3)
                .ok_or("replay_row_shape")?;
            let script = script_id(
                row[2].string().ok_or("replay_script_type")?,
                &audit.main_scripts,
            )
            .ok_or("replay_script_missing")?;
            audit.replay_scripts.insert(script);
        }
        Ok(())
    })();
    if let Err(error) = result {
        audit.issues.push(error);
    }
    if audit.main_scripts.is_empty() {
        audit.issues.push("no_main_scripts".into());
    }
    for script in &audit.main_scripts {
        if !audit.events.iter().any(|e| &e.script == script) {
            audit
                .issues
                .push(format!("main_not_in_event_database:{script}"));
        }
    }
    audit
}

pub fn audit(assets: &Store) -> Audit {
    let mut sources = BTreeMap::new();
    let mut issues = Vec::new();
    let mut bytes = 0usize;
    for name in assets
        .names()
        .into_iter()
        .filter(|n| n.to_lowercase().ends_with(".ks") || n.to_lowercase().ends_with(".tjs"))
    {
        match assets.text(&name) {
            Ok(text) => {
                bytes = bytes.saturating_add(text.len());
                if bytes > 64 * 1024 * 1024 || sources.len() >= 4096 {
                    issues.push("source_audit_budget".into());
                    break;
                }
                sources.insert(name, text);
            }
            Err(_) => issues.push(format!(
                "script_integrity_or_encoding_failed:{}",
                canonical(&name)
            )),
        }
    }
    let mut audit = audit_sources(&sources);
    audit.issues.extend(issues);
    audit
}

/// The full-State restore must reproduce every serialized field exactly. A fake driver can test
/// traversal independently; production always delegates to the public Engine methods below.
pub trait Driver {
    fn state(&self) -> &State;
    fn start(&mut self) -> Result<()>;
    fn restore(&mut self, state: State) -> Result<()>;
    fn advance(&mut self) -> Result<()>;
    fn choose(&mut self, id: usize) -> Result<()>;
    fn tick(&mut self, milliseconds: u64) -> Result<bool>;
    fn finish_video(&mut self, sequence: u64) -> Result<()>;
    fn gallery(&self) -> Result<Vec<GalleryEntry>>;
    fn replay_list(&self) -> Result<Vec<GalleryEntry>>;
    fn start_replay(&mut self, id: &str) -> Result<()>;
}
impl Driver for Engine {
    fn state(&self) -> &State {
        &self.state
    }
    fn start(&mut self) -> Result<()> {
        Engine::start(self)
    }
    fn restore(&mut self, state: State) -> Result<()> {
        // A native restore may merge current global progress. Start from a fresh instance so one
        // alternative branch cannot grant another branch unlocks it did not actually earn.
        let mut isolated = Engine::open(self.assets.clone())?;
        Engine::restore(&mut isolated, state)?;
        *self = isolated;
        Ok(())
    }
    fn advance(&mut self) -> Result<()> {
        Engine::advance(self)
    }
    fn choose(&mut self, id: usize) -> Result<()> {
        Engine::choose(self, id)
    }
    fn tick(&mut self, milliseconds: u64) -> Result<bool> {
        Engine::tick(self, milliseconds)
    }
    fn finish_video(&mut self, sequence: u64) -> Result<()> {
        Engine::finish_video(self, sequence)
    }
    fn gallery(&self) -> Result<Vec<GalleryEntry>> {
        Engine::gallery(self)
    }
    fn replay_list(&self) -> Result<Vec<GalleryEntry>> {
        Engine::replay_list(self)
    }
    fn start_replay(&mut self, id: &str) -> Result<()> {
        Engine::start_replay(self, id)
    }
}

struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
    overflow: bool,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.overflow = true;
            return Err(io::Error::other("snapshot_budget"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn serialized_digest(value: &impl Serialize) -> Result<String> {
    let mut hash = HashWriter(Sha256::new());
    serde_json::to_writer(&mut hash, value).map_err(|_| "state_serialization_failed")?;
    Ok(format!("{:x}", hash.0.finalize()))
}
fn snapshot(state: &State, limits: &Limits) -> Result<Vec<u8>> {
    let mut out = BoundedBytes {
        bytes: Vec::new(),
        limit: limits.max_snapshot_bytes,
        overflow: false,
    };
    if serde_json::to_writer(&mut out, state).is_err() {
        return Err(if out.overflow {
            "snapshot_budget"
        } else {
            "state_serialization_failed"
        }
        .into());
    }
    Ok(out.bytes)
}
fn site(audit: &Audit, state: &State) -> Site {
    let script = canonical(&state.cursor.script);
    let script = script_id(&script, &audit.main_scripts).unwrap_or(script);
    let token = state.cursor.token.saturating_sub(1);
    let t = audit.programs.get(&script).and_then(|p| p.get(token));
    Site {
        script,
        token,
        line: t.map(|t| t.line),
        tag: t.and_then(|t| t.tag.clone()),
    }
}
fn error_code(error: &str) -> &'static str {
    let error = error.to_lowercase();
    if error.contains("not implemented") {
        "engine_unimplemented"
    } else if error.contains("unsupported") {
        "unsupported_instruction"
    } else if error.contains("checksum") {
        "asset_checksum_failed"
    } else if error.contains("budget") {
        "engine_budget"
    } else {
        "engine_error"
    }
}
fn blocker(
    report: &mut Report,
    state: &State,
    origin: &str,
    decisions: &[Decision],
    code: &str,
    error: Option<&str>,
    operation: Option<Action>,
) {
    let mut b = Blocker {
        code: code.into(),
        site: site(&report.audit, state),
        state_sha256: serialized_digest(state).unwrap_or_default(),
        error_sha256: error.map(|s| digest(s.as_bytes())),
        operation,
        origin: origin.into(),
        decisions: decisions.to_vec(),
    };
    // Engine errors conventionally start with script:line. Retain only a validated numeric line.
    if let Some(error) = error {
        let prefix = format!("{}:", state.cursor.script);
        if let Some(rest) = error.strip_prefix(&prefix) {
            if let Some(line) = rest.split(':').next().and_then(|s| s.parse::<usize>().ok()) {
                let locate = b.site.line != Some(line) || b.site.tag.is_none();
                b.site.line = Some(line);
                if let Some(tokens) = report.audit.programs.get(&b.site.script).filter(|_| locate) {
                    if let Some((index, t)) = tokens
                        .iter()
                        .enumerate()
                        .find(|(_, t)| t.line == line && t.tag.is_some())
                    {
                        b.site.token = index;
                        b.site.tag = t.tag.clone();
                    }
                }
            }
        }
    }
    report.failures.push(format!(
        "{}:{}:{}:{}",
        b.code,
        b.site.script,
        b.site
            .line
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unknown".into()),
        b.site.tag.as_deref().unwrap_or("unknown")
    ));
    if report.first_blocker.is_none() {
        report.first_blocker = Some(b.clone());
    }
    report.blockers.push(b);
}
fn evidence(
    audit: &Audit,
    state: &State,
    origin: &str,
    decisions: &[Decision],
    sha: &str,
) -> Evidence {
    Evidence {
        site: site(audit, state),
        state_sha256: sha.into(),
        origin: origin.into(),
        decisions: decisions.to_vec(),
    }
}
fn observe(
    report: &mut Report,
    state: &State,
    origin: &str,
    decisions: &[Decision],
    sha: &str,
    inherited_visits: &BTreeSet<String>,
) {
    let mut scripts = BTreeSet::new();
    if let Some(script) = script_id(&state.cursor.script, &report.audit.main_scripts) {
        scripts.insert(script);
    }
    for script in state.flow.visited.difference(inherited_visits) {
        if let Some(script) = script_id(script, &report.audit.main_scripts) {
            scripts.insert(script);
        }
    }
    for script in scripts {
        let key = format!("{origin}:script:{script}");
        report
            .evidence
            .entry(key)
            .or_insert_with(|| evidence(&report.audit, state, origin, decisions, sha));
        if origin == "story" {
            report.scripts.insert(script);
        } else {
            report.replay_scripts.insert(script);
        }
    }
}
fn operate<D: Driver>(driver: &mut D, action: &Action) -> Result<()> {
    match action {
        Action::Advance => driver.advance(),
        Action::Choose { id } => driver.choose(*id),
        Action::Tick { milliseconds } => driver.tick(*milliseconds).map(|_| ()),
        Action::FinishVideo { sequence } => driver.finish_video(*sequence),
        Action::NewGame => driver.start(),
        Action::Replay { id } => driver.start_replay(id),
    }
}
struct Job {
    bytes: Vec<u8>,
    decisions: Vec<Decision>,
    origin: String,
    action: Action,
    inherited_visits: BTreeSet<String>,
}
struct Work {
    queue: VecDeque<Job>,
    bytes: usize,
}
/// A live state and its full serialized representation describe the same branch point.
struct SnapshotRef<'a> {
    state: &'a State,
    bytes: &'a [u8],
}
impl Work {
    fn push(
        &mut self,
        report: &mut Report,
        snapshot: SnapshotRef<'_>,
        decisions: &[Decision],
        origin: &str,
        action: Action,
        inherited_visits: &BTreeSet<String>,
    ) -> bool {
        let SnapshotRef { state, bytes } = snapshot;
        if self.queue.len() >= report.limits.max_frontier
            || self.bytes.saturating_add(bytes.len()) > report.limits.max_frontier_bytes
            || decisions.len() >= report.limits.max_decisions
        {
            report.budget_exhausted = true;
            blocker(
                report,
                state,
                origin,
                decisions,
                "frontier_or_decision_budget",
                None,
                Some(action),
            );
            return false;
        }
        let mut decisions = decisions.to_vec();
        decisions.push(Decision {
            site: site(&report.audit, state),
            state_sha256: digest(bytes),
            action: action.clone(),
        });
        self.bytes += bytes.len();
        let inherited_visits = if matches!(&action, Action::Replay { .. }) {
            state.flow.visited.clone()
        } else {
            inherited_visits.clone()
        };
        self.queue.push_back(Job {
            bytes: bytes.to_vec(),
            decisions,
            origin: origin.into(),
            action,
            inherited_visits,
        });
        true
    }
}
fn consume(
    report: &mut Report,
    state: &State,
    origin: &str,
    decisions: &[Decision],
    action: Option<Action>,
) -> bool {
    if report.operations >= report.limits.max_steps {
        report.budget_exhausted = true;
        blocker(
            report,
            state,
            origin,
            decisions,
            "operation_budget",
            None,
            action,
        );
        return false;
    }
    report.operations += 1;
    true
}
fn finish(mut report: Report) -> Report {
    report.unreachable = report
        .audit
        .main_scripts
        .difference(&report.scripts)
        .cloned()
        .collect();
    report.unreachable_endings = report
        .audit
        .expected_endings
        .difference(&report.reached_terminal_scripts)
        .cloned()
        .collect();
    report.unreachable_replays = report
        .audit
        .replay_scripts
        .difference(&report.replay_scripts)
        .cloned()
        .collect();
    for selection in &report.audit.selections {
        for id in 0..selection.choices {
            let key = format!("selection:{}:choice:{id}", selection.id);
            if !report.evidence.contains_key(&format!("story:{key}")) {
                report.untraversed_choices.insert(key);
            }
        }
    }
    let reason = if report.budget_exhausted {
        "not_proven_within_budget"
    } else if !report.blockers.is_empty() {
        "not_proven_due_to_observed_blockers"
    } else {
        "not_reached_by_legal_traversal"
    };
    for item in report
        .unreachable
        .iter()
        .map(|s| format!("script:{s}"))
        .chain(
            report
                .unreachable_endings
                .iter()
                .map(|s| format!("ending:{s}")),
        )
        .chain(
            report
                .unreachable_replays
                .iter()
                .map(|s| format!("replay:{s}")),
        )
        .chain(report.untraversed_choices.iter().cloned())
        .chain(
            report
                .unlocked_replays
                .difference(&report.completed_replays)
                .map(|id| format!("replay_entry:{id}")),
        )
    {
        report.gaps.push(Gap {
            item,
            reason: reason.into(),
            first_observed_blocker: (!report.blockers.is_empty()).then_some(0),
        });
    }
    report.complete = report.exhausted
        && !report.budget_exhausted
        && report.failures.is_empty()
        && report.unreachable.is_empty()
        && report.unreachable_endings.is_empty()
        && report.unreachable_replays.is_empty()
        && report.untraversed_choices.is_empty()
        && report.unlocked_replays.is_subset(&report.completed_replays)
        && !report.audit.expected_endings.is_empty();
    if report.limits.target_alternatives {
        report.targets_complete = report.complete;
        report.complete = false;
        report.limitations.push("Target audit: each observed alternative is forked once, with canonical continuation at previously covered selections. All input combinations are not exhausted; complete remains false. Earned replays are tested once from a naturally reached terminal snapshot, never by setting unlock flags.".into());
    }
    report
}
fn initial_report(audit: Audit, fingerprint: String, limits: Limits) -> Report {
    Report{schema_version:1,fingerprint,limits,failures:audit.issues.iter().map(|s|format!("audit:{s}")).collect(),audit,
        limitations:vec!["Static commands/edges are syntactic expectations, not executed-instruction coverage.".into(),
        "Only public Engine snapshots and flow.visited establish script observations; no per-token execution trace API exists.".into(),
        "Full-state hashes include clocks/counters; changing-state loops stop at explicit budgets, never by dropping semantic fields.".into(),
        "Headless timer/video completion tests continuation only; no rendering, audible audio or native-device acceptance.".into(),
        "Progress from different choice branches is never unioned or invented; cumulative unlocks require successful authentic new-game cycles.".into()],..Report::default()}
}
fn valid_limits(limits: &Limits) -> Result<()> {
    if limits.max_steps == 0
        || limits.max_states == 0
        || limits.max_frontier == 0
        || limits.max_snapshot_bytes == 0
        || limits.max_frontier_bytes == 0
        || limits.max_decisions == 0
        || limits.tick_ms == 0
    {
        return Err("verification limits must be positive".into());
    }
    Ok(())
}

/// Exhaust legal input paths. Frontier snapshots are full JSON States; only digests leave memory.
/// A branch is abandoned on errors/warnings, failed restore, loop or budget; none is skipped green.
pub fn verify_with_driver<D: Driver>(
    driver: &mut D,
    audit: Audit,
    fingerprint: String,
    limits: Limits,
) -> Result<Report> {
    valid_limits(&limits)?;
    let mut report = initial_report(audit, fingerprint, limits);
    let mut work = Work {
        queue: VecDeque::new(),
        bytes: 0,
    };
    let mut seen = BTreeSet::new();
    let mut branch_seen = BTreeSet::new();
    let mut restarted = BTreeSet::new();
    let mut replays = BTreeSet::new();
    let mut scheduled_choices = BTreeSet::new();
    let mut origin = "story".to_string();
    let mut decisions = Vec::new();
    let mut inherited_visits = BTreeSet::new();
    let mut running = true;
    report.operations += 1;
    if let Err(error) = driver.start() {
        let sha = serialized_digest(driver.state())?;
        observe(
            &mut report,
            driver.state(),
            &origin,
            &decisions,
            &sha,
            &inherited_visits,
        );
        blocker(
            &mut report,
            driver.state(),
            &origin,
            &decisions,
            error_code(&error),
            Some(&error),
            Some(Action::NewGame),
        );
        running = false;
    }
    report.pages = driver.state().pages;
    loop {
        if !running {
            let Some(job) = work.queue.pop_front() else {
                break;
            };
            work.bytes -= job.bytes.len();
            origin = job.origin;
            decisions = job.decisions;
            inherited_visits = job.inherited_visits;
            branch_seen.clear();
            let state: State =
                serde_json::from_slice(&job.bytes).map_err(|_| "internal_snapshot_invalid")?;
            if !consume(&mut report, &state, &origin, &decisions, None) {
                break;
            }
            if let Err(error) = driver.restore(state.clone()) {
                blocker(
                    &mut report,
                    &state,
                    &origin,
                    &decisions,
                    "restore_failed",
                    Some(&error),
                    None,
                );
                continue;
            }
            report.restored_states += 1;
            if serialized_digest(driver.state())? != digest(&job.bytes) {
                blocker(
                    &mut report,
                    &state,
                    &origin,
                    &decisions,
                    "restore_mismatch",
                    None,
                    None,
                );
                continue;
            }
            if !consume(
                &mut report,
                driver.state(),
                &origin,
                &decisions,
                Some(job.action.clone()),
            ) {
                break;
            }
            if let Err(error) = operate(driver, &job.action) {
                let sha = serialized_digest(driver.state())?;
                observe(
                    &mut report,
                    driver.state(),
                    &origin,
                    &decisions,
                    &sha,
                    &inherited_visits,
                );
                blocker(
                    &mut report,
                    driver.state(),
                    &origin,
                    &decisions,
                    error_code(&error),
                    Some(&error),
                    Some(job.action),
                );
                continue;
            }
            let next = driver.state();
            let postcondition = match &job.action {
                Action::Replay { .. } if !next.flow.replay || next.title || next.finished => {
                    Some("replay_did_not_start")
                }
                Action::NewGame if next.title || next.finished || next.flow.replay => {
                    Some("new_game_did_not_start")
                }
                Action::NewGame
                    if !state.progress.events.is_subset(&next.progress.events)
                        || !state.progress.cg.is_subset(&next.progress.cg)
                        || !state.progress.endings.is_subset(&next.progress.endings) =>
                {
                    Some("new_game_lost_progress")
                }
                _ if serialized_digest(next)? == digest(&job.bytes) => {
                    Some("input_did_not_change_state")
                }
                _ => None,
            };
            if let Some(code) = postcondition {
                blocker(
                    &mut report,
                    next,
                    &origin,
                    &decisions,
                    code,
                    None,
                    Some(job.action),
                );
                continue;
            }
            report.pages = report.pages.saturating_add(
                if matches!(&job.action, Action::NewGame | Action::Replay { .. }) {
                    driver.state().pages
                } else {
                    driver.state().pages.saturating_sub(state.pages)
                },
            );
            match job.action {
                Action::Choose { id } => {
                    report.choices += 1;
                    if origin == "story" {
                        if let Some(selection) = state.flow.selection_id {
                            report
                                .evidence
                                .entry(format!("story:selection:{selection}:choice:{id}"))
                                .or_insert_with(|| {
                                    evidence(
                                        &report.audit,
                                        &state,
                                        &origin,
                                        &decisions,
                                        &digest(&job.bytes),
                                    )
                                });
                        }
                    }
                }
                Action::Replay { id } => {
                    report.started_replays.insert(id);
                }
                Action::NewGame => {
                    report.restarts += 1;
                }
                _ => (),
            }
            running = true;
        }
        let state = driver.state().clone();
        let bytes = match snapshot(&state, &report.limits) {
            Ok(bytes) => bytes,
            Err(code) => {
                report.budget_exhausted = true;
                blocker(&mut report, &state, &origin, &decisions, &code, None, None);
                running = false;
                continue;
            }
        };
        let sha = digest(&bytes);
        observe(
            &mut report,
            &state,
            &origin,
            &decisions,
            &sha,
            &inherited_visits,
        );
        if state.blocked.is_some() {
            let error = state.blocked.as_deref().unwrap_or_default();
            blocker(
                &mut report,
                &state,
                &origin,
                &decisions,
                error_code(error),
                Some(error),
                None,
            );
            running = false;
            continue;
        }
        if let Some(warning) = state.warnings.first() {
            blocker(
                &mut report,
                &state,
                &origin,
                &decisions,
                "engine_warning",
                Some(warning),
                None,
            );
            running = false;
            continue;
        }
        if !branch_seen.insert(sha.clone()) {
            blocker(
                &mut report,
                &state,
                &origin,
                &decisions,
                "cycle_without_terminal",
                None,
                None,
            );
            running = false;
            continue;
        }
        if seen.contains(&(origin.clone(), sha.clone())) {
            report.duplicate_states += 1;
            if decisions
                .iter()
                .any(|d| d.state_sha256 == sha && matches!(&d.action, Action::Choose { .. }))
            {
                blocker(
                    &mut report,
                    &state,
                    &origin,
                    &decisions,
                    "choice_cycle_without_terminal",
                    None,
                    None,
                );
            }
            // Convergent fork states are safe; a repeated state within a running branch is a cycle.
            // Branch entry is already after its action, so no unexecuted choice can be lost.
            running = false;
            continue;
        }
        if seen.len() >= report.limits.max_states {
            report.budget_exhausted = true;
            blocker(
                &mut report,
                &state,
                &origin,
                &decisions,
                "state_budget",
                None,
                None,
            );
            break;
        }
        seen.insert((origin.clone(), sha.clone()));
        report.unique_states = seen.len();
        if origin != "story" && state.title && !state.finished {
            if let Some(id) = origin.strip_prefix("replay:") {
                report.completed_replays.insert(id.into());
            }
            running = false;
            continue;
        }
        if state.finished {
            let Some(ending) = state.flow.ending.as_ref().filter(|s| !s.is_empty()) else {
                blocker(
                    &mut report,
                    &state,
                    &origin,
                    &decisions,
                    "finished_without_genuine_ending",
                    None,
                    None,
                );
                running = false;
                continue;
            };
            if origin == "story" {
                let terminal = state
                    .flow
                    .event_id
                    .and_then(|id| report.audit.events.get(id))
                    .map(|e| e.script.clone())
                    .or_else(|| script_id(&state.cursor.script, &report.audit.main_scripts));
                if let Some(terminal) = terminal.filter(|s| {
                    report.audit.expected_endings.contains(s)
                        && (script_id(&state.cursor.script, &report.audit.main_scripts).as_ref()
                            == Some(s)
                            || state.flow.visited.contains(s))
                        && !state.flow.replay
                }) {
                    report.endings.insert(ending.clone());
                    report.reached_terminal_scripts.insert(terminal.clone());
                    report
                        .evidence
                        .entry(format!("story:ending:{terminal}"))
                        .or_insert_with(|| {
                            evidence(&report.audit, &state, &origin, &decisions, &sha)
                        });
                } else {
                    blocker(
                        &mut report,
                        &state,
                        &origin,
                        &decisions,
                        "ending_not_at_database_terminal",
                        None,
                        None,
                    );
                    running = false;
                    continue;
                }
                // Menus are consulted only after authentic progression. No flag or progress mutation.
                let progress = serialized_digest(&state.progress)?;
                if !consume(&mut report, &state, &origin, &decisions, None) {
                    break;
                }
                match driver.gallery() {
                    Ok(entries) => {
                        for e in entries.into_iter().filter(|e| e.unlocked) {
                            report.unlocked_gallery.insert(e.id);
                        }
                    }
                    Err(error) => blocker(
                        &mut report,
                        &state,
                        &origin,
                        &decisions,
                        "gallery_query_failed",
                        Some(&error),
                        None,
                    ),
                }
                if !consume(&mut report, &state, &origin, &decisions, None) {
                    break;
                }
                match driver.replay_list() {
                    Ok(entries) => {
                        for e in entries {
                            if e.unlocked {
                                report.unlocked_replays.insert(e.id.clone());
                                report.locked_replays.remove(&e.id);
                                if replays.insert((
                                    e.id.clone(),
                                    if report.limits.target_alternatives {
                                        String::new()
                                    } else {
                                        progress.clone()
                                    },
                                )) {
                                    let replay_origin = format!("replay:{}", e.id);
                                    work.push(
                                        &mut report,
                                        SnapshotRef {
                                            state: &state,
                                            bytes: &bytes,
                                        },
                                        &decisions,
                                        &replay_origin,
                                        Action::Replay { id: e.id },
                                        &inherited_visits,
                                    );
                                }
                            } else if !report.unlocked_replays.contains(&e.id) {
                                report.locked_replays.insert(e.id);
                            }
                        }
                    }
                    Err(error) => blocker(
                        &mut report,
                        &state,
                        &origin,
                        &decisions,
                        "replay_query_failed",
                        Some(&error),
                        None,
                    ),
                }
                if !report.limits.target_alternatives
                    && state.progress != Default::default()
                    && restarted.insert(progress)
                {
                    work.push(
                        &mut report,
                        SnapshotRef {
                            state: &state,
                            bytes: &bytes,
                        },
                        &decisions,
                        "story",
                        Action::NewGame,
                        &inherited_visits,
                    );
                }
            } else {
                report.replay_endings.insert(ending.clone());
                if let Some(id) = origin.strip_prefix("replay:") {
                    report.completed_replays.insert(id.into());
                }
            }
            running = false;
            continue;
        }
        if state.title {
            blocker(
                &mut report,
                &state,
                &origin,
                &decisions,
                "returned_to_title_without_ending",
                None,
                None,
            );
            running = false;
            continue;
        }
        let action = if let Some(video) = &state.scene.video {
            if video.looping {
                blocker(
                    &mut report,
                    &state,
                    &origin,
                    &decisions,
                    "looping_video_requires_defined_exit",
                    None,
                    None,
                );
                running = false;
                continue;
            }
            Action::FinishVideo {
                sequence: video.sequence,
            }
        } else if state.flow.wait_ms > 0 {
            // Visual deadlines include asynchronous/infinite work and never gate input.
            // Only explicit flow waits authorize ticking instead of advancing/choosing.
            Action::Tick {
                milliseconds: state.flow.wait_ms.min(report.limits.tick_ms),
            }
        } else if !state.flow.choices.is_empty() {
            let mut ids = BTreeSet::new();
            let mut enabled = Vec::new();
            let mut invalid = false;
            for choice in &state.flow.choices {
                if !ids.insert(choice.id) {
                    invalid = true;
                }
                if choice.enabled {
                    enabled.push(choice.id);
                }
            }
            if invalid || enabled.is_empty() {
                blocker(
                    &mut report,
                    &state,
                    &origin,
                    &decisions,
                    "invalid_or_deadlocked_choices",
                    None,
                    None,
                );
            } else {
                enabled.sort_unstable();
                let canonical = enabled[0];
                if report.limits.target_alternatives {
                    enabled.retain(|id| {
                        scheduled_choices.insert((origin.clone(), state.flow.selection_id, *id))
                    });
                    if enabled.is_empty() {
                        enabled.push(canonical);
                    }
                }
                for id in enabled {
                    work.push(
                        &mut report,
                        SnapshotRef {
                            state: &state,
                            bytes: &bytes,
                        },
                        &decisions,
                        &origin,
                        Action::Choose { id },
                        &inherited_visits,
                    );
                }
            }
            running = false;
            continue;
        } else {
            Action::Advance
        };
        if !consume(
            &mut report,
            &state,
            &origin,
            &decisions,
            Some(action.clone()),
        ) {
            break;
        }
        if let Err(error) = operate(driver, &action) {
            let sha = serialized_digest(driver.state())?;
            observe(
                &mut report,
                driver.state(),
                &origin,
                &decisions,
                &sha,
                &inherited_visits,
            );
            blocker(
                &mut report,
                driver.state(),
                &origin,
                &decisions,
                error_code(&error),
                Some(&error),
                Some(action),
            );
            running = false;
            continue;
        }
        let next = driver.state();
        if matches!(&action,Action::FinishVideo{sequence} if next.scene.video.as_ref().is_some_and(|v|v.sequence==*sequence))
        {
            blocker(
                &mut report,
                next,
                &origin,
                &decisions,
                "video_completion_did_not_release_sequence",
                None,
                Some(action),
            );
            running = false;
            continue;
        }
        report.pages = report
            .pages
            .saturating_add(next.pages.saturating_sub(state.pages));
        match action {
            Action::Tick { .. } if next.flow.wait_ms == 0 => {
                report.timers_completed += 1;
            }
            Action::Tick { .. } => (),
            Action::FinishVideo { .. } => report.videos_completed += 1,
            _ => (),
        }
        if serialized_digest(next)? == sha {
            blocker(
                &mut report,
                next,
                &origin,
                &decisions,
                "no_progress",
                None,
                Some(action),
            );
            running = false;
        } else if matches!(&action, Action::Tick { .. })
            && next.flow == state.flow
            && next.cursor == state.cursor
            && next.stack == state.stack
            && next.pages == state.pages
            && next.finished == state.finished
            && next.title == state.title
        {
            // An idle visual/ASD clock can advance even if a blocking flow wait is stuck.
            // A tick that continues the script/rearms a new wait is not a stalled wait.
            blocker(
                &mut report,
                next,
                &origin,
                &decisions,
                "timer_did_not_progress",
                None,
                Some(action),
            );
            running = false;
        }
    }
    report.exhausted = work.queue.is_empty() && !report.budget_exhausted;
    Ok(finish(report))
}

pub fn verify_with_limits(assets: Arc<Store>, limits: Limits) -> Result<Report> {
    valid_limits(&limits)?;
    let audit = audit(&assets);
    let fingerprint = assets.fingerprint.clone();
    match Engine::open(assets) {
        Ok(mut engine) => verify_with_driver(&mut engine, audit, fingerprint, limits),
        Err(error) => {
            let mut report = initial_report(audit, fingerprint, limits);
            blocker(
                &mut report,
                &State::default(),
                "story",
                &[],
                "engine_open_failed",
                Some(&error),
                None,
            );
            Ok(finish(report))
        }
    }
}
pub fn verify(assets: Arc<Store>, max_steps: u64) -> Result<Report> {
    verify_with_limits(
        assets,
        Limits {
            max_steps,
            ..Limits::default()
        },
    )
}
