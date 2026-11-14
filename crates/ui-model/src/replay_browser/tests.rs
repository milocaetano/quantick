use super::*;
use crate::Model as _;
use quantick_replay::{SessionDate, UtcOffset};

fn library(days: &[&str]) -> Library {
    Library {
        root: PathBuf::from("recordings"),
        sessions: days
            .iter()
            .map(|day| SessionEntry {
                path: PathBuf::from(format!("recordings/WINJ26/{day}.csv")),
                symbol: "WINJ26".to_owned(),
                date: SessionDate::parse(day),
                size_bytes: 50,
                timezone: UtcOffset::UTC,
                timezone_declared: true,
                side_source: None,
                has_quotes: false,
                notes: Vec::new(),
                has_context: false,
            })
            .collect(),
        problems: Vec::new(),
    }
}

#[test]
fn replay_browser_choice_and_staging_have_one_owner() {
    let mut state = State::default();
    assert!(!state.browser_open);
    assert_eq!(state.speed, 1.0);
    assert!(!state.autoplay, "a recording starts paused");
    assert!(state.day_before, "yesterday comes unless it was turned off");
    assert!(state.library.is_none());
    assert!(state.loading.is_none());
    assert!(
        state.folder.is_empty(),
        "a default model does not resolve a home"
    );
    Model::update(
        &mut state,
        Intent::Initialize {
            folder: "scratch".into(),
            stored: Some("standing".into()),
            day_before: Some(true),
        },
    );
    assert_eq!(
        state.folder, "scratch",
        "the host supplies the run's resolved folder"
    );
    assert_eq!(state.stored.as_deref(), Some("standing"));
    assert_eq!(
        Model::update(&mut state, Intent::TakeFolderChange),
        Effects::default()
    );
    Model::update(&mut state, Intent::SetFolderDraft("  chosen  ".into()));
    assert_eq!(
        Model::update(&mut state, Intent::TakeFolderChange).folder_change,
        None
    );
    let chosen = Model::update(&mut state, Intent::ChooseFolder);
    assert_eq!(
        chosen.local,
        Some(LocalEffect::Scan {
            folder: "chosen".into()
        })
    );
    Model::update(&mut state, Intent::SetFolderDraft("partial draft".into()));
    assert_eq!(
        Model::update(&mut state, Intent::TakeFolderChange).folder_change,
        Some(Some("chosen".into()))
    );
    assert_eq!(
        Model::update(&mut state, Intent::TakeFolderChange).folder_change,
        None,
        "a choice persists once"
    );
    Model::update(&mut state, Intent::SetDayBefore(false));
    Model::update(&mut state, Intent::StageDayBefore(true));
    assert!(state.day_before);
    assert_eq!(state.day_before_stored, Some(false));
    assert_eq!(
        Model::update(&mut state, Intent::TakeDayBeforeChange).day_before_change,
        Some(false)
    );
    assert_eq!(
        Model::update(&mut state, Intent::TakeDayBeforeChange).day_before_change,
        None
    );
    Model::update(&mut state, Intent::SetFolderDraft(String::new()));
    Model::update(&mut state, Intent::ChooseFolder);
    assert_eq!(
        Model::update(&mut state, Intent::TakeFolderChange).folder_change,
        Some(None),
        "an empty field forgets the pick"
    );
}

