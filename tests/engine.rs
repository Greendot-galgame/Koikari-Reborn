mod common;
use koikari_reborn::{
    story::Engine,
    xp3::{Assets, Store},
};
use std::sync::Arc;

#[test]
fn unicode_plain_and_compressed_multisegment_roundtrip() {
    for packed in [false, true] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("data.xp3"),
            common::archive(
                &[("目录/样本.txt", b"synthetic body")],
                packed,
                packed,
                0,
                false,
            ),
        )
        .unwrap();
        let store = Store::open(d.path()).unwrap();
        assert_eq!(store.bytes("目录\\样本.txt").unwrap(), b"synthetic body");
    }
}
#[test]
fn patch_overrides_main() {
    let d = tempfile::tempdir().unwrap();
    for (file, text) in [("data.xp3", b"old"), ("patch.xp3", b"new")] {
        std::fs::write(
            d.path().join(file),
            common::archive(&[("a.txt", text)], true, true, 0, false),
        )
        .unwrap();
    }
    assert_eq!(
        Store::open(d.path()).unwrap().bytes("a.txt").unwrap(),
        b"new"
    );
}
#[test]
fn supplemental_and_numbered_patches_follow_numeric_priority_and_change_fingerprint() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("data.xp3"),
        common::archive(&[("same.txt", b"base")], true, true, 0, false),
    )
    .unwrap();
    let baseline = Store::open(d.path()).unwrap().fingerprint;
    for (name, value) in [
        ("adult.xp3", b"supplement".as_slice()),
        ("patch.xp3", b"patch"),
        ("patch2.xp3", b"two"),
        ("patch10.xp3", b"ten"),
        ("patch50.xp3", b"fifty"),
    ] {
        std::fs::write(
            d.path().join(name),
            common::archive(&[("same.txt", value)], true, true, 0, false),
        )
        .unwrap();
        assert_eq!(
            Store::open(d.path()).unwrap().bytes("same.txt").unwrap(),
            value
        );
    }
    let store = Store::open(d.path()).unwrap();
    assert_eq!(
        store.archive_names(),
        [
            "data.xp3",
            "adult.xp3",
            "patch.xp3",
            "patch2.xp3",
            "patch10.xp3",
            "patch50.xp3"
        ]
    );
    assert_ne!(store.fingerprint, baseline);
    std::fs::remove_file(d.path().join("adult.xp3")).unwrap();
    assert_ne!(
        Store::open(d.path()).unwrap().fingerprint,
        store.fingerprint
    );
    for name in ["patch.xp3", "patch2.xp3", "patch10.xp3", "patch50.xp3"] {
        std::fs::remove_file(d.path().join(name)).unwrap();
    }
    assert_eq!(Store::open(d.path()).unwrap().fingerprint, baseline);
}
#[test]
fn empty_entries_require_a_segment_table_and_valid_checksum() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("data.xp3");
    let bytes = common::archive(&[("marker.tjs", b"")], false, false, 0, false);
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        Store::open(d.path()).unwrap().text("marker.tjs").unwrap(),
        ""
    );
    let mut missing_table = bytes.clone();
    let at = missing_table.windows(4).position(|v| v == b"segm").unwrap();
    missing_table[at..at + 4].copy_from_slice(b"skip");
    std::fs::write(&path, missing_table).unwrap();
    assert!(Store::open(d.path()).is_err());
    let mut wrong_size = bytes;
    let at = wrong_size.windows(4).position(|v| v == b"info").unwrap() + 16;
    wrong_size[at..at + 8].copy_from_slice(&1u64.to_le_bytes());
    std::fs::write(&path, wrong_size).unwrap();
    assert!(Store::open(d.path()).is_err());
    for (delta, protected) in [(1, false), (0, true)] {
        std::fs::write(
            &path,
            common::archive(&[("marker.tjs", b"")], true, true, delta, protected),
        )
        .unwrap();
        assert!(Store::open(d.path()).unwrap().bytes("marker.tjs").is_err());
    }
}
#[test]
fn invalid_supplements_never_fall_back_to_the_base() {
    let d = common::game("Preview[c]");
    std::fs::write(d.path().join("adult.xp3"), b"not an archive").unwrap();
    assert!(Store::open(d.path()).is_err());
    std::fs::write(
        d.path().join("adult.xp3"),
        common::archive(&[("intro.ks", b"replacement")], true, true, 0, true),
    )
    .unwrap();
    assert!(Store::open(d.path()).unwrap().text("intro.ks").is_err());
}
#[test]
fn checksum_and_protection_fail_closed() {
    for (delta, protected) in [(1, false), (0, true)] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("data.xp3"),
            common::archive(&[("a", b"body")], true, true, delta, protected),
        )
        .unwrap();
        assert!(Store::open(d.path()).unwrap().bytes("a").is_err());
    }
}
#[test]
fn unsafe_duplicate_and_truncated_archives_rejected() {
    for files in [
        vec![("../a", b"body".as_slice())],
        vec![("A", b"body".as_slice()), ("a", b"body".as_slice())],
    ] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("data.xp3"),
            common::archive(&files, true, false, 0, false),
        )
        .unwrap();
        assert!(Store::open(d.path()).is_err());
    }
    let bytes = common::archive(&[("a", b"body")], true, true, 0, false);
    let d = tempfile::tempdir().unwrap();
    for end in [0, 10, 18, bytes.len() - 2] {
        std::fs::write(d.path().join("data.xp3"), &bytes[..end]).unwrap();
        assert!(Store::open(d.path()).is_err());
    }
}
#[test]
fn pages_and_literal_labels_preserve_dialogue() {
    let d = common::game(
        "*start\n[Actor]Hello[c]\n@jump target=*end\n[Actor]Skipped[c]\n*end\n[地]World[c]\n@SC_EndProcess",
    );
    let mut game = Engine::open(Arc::new(Store::open(d.path()).unwrap())).unwrap();
    game.start().unwrap();
    assert_eq!(game.state.scene.text, "Hello");
    assert_eq!(game.state.scene.speaker, "Character");
    game.advance().unwrap();
    assert_eq!(game.state.scene.text, "World");
    game.advance().unwrap();
    assert!(game.state.finished);
}
#[test]
fn unknown_command_and_tjs_do_not_get_silently_skipped() {
    for tag in [
        "unsupported",
        "未知命令",
        "eval exp=\"System.exit()\"",
        "jump storage=\"&f.next\"",
    ] {
        let d = common::game(&format!("[地]Before[c]\n@{tag}\n[地]After[c]"));
        let mut game = Engine::open(Arc::new(Store::open(d.path()).unwrap())).unwrap();
        game.start().unwrap();
        assert!(game.advance().is_err());
        assert!(game.state.blocked.as_ref().unwrap().contains("intro.ks:2"));
        assert_ne!(game.state.scene.text, "After");
    }
}

