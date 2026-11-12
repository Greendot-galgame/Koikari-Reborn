mod common;
use koikari_reborn::{events::Database, kag, model::State, story::Engine, tjs, xp3::Store};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

const BRANCH_DB: &str = r#"
lf.EV_EventListInfo = [
 ['intro',0,'End',-1,[-1,0],'Begin'],
 ['late',2,-1,'End',[-1,-1],'Finish']
];
lf.EV_SelectListInfo = [
 ['intro',['late','late'],[-1,-1],[1,1],['Left','Right']]
];
"#;
const BRANCH: &str = r#"
@SL_StartSelect num=0
@s
*SLP_0_0
@SL_EndSelect
@eval exp="f.branch=10"
Left[c]
@SL_Join num=0
*SLP_0_1
@SL_EndSelect
@eval exp="f.branch=20"
Right[c]
@SL_Join num=0
*ENDSLP_0
Joined[c]
@SC_EndProcess
"#;

#[test]
fn real_ffi_skips_only_explicitly_skippable_waits_once() {
    use koikari_reborn::ffi::*;
    use std::ffi::{CStr, CString};
    let (directory, _) = fixture(
        "@wait time=150 canskip=true\nFirst[c]\n@wait time=200 canskip=false\nSecond[c]",
        &[],
    );
    let saves = tempfile::tempdir().unwrap();
    unsafe {
        let handle = kr_open(
            CString::new(directory.path().to_str().unwrap())
                .unwrap()
                .as_ptr(),
            CString::new(saves.path().to_str().unwrap())
                .unwrap()
                .as_ptr(),
        );
        assert!(!handle.is_null());
        let action = |action: &str| kr_action(handle, CString::new(action).unwrap().as_ptr());
        let state = || {
            let pointer = kr_snapshot(handle);
            assert!(!pointer.is_null());
            let value: State = serde_json::from_slice(CStr::from_ptr(pointer).to_bytes()).unwrap();
            kr_string_free(pointer);
            value
        };
        assert_eq!(action("new"), 1);
        assert_eq!(state().flow.wait_ms, 150);
        assert_eq!(action("advance"), 1);
        assert_eq!(state().scene.text, "First");
        assert_eq!(action("advance"), 1);
        let waiting = state();
        assert_eq!(waiting.flow.wait_ms, 200);
        assert_eq!(action("advance"), 0);
        assert_eq!(state(), waiting);
        assert_eq!(kr_tick(handle, 200), 2);
        assert_eq!(state().scene.text, "Second");
        assert_eq!(action("save:1"), 1);
        assert_eq!(action("title"), 1);
        assert_eq!(action("load:1"), 1);
        assert_eq!(state().scene.text, "Second");
        assert_eq!(action("back"), 1);
        assert_eq!(state().scene.text, "First");
        kr_close(handle);
    }
}

#[test]
fn missing_effect_remains_a_visible_recoverable_request_and_restores() {
    let (_directory, mut engine) = fixture("@se play=absent_effect\nVisible[c]", &[]);
    engine.start().unwrap();
    assert_eq!(engine.state.scene.text, "Visible");
    assert!(engine.state.scene.se.missing);
    assert_eq!(engine.state.scene.se.asset, "absent_effect");
    let state = engine.state.clone();
    let mut restored = Engine::open(engine.assets.clone()).unwrap();
    restored.restore(state.clone()).unwrap();
    assert_eq!(restored.state, state);
}

fn fixture(script: &str, files: &[(&str, &[u8])]) -> (tempfile::TempDir, Engine) {
    let directory = common::game(script);
    if !files.is_empty() {
        std::fs::write(
            directory.path().join("patch.xp3"),
            common::archive(files, true, true, 0, false),
        )
        .unwrap();
    }
    let engine = Engine::open(Arc::new(Store::open(directory.path()).unwrap())).unwrap();
    (directory, engine)
}

