//! The window follows the canvas model: focus leaves what the model folds,
//! the divider is the tree's one share whichever painter draws, a folded
//! chart's whole strip restores it, and the readback says what is drawn.
use super::*;

use crate::canvas_layout::preset;
use quantick_control_handlers::canvas::{CanvasAct, CanvasHost};
use quantick_workspace::canvas::{PaneMode, PaneSlot, seat_slot};

fn frames(app: &mut QuantickApp, ctx: &egui::Context, count: usize) -> egui::FullOutput {
    for _ in 1..count {
        run_frame(app, ctx);
    }
    run_frame(app, ctx)
}

/// A tab laid out as `id`, its charts built.
fn laid_out(id: &str) -> (QuantickApp, egui::Context) {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    app.apply_layout_preset(preset(id).unwrap());
    frames(&mut app, &ctx, 6);
    (app, ctx)
}

/// The canvas slot that shows `side`.
fn slot_of(app: &QuantickApp, side: PaneSide) -> PaneSlot {
    let canvas = &app.active_tab().canvas;
    let seat = side.index().checked_sub(1);
    seat_slot(&canvas.state, &mut canvas.seats.clone(), seat).expect("the canvas shows it")
}

fn act(app: &mut QuantickApp, act: CanvasAct) -> Option<PaneSlot> {
    app.act_on_canvas(act)
        .expect("the canvas takes the act")
        .slot
}

fn band(app: &QuantickApp, side: PaneSide) -> Option<egui::Rect> {
    let bands = app.active_tab().canvas.drawn_bands.iter();
    bands
        .filter(|(drawn, _)| *drawn == side)
        .map(|(_, rect)| *rect)
        .next()
}

fn drawn(app: &QuantickApp, id: &str) -> Option<egui::Rect> {
    (app.active_tab().canvas.menus.drawn.iter())
        .find(|control| control.id == id)
        .map(|control| control.rect)
}

/// C6: the focused chart, minimized, hands focus to a drawn chart, and the
/// focus frame shows on it.
#[test]
fn focus_leaves_a_minimized_pane() {
    let (mut app, ctx) = laid_out("time+time+flow");
    app.active_tab_mut().focus = PaneSide::Time(0);
    frames(&mut app, &ctx, 2);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
    let slot = slot_of(&app, PaneSide::Time(0));
    act(&mut app, CanvasAct::Minimize { slot });
    let output = frames(&mut app, &ctx, 3);
    let tab = app.active_tab();
    let focused = tab.focused_side();
    assert_ne!(focused, PaneSide::Time(0), "focus left the folded chart");
    assert!(tab.drawn_panes().shows(focused), "{focused:?} is drawn");
    assert!(!tab.drawn_panes().shows(PaneSide::Time(0)), "a strip only");
    let band = band(&app, focused).expect("the focused chart has a band");
    let palette = crate::theme::get(&ctx);
    let frame = egui::Stroke::new(crate::tab::FOCUS_FRAME_PX, palette.widget_active_border());
    assert!(
        painted_rects(&output)
            .iter()
            .any(|shape| shape.stroke == frame && band.expand(1.0).contains_rect(shape.rect)),
        "the focus frame is drawn around {focused:?}"
    );
}

