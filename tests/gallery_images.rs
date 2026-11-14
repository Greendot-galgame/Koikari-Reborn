#![cfg(any(target_os = "macos", target_os = "ios"))]

mod common;

use koikari_reborn::{
    ffi::*,
    model::{GalleryEntry, State},
};
use std::{
    collections::BTreeMap,
    ffi::{c_char, CStr, CString},
    path::{Path, PathBuf},
    time::SystemTime,
};

const ID: &str = "ev101";
const ASSET: &str = "cg/ev101_1.png";
const OTHER_VARIANT: &str = "cg/ev101_2.png";
const LOCKED: &str = "ev102";
const LIMIT: u64 = 32 * 1024 * 1024;
const SCRIPT: &str = "@bg_image num=ev101_1\nFirst[c]\nSecond[c]\n@SC_EndProcess";

fn png(color: [u8; 4]) -> Vec<u8> {
    let mut output = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba(color))
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn fixture(script: &str, image: &[u8]) -> tempfile::TempDir {
    let directory = common::game(script);
    patch(
        directory.path(),
        "patch.xp3",
        &common::archive(
            &[(ASSET, image), ("cg/ev102_1.png", image)],
            true,
            true,
            0,
            false,
        ),
    );
    directory
}

fn patch(directory: &Path, name: &str, bytes: &[u8]) {
    std::fs::write(directory.join(name), bytes).unwrap();
}

fn oversized_archive() -> Vec<u8> {
    // One stored synthetic byte, not a large fixture or decompression bomb.
    let mut bytes = common::archive(&[(ASSET, b"x")], false, true, 0, false);
    let info = bytes.windows(4).position(|v| v == b"info").unwrap() + 12;
    let segment = bytes.windows(4).position(|v| v == b"segm").unwrap() + 12;
    let size = (LIMIT + 1).to_le_bytes();
    bytes[info + 4..info + 12].copy_from_slice(&size);
    bytes[segment + 12..segment + 20].copy_from_slice(&size);
    bytes
}

unsafe fn owned_string(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null());
    let result = CStr::from_ptr(pointer).to_str().unwrap().to_owned();
    kr_string_free(pointer);
    result
}

fn error() -> String {
    unsafe { owned_string(kr_error()) }
}

#[derive(Debug, PartialEq)]
struct Observation {
    snapshot: String,
    timeline: String,
    gallery: String,
    saves: BTreeMap<PathBuf, (Vec<u8>, SystemTime)>,
}

struct TestHost {
    raw: *mut Host,
    saves: tempfile::TempDir,
}

impl TestHost {
    fn open(directory: &Path) -> Self {
        let saves = tempfile::tempdir().unwrap();
        let directory = CString::new(directory.to_str().unwrap()).unwrap();
        let save_directory = CString::new(saves.path().to_str().unwrap()).unwrap();
        let raw = unsafe { kr_open(directory.as_ptr(), save_directory.as_ptr()) };
        assert!(!raw.is_null(), "{}", error());
        Self { raw, saves }
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
        let snapshot = unsafe { owned_string(kr_snapshot(self.raw)) };
        serde_json::from_str(&snapshot).unwrap()
    }

    fn gallery(&self) -> Vec<GalleryEntry> {
        let gallery = unsafe { owned_string(kr_gallery(self.raw)) };
        serde_json::from_str(&gallery).unwrap()
    }

    fn observe(&self) -> Observation {
        let saves = std::fs::read_dir(self.saves.path())
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
                saves,
            }
        }
    }

    fn image(&self, id: &str) -> Result<Vec<u8>, String> {
        let id = CString::new(id).unwrap();
        let mut length = usize::MAX;
        unsafe {
            let bytes = kr_gallery_image(self.raw, id.as_ptr(), &mut length);
            if bytes.is_null() {
                assert_eq!(length, 0);
                let message = error();
                assert!(!message.is_empty());
                Err(message)
            } else {
                let result = std::slice::from_raw_parts(bytes, length).to_vec();
                // The original returned pointer and count, exactly once.
                kr_bytes_free(bytes, length);
                assert!(error().is_empty());
                Ok(result)
            }
        }
    }
}

