// The `toolrail.rs` unit tests, moved out of the file so a session opening
// the rail no longer reads 1,323 lines of tests it did not ask for. The
// rail's own `#[cfg(test)]` instrumentation -- its recorders, probe fields
// and initialisers -- stays in the host, where the production code that
// carries it lives.
//
// They stay a child module of `crate::toolrail` rather than moving to an
// integration test: a child sees its ancestor's private items, so the move
// widens no visibility in production code, and the `use super::*` below is
// the line the module already had inline.

use super::*;
use crate::drawings::DRAWING_TOOLS;
use crate::drawings::Drawings;

mod launch;

pub(crate) trait ToolRailTestExt {
    fn draw(&mut self, ctx: &egui::Context, drawings: &mut Drawings, manager_open: &mut bool);
    fn launch_pending_for_test(&self) -> (Option<f32>, Option<&str>);
    fn launch_draw_for_test(&mut self, ctx: &egui::Context, size: egui::Vec2);
    fn launch_rendered_for_test(&self) -> (bool, f32, Option<&str>);
}
impl ToolRailTestExt for ToolRail {
    fn draw(&mut self, ctx: &egui::Context, drawings: &mut Drawings, manager_open: &mut bool) {
        use quantick_ui_model::tool_rail::{Effect, Facts};
        self.update(Intent::Observe(Facts {
            count: drawings.items().len(),
            draft: drawings.draft().is_some(),
            draft_len: drawings.draft_len(),
            all_hidden: drawings.all_hidden(),
            all_locked: drawings.all_locked(),
            manager_open: *manager_open,
        }));
        self.with_updates(
            |response| {
                for effect in response.model_effects {
                    if let quantick_ui_model::HostEffect::ToolRail(effects) = effect {
                        for effect in effects {
                            match effect {
                                Effect::SetAllHidden(hidden) => drawings.set_all_hidden(hidden),
                                Effect::SetAllLocked(locked) => drawings.set_all_locked(locked),
                                Effect::SetManagerOpen(open) => *manager_open = open,
                                Effect::UnknownFavorites(_) => {}
                            }
                        }
                    }
                }
            },
            |view, session| {
                crate::surfaces::View::paint_updated(
                    view,
                    session,
                    &mut crate::surfaces::ViewContext { ctx, ui: None },
                    None,
                )
            },
        );
    }

    /// Read the launch request before the real draw is allowed to consume it.
    fn launch_pending_for_test(&self) -> (Option<f32>, Option<&str>) {
        (
            self.model_state.requested_scroll,
            self.model_state.requested_flyout.as_deref(),
        )
    }

    fn launch_draw_for_test(&mut self, ctx: &egui::Context, size: egui::Vec2) {
        rail_frame_with(
            self,
            &mut Drawings::default(),
            ctx,
            egui::Rect::from_min_size(egui::Pos2::ZERO, size),
            Vec::new(),
        );
    }

    fn launch_rendered_for_test(&self) -> (bool, f32, Option<&str>) {
        (
            self.view.band_rect.is_some(),
            self.view.band_offset,
            self.model_state.flyout,
        )
    }
}

#[test]
fn the_rail_never_borrows_the_provenance_amber() {
    // Amber is reserved for data honesty (replay, backfill, inferred
    // data) and is never rail decoration — grep-guarded like the
    // indicators crate's libm rule.
    // Relative to this file, so it climbs back out of `toolrail/tests/`
    // to reach the host it greps. The one path the move had to rewrite.
    let source = include_str!("../mod.rs");
    assert!(
        !source.contains(concat!("theme::", "AM", "BER")),
        "the rail's only accent is ACCENT"
    );
}