#[test]
fn replay_scan_completion_preserves_requested_selection() {
    let mut state = State::default();
    Model::update(&mut state, Intent::Rescan);
    let scanned = library(&["20260313", "20260316"]);
    let sessions = scanned.sessions.as_ptr();
    assert_eq!(
        Model::update(&mut state, Intent::ScanCompleted(scanned)),
        Effects::default()
    );
    assert_eq!(
        state.library.as_ref().unwrap().sessions.as_ptr(),
        sessions,
        "the scan payload moves into its sole owner"
    );
    assert_eq!(
        state.selected,
        Some(0),
        "ordinary scans pick the oldest first"
    );
    assert_eq!(
        Model::update(&mut state, Intent::SelectDay("2026-03-16".into())).selected_day,
        Some(true)
    );
    assert_eq!(state.selected, Some(1));
    assert_eq!(
        Model::update(&mut state, Intent::SelectDay("2026-03-14".into())).selected_day,
        Some(false)
    );
    assert_eq!(
        state.selected,
        Some(1),
        "a select_day miss keeps the previous pick"
    );
    Model::update(&mut state, Intent::DownloadRefreshed);
    Model::update(
        &mut state,
        Intent::ScanCompleted(library(&["20260312", "20260313", "20260316"])),
    );
    assert_eq!(
        state.selected,
        Some(2),
        "batch refresh follows the same path after its index moves"
    );
    state.tab = BrowserTab::GetData;
    let downloaded = PathBuf::from("recordings/WINJ26/20260313.csv");
    assert!(
        Model::update(&mut state, Intent::Downloaded(downloaded))
            .local
            .is_some()
    );
    assert_eq!(
        Model::update(
            &mut state,
            Intent::ScanCompleted(library(&["20260312", "20260313", "20260316"]))
        ),
        Effects::default()
    );
    assert_eq!(
        state.selected,
        Some(1),
        "final download picks the downloaded path"
    );
    assert_eq!(state.tab, BrowserTab::Sessions);
    let missed = Model::update(
        &mut state,
        Intent::AutostartSelection {
            speed: 4.0,
            day: Some("2026-03-14".into()),
            play: true,
        },
    );
    assert!(!missed.load_started);
    assert!(missed.local.is_none());
    assert!(state.browser_open);
    assert_eq!(
        state.selected, None,
        "an autostart miss cannot leave a different recording armed"
    );
    assert!(
        !state.folder_dirty,
        "scanning is never a standing folder choice"
    );
    Model::update(&mut state, Intent::Select(1));
    assert_eq!(state.selected, Some(1));
    let missing = Library {
        root: PathBuf::from("missing-recordings"),
        problems: vec![quantick_replay::library::Problem {
            path: None,
            kind: quantick_replay::library::ProblemKind::FolderMissing,
            detail: "recordings folder does not exist".into(),
        }],
        ..library(&[])
    };
    assert_eq!(
        Model::update(&mut state, Intent::ScanCompleted(missing)),
        Effects::default()
    );
    assert_eq!(state.selected, None);
    let missing = state.library.as_ref().unwrap();
    assert!(missing.sessions.is_empty());
    assert_eq!(missing.root, PathBuf::from("missing-recordings"));
    assert_eq!(missing.problems.len(), 1);
    assert_eq!(
        missing.problems[0].kind,
        quantick_replay::library::ProblemKind::FolderMissing
    );
    assert_eq!(missing.problems[0].path, None);
    assert_eq!(missing.problems[0].detail, "recordings folder does not exist");
}

#[test]
fn replay_load_replacement_captures_day_before_and_uses_completion_options() {
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::ScanCompleted(library(&["20260313", "20260316"])),
    );
    Model::update(&mut state, Intent::SetOpen(true));
    let first = Model::update(&mut state, Intent::LoadSelected);
    assert_eq!(
        first.local,
        Some(LocalEffect::Load {
            path: PathBuf::from("recordings/WINJ26/20260313.csv"),
            day_before: true
        })
    );
    Model::update(&mut state, Intent::SetDayBefore(false));
    Model::update(&mut state, Intent::Select(1));
    let replacement = Model::update(&mut state, Intent::LoadSelected);
    assert_eq!(
        replacement.local,
        Some(LocalEffect::Load {
            path: PathBuf::from("recordings/WINJ26/20260316.csv"),
            day_before: false
        })
    );
    assert!(
        replacement.load_started,
        "a double click replaces an in-flight request"
    );
    assert_eq!(
        Model::update(&mut state, Intent::LoadStarted),
        Effects::default(),
        "a terminal receipt cannot start more IO"
    );
    Model::update(&mut state, Intent::SetSpeed(8.0));
    Model::update(&mut state, Intent::SetAutoplay(true));
    let completed = Model::update(&mut state, Intent::LoadCompleted(Ok(())));
    assert_eq!(
        completed.open_loaded,
        Some((8.0, true)),
        "speed and autoplay are chosen at completion"
    );
    assert!(completed.local.is_none());
    assert!(state.loading.is_none());
    assert!(
        !state.browser_open,
        "success closes the browser before painting"
    );
    Model::update(&mut state, Intent::SetOpen(true));
    Model::update(&mut state, Intent::LoadSelected);
    let failed = Model::update(
        &mut state,
        Intent::LoadCompleted(Err("loader disconnected".into())),
    );
    assert_eq!(failed, Effects::default());
    assert!(state.loading.is_none());
    assert!(state.browser_open);
    assert_eq!(state.error.as_deref(), Some("loader disconnected"));
}