#[test]
fn database_drives_choices_joins_and_non_numeric_successors() {
    for id in 0..2 {
        let (_directory, mut engine) = fixture(
            BRANCH,
            &[
                ("DB_EV.ks", BRANCH_DB.as_bytes()),
                ("late.ks", b"Later[c]\n@SC_EndProcess"),
            ],
        );
        assert!(engine.choose(0).is_err());
        engine.start().unwrap();
        let pending = engine.state.clone();
        assert_eq!(pending.flow.choices.len(), 2);
        assert!(engine.choose(100).is_err());
        assert_eq!(engine.state, pending);
        assert!(engine.advance().is_err());
        assert_eq!(engine.state, pending);
        assert!(engine.tick(1000).unwrap());
        engine.choose(id).unwrap();
        assert_eq!(
            engine.state.flow.variables["f.branch"],
            json!(if id == 0 { 10 } else { 20 })
        );
        assert!(engine.choose(id).is_err());
        let bytes = serde_json::to_vec(&engine.state).unwrap();
        let saved: State = serde_json::from_slice(&bytes).unwrap();
        let mut resumed = Engine::open(engine.assets.clone()).unwrap();
        resumed.restore(saved.clone()).unwrap();
        assert_eq!(resumed.state, saved);
        resumed.advance().unwrap();
        assert_eq!(resumed.state.scene.text, "Joined");
        resumed.advance().unwrap();
        assert_eq!(resumed.state.scene.text, "Later");
        assert!(!resumed.state.finished);
        resumed.advance().unwrap();
        assert_eq!(resumed.state.flow.ending.as_deref(), Some("route:2"));
        assert!(resumed.state.finished);
    }
}

#[test]
fn conditionals_script_blocks_macros_and_call_frames_survive_restore() {
    let script = r#"
@iscript
f.value=3; if(f.value==3) {f.value+=2;} else {f.value=99;}
@endscript
@if exp="false"
@eval exp="System.exit()"
@unimplemented
@else
@macro name=show
@if exp="int(mp.amount)==5"
@call storage=sub target=*run
@endif
@endmacro
@show amount="&f.value"
@endif
Done[c]
@SC_EndProcess
"#;
    let (_directory, mut engine) = fixture(
        script,
        &[("sub.ks", b"*run\n[emb exp=\"f.value+2\"][c]\n@return")],
    );
    engine.start().unwrap();
    assert_eq!(engine.state.scene.text, "7");
    assert_eq!(engine.state.stack.len(), 1);
    assert_eq!(engine.state.flow.call_conditions.len(), 1);
    assert_eq!(engine.state.flow.macro_stack.len(), 1);
    let saved = serde_json::from_slice(&serde_json::to_vec(&engine.state).unwrap()).unwrap();
    let mut resumed = Engine::open(engine.assets.clone()).unwrap();
    resumed.restore(saved).unwrap();
    resumed.advance().unwrap();
    assert_eq!(resumed.state.scene.text, "Done");
    assert!(resumed.state.stack.is_empty());
    assert!(resumed.state.flow.macro_stack.is_empty());
    assert!(resumed.state.flow.conditions.is_empty());
    resumed.advance().unwrap();
    assert!(resumed.state.finished);
}

#[test]
fn waits_are_serializable_and_never_discard_partial_text() {
    let (_directory, mut engine) = fixture(
        "Before\n@wait time=100 canskip=false\nAfter[c]\n@SC_EndProcess",
        &[],
    );
    engine.start().unwrap();
    assert_eq!(engine.state.scene.text, "Before");
    engine.advance().unwrap();
    assert_eq!(engine.state.flow.wait_ms, 100);
    assert!(engine.tick(40).unwrap());
    let saved = engine.state.clone();
    let mut resumed = Engine::open(engine.assets.clone()).unwrap();
    resumed.restore(saved.clone()).unwrap();
    assert_eq!(resumed.state, saved);
    assert!(!resumed.tick(0).unwrap());
    assert!(resumed.tick(60).unwrap());
    assert_eq!(resumed.state.scene.text, "BeforeAfter");
    resumed.advance().unwrap();
    assert!(resumed.state.finished);
}