/// The egui key path reaches the rail's model, which owns the chord table
/// and the hidden-rail gate (audit M9).
#[test]
fn shortcuts_cannot_arm_a_tool_while_the_rail_is_hidden() {
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    for (visible, expected) in [
        (false, Tool::Pointer),
        (true, Tool::Drawing(DRAWING_TOOLS[0])),
    ] {
        rail.set_visible(visible);
        let shortcut = DRAWING_TOOLS[0].shortcut().expect("the first tool has one");
        let key = crate::drawings::shortcut_key(shortcut).expect("a letter key");
        let press = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(press, |ctx| rail.handle_keys(ctx));
        assert_eq!(rail.tool(), expected, "visible={visible}");
    }
}

fn draw_rail_frame(
    rail: &mut ToolRail,
    ctx: &egui::Context,
    screen: egui::Rect,
    events: Vec<egui::Event>,
) {
    let mut drawings = Drawings::default();
    rail_frame_with(rail, &mut drawings, ctx, screen, events);
}

fn primary_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn dragging_the_real_grip_docks_at_each_screen_edge() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    draw_rail_frame(&mut rail, &ctx, screen, Vec::new());

    for (target, expected) in [
        (egui::pos2(400.0, 10.0), ToolboxDock::Top),
        (egui::pos2(400.0, 590.0), ToolboxDock::Bottom),
        (egui::pos2(10.0, 300.0), ToolboxDock::Left),
    ] {
        let start = rail.view.grip_rect.expect("grip was rendered").center();
        draw_rail_frame(
            &mut rail,
            &ctx,
            screen,
            vec![
                egui::Event::PointerMoved(start),
                primary_button(start, true),
            ],
        );
        draw_rail_frame(
            &mut rail,
            &ctx,
            screen,
            vec![egui::Event::PointerMoved(target)],
        );
        draw_rail_frame(
            &mut rail,
            &ctx,
            screen,
            vec![
                egui::Event::PointerMoved(target),
                primary_button(target, false),
            ],
        );
        assert_eq!(rail.model_state.dock, expected);
        draw_rail_frame(&mut rail, &ctx, screen, Vec::new());
    }
}

#[test]
fn escape_during_a_grip_drag_keeps_the_current_dock() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    draw_rail_frame(&mut rail, &ctx, screen, Vec::new());

    let start = rail.view.grip_rect.expect("grip was rendered").center();
    draw_rail_frame(
        &mut rail,
        &ctx,
        screen,
        vec![
            egui::Event::PointerMoved(start),
            primary_button(start, true),
        ],
    );
    let target = egui::pos2(400.0, 590.0);
    draw_rail_frame(
        &mut rail,
        &ctx,
        screen,
        vec![egui::Event::PointerMoved(target)],
    );
    assert!(rail.drag_active());
    draw_rail_frame(
        &mut rail,
        &ctx,
        screen,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!rail.drag_active());
    draw_rail_frame(
        &mut rail,
        &ctx,
        screen,
        vec![
            egui::Event::PointerMoved(target),
            primary_button(target, false),
        ],
    );
    assert_eq!(
        rail.model_state.dock,
        ToolboxDock::Left,
        "Esc aborted the drag"
    );
}

fn rail_frame_with(
    rail: &mut ToolRail,
    drawings: &mut Drawings,
    ctx: &egui::Context,
    screen: egui::Rect,
    events: Vec<egui::Event>,
) {
    let input = egui::RawInput {
        screen_rect: Some(screen),
        events,
        ..Default::default()
    };
    let mut manager_open = false;
    let _ = ctx.run(input, |ctx| rail.draw(ctx, drawings, &mut manager_open));
}

fn click_at(
    rail: &mut ToolRail,
    drawings: &mut Drawings,
    ctx: &egui::Context,
    screen: egui::Rect,
    position: egui::Pos2,
) {
    rail_frame_with(
        rail,
        drawings,
        ctx,
        screen,
        vec![
            egui::Event::PointerMoved(position),
            primary_button(position, true),
        ],
    );
    rail_frame_with(
        rail,
        drawings,
        ctx,
        screen,
        vec![
            egui::Event::PointerMoved(position),
            primary_button(position, false),
        ],
    );
}

