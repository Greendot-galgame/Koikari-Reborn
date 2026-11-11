//! Main-thread-only C bridge. Handles must not be accessed concurrently.
use crate::{
    media,
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

pub struct Host {
    engine: Engine,
    saves: PathBuf,
    renderer: Option<Renderer>,
}
fn saves(h: &mut Host) -> Vec<save::SaveInfo> {
    let fingerprint = h.engine.assets.fingerprint.clone();
    save::catalog(&h.saves, &fingerprint, |state| {
        h.engine.validate_state(state)
    })
}
thread_local! { static ERROR:RefCell<String>=const{RefCell::new(String::new())}; }
fn run<T: Default>(f: impl FnOnce() -> Result<T, String>) -> T {
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
        Ok(Box::into_raw(Box::new(Host {
            engine: Engine::open(assets)?,
            saves: PathBuf::from(string(saves)?),
            renderer: None,
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
        if value.is_some() && !["save", "load", "rewind"].contains(&command) {
            return Err("unexpected action argument".into());
        }
        match command {
            "new" => h.engine.start()?,
            "advance" => h.engine.advance()?,
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
                save::write(
                    &h.saves,
                    slot,
                    &h.engine.assets.fingerprint,
                    &h.engine.state,
                )?;
            }
            "load" => {
                let state = save::read(&h.saves, slot, &h.engine.assets.fingerprint)?;
                h.engine.restore(state)?;
            }
            "continue" => {
                let mut entries = saves(h);
                let mut failure = "no compatible save available".to_owned();
                loop {
                    let slot = save::latest(&entries).ok_or(failure.clone())?;
                    match save::read(&h.saves, slot, &h.engine.assets.fingerprint)
                        .and_then(|state| h.engine.restore(state))
                    {
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
        if h.renderer.is_none() {
            h.renderer = Some(Renderer::new(layer)?);
        }
        h.renderer.as_mut().ok_or("renderer unavailable")?.draw(
            layer,
            &h.engine.state.scene,
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
/// Pass null or a kr_audio allocation with its original length, exactly once.
#[no_mangle]
pub unsafe extern "C" fn kr_bytes_free(pointer: *mut u8, length: usize) {
    if !pointer.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            pointer, length,
        )));
    }
}