#[test]
fn replay_picker_and_arrival_request_one_local_operation() {
    let mut state = State::default();
    assert_eq!(
        Model::update(&mut state, Intent::OpenBrowser),
        Effects::default(),
        "no configured folder scans nothing"
    );
    assert!(state.browser_open);
    assert!(state.library.is_none());
    Model::update(&mut state, Intent::SetFolderDraft("recordings".into()));
    assert!(
        Model::update(&mut state, Intent::OpenBrowser)
            .local
            .is_some()
    );
    Model::update(&mut state, Intent::ScanCompleted(library(&[])));
    assert_eq!(
        Model::update(&mut state, Intent::OpenBrowser),
        Effects::default(),
        "cached library prevents repeated scans"
    );
    assert!(matches!(
        Model::update(&mut state, Intent::PickFolder).local,
        Some(LocalEffect::PickFolder { .. })
    ));
    assert_eq!(
        Model::update(&mut state, Intent::PickerStarted),
        Effects::default()
    );
    assert_eq!(
        Model::update(&mut state, Intent::PickFolder),
        Effects::default(),
        "one native dialog at a time"
    );
    assert_eq!(
        Model::update(&mut state, Intent::PickerCompleted(None)),
        Effects::default()
    );
    assert!(!state.picker_pending);
    assert!(matches!(
        Model::update(&mut state, Intent::PickerCompleted(Some("selected".into()))).local,
        Some(LocalEffect::Scan { .. })
    ));
    assert_eq!(state.stored.as_deref(), Some("selected"));
    assert!(matches!(
        Model::update(
            &mut state,
            Intent::EnterDownload {
                symbol: Some("WINJ26".into()),
                explicit: false
            }
        )
        .local,
        Some(LocalEffect::EnterDownload { .. })
    ));
    assert_eq!(
        Model::update(
            &mut state,
            Intent::EnterDownload {
                symbol: None,
                explicit: false
            }
        ),
        Effects::default(),
        "staying on a tab is not another arrival"
    );
    assert!(
        Model::update(
            &mut state,
            Intent::EnterDownload {
                symbol: None,
                explicit: true
            }
        )
        .local
        .is_some(),
        "the explicit launch hook may ask again"
    );
}

#[test]
fn replay_offerings_rebuild_only_for_changed_borrowed_inputs_or_scan() {
    let mut state = State::default();
    Model::update(&mut state, Intent::ScanCompleted(library(&["20260313"])));
    assert!(state.observe_market(Some("WINV26"), &["WINQ26", "WINV26", " "]));
    assert_eq!(state.offered_symbols, ["WINV26", "WINQ26", "WINJ26"]);
    let allocation = state.offered_symbols[0].as_ptr();
    assert!(!state.observe_market(Some("WINV26"), &["WINQ26", "WINV26", " "]));
    assert_eq!(
        state.offered_symbols[0].as_ptr(),
        allocation,
        "an unchanged paint retains the cached strings"
    );
    assert!(state.observe_market(Some("WINJ26"), &["WINV26"]));
    assert_eq!(state.offered_symbols, ["WINJ26", "WINV26"]);
}

// These semantic tests moved from app::replay_view. Their scan receipts are
// supplied values; actual folder and CSV behavior remains covered in app.
fn initialized(stored: Option<&str>, day_before: Option<bool>) -> State {
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Initialize {
            folder: stored.unwrap_or_default().to_owned(),
            stored: stored.map(str::to_owned),
            day_before,
        },
    );
    state
}
fn settle_scan(state: &mut State, intent: Intent) {
    let effect = Model::update(state, intent);
    let Some(LocalEffect::Scan { folder }) = effect.local else {
        panic!("the fixture must request a scan");
    };
    Model::update(
        state,
        Intent::ScanCompleted(Library {
            root: PathBuf::from(folder),
            ..library(&[])
        }),
    );
}
#[test]
fn a_fresh_view_is_closed_paused_and_starts_at_one_times() {
    let view = initialized(None, None);
    assert!(!view.browser_open);
    assert_eq!(view.speed, 1.0);
    assert!(
        !view.autoplay,
        "a replay opens on its first print and waits for the trader"
    );
    assert!(view.day_before, "yesterday comes unless it was turned off");
    assert_eq!(
        view.day_before_stored, None,
        "and a default nobody chose is not a choice to write down"
    );
    assert!(view.library.is_none());
    assert!(view.loading.is_none(), "nothing is being parsed yet");
}

