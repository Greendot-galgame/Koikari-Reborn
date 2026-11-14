#![cfg(any(target_os = "macos", target_os = "ios"))]

mod common;

use koikari_reborn::{
    animation,
    ffi::*,
    model::{Layer, State, HEIGHT, WIDTH},
    save,
    xp3::Store,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::{c_char, CStr, CString},
    path::{Path, PathBuf},
    time::SystemTime,
};

const SCRIPT: &str = "@bg_image num=ev101_1\nFirst[c]\n@bg_image num=ev101_2\nSecond[c]\n@wait time=100 canskip=false\nThird[c]";
const RED: &str = "cg/ev101_1.png";
const YELLOW: &str = "cg/ev101_2.png";

fn png(color: [u8; 4]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba(color))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn fixture() -> tempfile::TempDir {
    let directory = common::game(SCRIPT);
    let archive = common::archive(
        &[
            (RED, &png([255, 0, 0, 255])),
            (YELLOW, &png([255, 255, 0, 255])),
            ("blue.png", &png([0, 0, 255, 255])),
            ("face.png", &png([0, 255, 0, 128])),
            ("broken.png", b"not a decodable image"),
            ("movie.mp4", b"synthetic media, never played"),
        ],
        true,
        true,
        0,
        false,
    );
    std::fs::write(directory.path().join("patch.xp3"), archive).unwrap();
    directory
}

unsafe fn owned_string(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null());
    let value = CStr::from_ptr(pointer).to_str().unwrap().to_owned();
    kr_string_free(pointer);
    value
}

fn error() -> String {
    unsafe { owned_string(kr_error()) }
}

#[derive(Debug, PartialEq)]
struct Observation {
    snapshot: String,
    timeline: String,
    gallery: String,
    files: BTreeMap<PathBuf, (Vec<u8>, SystemTime)>,
}

struct TestHost {
    raw: *mut Host,
    saves: tempfile::TempDir,
    fingerprint: String,
}

impl TestHost {
    fn open(directory: &Path) -> Self {
        let saves = tempfile::tempdir().unwrap();
        let fingerprint = Store::open(directory).unwrap().fingerprint;
        let resource = CString::new(directory.to_str().unwrap()).unwrap();
        let storage = CString::new(saves.path().to_str().unwrap()).unwrap();
        let raw = unsafe { kr_open(resource.as_ptr(), storage.as_ptr()) };
        assert!(!raw.is_null(), "{}", error());
        Self {
            raw,
            saves,
            fingerprint,
        }
    }

    fn action(&self, action: &str) {
        let action = CString::new(action).unwrap();
        assert_eq!(
            unsafe { kr_action(self.raw, action.as_ptr()) },
            1,
            "{}",
            error()
        );
    }

    fn state(&self) -> State {
        serde_json::from_str(&unsafe { owned_string(kr_snapshot(self.raw)) }).unwrap()
    }

    fn catalog(&self) -> Vec<Value> {
        serde_json::from_str(&unsafe { owned_string(kr_saves(self.raw)) }).unwrap()
    }

    fn revision(&self, slot: u32) -> Option<String> {
        self.catalog()
            .into_iter()
            .find(|v| v["slot"] == slot)
            .unwrap()["preview_revision"]
            .as_str()
            .map(str::to_owned)
    }

    fn request(&self, slot: u32, revision: &str) -> Result<Preview, String> {
        let revision = CString::new(revision).unwrap();
        let pointer = unsafe { kr_save_preview_request(self.raw, slot, revision.as_ptr()) };
        if pointer.is_null() {
            let message = error();
            assert!(!message.is_empty());
            Err(message)
        } else {
            assert!(error().is_empty());
            Ok(Preview(pointer))
        }
    }

    fn observe(&self) -> Observation {
        let files = std::fs::read_dir(self.saves.path())
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let path = entry.path();
                let modified = entry.metadata().unwrap().modified().unwrap();
                let bytes = std::fs::read(&path).unwrap();
                (path, (bytes, modified))
            })
            .collect();
        unsafe {
            Observation {
                snapshot: owned_string(kr_snapshot(self.raw)),
                timeline: owned_string(kr_timeline(self.raw)),
                gallery: owned_string(kr_gallery(self.raw)),
                files,
            }
        }
    }

    fn write(&self, state: &State) {
        save::write(self.saves.path(), 1, &self.fingerprint, state).unwrap();
    }

    fn path(&self) -> PathBuf {
        self.saves.path().join("slot-1.json")
    }

    fn bytes(&self) -> Vec<u8> {
        std::fs::read(self.path()).unwrap()
    }

    fn overwrite(&self, bytes: &[u8]) -> String {
        std::fs::write(self.path(), bytes).unwrap();
        format!("{:x}", Sha256::digest(bytes))
    }

    fn close(&mut self) {
        unsafe { kr_close(self.raw) };
        self.raw = std::ptr::null_mut();
    }
}

