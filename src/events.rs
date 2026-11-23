//! The game database is authoritative; script names are never incremented.
use crate::tjs;
use serde_json::Value;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug)]
pub struct Event {
    pub script: String,
    pub route: usize,
    pub next: Option<String>,
    pub terminal: bool,
    pub title: String,
}
#[derive(Clone, Debug)]
pub struct Selection {
    pub script: String,
    pub destinations: Vec<String>,
    pub text: Vec<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Database {
    pub routes: Vec<Route>,
    pub events: Vec<Event>,
    pub selections: Vec<Selection>,
}
#[derive(Clone, Debug)]
pub struct Route {
    pub title: Option<String>,
    pub previous: Option<String>,
    pub terminal: bool,
}

/// A read-only projection of the event/selection graph as a navigable tree.
/// The database is authoritative; the tree is rebuilt on demand from `progress`
/// and never persists additional state.
#[derive(Clone, Debug, serde::Serialize)]
pub struct FlowTree {
    /// Display titles per route index; `route_title` already synthesizes a name
    /// for placeholder rows. Index 0 is the common route.
    pub routes: Vec<String>,
    /// `nodes` indices of each route's entry point, parallel to `routes`.
    pub roots: Vec<usize>,
    pub nodes: Vec<FlowNode>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowNodeKind {
    /// A linear segment start: the common-route head or a route's `previous`.
    Chapter,
    /// A choice point: this event's script hosts a selection with destinations.
    Choice,
    /// `event.terminal`: the end of a route.
    Ending,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct FlowEdge {
    /// Selection text when this edge is a branch; `None` for linear `next`.
    pub label: Option<String>,
    /// `(selection_id, choice_id)` when this edge comes from a choice point.
    pub selection: Option<usize>,
    pub choice: Option<usize>,
    /// `nodes` index of the destination; `None` when the edge is a dead end
    /// (terminal event, or a selection destination the database left empty).
    pub to: Option<usize>,
    /// The player took this branch before (`selection:{sel}:{choice}` flag).
    pub walked: bool,
    /// The destination event has been read (and may be jumped to).
    pub unlocked: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct FlowNode {
    /// `Database::events` index this node was projected from.
    pub event: usize,
    pub route: usize,
    pub kind: FlowNodeKind,
    pub title: String,
    /// `progress.events` contains this event's script.
    pub visited: bool,
    /// The player may jump to this node (visited, or an ending/walked edge).
    pub unlocked: bool,
    pub edges: Vec<FlowEdge>,
}

/// Where `Engine::start_from` should enter the tree.
#[derive(Clone, Copy, Debug)]
pub enum FlowTarget {
    /// Jump to the start of `events[index]` (a Chapter or ChoicePoint node).
    Event(usize),
    /// Enter the event that hosts `selection` and immediately take `choice`.
    Choice { selection: usize, choice: usize },
}

/// Route the tree should mark as unlocked because the player reached it.
fn route_unlocked(progress: &crate::model::Progress, route: usize) -> bool {
    progress.endings.contains(&format!("route:{route}"))
}

impl Database {
    /// Project the event/selection graph into a tree the UI can draw and the
    /// engine can jump into. `progress` decides visited/walked/unlocked; the
    /// result is derived, so it is rebuilt per call and never stored.
    pub fn flow_tree(&self, progress: &crate::model::Progress) -> FlowTree {
        let routes: Vec<String> = self
            .routes
            .iter()
            .enumerate()
            .map(|(i, r)| {
                r.title
                    .clone()
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| format!("路线 {}", i + 1))
            })
            .collect();
        // Which event index each selection lives on; selections[].script is an
        // event script, so a node's choice edges come from its own selections.
        let mut choice_events: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (sel_id, sel) in self.selections.iter().enumerate() {
            if let Some(event) = self.index(&sel.script) {
                choice_events.entry(event).or_default().push(sel_id);
            }
        }
        // Roots: each route's entry event index. Route 0 is the common head
        // (event 0); route i starts at the event its `previous` points at,
        // falling back to the first event of that route when the metadata is a
        // placeholder. Computed before the node loop so `is_route_head` and
        // `roots` share one source of truth.
        let mut roots = Vec::with_capacity(routes.len());
        for (route, _) in routes.iter().enumerate() {
            let root = if route == 0 {
                Some(0)
            } else {
                self.routes
                    .get(route)
                    .and_then(|r| r.previous.as_ref())
                    .and_then(|prev| self.index(prev))
                    .or_else(|| self.events.iter().position(|e| e.route == route))
            };
            if let Some(root) = root {
                roots.push(root);
            }
        }
        let mut nodes = Vec::with_capacity(self.events.len());
        for (index, event) in self.events.iter().enumerate() {
            let visited = progress.events.contains(&event.script);
            let mut edges = Vec::new();
            if let Some(sel_ids) = choice_events.get(&index) {
                for &sel_id in sel_ids {
                    let sel = &self.selections[sel_id];
                    for (choice_id, (label, destination)) in sel
                        .text
                        .iter()
                        .zip(sel.destinations.iter())
                        .enumerate()
                    {
                        let to = if destination.is_empty() {
                            None
                        } else {
                            self.index(destination)
                        };
                        let walked = progress
                            .variables
                            .get(&format!("selection:{sel_id}:{choice_id}"))
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        let unlocked = to
                            .map(|d| progress.events.contains(&self.events[d].script))
                            .unwrap_or(false);
                        edges.push(FlowEdge {
                            label: Some(label.clone()),
                            selection: Some(sel_id),
                            choice: Some(choice_id),
                            to,
                            walked,
                            unlocked: unlocked || walked,
                        });
                    }
                }
            }
            if let Ok(Some(next)) = self.next(index) {
                let unlocked = progress.events.contains(&self.events[next].script);
                edges.push(FlowEdge {
                    label: None,
                    selection: None,
                    choice: None,
                    to: Some(next),
                    walked: unlocked,
                    unlocked,
                });
            }
            let kind = if event.terminal {
                FlowNodeKind::Ending
            } else if choice_events.contains_key(&index) {
                FlowNodeKind::Choice
            } else {
                FlowNodeKind::Chapter
            };
            // A node is jumpable when the player has read it or reached its
            // route's ending. Route heads are also open: the common head is the
            // new-game entry, and a heroine route's head is open once its route
            // is reachable — which `route_done` already covers — so heads are
            // jumpable only when visited or their route is done.
            let route_done = route_unlocked(progress, event.route);
            let is_common_head = index == 0;
            nodes.push(FlowNode {
                event: index,
                route: event.route,
                kind,
                title: event.title.clone(),
                visited,
                unlocked: visited || route_done || is_common_head,
                edges,
            });
        }
        FlowTree { routes, roots, nodes }
    }
}
fn string(v: Option<&Value>) -> Result<String> {
    v.and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or("invalid event database string".into())
}
fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}
impl Database {
    pub fn parse(source: &str) -> Result<Self> {
        let table = tjs::table(source, "lf.EV_EventListInfo")?;
        let rows = table.as_array().ok_or("event table is not an array")?;
        if rows.is_empty() || rows.len() > 2048 {
            return Err("invalid event table size".into());
        }
        let mut db = Self::default();
        for row in rows {
            // Compatibility for the original engine's single-event synthetic fixture.
            let event = if let Some(s) = row.as_str() {
                Event {
                    script: s.into(),
                    route: 0,
                    next: None,
                    terminal: true,
                    title: s.into(),
                }
            } else {
                let r = row.as_array().ok_or("invalid event row")?;
                if r.len() < 6 {
                    return Err("short event row".into());
                }
                let script = string(r.first())?;
                let route = r[1]
                    .as_u64()
                    .filter(|n| *n < 256)
                    .ok_or("invalid event route")? as usize;
                let terminal = r[3].as_str() == Some("End");
                let next = if terminal || r[3].as_i64() == Some(-1) {
                    None
                } else {
                    Some(string(r.get(3))?)
                };
                Event {
                    script,
                    route,
                    next,
                    terminal,
                    title: string(r.get(5))?,
                }
            };
            if !safe_id(&event.script)
                || event.title.len() > 8192
                || db
                    .events
                    .iter()
                    .any(|e| e.script.eq_ignore_ascii_case(&event.script))
            {
                return Err("unsafe or duplicate event ID".into());
            }
            db.events.push(event);
        }
        if source.contains("lf.EV_RouteInfo") {
            let routes = tjs::table(source, "lf.EV_RouteInfo")?;
            for row in routes
                .as_array()
                .filter(|r| r.len() <= 256)
                .ok_or("invalid route table")?
            {
                let row = row.as_array().ok_or("invalid route row")?;
                if row.len() == 1 && row[0] == Value::Bool(false) {
                    db.routes.push(Route {
                        title: None,
                        previous: None,
                        terminal: false,
                    });
                    continue;
                }
                if row.len() != 3 {
                    return Err("invalid route columns".into());
                }
                let title = string(row.first())?;
                let previous = string(row.get(1))?;
                if title.len() > 8192
                    || previous.len() > 128
                    || !matches!(row[2].as_str(), Some("End" | "Next"))
                {
                    return Err("invalid route metadata".into());
                }
                db.routes.push(Route {
                    title: Some(title),
                    previous: Some(previous),
                    terminal: row[2] == "End",
                });
            }
            if db.events.iter().any(|e| e.route >= db.routes.len()) {
                return Err("event references unknown route".into());
            }
        }
        if source.contains("lf.EV_SelectListInfo") {
            let value = tjs::table(source, "lf.EV_SelectListInfo")?;
            let rows = value.as_array().ok_or("selection table is not an array")?;
            if rows.len() > 256 {
                return Err("selection count limit".into());
            }
            for row in rows {
                let r = row.as_array().ok_or("invalid selection row")?;
                if r.len() < 5 {
                    return Err("short selection row".into());
                }
                let script = string(r.first())?;
                if db.index(&script).is_none() {
                    return Err("selection references unknown event".into());
                }
                let text = r[4]
                    .as_array()
                    .ok_or("invalid selection text array")?
                    .iter()
                    .map(|v| string(Some(v)))
                    .collect::<Result<Vec<_>>>()?;
                let destinations = r[1]
                    .as_array()
                    .ok_or("invalid selection destinations")?
                    .iter()
                    .map(|v| string(Some(v)))
                    .collect::<Result<Vec<_>>>()?;
                if text.is_empty()
                    || text.len() > 32
                    || destinations.len() < text.len()
                    || text.iter().any(|t| t.len() > 8192)
                {
                    return Err("invalid selection dimensions".into());
                }
                if destinations
                    .iter()
                    .any(|s| !s.is_empty() && db.index(s).is_none())
                {
                    return Err("unknown selection destination".into());
                }
                db.selections.push(Selection {
                    script,
                    destinations,
                    text,
                });
            }
        }
        for (i, event) in db.events.iter().enumerate() {
            if let Some(next) = &event.next {
                if db.index(next).is_none() {
                    return Err("event has unknown successor".into());
                }
            }
            if !event.terminal && event.next.is_none() && i + 1 == db.events.len() {
                return Err("unterminated final event".into());
            }
        }
        Ok(db)
    }
    pub fn index(&self, script: &str) -> Option<usize> {
        let script = script
            .rsplit('/')
            .next()
            .unwrap_or(script)
            .trim_end_matches(".ks");
        self.events
            .iter()
            .position(|e| e.script.eq_ignore_ascii_case(script))
    }
    pub fn next(&self, index: usize) -> Result<Option<usize>> {
        let event = self.events.get(index).ok_or("event index out of range")?;
        if event.terminal {
            Ok(None)
        } else if let Some(next) = &event.next {
            self.index(next)
                .map(Some)
                .ok_or("event successor missing".into())
        } else if index + 1 < self.events.len() {
            Ok(Some(index + 1))
        } else {
            Err("event successor out of range".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn database_order_not_numeric_chaining() {
        let db=Database::parse("lf.EV_EventListInfo = [['intro',0,'End',-1,[-1,0],'Intro'],['later',1,-1,'End',[-1,-1],'Later']];").unwrap();
        assert_eq!(db.next(0).unwrap(), Some(1));
        assert_eq!(db.next(1).unwrap(), None);
        assert!(db.next(2).is_err());
    }
    #[test]
    fn route_metadata_and_event_routes_are_validated() {
        let source="lf.EV_RouteInfo=[['Common','End','Next'],['Route','intro','End'],[false]];\nlf.EV_EventListInfo=[['intro',0,'End',-1,[-1,-1],'Intro'],['last',1,'intro','End',[-1,-1],'Last']];";
        let db = Database::parse(source).unwrap();
        assert!(!db.routes[0].terminal);
        assert!(db.routes[1].terminal);
        assert_eq!(db.routes[1].previous.as_deref(), Some("intro"));
        assert_eq!(db.routes[1].title.as_deref(), Some("Route"));
        assert!(db.routes[2].title.is_none());
        assert!(Database::parse(&source.replace("'last',1", "'last',8")).is_err());
    }
    #[test]
    fn flow_tree_projects_routes_choices_and_progress() {
        let source = "lf.EV_RouteInfo=[['Common','End','Next'],['heroine','a1','End']];\n\
            lf.EV_EventListInfo=[\
              ['a1',0,-1,'a2',[-1,-1],'共通 1'],\
              ['a2',0,-1,'h1',[-1,-1],'共通 2'],\
              ['h1',1,-1,'End',[-1,-1],'女主 1']];\n\
            lf.EV_SelectListInfo=[['a2',['h1',''],0,-1,['去女主线','留共通']]];";
        let db = Database::parse(source).unwrap();
        let mut progress = crate::model::Progress::default();
        progress.events.insert("a1".into());
        progress.events.insert("a2".into());
        progress
            .variables
            .insert("selection:0:0".into(), serde_json::Value::Bool(true));
        let tree = db.flow_tree(&progress);
        assert_eq!(tree.routes, vec!["Common".to_string(), "heroine".to_string()]);
        assert_eq!(tree.roots.len(), 2);
        // a1: linear chapter, visited and unlocked, edge to a2.
        assert!(tree.nodes[0].visited && tree.nodes[0].unlocked);
        assert_eq!(tree.nodes[0].edges.len(), 1);
        assert_eq!(tree.nodes[0].edges[0].to, Some(1));
        // a2: choice point; the first branch leads to h1 and was walked.
        assert!(matches!(tree.nodes[1].kind, FlowNodeKind::Choice));
        let branch = &tree.nodes[1].edges[0];
        assert_eq!(branch.selection, Some(0));
        assert_eq!(branch.choice, Some(0));
        assert!(branch.walked && branch.unlocked);
        // a2 also carries a linear edge to h1 (its `next`), so it has both the
        // two labelled branches and one unlabelled linear edge.
        assert!(tree.nodes[1].edges.iter().any(|e| e.label.is_none() && e.to == Some(2)));
        // h1 is an unread route head: locked until its route ending is hit or
        // it is read via the branch from a2.
        assert!(matches!(tree.nodes[2].kind, FlowNodeKind::Ending));
        assert!(!tree.nodes[2].visited && !tree.nodes[2].unlocked);
        // Once the route ending is recorded the head becomes re-jumpable.
        progress.endings.insert("route:1".into());
        let tree = db.flow_tree(&progress);
        assert!(tree.nodes[2].unlocked);
    }
}
