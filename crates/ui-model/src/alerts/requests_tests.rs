use super::*;
use quantick_alerts::{AlertId, TargetKind};
use quantick_control_schema::alerts::TargetResolveError;

fn key(asset: &str) -> DrawingKey {
    DrawingKey::of_pane("fixture", asset.to_owned(), None)
}

/// An identity issued for drawing `id` under `key`, as the store issues it.
fn identity(key: &DrawingKey, id: u64) -> DurableDrawingRef {
    drawing_identity::issue("workspace", key, id, "entropy").unwrap()
}

fn pane(tab_index: usize, pane_id: u64, key: Option<DrawingKey>, focused: bool) -> ShownPane {
    ShownPane {
        tab_index,
        pane_id,
        key,
        focused,
        drawings: vec![],
    }
}

fn shown(index: usize, identity: Option<DurableDrawingRef>) -> ShownDrawing {
    ShownDrawing {
        index,
        identity,
        all_timeframes: false,
    }
}

fn copy(tab_index: usize, pane_id: u64, index: usize) -> Located {
    Located::Found(DrawingCopy {
        tab_index,
        pane_id,
        index,
    })
}

/// A window that records what it was asked to do.
struct FakeHost {
    runtime: quantick_stores::alerts::AlertRuntime,
    panes: Vec<ShownPane>,
    resolved: Result<DurableDrawingRef, TargetResolveError>,
    revealed: usize,
    selected: Vec<DrawingCopy>,
    notes: Vec<String>,
}

impl FakeHost {
    fn new(label: &str) -> Self {
        let path = crate::scratch::thread_dir("alert-requests").join(format!("{label}.json"));
        Self {
            runtime: quantick_stores::alerts::AlertRuntime::open(path, 0),
            panes: vec![],
            resolved: Err(TargetResolveError::Missing),
            revealed: 0,
            selected: vec![],
            notes: vec![],
        }
    }
}

impl AlertDefinitionsPort for FakeHost {
    fn alert_resolve_target(
        &mut self,
        _: DrawingSelector,
    ) -> Result<DurableDrawingRef, TargetResolveError> {
        self.resolved.clone()
    }
    fn alert_runtime(&mut self) -> &mut quantick_stores::alerts::AlertRuntime {
        &mut self.runtime
    }
}

impl RequestHost for FakeHost {
    fn reveal_dock(&mut self) {
        self.revealed += 1;
    }
    fn shown_panes(&self) -> Vec<ShownPane> {
        self.panes.clone()
    }
    fn select_copy(&mut self, copy: DrawingCopy) {
        self.selected.push(copy);
    }
    fn note(&mut self, message: String) {
        self.notes.push(message);
    }
}

const SELECTOR: DrawingSelector = DrawingSelector {
    tab_id: 1,
    pane_id: 2,
    drawing_id: 3,
};

#[test]
fn locate_finds_the_one_shown_copy() {
    let key = key("A");
    let target = identity(&key, 3);
    let mut other = pane(0, 7, Some(key.clone()), false);
    other.drawings = vec![shown(0, None), shown(1, Some(identity(&key, 4)))];
    let mut holder = pane(1, 9, Some(key.clone()), false);
    holder.drawings = vec![shown(0, None), shown(2, Some(target.clone()))];
    assert_eq!(locate(&target, &[other, holder]), copy(1, 9, 2));
}

#[test]
fn locate_reports_a_target_no_pane_shows() {
    let key = key("A");
    let mut holder = pane(0, 7, Some(key.clone()), true);
    holder.drawings = vec![shown(0, Some(identity(&key, 4)))];
    assert_eq!(locate(&identity(&key, 3), &[holder]), Located::Missing);
    assert_eq!(locate(&identity(&key, 3), &[]), Located::Missing);
}

#[test]
fn a_known_mirror_is_not_ambiguous_and_the_focused_copy_wins() {
    let key = key("A");
    let target = identity(&key, 3);
    let mut first = pane(0, 7, Some(key.clone()), false);
    first.drawings = vec![shown(4, Some(target.clone()))];
    let mut focused = pane(1, 9, Some(key.clone()), true);
    focused.drawings = vec![shown(5, Some(target.clone()))];
    assert_eq!(
        locate(&target, &[first.clone(), focused.clone()]),
        copy(1, 9, 5)
    );
    // With nothing focused the first shown copy wins.
    focused.focused = false;
    assert_eq!(locate(&target, &[first, focused]), copy(0, 7, 4));
}

#[test]
fn an_all_timeframes_copy_belongs_under_its_market_key() {
    let pane_key = key("A");
    let target = identity(&DrawingKey::all_timeframes(pane_key.market.clone()), 3);
    let mut holder = pane(0, 7, Some(pane_key), true);
    holder.drawings = vec![ShownDrawing {
        index: 1,
        identity: Some(target.clone()),
        all_timeframes: true,
    }];
    assert_eq!(locate(&target, &[holder.clone()]), copy(0, 7, 1));
    holder.drawings[0].all_timeframes = false;
    assert_eq!(locate(&target, &[holder]), Located::Ambiguous);
}

