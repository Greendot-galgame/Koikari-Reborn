#[allow(dead_code)]
mod common;
use koikari_reborn::animation::{self, AtlasAnimation, Outcome};
use koikari_reborn::model::{Layer, Scene, HEIGHT, WIDTH};
use std::collections::BTreeMap;

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn call(scene: &mut Scene, name: &str, pairs: &[(&str, &str)]) -> Outcome {
    animation::command(scene, name, &args(pairs)).unwrap()
}
fn layer(asset: &str, group: u32, x: f32, y: f32) -> Layer {
    Layer {
        asset: asset.into(),
        group,
        x,
        y,
        width: WIDTH,
        height: HEIGHT,
        opacity: 1.0,
    }
}
fn scene() -> Scene {
    Scene {
        layers: vec![
            layer("body.png", 1, 100.0, 200.0),
            layer("face.png", 1, 100.0, 200.0),
            layer("other.png", 2, 500.0, 200.0),
        ],
        ..Scene::default()
    }
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}

#[test]
fn source_power_curves_have_correct_endpoints_and_linear_special_cases() {
    for accel in [-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0] {
        close(animation::easing(0.0, accel), 0.0);
        close(animation::easing(1.0, accel), 1.0);
    }
    close(animation::easing(0.5, 2.0), 0.25);
    close(animation::easing(0.5, -2.0), 0.75);
    for accel in [-1.0, 0.0, 1.0] {
        close(animation::easing(0.3, accel), 0.3);
    }
}
#[test]
fn move_interpolates_group_and_face_together_and_commits_endpoint_once() {
    let mut s = scene();
    assert_eq!(
        call(
            &mut s,
            "move",
            &[
                ("layer", "1"),
                ("left", "300"),
                ("top", "400"),
                ("opacity", "0"),
                ("time", "100"),
                ("wait", "true")
            ]
        ),
        Outcome::Wait(100)
    );
    assert_eq!(animation::layers(&s), s.layers);
    animation::tick(&mut s, 50);
    let mid = animation::layers(&s);
    for l in &mid[..2] {
        close(l.x, 200.0);
        close(l.y, 300.0);
        close(l.opacity, 0.5);
    }
    close(mid[2].x, 500.0);
    animation::tick(&mut s, 50);
    let end = s.layers.clone();
    for l in &end[..2] {
        close(l.x, 300.0);
        close(l.y, 400.0);
        close(l.opacity, 0.0);
    }
    animation::tick(&mut s, 100);
    assert_eq!(s.layers, end);
}
#[test]
fn kag_move_path_time_applies_per_segment_and_wait_reports_remaining() {
    let mut s = scene();
    call(
        &mut s,
        "move",
        &[
            ("layer", "1"),
            ("time", "90"),
            ("path", "(190,200,255),(280,290,128)"),
        ],
    );
    assert_eq!(s.visual.duration_ms, 180);
    animation::tick(&mut s, 90);
    close(animation::layers(&s)[0].x, 190.0);
    assert_eq!(call(&mut s, "wm", &[("layer", "1")]), Outcome::Wait(90));
    animation::tick(&mut s, 90);
    close(s.layers[0].x, 280.0);
    close(s.layers[0].opacity, 128.0 / 255.0);
    assert_eq!(call(&mut s, "wm", &[("layer", "1")]), Outcome::Continue);
}
#[test]
fn replacing_move_starts_at_current_position_without_restarting_other_groups() {
    let mut s = scene();
    call(
        &mut s,
        "move",
        &[("layer", "1"), ("left", "300"), ("time", "100")],
    );
    call(&mut s, "t_move", &[("lay", "2"), ("un", "true")]);
    animation::tick(&mut s, 50);
    let before = animation::layers(&s);
    let sequence = s.visual.sequence;
    call(
        &mut s,
        "move",
        &[("layer", "1"), ("left", "400"), ("time", "200")],
    );
    assert!(s.visual.sequence > sequence);
    assert_eq!(animation::layers(&s), before);
    animation::tick(&mut s, 100);
    close(animation::layers(&s)[0].x, 300.0);
    close(animation::layers(&s)[2].y, 207.5);
    animation::tick(&mut s, 100);
    close(s.layers[0].x, 400.0);
}
#[test]
fn authentic_alias_forms_and_resolved_named_forms_agree() {
    for (alias, id, duration) in [
        ("j", "ジャンプ", 260),
        ("pp", "ぷるぷる", 160),
        ("un", "うん", 400),
        ("un2", "うんうん", 800),
        ("yuyu", "ゆらゆらゆっくり", 1200),
        ("sk", "深呼吸", 1600),
    ] {
        let mut a = scene();
        let mut b = scene();
        call(&mut a, "t_move", &[("lay", "1"), (alias, "true")]);
        assert_eq!(
            call(
                &mut b,
                "motion_start",
                &[("layer", "1"), ("name", id), ("wait", "true")]
            ),
            Outcome::Wait(duration)
        );
        for ms in [0, 17, 41, 101, duration] {
            animation::tick(&mut a, ms);
            animation::tick(&mut b, ms);
            assert_eq!(animation::layers(&a), animation::layers(&b));
        }
    }
}
#[test]
fn actual_down_and_up_aliases_follow_definitions_not_english_guesses() {
    for (id, offset) in [
        ("u", 50.0),
        ("uu", 30.0),
        ("dt", -70.0),
        ("l", -50.0),
        ("rr", 40.0),
    ] {
        let mut s = scene();
        call(&mut s, "t_move", &[("lay", "1"), (id, "true")]);
        animation::tick(&mut s, 200);
        if ["l", "rr"].contains(&id) {
            close(s.layers[0].x, 100.0 + offset);
        } else {
            close(s.layers[0].y, 200.0 + offset);
        }
    }
}
#[test]
fn relative_paths_inherit_duration_and_acceleration_and_loops_accumulate() {
    let mut s = scene();
    assert_eq!(
        call(
            &mut s,
            "motion_define",
            &[("name", "synthetic"), ("locatex", "(10,40,2),(r5,,)")]
        ),
        Outcome::Continue
    );
    assert_eq!(
        call(
            &mut s,
            "motion_start",
            &[
                ("name", "synthetic"),
                ("layer", "1"),
                ("loop", "2"),
                ("wait", "true")
            ]
        ),
        Outcome::Wait(160)
    );
    animation::tick(&mut s, 80);
    close(animation::layers(&s)[0].x, 115.0);
    animation::tick(&mut s, 20);
    close(animation::layers(&s)[0].x, 117.5);
    animation::tick(&mut s, 60);
    close(s.layers[0].x, 130.0);
}
#[test]
fn additive_motion_tracks_and_quake_preserve_sprite_registration() {
    let mut s = scene();
    call(&mut s, "t_move", &[("lay", "1"), ("j", "true")]);
    call(&mut s, "t_move", &[("lay", "1"), ("un", "true")]);
    call(&mut s, "quake", &[("time", "300")]);
    animation::tick(&mut s, 65);
    let v = animation::layers(&s);
    close(v[0].x - v[1].x, 0.0);
    close(v[0].y - v[1].y, 0.0);
    assert_ne!(v[2].x, s.layers[2].x);
    call(&mut s, "stopquake", &[]);
    let v = animation::layers(&s);
    close(v[2].x, 500.0);
    close(v[0].y, 193.25);
    animation::tick(&mut s, 1000);
    assert_eq!(s.layers, scene().layers);
}
#[test]
fn infinite_motion_never_auto_completes_and_wait_is_an_error() {
    let mut s = scene();
    call(&mut s, "t_move", &[("lay", "1"), ("ppe", "true")]);
    assert_eq!(s.visual.duration_ms, u64::MAX);
    let before = s.clone();
    assert!(animation::command(&mut s, "motion_wait", &args(&[("layer", "1")])).is_err());
    assert_eq!(s, before);
    animation::tick(&mut s, 1_000_007);
    assert_ne!(animation::layers(&s)[0].x, 100.0);
    let mut endpoint_stop = s.clone();
    call(&mut endpoint_stop, "motion_stop", &[("layer", "1")]);
    assert_eq!(animation::layers(&endpoint_stop), scene().layers);
    call(
        &mut s,
        "motion_stop",
        &[("layer", "1"), ("lastpos", "false")],
    );
    assert_eq!(animation::layers(&s), scene().layers);
}
#[test]
fn serializing_pending_tracks_and_before_layers_restores_identical_future_frames() {
    let mut s = scene();
    call(&mut s, "t_move", &[("lay", "1"), ("jj", "true")]);
    call(&mut s, "t_move", &[("lay", "2"), ("ppe", "true")]);
    call(&mut s, "ts", &[]);
    s.layers[0].asset = "changed.png".into();
    call(&mut s, "te", &[("time", "300")]);
    animation::tick(&mut s, 77);
    let mut restored: Scene = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    animation::validate(&restored).unwrap();
    for delta in [1, 29, 70, 300, 5000] {
        animation::tick(&mut s, delta);
        animation::tick(&mut restored, delta);
        assert_eq!(s, restored);
        assert_eq!(animation::layers(&s), animation::layers(&restored));
    }
}
#[test]
fn tick_partition_does_not_change_endpoints_or_active_motion() {
    let mut a = scene();
    call(&mut a, "t_move", &[("lay", "1"), ("r", "true")]);
    call(&mut a, "t_move", &[("lay", "2"), ("ppe", "true")]);
    let mut b = a.clone();
    animation::tick(&mut a, 375);
    for _ in 0..15 {
        animation::tick(&mut b, 25);
    }
    assert_eq!(a, b);
    assert_eq!(animation::layers(&a), animation::layers(&b));
}
#[test]
fn capture_is_stable_across_nested_asset_loads_and_macro_stoptrans() {
    let mut s = scene();
    let before = s.layers.clone();
    call(&mut s, "ts", &[]);
    s.layers[0].asset = "changed.png".into();
    call(&mut s, "ts", &[]);
    call(&mut s, "stoptrans", &[]);
    assert_eq!(call(&mut s, "te", &[]), Outcome::Wait(200));
    assert_eq!(s.visual.previous, before);
    animation::tick(&mut s, 99);
    assert_eq!(call(&mut s, "wt", &[]), Outcome::Wait(101));
    animation::tick(&mut s, 101);
    assert!(s.visual.previous.is_empty());
    assert_eq!(call(&mut s, "wt", &[]), Outcome::Continue);
}
#[test]
fn whiteout_is_two_six_hundred_ms_phases_and_requires_staged_target() {
    let mut s = scene();
    assert!(animation::command(&mut s, "whiteout", &args(&[("bg", "synthetic")])).is_err());
    call(&mut s, "ts", &[]);
    s.layers[0].asset = "new.png".into();
    assert_eq!(
        call(&mut s, "whiteout", &[("bg", "synthetic")]),
        Outcome::Wait(1200)
    );
    animation::tick(&mut s, 600);
    assert!(!s.visual.previous.is_empty());
    animation::tick(&mut s, 600);
    assert!(s.visual.previous.is_empty());
}
#[test]
fn camera_zoom_pan_preserves_relative_sprite_geometry() {
    let mut s = scene();
    call(
        &mut s,
        "camera",
        &[
            ("zoom", "2"),
            ("left", "100"),
            ("top", "50"),
            ("time", "100"),
        ],
    );
    let before = s.clone();
    assert!(animation::command(&mut s, "t_move", &args(&[("lay", "1"), ("j", "true")])).is_err());
    assert_eq!(s, before);
    animation::tick(&mut s, 50);
    let v = animation::layers(&s);
    close(v[0].width, WIDTH * 1.5);
    close(v[0].x, (100.0 - WIDTH * 0.5) * 1.5 + WIDTH * 0.5 + 50.0);
    close(v[2].x - v[0].x, 600.0);
    animation::tick(&mut s, 50);
    let end = s.layers.clone();
    animation::tick(&mut s, 100);
    assert_eq!(s.layers, end);
}
#[test]
fn unsupported_parameters_and_nonfinite_or_unresolved_values_do_not_mutate_scene() {
    for (name, pairs) in [
        ("move", vec![("left", "NaN")]),
        ("quake", vec![("speed", "5")]),
        ("t_move", vec![("lay", "&mp.lay"), ("j", "true")]),
        ("t_move", vec![("lay", "1"), ("id", "imaginary")]),
        ("move", vec![("path", "(1,2)")]),
        ("te", vec![("method", "imaginary")]),
    ] {
        let mut s = scene();
        let before = s.clone();
        assert!(animation::command(&mut s, name, &args(&pairs)).is_err());
        assert_eq!(s, before);
    }
    let mut s = scene();
    let before = s.clone();
    assert_eq!(call(&mut s, "unrecognized", &[]), Outcome::Unhandled);
    assert_eq!(s, before);
}
#[test]
fn corrupt_runtime_payload_is_rejected_by_save_validation() {
    let mut s = scene();
    call(&mut s, "t_move", &[("lay", "1"), ("j", "true")]);
    s.visual.commands[0].args.insert("data".into(), "{".into());
    assert!(animation::validate(&s).is_err());
}
#[test]
fn restored_runtime_rejects_invalid_styles_definitions_transitions_and_overflow() {
    let mut s = scene();
    call(&mut s, "t_move", &[("lay", "1"), ("j", "true")]);
    call(&mut s, "visual_style", &[("layer", "1"), ("type", "2")]);
    call(
        &mut s,
        "motion_define",
        &[("name", "synthetic"), ("locatex", "(5,10,2)")],
    );
    call(&mut s, "ts", &[]);
    call(&mut s, "te", &[("time", "100")]);
    let runtime: serde_json::Value =
        serde_json::from_str(&s.visual.commands[0].args["data"]).unwrap();
    for pointer in [
        "/styles/1/gamma/0",
        "/definitions/synthetic/loops_x",
        "/transition/kind",
        "/tracks/0/motion/x",
    ] {
        let mut r = runtime.clone();
        *r.pointer_mut(pointer).unwrap() = match pointer {
            "/styles/1/gamma/0" => serde_json::json!(0),
            "/tracks/0/motion/x" => serde_json::json!([
                {"value": 1, "ms": u64::MAX, "accel": 0},
                {"value": 2, "ms": u64::MAX, "accel": 0}]),
            _ => serde_json::json!(10001),
        };
        let mut corrupted = s.clone();
        corrupted.visual.commands[0]
            .args
            .insert("data".into(), r.to_string());
        assert!(animation::validate(&corrupted).is_err(), "{pointer}");
    }
}
#[test]
fn named_motion_wait_and_stop_select_only_matching_tracks() {
    let mut s = scene();
    call(&mut s, "t_move", &[("lay", "1"), ("id", "j")]);
    call(&mut s, "t_move", &[("lay", "2"), ("id", "r")]);
    animation::tick(&mut s, 100);
    assert_eq!(
        call(&mut s, "motion_wait", &[("name", "j")]),
        Outcome::Wait(160)
    );
    call(
        &mut s,
        "motion_stop",
        &[("name", "j"), ("lastpos", "false")],
    );
    assert_eq!(
        call(&mut s, "motion_wait", &[("name", "j")]),
        Outcome::Continue
    );
    assert_eq!(
        call(&mut s, "motion_wait", &[("name", "r")]),
        Outcome::Wait(100)
    );
    close(animation::layers(&s)[0].y, 200.0);
    close(animation::layers(&s)[2].x, 512.5);
}
#[test]
fn synthetic_asd_crops_use_raw_offsets_and_apply_wait_before_next_clip() {
    let a=AtlasAnimation::parse("*start\n@wait time=17\n; @clip left=999\n@clip left=3 top=0\n@wait time=19\n@clip left=6\n@wait time=23\n@clip left=0\n@jump target=*start",[0,0,6,2]).unwrap();
    a.validate_dimensions(20, 2).unwrap();
    assert_eq!(a.duration_ms, 59);
    assert_eq!(a.clip_at(0), [0, 0, 6, 2]);
    assert_eq!(a.clip_at(17), [3, 0, 6, 2]);
    assert_eq!(a.clip_at(36), [6, 0, 6, 2]);
    assert_eq!(a.clip_at(59), [0, 0, 6, 2]);
}
#[test]
fn asd_long_holds_and_reverse_clips_preserve_timing_and_reject_unknown_semantics() {
    let a=AtlasAnimation::parse("*loop\n@wait time=7\n@clip left=4\n@wait time=11\n@wait time=101\n@clip left=0\n@wait time=13\n@jump target=*loop",[0,0,4,4]).unwrap();
    assert_eq!(a.clip_at(118), [4, 0, 4, 4]);
    assert_eq!(a.clip_at(119), [0, 0, 4, 4]);
    assert!(a.validate_dimensions(6, 4).is_err());
    assert!(AtlasAnimation::parse("*x\n@wait time=0\n@jump target=*x", [0, 0, 1, 1]).is_err());
    assert!(AtlasAnimation::parse("*x\n@script exp=1\n@jump target=*x", [0, 0, 1, 1]).is_err());
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod gpu {
    use super::*;
    use koikari_reborn::{renderer::Renderer, xp3::Store};
    use metal::*;
    fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    fn store(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("data.xp3"),
            common::archive(files, true, true, 0, false),
        )
        .unwrap();
        let store = Store::open(d.path()).unwrap();
        (d, store)
    }
    fn target(device: &DeviceRef, width: u64, height: u64) -> Texture {
        let d = TextureDescriptor::new();
        d.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        d.set_width(width);
        d.set_height(height);
        d.set_usage(MTLTextureUsage::RenderTarget);
        d.set_storage_mode(MTLStorageMode::Shared);
        device.new_texture(&d)
    }
    fn pixel(texture: &TextureRef, x: u64, y: u64) -> [u8; 4] {
        let mut out = [0u8; 4];
        texture.get_bytes(out.as_mut_ptr().cast(), 4, MTLRegion::new_2d(x, y, 1, 1), 0);
        out
    }
    fn rgb_near(actual: [u8; 4], rgb: [u8; 3]) {
        let rgba = [actual[2], actual[1], actual[0], actual[3]];
        for i in 0..3 {
            assert!(
                (rgba[i] as i32 - rgb[i] as i32).abs() <= 3,
                "pixel {rgba:?} expected {rgb:?}"
            );
        }
        assert_eq!(actual[3], 255);
    }

    #[test]
    #[ignore = "requires Metal device; explicit headless GPU validation"]
    fn metal_scene_composition_alpha_filters_whiteout_rule_and_letterbox() {
        let device = Device::system_default().expect("Metal device required");
        let mut renderer = Renderer::for_device(device.clone()).unwrap();
        let target = target(&device, 64, 64);
        let red = png(2, 2, |_, _| [255, 0, 0, 255]);
        let blue = png(2, 2, |_, _| [0, 0, 255, 255]);
        let face = png(2, 2, |_, _| [0, 255, 0, 128]);
        let rule = png(16, 2, |x, _| {
            if x < 8 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let (_d, store) = store(&[
            ("red.png", &red),
            ("blue.png", &blue),
            ("face.png", &face),
            ("mask.png", &rule),
        ]);
        let mut s = Scene {
            layers: vec![layer("red.png", 0, 0.0, 0.0)],
            ..Scene::default()
        };
        call(&mut s, "ts", &[]);
        s.layers = vec![
            layer("blue.png", 1, 0.0, 0.0),
            layer("face.png", 1, 0.0, 0.0),
        ];
        call(&mut s, "te", &[("time", "100")]);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 32), [255, 0, 0]);
        animation::tick(&mut s, 50);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 32), [128, 64, 63]);
        // The top letterbox remains the renderer clear color throughout.
        rgb_near(pixel(&target, 32, 2), [5, 6, 10]);
        animation::tick(&mut s, 50);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 32), [0, 128, 127]);
        call(&mut s, "ts", &[]);
        s.layers = vec![layer("red.png", 0, 0.0, 0.0)];
        call(&mut s, "whiteout", &[]);
        animation::tick(&mut s, 600);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 32), [255, 255, 255]);
        animation::tick(&mut s, 300);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 32), [255, 128, 128]);
        animation::tick(&mut s, 300);
        call(&mut s, "ts", &[]);
        s.layers = vec![layer("blue.png", 0, 0.0, 0.0)];
        call(
            &mut s,
            "trans",
            &[
                ("method", "universal"),
                ("rule", "mask"),
                ("vague", "0"),
                ("time", "100"),
            ],
        );
        animation::tick(&mut s, 50);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 8, 32), [0, 0, 255]);
        rgb_near(pixel(&target, 56, 32), [255, 0, 0]);
        animation::tick(&mut s, 50);
        call(&mut s, "visual_style", &[("layer", "0"), ("type", "3")]);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 32), [255, 255, 0]);
        // Type 3 reverses channel endpoints; type 5 adds a blue tint.
        call(&mut s, "visual_style", &[("layer", "0"), ("type", "5")]);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 32), [0, 2, 191]);
        assert!(renderer.cache_usage().0 <= 256 * 1024 * 1024);
    }

    #[test]
    #[ignore = "requires Metal device; explicit headless GPU validation"]
    fn metal_wide_atlas_uploads_only_bounded_raw_pixel_clips_and_resumes_after_save() {
        let device = Device::system_default().expect("Metal device required");
        let mut renderer = Renderer::for_device(device.clone()).unwrap();
        let target = target(&device, 64, 36);
        let wide = png(9000, 2, |x, _| {
            if x < 1000 {
                [255, 0, 0, 255]
            } else if x < 2000 {
                [0, 255, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let asd=b"*loop\n@wait time=7\n@clip left=1000 top=0\n@wait time=11\n@clip left=2000 top=0\n@wait time=13\n@clip left=0\n@jump target=*loop";
        let (_d, store) = store(&[
            ("synthetic.png", &wide),
            ("synthetic.asd", asd),
            ("wide-no-asd.png", &wide),
        ]);
        let mut s = Scene {
            layers: vec![layer("synthetic.png", 0, 0.0, 0.0)],
            ..Scene::default()
        };
        assert!(renderer
            .render_to_texture(&s, &store, &target)
            .unwrap_err()
            .contains("explicit"));
        assert!(
            animation::command(&mut s, "atlas_start", &args(&[("asset", "synthetic.png")]))
                .is_err()
        );
        call(
            &mut s,
            "atlas_start",
            &[
                ("asset", "synthetic.png"),
                ("clipwidth", "4"),
                ("clipheight", "2"),
            ],
        );
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 18), [255, 0, 0]);
        animation::tick(&mut s, 7);
        renderer.render_to_texture(&s, &store, &target).unwrap();
        rgb_near(pixel(&target, 32, 18), [0, 255, 0]);
        let mut restored: Scene = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        animation::tick(&mut restored, 11);
        renderer
            .render_to_texture(&restored, &store, &target)
            .unwrap();
        rgb_near(pixel(&target, 32, 18), [0, 0, 255]);
        // Three 4x2 RGBA frames; the 9000-pixel source never becomes a GPU texture.
        assert_eq!(renderer.cache_usage(), (96, 4));
        let without_atlas_metadata = Scene {
            layers: s.layers.clone(),
            ..Scene::default()
        };
        assert!(renderer
            .render_to_texture(&without_atlas_metadata, &store, &target)
            .unwrap_err()
            .contains("explicit"));
        call(&mut restored, "atlas_stop", &[("asset", "synthetic.png")]);
        animation::tick(&mut restored, 1000);
        call(&mut restored, "atlas_stop", &[("asset", "synthetic.png")]);
        renderer
            .render_to_texture(&restored, &store, &target)
            .unwrap();
        rgb_near(pixel(&target, 32, 18), [0, 0, 255]);
        let bad = Scene {
            layers: vec![layer("wide-no-asd.png", 0, 0.0, 0.0)],
            ..Scene::default()
        };
        assert!(renderer
            .render_to_texture(&bad, &store, &target)
            .unwrap_err()
            .contains("ASD"));
    }

    #[test]
    #[ignore = "requires Metal device and 90 MiB synthetic image; headless validation"]
    fn metal_full_size_flash_dimensions_keep_raw_offsets_and_bounded_frames() {
        let device = Device::system_default().expect("Metal device required");
        let mut renderer = Renderer::for_device(device.clone()).unwrap();
        let target = target(&device, 64, 36);
        // Owned color bands, matching only the reported source dimensions and timing.
        let colors = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
            [255, 0, 255, 255],
        ];
        let sheet = png(15360, 1440, |x, _| colors[((x / 1280) % 5) as usize]);
        let asd = b"*loop\n@wait time=80\n@clip left=1280\n@wait time=80\n@clip left=2560\n@wait time=80\n@clip left=3840\n@wait time=80\n@clip left=5120\n@wait time=80\n@clip left=0\n@jump target=*loop";
        let (_d, store) = store(&[("large.png", &sheet), ("large.asd", asd)]);
        let mut s = Scene {
            layers: vec![layer("large.png", 0, 0.0, 0.0)],
            ..Scene::default()
        };
        call(
            &mut s,
            "atlas_start",
            &[
                ("asset", "large.png"),
                ("clipwidth", "1280"),
                ("clipheight", "1440"),
            ],
        );
        for expected in colors.into_iter().chain([colors[0]]) {
            renderer.render_to_texture(&s, &store, &target).unwrap();
            rgb_near(
                pixel(&target, 32, 18),
                [expected[0], expected[1], expected[2]],
            );
            animation::tick(&mut s, 80);
        }
        assert_eq!(renderer.cache_usage(), (5 * 1280 * 1440 * 4, 1440));
    }
}
