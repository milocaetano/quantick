use super::*;
use quantick_drawings::PresetHost;
/// A bank with one saved preset, to test the recorder's refusal against
/// the real store's.
///
/// Its own file every call. The store persists on every write, so two
/// banks sharing a path would let the first call's save leak into the
/// second and turn a free name into a taken one.
/// `thread_dir` rather than a `ScratchFile`: the store outlives the
/// expression that built it and keeps saving to this path, so nothing
/// here could hold a value that removed the file at the right moment.
fn bank(tag: &str) -> quantick_drawings::presets::PresetStore {
    let path = crate::scratch::thread_dir("drawing-presets").join(format!("{tag}.toml"));
    let mut store = quantick_drawings::presets::PresetStore::load_from(path);
    store.save_custom_preset("fib", "taken", toml::Value::Integer(1), false);
    store
}

/// The one rule the recorder has to answer for itself, pinned against the
/// bank that would otherwise answer it. Two surfaces that disagree about
/// whether a name is free would let the inspector's "name already used?"
/// prompt appear when the save would have succeeded, or skip it when the
/// save is about to overwrite.
#[test]
fn recorder_refuses_a_taken_name_exactly_as_the_bank_does() {
    for (name, overwrite) in [
        ("taken", false),
        ("taken", true),
        ("free", false),
        ("free", true),
    ] {
        let mut real = bank(&format!("real-{name}-{overwrite}"));
        let real_answer = real.save_custom_preset("fib", name, toml::Value::Integer(2), overwrite);
        let read_only = bank(&format!("copy-{name}-{overwrite}"));
        let mut recorder = RecordingPresetHost::new(&read_only);
        let recorded = recorder.save_custom_preset("fib", name, toml::Value::Integer(2), overwrite);
        assert_eq!(
            recorded, real_answer,
            "the recorder and the bank disagreed about {name:?} with overwrite={overwrite}"
        );
        assert_eq!(
            recorder.into_writes().is_empty(),
            !real_answer,
            "a refused save must record nothing, and an accepted one exactly one write"
        );
        let _ = std::fs::remove_file(real.path());
        let _ = std::fs::remove_file(read_only.path());
    }
}

/// Every read is the bank's own answer, so a tool tab cannot see a
/// different world through the recorder than through the store.
#[test]
fn recorder_delegates_every_read_to_the_bank() {
    let store = bank("reads");
    let recorder = RecordingPresetHost::new(&store);
    assert_eq!(
        recorder.custom_preset_names("fib"),
        store.custom_preset_names("fib")
    );
    assert_eq!(
        recorder.load_custom_preset("fib", "taken"),
        store.load_custom_preset("fib", "taken")
    );
    assert_eq!(recorder.default_preset("fib"), store.default_preset("fib"));
    assert_eq!(recorder.default_style("fib"), store.default_style("fib"));
    assert_eq!(recorder.default_config("fib"), store.default_config("fib"));
    assert_eq!(
        recorder.has_default_config("fib"),
        store.has_default_config("fib")
    );
    let _ = std::fs::remove_file(store.path());
}

/// A frame that deletes a preset and saves under the freed name must not
/// be told the name is taken. The bank is not written until the host
/// replays the list, so a recorder that consulted it alone would raise the
/// "Overwrite?" prompt for a name its own pending delete is about to free.
#[test]
fn a_read_sees_the_writes_the_same_frame_already_made() {
    let store = bank("same-frame");
    let mut recorder = RecordingPresetHost::new(&store);
    assert!(
        !recorder.save_custom_preset("fib", "taken", toml::Value::Integer(2), false),
        "the name is taken and overwrite was not asked for"
    );
    recorder.delete_custom_preset("fib", "taken");
    assert!(
        recorder.save_custom_preset("fib", "taken", toml::Value::Integer(2), false),
        "the delete this frame recorded freed the name"
    );
    let _ = std::fs::remove_file(store.path());
}

/// And the list the trader picks from is the list the frame's own writes
/// leave behind — a combo box that still offers a preset deleted a moment
/// ago is offering something that will not load.
#[test]
fn the_name_list_follows_the_frame_s_own_writes() {
    let store = bank("name-list");
    let mut recorder = RecordingPresetHost::new(&store);
    assert_eq!(
        recorder.custom_preset_names("fib"),
        vec!["taken".to_owned()]
    );
    recorder.save_custom_preset("fib", "another", toml::Value::Integer(3), false);
    assert_eq!(
        recorder.custom_preset_names("fib"),
        vec!["another".to_owned(), "taken".to_owned()],
        "a saved name joins the list, in the order the bank would give it back"
    );
    assert_eq!(
        recorder.load_custom_preset("fib", "another"),
        Some(toml::Value::Integer(3)),
        "and it loads what was saved rather than nothing"
    );
    recorder.delete_custom_preset("fib", "taken");
    assert_eq!(
        recorder.custom_preset_names("fib"),
        vec!["another".to_owned()]
    );
    assert_eq!(recorder.load_custom_preset("fib", "taken"), None);
    let _ = std::fs::remove_file(store.path());
}

/// The fold keeps a flag set and takes the first valued ask, which is the
/// half of a merge a newly added field is most likely to get wrong.
#[test]
fn the_fold_keeps_flags_and_the_first_value() {
    let mut first = DrawingChromeAsk {
        show_all: true,
        manager_delete: Some(3),
        presets: vec![PresetWrite::Delete {
            tool_id: "fib".into(),
            name: "a".into(),
        }],
        ..DrawingChromeAsk::default()
    };
    first.merge(DrawingChromeAsk {
        unlock_all: true,
        manager_delete: Some(7),
        presets: vec![PresetWrite::Delete {
            tool_id: "fib".into(),
            name: "b".into(),
        }],
        ..DrawingChromeAsk::default()
    });
    assert!(first.show_all, "a set flag survives the fold");
    assert!(first.unlock_all, "and the other side's flag arrives");
    assert_eq!(
        first.manager_delete,
        Some(3),
        "the first valued ask wins; the later one is dropped"
    );
    assert_eq!(
        first.presets.len(),
        2,
        "preset writes are a sequence, not an answer to one question: both land, in order"
    );
}
