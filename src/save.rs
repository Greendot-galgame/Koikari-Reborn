use crate::model::{Progress, State};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: u32 = 2;
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
    #[serde(default)]
    rollback: Vec<State>,
}

pub fn write(directory: &Path, slot: u32, fingerprint: &str, state: &State) -> Result<(), String> {
    write_with_rollback(directory, slot, fingerprint, state, &[])
}

pub fn write_with_rollback(
    directory: &Path,
    slot: u32,
    fingerprint: &str,
    state: &State,
    rollback: &[State],
) -> Result<(), String> {
    if !SLOTS.contains(&slot) {
        return Err("invalid save slot".into());
    }
    validate_state(state)?;
    let mut envelope = Envelope {
        version: VERSION,
        assets: fingerprint.into(),
        state: state.clone(),
        rollback: rollback.iter().rev().take(100).cloned().collect(),
    };
    envelope.rollback.reverse();
    for checkpoint in &envelope.rollback {
        validate_state(checkpoint)?;
    }
    let bytes = loop {
        let bytes = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
        if bytes.len() as u64 <= LIMIT {
            break bytes;
        }
        if envelope.rollback.is_empty() {
            return Err("save exceeds limit".into());
        }
        envelope.rollback.remove(0);
    };
    atomic_write(directory, &format!("slot-{slot}.json"), &bytes)
}

// Never follow a destination symlink or replace any file until serialization and
// size checks have succeeded. The directory must be app-owned (checked by FFI).
fn atomic_write(directory: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join(name);
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("refusing to replace a symlink in save storage".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let sequence = TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(
        ".{name}-{}-{stamp}-{sequence}.tmp",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        std::fs::File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
pub fn read(directory: &Path, slot: u32, fingerprint: &str) -> Result<State, String> {
    read_record(directory, slot, fingerprint).map(|record| record.state)
}

pub struct LoadedSave {
    pub state: State,
    pub needs_legacy_migration: bool,
    pub rollback: Vec<State>,
}

pub fn read_record(directory: &Path, slot: u32, fingerprint: &str) -> Result<LoadedSave, String> {
    if !SLOTS.contains(&slot) {
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
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let save: Envelope = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if save.version != 1 && save.version != VERSION {
        return Err("save version unsupported".into());
    }
    if save.assets != fingerprint {
        return Err("save belongs to different XP3 archive/patch indexes".into());
    }
    if save.version == VERSION
        && (value["state"].get("flow").is_none() || value["state"].get("progress").is_none())
    {
        return Err("full-state save is missing execution/progress data".into());
    }
    validate_state(&save.state)?;
    if save.rollback.len() > 100 {
        return Err("saved rollback exceeds limit".into());
    }
    for checkpoint in &save.rollback {
        validate_state(checkpoint)?;
    }
    // v1 files are read without rewriting them. Missing additive fields use
    // serde defaults; Engine::restore must migrate/validate the legacy cursor.
    let needs_legacy_migration = save.version == 1
        && (value["state"].get("flow").is_none()
            || (save.state.flow.event_id.is_none() && save.state.flow.variables.is_empty()));
    Ok(LoadedSave {
        state: save.state,
        needs_legacy_migration,
        rollback: save.rollback,
    })
}

/// Bounds independent of scenario semantics. Engine::validate_state still owns
/// cursor, route, branch and resource validation before accepting a restored save.
pub fn validate_state(state: &State) -> Result<(), String> {
    let flow = &state.flow;
    if flow.choices.len() > 128
        || flow.variables.len() > 16384
        || flow.visited.len() > 16384
        || flow.wait_ms > 86_400_000
        || state.scene.visual.commands.len() > 4096
        || state.scene.visual.previous.len() > 128
    {
        return Err("saved runtime exceeds limits".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    if flow
        .choices
        .iter()
        .any(|c| !ids.insert(c.id) || c.text.len() > 65536)
    {
        return Err("invalid saved choices".into());
    }
    for audio in [&state.scene.bgm, &state.scene.voice, &state.scene.se] {
        if audio
            .volume
            .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
            || audio.fade_ms > 86_400_000
        {
            return Err("invalid saved audio gain/fade".into());
        }
    }
    for l in state
        .scene
        .layers
        .iter()
        .chain(&state.scene.visual.previous)
    {
        if ![l.x, l.y, l.width, l.height, l.opacity]
            .iter()
            .all(|v| v.is_finite())
            || l.width < 0.0
            || l.height < 0.0
            || !(0.0..=1.0).contains(&l.opacity)
        {
            return Err("invalid saved visual layer".into());
        }
    }
    if state
        .scene
        .video
        .as_ref()
        .is_some_and(|v| v.asset.is_empty() || v.asset.len() > 4096)
    {
        return Err("invalid saved video".into());
    }
    validate_progress(&state.progress)
}

#[derive(Serialize, Deserialize)]
struct GlobalEnvelope {
    version: u32,
    assets: String,
    progress: Progress,
}

fn progress_name(fingerprint: &str) -> String {
    format!("progress-{:x}.json", Sha256::digest(fingerprint.as_bytes()))
}
fn validate_progress(progress: &Progress) -> Result<(), String> {
    if [&progress.events, &progress.cg, &progress.endings]
        .iter()
        .any(|s| s.len() > 65536 || s.iter().any(|v| v.len() > 4096))
        || progress.variables.len() > 16384
    {
        return Err("global progress exceeds limits".into());
    }
    Ok(())
}

/// Union real unlocks; keep the authoritative target's global variables on
/// conflicts. In particular, older saves/rollback cannot reset current globals.
pub fn merge_progress(target: &mut Progress, older: &Progress) {
    target.events.extend(older.events.iter().cloned());
    target.cg.extend(older.cg.iter().cloned());
    target.endings.extend(older.endings.iter().cloned());
    for (key, value) in &older.variables {
        target
            .variables
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
}
pub fn read_progress(directory: &Path, fingerprint: &str) -> Result<Progress, String> {
    let path = directory.join(progress_name(fingerprint));
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Progress::default()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("global progress exceeds limit".into());
    }
    let envelope: GlobalEnvelope =
        serde_json::from_slice(&bytes).map_err(|e| format!("global progress is corrupt: {e}"))?;
    if envelope.version != 1 {
        return Err("global progress version unsupported".into());
    }
    if envelope.assets != fingerprint {
        return Err("global progress resource fingerprint mismatch".into());
    }
    validate_progress(&envelope.progress)?;
    Ok(envelope.progress)
}
pub fn write_progress(
    directory: &Path,
    fingerprint: &str,
    progress: &Progress,
) -> Result<Progress, String> {
    let disk = read_progress(directory, fingerprint)?; // Corruption must never be silently reset.
    let mut merged = progress.clone();
    merge_progress(&mut merged, &disk);
    validate_progress(&merged)?;
    let bytes = serde_json::to_vec(&GlobalEnvelope {
        version: 1,
        assets: fingerprint.into(),
        progress: merged.clone(),
    })
    .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("global progress exceeds limit".into());
    }
    atomic_write(directory, &progress_name(fingerprint), &bytes)?;
    Ok(merged)
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
