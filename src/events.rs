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
}
