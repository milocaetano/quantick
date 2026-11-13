use super::*;
use crate::Model as _;
use quantick_drawings::DRAWING_TOOLS;

#[test]
fn toolbox_opens_outside_the_chart_docked_left() {
    let rail = State::default();
    assert!(rail.visible);
    assert_eq!(rail.tool, Tool::Pointer);
    assert_eq!(rail.dock, ToolboxDock::Left);
}

#[test]
fn hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed() {
    let mut rail = State::default();
    Model::update(&mut rail, Intent::Arm(Tool::Drawing(DRAWING_TOOLS[0])));
    Model::update(&mut rail, Intent::SetVisible(false));
    assert!(!rail.visible);
    assert_eq!(rail.tool, Tool::Pointer);
}

#[test]
fn arming_a_family_member_becomes_the_slot_memory() {
    let mut rail = State::default();
    let extension = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "fib-extension")
        .expect("registered");
    Model::update(&mut rail, Intent::Arm(Tool::Drawing(extension)));
    let family = extension.family().expect("fib family");
    assert_eq!(
        rail.family_member(family, &[extension]),
        Some(extension),
        "the slot remembers the last-armed member"
    );
}

/// A saved id this build does not offer is reported, not swallowed.
///
/// It used to be harmless: the pruned list only reached the disk on an
/// explicit save. Now the trader's next star click writes it back, so the
/// drop is permanent — and dropping something a file said without a word
/// is the silent patching `CLAUDE.md` rules out.
#[test]
fn a_starred_id_this_build_does_not_offer_is_reported() {
    let mut rail = State::default();

    let effects = Model::update(
        &mut rail,
        Intent::RestoreFavorites(vec![
            "measure".to_owned(),
            "volume-profile-from-tomorrow".to_owned(),
        ]),
    );
    let [Effect::UnknownFavorites(unknown)] = effects.as_slice() else {
        panic!("expected unknown favorite report")
    };

    assert_eq!(
        unknown,
        &vec!["volume-profile-from-tomorrow".to_owned()],
        "the caller is told exactly what it lost"
    );
    assert_eq!(
        rail.favorites.as_slice().len(),
        1,
        "and the rail keeps the tools it does have"
    );
}

/// Star order is the order the trader starred in: unstarring removes in
/// place, re-starring appends at the end — pinned muscle memory holds.
#[test]
fn star_order_is_stable_under_unstar_and_restar() {
    let mut rail = State::default();
    let tools: Vec<DrawingTool> = DRAWING_TOOLS.into_iter().take(3).collect();
    for tool in &tools {
        Model::update(&mut rail, Intent::ToggleFavorite(*tool));
    }
    Model::update(&mut rail, Intent::ToggleFavorite(tools[1]));
    assert_eq!(rail.favorites.as_slice(), &[tools[0], tools[2]]);
    Model::update(&mut rail, Intent::ToggleFavorite(tools[1]));
    assert_eq!(rail.favorites.as_slice(), &[tools[0], tools[2], tools[1]]);
}

/// The saved-ids path: order kept, unknown ids dropped, duplicates kept
/// once — a stale or hand-edited file cannot corrupt the rail.
#[test]
fn set_favorites_round_trips_ids_and_survives_junk() {
    let mut rail = State::default();
    let tools: Vec<DrawingTool> = DRAWING_TOOLS.into_iter().take(2).collect();
    let ids = vec![
        tools[1].id().to_owned(),
        "no-such-tool".to_owned(),
        tools[0].id().to_owned(),
        tools[1].id().to_owned(),
    ];
    Model::update(&mut rail, Intent::RestoreFavorites(ids));
    assert_eq!(rail.favorites.as_slice(), &[tools[1], tools[0]]);

    let saved: Vec<String> = rail
        .favorites
        .as_slice()
        .iter()
        .map(|tool| tool.id().to_owned())
        .collect();
    let mut restored = State::default();
    Model::update(&mut restored, Intent::RestoreFavorites(saved));
    assert_eq!(
        restored.favorites.as_slice(),
        rail.favorites.as_slice(),
        "round-trip is exact"
    );
}