#[test]
fn a_copy_outside_its_key_or_twice_in_a_pane_is_ambiguous() {
    let key_a = key("A");
    let target = identity(&key_a, 3);
    let mut foreign = pane(0, 7, Some(key("B")), true);
    foreign.drawings = vec![shown(0, Some(target.clone()))];
    assert_eq!(locate(&target, &[foreign]), Located::Ambiguous);
    let mut keyless = pane(0, 7, None, true);
    keyless.drawings = vec![shown(0, Some(target.clone()))];
    assert_eq!(locate(&target, &[keyless]), Located::Ambiguous);
    let mut twice = pane(0, 7, Some(key_a), true);
    twice.drawings = vec![
        shown(0, Some(target.clone())),
        shown(1, Some(target.clone())),
    ];
    assert_eq!(locate(&target, &[twice]), Located::Ambiguous);
}

#[test]
fn apply_selects_the_located_copy_or_notes_why_not() {
    let key = key("A");
    let target = identity(&key, 3);
    let mut host = FakeHost::new("locate");
    let mut state = State::default();
    let locate_request = || Request {
        locate: Some(target.clone()),
        ..Request::default()
    };
    apply(&mut state, &mut host, locate_request());
    assert_eq!(host.notes, [MISSING]);
    let mut holder = pane(0, 7, Some(key.clone()), true);
    holder.drawings = vec![shown(2, Some(target.clone()))];
    host.panes = vec![holder.clone()];
    apply(&mut state, &mut host, locate_request());
    assert_eq!(
        host.selected,
        [DrawingCopy {
            tab_index: 0,
            pane_id: 7,
            index: 2,
        }]
    );
    holder.drawings.push(shown(3, Some(target.clone())));
    host.panes = vec![holder];
    apply(&mut state, &mut host, locate_request());
    assert_eq!(host.notes, [MISSING, AMBIGUOUS]);
    assert_eq!(host.selected.len(), 1);
    assert_eq!(
        state.error(),
        None,
        "the model never sees a host-owned locate"
    );
    assert_eq!(host.revealed, 0);
}

#[test]
fn apply_opens_the_dock_and_the_manager() {
    let mut host = FakeHost::new("open");
    let mut state = State::default();
    apply(
        &mut state,
        &mut host,
        Request {
            open: true,
            ..Request::default()
        },
    );
    assert_eq!(host.revealed, 1);
    assert!(state.is_open());
}

#[test]
fn apply_settles_a_resolve_on_the_waiting_condition() {
    let target = identity(&key("A"), 3);
    let mut host = FakeHost::new("resolve");
    let mut state = State::default();
    let mut chart = super::super::tests::context();
    chart.drawings.push(super::super::DrawingChoice {
        selector: SELECTOR,
        target: None,
        kind: TargetKind::Rectangle,
        name: "Region fixture".into(),
    });
    Model::update(
        &mut state,
        Intent::Create {
            context: Box::new(chart),
            target: None,
        },
    );
    Model::update(
        &mut state,
        Intent::Resolve {
            index: 0,
            selector: SELECTOR,
        },
    );
    host.resolved = Ok(target.clone());
    apply(
        &mut state,
        &mut host,
        Request {
            resolve: Some(SELECTOR),
            ..Request::default()
        },
    );
    assert_eq!(
        state.draft().unwrap().spec.conditions[0].target,
        quantick_alerts::TargetRef::Drawing(target)
    );
    assert_eq!(state.error(), None);
}

#[test]
fn open_drawing_opens_the_editor_or_notes_the_refusal() {
    let mut host = FakeHost::new("drawing");
    let mut state = State::default();
    open_drawing(&mut state, &mut host, SELECTOR);
    assert_eq!(host.notes, ["Alert target unavailable: Missing"]);
    assert!(!state.has_pending_drawing());
    host.resolved = Ok(identity(&key("A"), 3));
    open_drawing(&mut state, &mut host, SELECTOR);
    assert!(state.is_open());
    assert!(state.has_pending_drawing());
    assert_eq!(host.notes.len(), 1);
}

#[test]
fn settle_command_runs_on_the_owner_and_reports_its_outcome() {
    let mut host = FakeHost::new("command");
    let mut state = super::super::tests::opened_draft();
    let missing = AlertCommand::Pause {
        id: AlertId(42),
        expected_revision: 0,
    };
    settle_command(&mut state, host.alert_runtime(), missing, 0);
    assert!(state.error().is_some(), "an unknown alert is refused");
    assert!(state.draft().is_some(), "a refusal keeps the draft");
    settle_command(
        &mut state,
        host.alert_runtime(),
        AlertCommand::ClearHistory,
        0,
    );
    assert_eq!(state.error(), None);
    assert!(state.draft().is_none(), "success closes the draft");
}