#[test]
fn video_completion_is_explicit_and_saved_callback_sequence_is_preserved() {
    let script = "@openvideo storage=movie.mp4\n@playvideo\n@wv canskip=false\n@stopvideo\nAfter[c]\n@SC_EndProcess";
    let (_directory, mut engine) = fixture(script, &[("movie.mp4", b"synthetic video")]);
    engine.start().unwrap();
    let first = engine.state.scene.video.as_ref().unwrap().sequence;
    let saved = engine.state.clone();
    assert!(engine.finish_video(first + 1).is_err());
    assert!(engine.tick(u64::MAX).unwrap());
    assert!(engine.advance().is_err());
    assert_eq!(engine.state.flow, saved.flow);
    assert_eq!(engine.state.scene.video, saved.scene.video);
    assert_eq!(engine.state.scene.visual.elapsed_ms, u64::MAX);
    engine.finish_video(first).unwrap();
    assert_eq!(engine.state.scene.text, "After");
    assert!(engine.finish_video(first).is_err());
    engine.restore(saved).unwrap();
    let fresh = engine.state.scene.video.as_ref().unwrap().sequence;
    assert_eq!(fresh, first);
    assert!(engine.finish_video(first + 1).is_err());
    engine.finish_video(fresh).unwrap();
    assert_eq!(engine.state.scene.text, "After");
    engine.start().unwrap();
    assert!(engine.state.scene.video.as_ref().unwrap().sequence > fresh);
}

#[test]
fn progress_and_replay_unlock_naturally_and_survive_rollback() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::new(2, 2)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let db = "lf.EV_EventListInfo=[['intro',0,'End',-1,[-1,-1],'First'],['late',1,-1,'End',[-1,-1],'Last']];";
    let (_directory, mut engine) = fixture(
        "@bg_image num=ev101_1\nFirst[c]\n@SC_EndProcess",
        &[
            ("DB_EV.ks", db.as_bytes()),
            ("DB_EX.ks", b"lf.EX_SCENEInfo=[[-1,-1,'late']];"),
            (
                "late.ks",
                b"@SC_SceneStartProcess\nLater[c]\n@SC_SceneEnd\n@SC_EndProcess",
            ),
            ("ev101_1.png", png.get_ref()),
            ("ev102_1.png", png.get_ref()),
        ],
    );
    assert!(engine.gallery().unwrap().iter().all(|e| !e.unlocked));
    assert!(engine.start_replay("event:late").is_err());
    engine.start().unwrap();
    assert!(
        engine
            .gallery()
            .unwrap()
            .iter()
            .find(|e| e.id == "ev101")
            .unwrap()
            .unlocked
    );
    assert!(
        !engine
            .gallery()
            .unwrap()
            .iter()
            .find(|e| e.id == "ev102")
            .unwrap()
            .unlocked
    );
    let first = engine.state.clone();
    engine.advance().unwrap();
    assert_eq!(engine.state.scene.text, "Later");
    assert!(engine.state.progress.events.contains("scene:late"));
    engine.advance().unwrap();
    assert!(engine.state.finished);
    engine.restore(first).unwrap();
    assert!(engine.state.progress.events.contains("late"));
    assert!(engine.state.progress.endings.contains("route:1"));
    let before = engine.state.clone();
    engine.start_replay("scene:late").unwrap();
    assert!(engine.start_replay("event:intro").is_err());
    assert_eq!(engine.state.scene.text, "Later");
    engine.advance().unwrap();
    assert_eq!(engine.state, before);
    engine.title().unwrap();
    assert!(engine
        .replay_list()
        .unwrap()
        .iter()
        .filter(|e| e.id == "event:late" || e.id == "scene:late")
        .all(|e| e.unlocked));
}