/// The magnet is a state, not a command: it reads off the rail and it
/// starts off, because a magnet nobody asked for moves marks the trader
/// placed deliberately (`docs/ux/drawing-tools-2026-08.md` §D6).
#[test]
fn the_magnet_is_a_rail_toggle_that_starts_off() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    let mut drawings = Drawings::default();
    assert!(!rail.magnet(), "the magnet opens off");

    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    let button = rail
        .view
        .magnet_rect
        .expect("the magnet button rendered")
        .center();
    // A click asks; the window answers through `annotate.magnet.set`, the
    // operation that journals, so a hand and a script leave the same event.
    click_at(&mut rail, &mut drawings, &ctx, screen, button);
    assert!(!rail.magnet(), "the rail asks rather than flipping itself");
    assert_eq!(rail.take_magnet_request(), Some(true));
    assert_eq!(rail.take_magnet_request(), None, "one click, one request");
    rail.set_magnet(true);
    click_at(&mut rail, &mut drawings, &ctx, screen, button);
    assert_eq!(
        rail.take_magnet_request(),
        Some(false),
        "the same button asks to turn it back off"
    );
}

/// The first family slot and its members — every favorites test walks
/// in through a real family flyout, the way the trader does.
fn first_family() -> &'static [DrawingTool] {
    tool_slots()
        .iter()
        .find_map(|slot| match slot {
            RailSlot::Family { members, .. } => Some(members.as_slice()),
            RailSlot::Single(_) => None,
        })
        .expect("the registry folds at least one family")
}

/// Open the family's flyout through the slot's caret zone and give the
/// next frame a chance to record the row and star rects.
fn open_flyout(
    rail: &mut ToolRail,
    drawings: &mut Drawings,
    ctx: &egui::Context,
    screen: egui::Rect,
    members: &[DrawingTool],
) {
    rail_frame_with(rail, drawings, ctx, screen, Vec::new());
    let slot = rail
        .button_rect(Tool::Drawing(members[0]))
        .expect("the family slot rendered");
    click_at(rail, drawings, ctx, screen, slot.max - egui::vec2(3.0, 3.0));
    rail_frame_with(rail, drawings, ctx, screen, Vec::new());
    assert!(
        rail.model_state.flyout.is_some(),
        "the caret click opened the flyout"
    );
}

/// Clicking the tiny star pins the tool: nothing arms, the flyout stays
/// open — the trader is curating the rail, not drawing.
#[test]
fn starring_from_the_flyout_pins_without_arming_or_closing() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    let mut drawings = Drawings::default();
    let members = first_family();
    open_flyout(&mut rail, &mut drawings, &ctx, screen, members);

    let star = rail
        .view
        .flyout_star_rects
        .iter()
        .find_map(|(tool, rect)| (*tool == members[1]).then_some(*rect))
        .expect("every flyout row carries a star");
    click_at(&mut rail, &mut drawings, &ctx, screen, star.center());

    assert!(rail.is_favorite(members[1]), "the star click pinned");
    assert_eq!(rail.tool(), Tool::Pointer, "starring armed nothing");
    assert!(
        rail.model_state.flyout.is_some(),
        "the flyout stayed open to keep curating"
    );
}

/// A click on the row's own icon arms the tool — the star's hit zone
/// must never swallow the natural arming gesture (trader review).
#[test]
fn a_click_on_the_flyout_icon_arms_and_never_stars() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    let mut drawings = Drawings::default();
    let members = first_family();
    open_flyout(&mut rail, &mut drawings, &ctx, screen, members);

    let row = rail
        .flyout_row_rect(members[1])
        .expect("the flyout lists the member");
    let glyph_center = egui::pos2(row.left() + FLYOUT_GLYPH_CENTER_X_PX, row.center().y);
    click_at(&mut rail, &mut drawings, &ctx, screen, glyph_center);

    assert_eq!(
        rail.tool(),
        Tool::Drawing(members[1]),
        "the icon click armed the tool"
    );
    assert!(
        !rail.is_favorite(members[1]),
        "an arming click must never silently star"
    );
}

