use koikari_reborn::{
    events::Database,
    save,
    story::Engine,
    xp3::{Archive, Assets, Store},
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

fn directory() -> PathBuf {
    std::env::var_os("KOIKARI_PATCH_ASSETS")
        .expect("set KOIKARI_PATCH_ASSETS to a folder with data/adult/patch.xp3")
        .into()
}

#[test]
#[ignore = "requires authorized merged XP3; optionally KOIKARI_PENDING_PAGE_SAVES"]
fn authentic_pending_page_save_restores_without_rewriting_original() {
    let store = Arc::new(Store::open(directory()).unwrap());
    let scratch = tempfile::tempdir().unwrap();
    let saves = if let Some(directory) = std::env::var_os("KOIKARI_PENDING_PAGE_SAVES") {
        PathBuf::from(directory)
    } else {
        let mut engine = Engine::open(store.clone()).unwrap();
        engine.start().unwrap();
        for _ in 0..1000 {
            if engine.state.flow.page_open
                && engine.state.flow.wait_ms > 0
                && engine
                    .previous_pages()
                    .last()
                    .is_some_and(|s| s.pages == engine.state.pages)
            {
                break;
            }
            step(&mut engine, &BTreeMap::new());
        }
        let window: Vec<_> = engine.previous_pages().cloned().collect();
        save::write_with_rollback(
            scratch.path(),
            9,
            &store.fingerprint,
            &engine.state,
            &window,
        )
        .unwrap();
        scratch.path().to_owned()
    };
    let path = saves.join("slot-9.json");
    let original = std::fs::read(&path).unwrap();
    let record = save::read_record(&saves, 9, &store.fingerprint).unwrap();
    assert!(record.state.flow.page_open && record.state.flow.wait_ms > 0);
    assert_eq!(record.rollback.last().unwrap().pages, record.state.pages);
    let expected = record.state.clone();
    let mut restored = Engine::open(store).unwrap();
    restored.restore(record.state).unwrap();
    restored.restore_checkpoints(record.rollback).unwrap();
    assert_eq!(restored.state, expected);
    assert_eq!(
        restored.previous_pages().last().unwrap().pages,
        expected.pages
    );
    restored.tick(expected.flow.wait_ms).unwrap();
    assert!(restored.state.blocked.is_none());
    assert_eq!(std::fs::read(path).unwrap(), original);
}

fn step(engine: &mut Engine, decisions: &BTreeMap<usize, usize>) {
    if !engine.state.flow.choices.is_empty() {
        let selection = engine.state.flow.selection_id.unwrap();
        engine
            .choose(*decisions.get(&selection).unwrap_or(&0))
            .unwrap();
    } else if let Some(video) = &engine.state.scene.video {
        engine.finish_video(video.sequence).unwrap();
    } else if engine.state.flow.wait_ms > 0 {
        engine.tick(engine.state.flow.wait_ms).unwrap();
    } else {
        engine.advance().unwrap();
    }
}

fn next_page(engine: &mut Engine) {
    let pages = engine.state.pages;
    for _ in 0..1000 {
        step(engine, &BTreeMap::new());
        if engine.state.pages > pages || engine.state.finished {
            return;
        }
    }
    panic!("page continuation exceeded budget");
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
    let first = engine.state.flow.event_id;
    let mut introductions = Vec::new();
    for _ in 0..30000 {
        if engine.state.flow.event_id != first {
            break;
        }
        assert!(!engine.state.finished);
        if let Some(interlude) = &engine.state.interlude {
            introductions.push((
                engine.state.pages,
                engine.state.scene.layers[0].asset.clone(),
            ));
            assert!(!engine.state.scene.message_visible);
            let background = interlude.background.clone();
            let saved = engine.state.clone();
            next_page(&mut engine);
            let next = engine.state.clone();
            assert_eq!(next.scene.layers[0].asset, background);
            engine.restore(saved).unwrap();
            next_page(&mut engine);
            assert_eq!(engine.state, next);
        } else {
            step(&mut engine, &BTreeMap::new());
        }
    }
    assert_ne!(engine.state.flow.event_id, first);
    assert!(!engine.state.finished);
    assert!(engine.state.flow.ending.is_none());
    assert_eq!(introductions.len(), 3);
    println!(
        "Patched continuous chapter at {}; introductions: {introductions:?}",
        engine.state.pages
    );
}

#[test]
#[ignore = "requires complete user-owned XP3 and implemented animation peer; headless only"]
fn seven_legal_paths_reach_every_database_event_and_true_terminal_routes() {
    let assets = Arc::new(Store::open(directory()).unwrap());
    let db = Database::parse(&assets.text("DB_EV.ks").unwrap()).unwrap();
    assert_eq!(db.events.len(), 124);
    assert_eq!(db.selections.len(), 13);
    let paths = [
        vec![(7, 0)],
        vec![(7, 1), (8, 0)],
        vec![(7, 1), (8, 1), (9, 1)],
        vec![(7, 1), (8, 1), (9, 0), (10, 0), (11, 0)],
        vec![(7, 1), (8, 1), (9, 0), (10, 0), (11, 1)],
        vec![(7, 1), (8, 1), (9, 0), (10, 1), (12, 0)],
        vec![(7, 1), (8, 1), (9, 0), (10, 1), (12, 1)],
    ];
    let mut visited = BTreeSet::new();
    let mut endings = BTreeSet::new();
    let mut selections = BTreeSet::new();
    let mut choice_states = BTreeMap::new();
    for path in paths {
        let decisions = path.into_iter().collect();
        let mut engine = Engine::open(assets.clone()).unwrap();
        engine.start().unwrap();
        for _ in 0..100000 {
            if engine.state.finished {
                break;
            }
            if !engine.state.flow.choices.is_empty() {
                let group = engine.state.flow.selection_id.unwrap();
                selections.insert(group);
                choice_states
                    .entry(group)
                    .or_insert_with(|| engine.state.clone());
            }
            step(&mut engine, &decisions);
        }
        assert!(engine.state.finished, "route step budget exceeded");
        assert!(engine.state.blocked.is_none());
        assert!(engine.state.flow.ending.is_some());
        visited.extend(engine.state.flow.visited.iter().cloned());
        endings.extend(engine.state.progress.endings.iter().cloned());
    }
    let main: BTreeSet<_> = db
        .events
        .iter()
        .map(|e| assets.find(&e.script, &["ks"]).unwrap().to_lowercase())
        .collect();
    assert_eq!(
        visited
            .intersection(&main)
            .cloned()
            .collect::<BTreeSet<_>>(),
        main
    );
    assert_eq!(selections, (0..13).collect());
    assert_eq!(endings, (1..=7).map(|r| format!("route:{r}")).collect());
    let mut branches = 0;
    for (group, state) in choice_states {
        for choice in &state.flow.choices {
            let mut engine = Engine::open(assets.clone()).unwrap();
            engine.restore(state.clone()).unwrap();
            engine.choose(choice.id).unwrap();
            for _ in 0..20000 {
                if engine.state.finished
                    || engine.state.flow.event_id != state.flow.event_id
                    || engine.state.flow.selection_id != Some(group)
                {
                    break;
                }
                step(&mut engine, &BTreeMap::new());
            }
            assert!(
                engine.state.finished
                    || engine.state.flow.event_id != state.flow.event_id
                    || engine.state.flow.selection_id != Some(group),
                "choice branch continuation budget exceeded"
            );
            assert!(engine.state.blocked.is_none());
            branches += 1;
        }
    }
    assert_eq!(branches, 31);
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