impl Drop for TestHost {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug)]
struct Preview(*mut SavePreview);
// Test-only owner: this wrapper is never cloned/shared; moving it transfers the
// sole lease to a worker. All methods borrow it and Drop frees it exactly once.
unsafe impl Send for Preview {}
impl Preview {
    fn png(&self) -> Result<Vec<u8>, String> {
        let mut length = usize::MAX;
        unsafe {
            let bytes = kr_save_preview_png(self.0, &mut length);
            if bytes.is_null() {
                assert_eq!(length, 0);
                let message = error();
                assert!(!message.is_empty());
                Err(message)
            } else {
                assert!(length <= 1024 * 1024);
                let result = std::slice::from_raw_parts(bytes, length).to_vec();
                kr_bytes_free(bytes, length);
                assert!(error().is_empty());
                Ok(result)
            }
        }
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        unsafe { kr_save_preview_free(self.0) }
    }
}

#[test]
fn revision_is_exact_validated_bytes_not_timestamp_page_or_reserialization() {
    let directory = fixture();
    let host = TestHost::open(directory.path());
    assert!(host
        .catalog()
        .iter()
        .all(|v| v["preview_revision"].is_null()));
    host.action("new");
    host.action("save:1");
    let original = host.bytes();
    let revision = host.revision(1).unwrap();
    assert_eq!(revision, format!("{:x}", Sha256::digest(&original)));
    assert_eq!(revision.len(), 64);
    let read =
        save::read_versioned_record(host.saves.path(), 1, &host.fingerprint, Some(&revision))
            .unwrap();
    assert_eq!(read.revision, revision);
    assert_eq!(read.record.state, host.state());
    assert_eq!(
        save::read_record(host.saves.path(), 1, &host.fingerprint)
            .unwrap()
            .state,
        read.record.state
    );

    let modified = host.path().metadata().unwrap().modified().unwrap();
    let page = host.state().pages;
    let mut changed: Value = serde_json::from_slice(&original).unwrap();
    changed["state"]["scene"]["text"] = json!("Same timestamp and page, different persisted scene");
    let changed = serde_json::to_vec(&changed).unwrap();
    let next_revision = host.overwrite(&changed);
    std::fs::File::open(host.path())
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(
        host.path().metadata().unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(host.catalog()[0]["pages"], page);
    assert_eq!(host.revision(1).as_deref(), Some(next_revision.as_str()));
    assert_ne!(next_revision, revision);
    let before = host.observe();
    assert!(host
        .request(1, &revision)
        .unwrap_err()
        .contains("revision changed"));
    drop(host.request(1, &next_revision).unwrap());
    assert_eq!(host.observe(), before);

    let mut whitespace = changed;
    whitespace.extend_from_slice(b" \n\t");
    let whitespace_revision = host.overwrite(&whitespace);
    assert_ne!(whitespace_revision, next_revision);
    assert_eq!(host.revision(1).unwrap(), whitespace_revision);
    assert!(host.request(1, &next_revision).is_err());
    drop(host.request(1, &whitespace_revision).unwrap());
}

#[test]
fn requests_are_readonly_preserve_gallery_and_release_after_host_close() {
    let directory = fixture();
    let mut host = TestHost::open(directory.path());
    host.action("new");
    host.action("save:1");
    let revision = host.revision(1).unwrap();
    host.action("advance");
    host.action("advance");
    assert_eq!(host.state().flow.wait_ms, 100);
    let before = host.observe();
    let lease = host.request(1, &revision).unwrap();
    assert_eq!(host.observe(), before);
    assert_eq!(unsafe { kr_tick(host.raw, 25) }, 2); // Did not enter gallery.

    let id = CString::new("ev101").unwrap();
    assert_eq!(unsafe { kr_gallery_show(host.raw, id.as_ptr()) }, 1);
    let before = host.observe();
    drop(host.request(1, &revision).unwrap());
    assert!(host.request(42, &revision).is_err());
    assert_eq!(host.observe(), before);
    assert_eq!(unsafe { kr_tick(host.raw, 25) }, 1); // Did not leave gallery.
    assert_eq!(host.observe(), before);
    host.action("title"); // Save cards on title still preview the persisted story.
    assert!(host.state().title);
    let before = host.observe();
    drop(host.request(1, &revision).unwrap());
    assert_eq!(host.observe(), before);
    host.close();
    std::thread::spawn(move || drop(lease)).join().unwrap(); // Unused/canceled lease.
    unsafe { kr_save_preview_free(std::ptr::null_mut()) };
}

#[test]
fn invalid_save_bytes_and_engine_states_never_publish_or_lease_a_revision() {
    let directory = fixture();
    let host = TestHost::open(directory.path());
    host.action("new");
    host.action("save:1");
    let valid: Value = serde_json::from_slice(&host.bytes()).unwrap();
    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("syntax", b"not JSON".to_vec()),
        ("size", vec![b' '; 4 * 1024 * 1024 + 1]),
    ];
    for (label, mutate) in [
        ("fingerprint", 0),
        ("version", 1),
        ("flow missing", 2),
        ("structural layer", 3),
        ("engine cursor", 4),
        ("engine resource", 5),
        ("visual command", 6),
        ("rollback invalid", 7),
        ("future flow", 8),
    ] {
        let mut value = valid.clone();
        match mutate {
            0 => value["assets"] = json!("other-resource-fingerprint"),
            1 => value["version"] = json!(99),
            2 => {
                value["state"].as_object_mut().unwrap().remove("flow");
            }
            3 => value["state"]["scene"]["layers"][0]["opacity"] = json!(2.0),
            4 => value["state"]["cursor"]["token"] = json!(1000000),
            5 => value["state"]["scene"]["layers"][0]["asset"] = json!("missing.png"),
            6 => {
                value["state"]["scene"]["visual"]["commands"] =
                    json!([{"name":"not-real", "args":{}}])
            }
            7 => {
                let mut bad = value["state"].clone();
                bad["scene"]["layers"][0]["width"] = json!(-1);
                value["rollback"] = json!([bad]);
            }
            8 => value["state"]["flow"]["revision"] = json!(99),
            _ => unreachable!(),
        }
        cases.push((label, serde_json::to_vec(&value).unwrap()));
    }
    for (label, bytes) in cases {
        let revision = host.overwrite(&bytes);
        let before = host.observe();
        assert!(host.revision(1).is_none(), "{label}");
        assert!(
            !host.catalog()[0]["compatible"].as_bool().unwrap(),
            "{label}"
        );
        assert!(host.request(1, &revision).is_err(), "{label}");
        assert_eq!(host.observe(), before, "{label}");
    }
}