#[test]
fn legacy_chapter_boundary_migrates_to_next_event_instead_of_false_ending() {
    let db="lf.EV_EventListInfo=[['intro',0,'End',-1,[-1,-1],'First'],['late',3,-1,'End',[-1,-1],'Last']];";
    let (_directory, mut engine) = fixture(
        "First[c]\n@SC_EndProcess",
        &[
            ("DB_EV.ks", db.as_bytes()),
            ("late.ks", b"Later[c]\n@SC_EndProcess"),
        ],
    );
    engine.start().unwrap();
    let mut legacy = engine.state.clone();
    legacy.flow = Default::default();
    legacy.finished = true;
    legacy.cursor.token += 1;
    legacy.warnings.push("legacy preview".into());
    engine.restore(legacy).unwrap();
    assert_eq!(engine.state.cursor.script, "late.ks");
    assert_eq!(engine.state.scene.text, "Later");
    assert!(!engine.state.finished);
    assert!(engine.state.warnings.is_empty());
}

#[test]
fn eof_halt_and_unknown_side_effects_fail_closed() {
    for (script, expected) in [
        ("Unterminated", "unexpected end"),
        ("@s", "halt"),
        (
            "@eval exp=\"f.value=1;System.exit()\"",
            "unsupported side effect/function",
        ),
        ("@unknownvisual", "unsupported command"),
    ] {
        let (_directory, mut engine) = fixture(script, &[]);
        let error = engine.start().unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(error.contains("intro.ks:"));
        assert!(!engine.state.finished);
        assert!(engine.state.flow.ending.is_none());
        assert!(!engine.state.flow.variables.contains_key("f.value"));
    }
}

#[test]
fn changing_or_static_loops_cannot_exhaust_the_native_call_stack() {
    let (_directory, mut engine) =
        fixture("*Loop\n@eval exp=\"f.count++\"\n@jump target=*Loop", &[]);
    let error = engine.start().unwrap_err();
    assert!(
        error.contains("intro.ks:") && error.contains("instruction budget"),
        "{error}"
    );
    assert!(engine.state.blocked.is_some());
    assert!(!engine.state.finished);
}

#[test]
fn tampered_choices_routes_and_execution_frames_are_rejected() {
    let (_directory, mut engine) = fixture(
        BRANCH,
        &[
            ("DB_EV.ks", BRANCH_DB.as_bytes()),
            ("late.ks", b"Last[c]\n@SC_EndProcess"),
        ],
    );
    engine.start().unwrap();
    let saved = engine.state.clone();
    let mut bad = saved.clone();
    bad.flow.choices[0].text = "forged".into();
    assert!(engine.restore(bad).is_err());
    let mut bad = saved.clone();
    bad.flow.route = Some(99);
    assert!(engine.restore(bad).is_err());
    let mut bad = saved.clone();
    bad.cursor.token = usize::MAX;
    assert!(engine.restore(bad).is_err());
    let mut bad = saved.clone();
    bad.flow.selected.insert(0, 99);
    assert!(engine.restore(bad).is_err());
    let mut bad = saved.clone();
    bad.flow.wait_ms = u64::MAX;
    assert!(engine.restore(bad).is_err());
    let mut bad = saved.clone();
    bad.flow.call_conditions.push(Vec::new());
    assert!(engine.restore(bad).is_err());
    assert_eq!(engine.state, saved);
}

#[test]
fn evaluator_updates_nested_arrays_and_has_read_only_conditions_and_limits() {
    let mut vars = BTreeMap::new();
    tjs::execute("f.a=[[1,2]];f.a[0][1]+=4;f.a[1]=[7];", &mut vars).unwrap();
    assert_eq!(tjs::eval("f.a[0][1]+f.a[1][0]", &vars).unwrap(), json!(13));
    assert_eq!(tjs::eval("f.a", &vars).unwrap(), json!([[1, 6], [7]]));
    assert!(tjs::eval("f.a=1", &vars).is_err());
    assert!(tjs::eval("f.a[0][1]++", &vars).is_err());
    assert!(tjs::execute("f.a[-1]=1", &mut vars).is_err());
    assert!(tjs::eval(&vec!["1"; 200].join("+"), &vars).is_err());
    assert!(tjs::eval("missing.native()", &vars).is_err());
}

