mod common;

use koikari_reborn::xp3::{Assets, Error, Store};

const BASE: &str = "ui/title/TI_base_0.webp";
const LOGO: &str = "ui/title/TI_タイトルロゴ.webp";
const UI_LIMIT: u64 = 8 * 1024 * 1024;

fn oversized_archive(name: &str) -> Vec<u8> {
    // Only one synthetic byte is stored. The declared decompressed size is large.
    let mut bytes = common::archive(&[(name, b"x")], false, true, 0, false);
    let info = bytes.windows(4).position(|v| v == b"info").unwrap() + 12;
    let segm = bytes.windows(4).position(|v| v == b"segm").unwrap() + 12;
    let size = (UI_LIMIT + 1).to_le_bytes();
    bytes[info + 4..info + 12].copy_from_slice(&size);
    bytes[segm + 12..segm + 20].copy_from_slice(&size);
    bytes
}

#[test]
fn limited_store_reads_at_cap_and_keeps_normal_resolution_and_patch_priority() {
    for packed in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        for (archive, value) in [
            ("data.xp3", b"base".as_slice()),
            ("adult.xp3", b"supplement"),
            ("patch.xp3", b"patch"),
            ("patch2.xp3", b"two"),
            ("patch10.xp3", b"ten"),
            ("patch50.xp3", b"fifty"),
        ] {
            std::fs::write(
                directory.path().join(archive),
                common::archive(&[(BASE, value), (LOGO, b"")], packed, packed, 0, false),
            )
            .unwrap();
            let store = Store::open(directory.path()).unwrap();
            for name in [BASE, "UI\\TITLE\\TI_BASE_0.WEBP", "TI_base_0.webp"] {
                assert_eq!(
                    store.bytes_limited(name, value.len() as u64).unwrap(),
                    value
                );
                assert!(matches!(
                    store.bytes_limited(name, value.len() as u64 - 1),
                    Err(Error::Invalid(message)) if message.contains("byte limit")
                ));
                assert_eq!(store.bytes(name).unwrap(), value);
            }
            assert_eq!(store.bytes_limited(LOGO, 0).unwrap(), b"");
        }
    }
}

#[test]
fn limited_store_checks_decompressed_cap_before_opening_or_reading_archive() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("data.xp3"), oversized_archive(BASE)).unwrap();
    let store = Store::open(directory.path()).unwrap();
    // Removing only the temporary fixture makes any attempt to read it fail with Io.
    directory.close().unwrap();
    assert!(matches!(
        store.bytes_limited(BASE, UI_LIMIT),
        Err(Error::Invalid(message)) if message.contains("byte limit")
    ));
    assert!(matches!(store.bytes(BASE), Err(Error::Io(_))));
}

