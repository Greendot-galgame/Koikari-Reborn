#[allow(dead_code)]
mod common;

use koikari_reborn::media::{clear_video_cache, export_video, prune_video_partials, wav};
use koikari_reborn::xp3::Store;
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use std::fs;
use std::io::Cursor;
use std::sync::Arc;

fn mp4_box(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend(tag);
    out.extend(body);
    out
}

fn movie(version: u8, rate: u32, duration: u64) -> Vec<u8> {
    let mut header = vec![0; if version == 1 { 112 } else { 100 }];
    header[0] = version;
    let offset = if version == 1 { 20 } else { 12 };
    header[offset..offset + 4].copy_from_slice(&rate.to_be_bytes());
    if version == 1 {
        header[24..32].copy_from_slice(&duration.to_be_bytes());
    } else {
        header[16..20].copy_from_slice(&(duration as u32).to_be_bytes());
    }
    let mut bytes = mp4_box(b"ftyp", b"isom\0\0\0\0isommp42");
    bytes.extend(mp4_box(b"moov", &mp4_box(b"mvhd", &header)));
    bytes.extend(mp4_box(
        b"mdat",
        b"synthetic container payload, not a playable movie",
    ));
    bytes
}

fn store(bytes: &[u8], delta: u32, protected: bool) -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("data.xp3"),
        common::archive(
            &[("bgimage/OPmovie.mp4", bytes)],
            true,
            true,
            delta,
            protected,
        ),
    )
    .unwrap();
    let store = Store::open(root.path()).unwrap();
    (root, store)
}

#[test]
fn export_checks_full_xp3_and_reuses_content_without_rewriting() {
    let bytes = movie(0, 1000, 1500);
    let (_source, assets) = store(&bytes, 0, false);
    let cache = tempfile::tempdir().unwrap();
    let path = export_video(&assets, "OPmovie", cache.path()).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(path.extension().unwrap(), "mp4");
    let stem = path.file_stem().unwrap().to_str().unwrap();
    assert!(stem.starts_with("koikari-video-v1-"));
    assert_eq!(stem.len(), "koikari-video-v1-".len() + 64);
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(
        export_video(&assets, "bgimage/OPmovie.mp4", cache.path()).unwrap(),
        path
    );
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    let (_source, corrupt) = store(&bytes, 1, false);
    assert!(export_video(&corrupt, "OPmovie", cache.path()).is_err());
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 1);
}

#[test]
fn overlay_can_fix_corrupt_base_but_base_is_not_silently_accepted() {
    let bytes = movie(1, 1000, 2000);
    let (source, corrupt) = store(&bytes, 1, false);
    let cache = tempfile::tempdir().unwrap();
    assert!(export_video(&corrupt, "OPmovie", cache.path()).is_err());
    fs::write(
        source.path().join("adult.xp3"),
        common::archive(&[("bgimage/OPmovie.mp4", &bytes)], true, true, 0, false),
    )
    .unwrap();
    let overlaid = Store::open(source.path()).unwrap();
    assert_eq!(
        fs::read(export_video(&overlaid, "OPmovie", cache.path()).unwrap()).unwrap(),
        bytes
    );
}

#[test]
fn unsafe_names_protected_entries_and_non_movie_data_fail() {
    let cache = tempfile::tempdir().unwrap();
    let bytes = movie(0, 1000, 1000);
    let (_source, assets) = store(&bytes, 0, false);
    for name in [
        "../OPmovie",
        "/OPmovie",
        "C:\\OPmovie",
        "bgimage/../OPmovie",
        "",
        "a//OPmovie",
        "missing",
    ] {
        assert!(export_video(&assets, name, cache.path()).is_err(), "{name}");
    }
    let (_source, protected) = store(&bytes, 0, true);
    assert!(export_video(&protected, "OPmovie", cache.path()).is_err());
    let (_source, garbage) = store(b"not an mp4", 0, false);
    assert!(export_video(&garbage, "OPmovie", cache.path()).is_err());
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
fn cache_repairs_tampering_and_does_not_reuse_stale_partials() {
    let bytes = movie(0, 1000, 1000);
    let (_source, assets) = store(&bytes, 0, false);
    let cache = tempfile::tempdir().unwrap();
    let stale = cache.path().join(".koikari-video-999999-0.part");
    fs::write(&stale, b"abandoned").unwrap();
    let path = export_video(&assets, "OPmovie", cache.path()).unwrap();
    fs::write(&path, vec![0; bytes.len()]).unwrap();
    assert_eq!(
        export_video(&assets, "OPmovie", cache.path()).unwrap(),
        path
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::write(&path, b"truncated").unwrap();
    export_video(&assets, "OPmovie", cache.path()).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read(&stale).unwrap(), b"abandoned");
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 2);
}