/// The scripted-validation hook (`QUANTICK_DRAWING_MAGNET`) sets the same
/// flag the button does; nothing may fork the two.
#[test]
fn the_hook_setter_moves_the_same_flag_the_button_does() {
    let mut rail = State::default();
    Model::update(&mut rail, Intent::ConfirmMagnet(true));
    assert!(rail.magnet);
    Model::update(&mut rail, Intent::ConfirmMagnet(false));
    assert!(!rail.magnet);
}
#[test]
fn toolrail_has_one_owner_for_host_and_view_inputs() {
    let mut rail = crate::Adapter::<Model, (), Vec<Effect>>::new((), |effects| effects);
    let tool = Tool::Drawing(DRAWING_TOOLS[0]);
    let owner = std::ptr::from_ref(&rail.model_state);
    rail.update(Intent::Arm(tool));
    rail.update(Intent::Observe(Facts {
        count: 3,
        ..Facts::default()
    }));
    let mut effects = Vec::new();
    rail.with_updates(
        |batch| effects.extend(batch),
        |_, session| {
            assert_eq!(std::ptr::from_ref(session.state()), owner);
            assert_eq!(
                session.state().tool,
                tool,
                "view sees the host's armed tool"
            );
            assert_eq!(session.state().facts.count, 3);
            session.update(Intent::SetRepeat(true));
            assert!(
                session.state().repeat,
                "the next widget sees this event immediately"
            );
            session.update(Intent::SetManagerOpen(true));
            assert!(
                !session.state().facts.manager_open,
                "a request is not the host projection"
            );
            session.update(Intent::Observe(Facts {
                manager_open: true,
                ..session.state().facts
            }));
            assert!(
                session.state().facts.manager_open,
                "the host receipt reaches the same owner"
            );
            session.update(Intent::Arm(Tool::Crosshair));
            assert_eq!(session.state().tool, Tool::Crosshair);
        },
    );
    assert_eq!(
        rail.model_state.tool,
        Tool::Crosshair,
        "host reads the view's owner"
    );
    assert!(rail.model_state.repeat);
    assert_eq!(effects, vec![Effect::SetManagerOpen(true)]);
    rail.update(Intent::SetVisible(false));
    rail.with_updates(
        |_| {},
        |_, session| {
            assert!(!session.state().visible);
            assert_eq!(session.state().tool, Tool::Pointer);
        },
    );
}

#[test]
fn toolrail_favorites_acknowledgment_and_magnet_settlement_are_once() {
    let mut rail = crate::Adapter::<Model, (), Vec<Effect>>::new((), |effects| effects);
    let tool = DRAWING_TOOLS[0];
    rail.update(Intent::RestoreFavorites(vec![tool.id().to_owned()]));
    assert!(
        !rail.model_state.favorites_changed,
        "restoration does not request a save"
    );
    rail.with_updates(
        |_| {},
        |_, session| {
            session.update(Intent::ToggleFavorite(tool));
            session.update(Intent::ToggleFavorite(tool));
            assert_eq!(session.state().favorites, vec![tool]);
            assert!(session.state().favorites_changed);
            session.update(Intent::RequestMagnet);
            assert!(
                !session.state().magnet,
                "a request is not a control confirmation"
            );
            assert_eq!(session.state().magnet_request, Some(true));
        },
    );
    let mut saves = Vec::new();
    let mut requests = Vec::new();
    for _ in 0..2 {
        if rail.model_state.favorites_changed {
            saves.push(rail.model_state.favorites.clone());
            rail.update(Intent::AcknowledgeFavorites);
        }
        if let Some(request) = rail.model_state.magnet_request {
            requests.push(request);
            rail.update(Intent::AcknowledgeMagnet);
        }
    }
    assert_eq!(saves, vec![vec![tool]], "one dirty batch produces one save");
    assert_eq!(requests, vec![true], "one request reaches the host once");
    assert!(
        !rail.model_state.magnet,
        "acknowledgment cannot confirm a control operation"
    );
    rail.update(Intent::ConfirmMagnet(true));
    rail.with_updates(
        |_| {},
        |_, session| {
            assert!(
                session.state().magnet,
                "control settlement reaches the next view"
            );
            assert_eq!(session.state().magnet_request, None);
            session.update(Intent::RequestMagnet);
            assert_eq!(session.state().magnet_request, Some(false));
            assert!(
                session.state().magnet,
                "the next request still awaits confirmation"
            );
        },
    );
}