#[test]
fn limited_store_keeps_strict_errors_without_falling_back_to_base() {
    for (delta, protected) in [(1, false), (0, true)] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("data.xp3"),
            common::archive(&[(BASE, b"base")], true, true, 0, false),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("patch.xp3"),
            common::archive(&[(BASE, b"bad patch")], true, true, delta, protected),
        )
        .unwrap();
        let store = Store::open(directory.path()).unwrap();
        assert_eq!(
            store.bytes_limited(BASE, UI_LIMIT).unwrap_err().to_string(),
            store.bytes(BASE).unwrap_err().to_string()
        );
        assert!(matches!(
            store.bytes_limited(LOGO, UI_LIMIT),
            Err(Error::Missing(_))
        ));
        assert!(matches!(
            store.bytes_limited("../title", UI_LIMIT),
            Err(Error::Invalid(_))
        ));
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod bridge {
    use super::*;
    use koikari_reborn::ffi::*;
    use std::ffi::{CStr, CString};
    use std::path::Path;

    struct TestHost {
        raw: *mut Host,
        _saves: tempfile::TempDir,
    }
    impl TestHost {
        fn open(resources: &Path) -> Self {
            let saves = tempfile::tempdir().unwrap();
            let directory = CString::new(resources.to_str().unwrap()).unwrap();
            let save_directory = CString::new(saves.path().to_str().unwrap()).unwrap();
            let raw = unsafe { kr_open(directory.as_ptr(), save_directory.as_ptr()) };
            assert!(!raw.is_null(), "{}", error());
            Self { raw, _saves: saves }
        }
        fn ui(&self, name: &str) -> Result<Vec<u8>, String> {
            let name = CString::new(name).unwrap();
            let mut length = usize::MAX;
            unsafe {
                let bytes = kr_ui_asset(self.raw, name.as_ptr(), &mut length);
                if bytes.is_null() {
                    assert_eq!(length, 0);
                    let message = error();
                    assert!(!message.is_empty());
                    Err(message)
                } else {
                    let result = std::slice::from_raw_parts(bytes, length).to_vec();
                    kr_bytes_free(bytes, length);
                    assert!(error().is_empty());
                    Ok(result)
                }
            }
        }
        fn snapshot(&self) -> String {
            unsafe {
                let json = kr_snapshot(self.raw);
                assert!(!json.is_null());
                let result = CStr::from_ptr(json).to_str().unwrap().to_owned();
                kr_string_free(json);
                result
            }
        }
        fn action(&self, name: &str) -> i32 {
            unsafe { kr_action(self.raw, CString::new(name).unwrap().as_ptr()) }
        }
    }
    impl Drop for TestHost {
        fn drop(&mut self) {
            unsafe { kr_close(self.raw) }
        }
    }
    fn error() -> String {
        unsafe {
            let text = kr_error();
            let result = CStr::from_ptr(text).to_str().unwrap().to_owned();
            kr_string_free(text);
            result
        }
    }

    #[test]
    fn ui_asset_reads_both_exact_names_and_patch_without_changing_gameplay() {
        let directory = common::game("First[c]\n@unsupported");
        for (archive, value) in [("patch2.xp3", b"old"), ("patch10.xp3", b"new")] {
            std::fs::write(
                directory.path().join(archive),
                common::archive(
                    &[(BASE, value), (LOGO, b"synthetic logo")],
                    true,
                    true,
                    0,
                    false,
                ),
            )
            .unwrap();
        }
        let host = TestHost::open(directory.path());
        for state in ["title", "reading", "blocked"] {
            match state {
                "reading" => assert_eq!(host.action("new"), 1),
                "blocked" => assert_eq!(host.action("advance"), 0),
                _ => (),
            }
            let before = host.snapshot();
            assert_eq!(host.ui(BASE).unwrap(), b"new");
            assert_eq!(host.ui(LOGO).unwrap(), b"synthetic logo");
            assert!(host
                .ui("intro.ks")
                .unwrap_err()
                .contains("unsupported UI asset"));
            assert_eq!(host.snapshot(), before, "{state}");
        }
    }

    #[test]
    fn ui_asset_rejects_unknown_names_and_path_aliases_but_generic_asset_is_unchanged() {
        let directory = common::game("Synthetic[c]");
        let host = TestHost::open(directory.path());
        let before = host.snapshot();
        for name in [
            "",
            "intro.ks",
            "TI_base_0.webp",
            "ui/title/ti_base_0.webp",
            "UI/title/TI_base_0.webp",
            "ui\\title\\TI_base_0.webp",
            "./ui/title/TI_base_0.webp",
            "ui/title/../title/TI_base_0.webp",
            "/ui/title/TI_base_0.webp",
            "ui//title/TI_base_0.webp",
            "ui/title/TI_base_0.webp/extra",
            "ui/title/TI_base_1.webp",
        ] {
            assert_eq!(
                host.ui(name).unwrap_err(),
                "unsupported UI asset name",
                "{name}"
            );
        }
        assert!(host.ui(LOGO).unwrap_err().contains("Asset not found"));
        assert_eq!(host.ui(BASE).unwrap(), b"synthetic title");
        unsafe {
            let mut length = 0;
            let name = CString::new("intro.ks").unwrap();
            let bytes = kr_asset(host.raw, name.as_ptr(), &mut length);
            assert!(!bytes.is_null());
            assert_eq!(std::slice::from_raw_parts(bytes, length), b"Synthetic[c]");
            kr_bytes_free(bytes, length);
        }
        assert_eq!(host.snapshot(), before);
    }

    #[test]
    fn ui_asset_protected_corrupt_and_oversized_patches_fail_closed() {
        for (archive, expected) in [
            (
                common::archive(&[(BASE, b"patch")], true, true, 0, true),
                "Protected",
            ),
            (
                common::archive(&[(BASE, b"patch")], true, true, 1, false),
                "checksum",
            ),
            (oversized_archive(BASE), "byte limit"),
        ] {
            let directory = common::game("Synthetic[c]");
            std::fs::write(directory.path().join("patch.xp3"), archive).unwrap();
            let host = TestHost::open(directory.path());
            let before = host.snapshot();
            assert!(host.ui(BASE).unwrap_err().contains(expected), "{expected}");
            assert_eq!(host.snapshot(), before);
        }
    }

    #[test]
    fn ui_asset_invalid_arguments_return_null_and_report_errors() {
        let directory = common::game("Synthetic[c]");
        let host = TestHost::open(directory.path());
        let name = CString::new(BASE).unwrap();
        unsafe {
            let mut length = usize::MAX;
            assert!(kr_ui_asset(std::ptr::null_mut(), name.as_ptr(), &mut length).is_null());
            assert_eq!(length, 0);
            assert!(error().contains("null engine handle"));
            length = usize::MAX;
            assert!(kr_ui_asset(host.raw, std::ptr::null(), &mut length).is_null());
            assert_eq!(length, 0);
            assert!(error().contains("null string"));
            assert!(kr_ui_asset(host.raw, name.as_ptr(), std::ptr::null_mut()).is_null());
            assert!(error().contains("length pointer"));
        }
    }
}