#[test]
fn content_changes_get_distinct_cache_paths_and_directory_collisions_fail() {
    let (_source, first) = store(&movie(0, 1000, 1000), 0, false);
    let (_source, second) = store(&movie(0, 1000, 2000), 0, false);
    let cache = tempfile::tempdir().unwrap();
    let a = export_video(&first, "OPmovie", cache.path()).unwrap();
    let b = export_video(&second, "OPmovie", cache.path()).unwrap();
    assert_ne!(a, b);
    fs::remove_file(&a).unwrap();
    fs::create_dir(&a).unwrap();
    assert!(export_video(&first, "OPmovie", cache.path()).is_err());
    let not_directory = cache.path().join("file");
    fs::write(&not_directory, b"keep").unwrap();
    assert!(export_video(&first, "OPmovie", &not_directory).is_err());
    assert_eq!(fs::read(not_directory).unwrap(), b"keep");
}

#[test]
fn explicit_quiescent_cleanup_removes_only_owned_regular_cache_files() {
    let (_source, assets) = store(&movie(0, 1000, 1000), 0, false);
    let cache = tempfile::tempdir().unwrap();
    let path = export_video(&assets, "OPmovie", cache.path()).unwrap();
    let partial = cache.path().join(".koikari-video-999999-0.part");
    fs::write(&partial, b"abandoned").unwrap();
    let keep = cache.path().join("keep.mp4");
    fs::write(&keep, b"unrelated").unwrap();
    let subdirectory = cache.path().join(".koikari-video-1-1.part");
    fs::create_dir(&subdirectory).unwrap();
    prune_video_partials(cache.path()).unwrap();
    assert!(path.is_file());
    assert!(!partial.exists());
    clear_video_cache(cache.path()).unwrap();
    assert!(!path.exists());
    assert!(!partial.exists());
    assert_eq!(fs::read(&keep).unwrap(), b"unrelated");
    assert!(subdirectory.is_dir());
    clear_video_cache(&cache.path().join("missing")).unwrap();
    assert!(clear_video_cache(&keep).is_err());
}

#[test]
fn oversized_xp3_entry_is_rejected_before_media_allocation() {
    let source = tempfile::tempdir().unwrap();
    let mut archive = common::archive(&[("OPmovie.mp4", b"x")], false, false, 0, false);
    let index = u64::from_le_bytes(archive[11..19].try_into().unwrap()) as usize + 9;
    let info = index + 12 + 12;
    let oversized = 512u64 * 1024 * 1024 + 1;
    archive[info + 4..info + 12].copy_from_slice(&oversized.to_le_bytes());
    let segm = archive
        .windows(4)
        .position(|window| window == b"segm")
        .unwrap()
        + 12;
    archive[segm + 12..segm + 20].copy_from_slice(&oversized.to_le_bytes());
    fs::write(source.path().join("data.xp3"), archive).unwrap();
    let assets = Store::open(source.path()).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let error = export_video(&assets, "OPmovie", cache.path()).unwrap_err();
    assert!(error.contains("memory budget"), "{error}");
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
fn directory_collision_does_not_leave_partial_files() {
    let (_source, assets) = store(&movie(0, 1000, 1000), 0, false);
    let cache = tempfile::tempdir().unwrap();
    let path = export_video(&assets, "OPmovie", cache.path()).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(export_video(&assets, "OPmovie", cache.path()).is_err());
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn symlink_cache_root_and_leaf_are_rejected_without_touching_targets() {
    use std::os::unix::fs::symlink;
    let (_source, assets) = store(&movie(0, 1000, 1000), 0, false);
    let cache = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let root_link = cache.path().join("link");
    symlink(outside.path(), &root_link).unwrap();
    assert!(export_video(&assets, "OPmovie", &root_link).is_err());
    let path = export_video(&assets, "OPmovie", cache.path()).unwrap();
    fs::remove_file(&path).unwrap();
    let target = outside.path().join("keep");
    fs::write(&target, b"untouched").unwrap();
    symlink(&target, &path).unwrap();
    assert!(export_video(&assets, "OPmovie", cache.path()).is_err());
    clear_video_cache(cache.path()).unwrap();
    assert!(fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(clear_video_cache(&root_link).is_err());
    assert_eq!(fs::read(target).unwrap(), b"untouched");
}

#[test]
fn extended_and_end_sized_mp4_boxes_are_bounded_and_supported() {
    let cache = tempfile::tempdir().unwrap();
    let original = movie(0, 1000, 1000);
    let size = u32::from_be_bytes(original[..4].try_into().unwrap()) as usize;
    let mut extended = 1u32.to_be_bytes().to_vec();
    extended.extend(b"ftyp");
    extended.extend(((size + 8) as u64).to_be_bytes());
    extended.extend(&original[8..]);
    let (_source, assets) = store(&extended, 0, false);
    export_video(&assets, "OPmovie", cache.path()).unwrap();
    let mut end_sized = original.clone();
    let mdat = end_sized.windows(4).position(|v| v == b"mdat").unwrap() - 4;
    end_sized[mdat..mdat + 4].copy_from_slice(&0u32.to_be_bytes());
    let (_source, assets) = store(&end_sized, 0, false);
    export_video(&assets, "OPmovie", cache.path()).unwrap();
    let mut duplicate = original.clone();
    duplicate.extend(&original[..size]);
    let (_source, assets) = store(&duplicate, 0, false);
    assert!(export_video(&assets, "OPmovie", cache.path()).is_err());
}

#[test]
fn parallel_exports_are_atomic_and_leave_no_own_partials() {
    let (_source, assets) = store(&movie(0, 1000, 1000), 0, false);
    let assets = Arc::new(assets);
    let cache = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let assets = assets.clone();
                let root = cache.path();
                scope.spawn(move || export_video(&assets, "OPmovie", root).unwrap())
            })
            .collect();
        let paths: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(paths.iter().all(|p| p == &paths[0]));
        assert_eq!(fs::read(&paths[0]).unwrap(), movie(0, 1000, 1000));
    });
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 1);
}