impl Drop for TestHost {
    fn drop(&mut self) {
        unsafe { kr_close(self.raw) }
    }
}

#[test]
fn only_current_unlocked_entry_ids_are_accepted_without_state_or_save_changes() {
    let image = png([30, 90, 150, 255]);
    let directory = fixture(SCRIPT, &image);
    let host = TestHost::open(directory.path());
    assert!(host.gallery().iter().all(|entry| !entry.unlocked));
    let before = host.observe();
    assert_eq!(host.image(ID).unwrap_err(), "CG is locked or unknown");
    assert_eq!(host.observe(), before);

    // Actual normal scenario execution unlocks this CG, never injected progress.
    host.action("new");
    assert_eq!(host.state().scene.text, "First");
    assert!(host.state().blocked.is_none());
    assert!(host.state().progress.cg.contains(ID));
    host.action("save:1");
    let before = host.observe();
    assert!(!before.saves.is_empty());
    assert_eq!(host.image(ID).unwrap(), image);
    for id in [
        LOCKED,
        "ev999",
        "",
        "EV101",
        " ev101",
        "ev101 ",
        ASSET,
        "ev101_1.png",
        "ev101_1",
        "intro.ks",
        "../ev101",
        "./ev101",
        "/ev101",
        "cg/ev101",
        "cg\\ev101",
        "ev101/../ev102",
        "event:intro",
    ] {
        assert_eq!(
            host.image(id).unwrap_err(),
            "CG is locked or unknown",
            "{id}"
        );
    }
    assert_eq!(host.image(ID).unwrap(), image); // Also clears the previous error.
    assert_eq!(host.observe(), before);

    host.action("title");
    let before = host.observe();
    assert_eq!(host.image(ID).unwrap(), image);
    assert_eq!(host.observe(), before);
}

#[test]
fn each_read_resolves_the_current_seen_variant_and_highest_valid_patch() {
    for packed in [false, true] {
        let base = png([0, 0, 0, 255]);
        let old = png([40, 50, 60, 255]);
        let current = png([100, 110, 120, 255]);
        let other = png([200, 210, 220, 255]);
        let directory = fixture(
            "@bg_image num=ev101_2\nFirst[c]\n@bg_image num=ev101_1\nSecond[c]",
            &base,
        );
        for (name, image) in [("patch2.xp3", &old), ("patch10.xp3", &current)] {
            patch(
                directory.path(),
                name,
                &common::archive(
                    &[(ASSET, image), (OTHER_VARIANT, &other)],
                    packed,
                    packed,
                    0,
                    false,
                ),
            );
        }
        let host = TestHost::open(directory.path());
        assert_eq!(host.gallery()[0].asset, ASSET);
        host.action("new");
        assert_eq!(host.gallery()[0].asset, OTHER_VARIANT);
        let before = host.observe();
        assert_eq!(host.image(ID).unwrap(), other);
        assert_eq!(host.observe(), before);

        host.action("advance");
        assert_eq!(host.state().scene.text, "Second");
        assert_eq!(host.gallery()[0].asset, ASSET);
        let before = host.observe();
        assert_eq!(host.image(ID).unwrap(), current);
        assert_eq!(host.observe(), before);
    }
}

