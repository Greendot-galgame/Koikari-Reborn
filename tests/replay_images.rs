#[allow(dead_code)]
mod common;

use koikari_reborn::{story::Engine, xp3::Store};
use std::sync::Arc;

const CG: &[u8] = b"synthetic encoded CG payload";
const BACKGROUND: &[u8] = b"synthetic encoded background payload";

fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let data = common::archive(
        &[
            ("ui/title/TI_base_0.webp", b"synthetic title"),
            ("Init_VariableSTAND.ks", b"; synthetic metadata"),
            (
                "Init_VariableADV.ks",
                b"tf.adv_NameMappings = %[\n\"ch\" => \"character\"\n];",
            ),
            (
                "DB_EV.ks",
                b"lf.EV_EventListInfo = [\n['intro',0,'End',-1,[-1,0],'Intro'],\n['second',1,-1,'End',[-1,0],'Second']\n];",
            ),
            (
                "intro.ks",
                b"@bgset_withoutblack bg=bg1_0\n@bg_image num=ev101_1\nFirst[c]\nSecond[c]\n@SC_EndProcess",
            ),
            (
                "second.ks",
                b"@bgset_withoutblack bg=bg1_0\nOnly background[c]\n@SC_EndProcess",
            ),
            ("evimage/ev101_1.webp", CG),
            ("bgimage/bg1_0.webp", BACKGROUND),
        ],
        true,
        true,
        0,
        false,
    );
    std::fs::write(directory.path().join("data.xp3"), data).unwrap();
    directory
}

#[test]
fn replay_previews_are_resolved_from_the_scenario_and_unlock_with_progress() {
    let directory = fixture();
    let store = Store::open(directory.path()).unwrap();
    let mut engine = Engine::open(Arc::new(store)).unwrap();
    let locked = engine.replay_list().unwrap();
    assert_eq!(locked.len(), 2);
    assert!(!locked[0].unlocked);
    assert_eq!(locked[0].preview.as_deref(), Some("evimage/ev101_1.webp"));
    assert_eq!(locked[1].preview.as_deref(), Some("bgimage/bg1_0.webp"));
    assert!(engine.replay_image("event:intro").is_err());
    assert!(engine.replay_image("event:missing").is_err());

    engine.start().unwrap();
    let unlocked = engine.replay_list().unwrap();
    assert!(unlocked[0].unlocked);
    assert!(!unlocked[1].unlocked);
    assert_eq!(engine.replay_image("event:intro").unwrap(), CG);
    assert!(engine.replay_image("event:second").is_err());
    assert!(engine.replay_image("event:missing").is_err());
    // An unlocked event without a CG falls back to its first background.
    engine.state.progress.events.insert("second".into());
    assert!(engine.replay_list().unwrap()[1].unlocked);
    assert_eq!(engine.replay_image("event:second").unwrap(), BACKGROUND);
}