/// The pinned button arms in one click and never unstars — removal lives
/// only on the flyout star, so using a favorite cannot destroy it.
#[test]
fn the_pinned_button_arms_and_never_unstars() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    let mut drawings = Drawings::default();
    let members = first_family();
    rail.toggle_favorite(members[1]);

    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    let pinned = rail
        .view
        .favorite_rects
        .iter()
        .find_map(|(tool, rect)| (*tool == members[1]).then_some(*rect))
        .expect("the favorites section rendered the pin");
    click_at(&mut rail, &mut drawings, &ctx, screen, pinned.center());

    assert_eq!(rail.tool(), Tool::Drawing(members[1]), "one click armed");
    assert!(
        rail.is_favorite(members[1]),
        "arming a favorite must never unstar it"
    );
}

/// The painted buttons are the scene's controls at every stage, so the
/// stage table `painted_controls` owns in the model is what reaches pixels;
/// only Minimal folds the global eye and lock into More.
#[test]
fn painted_buttons_match_the_scene_controls_at_every_stage() {
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    rail.set_dock(ToolboxDock::Top);
    rail.arm(Tool::Drawing(DRAWING_TOOLS[1]));
    let mut drawings = Drawings::default();
    // A horizontal rail: 400 px is Compact, 250 px Minimal, 900 px Full.
    for (width, globals) in [(400.0, true), (250.0, false), (900.0, true)] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0));
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
        let mut scene: Vec<_> = rail.painted_controls().iter().map(|c| c.id).collect();
        // A family slot paints its shown member; the scene names the family.
        let slot = |tool: &Tool| match tool.drawing_tool().and_then(DrawingTool::family) {
            Some(family) if !scene.contains(&tool.id()) => family.id,
            _ => tool.id(),
        };
        let mut painted: Vec<_> = rail
            .view
            .button_rects
            .iter()
            .flatten()
            .map(|(tool, _)| slot(tool))
            .collect();
        painted.sort_unstable();
        scene.sort_unstable();
        assert_eq!(painted, scene, "{width} px");
        assert!(rail.view.objects_rect.is_some(), "{width} px keeps Objects");
        assert_eq!(rail.view.hide_all_rect.is_some(), globals, "{width} px");
        assert_eq!(rail.view.lock_all_rect.is_some(), globals, "{width} px");
    }
}

#[test]
fn orientation_changes_positions_never_inventory() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let mut inventories: Vec<Vec<&'static str>> = Vec::new();
    for dock in [ToolboxDock::Left, ToolboxDock::Top, ToolboxDock::Bottom] {
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new_rail();
        rail.set_dock(dock);
        let mut drawings = Drawings::default();
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
        let mut tools: Vec<&'static str> = rail
            .view
            .button_rects
            .iter()
            .flatten()
            .map(|(tool, _)| tool.name())
            .collect();
        tools.sort_unstable();
        inventories.push(tools);
    }
    assert!(
        inventories.windows(2).all(|pair| pair[0] == pair[1]),
        "every dock shows the same buttons at the same extent: {inventories:?}"
    );
}