#[test]
fn invalid_mp4_duration_and_malformed_box_lengths_fail() {
    let cache = tempfile::tempdir().unwrap();
    for bytes in [
        movie(0, 0, 1000),
        movie(0, 1000, 0),
        movie(0, 1, 21601),
        movie(1, 1, u64::MAX),
        movie(2, 1000, 1000),
        b"\0\0\0\x01ftyp\xff\xff\xff\xff\xff\xff\xff\xff".to_vec(),
        b"\0\0\0\x07ftyp".to_vec(),
        b"\0\0\0\x01ftyp".to_vec(),
        mp4_box(b"ftyp", b"isom\0\0\0\0isom"),
    ] {
        let (_source, assets) = store(&bytes, 0, false);
        assert!(export_video(&assets, "OPmovie", cache.path()).is_err());
    }
    for version in [0, 1] {
        let (_source, assets) = store(&movie(version, 1, 21600), 0, false);
        export_video(&assets, "OPmovie", cache.path()).unwrap();
    }
}

fn opus_stream(channels: opus::Channels, preskip: u16, granule: u64) -> Vec<u8> {
    let count = if channels == opus::Channels::Mono {
        1
    } else {
        2
    };
    let mut encoder = opus::Encoder::new(48000, channels, opus::Application::Audio).unwrap();
    let packet = encoder.encode_vec(&vec![0; 960 * count], 4000).unwrap();
    let mut head = b"OpusHead".to_vec();
    head.extend([1, count as u8]);
    head.extend(preskip.to_le_bytes());
    head.extend(48000u32.to_le_bytes());
    head.extend([0, 0, 0]);
    rewrap(&[
        (head, 0),
        (b"OpusTags\0\0\0\0\0\0\0\0".to_vec(), 0),
        (packet, granule),
    ])
}

fn rewrap(packets: &[(Vec<u8>, u64)]) -> Vec<u8> {
    let mut writer = PacketWriter::new(Vec::new());
    for (i, (packet, granule)) in packets.iter().enumerate() {
        let ending = if i + 1 == packets.len() {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::EndPage
        };
        writer
            .write_packet(packet.clone(), 7, ending, *granule)
            .unwrap();
    }
    writer.into_inner()
}

fn packets(bytes: &[u8]) -> Vec<(Vec<u8>, u64)> {
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(bytes));
    let mut out = Vec::new();
    while let Some(packet) = reader.read_packet().unwrap() {
        let granule = packet.absgp_page();
        out.push((packet.data, granule));
    }
    out
}

#[test]
fn real_opus_codec_decodes_mono_stereo_and_honors_preskip_and_end_trim() {
    for channels in [opus::Channels::Mono, opus::Channels::Stereo] {
        let count = if channels == opus::Channels::Mono {
            1
        } else {
            2
        };
        let output = wav(&opus_stream(channels, 120, 900)).unwrap();
        assert_eq!(&output[..4], b"RIFF");
        assert_eq!(output.len(), 44 + (900 - 120) * count * 2);
        assert_eq!(
            u32::from_le_bytes(output[24..28].try_into().unwrap()),
            48000
        );
        assert_eq!(
            u16::from_le_bytes(output[22..24].try_into().unwrap()) as usize,
            count
        );
    }
}

