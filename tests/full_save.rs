use koikari_reborn::{model::*, save};
use serde_json::json;
mod common;

#[test]
fn pending_page_autosave_preserves_same_numbered_committed_checkpoint() {
    use koikari_reborn::{story::Engine, xp3::Store};
    use std::sync::Arc;
    let assets = common::game("First[c]\n[wait time=200 canskip=false]Second[c]");
    let store = Arc::new(Store::open(assets.path()).unwrap());
    let mut engine = Engine::open(store.clone()).unwrap();
    engine.start().unwrap();
    let first = engine.state.clone();
    engine.advance().unwrap();
    engine.tick(50).unwrap();
    assert_eq!(engine.state.pages, first.pages);
    assert!(engine.state.flow.page_open);
    assert_eq!(engine.state.flow.wait_ms, 150);
    let window: Vec<_> = engine.previous_pages().cloned().collect();
    assert_eq!(window.last(), Some(&first));
    let saves = tempfile::tempdir().unwrap();
    save::write_with_rollback(saves.path(), 9, &store.fingerprint, &engine.state, &window).unwrap();
    let record = save::read_record(saves.path(), 9, &store.fingerprint).unwrap();
    let mut restored = Engine::open(store).unwrap();
    restored.restore(record.state).unwrap();
    restored.restore_checkpoints(record.rollback).unwrap();
    assert_eq!(restored.state, engine.state);
    assert_eq!(restored.previous_pages().last(), Some(&first));
    engine.tick(150).unwrap();
    restored.tick(150).unwrap();
    assert_eq!(restored.state, engine.state);
    restored.rewind(first.pages).unwrap();
    assert_eq!(restored.state, first);
}