#[test]
fn the_grip_leads_and_objects_trails_in_every_dock() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    for dock in [ToolboxDock::Left, ToolboxDock::Top, ToolboxDock::Bottom] {
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new_rail();
        rail.set_dock(dock);
        let mut drawings = Drawings::default();
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

        let grip = rail.view.grip_rect.expect("grip rendered");
        let objects = rail.view.objects_rect.expect("objects rendered");
        let rail_rect = rail.view.rail_rect.expect("rail rect recorded");
        let along = |rect: egui::Rect| {
            if dock.is_vertical() {
                rect.top()
            } else {
                rect.left()
            }
        };
        for (_, rect) in rail.view.button_rects.iter().flatten() {
            assert!(
                along(grip) <= along(*rect),
                "the grip precedes every button in {dock:?}"
            );
        }
        // The trailing cluster is pinned: Objects sits one margin off
        // the rail's far end.
        let far_gap = if dock.is_vertical() {
            rail_rect.bottom() - objects.bottom()
        } else {
            rail_rect.right() - objects.right()
        };
        assert!(
            (far_gap - TOOLBOX_MARGIN_PX).abs() < 0.6,
            "objects must trail one margin off the rail end in {dock:?}, gap {far_gap}"
        );
    }
}

/// Lay out a central panel with the rail in a given state and report the
/// rect the chart is left with. `visible: false` gives the baseline: what
/// the canvas looks like when no rail is competing for it.
fn central_rect_with_rail(screen: egui::Rect, dock: ToolboxDock, visible: bool) -> egui::Rect {
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    rail.set_visible(visible);
    rail.set_dock(dock);
    let mut central = egui::Rect::NOTHING;
    let input = egui::RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    let mut drawings = Drawings::default();
    let mut manager_open = false;
    let _ = ctx.run(input, |ctx| {
        rail.draw(ctx, &mut drawings, &mut manager_open);
        egui::CentralPanel::default().show(ctx, |ui| {
            central = ui.max_rect();
        });
    });
    central
}

#[test]
fn every_dock_position_reserves_space_outside_the_central_chart() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    // The canvas with no rail at all: the right edge to beat.
    let bare = central_rect_with_rail(screen, ToolboxDock::Left, false);
    for dock in [ToolboxDock::Left, ToolboxDock::Top, ToolboxDock::Bottom] {
        let central = central_rect_with_rail(screen, dock, true);
        match dock {
            ToolboxDock::Left => assert!(central.left() >= TOOLBOX_THICKNESS_PX),
            ToolboxDock::Top => assert!(central.top() >= TOOLBOX_THICKNESS_PX),
            ToolboxDock::Bottom => {
                assert!(central.bottom() <= screen.bottom() - TOOLBOX_THICKNESS_PX);
            }
        }
        // Whichever edge it took, it never took the right one: that
        // border is the price axis and the live column.
        assert!(
            (central.right() - bare.right()).abs() < f32::EPSILON,
            "the rail docked {dock:?} still narrowed the chart from the \
                 right: {} vs {}",
            central.right(),
            bare.right()
        );
    }
}

#[test]
fn global_eye_and_lock_buttons_protect_every_drawing_without_deleting() {
    use crate::drawings::ChartPoint;

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    let mut drawings = Drawings::default();
    // Registry-order-proof: place whatever the first tool needs rather
    // than assuming it is a one-click tool.
    let tool = DRAWING_TOOLS[0];
    for anchor in 0..tool.required_points() {
        drawings.place(tool, ChartPoint::at(anchor as f32, 100.0 + anchor as f64));
    }
    assert_eq!(drawings.items().len(), 1, "the object was placed");
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

    let eye = rail.view.hide_all_rect.expect("hide-all rendered").center();
    click_at(&mut rail, &mut drawings, &ctx, screen, eye);
    assert!(drawings.all_hidden(), "hide-all engages the global layer");
    click_at(&mut rail, &mut drawings, &ctx, screen, eye);
    assert!(!drawings.all_hidden(), "hide-all toggles back off");

    let lock = rail.view.lock_all_rect.expect("lock-all rendered").center();
    click_at(&mut rail, &mut drawings, &ctx, screen, lock);
    assert!(drawings.all_locked(), "lock-all locks every drawing");
    click_at(&mut rail, &mut drawings, &ctx, screen, lock);
    assert!(!drawings.all_locked(), "lock-all toggles back to unlocked");
    assert_eq!(
        drawings.items().len(),
        1,
        "global protections must never delete"
    );
}

