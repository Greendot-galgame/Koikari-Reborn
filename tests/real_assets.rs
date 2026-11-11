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
#[ignore = "requires user-owned XP3"]
fn opening_reaches_explicit_preview_boundary() {
    let mut engine = Engine::open(store()).unwrap();
    engine.start().unwrap();
    while !engine.state.finished && engine.state.pages < 10000 {
        engine.advance().unwrap();
    }
    assert!(engine.state.finished);
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
    let mut introductions = 0;
    while !engine.state.finished {
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
            engine.advance().unwrap();
            assert_eq!(engine.state.scene.layers[0].asset, background);
            assert!(engine.state.scene.message_visible);
            if introductions == 1 {
                engine.advance().unwrap();
                engine.advance().unwrap();
                assert_eq!(engine.state.scene.layers[0].asset, classroom);
                println!(
                    "Reported classroom regression now at page {}",
                    engine.state.pages
                );
            }
        } else {
            engine.advance().unwrap();
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
        engine.advance().unwrap();
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
    let mut maximum = 0u64;
    while !engine.state.finished && engine.state.pages < 10000 {
        let mut unique = BTreeSet::new();
        let mut total = 0u64;
        for layer in &engine.state.scene.layers {
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
        engine.advance().unwrap();
    }
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
        engine.advance().unwrap();
    }
    let directory = tempfile::tempdir().unwrap();
    let saved = engine.state.clone();
    save::write(directory.path(), 1, &assets.fingerprint, &saved).unwrap();
    engine.advance().unwrap();
    let next = engine.state.clone();
    let restored = save::read(directory.path(), 1, &assets.fingerprint).unwrap();
    engine.validate_state(&restored).unwrap();
    engine.state = restored;
    assert_eq!(engine.state, saved);
    engine.advance().unwrap();
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
