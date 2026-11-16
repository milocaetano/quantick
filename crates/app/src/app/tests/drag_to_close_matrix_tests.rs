//! The drag-to-close matrix: every registered layout, every divider it
//! draws, folding each side of it, at two window sizes, in the main window
//! and in a market window torn off it, all through the pointer's own events
//! (a press on the divider, a move in steps, a release), never a call.
//!
//! Each case checks what the trader sees and what an agent reads: the folded
//! side leaves one 8 px rail and nothing else (no pane band, no line beside
//! it, no gutter), its neighbour comes up to the rail and keeps the far edge,
//! a click on the rail or beside it changes nothing (#510), and the rail
//! dragged back opens every pane where it stood. The scene and the canvas
//! readback name the rail where it is painted. Cases come from the preset
//! table and each preset's tree, so a registered layout is covered by being
//! registered. Every failure is gathered into one table before the test
//! fails, so a run says everything that is wrong at once.

use super::*;

use crate::canvas_layout::{CANVAS_DIVIDER_PX, COLLAPSED_PANE_WIDTH_PX, LAYOUT_PRESETS};
use crate::floating_panes::{model_rect, screen_rect};
use quantick_workspace::arrangement::{
    TabId,
    windows::{TransferTarget, WindowId},
};
use quantick_workspace::canvas::{
    LayoutPreset, PaneSlot, PaneTree, RailInk, SeamAddress, SplitAxis, slot_seat, tree_rails,
    tree_seams,
};
use std::{cell::RefCell, rc::Rc};

/// The two windows every case runs in: a trading monitor and a laptop.
const SIZES: [egui::Vec2; 2] = [egui::vec2(1600.0, 1000.0), egui::vec2(1100.0, 650.0)];
/// The layouts a torn-off market window runs the whole matrix for.
const TORN_OFF: [&str; 2] = ["time+time+flow", "charts-4"];
/// How many moves a drag takes between its press and its release.
const STEPS: usize = 12;
/// How far two edges may differ and still be one edge.
const SLACK: f32 = 0.5;
/// How far along its cross span a hand grabs a divider or a rail: off the
/// middle, where the dividers of a 2 x 2 grid cross.
const GRAB_AT: f32 = 0.3;
/// How far a middle rail is dragged open the way it did not fold: past the
/// fold threshold, short of the neighbour's floor.
const OTHER_WAY_PX: f32 = 120.0;
/// How far a drag that comes back goes first: under every floor and fold.
const OUT_AND_BACK_PX: f32 = 40.0;
/// Float rounding in a share, far under a point on any screen.
const ROUNDING: f32 = 1e-4;
/// A hand's jitter at the press, under any seam's pick.
const JITTER_PX: f32 = 1.0;
/// Most frames a layout gets to build its charts before a case starts.
const SETTLE_FRAMES: usize = 60;

/// The fills a frame painted, with their rectangles.
type Fills = Vec<(egui::Rect, egui::Color32)>;

fn fills(output: &egui::FullOutput) -> Fills {
    (painted_rects(output).into_iter())
        .map(|shape| (shape.rect, shape.fill))
        .collect()
}

/// A market window torn off the main one, painted as eframe paints it: its
/// own viewport, run inside the main window's frame, with its own pointer.
struct Child {
    /// The tab left in the main window, which no drag in this one may move.
    root_tab: u64,
    events: Rc<RefCell<Vec<egui::Event>>>,
    painted: Rc<RefCell<Fills>>,
}

/// Where a case's frames run, and the channel ends that keep its feeds open.
struct Rig {
    ctx: egui::Context,
    size: egui::Vec2,
    child: Option<Child>,
    /// The case's tab: the active one, or the torn-off window's.
    tab: u64,
    _feeds: (
        mpsc::Sender<FeedEvent>,
        mpsc::Receiver<FeedCommand>,
        mpsc::Sender<DepthEvent>,
        Option<TabEnds>,
    ),
}

impl Rig {
    /// One frame with `events` on the case's window: the fills painted there.
    fn frame(&self, app: &mut QuantickApp, events: Vec<egui::Event>) -> Fills {
        let none = egui::Modifiers::NONE;
        let Some(child) = &self.child else {
            return fills(&run_frame_sized(app, &self.ctx, self.size, events, none));
        };
        *child.events.borrow_mut() = events;
        run_frame_sized(app, &self.ctx, TEST_WINDOW, Vec::new(), none);
        child.painted.borrow().clone()
    }

    /// A press at `from`, [`STEPS`] moves to `to` and a release there, then
    /// a frame to settle: the fills that frame painted.
    fn drag(&self, app: &mut QuantickApp, from: egui::Pos2, to: egui::Pos2) -> Fills {
        self.frame(app, vec![egui::Event::PointerMoved(from)]);
        let press = vec![egui::Event::PointerMoved(from), pointer_button(from, true)];
        self.frame(app, press);
        for step in 1..=STEPS {
            let at = from.lerp(to, step as f32 / STEPS as f32);
            self.frame(app, vec![egui::Event::PointerMoved(at)]);
        }
        let release = vec![egui::Event::PointerMoved(to), pointer_button(to, false)];
        self.frame(app, release);
        self.frame(app, Vec::new())
    }

