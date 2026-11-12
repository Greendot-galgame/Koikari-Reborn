//! Owned synthetic scenarios. Authentic checks require explicit ignored-test opt-in and XP3 paths.
mod common;
use koikari_reborn::{
    coverage::{self, Action, Audit, Driver, Limits},
    model::{Choice, Cursor, GalleryEntry, State, Video},
    xp3::Store,
};
use std::collections::BTreeMap;
use std::sync::Arc;

const INTRO: &str = "scenario/main/intro.ks";
const A: &str = "scenario/main/a.ks";
const B: &str = "scenario/main/b.ks";
fn sources(extra: bool, replay: bool) -> BTreeMap<String, String> {
    let mut sources: BTreeMap<String, String> = [
        (INTRO, "[SL_StartSelect num=0]\n[s]\n*SLP_0_0\n[SL_EndSelect]\n[SL_Join]\n*SLP_0_1\n[SL_EndSelect]\n[SL_Join]\n*ENDSLP_0"),
        (A, "[wait time=5]\n[playvideo]\n[SC_EndProcess]"),
        (B, "[SC_EndProcess]"),
        ("scenario/db/DB_EV.ks", "\n// lf.EV_EventListInfo = [] is a comment\nlf.EV_EventListInfo = [\n['intro',0,'End',-1,[-1,0],'synthetic title'],\n['a',1,'intro','End',[0,-1],'synthetic title'],\n['b',2,'intro','End',[0,-1],'synthetic title'],\n];\nlf.EV_SelectListInfo = [['intro',['a','b',''],-1,['E','E',''],['LEFT PRIVATE TEXT','RIGHT PRIVATE TEXT']]];\nlf.EV_RouteInfo = [['private route','End','Next'],['private route','intro','End'],['private route','intro','End']];"),
        ("scenario/db/DB_EX.ks", "lf.EX_SCENEInfo = [];"),
    ].into_iter().map(|(n,s)|(n.into(),s.into())).collect();
    if extra {
        sources.insert(
            "scenario/main/unreachable.ks".into(),
            "[SC_EndProcess]".into(),
        );
    }
    if replay {
        sources.insert(
            "scenario/db/DB_EX.ks".into(),
            "lf.EX_SCENEInfo = [[tf.EX_Flag,'101','a','']];".into(),
        );
    }
    sources
}
fn audit() -> Audit {
    coverage::audit_sources(&sources(false, false))
}
fn limits() -> Limits {
    Limits {
        max_steps: 1000,
        tick_ms: 2,
        ..Limits::default()
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Branch,
    Loop,
    CounterLoop,
    CorruptRestore,
    Unsupported,
    FakeFinish,
    WrongTerminal,
    ProgressReplay,
    FrozenTimer,
    LoopingVideo,
    Disabled,
    PartDisabled,
    ChoiceCycle,
    StuckVideo,
    LostProgress,
    ReplayNoop,
    ReplayTitle,
    InfiniteBackgroundVisual,
    FiniteBackgroundVisual,
    ClockAdvancingFrozenTimer,
}
fn records_progress(mode: Mode) -> bool {
    matches!(
        mode,
        Mode::ProgressReplay | Mode::LostProgress | Mode::ReplayNoop | Mode::ReplayTitle
    )
}
struct Fake {
    state: State,
    mode: Mode,
    log: Vec<Action>,
}
impl Fake {
    fn new(mode: Mode) -> Self {
        Self {
            state: State::default(),
            mode,
            log: Vec::new(),
        }
    }
    fn terminal(&mut self) {
        let ending = if self.state.cursor.script == A {
            "end-a"
        } else {
            "end-b"
        };
        self.state.finished = true;
        self.state.flow.ending = Some(ending.into());
        if records_progress(self.mode) && !self.state.flow.replay {
            self.state.progress.endings.insert(ending.into());
        }
        self.state.cursor.token = if self.state.cursor.script == A { 3 } else { 1 };
    }
}
impl Driver for Fake {
    fn state(&self) -> &State {
        &self.state
    }
    fn start(&mut self) -> Result<(), String> {
        let progress = self.state.progress.clone();
        self.state = State {
            title: false,
            cursor: Cursor {
                script: INTRO.into(),
                token: 1,
            },
            ..State::default()
        };
        if records_progress(self.mode) && self.mode != Mode::LostProgress {
            self.state.progress = progress;
        }
        self.state.flow.variables.insert(
            "nested-state".into(),
            serde_json::json!({"array":[1,{"flag":true}]}),
        );
        self.state.scene.text = "PRIVATE DIALOGUE MUST NEVER LEAK".into();
        self.state
            .history
            .push("PRIVATE HISTORY MUST NEVER LEAK".into());
        self.state.flow.visited.insert(INTRO.into());
        if matches!(
            self.mode,
            Mode::InfiniteBackgroundVisual
                | Mode::FiniteBackgroundVisual
                | Mode::ClockAdvancingFrozenTimer
        ) {
            // The visual module's monotonic clock/deadline describes asynchronous work,
            // including wait=false motion. Only flow.wait_ms blocks scenario input.
            self.state.scene.visual.elapsed_ms = 37;
            self.state.scene.visual.duration_ms = if self.mode == Mode::FiniteBackgroundVisual {
                537
            } else {
                u64::MAX
            };
        }
        if !matches!(
            self.mode,
            Mode::Loop
                | Mode::CounterLoop
                | Mode::Unsupported
                | Mode::FakeFinish
                | Mode::WrongTerminal
        ) {
            self.state.flow.selection_id = Some(0);
            self.state.flow.choices = vec![
                Choice {
                    id: 0,
                    text: "PRIVATE CHOICE".into(),
                    enabled: self.mode != Mode::Disabled,
                },
                Choice {
                    id: 1,
                    text: "PRIVATE CHOICE".into(),
                    enabled: self.mode != Mode::Disabled && self.mode != Mode::PartDisabled,
                },
            ];
        }
        Ok(())
    }
    fn restore(&mut self, state: State) -> Result<(), String> {
        self.state = state;
        if self.mode == Mode::CorruptRestore {
            self.state.flow.variables.clear();
        }
        Ok(())
    }
    fn advance(&mut self) -> Result<(), String> {
        self.log.push(Action::Advance);
        if self.mode == Mode::WrongTerminal {
            self.terminal();
            return Ok(());
        }
        if self.mode == Mode::ReplayTitle && self.state.flow.replay {
            self.state.title = true;
            return Ok(());
        }
        match self.mode {
            Mode::Loop => {
                self.state.cursor.token = if self.state.cursor.token == 1 { 2 } else { 1 };
                return Ok(());
            }
            Mode::CounterLoop => {
                self.state.pages += 1;
                return Ok(());
            }
            Mode::Unsupported => {
                return Err(format!(
                    "{INTRO}:1: unsupported PRIVATE DIALOGUE MUST NEVER LEAK"
                ))
            }
            Mode::FakeFinish => {
                self.state.finished = true;
                return Ok(());
            }
            _ => (),
        }
        assert!(self.state.flow.choices.is_empty());
        assert!(self.state.flow.wait_ms == 0 && self.state.scene.video.is_none());
        self.state.pages += 1;
        self.terminal();
        Ok(())
    }
    fn choose(&mut self, id: usize) -> Result<(), String> {
        assert!(self
            .state
            .flow
            .choices
            .iter()
            .any(|c| c.id == id && c.enabled));
        self.log.push(Action::Choose { id });
        if self.mode == Mode::ChoiceCycle {
            self.state.cursor.token = if self.state.cursor.token == 1 { 2 } else { 1 };
            return Ok(());
        }
        self.state.flow.choices.clear();
        self.state.flow.selection_id = None;
        self.state.cursor = Cursor {
            script: if id == 0 { A } else { B }.into(),
            token: 1,
        };
        self.state.flow.event_id = Some(if id == 0 { 1 } else { 2 });
        self.state
            .flow
            .visited
            .insert(self.state.cursor.script.clone());
        if id == 0 {
            self.state.flow.wait_ms = 5;
            self.state.scene.video = Some(Video {
                asset: "synthetic.mp4".into(),
                sequence: 77,
                looping: self.mode == Mode::LoopingVideo,
                skippable: false,
            });
        }
        Ok(())
    }
    fn tick(&mut self, milliseconds: u64) -> Result<bool, String> {
        if matches!(
            self.mode,
            Mode::InfiniteBackgroundVisual
                | Mode::FiniteBackgroundVisual
                | Mode::ClockAdvancingFrozenTimer
        ) {
            assert!(
                self.state.flow.wait_ms > 0,
                "background visual work must not trigger verifier ticks"
            );
            self.state.scene.visual.elapsed_ms = self
                .state
                .scene
                .visual
                .elapsed_ms
                .saturating_add(milliseconds);
        }
        self.log.push(Action::Tick { milliseconds });
        if !matches!(
            self.mode,
            Mode::FrozenTimer | Mode::ClockAdvancingFrozenTimer
        ) {
            self.state.flow.wait_ms = self.state.flow.wait_ms.saturating_sub(milliseconds);
        }
        Ok(true)
    }
    fn finish_video(&mut self, sequence: u64) -> Result<(), String> {
        self.log.push(Action::FinishVideo { sequence });
        assert_eq!(self.state.scene.video.as_ref().unwrap().sequence, sequence);
        if self.mode == Mode::StuckVideo {
            self.state.pages += 1;
            return Ok(());
        }
        self.state.scene.video = None;
        Ok(())
    }
    fn gallery(&self) -> Result<Vec<GalleryEntry>, String> {
        Ok(Vec::new())
    }
    fn replay_list(&self) -> Result<Vec<GalleryEntry>, String> {
        if !records_progress(self.mode) {
            return Ok(Vec::new());
        }
        Ok(vec![GalleryEntry {
            id: "bonus-a".into(),
            unlocked: self.state.progress.endings.len() == 2,
            ..GalleryEntry::default()
        }])
    }
    fn start_replay(&mut self, id: &str) -> Result<(), String> {
        assert_eq!(id, "bonus-a");
        assert_eq!(
            self.state.progress.endings.len(),
            2,
            "no artificial all-unlocked state"
        );
        if self.mode == Mode::ReplayNoop {
            return Ok(());
        }
        self.state.finished = false;
        self.state.flow.ending = None;
        self.state.flow.replay = true;
        self.state.cursor = Cursor {
            script: A.into(),
            token: 1,
        };
        self.state.flow.event_id = Some(1);
        Ok(())
    }
}
fn run(mode: Mode) -> coverage::Report {
    coverage::verify_with_driver(
        &mut Fake::new(mode),
        audit(),
        "synthetic-fingerprint".into(),
        limits(),
    )
    .unwrap()
}

#[test]
fn audits_database_edges_and_tags_without_dialogue() {
    let audit = audit();
    assert!(audit.issues.is_empty(), "{:?}", audit.issues);
    assert_eq!(audit.main_scripts.len(), 3);
    assert_eq!(audit.selections[0].choices, 2);
    assert_eq!(audit.expected_endings.len(), 2);
    assert!(
        !audit.expected_endings.contains(INTRO),
        "route predecessor is not an ending"
    );
    assert_eq!(
        audit
            .edges
            .iter()
            .filter(|e| e.kind == "selection_event_jump")
            .count(),
        2
    );
    let json = serde_json::to_string(&audit).unwrap();
    assert!(
        !json.contains("PRIVATE")
            && !json.contains("private route")
            && !json.contains("synthetic title")
    );
    assert!(!format!("{audit:?}").contains("PRIVATE"));
}
#[test]
fn legal_branches_restore_and_complete_timers_video() {
    let report = run(Mode::Branch);
    assert!(report.complete, "{:?}", report.failures);
    assert_eq!(report.choices, 2);
    assert_eq!(report.restored_states, 2);
    assert_eq!(report.timers_completed, 1);
    assert_eq!(report.videos_completed, 1);
    assert_eq!(report.endings.len(), 2);
    assert!(report.unreachable.is_empty() && report.untraversed_choices.is_empty());
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("PRIVATE") && !json.contains("private route"));
}