/// C7: a divider dragged to a quarter stays there while a float makes the
/// tree painter draw, through a minimize and a restore, and back on the
/// column painter.
#[test]
fn minimizing_keeps_the_dragged_split() {
    let (mut app, ctx) = laid_out("time+flow");
    let area = app.active_tab().canvas.last_canvas;
    let time = band(&app, PaneSide::Time(0)).expect("the timeframe chart");
    let flow = band(&app, PaneSide::Flow).expect("the flow chart");
    let grab = egui::pos2((time.right() + flow.left()) / 2.0, area.center().y);
    let quarter = egui::pos2(area.left() + area.width() * 0.25, grab.y);
    drag_chart(&mut app, &ctx, grab, quarter);
    frames(&mut app, &ctx, 2);
    let split = app.active_tab().canvas.split_fraction;
    assert!(
        (split - 0.25).abs() < 0.005,
        "dragged to a quarter: {split}"
    );
    let divider = |app: &QuantickApp| band(app, PaneSide::Flow).expect("the flow chart").left();
    let dragged = divider(&app);
    let kept = |app: &QuantickApp, when: &str| {
        let (at, split) = (divider(app), app.active_tab().canvas.split_fraction);
        assert!((at - dragged).abs() < 1.0, "{when}: {at} vs {dragged}");
        assert!((split - 0.25).abs() < 0.005, "{when}: split {split}");
    };

    let widget_id = "timeframe-chart".to_owned();
    let float = act(&mut app, CanvasAct::AddWidget { widget_id }).expect("a new chart");
    frames(&mut app, &ctx, 6);
    assert!(!app.active_tab().canvas.column_painted(), "the tree draws");
    kept(&app, "a float added");

    let slot = slot_of(&app, PaneSide::Time(0));
    act(&mut app, CanvasAct::Minimize { slot });
    frames(&mut app, &ctx, 3);
    act(&mut app, CanvasAct::Restore { slot });
    frames(&mut app, &ctx, 3);
    kept(&app, "minimized and restored");

    let slot = float;
    act(
        &mut app,
        CanvasAct::Remove {
            slot,
            confirm: true,
        },
    );
    frames(&mut app, &ctx, 3);
    assert!(app.active_tab().canvas.column_painted(), "the columns draw");
    kept(&app, "the float removed");
}

/// C8: a click anywhere along a folded chart's strip restores it, and its ⋯
/// opens the chart's menu instead.
#[test]
fn a_click_anywhere_on_a_strip_restores_it() {
    let (mut app, ctx) = laid_out("time+flow");
    let slot = slot_of(&app, PaneSide::Time(0));
    for along in [0.5, 0.85] {
        act(&mut app, CanvasAct::Minimize { slot });
        frames(&mut app, &ctx, 3);
        let id = format!("pane_menu.{slot}.strip");
        let strip = drawn(&app, &id).expect("the strip");
        assert!(strip.height() > strip.width() * 10.0, "the whole strip");
        let point = egui::pos2(strip.center().x, strip.top() + strip.height() * along);
        click_sized(&mut app, &ctx, TEST_WINDOW, point);
        frames(&mut app, &ctx, 2);
        let mode = app.active_tab().canvas.state.mode(slot);
        assert_eq!(mode, Some(PaneMode::Docked), "{along} down the strip");
    }

    act(&mut app, CanvasAct::Minimize { slot });
    frames(&mut app, &ctx, 3);
    let strip = drawn(&app, &format!("pane_menu.{slot}.strip")).unwrap();
    let door = drawn(&app, &format!("pane_menu.{slot}")).expect("the strip's ⋯");
    assert!(strip.contains_rect(door), "{door:?} inside {strip:?}");
    click_sized(&mut app, &ctx, TEST_WINDOW, door.center());
    frames(&mut app, &ctx, 2);
    let id = app.runtime.tabs.active_id();
    let open = app.surfaces.remove_chart.scoped(id).0.menu_open();
    assert_eq!(open, Some(slot), "the ⋯ opens the menu");
    let tab = app.active_tab();
    assert_eq!(tab.canvas.state.mode(slot), Some(PaneMode::Minimized));
}

/// C17: the summary says what the canvas draws: a minimized chart neither
/// visible nor focused, a floating one visible.
#[test]
fn summary_matches_what_the_canvas_draws() {
    let (mut app, ctx) = laid_out("time+time+flow");
    app.active_tab_mut().focus = PaneSide::Time(0);
    frames(&mut app, &ctx, 2);
    let slot = slot_of(&app, PaneSide::Time(0));
    act(&mut app, CanvasAct::Minimize { slot });
    let widget_id = "timeframe-chart".to_owned();
    act(&mut app, CanvasAct::AddWidget { widget_id });
    frames(&mut app, &ctx, 6);
    assert_eq!(app.active_tab().time_panes.len(), 3, "the float is built");

    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("workspace.summary");
    let capture = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    let panes = capture.scopes[&scope].value["tabs"][0]["panes"].clone();
    let pane = |index: usize| {
        (panes.as_array().unwrap().iter())
            .find(|pane| pane["pane_index"].to_string().trim_matches('"') == index.to_string())
            .unwrap_or_else(|| panic!("pane {index} in {panes}"))
            .clone()
    };
    let folded = pane(PaneSide::Time(0).index());
    assert_eq!(folded["visible"], false, "a strip is not the chart");
    assert_eq!(folded["focused"], false, "focus left it");
    let floating = pane(PaneSide::Time(2).index());
    assert_eq!(floating["visible"], true, "a float is on screen");
    let focused = (panes.as_array().unwrap().iter()).filter(|pane| pane["focused"] == true);
    assert_eq!(focused.count(), 1, "one focused chart");
}