#[test]
fn protected_corrupt_and_oversized_overrides_fail_closed_without_base_fallback() {
    let image = png([1, 2, 3, 255]);
    for (archive, expected) in [
        (
            common::archive(&[(ASSET, &image)], true, true, 0, true),
            "Protected",
        ),
        (
            common::archive(&[(ASSET, &image)], true, true, 1, false),
            "checksum",
        ),
        (oversized_archive(), "byte limit"),
    ] {
        let directory = fixture(SCRIPT, &image);
        patch(directory.path(), "patch10.xp3", &archive);
        let host = TestHost::open(directory.path());
        // Normal bg_image execution resolves the CG without rendering/decoding it.
        host.action("new");
        assert_eq!(host.state().scene.text, "First");
        assert!(host.state().blocked.is_none());
        assert!(host.gallery()[0].unlocked);
        let before = host.observe();
        let failure = host.image(ID).unwrap_err();
        assert!(failure.contains(expected), "{expected}: {failure}");
        assert_eq!(host.observe(), before);

        if expected == "byte limit" {
            // The Store already indexed this tiny fixture. Empty its archive so
            // any attempted payload read would instead fail with an I/O error.
            patch(directory.path(), "patch10.xp3", b"");
            assert!(host.image(ID).unwrap_err().contains("byte limit"));
            assert_eq!(host.observe(), before);
        }
    }
}

#[test]
fn encoded_bytes_are_returned_verbatim_without_rust_image_decoding() {
    let encoded = b"synthetic encoded payload, deliberately not a decodable image";
    let directory = fixture(SCRIPT, encoded);
    let host = TestHost::open(directory.path());
    host.action("new");
    let before = host.observe();
    assert_eq!(host.image(ID).unwrap(), encoded);
    assert_eq!(host.observe(), before);
}

#[test]
fn thumbnail_reads_neither_enter_nor_leave_an_existing_gallery_preview() {
    let image = png([10, 20, 30, 255]);
    let directory = fixture(
        "@bg_image num=ev101_1\nFirst[c]\n@wait time=100 canskip=false\nSecond[c]",
        &image,
    );
    let host = TestHost::open(directory.path());
    host.action("new");
    host.action("advance");
    assert_eq!(host.state().flow.wait_ms, 100);
    let before = host.observe();
    assert_eq!(host.image(ID).unwrap(), image);
    assert_eq!(host.observe(), before);
    assert_eq!(unsafe { kr_tick(host.raw, 25) }, 2);
    assert_eq!(host.state().flow.wait_ms, 75); // Read did not enter preview.

    let id = CString::new(ID).unwrap();
    assert_eq!(unsafe { kr_gallery_show(host.raw, id.as_ptr()) }, 1);
    let before = host.observe();
    assert_eq!(host.image(ID).unwrap(), image);
    assert!(host.image(LOCKED).is_err());
    assert_eq!(host.observe(), before);
    assert_eq!(unsafe { kr_tick(host.raw, 25) }, 1);
    assert_eq!(host.observe(), before); // Read did not leave preview.
    assert_eq!(unsafe { kr_gallery_hide(host.raw) }, 1);
    assert_eq!(unsafe { kr_tick(host.raw, 25) }, 2);
    assert_eq!(host.state().flow.wait_ms, 50);
}

#[test]
fn invalid_arguments_fail_with_null_zero_length_and_error() {
    let directory = fixture(SCRIPT, &png([1, 1, 1, 255]));
    let host = TestHost::open(directory.path());
    let id = CString::new(ID).unwrap();
    let before = host.observe();
    unsafe {
        let mut length = usize::MAX;
        assert!(kr_gallery_image(std::ptr::null_mut(), id.as_ptr(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(error().contains("null engine handle"));
        length = usize::MAX;
        assert!(kr_gallery_image(host.raw, std::ptr::null(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(error().contains("null string"));
        length = usize::MAX;
        let invalid_utf8 = [0xffu8, 0];
        assert!(kr_gallery_image(host.raw, invalid_utf8.as_ptr().cast(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(error().contains("invalid utf-8"));
        assert!(kr_gallery_image(host.raw, id.as_ptr(), std::ptr::null_mut()).is_null());
        assert!(error().contains("length pointer"));
    }
    assert_eq!(host.observe(), before);
}