    /// A press at the first of `points`, [`STEPS`] moves to each next one,
    /// a release at the last, then a frame to settle.
    fn drag_through(&self, app: &mut QuantickApp, points: &[egui::Pos2]) {
        let from = points[0];
        self.frame(app, vec![egui::Event::PointerMoved(from)]);
        self.frame(
            app,
            vec![egui::Event::PointerMoved(from), pointer_button(from, true)],
        );
        for leg in points.windows(2) {
            for step in 1..=STEPS {
                let at = leg[0].lerp(leg[1], step as f32 / STEPS as f32);
                self.frame(app, vec![egui::Event::PointerMoved(at)]);
            }
        }
        let to = points[points.len() - 1];
        self.frame(
            app,
            vec![egui::Event::PointerMoved(to), pointer_button(to, false)],
        );
        self.frame(app, Vec::new());
    }

    /// A press and a release at `at` without a move between.
    fn click(&self, app: &mut QuantickApp, at: egui::Pos2) {
        self.frame(app, vec![egui::Event::PointerMoved(at)]);
        self.frame(
            app,
            vec![egui::Event::PointerMoved(at), pointer_button(at, true)],
        );
        self.frame(
            app,
            vec![egui::Event::PointerMoved(at), pointer_button(at, false)],
        );
        self.frame(app, Vec::new());
    }

    fn tab<'a>(&self, app: &'a QuantickApp) -> &'a crate::tab::Tab {
        tab_by_id(app, self.tab)
    }
}

fn tab_by_id(app: &QuantickApp, id: u64) -> &crate::tab::Tab {
    let mut tabs = app.runtime.tabs.iter_with_ids();
    tabs.find(|(held, _)| *held == id)
        .expect("the case's tab")
        .1
}

/// A fresh app at `size` laid out as `preset`, settled; `torn` tears the
/// case's tab off into a market window of its own, the main window keeping
/// a second tab laid out the same.
fn laid_out(preset: &'static LayoutPreset, size: egui::Vec2, torn: bool) -> (QuantickApp, Rig) {
    let ctx = egui::Context::default();
    pin_frame_clock(&ctx);
    let (mut app, events, commands, book) = test_app();
    run_frame_sized(&mut app, &ctx, size, Vec::new(), egui::Modifiers::NONE);
    app.apply_layout_preset(preset);
    let root_tab = app.runtime.tabs.active_id();
    let mut rig = Rig {
        ctx,
        size,
        child: None,
        tab: root_tab,
        _feeds: (events, commands, book, None),
    };
    if torn {
        rig._feeds.3 = Some(open_second_tab(&mut app, &rig.ctx, "ETHUSDT"));
        app.apply_layout_preset(preset);
        rig.tab = app.runtime.tabs.active_id();
        assert_ne!(rig.tab, root_tab, "the second market is its own tab");
        let (transfer, target) = (TabId(rig.tab), TransferTarget::NewWindow);
        let moved = app.runtime.tabs.transfer_tab(transfer, target, 0);
        let window = moved.expect("the tab tears off").destination;
        rig.child = Some(tear_off(&rig.ctx, window, size, root_tab));
    }
    // Every chart the layout names is built a frame at a time: settle until
    // each draws its band.
    for _ in 0..SETTLE_FRAMES {
        rig.frame(&mut app, Vec::new());
        if rig.tab(&app).canvas.drawn_bands.len() == preset.kinds.len() {
            break;
        }
    }
    rig.frame(&mut app, Vec::new());
    (app, rig)
}

/// Paint market window `window` as a native child `size` big, focused, its
/// pointer events whatever the rig queues for the frame.
fn tear_off(ctx: &egui::Context, window: WindowId, size: egui::Vec2, root_tab: u64) -> Child {
    let events: Rc<RefCell<Vec<egui::Event>>> = Rc::default();
    let painted: Rc<RefCell<Fills>> = Rc::default();
    let (queued, seen) = (Rc::clone(&events), Rc::clone(&painted));
    let id = super::super::native_windows::viewport(window);
    ctx.set_embed_viewports(false);
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut input = egui::RawInput {
            viewport_id: viewport.ids.this,
            screen_rect: Some(screen),
            events: if viewport.ids.this == id {
                std::mem::take(&mut *queued.borrow_mut())
            } else {
                Vec::new()
            },
            ..Default::default()
        };
        let info = input.viewports.entry(viewport.ids.this).or_default();
        info.native_pixels_per_point = Some(1.0);
        info.inner_rect = Some(screen);
        info.focused = Some(viewport.ids.this == id);
        let output = ctx.run(input, |ctx| (viewport.viewport_ui_cb)(ctx));
        if viewport.ids.this == id {
            *seen.borrow_mut() = fills(&output);
        }
    });
    Child {
        root_tab,
        events,
        painted,
    }
}

/// One case: fold the side of `seam` before it (`first`) or after it.
struct Case {
    preset: &'static LayoutPreset,
    size: egui::Vec2,
    seam: SeamAddress,
    axis: SplitAxis,
    first: bool,
    torn: bool,
}