#[test]
fn rollback_order_validation_still_rejects_uncommitted_equal_future_and_duplicate_pages() {
    use koikari_reborn::{story::Engine, xp3::Store};
    use std::sync::Arc;
    let assets = common::game("First[c]\n[wait time=200]Second[c]");
    let mut engine = Engine::open(Arc::new(Store::open(assets.path()).unwrap())).unwrap();
    engine.start().unwrap();
    let first = engine.state.clone();
    assert!(engine.restore_checkpoints(vec![first.clone()]).is_err());
    engine.advance().unwrap();
    let window: Vec<_> = engine.previous_pages().cloned().collect();
    let current = engine.state.clone();
    let mut future = first.clone();
    future.pages += 1;
    let mut title = first.clone();
    title.title = true;
    for invalid in [
        vec![current.clone()],
        vec![future],
        vec![first.clone(), first],
        vec![title],
        vec![window[0].clone(); 101],
    ] {
        assert!(engine.restore_checkpoints(invalid).is_err());
        assert_eq!(engine.state, current);
        assert_eq!(engine.previous_pages().cloned().collect::<Vec<_>>(), window);
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn ffi_pending_page_autosave_loads_after_host_restart_without_mutating_slot() {
    use koikari_reborn::ffi::*;
    use std::ffi::{CStr, CString};
    let assets = common::game("First[c]\n[wait time=200 canskip=false]Second[c]");
    let saves = tempfile::tempdir().unwrap();
    let resource = CString::new(assets.path().to_str().unwrap()).unwrap();
    let storage = CString::new(saves.path().to_str().unwrap()).unwrap();
    unsafe {
        let snapshot = |host| {
            let text = kr_snapshot(host);
            assert!(!text.is_null());
            let value: serde_json::Value =
                serde_json::from_str(CStr::from_ptr(text).to_str().unwrap()).unwrap();
            kr_string_free(text);
            value
        };
        let host = kr_open(resource.as_ptr(), storage.as_ptr());
        assert!(!host.is_null());
        for command in ["new", "advance", "save:9"] {
            assert_eq!(kr_action(host, CString::new(command).unwrap().as_ptr()), 1);
        }
        let before = snapshot(host);
        assert_eq!(before["pages"], 1);
        assert_eq!(before["flow"]["wait_ms"], 200);
        let bytes = std::fs::read(saves.path().join("slot-9.json")).unwrap();
        kr_close(host);
        let fresh = kr_open(resource.as_ptr(), storage.as_ptr());
        assert!(!fresh.is_null());
        assert_eq!(kr_has_save(fresh), 1);
        assert_eq!(
            kr_action(fresh, CString::new("load:9").unwrap().as_ptr()),
            1
        );
        assert_eq!(snapshot(fresh), before);
        assert_eq!(kr_tick(fresh, 200), 2);
        assert_eq!(snapshot(fresh)["pages"], 2);
        assert_eq!(
            std::fs::read(saves.path().join("slot-9.json")).unwrap(),
            bytes
        );
        kr_close(fresh);
    }
}

#[test]
fn full_state_roundtrip_retains_pending_input_and_media_clocks() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = State {
        title: false,
        pages: 7,
        ..State::default()
    };
    state.cursor = Cursor {
        script: "synthetic.ks".into(),
        token: 42,
    };
    state.flow.event_id = Some(12);
    state.flow.selection_id = Some(3);
    state
        .flow
        .variables
        .insert("f.route".into(), json!([1, 2, 3]));
    state.flow.choices = vec![
        Choice {
            id: 9,
            text: "Synthetic A".into(),
            enabled: true,
        },
        Choice {
            id: 11,
            text: "Synthetic B".into(),
            enabled: false,
        },
    ];
    state.flow.wait_ms = 700;
    state.flow.replay = true;
    state.flow.visited.insert("synthetic-event".into());
    state.scene.video = Some(Video {
        asset: "synthetic.mp4".into(),
        sequence: 78,
        skippable: false,
        looping: false,
    });
    state.scene.visual.sequence = 11;
    state.scene.visual.elapsed_ms = 127;
    state.scene.visual.duration_ms = 600;
    state.scene.visual.previous.push(Layer {
        asset: "synthetic-before.webp".into(),
        x: 0.0,
        y: 0.0,
        width: 2560.0,
        height: 1440.0,
        opacity: 1.0,
        group: 0,
    });
    state.scene.visual.commands.push(VisualCommand {
        name: "synthetic".into(),
        args: Default::default(),
    });
    state.scene.bgm = Audio {
        asset: "synthetic.ogg".into(),
        missing: false,
        sequence: 89,
        looping: true,
        volume: Some(0.3),
        fade_ms: 900,
    };
    state.progress.cg.insert("synthetic-cg".into());
    save::write(dir.path(), 6, "resources-a", &state).unwrap();
    assert_eq!(save::read(dir.path(), 6, "resources-a").unwrap(), state);
    assert!(save::read(dir.path(), 6, "resources-b").is_err());
}

#[test]
fn legacy_migration_preserves_original_bytes_and_defaults_only_additive_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = serde_json::to_value(State::default()).unwrap();
    for key in ["flow", "progress"] {
        value.as_object_mut().unwrap().remove(key);
    }
    for key in ["video", "visual"] {
        value["scene"].as_object_mut().unwrap().remove(key);
    }
    let bytes = serde_json::to_vec(&json!({"version":1,"assets":"a","state":value})).unwrap();
    let path = dir.path().join("slot-1.json");
    std::fs::write(&path, &bytes).unwrap();
    let loaded = save::read(dir.path(), 1, "a").unwrap();
    assert_eq!(loaded.flow, FlowState::default());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn rollback_unions_real_unlocks_without_resetting_current_globals() {
    let mut current = Progress::default();
    current.endings.insert("ending-a".into());
    current
        .variables
        .insert("sf.route_seen".into(), json!(true));
    let mut old = Progress::default();
    old.cg.insert("cg-b".into());
    old.variables.insert("sf.route_seen".into(), json!(false));
    old.variables.insert("sf.other".into(), json!(2));
    save::merge_progress(&mut current, &old);
    assert!(current.endings.contains("ending-a") && current.cg.contains("cg-b"));
    assert_eq!(current.variables["sf.route_seen"], json!(true));
    assert_eq!(current.variables["sf.other"], json!(2));
    assert_eq!(current.events.len(), 0);
}

#[test]
fn global_progress_is_fingerprint_bound_atomic_and_corruption_is_visible() {
    let dir = tempfile::tempdir().unwrap();
    let mut progress = Progress::default();
    progress.events.insert("event-a".into());
    save::write_progress(dir.path(), "a", &progress).unwrap();
    assert_eq!(save::read_progress(dir.path(), "a").unwrap(), progress);
    assert_eq!(
        save::read_progress(dir.path(), "b").unwrap(),
        Progress::default()
    );
    let mut other = Progress::default();
    other.cg.insert("cg-b".into());
    let merged = save::write_progress(dir.path(), "a", &other).unwrap();
    assert!(merged.events.contains("event-a") && merged.cg.contains("cg-b"));
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    std::fs::write(&files[0], b"broken").unwrap();
    assert!(save::write_progress(dir.path(), "a", &progress).is_err());
    assert_eq!(std::fs::read(&files[0]).unwrap(), b"broken");
}

#[test]
fn invalid_runtime_is_rejected_without_overwriting_manual_slot() {
    let dir = tempfile::tempdir().unwrap();
    let good = State::default();
    save::write(dir.path(), 1, "a", &good).unwrap();
    let mut bad = good.clone();
    bad.flow.choices = vec![Choice::default(), Choice::default()];
    assert!(save::write(dir.path(), 1, "a", &bad).is_err());
    bad = good.clone();
    bad.scene.bgm.volume = Some(f32::NAN);
    assert!(save::write(dir.path(), 1, "a", &bad).is_err());
    assert!(save::write(dir.path(), 7, "a", &good).is_err());
    assert_eq!(save::read(dir.path(), 1, "a").unwrap(), good);
}

#[test]
fn full_state_envelope_cannot_silently_default_missing_execution_data() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = serde_json::to_value(State::default()).unwrap();
    value.as_object_mut().unwrap().remove("flow");
    std::fs::write(
        dir.path().join("slot-1.json"),
        serde_json::to_vec(&json!({"version":2,"assets":"a","state":value})).unwrap(),
    )
    .unwrap();
    assert!(save::read(dir.path(), 1, "a")
        .unwrap_err()
        .contains("missing execution"));
}