/// A screen short enough to push the rail into the band, tall enough to
/// stay clear of Compact.
fn scrolling_screen() -> egui::Rect {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 560.0));
    let slots = tool_slots().len();
    assert_eq!(
        stage_for(560.0, slots, 0),
        RailStage::Scroll,
        "the fixture height must exercise the band"
    );
    screen
}

/// Click a chevron and let the rail settle. The trailing one is drawn
/// past the band, so it lands on the frame the rail requests next.
fn click_chevron(
    rail: &mut ToolRail,
    drawings: &mut Drawings,
    ctx: &egui::Context,
    screen: egui::Rect,
    arrow: egui::Rect,
) {
    click_at(rail, drawings, ctx, screen, arrow.center());
    rail_frame_with(rail, drawings, ctx, screen, Vec::new());
}

/// Ids of the first `count` registry tools, for starring in bulk.
fn tool_ids(count: usize) -> Vec<String> {
    DRAWING_TOOLS
        .iter()
        .take(count)
        .map(|tool| tool.id().to_owned())
        .collect()
}

/// Every tool the band actually showed this frame — allocated *and*
/// inside the viewport, which is what "reachable" means once it clips.
fn tools_on_screen(rail: &ToolRail) -> Vec<DrawingTool> {
    let Some(band) = rail.view.band_rect else {
        return Vec::new();
    };
    rail.view
        .button_rects
        .iter()
        .flatten()
        .filter(|(_, rect)| band.contains_rect(*rect))
        .filter_map(|(tool, _)| tool.drawing_tool())
        .collect()
}

/// The bug this branch closes: starring past the rail's height used to
/// drop the entire tool run for the More menu. Now every tool is still
/// reachable by scrolling, and every pin still exists.
#[test]
fn no_tool_is_unreachable_once_favorites_overflow_the_rail() {
    let screen = scrolling_screen();
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    rail.set_favorites(&tool_ids(6));
    assert_eq!(rail.favorites().len(), 6, "six pins are live");

    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    assert!(
        rail.view.band_rect.is_some(),
        "the rail scrolls instead of collapsing to Compact"
    );
    assert!(
        rail.view.more_rect.is_none(),
        "no tool was swallowed into More"
    );

    let mut seen: Vec<DrawingTool> = tools_on_screen(&rail);
    for _ in 0..40 {
        let Some((arrow, live)) = rail.view.band_trailing_arrow else {
            break;
        };
        if !live {
            break;
        }
        click_chevron(&mut rail, &mut drawings, &ctx, screen, arrow);
        for tool in tools_on_screen(&rail) {
            if !seen.contains(&tool) {
                seen.push(tool);
            }
        }
    }

    for tool in DRAWING_TOOLS {
        let folded = tool_slots().iter().any(|slot| match slot {
            RailSlot::Family { members, .. } => {
                members.contains(&tool) && members.first() != Some(&tool)
            }
            RailSlot::Single(_) => false,
        });
        assert!(
            folded || seen.contains(&tool),
            "{} never became reachable by scrolling",
            tool.id()
        );
    }
}

/// Both ends of the band travel, and each chevron is live only while its
/// own direction has somewhere to go.
#[test]
fn each_chevron_is_live_only_while_its_end_has_travel() {
    let screen = scrolling_screen();
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

    let (_, up_live) = rail
        .view
        .band_leading_arrow
        .expect("leading chevron rendered");
    let (down, down_live) = rail
        .view
        .band_trailing_arrow
        .expect("trailing chevron rendered");
    assert!(!up_live, "nothing above the band at rest");
    assert!(down_live, "the run overflows, so the band can descend");

    click_chevron(&mut rail, &mut drawings, &ctx, screen, down);
    assert!(rail.view.band_offset > 0.0, "the click moved the band");
    let (_, up_live) = rail
        .view
        .band_leading_arrow
        .expect("leading chevron rendered");
    assert!(up_live, "the way back opens as soon as the band moves");

    for _ in 0..40 {
        let Some((arrow, live)) = rail.view.band_trailing_arrow else {
            break;
        };
        if !live {
            break;
        }
        click_chevron(&mut rail, &mut drawings, &ctx, screen, arrow);
    }
    let (_, down_live) = rail
        .view
        .band_trailing_arrow
        .expect("trailing chevron rendered");
    assert!(!down_live, "the chevron dies at the end of travel");
    assert!(
        rail.view.band_leading_arrow.is_some_and(|(_, live)| live),
        "and the way back stays open"
    );
}

