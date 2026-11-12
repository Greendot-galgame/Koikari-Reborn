//! Opt-in tests: assets remain outside the source tree and are never embedded.
use koikari_reborn::{
    media, save,
    story::Engine,
    xp3::{Assets, Store},
};
use std::sync::Arc;

fn store() -> Arc<Store> {
    Arc::new(
        Store::open(
            std::env::var("KOIKARI_ASSETS")
                .expect("set KOIKARI_ASSETS to a directory containing data.xp3"),
        )
        .unwrap(),
    )
}

#[test]
#[ignore = "requires user-owned XP3; archive integrity only, independent of codec validity"]
fn all_effective_entries_pass_strict_archive_integrity() {
    let store = store();
    let names = store.names();
    let mut bytes = 0u64;
    for name in &names {
        bytes += store.bytes(name).unwrap().len() as u64;
    }
    println!(
        "Strict effective XP3 integrity: {} entries, {bytes} uncompressed bytes",
        names.len()
    );
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
#[ignore = "requires user-owned XP3; validates the actual native message-layer ABI"]
fn message_glyph_uses_verified_base_and_sheet_geometry() {
    use koikari_reborn::ffi::*;
    use std::ffi::{CStr, CString};
    let directory = std::env::var("KOIKARI_ASSETS").unwrap();
    let saves = tempfile::tempdir().unwrap();
    unsafe {
        let handle = kr_open(
            CString::new(directory).unwrap().as_ptr(),
            CString::new(saves.path().to_str().unwrap())
                .unwrap()
                .as_ptr(),
        );
        assert!(!handle.is_null());
        let pointer = kr_message_glyph(handle);
        if pointer.is_null() {
            let error = kr_error();
            let message = CStr::from_ptr(error).to_string_lossy().into_owned();
            kr_string_free(error);
            panic!("{message}");
        }
        let value: serde_json::Value =
            serde_json::from_slice(CStr::from_ptr(pointer).to_bytes()).unwrap();
        kr_string_free(pointer);
        let frames = value["timeline"]["frames"].as_array().unwrap();
        assert!(!frames.is_empty());
        assert!(value["timeline"]["duration_ms"].as_u64().unwrap() > 0);
        for frame in frames {
            let clip = frame["clip"].as_array().unwrap();
            assert_eq!(clip.len(), 4);
            assert!(
                clip[0].as_u64().unwrap() + clip[2].as_u64().unwrap()
                    <= value["width"].as_u64().unwrap()
            );
            assert!(
                clip[1].as_u64().unwrap() + clip[3].as_u64().unwrap()
                    <= value["height"].as_u64().unwrap()
            );
        }
        println!(
            "verified glyph: {} frames, {} ms, {}x{} sheet, {}x{} cells",
            frames.len(),
            value["timeline"]["duration_ms"],
            value["width"],
            value["height"],
            frames[0]["clip"][2],
            frames[0]["clip"][3]
        );
        kr_close(handle);
    }
}

fn step(engine: &mut Engine) {
    if let Some(choice) = engine.state.flow.choices.iter().find(|c| c.enabled) {
        engine.choose(choice.id).unwrap();
    } else if let Some(video) = &engine.state.scene.video {
        engine.finish_video(video.sequence).unwrap();
    } else if engine.state.flow.wait_ms > 0 {
        engine.tick(engine.state.flow.wait_ms).unwrap();
    } else {
        engine.advance().unwrap();
    }
}

fn next_page(engine: &mut Engine) {
    let before = engine.state.pages;
    for _ in 0..1000 {
        step(engine);
        if engine.state.pages > before || engine.state.finished {
            return;
        }
    }
    panic!("page continuation exceeded budget");
}

#[test]
#[ignore = "requires user-owned XP3"]
fn opening_continues_into_the_next_database_event_without_false_ending() {
    let mut engine = Engine::open(store()).unwrap();
    engine.start().unwrap();
    let first = engine.state.flow.event_id;
    for _ in 0..30000 {
        if engine.state.flow.event_id != first {
            break;
        }
        assert!(!engine.state.finished);
        step(&mut engine);
    }
    assert_ne!(engine.state.flow.event_id, first);
    assert!(!engine.state.finished);
    assert!(engine.state.flow.ending.is_none());
    assert!(engine.state.pages > 600);
    assert!(engine.state.blocked.is_none());
    println!("Opening pages: {}", engine.state.pages);
}

#[test]
#[ignore = "requires user-owned XP3"]
fn introduction_image_and_following_classroom_match_the_original_macro() {
    let assets = store();
    let classroom = assets.find("bg3_0", &["webp", "png"]).unwrap();
    let mut engine = Engine::open(assets.clone()).unwrap();
    engine.start().unwrap();
    let first = engine.state.flow.event_id;
    let mut introductions = 0;
    for _ in 0..30000 {
        if engine.state.flow.event_id != first {
            break;
        }
        if let Some(interlude) = &engine.state.interlude {
            introductions += 1;
            assert!(!engine.state.scene.message_visible);
            assert!(engine.state.sprites.is_empty());
            let bytes = assets.bytes(&engine.state.scene.layers[0].asset).unwrap();
            assert!(image::load_from_memory(&bytes).is_ok());
            let background = interlude.background.clone();
            if introductions == 1 {
                assert!(engine.state.scene.layers[0]
                    .asset
                    .to_lowercase()
                    .contains("eye_c_1"));
                assert_eq!(background, classroom);
            }
            next_page(&mut engine);
            assert_eq!(engine.state.scene.layers[0].asset, background);
            assert!(engine.state.scene.message_visible);
            if introductions == 1 {
                next_page(&mut engine);
                next_page(&mut engine);
                assert_eq!(engine.state.scene.layers[0].asset, classroom);
                println!(
                    "Reported classroom regression now at page {}",
                    engine.state.pages
                );
            }
        } else {
            step(&mut engine);
        }
    }
    assert_eq!(introductions, 3);
}

#[test]
#[ignore = "requires user-owned XP3"]
fn opening_cg_clears_stage_sprites_and_displays_only_one_face_portrait() {
    let mut engine = Engine::open(store()).unwrap();
    engine.start().unwrap();
    let mut cg_pages = 0;
    for _ in 0..50 {
        if engine
            .state
            .scene
            .layers
            .iter()
            .any(|l| l.group == 0 && l.asset.contains("ev101_"))
        {
            if cg_pages == 0 {
                assert!(engine.state.sprites.is_empty());
            }
            cg_pages += 1;
            assert!(engine.state.sprites.len() <= 1);
            assert!(engine
                .state
                .sprites
                .values()
                .all(|s| s.position == "fc" && s.size == 3));
            assert!(engine
                .state
                .scene
                .layers
                .iter()
                .filter(|l| l.group != 0)
                .all(|l| l.y >= 960.0));
        }
        next_page(&mut engine);
    }
    assert!(cg_pages > 10);
}

#[test]
#[ignore = "requires user-owned XP3"]
fn every_opening_image_decodes_within_the_texture_budget() {
    use std::collections::BTreeSet;
    let assets = store();
    let mut engine = Engine::open(assets.clone()).unwrap();
    let mut images: BTreeSet<String> = engine
        .state
        .scene
        .layers
        .iter()
        .map(|l| l.asset.clone())
        .collect();
    engine.start().unwrap();
    let first = engine.state.flow.event_id;
    let mut maximum = 0u64;
    for _ in 0..30000 {
        if engine.state.flow.event_id != first {
            break;
        }
        let mut unique = BTreeSet::new();
        let mut total = 0u64;
        for layer in engine
            .state
            .scene
            .layers
            .iter()
            .chain(engine.state.scene.visual.previous.iter())
        {
            images.insert(layer.asset.clone());
            if unique.insert(&layer.asset) {
                total += (layer.width as u64) * (layer.height as u64) * 4;
            }
        }
        maximum = maximum.max(total);
        assert!(
            total <= 256 * 1024 * 1024,
            "frame exceeds texture budget at page {}",
            engine.state.pages
        );
        step(&mut engine);
    }
    assert_ne!(engine.state.flow.event_id, first);
    for name in &images {
        let bytes = assets.bytes(name).unwrap();
        let rgba = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert!(rgba.width() > 0 && rgba.height() > 0);
    }
    println!(
        "{} unique images decoded; peak active texture pixels {:.1} MiB",
        images.len(),
        maximum as f64 / 1048576.0
    );
}

#[test]
#[ignore = "requires user-owned XP3"]
fn restored_page_replays_identically() {
    let assets = store();
    let mut engine = Engine::open(assets.clone()).unwrap();
    engine.start().unwrap();
    for _ in 0..100 {
        next_page(&mut engine);
    }
    let directory = tempfile::tempdir().unwrap();
    let saved = engine.state.clone();
    save::write(directory.path(), 1, &assets.fingerprint, &saved).unwrap();
    next_page(&mut engine);
    let next = engine.state.clone();
    let restored = save::read(directory.path(), 1, &assets.fingerprint).unwrap();
    engine.validate_state(&restored).unwrap();
    engine.restore(restored).unwrap();
    assert_eq!(engine.state, saved);
    next_page(&mut engine);
    assert_eq!(engine.state, next);
}

#[test]
#[ignore = "requires user-owned XP3"]
fn genuine_opus_and_vorbis_decode_to_pcm() {
    let assets = store();
    for name in ["vo6_0001.opus", "bgm5.ogg"] {
        let bytes = assets.bytes(name).unwrap();
        let wave = media::wav(&bytes).unwrap();
        assert_eq!(&wave[..4], b"RIFF");
        assert!(wave.len() > 48000);
        let peak = wave[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs())
            .max()
            .unwrap();
        assert!(peak > 100);
        println!("{name}: {} PCM bytes, peak {peak}", wave.len() - 44);
    }
}

#[test]
#[ignore = "requires user-owned XP3 with the audited video anomaly"]
fn damaged_video_is_rejected_not_silently_used() {
    assert!(store().bytes("bgimage/OPmovie.mp4").is_err());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires Metal GPU"]
fn real_metal_shader_compiles() {
    use foreign_types::ForeignType;
    let layer = metal::MetalLayer::new();
    let device = metal::Device::system_default().expect("Metal GPU required");
    layer.set_device(&device);
    unsafe {
        koikari_reborn::renderer::Renderer::new(layer.as_ptr().cast()).unwrap();
    }
}