#[cfg(unix)]
#[test]
fn atomic_save_refuses_symlink_and_leaves_external_file_intact() {
    let dir = tempfile::tempdir().unwrap();
    let external = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(external.path(), b"original savedata").unwrap();
    std::os::unix::fs::symlink(external.path(), dir.path().join("slot-1.json")).unwrap();
    assert!(save::write(dir.path(), 1, "a", &State::default()).is_err());
    assert_eq!(
        std::fs::read(external.path()).unwrap(),
        b"original savedata"
    );
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn ffi_contains_null_handles_invalid_clock_and_decode_buffers() {
    use koikari_reborn::ffi::*;
    unsafe {
        assert_eq!(kr_tick(std::ptr::null_mut(), 5), 0);
        let message = kr_error();
        assert!(!message.is_null());
        kr_string_free(message);
        assert!(kr_gallery(std::ptr::null_mut()).is_null());
        assert!(kr_video(std::ptr::null_mut()).is_null());
        let mut count = 7;
        assert!(kr_decode_audio(std::ptr::null(), 4, &mut count).is_null());
        assert_eq!(count, 0);
        kr_close(std::ptr::null_mut());
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn ffi_rejects_resource_save_directory_and_guards_pending_input_commands() {
    use koikari_reborn::ffi::*;
    use std::ffi::CString;
    let assets = common::game("Synthetic page[p]");
    let saves = tempfile::tempdir().unwrap();
    let resource = CString::new(assets.path().to_str().unwrap()).unwrap();
    let storage = CString::new(saves.path().join("Saves").to_str().unwrap()).unwrap();
    unsafe {
        assert!(kr_open(resource.as_ptr(), resource.as_ptr()).is_null());
        assert!(!assets.path().join("slot-1.json").exists());
        let host = kr_open(resource.as_ptr(), storage.as_ptr());
        assert!(!host.is_null());
        for command in [
            "choose:9",
            "choose:bogus",
            "video-complete:0",
            "advance",
            "save:7",
            "new:extra",
        ] {
            let command = CString::new(command).unwrap();
            assert_eq!(kr_action(host, command.as_ptr()), 0);
        }
        assert_eq!(kr_tick(host, 1001), 0);
        let command = CString::new("new").unwrap();
        assert_eq!(kr_action(host, command.as_ptr()), 1);
        kr_close(host);
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn ffi_new_and_title_keep_real_global_unlocks_without_unlocking_other_content() {
    use koikari_reborn::{ffi::*, xp3::Store};
    use std::ffi::{CStr, CString};
    let assets = common::game("Synthetic page[p]");
    let saves = tempfile::tempdir().unwrap();
    let fingerprint = Store::open(assets.path()).unwrap().fingerprint;
    let mut progress = Progress::default();
    progress.endings.insert("synthetic-ending".into());
    progress
        .variables
        .insert("sf.synthetic".into(), json!(true));
    save::write_progress(saves.path(), &fingerprint, &progress).unwrap();
    let resource = CString::new(assets.path().to_str().unwrap()).unwrap();
    let storage = CString::new(saves.path().to_str().unwrap()).unwrap();
    unsafe {
        let host = kr_open(resource.as_ptr(), storage.as_ptr());
        assert!(!host.is_null());
        for command in ["new", "title"] {
            let command = CString::new(command).unwrap();
            assert_eq!(kr_action(host, command.as_ptr()), 1);
            let snapshot = kr_snapshot(host);
            assert!(!snapshot.is_null());
            let value: serde_json::Value =
                serde_json::from_str(CStr::from_ptr(snapshot).to_str().unwrap()).unwrap();
            kr_string_free(snapshot);
            assert_eq!(value["progress"]["endings"], json!(["synthetic-ending"]));
            assert_eq!(value["progress"]["cg"], json!([]));
            assert_eq!(value["progress"]["variables"]["sf.synthetic"], json!(true));
        }
        kr_close(host);
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn ffi_load_never_accepts_silent_loss_of_full_state_or_mutates_live_game_on_failure() {
    use koikari_reborn::{ffi::*, story::Engine, xp3::Store};
    use std::{
        ffi::{CStr, CString},
        sync::Arc,
    };
    let assets = common::game("Synthetic first[p]Synthetic second[p]");
    let saves = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(assets.path()).unwrap());
    let mut engine = Engine::open(store.clone()).unwrap();
    engine.start().unwrap();
    let mut saved = engine.state.clone();
    saved
        .flow
        .variables
        .insert("f.synthetic_retained".into(), json!(17));
    save::write(saves.path(), 1, &store.fingerprint, &saved).unwrap();
    let resource = CString::new(assets.path().to_str().unwrap()).unwrap();
    let storage = CString::new(saves.path().to_str().unwrap()).unwrap();
    unsafe {
        let host = kr_open(resource.as_ptr(), storage.as_ptr());
        assert!(!host.is_null());
        let command = CString::new("load:1").unwrap();
        let result = kr_action(host, command.as_ptr());
        if result == 0 {
            let error = kr_error();
            assert!(!CStr::from_ptr(error).to_bytes().is_empty());
            kr_string_free(error);
        }
        let snapshot = kr_snapshot(host);
        assert!(!snapshot.is_null());
        let value: serde_json::Value =
            serde_json::from_str(CStr::from_ptr(snapshot).to_str().unwrap()).unwrap();
        kr_string_free(snapshot);
        if result == 1 {
            assert_eq!(
                value["flow"]["variables"]["f.synthetic_retained"],
                json!(17)
            );
        } else {
            assert_eq!(value["title"], json!(true));
        }
        kr_close(host);
    }
    assert_eq!(
        save::read(saves.path(), 1, &store.fingerprint).unwrap(),
        saved
    );
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn ffi_migrates_only_reachable_legacy_prefix_and_never_rewrites_manual_slots() {
    use koikari_reborn::{ffi::*, story::Engine, xp3::Store};
    use std::{
        ffi::{CStr, CString},
        sync::Arc,
    };
    let assets = common::game("Synthetic first[p]Synthetic second[p]");
    let saves = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(assets.path()).unwrap());
    let mut engine = Engine::open(store.clone()).unwrap();
    engine.start().unwrap();
    engine.advance().unwrap();
    let mut legacy = serde_json::to_value(&engine.state).unwrap();
    legacy.as_object_mut().unwrap().remove("flow");
    legacy.as_object_mut().unwrap().remove("progress");
    let bytes = serde_json::to_vec(&json!({"version":1,"assets":store.fingerprint,"state":legacy}))
        .unwrap();
    std::fs::write(saves.path().join("slot-1.json"), &bytes).unwrap();
    legacy["cursor"]["token"] = json!(usize::MAX);
    std::fs::write(
        saves.path().join("slot-2.json"),
        serde_json::to_vec(&json!({"version":1,"assets":store.fingerprint,"state":legacy}))
            .unwrap(),
    )
    .unwrap();
    let resource = CString::new(assets.path().to_str().unwrap()).unwrap();
    let storage = CString::new(saves.path().to_str().unwrap()).unwrap();
    unsafe {
        let host = kr_open(resource.as_ptr(), storage.as_ptr());
        assert!(!host.is_null());
        let command = CString::new("load:1").unwrap();
        assert_eq!(kr_action(host, command.as_ptr()), 1);
        let before = kr_snapshot(host);
        assert!(!before.is_null());
        let before: serde_json::Value = {
            let value = serde_json::from_str(CStr::from_ptr(before).to_str().unwrap()).unwrap();
            kr_string_free(before);
            value
        };
        let command = CString::new("load:2").unwrap();
        assert_eq!(kr_action(host, command.as_ptr()), 0);
        let after = kr_snapshot(host);
        assert!(!after.is_null());
        let after: serde_json::Value = {
            let value = serde_json::from_str(CStr::from_ptr(after).to_str().unwrap()).unwrap();
            kr_string_free(after);
            value
        };
        assert_eq!(before, after);
        kr_close(host);
    }
    assert_eq!(
        std::fs::read(saves.path().join("slot-1.json")).unwrap(),
        bytes
    );
}