#[test]
fn invalid_audio_and_non_story_states_are_rejected_before_restore() {
    let directory = common::game("[地]Before[c]\n[地]After[c]");
    let mut engine = Engine::open(Arc::new(Store::open(directory.path()).unwrap())).unwrap();
    assert!(engine.validate_state(&engine.state.clone()).is_err());
    engine.start().unwrap();
    let mut state = engine.state.clone();
    engine.validate_state(&state).unwrap();
    state.scene.voice.asset = "missing.opus".into();
    assert!(engine.validate_state(&state).is_err());
    state = engine.state.clone();
    state.blocked = Some("unsupported command".into());
    assert!(engine.validate_state(&state).is_err());
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn bridge_lists_slots_and_continues_the_latest_valid_save_without_losing_current_state() {
    use koikari_reborn::ffi::*;
    use std::ffi::{CStr, CString};
    let assets = common::game("[地]First[c]\n[地]Second[c]");
    let saves = tempfile::tempdir().unwrap();
    let path = CString::new(assets.path().to_str().unwrap()).unwrap();
    let directory = CString::new(saves.path().to_str().unwrap()).unwrap();
    unsafe {
        let host = kr_open(path.as_ptr(), directory.as_ptr());
        assert!(!host.is_null());
        let action = |name| kr_action(host, CString::new(name).unwrap().as_ptr());
        assert_eq!(kr_has_save(host), 0);
        assert_eq!(action("new"), 1);
        assert_eq!(action("save:3"), 1);
        assert_eq!(action("advance"), 1);
        assert_eq!(action("save:9"), 1);
        assert_eq!(action("title"), 1);
        assert_eq!(action("continue"), 1);
        let json = kr_snapshot(host);
        let state: serde_json::Value =
            serde_json::from_slice(CStr::from_ptr(json).to_bytes()).unwrap();
        kr_string_free(json);
        assert_eq!(state["scene"]["text"], "Second");
        assert_eq!(state["archives"], serde_json::json!(["data.xp3"]));
        std::fs::write(saves.path().join("slot-9.json"), b"broken").unwrap();
        assert_eq!(action("load:9"), 0);
        let json = kr_snapshot(host);
        assert!(CStr::from_ptr(json).to_str().unwrap().contains("Second"));
        kr_string_free(json);
        assert_eq!(action("continue"), 1);
        let json = kr_saves(host);
        let slots: Vec<serde_json::Value> =
            serde_json::from_slice(CStr::from_ptr(json).to_bytes()).unwrap();
        kr_string_free(json);
        assert_eq!(slots.len(), 7);
        assert!(slots
            .iter()
            .any(|s| s["slot"] == 9 && s["compatible"] == false));
        assert_eq!(action("save:../"), 0);
        assert_eq!(action("advance"), 1);
        let json = kr_timeline(host);
        let timeline: Vec<serde_json::Value> =
            serde_json::from_slice(CStr::from_ptr(json).to_bytes()).unwrap();
        kr_string_free(json);
        assert_eq!(timeline.last().unwrap()["page"], 1);
        assert_eq!(action("back"), 1);
        assert_eq!(action("rewind:99"), 0);
        assert_eq!(action("new:9"), 0);
        kr_close(host);
    }
}

fn sprite_game(script: &str) -> tempfile::TempDir {
    let directory = common::game(script);
    let mut metadata = String::from("STI_HAllPos[2] = 0;\nSTI_HAllPos[3] = 0;\n");
    for character in [6, 7] {
        for table in ["AX", "AY"] {
            metadata.push_str(&format!(
                "STI_{table}[{character}] = [[0,0],[0,0],[0,0],[0,0]];\n"
            ));
        }
        metadata.push_str(&format!("STI_AH[{character}] = [0,0,0,0];\n"));
        for table in ["FCAX", "FCAY"] {
            metadata.push_str(&format!("STI_{table}[{character}] = [0,0];\n"));
        }
    }
    let mut image = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(2, 2)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let mut files = vec![("Init_VariableSTAND.ks", metadata.as_bytes())];
    for name in [
        "bgA.webp",
        "bgB.webp",
        "stb6_s2_1_1.webp",
        "st6_s2_1_1.webp",
        "stb6_s3_1_1.webp",
        "st6_s3_1_1.webp",
        "stb7_s2_1_1.webp",
        "st7_s2_1_1.webp",
        "stb7_s3_1_1.webp",
        "st7_s3_1_1.webp",
    ] {
        files.push((name, image.get_ref().as_slice()));
    }
    std::fs::write(
        directory.path().join("patch.xp3"),
        common::archive(&files, true, true, 0, false),
    )
    .unwrap();
    directory
}

#[test]
fn scene_changes_clear_sprites_but_background_updates_preserve_them() {
    for change in [
        "bgset bg=bgB",
        "bgset_withoutblack bg=bgB",
        "bgset_interval bg2=bgB",
    ] {
        let directory = sprite_game(&format!(
            "@bg_image num=bgA\n@stc ch=6 pl f=111\n[地]Stage[c]\n@{change}\n[地]Scene[c]\n@stc ch=7 pr f=111\n@bg_image num=bgA\n[地]Update[c]"
        ));
        let mut engine = Engine::open(Arc::new(Store::open(directory.path()).unwrap())).unwrap();
        engine.start().unwrap();
        assert_eq!(engine.state.sprites.len(), 1);
        engine.advance().unwrap();
        assert!(engine.state.sprites.is_empty(), "{change}");
        assert!(engine.state.scene.layers.iter().all(|l| l.group == 0));
        engine.advance().unwrap();
        assert_eq!(engine.state.sprites.len(), 1);
        assert!(engine.state.sprites.contains_key(&7));
        assert!(engine.state.scene.layers.iter().any(|l| l.group == 7));
    }
}

#[test]
fn dialogue_continue_stops_old_motions_but_raw_line_wait_preserves_them() {
    use koikari_reborn::animation;
    for (delimiter, stops) in [("c", true), ("p", false), ("l", false)] {
        let directory = sprite_game(&format!(
            "@stc ch=6 f=111\n@t_move lay=6 ppe=true\nFirst[{delimiter}]\nSecond[c]\nThird[c]"
        ));
        let mut engine = Engine::open(Arc::new(Store::open(directory.path()).unwrap())).unwrap();
        engine.start().unwrap();
        while engine.state.flow.wait_ms > 0 {
            engine.tick(engine.state.flow.wait_ms).unwrap();
        }
        engine.tick(7).unwrap();
        assert_ne!(
            animation::layers(&engine.state.scene),
            engine.state.scene.layers
        );
        let saved = engine.state.clone();
        engine.restore(saved.clone()).unwrap();
        assert_eq!(engine.state, saved);
        engine.advance().unwrap();
        let second = engine.state.clone();
        engine.tick(19).unwrap();
        assert_eq!(
            animation::layers(&engine.state.scene) == second.scene.layers,
            stops
        );
        engine.advance().unwrap();
        engine.tick(123).unwrap();
        assert_eq!(
            animation::layers(&engine.state.scene),
            engine.state.scene.layers
        );
        engine.rewind(second.pages).unwrap();
        engine.advance().unwrap();
        engine.tick(13).unwrap();
        assert_eq!(
            animation::layers(&engine.state.scene),
            engine.state.scene.layers
        );
    }
}

#[test]
fn face_mode_overrides_stage_position_and_replaces_the_previous_portrait_only() {
    let directory = sprite_game(
        "@stc ch=7 pr f=111\n@stc ch=6 pc fc f=111\n[地]First[c]\n@stc ch=7 pr f=111 fcenabled=true\n[地]Second[c]"
    );
    let mut engine = Engine::open(Arc::new(Store::open(directory.path()).unwrap())).unwrap();
    engine.start().unwrap();
    let face = &engine.state.sprites[&6];
    assert_eq!(face.position, "fc");
    assert_eq!(face.size, 3);
    assert_eq!(engine.state.sprites[&7].position, "pr");
    for layer in engine.state.scene.layers.iter().filter(|l| l.group == 6) {
        assert_eq!((layer.x, layer.y), (-180.0, 960.0));
        assert!(layer.asset.contains("_s3_"));
    }
    engine.advance().unwrap();
    assert_eq!(engine.state.sprites.len(), 1);
    assert_eq!(engine.state.sprites[&7].position, "fc");
    assert!(engine
        .state
        .scene
        .layers
        .iter()
        .all(|l| l.group == 0 || l.group == 7));
}

#[test]
fn hiding_the_message_clears_sprites_unless_standhide_is_set() {
    for preserve in [false, true] {
        let directory = sprite_game(&format!(
            "@stc ch=6 pc f=111\n[地]Before[c]\n@MS_Hide{}\n[地]Hidden[c]\n@MS_Show\n[地]Visible[c]",
            if preserve { " standhide" } else { "" }
        ));
        let mut engine = Engine::open(Arc::new(Store::open(directory.path()).unwrap())).unwrap();
        engine.start().unwrap();
        engine.advance().unwrap();
        assert!(!engine.state.scene.message_visible);
        assert_eq!(engine.state.sprites.contains_key(&6), preserve);
        engine.advance().unwrap();
        assert!(engine.state.scene.message_visible);
    }
}

#[test]
fn introduction_is_a_resumable_frame_and_switches_the_following_background() {
    let directory =
        sprite_game("@bg_image num=bgA\n[地]Before[c]\n@eyecatch sp=bgB bg=bgA\n[地]After[c]");
    let assets = Arc::new(Store::open(directory.path()).unwrap());
    let mut engine = Engine::open(assets.clone()).unwrap();
    engine.start().unwrap();
    let before = engine.state.clone();
    engine.advance().unwrap();
    let intro = engine.state.clone();
    assert!(intro.interlude.is_some());
    assert!(!intro.scene.message_visible);
    assert!(intro.scene.layers[0].asset.ends_with("bgB.webp"));
    engine.advance().unwrap();
    let after = engine.state.clone();
    assert!(after.scene.message_visible);
    assert!(after.scene.layers[0].asset.ends_with("bgA.webp"));
    engine.rewind(intro.pages).unwrap();
    assert_eq!(engine.state, intro);
    engine.advance().unwrap();
    assert_eq!(engine.state, after);
    let mut restored = Engine::open(assets).unwrap();
    let window = engine
        .previous_pages()
        .filter(|s| s.pages < intro.pages)
        .cloned()
        .collect();
    restored.restore(intro).unwrap();
    restored.restore_checkpoints(window).unwrap();
    restored.rewind(before.pages).unwrap();
    assert_eq!(restored.state, before);
}

#[test]
fn old_saved_visuals_are_rebuilt_and_a_failed_restore_does_not_replace_the_page() {
    let directory =
        sprite_game("@bg_image num=bgA\n[地]Before[c]\n@eyecatch sp=bgA bg=bgB\n[地]After[c]");
    let mut engine = Engine::open(Arc::new(Store::open(directory.path()).unwrap())).unwrap();
    engine.start().unwrap();
    engine.advance().unwrap();
    engine.advance().unwrap();
    let expected = engine.state.clone();
    let mut legacy = expected.clone();
    legacy.visual_revision = 0;
    legacy.pages -= 1;
    legacy.scene.layers[0].asset = "bgA.webp".into();
    engine.restore(legacy).unwrap();
    assert_eq!(engine.state, expected);
    let mut invalid = expected.clone();
    invalid.cursor.token = usize::MAX;
    assert!(engine.restore(invalid).is_err());
    assert_eq!(engine.state, expected);
}

#[test]
fn rollback_is_bounded_and_advancing_after_rewind_replays_the_same_page() {
    let directory = common::game(&"[地]Page[c]\n".repeat(130));
    let mut engine = Engine::open(Arc::new(Store::open(directory.path()).unwrap())).unwrap();
    engine.start().unwrap();
    for _ in 1..130 {
        engine.advance().unwrap();
    }
    let expected = engine.state.clone();
    assert_eq!(engine.previous_pages().count(), 100);
    assert!(engine.rewind(29).is_err());
    assert_eq!(engine.state, expected);
    engine.rewind(129).unwrap();
    engine.advance().unwrap();
    assert_eq!(engine.state, expected);
    engine.restore(expected.clone()).unwrap();
    engine.rewind(129).unwrap();
    engine.advance().unwrap();
    assert_eq!(engine.state, expected);
}