#[test]
fn legacy_migrations_title_blocked_and_native_overlays_are_never_guessed() {
    let directory = fixture();
    let host = TestHost::open(directory.path());
    host.action("new");
    host.action("save:1");
    let valid: Value = serde_json::from_slice(&host.bytes()).unwrap();
    for kind in [
        "legacy",
        "flow revision",
        "visual revision",
        "title",
        "blocked",
        "video",
        "credits",
        "interlude",
    ] {
        let mut value = valid.clone();
        match kind {
            "legacy" => {
                value["version"] = json!(1);
                value["state"].as_object_mut().unwrap().remove("flow");
            }
            "flow revision" => value["state"]["flow"]["revision"] = json!(0),
            "visual revision" => value["state"]["visual_revision"] = json!(0),
            "title" => value["state"]["title"] = json!(true),
            "blocked" => value["state"]["blocked"] = json!("unsupported synthetic instruction"),
            "video" => {
                value["state"]["scene"]["video"] = json!({"asset":"movie.mp4", "sequence":0})
            }
            "credits" => value["state"]["scene"]["credits"] = json!({"text":"synthetic credits"}),
            "interlude" => value["state"]["interlude"] = json!({"background":RED,"bgm":""}),
            _ => unreachable!(),
        }
        let revision = host.overwrite(&serde_json::to_vec(&value).unwrap());
        let before = host.observe();
        assert!(host.revision(1).is_none(), "{kind}");
        assert!(host.request(1, &revision).is_err(), "{kind}");
        assert_eq!(host.observe(), before, "{kind}");
    }
}