/// #1057: a stacked chart folded by the stack's own resize is the canvas
/// model's fold, its rail published under the slot's restore with the
/// rail's words, and a restore by that name reopens it where it stood.
#[test]
fn a_stacked_fold_is_the_models_and_restores_by_name() {
    use quantick_control_handlers::layout::LayoutTab as _;
    let (mut app, ctx) = laid_out("stack2+flow");
    let lower = PaneSide::Time(1);
    let was = band(&app, lower).expect("the lower chart");
    let slot = slot_of(&app, lower);
    let tab_id = app.runtime.tabs.active_id();
    let tab = app.active_tab_mut();
    let pair = (tab.time_panes[0].id, tab.time_panes[1].id);
    let agent = quantick_control::wire::ActorKind::Agent;
    let folded = tab.resize_context_pair(tab_id, pair, 1.0, agent);
    assert!(matches!(folded, Some(Ok((_, true)))), "{folded:?}");
    frames(&mut app, &ctx, 2);
    assert!(app.active_tab().canvas.state.railed().contains(&slot));
    let rail = format!("pane_menu.{slot}.rail");
    let words = (app.active_tab().canvas.menus.drawn.iter())
        .find(|control| control.id == rail)
        .and_then(|control| control.description.clone());
    assert!(
        words.is_some_and(|words| words.starts_with("Time · ") && words.ends_with("drag to open")),
        "the rail is published by its restore"
    );
    act(&mut app, CanvasAct::Restore { slot });
    frames(&mut app, &ctx, 2);
    let back = band(&app, lower).expect("reopened");
    let near = (back.min - was.min).length() < 1.0 && (back.max - was.max).length() < 1.0;
    assert!(near, "reopened at {back:?}, stood at {was:?}");
}

/// A stacked chart folded inside a column folded whole: the context's
/// expand reopens the column with the chart still folded, read and drawn
/// alike on the same frame; a layout picked again carries nothing over.
#[test]
fn an_expand_reads_the_inner_fold_the_window_draws() {
    use quantick_control_handlers::layout::LayoutTab as _;
    let (mut app, ctx) = laid_out("stack2+flow");
    let lower = slot_of(&app, PaneSide::Time(1));
    let tab_id = app.runtime.tabs.active_id();
    let tab = app.active_tab_mut();
    let pair = (tab.time_panes[0].id, tab.time_panes[1].id);
    let agent = quantick_control::wire::ActorKind::Agent;
    tab.resize_context_pair(tab_id, pair, 1.0, agent);
    frames(&mut app, &ctx, 2);
    assert!(app.active_tab_mut().set_context_collapsed(true));
    frames(&mut app, &ctx, 2);
    assert!(app.active_tab_mut().set_context_collapsed(false));
    let railed = |app: &QuantickApp| app.active_tab().canvas.state.railed().clone();
    assert_eq!(
        railed(&app).into_iter().collect::<Vec<_>>(),
        [lower],
        "read"
    );
    frames(&mut app, &ctx, 2);
    assert_eq!(
        railed(&app).into_iter().collect::<Vec<_>>(),
        [lower],
        "drawn"
    );
    assert!(band(&app, PaneSide::Time(1)).is_none(), "painted folded");

    assert!(app.active_tab_mut().set_context_collapsed(true));
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    frames(&mut app, &ctx, 2);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    frames(&mut app, &ctx, 6);
    assert!(railed(&app).is_empty());
    assert!(band(&app, PaneSide::Time(1)).is_some(), "both charts open");
}