#[test]
fn a_tick_is_written_down_once_and_a_staged_hook_never_is() {
    let mut view = initialized(Some("D:/tape"), Some(true));
    assert!(view.day_before);
    assert_eq!(
        Model::update(&mut view, Intent::TakeDayBeforeChange).day_before_change,
        None,
        "nothing has changed"
    );

    Model::update(&mut view, Intent::SetDayBefore(false));
    assert!(!view.day_before);
    assert_eq!(
        Model::update(&mut view, Intent::TakeDayBeforeChange).day_before_change,
        Some(false)
    );
    assert_eq!(
        Model::update(&mut view, Intent::TakeDayBeforeChange).day_before_change,
        None,
        "one choice is written once"
    );
    assert_eq!(view.day_before_stored, Some(false));

    // A validation run wears the setting for that run and leaves the
    // trader's own cockpit exactly as it found it.
    Model::update(&mut view, Intent::StageDayBefore(true));
    assert!(view.day_before, "the run sees what the hook asked for");
    assert_eq!(
        Model::update(&mut view, Intent::TakeDayBeforeChange).day_before_change,
        None
    );
    assert_eq!(
        view.day_before_stored,
        Some(false),
        "and the file still holds the trader's answer"
    );
}

/// The report this whole change answers: the trader points the browser at
/// their recordings, and the app has something to write down instead of
/// starting the next launch on nowhere.
#[test]
fn a_folder_the_trader_chose_is_offered_for_remembering_once() {
    let mut view = initialized(None, None);
    view.folder = "D:/tape".to_string();
    settle_scan(&mut view, Intent::ChooseFolder);
    assert_eq!(
        Model::update(&mut view, Intent::TakeFolderChange).folder_change,
        Some(Some("D:/tape".to_owned())),
        "the pick is written on the frame it was made"
    );
    assert_eq!(
        Model::update(&mut view, Intent::TakeFolderChange).folder_change,
        None,
        "one pick is written once, not on every frame that follows it"
    );
    assert_eq!(view.stored.as_deref(), Some("D:/tape"));
}

/// Scanning is not choosing. Opening the browser and the autostart hook
/// both scan, and neither is the trader pointing anywhere — a scan that
/// stored its folder would freeze the default, or a QA run's scratch
/// folder, into the workspace.
#[test]
fn a_scan_the_app_started_stores_nothing() {
    let mut view = initialized(None, None);
    settle_scan(&mut view, Intent::Rescan);
    assert_eq!(
        Model::update(&mut view, Intent::TakeFolderChange).folder_change,
        None
    );
    assert_eq!(view.stored.as_deref(), None, "still never chosen");
}

/// Typing is not choosing either. A half-typed path must never reach the
/// file and replace the folder that was working.
#[test]
fn typing_in_the_field_alone_changes_nothing() {
    let mut view = initialized(Some("D:/tape"), None);
    view.folder = "D:/ta".to_string();
    assert_eq!(
        Model::update(&mut view, Intent::TakeFolderChange).folder_change,
        None
    );
    assert_eq!(view.stored.as_deref(), Some("D:/tape"));
}

/// Emptying the field is un-choosing, not choosing "" — the empty string
/// is what used to resolve to the process's working directory.
#[test]
fn an_emptied_field_forgets_the_pick_rather_than_storing_nothing() {
    let mut view = initialized(Some("D:/tape"), None);
    view.folder = String::new();
    settle_scan(&mut view, Intent::ChooseFolder);
    assert_eq!(
        Model::update(&mut view, Intent::TakeFolderChange).folder_change,
        Some(None)
    );
    assert_eq!(view.stored.as_deref(), None);
}

/// The chart's own contract comes first, the catalogue next, and what is
/// already on disk after that — each named once.
#[test]
fn the_offered_contracts_lead_with_the_chart_and_never_repeat() {
    let mut view = initialized(None, None);
    Model::update(&mut view, Intent::ScanCompleted(library(&[])));
    view.observe_market(Some("WINV26"), &["WINQ26", "WINV26"]);
    let offered = view.offered_symbols;
    assert_eq!(offered, vec!["WINV26".to_owned(), "WINQ26".to_owned()]);
}