#[test]
fn global_flags_are_authoritative_before_start_title_and_replay_logic() {
    let script = "@if exp=\"sf.unlocked\"\nUnlocked[c]\n@else\nLocked[c]\n@endif\n@SC_EndProcess";
    let (_directory, mut engine) = fixture(script, &[]);
    engine
        .state
        .progress
        .variables
        .insert("sf.unlocked".into(), json!(1));
    engine.start().unwrap();
    assert_eq!(engine.state.scene.text, "Unlocked");
    engine.title().unwrap();
    engine.start_replay("event:intro").unwrap();
    assert_eq!(engine.state.scene.text, "Unlocked");
    engine.advance().unwrap();
    assert!(engine.state.title);
    engine.start().unwrap();
    assert_eq!(engine.state.scene.text, "Unlocked");
    let mut older = engine.state.clone();
    older
        .progress
        .variables
        .insert("sf.unlocked".into(), json!(0));
    engine.restore(older).unwrap();
    assert_eq!(engine.state.progress.variables["sf.unlocked"], json!(1));
}

#[test]
fn fresh_restore_preserves_every_serialized_field_and_does_not_replay_a_prefix() {
    let script =
        "@openvideo storage=movie.mp4\n@playvideo\n@wv canskip=false\nAfter[c]\n@SC_EndProcess";
    let (_directory, mut engine) = fixture(
        script,
        &[("movie.mp4", b"synthetic"), ("bgm.ogg", b"synthetic")],
    );
    engine.start().unwrap();
    engine
        .state
        .flow
        .variables
        .insert("f.counter".into(), json!(23));
    engine.state.scene.visual.duration_ms = 500;
    engine.state.scene.visual.elapsed_ms = 75;
    engine.state.scene.bgm.asset = "bgm.ogg".into();
    engine.state.scene.bgm.sequence = 45;
    engine.state.scene.bgm.volume = Some(0.3);
    engine.state.scene.bgm.fade_ms = 225;
    engine.state.history.push("Synthetic backlog".into());
    engine.state.warnings.push("Synthetic warning".into());
    let expected = serde_json::from_slice(&serde_json::to_vec(&engine.state).unwrap()).unwrap();
    let mut fresh = Engine::open(engine.assets.clone()).unwrap();
    fresh.restore(expected).unwrap();
    assert_eq!(fresh.state, engine.state);
    assert!(fresh.tick(10).unwrap());
    assert_eq!(fresh.state.scene.visual.elapsed_ms, 85);
}

#[test]
fn absolute_visual_deadlines_and_infinite_motion_never_imply_an_input_wait() {
    let (_directory, mut engine) = fixture("First[c]\nSecond[c]\n@SC_EndProcess", &[]);
    engine.start().unwrap();
    engine.state.scene.visual.elapsed_ms = 200000000;
    for deadline in [0, 100000000, u64::MAX] {
        engine.state.scene.visual.duration_ms = deadline;
        let expected = engine.state.clone();
        let mut fresh = Engine::open(engine.assets.clone()).unwrap();
        fresh.restore(expected.clone()).unwrap();
        assert_eq!(fresh.state, expected);
        assert_eq!(fresh.state.flow.wait_ms, 0);
        fresh.advance().unwrap();
        assert_eq!(fresh.state.scene.text, "Second");
    }
}

