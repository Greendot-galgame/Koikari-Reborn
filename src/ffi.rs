//! Main-thread-only C bridge. Handles must not be accessed concurrently.
use crate::{
    media,
    model::{Layer, Progress, Scene},
    renderer::Renderer,
    save,
    story::Engine,
    xp3::{Assets, Store},
};
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

/// Immutable asset lease; unlike Host it may be transferred to one worker.
pub struct VideoExport {
    assets: Arc<Store>,
    asset: String,
    directory: PathBuf,
}
static VIDEO_EXPORT_LOCK: Mutex<()> = Mutex::new(());

pub struct Host {
    engine: Engine,
    saves: PathBuf,
    renderer: Option<Renderer>,
    progress: Progress,
    gallery_scene: Option<Scene>,
}
impl Host {
    fn restore_slot(&mut self, slot: u32) -> Result<(), String> {
        let record = save::read_record(&self.saves, slot, &self.engine.assets.fingerprint)?;
        let rollback = record.rollback;
        let mut state = record.state;
        let migrating =
            record.needs_legacy_migration || state.flow.revision == 0 || state.visual_revision == 0;
        if record.needs_legacy_migration {
            // Old preview saves never recorded branch variables. Reconstruct
            // only the authentic linear first-event prefix, with a finite budget.
            // Never guess a choice, skip a movie, or fabricate route variables.
            let mut migration = Engine::open(self.engine.assets.clone())?;
            migration.state.progress = self.progress.clone();
            migration.start()?;
            let first_event = migration.state.flow.event_id;
            let mut migrated = None;
            for _ in 0..10000 {
                let candidate = &migration.state;
                if candidate.blocked.is_some()
                    || candidate.flow.event_id != first_event
                    || !candidate.flow.choices.is_empty()
                    || candidate.scene.video.is_some()
                {
                    break;
                }
                if candidate.cursor == state.cursor
                    && candidate.stack == state.stack
                    && candidate.interlude.is_some() == state.interlude.is_some()
                    && candidate.finished == state.finished
                {
                    migrated = Some(candidate.clone());
                    break;
                }
                if candidate.finished {
                    break;
                }
                if candidate.flow.wait_ms > 0 {
                    migration.tick(candidate.flow.wait_ms.min(1000))?;
                } else {
                    migration.advance()?;
                }
            }
            let mut candidate = migrated.ok_or("legacy save cannot be migrated safely: no matching linear first-event state; original slot preserved")?;
            save::merge_progress(&mut candidate.progress, &state.progress);
            state = candidate;
        }
        save::validate_state(&state)?;
        self.engine.validate_state(&state)?;
        let mut progress = self.progress.clone();
        save::merge_progress(&mut progress, &state.progress);
        state.progress = progress;
        // Restore transactionally, including derived engine checkpoints. An
        // incomplete peer restore must not silently discard route/timer/media
        // fields or change the live game after returning an error.
        let mut restored = Engine::open(self.engine.assets.clone())?;
        restored.state.progress = self.progress.clone();
        restored.restore(state.clone())?;
        if !migrating && restored.state != state {
            return Err("runtime restore did not retain the complete saved state; original slot and current game preserved".into());
        }
        if !migrating {
            restored.restore_checkpoints(rollback)?;
        }
        self.engine = restored;
        Ok(())
    }
    fn sync_progress(&mut self, preserve_globals: bool) -> Result<(), String> {
        let mut merged = if preserve_globals {
            self.progress.clone()
        } else {
            self.engine.state.progress.clone()
        };
        save::merge_progress(&mut merged, &self.progress);
        save::merge_progress(&mut merged, &self.engine.state.progress);
        self.engine.state.progress = merged.clone();
        if merged != self.progress {
            // Keep current in-memory progress even when disk fails; do not lose it
            // on rollback. A later flush retries persistence and surfaces errors.
            self.progress = merged;
        }
        let disk = save::read_progress(&self.saves, &self.engine.assets.fingerprint)?;
        if disk != self.progress {
            self.progress =
                save::write_progress(&self.saves, &self.engine.assets.fingerprint, &self.progress)?;
        }
        self.engine.state.progress = self.progress.clone();
        Ok(())
    }
}
fn saves(h: &mut Host) -> Vec<save::SaveInfo> {
    let fingerprint = h.engine.assets.fingerprint.clone();
    save::catalog(&h.saves, &fingerprint, |state| {
        h.engine.validate_state(state)
    })
}
thread_local! { static ERROR:RefCell<String>=const{RefCell::new(String::new())}; }
fn run<T: Default>(f: impl FnOnce() -> Result<T, String>) -> T {
    ERROR.with(|e| e.borrow_mut().clear());
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            ERROR.with(|e| *e.borrow_mut() = error);
            T::default()
        }
        Err(_) => {
            ERROR.with(|e| *e.borrow_mut() = "internal Rust panic contained at C boundary".into());
            T::default()
        }
    }
}
unsafe fn string(pointer: *const c_char) -> Result<String, String> {
    if pointer.is_null() {
        return Err("null string".into());
    }
    CStr::from_ptr(pointer)
        .to_str()
        .map(str::to_owned)
        .map_err(|e| e.to_string())
}
fn cstring(value: String) -> *mut c_char {
    CString::new(value.replace('\0', "�"))
        .unwrap_or_default()
        .into_raw()
}
unsafe fn host<'a>(handle: *mut Host) -> Result<&'a mut Host, String> {
    handle.as_mut().ok_or_else(|| "null engine handle".into())
}