#[test]
fn opus_bad_headers_tags_granules_and_duration_are_rejected() {
    for (preskip, granule) in [
        (961, 960),
        (0, 961),
        (0, 0),
        (0, 48000 * 601),
        (0, u64::MAX),
    ] {
        assert!(wav(&opus_stream(opus::Channels::Mono, preskip, granule)).is_err());
    }
    let base = packets(&opus_stream(opus::Channels::Mono, 0, 960));
    for (at, value) in [(8, 0), (8, 16), (9, 0), (9, 3), (18, 1)] {
        let mut bad = base.clone();
        bad[0].0[at] = value;
        assert!(wav(&rewrap(&bad)).is_err());
    }
    let mut bad = base.clone();
    bad[1].0 = b"OpusTags".to_vec();
    assert!(wav(&rewrap(&bad)).is_err());
    bad[1].0 = b"OpusTags\xff\xff\xff\xff\0\0\0\0".to_vec();
    assert!(wav(&rewrap(&bad)).is_err());
    bad[1].0 = b"OpusTags\0\0\0\0\xff\xff\xff\xff".to_vec();
    assert!(wav(&rewrap(&bad)).is_err());
}

#[test]
fn malformed_ogg_crc_truncation_chaining_and_trailing_bytes_fail() {
    let bytes = opus_stream(opus::Channels::Mono, 0, 960);
    for end in [0, 1, 4, 26, bytes.len() - 1] {
        assert!(wav(&bytes[..end]).is_err());
    }
    let mut bad = bytes.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(wav(&bad).is_err());
    let mut bad = bytes.clone();
    bad.extend(&bytes);
    assert!(wav(&bad).is_err());
    let mut bad = bytes.clone();
    bad.extend(b"junk");
    assert!(wav(&bad).is_err());
    let mut bad = bytes;
    bad[5] = 0;
    assert!(wav(&bad).is_err());
    assert!(wav(&vec![0; 64 * 1024 * 1024 + 1]).is_err());
}

fn synthetic_vorbis() -> Vec<u8> {
    decode_hex(SYNTHETIC_VORBIS_HEX)
}

