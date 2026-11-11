use crate::model::State;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: u32 = 1;
const LIMIT: u64 = 4 * 1024 * 1024;
pub const SLOTS: [u32; 7] = [1, 2, 3, 4, 5, 6, 9];
static TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
pub struct SaveInfo {
    pub slot: u32,
    pub exists: bool,
    pub compatible: bool,
    pub saved_at: u64,
    pub pages: u64,
    pub speaker: String,
    pub text: String,
    pub error: Option<String>,
}

pub fn catalog(
    directory: &Path,
    fingerprint: &str,
    mut validate: impl FnMut(&State) -> Result<(), String>,
) -> Vec<SaveInfo> {
    SLOTS
        .into_iter()
        .map(|slot| {
            let path = directory.join(format!("slot-{slot}.json"));
            let mut info = SaveInfo {
                slot,
                exists: path.exists(),
                compatible: false,
                saved_at: 0,
                pages: 0,
                speaker: String::new(),
                text: String::new(),
                error: None,
            };
            if info.exists {
                info.saved_at = path
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|t| t.as_millis().min(u64::MAX as u128) as u64)
                    .unwrap_or(0);
                match read(directory, slot, fingerprint).and_then(|state| {
                    validate(&state)?;
                    Ok(state)
                }) {
                    Ok(state) => {
                        info.compatible = true;
                        info.pages = state.pages;
                        info.speaker = state.scene.speaker;
                        info.text = state.scene.text.chars().take(160).collect();
                    }
                    Err(error) => info.error = Some(error),
                }
            }
            info
        })
        .collect()
}

pub fn latest(catalog: &[SaveInfo]) -> Option<u32> {
    catalog
        .iter()
        .filter(|s| s.compatible)
        .max_by_key(|s| (s.saved_at, s.slot == 9, s.slot))
        .map(|s| s.slot)
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    assets: String,
    state: State,
}

pub fn write(directory: &Path, slot: u32, fingerprint: &str, state: &State) -> Result<(), String> {
    if !(1..=9).contains(&slot) {
        return Err("invalid save slot".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&Envelope {
        version: VERSION,
        assets: fingerprint.into(),
        state: state.clone(),
    })
    .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("save exceeds limit".into());
    }
    let path = directory.join(format!("slot-{slot}.json"));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let sequence = TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(
        ".slot-{slot}-{}-{stamp}-{sequence}.tmp",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
pub fn read(directory: &Path, slot: u32, fingerprint: &str) -> Result<State, String> {
    if !(1..=9).contains(&slot) {
        return Err("invalid save slot".into());
    }
    let file = std::fs::File::open(directory.join(format!("slot-{slot}.json")))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("save exceeds limit".into());
    }
    let save: Envelope = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if save.version != VERSION {
        return Err("save version unsupported".into());
    }
    if save.assets != fingerprint {
        return Err("save belongs to different XP3 archive/patch indexes".into());
    }
    Ok(save.state)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_wrong_resource_rejected() {
        let d = tempfile::tempdir().unwrap();
        let state = State::default();
        write(d.path(), 1, "a", &state).unwrap();
        assert_eq!(read(d.path(), 1, "a").unwrap(), state);
        assert!(read(d.path(), 1, "b").is_err());
    }
    #[test]
    fn malformed_missing_and_wrong_version_rejected() {
        let d = tempfile::tempdir().unwrap();
        assert!(read(d.path(), 1, "a").is_err());
        assert!(write(d.path(), 10, "a", &State::default()).is_err());
        std::fs::write(
            d.path().join("slot-1.json"),
            br#"{"version":99,"assets":"a","state":{}}"#,
        )
        .unwrap();
        assert!(read(d.path(), 1, "a").is_err());
    }

    #[test]
    fn slots_are_independent_and_corrupt_or_wrong_archive_saves_are_not_continuable() {
        let d = tempfile::tempdir().unwrap();
        let first = State {
            pages: 10,
            ..State::default()
        };
        let automatic = State {
            pages: 20,
            ..State::default()
        };
        write(d.path(), 1, "a", &first).unwrap();
        write(d.path(), 9, "a", &automatic).unwrap();
        write(d.path(), 2, "other", &first).unwrap();
        std::fs::write(d.path().join("slot-3.json"), b"corrupt").unwrap();
        let entries = catalog(d.path(), "a", |_| Ok(()));
        assert_eq!(latest(&entries), Some(9));
        assert_eq!(read(d.path(), 1, "a").unwrap().pages, 10);
        assert_eq!(read(d.path(), 9, "a").unwrap().pages, 20);
        assert!(entries
            .iter()
            .filter(|s| s.slot == 2 || s.slot == 3)
            .all(|s| s.exists && !s.compatible && s.error.is_some()));
        assert!(entries
            .iter()
            .filter(|s| (4..=6).contains(&s.slot))
            .all(|s| !s.exists));
        assert_eq!(latest(&catalog(d.path(), "missing", |_| Ok(()))), None);
        assert_eq!(
            latest(&catalog(d.path(), "a", |_| Err("invalid state".into()))),
            None
        );
    }

    #[test]
    fn stale_temporary_does_not_block_save_and_failed_write_keeps_previous_slot() {
        let d = tempfile::tempdir().unwrap();
        let stale = d.path().join(format!(".slot-1-{}.tmp", std::process::id()));
        std::fs::write(&stale, b"do not remove another writer").unwrap();
        write(d.path(), 1, "a", &State::default()).unwrap();
        let mut oversized = State::default();
        oversized.scene.text = "x".repeat(LIMIT as usize);
        assert!(write(d.path(), 1, "a", &oversized).is_err());
        assert_eq!(read(d.path(), 1, "a").unwrap(), State::default());
        assert!(stale.is_file());
    }
}