/// # Safety
/// Strings must be valid, null-terminated UTF-8 pointers for this call's duration.
#[no_mangle]
pub unsafe extern "C" fn kr_open(directory: *const c_char, saves: *const c_char) -> *mut Host {
    run(|| {
        let assets = Arc::new(Store::open(string(directory)?).map_err(|e| e.to_string())?);
        let resource_directory =
            std::fs::canonicalize(string(directory)?).map_err(|e| e.to_string())?;
        let saves = PathBuf::from(string(saves)?);
        // Resolve existing parents before creating storage: no files may be
        // written inside the selected original resource/savedata directory.
        let mut ancestor = saves.as_path();
        let mut suffix = Vec::new();
        while !ancestor.exists() {
            suffix.push(
                ancestor
                    .file_name()
                    .ok_or("invalid save directory")?
                    .to_owned(),
            );
            ancestor = ancestor.parent().ok_or("invalid save directory")?;
        }
        let mut resolved = std::fs::canonicalize(ancestor).map_err(|e| e.to_string())?;
        for part in suffix.into_iter().rev() {
            resolved.push(part);
        }
        if resolved.starts_with(&resource_directory) || resource_directory.starts_with(&resolved) {
            return Err("save storage must be separate from original resources/savedata".into());
        }
        let progress = save::read_progress(&resolved, &assets.fingerprint)?;
        let mut engine = Engine::open(assets)?;
        save::merge_progress(&mut engine.state.progress, &progress);
        Ok(Box::into_raw(Box::new(Host {
            engine,
            saves: resolved,
            renderer: None,
            progress,
            gallery_scene: None,
        })))
    })
}
/// # Safety
/// Pass null or a live handle from kr_open, once, on the main thread.
#[no_mangle]
pub unsafe extern "C" fn kr_close(handle: *mut Host) {
    if !handle.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(handle))));
    }
}
#[no_mangle]
pub extern "C" fn kr_error() -> *mut c_char {
    cstring(ERROR.with(|e| e.borrow().clone()))
}
/// # Safety
/// Pass null or a string returned by this bridge, exactly once.
#[no_mangle]
pub unsafe extern "C" fn kr_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}
/// # Safety
/// Handle must be live and exclusively accessible on the main thread.
#[no_mangle]
pub unsafe extern "C" fn kr_snapshot(handle: *mut Host) -> *mut c_char {
    run(|| {
        let h = host(handle)?;
        let mut snapshot = serde_json::to_value(&h.engine.state).map_err(|e| e.to_string())?;
        snapshot["archives"] = serde_json::json!(h.engine.assets.archive_names());
        Ok(cstring(
            serde_json::to_string(&snapshot).map_err(|e| e.to_string())?,
        ))
    })
}
/// # Safety
/// Supply a live main-thread handle and a valid null-terminated action string.
#[no_mangle]
pub unsafe extern "C" fn kr_action(handle: *mut Host, action: *const c_char) -> i32 {
    run(|| {
        let h = host(handle)?;
        let action = string(action)?;
        let (command, value) = action
            .split_once(':')
            .map_or((action.as_str(), None), |(command, value)| {
                (command, Some(value))
            });
        let slot = if command == "save" || command == "load" {
            let slot = value
                .unwrap_or("1")
                .parse::<u32>()
                .map_err(|_| "invalid save slot")?;
            if !save::SLOTS.contains(&slot) {
                return Err("invalid save slot".into());
            }
            slot
        } else {
            1
        };
        if value.is_some()
            && ![
                "save",
                "load",
                "rewind",
                "choose",
                "replay",
                "video-complete",
            ]
            .contains(&command)
        {
            return Err("unexpected action argument".into());
        }
        match command {
            "new" => h.engine.start()?,
            "advance" => {
                if h.engine.state.title
                    || h.engine.state.finished
                    || h.engine.state.blocked.is_some()
                    || !h.engine.state.flow.choices.is_empty()
                    || h.engine.state.scene.video.is_some()
                    || (h.engine.state.flow.wait_ms > 0 && !h.engine.state.flow.wait_skippable)
                {
                    return Err("story is not accepting advance input".into());
                }
                h.engine.advance()?;
            }
            "choose" => {
                let id = value
                    .ok_or("missing choice id")?
                    .parse::<usize>()
                    .map_err(|_| "invalid choice id")?;
                if h.engine.state.blocked.is_some()
                    || h.engine.state.title
                    || h.engine.state.finished
                    || h.engine.state.scene.video.is_some()
                    || h.engine.state.flow.wait_ms > 0
                    || !h
                        .engine
                        .state
                        .flow
                        .choices
                        .iter()
                        .any(|c| c.id == id && c.enabled)
                {
                    return Err("choice is unavailable or disabled".into());
                }
                h.engine.choose(id)?;
            }
            "replay" => {
                let id = value
                    .filter(|id| !id.is_empty())
                    .ok_or("missing replay id")?;
                if !h
                    .engine
                    .replay_list()?
                    .iter()
                    .any(|e| e.id == id && e.unlocked)
                {
                    return Err("replay is locked or unknown".into());
                }
                h.engine.start_replay(id)?;
            }
            "video-complete" => {
                let sequence = value
                    .ok_or("missing video sequence")?
                    .parse::<u64>()
                    .map_err(|_| "invalid video sequence")?;
                if h.engine.state.scene.video.as_ref().map(|v| v.sequence) != Some(sequence) {
                    return Err("stale video completion".into());
                }
                h.engine.finish_video(sequence)?;
            }
            "flush" => {}
            "title" => h.engine.title()?,
            "back" => {
                let page = h
                    .engine
                    .previous_pages()
                    .last()
                    .ok_or("no earlier page available")?
                    .pages;
                h.engine.rewind(page)?;
            }
            "rewind" => h.engine.rewind(
                value
                    .ok_or("missing rollback page")?
                    .parse()
                    .map_err(|_| "invalid rollback page")?,
            )?,
            "save" => {
                if h.engine.state.title {
                    return Err("cannot save the title screen".into());
                }
                if h.engine.state.blocked.is_some() {
                    return Err("cannot save a blocked script".into());
                }
                h.sync_progress(false)?;
                save::write_with_rollback(
                    &h.saves,
                    slot,
                    &h.engine.assets.fingerprint,
                    &h.engine.state,
                    &h.engine.previous_pages().cloned().collect::<Vec<_>>(),
                )?;
            }
            "load" => {
                h.restore_slot(slot)?;
            }
            "continue" => {
                let mut entries = saves(h);
                let mut failure = "no compatible save available".to_owned();
                loop {
                    let slot = save::latest(&entries).ok_or(failure.clone())?;
                    match h.restore_slot(slot) {
                        Ok(()) => break,
                        Err(error) => {
                            failure = error;
                            for entry in &mut entries {
                                if entry.slot == slot {
                                    entry.compatible = false;
                                }
                            }
                        }
                    }
                }
            }
            _ => return Err("unknown host action".into()),
        }
        h.gallery_scene = None;
        h.sync_progress(matches!(
            command,
            "new" | "title" | "back" | "rewind" | "load" | "continue" | "replay"
        ))?;
        Ok(1)
    })
}
/// # Safety
/// Handle must be live and exclusively accessible on the main thread.
#[no_mangle]
pub unsafe extern "C" fn kr_timeline(handle: *mut Host) -> *mut c_char {
    run(|| {
        let entries: Vec<_> = host(handle)?.engine.previous_pages().map(|state| serde_json::json!({
            "page": state.pages, "speaker": state.scene.speaker, "text": state.scene.text.chars().take(160).collect::<String>(),
            "interlude": state.interlude.is_some()
        })).collect();
        Ok(cstring(
            serde_json::to_string(&entries).map_err(|e| e.to_string())?,
        ))
    })
}
/// # Safety
/// Handle must be live and exclusively accessible on the main thread.
#[no_mangle]
pub unsafe extern "C" fn kr_has_save(handle: *mut Host) -> i32 {
    run(|| Ok(i32::from(save::latest(&saves(host(handle)?)).is_some())))
}
/// # Safety
/// Handle must be live and exclusively accessible on the main thread.
#[no_mangle]
pub unsafe extern "C" fn kr_saves(handle: *mut Host) -> *mut c_char {
    run(|| {
        Ok(cstring(
            serde_json::to_string(&saves(host(handle)?)).map_err(|e| e.to_string())?,
        ))
    })
}
/// # Safety
/// Live exclusive main-thread handle. Never call while the host is backgrounded.
#[no_mangle]
pub unsafe extern "C" fn kr_tick(handle: *mut Host, milliseconds: u64) -> i32 {
    run(|| {
        if milliseconds > 1000 {
            return Err("clock delta exceeds 1000ms".into());
        }
        let h = host(handle)?;
        if h.gallery_scene.is_some() || h.engine.state.blocked.is_some() {
            return Ok(1);
        }
        let changed = h.engine.tick(milliseconds)?;
        if h.engine.state.progress != h.progress {
            h.sync_progress(false)?;
        }
        Ok(if changed { 2 } else { 1 })
    })
}
/// # Safety
/// Live exclusive main-thread handle. Returned JSON must be freed.
#[no_mangle]
pub unsafe extern "C" fn kr_gallery(handle: *mut Host) -> *mut c_char {
    run(|| {
        Ok(cstring(
            serde_json::to_string(&host(handle)?.engine.gallery()?).map_err(|e| e.to_string())?,
        ))
    })
}
/// Read encoded bytes for a currently unlocked gallery entry, without decoding
/// or changing the gallery preview, story, progress, or saves.
/// # Safety
/// Supply a live exclusive main-thread handle, a valid UTF-8 gallery entry ID
/// C string, and a writable length pointer. Free returned bytes exactly once
/// with kr_bytes_free and the original length. Failure returns null and zero
/// length (when nonnull), with details available through kr_error.
#[no_mangle]
pub unsafe extern "C" fn kr_gallery_image(
    handle: *mut Host,
    id: *const c_char,
    length: *mut usize,
) -> *mut u8 {
    run(|| {
        if length.is_null() {
            return Err("null gallery image length pointer".into());
        }
        *length = 0;
        let h = host(handle)?;
        let id = string(id)?;
        let entry = h
            .engine
            .gallery()?
            .into_iter()
            .find(|entry| entry.id == id && entry.unlocked)
            .ok_or("CG is locked or unknown")?;
        let mut bytes = h
            .engine
            .assets
            .bytes_limited(&entry.asset, 32 * 1024 * 1024)
            .map_err(|e| e.to_string())?
            .into_boxed_slice();
        *length = bytes.len();
        let pointer = bytes.as_mut_ptr();
        std::mem::forget(bytes);
        Ok(pointer)
    })
}
/// # Safety
/// Live exclusive main-thread handle. Returned JSON must be freed.
#[no_mangle]
pub unsafe extern "C" fn kr_replays(handle: *mut Host) -> *mut c_char {
    run(|| {
        Ok(cstring(
            serde_json::to_string(&host(handle)?.engine.replay_list()?)
                .map_err(|e| e.to_string())?,
        ))
    })
}
/// # Safety
/// Live exclusive main-thread handle. Returned absolute path must be freed.
#[no_mangle]
pub unsafe extern "C" fn kr_video(handle: *mut Host) -> *mut c_char {
    run(|| {
        let h = host(handle)?;
        let video = h
            .engine
            .state
            .scene
            .video
            .as_ref()
            .ok_or("no active video")?;
        let _serial = VIDEO_EXPORT_LOCK
            .lock()
            .map_err(|_| "video export worker poisoned")?;
        let path =
            media::export_video(&h.engine.assets, &video.asset, &h.saves.join("VideoCache"))?;
        Ok(cstring(
            path.to_str().ok_or("video path is not UTF-8")?.to_owned(),
        ))
    })
}
/// # Safety
/// Live main-thread Host; free JSON with kr_string_free. Message-layer cell ASD
/// uses explicit copy dimensions and a separate sheet, independently of scene atlases.
#[no_mangle]
pub unsafe extern "C" fn kr_message_glyph(handle: *mut Host) -> *mut c_char {
    run(|| {
        let h = host(handle)?;
        let base = h
            .engine
            .assets
            .find("ui/ms/line_gryph", &["webp"])
            .map_err(|e| e.to_string())?;
        let size = |name: &str| -> Result<(u32, u32), String> {
            image::ImageReader::new(std::io::Cursor::new(
                h.engine.assets.bytes(name).map_err(|e| e.to_string())?,
            ))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|e| e.to_string())
        };
        let (width, height) = size(&base)?;
        let asset = h
            .engine
            .assets
            .find("ui/ms/line_gryph_a", &["webp"])
            .map_err(|e| e.to_string())?;
        let (sheet_width, sheet_height) = size(&asset)?;
        let timeline = crate::animation::AtlasAnimation::parse_message_cell(
            &h.engine
                .assets
                .text("ui/ms/line_gryph.asd")
                .map_err(|e| e.to_string())?,
            [width, height],
        )?;
        timeline.validate_dimensions(sheet_width, sheet_height)?;
        Ok(cstring(serde_json::to_string(&serde_json::json!({"asset":asset,"width":sheet_width,"height":sheet_height,"timeline":timeline})).map_err(|e|e.to_string())?))
    })
}
/// # Safety
/// Live main-thread Host. The returned lease must be consumed exactly once by
/// kr_video_export, on a serial worker. It never borrows the mutable Host.
#[no_mangle]
pub unsafe extern "C" fn kr_video_request(handle: *mut Host) -> *mut VideoExport {
    run(|| {
        let h = host(handle)?;
        let video = h
            .engine
            .state
            .scene
            .video
            .as_ref()
            .ok_or("no active video")?;
        Ok(Box::into_raw(Box::new(VideoExport {
            assets: h.engine.assets.clone(),
            asset: video.asset.clone(),
            directory: h.saves.join("VideoCache"),
        })))
    })
}
/// # Safety
/// Consume one live lease, exactly once, on any thread. Error is thread-local;
/// fetch kr_error on that same worker before returning to the main actor.
#[no_mangle]
pub unsafe extern "C" fn kr_video_export(request: *mut VideoExport) -> *mut c_char {
    run(|| {
        if request.is_null() {
            return Err("null video export lease".into());
        }
        let request = Box::from_raw(request);
        let _serial = VIDEO_EXPORT_LOCK
            .lock()
            .map_err(|_| "video export worker poisoned")?;
        media::prune_video_partials(&request.directory)?;
        let path = media::export_video(&request.assets, &request.asset, &request.directory)?;
        Ok(cstring(
            path.to_str().ok_or("video path is not UTF-8")?.to_owned(),
        ))
    })
}
/// # Safety
/// Live exclusive main-thread handle and UTF-8 C string. Does not alter story.
#[no_mangle]
pub unsafe extern "C" fn kr_gallery_show(handle: *mut Host, id: *const c_char) -> i32 {
    run(|| {
        let h = host(handle)?;
        let id = string(id)?;
        let entry = h
            .engine
            .gallery()?
            .into_iter()
            .find(|e| e.id == id && e.unlocked)
            .ok_or("CG is locked or unknown")?;
        let bytes = h
            .engine
            .assets
            .bytes(&entry.asset)
            .map_err(|e| e.to_string())?;
        let (width, height) = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|e| e.to_string())?;
        if width == 0 || height == 0 {
            return Err("invalid gallery image dimensions".into());
        }
        let scale = (crate::model::WIDTH / width as f32).min(crate::model::HEIGHT / height as f32);
        let width = width as f32 * scale;
        let height = height as f32 * scale;
        h.gallery_scene = Some(Scene {
            layers: vec![Layer {
                asset: entry.asset,
                x: (crate::model::WIDTH - width) / 2.0,
                y: (crate::model::HEIGHT - height) / 2.0,
                width,
                height,
                opacity: 1.0,
                group: 0,
            }],
            message_visible: false,
            ..Scene::default()
        });
        Ok(1)
    })
}
/// # Safety
/// Live exclusive main-thread handle.
#[no_mangle]
pub unsafe extern "C" fn kr_gallery_hide(handle: *mut Host) -> i32 {
    run(|| {
        host(handle)?.gallery_scene = None;
        Ok(1)
    })
}
/// # Safety
/// Handle and CAMetalLayer must be live on the main thread; layer needs a Metal device.
#[no_mangle]
pub unsafe extern "C" fn kr_draw(
    handle: *mut Host,
    layer: *mut c_void,
    width: f32,
    height: f32,
) -> i32 {
    run(|| {
        let h = host(handle)?;
        if !width.is_finite() || !height.is_finite() || width < 0.0 || height < 0.0 {
            return Err("invalid drawable dimensions".into());
        }
        if width == 0.0 || height == 0.0 {
            return Ok(1);
        }
        if h.renderer.is_none() {
            h.renderer = Some(Renderer::new(layer)?);
        }
        h.renderer.as_mut().ok_or("renderer unavailable")?.draw(
            layer,
            h.gallery_scene.as_ref().unwrap_or(&h.engine.state.scene),
            &h.engine.assets,
            width,
            height,
        )?;
        Ok(1)
    })
}
/// # Safety
/// Supply a live main-thread handle, a valid C string and a writable length pointer.
#[no_mangle]
pub unsafe extern "C" fn kr_audio(
    handle: *mut Host,
    name: *const c_char,
    length: *mut usize,
) -> *mut u8 {
    if length.is_null() {
        return std::ptr::null_mut();
    }
    *length = 0;
    run(|| {
        let h = host(handle)?;
        let name = string(name)?;
        let bytes = h.engine.assets.bytes(&name).map_err(|e| e.to_string())?;
        let mut wave = media::wav(&bytes)?.into_boxed_slice();
        *length = wave.len();
        let pointer = wave.as_mut_ptr();
        std::mem::forget(wave);
        Ok(pointer)
    })
}
/// # Safety
/// Supply a live main-thread handle, valid C string and writable length pointer.
#[no_mangle]
pub unsafe extern "C" fn kr_asset(
    handle: *mut Host,
    name: *const c_char,
    length: *mut usize,
) -> *mut u8 {
    if length.is_null() {
        return std::ptr::null_mut();
    }
    *length = 0;
    run(|| {
        let mut bytes = host(handle)?
            .engine
            .assets
            .bytes(&string(name)?)
            .map_err(|e| e.to_string())?
            .into_boxed_slice();
        *length = bytes.len();
        let pointer = bytes.as_mut_ptr();
        std::mem::forget(bytes);
        Ok(pointer)
    })
}
/// Read one of the two optional title decorations without changing gameplay state.
/// # Safety
/// Supply a live exclusive main-thread handle, valid UTF-8 C string and writable
/// length pointer. Free returned bytes with kr_bytes_free and the original length.
#[no_mangle]
pub unsafe extern "C" fn kr_ui_asset(
    handle: *mut Host,
    name: *const c_char,
    length: *mut usize,
) -> *mut u8 {
    run(|| {
        if length.is_null() {
            return Err("null UI asset length pointer".into());
        }
        *length = 0;
        let h = host(handle)?;
        let name = string(name)?;
        if !matches!(
            name.as_str(),
            "ui/title/TI_base_0.webp" | "ui/title/TI_タイトルロゴ.webp"
        ) {
            return Err("unsupported UI asset name".into());
        }
        let mut bytes = h
            .engine
            .assets
            .bytes_limited(&name, 8 * 1024 * 1024)
            .map_err(|e| e.to_string())?
            .into_boxed_slice();
        *length = bytes.len();
        let pointer = bytes.as_mut_ptr();
        std::mem::forget(bytes);
        Ok(pointer)
    })
}
/// Independent of handles; may run on an audio worker thread.
/// # Safety
/// Bytes must reference length readable bytes; output_length must be writable.
#[no_mangle]
pub unsafe extern "C" fn kr_decode_audio(
    bytes: *const u8,
    length: usize,
    output_length: *mut usize,
) -> *mut u8 {
    if output_length.is_null() {
        return std::ptr::null_mut();
    }
    *output_length = 0;
    run(|| {
        if bytes.is_null() || length > 128 * 1024 * 1024 {
            return Err("invalid audio buffer".into());
        }
        let mut wave = media::wav(std::slice::from_raw_parts(bytes, length))?.into_boxed_slice();
        *output_length = wave.len();
        let pointer = wave.as_mut_ptr();
        std::mem::forget(wave);
        Ok(pointer)
    })
}
/// # Safety
/// Pass null or a bridge byte allocation with its original length, exactly once.
#[no_mangle]
pub unsafe extern "C" fn kr_bytes_free(pointer: *mut u8, length: usize) {
    if !pointer.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            pointer, length,
        )));
    }
}
