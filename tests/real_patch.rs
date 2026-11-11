use koikari_reborn::{
    save,
    story::Engine,
    xp3::{Archive, Assets, Store},
};
use std::path::PathBuf;
use std::sync::Arc;

fn directory() -> PathBuf {
    std::env::var_os("KOIKARI_PATCH_ASSETS")
        .expect("set KOIKARI_PATCH_ASSETS to a folder with data/adult/patch.xp3")
        .into()
}

#[test]
#[ignore = "requires user-owned base XP3 and KG012Z4A0X0X-VER100"]
fn supplemental_markers_scripts_and_overlay_are_readable() {
    let directory = directory();
    let store = Store::open(&directory).unwrap();
    let archive = Archive::open(directory.join("adult.xp3")).unwrap();
    assert_eq!(
        store.archive_names(),
        ["data.xp3", "adult.xp3", "patch.xp3"]
    );
    for name in ["adult.tjs", "scenario/adult.tjs"] {
        assert_eq!(archive.entries[name].size, 0);
        assert!(archive.bytes(name).unwrap().is_empty());
        assert!(store.text(name).unwrap().is_empty());
    }
    assert_eq!(
        store.bytes("scenario/main/0_1.ks").unwrap(),
        archive.bytes("scenario/main/0_1.ks").unwrap()
    );
    println!(
        "{} supplemental entries indexed; empty markers and opening override verified",
        archive.entries.len()
    );
}

#[test]
#[ignore = "requires user-owned base XP3 and KG012Z4A0X0X-VER100"]
fn every_supplemental_entry_passes_integrity_and_images_decode() {
    let archive = Archive::open(directory().join("adult.xp3")).unwrap();
    let mut images = 0;
    for name in archive.entries.keys() {
        let bytes = archive
            .bytes(name)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        if name.ends_with(".webp") {
            let image = image::load_from_memory(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(image.width() > 0 && image.width() <= 8192);
            assert!(image.height() > 0 && image.height() <= 8192);
            images += 1;
        } else if name.ends_with(".ks") || name.ends_with(".tjs") {
            assert!(std::str::from_utf8(&bytes).is_ok(), "{name}");
        } else if name.ends_with(".opus") || name.ends_with(".ogg") {
            assert!(bytes.starts_with(b"OggS"), "{name}");
        } else if name.ends_with(".mp4") {
            assert_eq!(bytes.get(4..8), Some(b"ftyp".as_slice()), "{name}");
        }
    }
    println!(
        "{} supplemental entries passed strict integrity; {images} images decoded",
        archive.entries.len()
    );
}

#[test]
#[ignore = "requires user-owned base XP3 and KG012Z4A0X0X-VER100"]
fn patched_opening_preserves_introductions_and_deterministic_restore() {
    let mut engine = Engine::open(Arc::new(Store::open(directory()).unwrap())).unwrap();
    engine.start().unwrap();
    let mut introductions = Vec::new();
    while !engine.state.finished && engine.state.pages < 1000 {
        if let Some(interlude) = &engine.state.interlude {
            introductions.push((
                engine.state.pages,
                engine.state.scene.layers[0].asset.clone(),
            ));
            assert!(!engine.state.scene.message_visible);
            let background = interlude.background.clone();
            let saved = engine.state.clone();
            engine.advance().unwrap();
            let next = engine.state.clone();
            assert_eq!(next.scene.layers[0].asset, background);
            engine.restore(saved).unwrap();
            engine.advance().unwrap();
            assert_eq!(engine.state, next);
        } else {
            engine.advance().unwrap();
        }
    }
    assert!(engine.state.finished);
    assert_eq!(introductions.len(), 3);
    println!(
        "Patched preview ended at {}; introductions: {introductions:?}",
        engine.state.pages
    );
}

#[test]
#[ignore = "requires user-owned base XP3 and KG012Z4A0X0X-VER100"]
fn new_archive_fingerprint_does_not_accept_or_modify_old_saves() {
    let old =
        std::env::var_os("KOIKARI_ASSETS").expect("set KOIKARI_ASSETS to the original folder");
    let old = Arc::new(Store::open(old).unwrap());
    let new = Store::open(directory()).unwrap();
    let mut engine = Engine::open(old.clone()).unwrap();
    engine.start().unwrap();
    let saves = tempfile::tempdir().unwrap();
    save::write(saves.path(), 1, &old.fingerprint, &engine.state).unwrap();
    let before = std::fs::read(saves.path().join("slot-1.json")).unwrap();
    assert!(save::read(saves.path(), 1, &new.fingerprint).is_err());
    assert!(save::read(saves.path(), 1, &old.fingerprint).is_ok());
    assert_eq!(
        std::fs::read(saves.path().join("slot-1.json")).unwrap(),
        before
    );
}