#[test]
fn credits_preserve_ordered_formatting_and_timing_without_becoming_dialogue() {
    let script="@staffroll_init\n@staffroll_set storage=credits.ks\n@staffroll_start time=100\n@staffroll_wait canskip=false\n@staffroll_end\nAfter[c]\n@SC_EndProcess";
    let credits="*Start\n[style align=center][font size=26]Synthetic credit[r][style linespacing=12]\n[return]";
    let (_directory, mut engine) = fixture(script, &[("credits.ks", credits.as_bytes())]);
    engine.start().unwrap();
    assert!(engine.state.scene.text.is_empty());
    let roll = engine.state.scene.credits.as_ref().unwrap();
    assert_eq!(roll.text, "Synthetic credit\n");
    assert_eq!(roll.commands.len(), 5);
    assert_eq!(roll.commands[1].args["size"], "26");
    let expected = engine.state.clone();
    let mut fresh = Engine::open(engine.assets.clone()).unwrap();
    fresh.restore(expected.clone()).unwrap();
    assert_eq!(fresh.state, expected);
    assert!(fresh.tick(40).unwrap());
    assert_eq!(fresh.state.scene.credits.as_ref().unwrap().elapsed_ms, 40);
    fresh.advance().unwrap();
    assert_eq!(fresh.state.flow.wait_ms, 60);
    assert!(fresh.tick(60).unwrap());
    assert!(fresh.state.scene.credits.is_none());
    assert_eq!(fresh.state.scene.text, "After");
}

#[test]
fn previously_seen_ending_can_skip_on_input_without_exposing_native_capabilities() {
    let intro = "@call storage=endroll.ks\nAfter[c]\n@SC_EndProcess";
    let endroll="@eval exp=\"tf.ClickCount=kag.clickCount\"\n@staffroll_init\n@staffroll_start time=100\n@wait time=100 canskip=1\n@ED_ClickSkip\nNotSkipped[c]\n*endroll_end\n@staffroll_end\n@return";
    let (_directory, mut engine) = fixture(intro, &[("endroll.ks", endroll.as_bytes())]);
    engine
        .state
        .progress
        .variables
        .insert("sf.ED_Cancel".into(), json!(1));
    engine.start().unwrap();
    assert_eq!(engine.state.flow.wait_ms, 100);
    let saved = engine.state.clone();
    engine.advance().unwrap();
    assert_eq!(engine.state.scene.text, "After");
    engine.restore(saved).unwrap();
    engine.tick(100).unwrap();
    assert_eq!(engine.state.scene.text, "NotSkipped");
    assert!(engine.state.flow.visited.contains("endroll.ks"));
    let mut vars = BTreeMap::new();
    assert!(tjs::execute("kag.clickCount=100", &mut vars).is_err());
    assert!(tjs::eval("kag.application", &vars).is_err());
}

#[test]
#[ignore = "requires authorized private script reference; not real-asset gameplay"]
fn private_effective_tables_cover_all_main_scripts_and_choice_labels() {
    use std::path::Path;
    fn collect(root: &Path, files: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, files);
            } else if path.extension().is_some_and(|e| e == "ks" || e == "tjs") {
                files.insert(
                    path.file_name().unwrap().to_str().unwrap().to_lowercase(),
                    std::fs::read_to_string(path).unwrap(),
                );
            }
        }
    }
    let root = std::env::var("KOIKARI_PRIVATE_REFERENCE")
        .expect("set KOIKARI_PRIVATE_REFERENCE outside the repository");
    let mut files = BTreeMap::new();
    for tier in ["data", "adult", "patch"] {
        collect(&Path::new(&root).join(tier), &mut files);
    }
    let db = Database::parse(&files["db_ev.ks"]).unwrap();
    assert_eq!(db.events.len(), 124);
    assert_eq!(db.selections.len(), 13);
    assert_eq!(db.routes.len(), 8);
    let mut labels = 0;
    for event in &db.events {
        assert!(!kag::parse(&files[&format!("{}.ks", event.script)])
            .unwrap()
            .is_empty());
    }
    for (id, selection) in db.selections.iter().enumerate() {
        let tokens = kag::parse(&files[&format!("{}.ks", selection.script)]).unwrap();
        for choice in 0..selection.text.len() {
            assert!(tokens.iter().any(
                |t| matches!(&t.kind,kag::Kind::Label(l) if l==&format!("SLP_{id}_{choice}"))
            ));
            labels += 1;
        }
    }
    assert_eq!(labels, 31);
}