#[test]
fn repeat_and_reveal_follow_tool_changes_and_explicit_acknowledgment() {
    let mut rail = State::default();
    let tool = Tool::Drawing(DRAWING_TOOLS[0]);
    assert!(!rail.repeat);
    assert!(!rail.reveal_armed);
    Model::update(&mut rail, Intent::SetRepeat(true));
    Model::update(&mut rail, Intent::Arm(tool));
    assert!(rail.repeat, "arming a tool preserves the repeat pin");
    assert!(rail.reveal_armed);
    Model::update(&mut rail, Intent::Arm(tool));
    assert!(
        rail.reveal_armed,
        "repeated input cannot consume an unseen reveal"
    );
    Model::update(&mut rail, Intent::AcknowledgeReveal);
    Model::update(&mut rail, Intent::Arm(tool));
    assert!(
        !rail.reveal_armed,
        "the same armed tool does not pull back manual scrolling"
    );
    Model::update(&mut rail, Intent::Arm(Tool::Pointer));
    assert!(rail.reveal_armed, "a changed tool requests a fresh reveal");
    Model::update(&mut rail, Intent::SetRepeat(false));
    assert!(!rail.repeat);
    assert_eq!(rail.tool, Tool::Pointer);
}

#[test]
fn cancelled_drag_cannot_commit_its_preview_and_the_next_drag_can_dock() {
    let mut rail = State::default();
    Model::update(&mut rail, Intent::BeginDrag);
    Model::update(&mut rail, Intent::PreviewDrag(ToolboxDock::Top));
    assert!(rail.dragging);
    assert_eq!(rail.drag_preview, Some(ToolboxDock::Top));
    assert_eq!(
        rail.dock,
        ToolboxDock::Left,
        "preview does not reserve a new edge"
    );
    Model::update(&mut rail, Intent::CancelDrag);
    assert!(!rail.dragging);
    assert!(rail.drag_cancelled);
    assert_eq!(
        rail.drag_preview, None,
        "Escape removes the preview immediately"
    );
    Model::update(&mut rail, Intent::PreviewDrag(ToolboxDock::Bottom));
    assert_eq!(
        rail.drag_preview, None,
        "cancelled gestures cannot paint a stale target"
    );
    Model::update(&mut rail, Intent::EndDrag(Some(ToolboxDock::Top)));
    assert_eq!(
        rail.dock,
        ToolboxDock::Left,
        "release after Escape cannot dock"
    );
    assert_eq!(rail.drag_preview, None);
    assert!(!rail.drag_cancelled);
    Model::update(&mut rail, Intent::BeginDrag);
    Model::update(&mut rail, Intent::PreviewDrag(ToolboxDock::Bottom));
    Model::update(&mut rail, Intent::EndDrag(Some(ToolboxDock::Bottom)));
    assert_eq!(rail.dock, ToolboxDock::Bottom);
    assert!(!rail.dragging);
    assert_eq!(rail.drag_preview, None);
    Model::update(&mut rail, Intent::BeginDrag);
    Model::update(&mut rail, Intent::EndDrag(None));
    assert_eq!(
        rail.dock,
        ToolboxDock::Bottom,
        "release without a pointer keeps the dock"
    );
    assert!(!rail.dragging);
}