/// Arming a tool by shortcut pulls the band to it. Without this the
/// rail can show a scrolled-away run while a tool the trader cannot see
/// is armed — they would not know what the next click draws.
#[test]
fn arming_a_scrolled_away_tool_brings_it_back_into_view() {
    let screen = scrolling_screen();
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

    // The last registry tool is past the band's floor by construction.
    let last = *DRAWING_TOOLS.last().expect("the registry is not empty");
    assert!(
        !tools_on_screen(&rail).contains(&last),
        "the fixture must start with that tool out of view"
    );

    rail.arm(Tool::Drawing(last));
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    assert!(
        tools_on_screen(&rail).contains(&last),
        "arming scrolled the band to the armed tool"
    );

    // And the band stays put afterwards: a reveal is a one-frame event,
    // never a magnet that fights the trader scrolling away.
    let settled = rail.view.band_offset;
    let up = rail.view.band_leading_arrow.expect("leading chevron").0;
    click_chevron(&mut rail, &mut drawings, &ctx, screen, up);
    assert!(
        rail.view.band_offset < settled,
        "the band scrolled away and was not dragged back"
    );
}

/// The wheel scrolls the same band the chevrons do. A trader whose hand
/// is already on the mouse should not have to find a 14 px arrow.
#[test]
fn the_wheel_scrolls_the_band() {
    let screen = scrolling_screen();
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    let over = rail.view.band_rect.expect("the band renders").center();
    assert_eq!(rail.view.band_offset, 0.0, "the band starts at the top");

    for _ in 0..4 {
        rail_frame_with(
            &mut rail,
            &mut drawings,
            &ctx,
            screen,
            vec![
                egui::Event::PointerMoved(over),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -40.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(
        rail.view.band_offset > 0.0,
        "the wheel moved the band, offset={}",
        rail.view.band_offset
    );
}

/// "I cannot get it back to how it was": unstarring down to a rail that
/// fits must leave no scroll residue behind.
#[test]
fn unstarring_back_to_a_rail_that_fits_leaves_no_residue() {
    let screen = scrolling_screen();
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new_rail();
    rail.set_favorites(&tool_ids(6));
    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    let down = rail.view.band_trailing_arrow.expect("trailing chevron").0;
    click_chevron(&mut rail, &mut drawings, &ctx, screen, down);
    assert!(rail.view.band_offset > 0.0, "the band is scrolled");

    for tool in DRAWING_TOOLS.iter().take(6) {
        rail.toggle_favorite(*tool);
    }
    assert!(rail.favorites().is_empty(), "every pin was removed");

    let tall = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    rail_frame_with(&mut rail, &mut drawings, &ctx, tall, Vec::new());
    assert_eq!(rail.view.band_offset, 0.0, "the offset went home");
    assert!(
        rail.view.band_leading_arrow.is_none() && rail.view.band_trailing_arrow.is_none(),
        "a rail that fits shows no navigation chrome"
    );
    assert!(rail.view.band_rect.is_none(), "and no band");
}

/// The band follows the rail's long axis, so a horizontal dock scrolls
/// left/right — the chevrons are not hardcoded to up/down.
#[test]
fn the_band_scrolls_along_the_long_axis_in_every_dock() {
    let ctx = egui::Context::default();
    let mut drawings = Drawings::default();
    for (dock, screen) in [
        (
            ToolboxDock::Left,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 560.0)),
        ),
        (
            ToolboxDock::Top,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(560.0, 900.0)),
        ),
        (
            ToolboxDock::Bottom,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(560.0, 900.0)),
        ),
    ] {
        let mut rail = ToolRail::new_rail();
        rail.set_dock(dock);
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
        let band = rail.view.band_rect.expect("the band renders in every dock");
        let (leading, _) = rail.view.band_leading_arrow.expect("leading chevron");
        let (trailing, _) = rail.view.band_trailing_arrow.expect("trailing chevron");
        // The band is a window onto the tools, not a claim on the rail:
        // it must never eat the far cluster's room. Measured along the
        // long axis only — the trailing cluster's short-axis placement
        // on a horizontal dock is wrong on `main` too, in every stage,
        // and fixing that is not this change's job.
        let objects = rail
            .view
            .objects_rect
            .unwrap_or_else(|| panic!("{dock:?} kept its Objects button"));
        let rail_rect = rail.view.rail_rect.expect("rail rendered");
        assert!(
            rail.view.magnet_rect.is_some() && rail.view.lock_all_rect.is_some(),
            "{dock:?} kept its trailing cluster"
        );
        if dock.is_vertical() {
            assert!(
                objects.top() > trailing.bottom() && objects.bottom() <= rail_rect.bottom(),
                "{dock:?} keeps Objects past the band, inside the rail"
            );
        } else {
            assert!(
                objects.left() > trailing.right() && objects.right() <= rail_rect.right(),
                "{dock:?} keeps Objects past the band, inside the rail"
            );
        }
        if dock.is_vertical() {
            assert!(leading.bottom() <= band.top() + 1.0, "{dock:?} up chevron");
            assert!(
                trailing.top() >= band.bottom() - 1.0,
                "{dock:?} down chevron"
            );
        } else {
            assert!(
                leading.right() <= band.left() + 1.0,
                "{dock:?} left chevron"
            );
            assert!(
                trailing.left() >= band.right() - 1.0,
                "{dock:?} right chevron"
            );
        }
    }
}