fn decode_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn owned_encoder_generated_mp4_round_trips_strict_xp3_export() {
    let bytes = decode_hex(SYNTHETIC_MP4_HEX);
    let (_source, assets) = store(&bytes, 0, false);
    let cache = tempfile::tempdir().unwrap();
    let path = export_video(&assets, "OPmovie", cache.path()).unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn actual_vorbis_codec_decodes_owned_tone_and_enforces_rate_based_duration() {
    let bytes = synthetic_vorbis();
    let out = wav(&bytes).unwrap();
    assert_eq!(u32::from_le_bytes(out[24..28].try_into().unwrap()), 22050);
    assert_eq!(u16::from_le_bytes(out[22..24].try_into().unwrap()), 2);
    assert_eq!(out.len(), 44 + 1792 * 2 * 2);
    assert!(out[44..].iter().any(|v| *v != 0));
    let mut bad = packets(&bytes);
    bad.last_mut().unwrap().1 = 22050 * 601;
    assert!(wav(&rewrap(&bad)).is_err());
    let mut bad = packets(&bytes);
    bad[0].0[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert!(wav(&rewrap(&bad)).is_err());
}

#[test]
fn malformed_media_probes_do_not_panic_or_create_cache_files() {
    let cache = tempfile::tempdir().unwrap();
    let mut value = 0x12345678u32;
    for size in 0..256 {
        let bytes: Vec<_> = (0..size)
            .map(|_| {
                value ^= value << 13;
                value ^= value >> 17;
                value ^= value << 5;
                value as u8
            })
            .collect();
        assert!(wav(&bytes).is_err());
        let (_source, assets) = store(&bytes, 0, false);
        assert!(export_video(&assets, "OPmovie", cache.path()).is_err());
    }
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "requires separately supplied authorized base and supplemental XP3 resources"]
fn authorized_real_movie_rejects_corrupt_base_and_exports_overlay() {
    let base =
        Store::open(std::env::var_os("KOIKARI_ASSETS").expect("KOIKARI_ASSETS required")).unwrap();
    let patched = Store::open(
        std::env::var_os("KOIKARI_PATCH_ASSETS").expect("KOIKARI_PATCH_ASSETS required"),
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    assert!(export_video(&base, "OPmovie", cache.path()).is_err());
    let exported = export_video(&patched, "OPmovie", cache.path()).unwrap();
    assert!(fs::metadata(&exported).unwrap().len() > 0);
    assert_eq!(
        export_video(&patched, "OPmovie", cache.path()).unwrap(),
        exported
    );
}

#[cfg(target_os = "macos")]
#[test]
fn native_video_controller_lifecycle_without_ui() {
    let directory = tempfile::tempdir().unwrap();
    let harness = directory.path().join("checks.swift");
    let executable = directory.path().join("checks");
    let movie = directory.path().join("owned-black.mp4");
    fs::write(&harness, NATIVE_CONTROLLER_CHECKS).unwrap();
    fs::write(&movie, decode_hex(SYNTHETIC_MP4_HEX)).unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apple/GameVideo.swift");
    let compile = std::process::Command::new("xcrun")
        .args([
            "swiftc",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-target",
            "arm64-apple-macos13.0",
        ])
        .arg(source)
        .arg(&harness)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let result = std::process::Command::new(executable)
        .arg(movie)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[cfg(target_os = "macos")]
const NATIVE_CONTROLLER_CHECKS: &str = r#"
import AVFoundation
import Foundation

@main
struct MediaNativeChecks {
  @MainActor
  static func main() async {
    let url = URL(fileURLWithPath: CommandLine.arguments[1])
    let controller = GameVideoPlayback()
    var completed: [UInt64] = []
    var failures: [String] = []
    func load(_ sequence: UInt64, loop: Bool = false, skip: Bool = true) {
      controller.load(url: url, sequence: sequence, looping: loop, skippable: skip,
        muted: true, volume: 0.5, onCompleted: { completed.append($0) },
        onFailure: { failures.append($0) })
    }
    func drain() async { for _ in 0..<100 { await Task.yield() } }
    load(1, skip: false)
    let original = controller.player!
    assert(original.isMuted && original.volume == 0.5)
    controller.skip()
    assert(completed.isEmpty)
    controller.setOptions(looping: false, skippable: true, muted: false, volume: .nan)
    assert(original.volume == 0 && !original.isMuted)
    controller.setOptions(looping: false, skippable: true, muted: true, volume: 2)
    assert(original.volume == 1)
    controller.setActive(true)
    assert(original.rate == 1)
    controller.setActive(false)
    assert(original.rate == 0)
    controller.setActive(true)
    assert(original.rate == 1)
    controller.setActive(false)
    controller.skip()
    controller.skip()
    assert(completed == [1] && failures.isEmpty && controller.player == nil)
    assert(original.currentItem == nil)
    load(1)
    assert(controller.player == nil)

    load(2)
    let oldItem = controller.player!.currentItem!
    NotificationCenter.default.post(name: AVPlayerItem.didPlayToEndTimeNotification, object: oldItem)
    load(3)
    await drain()
    assert(completed == [1] && failures.isEmpty)
    let newItem = controller.player!.currentItem!
    NotificationCenter.default.post(name: AVPlayerItem.didPlayToEndTimeNotification, object: newItem)
    NotificationCenter.default.post(name: AVPlayerItem.didPlayToEndTimeNotification, object: newItem)
    await drain()
    assert(completed == [1, 3] && failures.isEmpty && controller.player == nil)

    load(4)
    let failedItem = controller.player!.currentItem!
    NotificationCenter.default.post(name: AVPlayerItem.failedToPlayToEndTimeNotification, object: failedItem)
    NotificationCenter.default.post(name: AVPlayerItem.failedToPlayToEndTimeNotification, object: failedItem)
    NotificationCenter.default.post(name: AVPlayerItem.didPlayToEndTimeNotification, object: failedItem)
    await drain()
    assert(failures.count == 1 && completed == [1, 3] && controller.player == nil)
    controller.skip()
    assert(failures.count == 1 && completed == [1, 3])

    load(5, loop: true)
    let loopItem = controller.player!.currentItem!
    NotificationCenter.default.post(name: AVPlayerItem.didPlayToEndTimeNotification, object: loopItem)
    await drain()
    assert(completed == [1, 3])
    controller.skip()
    await drain()
    assert(completed == [1, 3, 5] && controller.player == nil)

    load(6)
    let stoppedItem = controller.player!.currentItem!
    controller.stop()
    NotificationCenter.default.post(name: AVPlayerItem.didPlayToEndTimeNotification, object: stoppedItem)
    await drain()
    assert(completed == [1, 3, 5] && controller.player == nil)

    controller.load(url: URL(string: "https://example.invalid/movie.mp4")!, sequence: 7,
      looping: false, skippable: true, muted: true, volume: 0,
      onCompleted: { completed.append($0) }, onFailure: { failures.append($0) })
    controller.skip()
    assert(failures.count == 2 && completed == [1, 3, 5])
    controller.stop()

    weak var weakController: GameVideoPlayback?
    var disposable: GameVideoPlayback? = GameVideoPlayback()
    weakController = disposable
    disposable!.load(url: url, sequence: 8, looping: false, skippable: true, muted: true,
      volume: 0, onCompleted: { _ in }, onFailure: { _ in })
    disposable = nil
    assert(weakController == nil)
    print("Native controller checks passed: skip guard, terminal-once, stale-generation rejection, loop-no-complete, stop/observer cleanup, volume/mute, inactive pause, local URL validation and deinit. No UI or rendered/audible output asserted.")
  }
}

"#;

// Owned 32x32 black frames generated by FFmpeg, with no audio or game content.
const SYNTHETIC_MP4_HEX: &str = concat!(
    "0000001c6674797069736f6d0000020069736f6d69736f326d7034310000035b6d6f6f760000006c6d766864000000000000",
    "000000000000000003e8000000c8000100000100000000000000000000000001000000000000000000000000000000010000",
    "0000000000000000000000004000000000000000000000000000000000000000000000000000000000000002000002867472",
    "616b0000005c746b68640000000300000000000000000000000100000000000000c800000000000000000000000000000000",
    "0001000000000000000000000000000000010000000000000000000000000000400000000020000000200000000000246564",
    "74730000001c656c73740000000000000001000000c80000000000010000000001fe6d646961000000206d64686400000000",
    "0000000000000000000028000000080055c400000000002d68646c7200000000000000007669646500000000000000000000",
    "0000566964656f48616e646c657200000001a96d696e6600000014766d68640000000100000000000000000000002464696e",
    "660000001c6472656600000000000000010000000c75726c2000000001000001697374626c000000e9737473640000000000",
    "000001000000d96d703476000000000000000100000000000000000000000000000000002000200048000000480000000000",
    "000001124c61766336332e312e313032206d70656734000000000000000000000000000018ffff0000005f65736473000000",
    "00038080804e0001000480808040201100000000030d40000006b8058080802e000001b001000001b5891300000100000001",
    "2000c48d8800550104041443000001b24c61766336332e312e31303206808080010200000010706173700000000100000001",
    "00000014627472740000000000030d40000006b8000000187374747300000000000000010000000200000400000000147374",
    "73730000000000000001000000010000001c7374736300000000000000010000000100000002000000010000001c7374737a",
    "0000000000000000000000020000001f0000000c000000147374636f00000000000000010000038700000061756474610000",
    "00596d657461000000000000002168646c7200000000000000006d6469726170706c0000000000000000000000002c696c73",
    "7400000024a9746f6f0000001c6461746100000001000000004c61766636332e312e3130320000000866726565000000336d",
    "646174000001b3001007000001b610c0a3047b6fe36dbf7f0000c24608f6dfc6db7e000001b651e0477f0000c26f",
);

// Owned 80 ms 440 Hz tone generated by FFmpeg's native Vorbis encoder, not game content.
const SYNTHETIC_VORBIS_HEX: &str = concat!(
    "4f676753000200000000000000005963080d0000000030c70e8d011e01766f72626973000000000222560000000000000000",
    "000000000000bb014f676753000000000000000000005963080d0100000075e0a7580e3bffffffffffffffffffffffffbb03",
    "766f726269730c0000004c61766636332e312e313032010000001b000000656e636f6465723d4c61766336332e312e313032",
    "20766f726269730105766f726269731c424356020010000084749a59aa0122cc408681d0909500000200006084220c312034",
    "642500001000002086928368426bce37e738689683a6526c4e0727526d9ee4a6626ece39e79c73b239678c73ce39a7286716",
    "8366426bce39273168968266426bce39e7496c1eb4a64a6bce39679c733a18678471ce39a7496b1ea466636dce3967416b9a",
    "a3e6526cce3927526e9ed4e6526dce39e79c73ce39e79c73ce39a77a713a07e78473ce39276a6faee5267471ce39e79371ba",
    "37278473ce39e79c73ce39e79c73ce3927080d5909000001001084616318770a82f4391a88518498864c7ad03d3a4c82c620",
    "a7907a343a1a29a50e4249659c94d209424356020080000010424821851452482185145248218518628821869c72ca29a8a0",
    "924a2aaa28a3cc32cb2cb3cc32cb2cb30e3bebacc30e430c31c4d04a2bb1d4545b8d35d69a7bceb9e620ad95d65a6bad9452",
    "4a29a59482d09095000008000081904106196414524821851862ca29a79c820a2a2034642500000600c02167a081061a68a0",
    "81061a68a071c6198820820822a8a4924c3a0a29b5d86acc31d75e830e3af79e7befb9f81c84524a29a594524a29a594524a",
    "2925080d59090080000000082184105248218514528a31c61c730e3a09250442435602006000000c31c41864904148218518",
    "628a29c71c730c3a082194525268a1855c6a882596565a89a5a5986a8bb1d65873ed31d6de7befbdf7de7befbdf7de7bce81",
    "d090950040040000830c228820828c310621048486ac040040000010628831c6208410528821a79c824c32e9a4a39002a121",
    "2b01002700008411472471041267a081082aa920a3cc422cb1b5d65a6badb5d65a6badb5d65a6badb5d65a6badb5d65a6bad",
    "b5d65a6b2d101ab21200880000609041061944104104196480d090950000080000238c400419a514638e39e61874d041271d",
    "85165a2034642500e004004020a18832cc30041155545146155514524729a594524a29a594524a29a594524a29a594524a29",
    "a594524a29a5544a29a504424356020064000090a29452292d458222a518a4184b461573505a8aa8720c52cda952ce20e624",
    "9688318494935432e614420c42ea1c754c29062d951842c618a4d8724ba1730e080d592100846600381c07902c0b902c0b00",
    "000000000000244d0334cf032ccd030000000000000049d300cbd300cdf30000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000009034",
    "0dd03c0fd03c0f0000000000000034cf033c4f043c510400000000000000cbf3004df4004f14010000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "00000000000090340dd03c0fd03c0f000000000000002ccf033c5104344f0400000000000000cbf3004f14014ff400000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "00000000000000000000000400000a3800000a58088586ac0800e204001c920449822441f300926541d3a069304d806459d0",
    "34681a4c130000000000000000000049d3a069d034882240d234681a340da20800000000000000000080a469d034681a4411",
    "20691a340d9a065104000000000000000000c0334d88224411a609f04c13a208518469020000000000000000000000000000",
    "0000000000000000020000091c0000054c28038586ac0800e204001c8e62590000e0388e65010080e3389605000096658922",
    "00005896268a0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000020000091c0000054c28038586ac0400a200001c8a6259c0712c0b388e",
    "650149b22c806501340fa069005104000200000d1c0000056cd094581ca0d09095004014008041712c4bd3449124699ae689",
    "2249d234cf13459ae6799e679af03ccf334d88a2289a264451144d13a6699aaa0a4c53550500001a3800000ad8a029b13840",
    "a1212b018090000087a25896a6799ee789a269aa2649d234cf134551344dd3545592a4699e278aa2689aa6a9aa2c4bd33c4f",
    "1445d134555555a1699e278aa2689aaaaabaf03ccf134551344d55755d789ee789a2289aa6aaba2e4451144dd3345553555d",
    "1788a2699aa6aaaaaaeb02d11345d35455d7755de079a2689aaaeaaaae0b44d3345555555d579601a6699aaaeabab20c5055",
    "55755dd7956580aaaaaaebbaae2c0354d5755d5796651980ebbaae2ccbb2000040040700400123e824a3ca226c34e1c20350",
    "68c88a00200a0000308629c594328c490829848631092185904949a9b4942a08a994544a052195924ac928a5945a4a158454",
    "4a2aa582904a49a51400008be000008b6021141ab21200c80300208c518a31c69c930829c59873ce49849462cc39e7a4528c",
    "39e79c73524ac61c73ce3929a573ce39e79c949239e79c734e4ae99c73ce3927a594d239e79c93524a09a173d04929a574ce",
    "39e70400801a3800000ad828b239c14850a1212b018054000083e35896a6799e289aa625499ae6799e278aa6a94992a6799e",
    "e789a26af23ccf134551344d55e5799e278aa2689aaaca7545d1344d535555972c8ba2699aa6aaba2e4cd33455d5755d17a6",
    "699aaaeabaae0bdb565555755d59866dabaaaabaae2c03d7755d59b66520cbae2bbbb62c00005ec10100d4c086d5114e8ac6",
    "020b0d59090064000010c620a4104248198490420821a51442020000091c0000054c28038586ac04005201000063acb5d65a",
    "6bad35d0596badb5d65a2b20b3d65a6badb5d65a6badb5d65a6badb5d45a6badb5d65a6badb5d65a6badb5d65a6badb5d65a",
    "6badb5d65a6badb5d65a6badb5d65a6badb5d65a6badb5d65a6badb5d65a4b29a594524a29a594524a29a594524a29a59452",
    "0140bf160e00ff1036ac8e705234165868c84a00201c0000304629c61c83504a2915428c39271d95d662ac1062cc390929b5",
    "165bf19c73104a48a5b5188be79c83504a4ab1d558540aa19494528b2dd6a252e8a8a494526b3516634c2aa9b5d662abb118",
    "63520a2db5d6628cc5085b536a2db6da6a2cc6d89a4a0b2dc6186331c217195b8ba9b65a8331c2c8164b4bb5d61a8c3146f7",
    "d662a9ade6628c0fbeb6144b8c351700e0eee00080a860e30c2b496785a3c18586ac0400420200088494628c31c69c73ce39",
    "a914638e39e79c8310422895628c31e79c8310420825638c39e71c84104208a1949231e71c8410420821a4943ae71c841042",
    "082184524ae79c831042082184504ae920841042082184124a292985104208218410422a29a5104208a194104a4825a51442",
    "082184504a0929a5944208a19410422821a594524a2184104229a5a494524aa984524209a184544a4a2985124208a5949252",
    "4a2995524228a184524a4929a59452082184524a010080080e00800246d049469545d868c285072000000004002008911922",
    "51b0000c0e540042c214005058609003000d0e0f691717d065800bbab8eb4008410842108b032820010727dcf0c41b9e7083",
    "13748a4a0d08000000000019007c0000240f4044443473101112131415161718191a1b1c0000800002000010000000000008",
    "00000000104f676753000400070000000000005963080d02000000abd084fd05edff40ffe05ed762a4818d0b0c2184a24e18",
    "ca0809d5b5186960e3024308a1a81386324242e72700c06eb2aa0a00000000000000004466446651ab5621912061c2040912",
    "264c9048c244134924d144134924d144134924d1c413494ca289279298c4134f8cb98589b9d144a357158db999999931615c",
    "82f8847109e213c6c5c5c6c7c4c5c6c7c4c5c64662a2b19198686c2412ed763add6ed369bb4da7ed369db6db74da6ed369bb",
    "4da7ed369df6e5ab97af5ebe7a79e8877ee8877ee8877ee8877e3c7efcf8f1e3c78f1f3f7efcf8f1e3c78f1f3f7efcf8f1e3",
    "68341a8d46a3d168341a8d46a3d168341a8d46a3d168340ade05233806d704d842883d22746f4128101275c1088ec13501b6",
    "10628f08dd5b100a8444e7278004f6182ccb72555515010000a052af91101b25121313381a1f1b063171318ac6278c2a2e91",
    "044134d144e349606122f4969696e6aa51abd72b3a5de0687cac2389248884f109631d898b0b1c1b1f4b242e26706c7cac4e",
    "8f1e4df5ecd9d5f4e8d154b76757d3a3d354b76757d3a3d354b76757d3e9a4badd56d3e9a4badd56d3e9a4da6eabe974526d",
    "b7d5749a54db555f68efd7d78b79adbed0deafe3c5bcf6adbdcf7831af7d6bef335ecc6bdfdafb8c17f3dab7f63ee3c5bcf6",
    "fd7af0a436ee7fec6ff46df0a436ee7fec6ff46df0a436ee7fec6ff46df0a436ee7fec6ff46df0a436ee7fec6ff8d639f1e3",
    "6ac7fe866f9d133fae76ec6ff8d639f1e36ac7fe866f9d133fae76ec6ff875d6be551bfb1b7e9db56fd5029e662338861855",
    "88b1091c429d3034724ab3111c438c2ac4d8040ea14e181a394d2cca9045195116a5a25af9030028ca88b2284559a4a22c6e",
    "3deddc9873f6186334599665595655d51a35166351635094c89045b5521619182b16234654d45aace9a88683d84c9b61b358",
    "edea18c6251a174613241ae7682289c6116f6a945a55aac2a0379546a36a9e7882309a48a2318a4f24611049349158c7251a",
    "e768828411c5278857246182a86312c6391a9730a2d804f18ac427883a26618ca371f111c5c6c52a888f8b3a263ec6614c6c",
    "44b171b10a62634247e2631cc6c436e9f6e8a6e9d9a3ad4ecf4eb53d7a36e9f6e8a6e9d9a3ad4ecf4eb53d7a36e9f6e8a6e9",
    "76daeaf4ec54dbe936e976ba9a6ea7ad4eb7d176ba49b7d36aba9daa4eb7d176ba49db6935dd4e55d36db49d6ed2765a4db7",
    "53d5741b6da79bb49d56d36daa9a6ea3ed7493b6d36aba4d55d36db4aff2a5faea785e7eaff2a5faea785e7eaff2a5faea78",
    "f4ef55bef4abe3d1bf57f9d2af8e47ff5ee54bbf3a1efd7b152ffd4a79f4ef55bcf42be5d1bf57f1d2af9447ef235efa9747",
    "ef235efa9747ef235efa9f3e1da8a4169fcbebe9c703bf528bcfe5abe9c703bf529597e573fa7ae0345579593ea7af074e53",
    "9597e573fa7ae0345579593ea7af074e539597e5731a",
);