#[test]
fn invalid_arguments_fail_closed_and_png_failure_does_not_consume_lease() {
    let directory = fixture();
    let host = TestHost::open(directory.path());
    host.action("new");
    host.action("save:1");
    let revision = host.revision(1).unwrap();
    let c_revision = CString::new(revision.clone()).unwrap();
    let before = host.observe();
    for slot in [0, 7, 8, 10, u32::MAX, 2] {
        assert!(host.request(slot, &revision).is_err());
    }
    for revision in [
        "",
        "unknown",
        &"a".repeat(63),
        &"F".repeat(64),
        &"g".repeat(64),
    ] {
        assert!(host
            .request(1, revision)
            .unwrap_err()
            .contains("invalid save preview revision"));
    }
    unsafe {
        assert!(kr_save_preview_request(std::ptr::null_mut(), 1, c_revision.as_ptr()).is_null());
        assert!(error().contains("null engine handle"));
        assert!(kr_save_preview_request(host.raw, 1, std::ptr::null()).is_null());
        assert!(error().contains("null string"));
        let invalid_utf8 = [255u8, 0];
        assert!(kr_save_preview_request(host.raw, 1, invalid_utf8.as_ptr().cast()).is_null());
        assert!(error().contains("invalid utf-8"));
        let mut length = usize::MAX;
        assert!(kr_save_preview_png(std::ptr::null_mut(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(error().contains("null save preview lease"));
        let lease = host.request(1, &revision).unwrap();
        // Both failures borrow the same valid lease; Drop still frees it once.
        for _ in 0..2 {
            assert!(kr_save_preview_png(lease.0, std::ptr::null_mut()).is_null());
            assert!(error().contains("length pointer"));
        }
        drop(lease);
        kr_save_preview_free(std::ptr::null_mut());
        kr_bytes_free(std::ptr::null_mut(), 0);
    }
    assert_eq!(host.observe(), before);
}

fn layer(asset: &str, group: u32) -> Layer {
    Layer {
        asset: asset.into(),
        x: 0.0,
        y: 0.0,
        width: WIDTH,
        height: HEIGHT,
        opacity: 1.0,
        group,
    }
}

fn rgb_near(image: &image::RgbaImage, expected: [u8; 3]) {
    assert_eq!(image.dimensions(), (384, 216));
    for (x, y) in [(32, 32), (192, 108), (350, 190)] {
        let actual = image.get_pixel(x, y);
        for i in 0..3 {
            assert!(
                (i32::from(actual[i]) - i32::from(expected[i])).abs() <= 3,
                "{actual:?} != {expected:?}"
            );
        }
        assert_eq!(actual[3], 255);
    }
}

#[test]
#[ignore = "requires Metal device; explicit synthetic headless save-preview validation"]
fn gpu_saved_scene_elapsed_previous_and_character_layers_survive_overwrite_and_host_close() {
    let directory = fixture();
    let mut host = TestHost::open(directory.path());
    host.action("new");
    let mut saved = host.state();
    saved.scene.visual = Default::default();
    saved.scene.layers = vec![layer(RED, 0)];
    animation::command(&mut saved.scene, "ts", &BTreeMap::new()).unwrap();
    saved.scene.layers = vec![layer("blue.png", 1), layer("face.png", 1)];
    animation::command(
        &mut saved.scene,
        "te",
        &BTreeMap::from([("time".into(), "100".into())]),
    )
    .unwrap();
    animation::tick(&mut saved.scene, 50);
    assert_eq!(saved.scene.visual.elapsed_ms, 50);
    host.write(&saved);
    let revision = host.revision(1).unwrap();
    host.action("advance");
    assert_eq!(host.state().scene.layers[0].asset, YELLOW);
    let before = host.observe();
    let lease = host.request(1, &revision).unwrap();
    let (rendered, first_frame) = std::sync::mpsc::channel();
    let (proceed, host_closed) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let first = lease.png().unwrap();
        assert_eq!(first, lease.png().unwrap()); // png borrows, repeat is stable.
        rendered.send(first.clone()).unwrap();
        host_closed.recv().unwrap();
        assert_eq!(lease.png().unwrap(), first); // No Host/save path retained.
        drop(lease); // Same exclusive worker owns the lease throughout.
    });
    let first = first_frame.recv().unwrap();
    assert_eq!(host.observe(), before);
    let image = image::load_from_memory(&first).unwrap().into_rgba8();
    rgb_near(&image, [128, 64, 63]); // NOT live yellow, BG red, or completed blue+green.
    assert!(first.starts_with(b"\x89PNG\r\n\x1a\n"));

    host.action("save:1"); // Same slot now contains live yellow.
    assert_ne!(host.revision(1).unwrap(), revision);
    host.close();
    proceed.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
#[ignore = "requires Metal device; explicit synthetic failure/no-partial-image validation"]
fn gpu_corrupt_current_or_previous_asset_returns_no_partial_png_and_retains_lease() {
    let directory = fixture();
    let host = TestHost::open(directory.path());
    host.action("new");
    let valid = host.state();
    for previous in [false, true] {
        let mut saved = valid.clone();
        if previous {
            saved.scene.visual = Default::default();
            saved.scene.layers = vec![layer("broken.png", 0)];
            animation::command(&mut saved.scene, "ts", &BTreeMap::new()).unwrap();
            saved.scene.layers = vec![layer(RED, 0)];
            animation::command(
                &mut saved.scene,
                "te",
                &BTreeMap::from([("time".into(), "100".into())]),
            )
            .unwrap();
            animation::tick(&mut saved.scene, 50);
        } else {
            saved.scene.layers.push(layer("broken.png", 1));
        }
        host.write(&saved);
        let revision = host.revision(1).unwrap();
        let before = host.observe();
        let lease = host.request(1, &revision).unwrap();
        std::thread::spawn(move || {
            assert!(lease.png().is_err());
            assert!(lease.png().is_err()); // Failure also borrows.
            drop(lease);
        })
        .join()
        .unwrap();
        assert_eq!(host.observe(), before);
    }
}

#[test]
#[ignore = "requires Metal device; explicit bounded encoded asset read validation"]
fn gpu_encoded_asset_limit_is_checked_before_archive_payload_read() {
    let directory = fixture();
    let mut archive = common::archive(&[("oversized.png", b"x")], false, true, 0, false);
    let info = archive.windows(4).position(|v| v == b"info").unwrap() + 12;
    let segment = archive.windows(4).position(|v| v == b"segm").unwrap() + 12;
    let size = (32u64 * 1024 * 1024 + 1).to_le_bytes();
    archive[info + 4..info + 12].copy_from_slice(&size);
    archive[segment + 12..segment + 20].copy_from_slice(&size);
    let patch = directory.path().join("patch10.xp3");
    std::fs::write(&patch, archive).unwrap();
    let host = TestHost::open(directory.path());
    host.action("new");
    let mut saved = host.state();
    saved.scene.layers.push(layer("oversized.png", 1));
    host.write(&saved);
    let revision = host.revision(1).unwrap();
    let lease = host.request(1, &revision).unwrap();
    let before = host.observe();
    // Metadata already indexed. Any attempted payload read now fails with I/O,
    // so the byte-limit error proves a pre-read/pre-decompression rejection.
    std::fs::write(&patch, b"").unwrap();
    std::thread::spawn(move || {
        assert!(lease.png().unwrap_err().contains("byte limit"));
        drop(lease);
    })
    .join()
    .unwrap();
    assert_eq!(host.observe(), before);
}

#[test]
#[ignore = "requires Metal device; explicit 64 MiB preview-only GPU asset budget validation"]
fn gpu_preview_asset_budget_rejects_before_decode_or_large_texture_allocation() {
    let directory = fixture();
    // Tiny owned PNG with a valid oversized IHDR. The IDAT still contains only
    // 2x2 pixels: reaching decoding would fail differently. No large fixture or
    // large decoded allocation is needed to verify the pre-allocation budget.
    let mut image = png([20, 40, 60, 255]);
    assert_eq!(&image[12..16], b"IHDR");
    image[16..20].copy_from_slice(&4097u32.to_be_bytes());
    image[20..24].copy_from_slice(&4096u32.to_be_bytes());
    let mut crc = !0u32;
    for byte in &image[12..29] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    image[29..33].copy_from_slice(&(!crc).to_be_bytes());
    assert_eq!(
        image::ImageReader::new(std::io::Cursor::new(&image))
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap(),
        (4097, 4096)
    );
    std::fs::write(
        directory.path().join("patch10.xp3"),
        common::archive(&[("large.png", &image)], true, true, 0, false),
    )
    .unwrap();
    let host = TestHost::open(directory.path());
    host.action("new");
    let mut saved = host.state();
    saved.scene.layers.push(layer("large.png", 1));
    host.write(&saved);
    let revision = host.revision(1).unwrap();
    let lease = host.request(1, &revision).unwrap();
    let before = host.observe();
    std::thread::spawn(move || {
        assert_eq!(
            lease.png().unwrap_err(),
            "scene exceeds the 64 MiB texture budget"
        );
        drop(lease);
    })
    .join()
    .unwrap();
    assert_eq!(host.observe(), before);
}