#[test]
fn top_rail_has_no_unpainted_gutter() {
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for width in [1000.0, 1280.0] {
            let ctx = egui::Context::default();
            let mut appearance = quantick_appearance::Appearance::default();
            appearance.apply(source).unwrap();
            theme::sync(&ctx, &appearance);
            let mut rail = ToolRail::new_rail();
            let mut drawings = Drawings::default();
            for dock in [
                ToolboxDock::Left,
                ToolboxDock::Top,
                ToolboxDock::Bottom,
                ToolboxDock::Top,
            ] {
                rail.set_dock(dock);
                for _ in 0..3 {
                    let mut remaining = egui::Rect::NOTHING;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 780.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            rail.draw(ctx, &mut drawings, &mut false);
                            remaining = ctx.available_rect();
                        },
                    );
                    if dock == ToolboxDock::Top {
                        assert_eq!(
                            remaining.top(),
                            TOOLBOX_THICKNESS_PX,
                            "the rail reserves only the painted band"
                        );
                        let at = egui::pos2(width / 2.0, remaining.top() - 2.0);
                        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if shape.clip_rect.contains(at) && rect.rect.contains(at) && rect.fill == theme::get(&ctx).chrome())), "reserved rail is painted to its bottom edge");
                        for control in [
                            rail.view.objects_rect,
                            rail.view.magnet_rect,
                            rail.view.hide_all_rect,
                            rail.view.lock_all_rect,
                        ]
                        .into_iter()
                        .flatten()
                        {
                            assert!(
                                control.top() >= 0.0 && control.bottom() <= TOOLBOX_THICKNESS_PX,
                                "trailing control is inside the painted band: {control:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