impl Case {
    fn row(&self) -> String {
        let line = ["horizontal", "vertical"][usize::from(self.axis == SplitAxis::Row)];
        let way = match (self.axis, self.first) {
            (SplitAxis::Row, true) => "left",
            (SplitAxis::Row, false) => "right",
            (SplitAxis::Column, true) => "up",
            (SplitAxis::Column, false) => "down",
        };
        let window = if self.torn { "torn-off" } else { "main" };
        format!(
            "{} | {:?}/{} {line} | {way} | {window} {}x{}",
            self.preset.id, self.seam.path, self.seam.index, self.size.x, self.size.y
        )
    }
}

/// Every seam `tree` lays out, with the axis of its split: each split's
/// own, then its children's.
fn seams_of(tree: &PaneTree) -> Vec<(SeamAddress, SplitAxis)> {
    fn walk(node: &PaneTree, path: &mut Vec<usize>, out: &mut Vec<(SeamAddress, SplitAxis)>) {
        let PaneTree::Split { axis, children, .. } = node else {
            return;
        };
        for index in 0..children.len().saturating_sub(1) {
            let path = path.clone();
            out.push((SeamAddress { path, index }, *axis));
        }
        for (at, child) in children.iter().enumerate() {
            path.push(at);
            walk(child, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(tree, &mut Vec::new(), &mut out);
    out
}

/// The panes on each side of `seam` in `tree`, in reading order.
fn pair_of(tree: &PaneTree, seam: &SeamAddress) -> [Vec<PaneSlot>; 2] {
    let mut node = tree;
    for at in &seam.path {
        let PaneTree::Split { children, .. } = node else {
            unreachable!("a seam's path runs through splits");
        };
        node = &children[*at];
    }
    let PaneTree::Split { children, .. } = node else {
        unreachable!("a seam divides a split");
    };
    let slots = |child: &PaneTree| child.panes().into_iter().map(|(slot, _)| slot).collect();
    [
        slots(&children[seam.index]),
        slots(&children[seam.index + 1]),
    ]
}

/// Every case of the matrix: the presets in `ids` (all with `None`), each
/// size, each divider, each side.
fn cases(ids: Option<&[&str]>, torn: bool) -> Vec<Case> {
    let presets = LAYOUT_PRESETS.iter();
    let presets = presets.filter(|preset| ids.is_none_or(|ids| ids.contains(&preset.id)));
    let mut out = Vec::new();
    for preset in presets {
        for size in SIZES {
            for (seam, axis) in seams_of(preset.tree) {
                for first in [true, false] {
                    let seam = seam.clone();
                    out.push(Case {
                        preset,
                        size,
                        seam,
                        axis,
                        first,
                        torn,
                    });
                }
            }
        }
    }
    out
}

fn along(rect: egui::Rect, axis: SplitAxis) -> (f32, f32) {
    match axis {
        SplitAxis::Row => (rect.left(), rect.right()),
        SplitAxis::Column => (rect.top(), rect.bottom()),
    }
}

fn across(rect: egui::Rect, axis: SplitAxis) -> (f32, f32) {
    match axis {
        SplitAxis::Row => (rect.top(), rect.bottom()),
        SplitAxis::Column => (rect.left(), rect.right()),
    }
}

fn point(axis: SplitAxis, on: f32, off: f32) -> egui::Pos2 {
    match axis {
        SplitAxis::Row => egui::pos2(on, off),
        SplitAxis::Column => egui::pos2(off, on),
    }
}

fn union(rects: &[egui::Rect]) -> Option<egui::Rect> {
    rects.iter().copied().reduce(egui::Rect::union)
}

fn same(a: egui::Rect, b: egui::Rect) -> bool {
    (a.min - b.min).length() < SLACK && (a.max - b.max).length() < SLACK
}

/// The window's side for each of `slots`.
fn sides(tab: &crate::tab::Tab, slots: &[PaneSlot]) -> Vec<PaneSide> {
    let mut seats = tab.canvas.seats.clone();
    let seat = |slot: &PaneSlot| slot_seat(&tab.canvas.state, &mut seats, *slot);
    let seats: Vec<_> = slots.iter().filter_map(seat).collect();
    (seats.into_iter())
        .map(|seat| seat.map_or(PaneSide::Flow, PaneSide::Time))
        .collect()
}

/// The bands the window drew this frame for `sides`.
fn bands(tab: &crate::tab::Tab, sides: &[PaneSide]) -> Vec<egui::Rect> {
    let drawn = tab.canvas.drawn_bands.iter();
    let ours = drawn.filter(|(side, _)| sides.contains(side));
    ours.map(|(_, band)| *band).collect()
}

/// Every band the window drew, in a fixed order.
fn all_bands(tab: &crate::tab::Tab) -> Vec<(usize, egui::Rect)> {
    let drawn = tab.canvas.drawn_bands.iter();
    let mut out: Vec<_> = drawn.map(|(side, band)| (side.index(), *band)).collect();
    out.sort_by_key(|(index, _)| *index);
    out
}

/// Whether the window shows `slot` (`side`) folded: on a rail of the model,
/// or folded by the column painter's own stack.
fn folded(tab: &crate::tab::Tab, slot: PaneSlot, side: PaneSide) -> bool {
    let canvas = &tab.canvas;
    canvas.state.railed().contains(&slot)
        || (canvas.column_painted() && canvas.pane_collapsed(side))
}

/// What a click must leave alone: the canvas model, the column split and
/// the stack's heights and folds.
type Held = (
    quantick_workspace::canvas::CanvasState,
    f32,
    Vec<f32>,
    Vec<bool>,
);

/// Every share of `tree`, depth first.
fn tree_shares(tree: &PaneTree, out: &mut Vec<f32>) {
    if let PaneTree::Split {
        children, shares, ..
    } = tree
    {
        out.extend(shares.iter());
        for child in children.iter() {
            tree_shares(child, out);
        }
    }
}

/// Whether `tab` holds what `before` held but for float rounding in the
/// tree's shares and the split, far under a point on any screen: what a
/// drag replayed back to its press may leave. The stack's heights and every
/// fold must match exactly.
fn rounded(before: &Held, tab: &crate::tab::Tab) -> bool {
    let after = held(tab);
    let shares = |held: &Held| {
        let mut out = Vec::new();
        tree_shares(held.0.tree(), &mut out);
        out
    };
    let (was, now) = (shares(before), shares(&after));
    let close = |a: f32, b: f32| (a - b).abs() < ROUNDING;
    before.0.railed() == after.0.railed()
        && before.0.tree().panes() == after.0.tree().panes()
        && was.len() == now.len()
        && was.iter().zip(&now).all(|(a, b)| close(*a, *b))
        && close(before.1, after.1)
        && (&before.2, &before.3) == (&after.2, &after.3)
}

/// How `after` differs from `before`, if it does.
fn moved_by(before: &Held, after: &Held) -> Option<String> {
    if before == after {
        return None;
    }
    let read = |held: &Held| {
        let model = (
            held.0.tree().clone(),
            held.0.railed().iter().copied().collect::<Vec<_>>(),
        );
        format!(
            "model {model:?}, split {}, stack {:?} {:?}",
            held.1, held.2, held.3
        )
    };
    Some(format!("{} -> {}", read(before), read(after)))
}

fn held(tab: &crate::tab::Tab) -> Held {
    let canvas = &tab.canvas;
    (
        canvas.state.clone(),
        canvas.split_fraction,
        canvas.context_height_shares(),
        canvas.context_collapsed_slots(),
    )
}

/// Where the hand finds `seam` on screen, as the painter drawing it lays it.
fn divider(tab: &crate::tab::Tab, seam: &SeamAddress) -> Option<egui::Rect> {
    if tab.canvas.column_painted() {
        return match seam.path.as_slice() {
            [] => tab.canvas_divider_rect(),
            [0] => tab.context_divider_rect(seam.index),
            _ => None,
        };
    }
    let whole = model_rect(tab.canvas.last_canvas);
    let seams = tree_seams(&tab.canvas.state, whole, CANVAS_DIVIDER_PX).into_iter();
    let mut ours = seams.filter(|held| held.address == *seam);
    ours.next().map(|held| screen_rect(held.rect))
}

/// The rail the window paints for the fold of `slot` (`side`) at `seam`.
fn painted_rail(
    tab: &crate::tab::Tab,
    seam: &SeamAddress,
    (slot, side): (PaneSlot, PaneSide),
) -> Option<egui::Rect> {
    if tab.canvas.column_painted() {
        if seam.path.is_empty() {
            return tab.collapsed_rail_rect();
        }
        let PaneSide::Time(seat) = side else {
            return None;
        };
        let (_, bands) = tab.canvas.context_stack_geometry(&tab.time_panes)?;
        return bands.panes.get(seat).copied();
    }
    model_rail(tab, slot)
}

/// The rail the canvas model lays for the node whose first pane is `slot`.
fn model_rail(tab: &crate::tab::Tab, slot: PaneSlot) -> Option<egui::Rect> {
    let whole = model_rect(tab.canvas.last_canvas);
    let rails = tree_rails(&tab.canvas.state, whole, CANVAS_DIVIDER_PX).into_iter();
    let mut ours = rails.filter(|rail| rail.slot == slot);
    ours.next().map(|rail| screen_rect(rail.rect))
}

/// The boundary between the pair of `seam` along its axis in the canvas
/// model: where the first side ends.
fn model_boundary(tab: &crate::tab::Tab, slots: &[PaneSlot], axis: SplitAxis) -> f32 {
    let whole = model_rect(tab.canvas.last_canvas);
    let rects = tab.canvas.state.rects(whole).into_iter();
    let ours: Vec<egui::Rect> = (rects.filter(|(slot, _)| slots.contains(slot)))
        .map(|(_, rect)| screen_rect(rect))
        .collect();
    along(union(&ours).unwrap_or(egui::Rect::NOTHING), axis).1
}

fn scene_rect(bounds: &quantick_control_schema::scene::SceneBoundsSnapshot) -> Option<egui::Rect> {
    let value = |decimal: &quantick_control::wire::CanonicalDecimal| decimal.as_str().parse().ok();
    let (x, y) = (value(&bounds.x_pt)?, value(&bounds.y_pt)?);
    let (w, h) = (value(&bounds.width_pt)?, value(&bounds.height_pt)?);
    Some(egui::Rect::from_min_size(
        egui::pos2(x, y),
        egui::vec2(w, h),
    ))
}

/// What a case reads before its fold: the pair, where the hand grabs, and
/// every band as the window drew it.
struct Before {
    pair: [Vec<PaneSlot>; 2],
    /// The side that folds and the side that stays, as slots and as sides.
    fold: (Vec<PaneSlot>, Vec<PaneSide>),
    open: (Vec<PaneSlot>, Vec<PaneSide>),
    /// The divider's centre along the axis, and the pair's band.
    divider: f32,
    pair_band: egui::Rect,
    /// Where the hand grabs the divider across it.
    off: f32,
    /// The boundary between the pair in the canvas model.
    boundary: f32,
    bands: Vec<(usize, egui::Rect)>,
}

fn read_before(case: &Case, tab: &crate::tab::Tab) -> Result<Before, String> {
    let pair = pair_of(tab.canvas.state.tree(), &case.seam);
    let [first, second] = pair.clone();
    let (fold, open) = if case.first {
        (first, second)
    } else {
        (second, first)
    };
    let (fold_sides, open_sides) = (sides(tab, &fold), sides(tab, &open));
    let column = tab.canvas.column_painted();
    let grab = divider(tab, &case.seam)
        .ok_or_else(|| format!("no divider drawn (column painter: {column})"))?;
    let pair_band = union(&bands(
        tab,
        &[fold_sides.clone(), open_sides.clone()].concat(),
    ))
    .ok_or("the pair draws no band")?;
    let (cross_lo, cross_hi) = across(grab, case.axis);
    let on = along(grab, case.axis);
    Ok(Before {
        boundary: model_boundary(tab, &pair[0], case.axis),
        pair,
        fold: (fold, fold_sides),
        open: (open, open_sides),
        divider: (on.0 + on.1) / 2.0,
        pair_band,
        off: cross_lo + (cross_hi - cross_lo) * GRAB_AT,
        bands: all_bands(tab),
    })
}

/// The fold just made: the folded side shows as exactly its rail, the
/// neighbour comes up to it, and the model, the scene and the canvas read
/// say so where it is painted. Answers the painted rail, `None` when there
/// is none to go on with.
fn check_fold(
    (case, rig, app): (&Case, &Rig, &QuantickApp),
    before: &Before,
    painted: &Fills,
    found: &mut Vec<String>,
) -> Option<egui::Rect> {
    let (axis, tab) = (case.axis, rig.tab(app));
    let ((fold, fold_sides), (open, open_sides)) = (&before.fold, &before.open);
    let unfolded = |(slot, side): (&PaneSlot, &PaneSide)| !folded(tab, *slot, *side);
    let left: Vec<_> = fold
        .iter()
        .zip(fold_sides)
        .filter(|at| unfolded(*at))
        .collect();
    if !left.is_empty() {
        let drawn = union(&bands(tab, fold_sides));
        found.push(format!("did not fold: {left:?} still drawn at {drawn:?}"));
        return None;
    }
    let opened: Vec<_> = open
        .iter()
        .zip(open_sides)
        .filter(|at| !unfolded(*at))
        .collect();
    if !opened.is_empty() {
        found.push(format!("the open side folded too: {opened:?}"));
    }
    let column = tab.canvas.column_painted();
    let Some(rail) = painted_rail(tab, &case.seam, (fold[0], fold_sides[0])) else {
        found.push(format!(
            "folded, but no rail is painted (column painter: {column})"
        ));
        return None;
    };
    // Exactly the rail: 8 px, from the pair's edge to the neighbour.
    let ((lo, hi), (rail_lo, rail_hi)) = (along(before.pair_band, axis), along(rail, axis));
    if ((rail_hi - rail_lo) - COLLAPSED_PANE_WIDTH_PX).abs() > SLACK {
        found.push(format!(
            "the rail is {} px, not 8: {rail:?}",
            rail_hi - rail_lo
        ));
    }
    let outer = if case.first {
        rail_lo - lo
    } else {
        hi - rail_hi
    };
    if outer.abs() > SLACK {
        found.push(format!("the rail stands {outer} px off the pair's edge"));
    }
    let leftover = bands(tab, fold_sides);
    if !leftover.is_empty() {
        found.push(format!("a folded pane still draws its band: {leftover:?}"));
    }
    match union(&bands(tab, open_sides)).map(|band| along(band, axis)) {
        None => found.push("the neighbour draws no band".to_owned()),
        Some((near, far)) => {
            let (gap, short) = if case.first {
                (near - rail_hi, hi - far)
            } else {
                (rail_lo - far, near - lo)
            };
            if gap.abs() > SLACK {
                found.push(format!(
                    "a {gap} px gutter between the rail and the neighbour"
                ));
            }
            if short.abs() > SLACK {
                found.push(format!(
                    "the neighbour stops {short} px short of the far edge"
                ));
            }
        }
    }
    let (rail_cross, pair_cross) = (across(rail, axis), across(before.pair_band, axis));
    let off_cross = (rail_cross.0 - pair_cross.0)
        .abs()
        .max((rail_cross.1 - pair_cross.1).abs());
    if off_cross > SLACK {
        found.push(format!(
            "the rail spans {rail_cross:?} across, the pair {pair_cross:?}"
        ));
    }
    check_paint(case, (rig, rail), painted, found);
    check_reads((app, tab), (fold, fold_sides), rail, found);
    Some(rail)
}

/// The rail's band is painted in a rail ink, and no divider line lies on
/// the rail or in the gutter a divider would take beside it.
fn check_paint(
    case: &Case,
    (rig, rail): (&Rig, egui::Rect),
    painted: &Fills,
    found: &mut Vec<String>,
) {
    let palette = crate::theme::get(&rig.ctx);
    let band = RailInk::Band.index();
    let inks =
        [false, true].map(|hot| *palette.get(quantick_appearance::rail::rail_inks(hot)[band]));
    if !(painted.iter()).any(|(at, fill)| same(*at, rail) && inks.contains(fill)) {
        found.push(format!("no rail band painted at {rail:?}"));
    }
    let (lo, hi) = along(rail, case.axis);
    let (from, to) = if case.first {
        (lo, hi + CANVAS_DIVIDER_PX)
    } else {
        (lo - CANVAS_DIVIDER_PX, hi)
    };
    let strip = match case.axis {
        SplitAxis::Row => egui::Rect::from_x_y_ranges(from..=to, rail.y_range()),
        SplitAxis::Column => egui::Rect::from_x_y_ranges(rail.x_range(), from..=to),
    };
    // A line along the rail: a divider across it, ending at its edge, is the
    // neighbour's own and stays.
    let on_strip = |at: &egui::Rect| {
        let shared = at.intersect(strip);
        let (on, off) = (along(shared, case.axis), across(shared, case.axis));
        on.1 - on.0 > SLACK && off.1 - off.0 > on.1 - on.0
    };
    let lines: Vec<_> = (painted.iter())
        .filter(|(at, fill)| *fill == palette.border() && on_strip(at))
        .map(|(at, _)| *at)
        .collect();
    if !lines.is_empty() {
        found.push(format!("a divider line on or beside the rail: {lines:?}"));
    }
}

/// Two surfaces: the model's rail, the scene's and the canvas read agree
/// with the rail painted for the folded `slots`.
fn check_reads(
    (app, tab): (&QuantickApp, &crate::tab::Tab),
    (slots, sides): (&[PaneSlot], &[PaneSide]),
    rail: egui::Rect,
    found: &mut Vec<String>,
) {
    let railed = tab.canvas.state.railed().contains(&slots[0]);
    match (railed, model_rail(tab, slots[0])) {
        (true, Some(model)) if !same(model, rail) => {
            found.push(format!(
                "the model lays the rail at {model:?}, painted at {rail:?}"
            ));
        }
        (true, None) => found.push("the model rails it but lays no rail".to_owned()),
        _ => {}
    }
    let scene = crate::control::scene_snapshot(app);
    let rails: Vec<_> = (scene.controls.iter())
        .filter(|control| control.control_id.ends_with(".rail"))
        .filter_map(|control| control.bounds.as_ref().and_then(scene_rect))
        .collect();
    if !rails.iter().any(|at| same(*at, rail)) {
        found.push(format!(
            "the scene lists no rail at {rail:?} (its rails: {rails:?})"
        ));
    }
    for side in sides {
        let id = format!("pane.{}.canvas", tab.pane(*side).id);
        let shown = (scene.controls.iter()).find(|control| control.control_id == id);
        if let Some(bounds) = shown.and_then(|control| control.bounds.as_ref()) {
            let at = scene_rect(bounds);
            found.push(format!("the scene still bounds folded {id} at {at:?}"));
        }
    }
    let drawn = crate::app::canvas_wiring::drawn(&tab.canvas);
    let view = quantick_control_handlers::canvas::view(&tab.canvas.state, drawn);
    let mut read: Vec<PaneSlot> = (view.panes.iter())
        .filter(|pane| pane.railed)
        .map(|pane| pane.slot)
        .collect();
    let mut shown = slots.to_vec();
    read.sort_unstable();
    shown.sort_unstable();
    if read != shown {
        found.push(format!(
            "the canvas read rails {read:?}, the window folds {shown:?}"
        ));
    }
}

/// How far the hand drags the rail at `rail_mid` to put the boundary back
/// where it stood: the column painter's divider counts from its centre and
/// a rail from its inner edge (the flow's) or its middle (the context's),
/// the stack's boundary from the divider beside the rail, and a seam from
/// the boundary the model holds: each moves by the pointer's travel.
fn travel_back(case: &Case, tab: &crate::tab::Tab, before: &Before, rail: egui::Rect) -> f32 {
    let (lo, hi) = along(rail, case.axis);
    let rail_mid = (lo + hi) / 2.0;
    match (tab.canvas.column_painted(), case.seam.path.is_empty()) {
        (true, true) => {
            let start = along(tab.canvas.last_canvas, case.axis).0;
            let at = if case.first {
                start + rail_mid - lo
            } else {
                lo
            };
            before.divider - at
        }
        (true, false) => {
            let now = tab.context_divider_rect(case.seam.index);
            now.map_or(0.0, |at| {
                before.divider - (along(at, case.axis).0 + along(at, case.axis).1) / 2.0
            })
        }
        (false, _) => before.boundary - model_boundary(tab, &before.pair[0], case.axis),
    }
}

/// The rail of the fold just made again, dragged open the way its chevron
/// points when that is not back the way it came, and both ways when it
/// stands between two open charts: each drag must open it. A chevron on a
/// rail at the edge of its split points back the way it came, which the
/// drag back already proved; one that points elsewhere is a finding.
fn check_other_way(
    (case, rig, app): (&Case, &Rig, &mut QuantickApp),
    (before, back): (&Before, f32),
    found: &mut Vec<String>,
) {
    let tab = rig.tab(app);
    if tab.canvas.column_painted() {
        return;
    }
    let whole = model_rect(tab.canvas.last_canvas);
    let slot = before.fold.0[0];
    let rails = tree_rails(&tab.canvas.state, whole, CANVAS_DIVIDER_PX);
    let Some(rail) = rails.into_iter().find(|rail| rail.slot == slot) else {
        found.push("folded again, no rail".to_owned());
        return;
    };
    let seams = tree_seams(&tab.canvas.state, whole, CANVAS_DIVIDER_PX).into_iter();
    let beside = seams.filter(|seam| seam.resizable && seam.rail.is_some_and(|at| at.slot == slot));
    let middle = beside.count() == 2;
    let chevron = if rail.meets_after { 1.0 } else { -1.0 };
    if !middle && chevron * back < 0.0 {
        found.push("the chevron points away from the way the rail opens".to_owned());
        return;
    }
    let ways: &[f32] = if middle { &[-1.0, 1.0] } else { &[] };
    for way in ways {
        let tab = rig.tab(app);
        let Some(rect) = model_rail(tab, slot) else {
            found.push("folded again, no rail".to_owned());
            return;
        };
        let (lo, hi) = along(rect, case.axis);
        let (cross_lo, cross_hi) = across(rect, case.axis);
        let at = |on: f32| point(case.axis, on, cross_lo + (cross_hi - cross_lo) * GRAB_AT);
        let mid = (lo + hi) / 2.0;
        // A hand's 1 px jitter the other way first must not pick the seam.
        let jitter = at(mid - way * JITTER_PX);
        rig.drag_through(app, &[at(mid), jitter, at(mid + way * OTHER_WAY_PX)]);
        let tab = rig.tab(app);
        let pair = before.pair.concat();
        let shut =
            (pair.iter().zip(sides(tab, &pair))).any(|(slot, side)| folded(tab, *slot, side));
        if shut {
            found.push(format!(
                "dragged {way:+} from the middle rail, still folded"
            ));
            return;
        }
        // Folded again for the next way, from where the divider now stands.
        let Ok(again) = read_before(case, rig.tab(app)) else {
            found.push("opened, but its divider is not drawn".to_owned());
            return;
        };
        let (pair_lo, pair_hi) = along(again.pair_band, case.axis);
        let edge = if case.first {
            pair_lo + 1.0
        } else {
            pair_hi - 1.0
        };
        let press = point(case.axis, again.divider, again.off);
        rig.drag(app, press, point(case.axis, edge, again.off));
    }
}

/// One case run start to end; what went wrong, in the order it was found.
fn run_case(case: &Case) -> Vec<String> {
    let (mut app, rig) = laid_out(case.preset, case.size, case.torn);
    let app = &mut app;
    let root_before = (rig.child.as_ref()).map(|child| held(tab_by_id(app, child.root_tab)));
    let before = match read_before(case, rig.tab(app)) {
        Ok(before) => before,
        Err(why) => return vec![why],
    };
    let mut found = Vec::new();
    let axis = case.axis;
    let press = point(axis, before.divider, before.off);
    let still = held(rig.tab(app));
    rig.click(app, press);
    if let Some(how) = moved_by(&still, &held(rig.tab(app))) {
        found.push(format!(
            "a click on the open divider changed the canvas: {how}"
        ));
    }
    // A drag that comes back to where it was pressed moves nothing.
    let still = held(rig.tab(app));
    let out = point(axis, before.divider + OUT_AND_BACK_PX, before.off);
    rig.drag_through(app, &[press, out, press]);
    if let Some(how) =
        moved_by(&still, &held(rig.tab(app))).filter(|_| !rounded(&still, rig.tab(app)))
    {
        found.push(format!("a drag out and back changed the canvas: {how}"));
    }
    // The fold, dragged to just inside the pair's far edge.
    let (lo, hi) = along(before.pair_band, axis);
    let edge = if case.first { lo + 1.0 } else { hi - 1.0 };
    let painted = rig.drag(app, press, point(axis, edge, before.off));
    let Some(rail) = check_fold((case, &rig, app), &before, &painted, &mut found) else {
        return found;
    };
    // A click on the rail, and on the divider's place beside it, moves nothing.
    let (rail_lo, rail_hi) = along(rail, axis);
    let rail_mid = (rail_lo + rail_hi) / 2.0;
    let beside = if case.first {
        rail_hi + CANVAS_DIVIDER_PX / 2.0
    } else {
        rail_lo - CANVAS_DIVIDER_PX / 2.0
    };
    let (cross_lo, cross_hi) = across(rail, axis);
    let off = cross_lo + (cross_hi - cross_lo) * GRAB_AT;
    let still = held(rig.tab(app));
    for (what, at) in [("rail", rail_mid), ("divider beside it", beside)] {
        rig.click(app, point(axis, at, off));
        if let Some(how) = moved_by(&still, &held(rig.tab(app))) {
            found.push(format!("a click on the {what} changed the canvas: {how}"));
        }
    }
    // The rail dragged back to where the divider stood opens it there.
    let travel = travel_back(case, rig.tab(app), &before, rail);
    rig.drag(
        app,
        point(axis, rail_mid, off),
        point(axis, rail_mid + travel, off),
    );
    let tab = rig.tab(app);
    let pair = before.pair.concat();
    let shut: Vec<_> = (pair.iter().zip(sides(tab, &pair)))
        .filter(|(slot, side)| folded(tab, **slot, *side))
        .map(|(slot, _)| *slot)
        .collect();
    if !shut.is_empty() {
        found.push(format!("dragged back, still folded: {shut:?}"));
        return found;
    }
    let after = all_bands(tab);
    let near = |a: egui::Rect, b: egui::Rect| {
        (a.min - b.min).length() <= 1.0 && (a.max - b.max).length() <= 1.0
    };
    let moved: Vec<_> = (before.bands.iter().zip(&after))
        .filter(|((a, at), (b, bt))| a != b || !near(*at, *bt))
        .collect();
    if before.bands.len() != after.len() || !moved.is_empty() {
        found.push(format!("reopened elsewhere: {moved:?}"));
    }
    // Folded again, the rail opens the way its chevron points too.
    rig.drag(app, press, point(axis, edge, before.off));
    check_other_way((case, &rig, app), (&before, travel), &mut found);
    if let (Some(child), Some(root)) = (&rig.child, root_before)
        && held(tab_by_id(app, child.root_tab)) != root
    {
        found.push("the main window's tab moved".to_owned());
    }
    found
}

/// Findings that wait on a decision, listed rather than fixed: a layout, a
/// divider and the start of what remained. Each must still be found, so the
/// list cannot outlive its finding.
///
/// "2 stacked + Flow" folds a stacked chart in the column painter's own
/// stack, outside the canvas model (#1008 kept it there): the window paints
/// its rail, but the scene names none and the canvas read says it is open.
/// Closing that gap means moving the stack's folds into the model.
const AWAITING_DECISION: [(&str, &str, &str); 2] = [
    ("stack2+flow", "[0]/0", "the scene lists no rail"),
    ("stack2+flow", "[0]/0", "the canvas read rails []"),
];

/// Which entry of [`AWAITING_DECISION`] `case`'s finding `what` is, if any.
fn awaiting(case: &Case, what: &str) -> Option<usize> {
    let divider = format!("{:?}/{}", case.seam.path, case.seam.index);
    (AWAITING_DECISION.iter()).position(|(layout, at, start)| {
        case.preset.id == *layout && divider == *at && what.starts_with(start)
    })
}

/// Run `cases`, each on a fresh app; panic with the table of every failure
/// not awaiting a decision, or when a listed one is no longer found.
fn run_matrix(cases: &[Case]) {
    let mut table = Vec::new();
    let mut seen = [false; AWAITING_DECISION.len()];
    for case in cases {
        for what in run_case(case) {
            match awaiting(case, &what) {
                Some(at) => seen[at] = true,
                None => table.push(format!("{} | {what}", case.row())),
            }
        }
    }
    for ((layout, at, start), seen) in AWAITING_DECISION.iter().zip(seen) {
        let ran = cases.iter().any(|case| case.preset.id == *layout);
        assert!(
            seen || !ran,
            "{layout} {at} \"{start}\" is no longer found: drop it from the list"
        );
    }
    assert!(
        table.is_empty(),
        "{} of {} cases left something behind:\nlayout | divider | direction | window | what remained\n{}",
        table.len(),
        cases.len(),
        table.join("\n")
    );
}

/// Every layout, every divider, each side, both window sizes, in the main
/// window, through pointer events.
#[test]
fn every_divider_of_every_layout_closes_by_dragging_in_the_main_window() {
    let cases = cases(None, false);
    let layouts = LAYOUT_PRESETS
        .iter()
        .filter(|preset| !seams_of(preset.tree).is_empty());
    assert_eq!(
        cases.len(),
        layouts
            .map(|preset| seams_of(preset.tree).len() * 2 * SIZES.len())
            .sum::<usize>(),
        "every divider of every layout, both sides, both sizes"
    );
    run_matrix(&cases);
}

/// The same matrix in a market window torn off the main one, which keeps
/// a tab laid out the same that no drag there may move.
#[test]
fn every_divider_closes_by_dragging_in_a_torn_off_market_window() {
    run_matrix(&cases(Some(&TORN_OFF), true));
}