fn background_visual_does_not_block(mode: Mode) {
    let mut driver = Fake::new(mode);
    let report =
        coverage::verify_with_driver(&mut driver, audit(), "asynchronous-visual".into(), limits())
            .unwrap();
    assert!(report.complete, "{:?}", report.failures);
    assert_eq!(
        report.choices, 2,
        "pending choices must remain usable with active background work"
    );
    assert_eq!(
        driver
            .log
            .iter()
            .filter(|a| matches!(a, Action::Advance))
            .count(),
        2,
        "both story branches must advance"
    );
    let ticks: Vec<_> = driver
        .log
        .iter()
        .filter_map(|a| {
            if let Action::Tick { milliseconds } = a {
                Some(*milliseconds)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        ticks,
        vec![2, 2, 1],
        "tick only the explicit five-millisecond flow wait"
    );
    assert_eq!(
        report.timers_completed, 1,
        "a released flow wait completes while visuals remain active"
    );
    assert_eq!(report.videos_completed, 1);
    assert!(
        driver.state.scene.visual.elapsed_ms < driver.state.scene.visual.duration_ms,
        "story completion must not drain the background animation"
    );
}
#[test]
fn infinite_asynchronous_visual_does_not_block_choices_or_story() {
    background_visual_does_not_block(Mode::InfiniteBackgroundVisual);
}
#[test]
fn finite_wait_false_visual_does_not_block_choices_or_story() {
    background_visual_does_not_block(Mode::FiniteBackgroundVisual);
}
#[test]
fn an_advancing_visual_clock_cannot_hide_a_stalled_flow_wait() {
    let report = run(Mode::ClockAdvancingFrozenTimer);
    assert!(!report.complete);
    assert!(
        !report.budget_exhausted,
        "detect the stalled wait independently of background clock motion"
    );
    assert!(report
        .blockers
        .iter()
        .any(|b| b.code == "timer_did_not_progress"));
    assert_eq!(report.timers_completed, 0);
}
#[test]
fn repeated_and_changing_state_loops_are_bounded() {
    let cycle = run(Mode::Loop);
    assert!(!cycle.complete);
    assert_eq!(cycle.first_blocker.unwrap().code, "cycle_without_terminal");
    let report = coverage::verify_with_driver(
        &mut Fake::new(Mode::CounterLoop),
        audit(),
        "fixture".into(),
        Limits {
            max_steps: 8,
            ..limits()
        },
    )
    .unwrap();
    assert!(report.budget_exhausted && !report.complete);
    assert_eq!(report.operations, 8);
    assert!(report
        .gaps
        .iter()
        .all(|g| g.reason == "not_proven_within_budget"));
}
#[test]
fn restore_must_preserve_all_serialized_execution_fields() {
    let report = run(Mode::CorruptRestore);
    assert!(!report.complete);
    assert!(report.blockers.iter().all(|b| b.code == "restore_mismatch"));
    assert_eq!(report.choices, 0);
}
#[test]
fn unreachable_expectations_are_listed_individually() {
    let report = coverage::verify_with_driver(
        &mut Fake::new(Mode::Branch),
        coverage::audit_sources(&sources(true, false)),
        "fixture".into(),
        limits(),
    )
    .unwrap();
    assert_eq!(
        report.unreachable.iter().cloned().collect::<Vec<_>>(),
        vec!["scenario/main/unreachable.ks"]
    );
    assert!(!report.complete);
    assert!(report
        .gaps
        .iter()
        .any(|g| g.item == "script:scenario/main/unreachable.ks"));
}
#[test]
fn unsupported_errors_are_metadata_only_and_reproducible() {
    let report = run(Mode::Unsupported);
    let b = report.first_blocker.as_ref().unwrap();
    assert_eq!(b.code, "unsupported_instruction");
    assert_eq!(b.site.script, INTRO);
    assert_eq!(b.site.line, Some(1));
    assert_eq!(b.site.tag.as_deref(), Some("sl_startselect"));
    assert!(b.error_sha256.is_some());
    assert!(!serde_json::to_string(&report).unwrap().contains("PRIVATE"));
}
#[test]
fn finished_flag_alone_does_not_establish_an_ending() {
    let report = run(Mode::FakeFinish);
    assert_eq!(
        report.first_blocker.unwrap().code,
        "finished_without_genuine_ending"
    );
    assert_eq!(report.unreachable_endings.len(), 2);
}

#[test]
fn an_unproven_ending_never_queries_unlock_menus() {
    let report = run(Mode::WrongTerminal);
    assert!(!report.complete);
    assert!(report.endings.is_empty());
    assert_eq!(report.blockers.len(), 1);
    assert_eq!(report.blockers[0].code, "ending_not_at_database_terminal");
    assert!(report.started_replays.is_empty());
}
#[test]
fn replay_requires_genuine_cumulative_unlock_and_has_separate_evidence() {
    let report = coverage::verify_with_driver(
        &mut Fake::new(Mode::ProgressReplay),
        coverage::audit_sources(&sources(false, true)),
        "fixture".into(),
        limits(),
    )
    .unwrap();
    assert!(report.complete, "{:?}", report.failures);
    assert!(report.restarts > 0);
    assert!(report.started_replays.contains("bonus-a"));
    assert_eq!(
        report.replay_scripts.iter().cloned().collect::<Vec<_>>(),
        vec![A]
    );
    assert!(report.direct_probes.is_empty());
    assert!(report.evidence[&format!("replay:bonus-a:script:{A}")]
        .decisions
        .iter()
        .any(|d| matches!(&d.action, Action::NewGame)));
}
#[test]
fn stalled_timer_looping_video_and_disabled_choices_fail_closed() {
    for (mode, code) in [
        (Mode::FrozenTimer, "no_progress"),
        (Mode::LoopingVideo, "looping_video_requires_defined_exit"),
        (Mode::Disabled, "invalid_or_deadlocked_choices"),
    ] {
        let report = run(mode);
        assert!(!report.complete);
        assert!(report.blockers.iter().any(|b| b.code == code));
    }
}
#[test]
fn state_frontier_snapshot_and_depth_budgets_are_enforced() {
    for budget in [
        Limits {
            max_states: 1,
            ..limits()
        },
        Limits {
            max_frontier: 1,
            ..limits()
        },
        Limits {
            max_snapshot_bytes: 1,
            ..limits()
        },
        Limits {
            max_frontier_bytes: 1,
            ..limits()
        },
        Limits {
            max_decisions: 1,
            ..limits()
        },
    ] {
        let mode = if budget.max_decisions == 1 {
            Mode::ProgressReplay
        } else {
            Mode::Branch
        };
        let report =
            coverage::verify_with_driver(&mut Fake::new(mode), audit(), "fixture".into(), budget)
                .unwrap();
        assert!(report.budget_exhausted && !report.complete);
    }
    assert!(coverage::verify_with_driver(
        &mut Fake::new(Mode::Branch),
        audit(),
        "fixture".into(),
        Limits {
            max_steps: 0,
            ..limits()
        }
    )
    .is_err());
}
#[test]
fn traversal_report_is_deterministic() {
    assert_eq!(
        serde_json::to_vec(&run(Mode::Branch)).unwrap(),
        serde_json::to_vec(&run(Mode::Branch)).unwrap()
    );
}

#[test]
fn cyclic_choices_stuck_video_and_lost_progress_cannot_certify_success() {
    for (mode, code) in [
        (Mode::ChoiceCycle, "choice_cycle_without_terminal"),
        (
            Mode::StuckVideo,
            "video_completion_did_not_release_sequence",
        ),
        (Mode::LostProgress, "new_game_lost_progress"),
        (Mode::ReplayNoop, "replay_did_not_start"),
    ] {
        let report = run(mode);
        assert!(!report.complete);
        assert!(
            report.blockers.iter().any(|b| b.code == code),
            "expected {code}; got {:?}",
            report.failures
        );
    }
}
#[test]
fn disabled_choices_are_reported_without_forcing_them() {
    let report = run(Mode::PartDisabled);
    assert!(!report.complete);
    assert_eq!(report.choices, 1);
    assert!(report.untraversed_choices.contains("selection:0:choice:1"));
    assert!(report.unreachable.contains(B) && report.unreachable_endings.contains(B));
}
#[test]
fn authentic_replay_return_to_title_is_separate_from_story_endings() {
    let report = run(Mode::ReplayTitle);
    assert!(report.complete, "{:?}", report.failures);
    assert!(report.completed_replays.contains("bonus-a"));
    assert_eq!(report.endings.len(), 2);
    assert!(report.replay_endings.is_empty());
}
#[test]
fn malformed_database_and_missing_branch_labels_are_audit_failures() {
    let mut invalid = sources(false, false);
    invalid.get_mut("scenario/db/DB_EV.ks").unwrap().push('\n');
    invalid.insert(
        "scenario/db/DB_EX.ks".into(),
        "lf.EX_SCENEInfo = run_private_expression();".into(),
    );
    let result = coverage::audit_sources(&invalid);
    assert!(result
        .issues
        .iter()
        .any(|i| i == "nonliteral_database_value"));
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("run_private_expression"));
    let mut invalid = sources(false, false);
    invalid.insert(INTRO.into(), "[SL_StartSelect num=0]".into());
    let result = coverage::audit_sources(&invalid);
    assert_eq!(
        result
            .issues
            .iter()
            .filter(|i| i.starts_with("selection_branch_label_missing"))
            .count(),
        2
    );
}
#[test]
fn strict_xp3_failure_is_not_turned_into_a_missing_script_success() {
    let fixture = tempfile::tempdir().unwrap();
    let sources = sources(false, false);
    let files: Vec<_> = sources
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_bytes()))
        .collect();
    std::fs::write(
        fixture.path().join("data.xp3"),
        common::archive(&files, true, true, 1, false),
    )
    .unwrap();
    let report = coverage::verify(Arc::new(Store::open(fixture.path()).unwrap()), 100).unwrap();
    assert!(!report.complete);
    assert!(report
        .audit
        .issues
        .iter()
        .any(|i| i.starts_with("script_integrity_or_encoding_failed:")));
}
#[test]
fn invalid_cli_arguments_and_missing_store_still_emit_json() {
    use std::process::Command;
    let fixture = common::game("Synthetic[c]\n[SC_EndProcess]");
    let cli = env!("CARGO_BIN_EXE_koikari");
    for tail in [
        vec!["invalid"],
        vec!["0"],
        vec!["--max-states"],
        vec!["--unknown", "2"],
        vec!["100", "--max-states", "0"],
    ] {
        let output = Command::new(cli)
            .arg("verify-all")
            .arg(fixture.path())
            .args(tail)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["complete"], false);
    }
    for args in [
        vec!["verify-all"],
        vec!["verify-all", "/does-not-exist"],
        vec!["verify-targets"],
        vec!["verify-targets", "/does-not-exist"],
    ] {
        let output = Command::new(cli).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok());
    }
}
#[test]
fn real_engine_baseline_is_not_falsely_certified() {
    let fixture = common::game("Synthetic owned line[c]\n[SC_EndProcess]");
    let report = coverage::verify(Arc::new(Store::open(fixture.path()).unwrap()), 100).unwrap();
    assert!(!report.complete);
    assert!(report.first_blocker.is_some());
}
#[test]
fn cli_preserves_legacy_commands_and_emits_failed_json_without_dialogue() {
    use std::process::Command;
    let fixture = common::game("PRIVATE DIALOGUE[c]\n[SC_EndProcess]");
    let cli = env!("CARGO_BIN_EXE_koikari");
    let output = Command::new(cli)
        .args(["verify-all", fixture.path().to_str().unwrap(), "100"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["complete"], false);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE DIALOGUE"));
    for args in [
        vec!["list"],
        vec!["text", "intro.ks"],
        vec!["verify", "intro.ks"],
        vec!["walk", "100"],
    ] {
        let mut command = Command::new(cli);
        command.arg(args[0]).arg(fixture.path()).args(&args[1..]);
        assert!(command.output().unwrap().status.success());
    }
}

/// Explicit opt-in: cargo test --test full_game authentic_ -- --ignored
/// Neither an absent environment variable nor a partial report is treated as a pass.
fn authentic(variable: &str) {
    let path = std::env::var_os(variable)
        .expect("authentic test requires its explicit XP3 environment variable");
    let store = Arc::new(Store::open(path).expect("strict XP3 store"));
    let report = coverage::verify(store, 2_000_000).expect("verifier configuration");
    assert_eq!(
        report.audit.main_scripts.len(),
        124,
        "unexpected effective resource edition"
    );
    assert_eq!(report.audit.selections.len(), 13);
    assert_eq!(report.audit.expected_endings.len(), 7);
    assert!(report.audit.issues.is_empty(), "metadata audit failed");
    assert!(
        report.complete,
        "whole game remains incomplete; run verify-all for metadata-only blockers"
    );
    assert!(report.direct_probes.is_empty());
}
#[test]
#[ignore = "requires user-owned complete base XP3 and integrated full scenario APIs"]
fn authentic_base_whole_game() {
    authentic("KOIKARI_ASSETS");
}
#[test]
#[ignore = "requires user-owned data/adult/patch XP3 and integrated full scenario APIs"]
fn authentic_patched_whole_game() {
    authentic("KOIKARI_PATCH_ASSETS");
}
