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

#[test]
fn toolbox_opens_outside_the_chart_docked_left() {
    let rail = ToolRail::new();
    assert!(rail.visible());
    assert_eq!(rail.tool(), Tool::Pointer);
    assert_eq!(rail.dock, ToolboxDock::Left);
}

#[test]
fn the_rail_never_borrows_the_provenance_amber() {
    // Amber is reserved for data honesty (replay, backfill, inferred
    // data) and is never rail decoration — grep-guarded like the
    // indicators crate's libm rule.
    // Relative to this file, so it climbs back out of `toolrail/tests/`
    // to reach the host it greps. The one path the move had to rewrite.
    let source = include_str!("../../toolrail.rs");
    assert!(
        !source.contains(concat!("theme::", "AM", "BER")),
        "the rail's only accent is ACCENT"
    );
}

#[test]
fn rail_thickness_is_margin_button_margin() {
    assert_eq!(
        TOOLBOX_THICKNESS_PX,
        2.0 * TOOLBOX_MARGIN_PX + TOOLRAIL_ICON.hit
    );
}

#[test]
fn stage_lengths_match_the_spec_for_the_shipped_registry() {
    let slots = tool_slots().len();
    assert_eq!(
        slots, 9,
        "Lines, Channels, Marks, Freehand, Shapes, Fib, Measure, Anchored VWAP and Text"
    );
    assert_eq!(full_length(slots, 0), 633.0);
    assert_eq!(scroll_length(0), 489.0);
    assert_eq!(compact_length(), 381.0);
    assert_eq!(minimal_length(), 191.0);
    // The band sits strictly between Full and Compact. Overlapping
    // either neighbour would make the rail ambiguous at that extent.
    assert!(scroll_length(0) < full_length(slots, 0));
    assert!(scroll_length(0) > compact_length());
}

#[test]
fn hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed() {
    let mut rail = ToolRail::new();
    rail.arm(Tool::Drawing(DRAWING_TOOLS[0]));
    rail.toggle_visible();
    assert!(!rail.visible());
    assert_eq!(rail.tool(), Tool::Pointer);
}

/// The keyboard twin of the invariant above (audit M9): the shortcut
/// path must be as gated as the toggle path, or `H` with the rail hidden
/// arms a tool nothing on screen reports.
#[test]
fn shortcuts_cannot_arm_a_tool_while_the_rail_is_hidden() {
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    rail.toggle_visible();
    let shortcut = DRAWING_TOOLS[0].shortcut().expect("the first tool has one");
    let press = egui::RawInput {
        events: vec![egui::Event::Key {
            key: shortcut.key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = ctx.run(press, |ctx| rail.handle_keys(ctx));
    assert_eq!(
        rail.tool(),
        Tool::Pointer,
        "a hidden rail must swallow its shortcuts"
    );

    // The same press with the rail visible arms the tool — proving the
    // gate above is the visibility, not a broken key path.
    rail.toggle_visible();
    let press = egui::RawInput {
        events: vec![egui::Event::Key {
            key: shortcut.key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = ctx.run(press, |ctx| rail.handle_keys(ctx));
    assert_eq!(rail.tool(), Tool::Drawing(DRAWING_TOOLS[0]));
}

#[test]
fn nearest_picks_each_edge_from_its_midpoint() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    assert_eq!(
        ToolboxDock::nearest(egui::pos2(10.0, 300.0), screen),
        ToolboxDock::Left
    );
    assert_eq!(
        ToolboxDock::nearest(egui::pos2(400.0, 10.0), screen),
        ToolboxDock::Top
    );
    assert_eq!(
        ToolboxDock::nearest(egui::pos2(400.0, 590.0), screen),
        ToolboxDock::Bottom
    );
}

/// The right border is not on offer. A drop anywhere in the right half —
/// including the far edge, where the old rail would have docked — lands
/// on one of the three edges that exist, and never on the price side of
/// the window.
#[test]
fn nothing_dropped_on_the_right_edge_docks_there() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    for point in [
        egui::pos2(799.0, 300.0),
        egui::pos2(790.0, 100.0),
        egui::pos2(760.0, 500.0),
        egui::pos2(799.0, 599.0),
    ] {
        let dock = ToolboxDock::nearest(point, screen);
        assert!(
            matches!(
                dock,
                ToolboxDock::Left | ToolboxDock::Top | ToolboxDock::Bottom
            ),
            "a drop at {point:?} resolved to {dock:?}"
        );
    }
}

#[test]
fn nearest_normalises_so_a_wide_screen_does_not_bias_top_bottom() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 600.0));
    // Raw pixel distances tie at 300 vs 300; normalised 0.156 vs 0.5.
    assert_eq!(
        ToolboxDock::nearest(egui::pos2(300.0, 300.0), screen),
        ToolboxDock::Left
    );
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
    let mut rail = ToolRail::new();
    draw_rail_frame(&mut rail, &ctx, screen, Vec::new());

    for (target, expected) in [
        (egui::pos2(400.0, 10.0), ToolboxDock::Top),
        (egui::pos2(400.0, 590.0), ToolboxDock::Bottom),
        (egui::pos2(10.0, 300.0), ToolboxDock::Left),
    ] {
        let start = rail.grip_rect.expect("grip was rendered").center();
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
        assert_eq!(rail.dock, expected);
        draw_rail_frame(&mut rail, &ctx, screen, Vec::new());
    }
}

#[test]
fn escape_during_a_grip_drag_keeps_the_current_dock() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    draw_rail_frame(&mut rail, &ctx, screen, Vec::new());

    let start = rail.grip_rect.expect("grip was rendered").center();
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
    assert_eq!(rail.dock, ToolboxDock::Left, "Esc aborted the drag");
}

#[test]
fn drawing_tool_carries_the_registered_implementation_without_an_adapter_match() {
    for tool in DRAWING_TOOLS {
        assert_eq!(Tool::Drawing(tool).drawing_tool(), Some(tool));
    }
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

#[test]
fn tool_shortcuts_arm_their_declared_tools() {
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    let send = |rail: &mut ToolRail, key: egui::Key, modifiers: egui::Modifiers| {
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            modifiers,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| rail.handle_keys(ctx));
    };

    send(&mut rail, egui::Key::H, egui::Modifiers::NONE);
    assert_eq!(
        rail.tool().drawing_tool().map(DrawingTool::id),
        Some("horizontal-line")
    );
    send(&mut rail, egui::Key::R, egui::Modifiers::NONE);
    assert_eq!(
        rail.tool().drawing_tool().map(DrawingTool::id),
        Some("rectangle")
    );
    send(&mut rail, egui::Key::C, egui::Modifiers::NONE);
    assert_eq!(
        rail.tool().drawing_tool().map(DrawingTool::id),
        Some("parallel-channel")
    );
    send(&mut rail, egui::Key::F, egui::Modifiers::NONE);
    assert_eq!(
        rail.tool().drawing_tool().map(DrawingTool::id),
        Some("fib-retracement")
    );
    // Shift+F is *flatten*. The Fib extension gave the chord up rather
    // than keep arming a tool and closing a position on one keystroke;
    // it is reached through the Fib family flyout. Its tooltip went on
    // advertising the chord for far longer — see
    // `a_tooltip_names_the_key_the_tool_really_answers_to`.
    send(&mut rail, egui::Key::F, egui::Modifiers::SHIFT);
    assert_eq!(
        rail.tool().drawing_tool().map(DrawingTool::id),
        Some("fib-retracement"),
        "Shift+F must arm nothing new, leaving the previous tool alone"
    );
    send(&mut rail, egui::Key::B, egui::Modifiers::NONE);
    assert_eq!(
        rail.tool().drawing_tool().map(DrawingTool::id),
        Some("arrow-mark-up")
    );
    send(&mut rail, egui::Key::S, egui::Modifiers::NONE);
    assert_eq!(
        rail.tool().drawing_tool().map(DrawingTool::id),
        Some("arrow-mark-down")
    );
    send(&mut rail, egui::Key::P, egui::Modifiers::NONE);
    assert_eq!(
        rail.tool().drawing_tool().map(DrawingTool::id),
        Some("brush")
    );
    send(&mut rail, egui::Key::Num1, egui::Modifiers::NONE);
    assert_eq!(rail.tool(), Tool::Pointer);
    // A held command modifier keeps the letters out of the tool map.
    send(&mut rail, egui::Key::R, egui::Modifiers::COMMAND);
    assert_eq!(rail.tool(), Tool::Pointer);
}

/// A tooltip that names a key must name the key the tool really answers
/// to, and a tool with no key must name none.
///
/// The Fib extension advertised `Shift+F` in its tooltip for a long time
/// after giving that chord up — and `Shift+F` is *flatten*. The tooltip
/// was telling the trader that the way to reach a drawing tool is the one
/// keystroke that closes their position and cancels their working orders.
/// Nothing failed; the words simply went stale. This is the test that
/// stops them.
///
/// The convention the rail already follows: a trailing `(<label>)`, with
/// `<label>` exactly what the menus print for that tool.
#[test]
fn a_tooltip_names_the_key_the_tool_really_answers_to() {
    for tool in DRAWING_TOOLS.into_iter().map(Tool::Drawing) {
        let advertised = tool
            .hover_text()
            .rsplit_once('(')
            .and_then(|(_, tail)| tail.strip_suffix(')'))
            .map(str::to_owned);
        assert_eq!(
            advertised,
            tool.shortcut_label(),
            "{} advertises one key and answers to another",
            tool.name()
        );
    }
}

#[test]
fn arming_a_family_member_becomes_the_slot_memory() {
    let mut rail = ToolRail::new();
    let extension = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "fib-extension")
        .expect("registered");
    rail.arm(Tool::Drawing(extension));
    let family = extension.family().expect("fib family");
    assert_eq!(
        rail.family_member(family, &[extension]),
        Some(extension),
        "the slot remembers the last-armed member"
    );
}

#[test]
fn the_full_vertical_rail_folds_the_fib_family_into_one_slot() {
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    let mut drawings = Drawings::default();
    // Tall enough for the Full stage (633 px since the anchored VWAP).
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

    let retracement = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "fib-retracement")
        .expect("registered");
    let extension = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "fib-extension")
        .expect("registered");
    assert!(
        rail.button_rect(Tool::Drawing(retracement)).is_some(),
        "the family slot records its shown member"
    );
    assert!(
        rail.button_rect(Tool::Drawing(extension)).is_none(),
        "the second fib entry must not hold its own slot"
    );
}

/// The magnet is a state, not a command: it reads off the rail and it
/// starts off, because a magnet nobody asked for moves marks the trader
/// placed deliberately (`docs/ux/drawing-tools-2026-08.md` §D6).
#[test]
fn the_magnet_is_a_rail_toggle_that_starts_off() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    let mut drawings = Drawings::default();
    assert!(!rail.magnet(), "the magnet opens off");

    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    let button = rail
        .magnet_rect
        .expect("the magnet button rendered")
        .center();
    click_at(&mut rail, &mut drawings, &ctx, screen, button);
    assert!(rail.magnet());
    click_at(&mut rail, &mut drawings, &ctx, screen, button);
    assert!(!rail.magnet(), "the same button turns it back off");
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
    assert!(rail.flyout.is_some(), "the caret click opened the flyout");
}

/// Clicking the tiny star pins the tool: nothing arms, the flyout stays
/// open — the trader is curating the rail, not drawing.
#[test]
fn starring_from_the_flyout_pins_without_arming_or_closing() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    let mut drawings = Drawings::default();
    let members = first_family();
    open_flyout(&mut rail, &mut drawings, &ctx, screen, members);

    let star = rail
        .flyout_star_rects
        .iter()
        .find_map(|(tool, rect)| (*tool == members[1]).then_some(*rect))
        .expect("every flyout row carries a star");
    click_at(&mut rail, &mut drawings, &ctx, screen, star.center());

    assert!(rail.is_favorite(members[1]), "the star click pinned");
    assert_eq!(rail.tool(), Tool::Pointer, "starring armed nothing");
    assert!(
        rail.flyout.is_some(),
        "the flyout stayed open to keep curating"
    );
}

/// A click on the row's own icon arms the tool — the star's hit zone
/// must never swallow the natural arming gesture (trader review).
#[test]
fn a_click_on_the_flyout_icon_arms_and_never_stars() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
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
    let mut rail = ToolRail::new();
    let mut drawings = Drawings::default();
    let members = first_family();
    rail.toggle_favorite(members[1]);

    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    let pinned = rail
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

/// A saved id this build does not offer is reported, not swallowed.
///
/// It used to be harmless: the pruned list only reached the disk on an
/// explicit save. Now the trader's next star click writes it back, so the
/// drop is permanent — and dropping something a file said without a word
/// is the silent patching `CLAUDE.md` rules out.
#[test]
fn a_starred_id_this_build_does_not_offer_is_reported() {
    let mut rail = ToolRail::default();

    let unknown = rail.set_favorites(&[
        "measure".to_owned(),
        "volume-profile-from-tomorrow".to_owned(),
    ]);

    assert_eq!(
        unknown,
        vec!["volume-profile-from-tomorrow".to_owned()],
        "the caller is told exactly what it lost"
    );
    assert_eq!(
        rail.favorites().len(),
        1,
        "and the rail keeps the tools it does have"
    );
}

/// Star order is the order the trader starred in: unstarring removes in
/// place, re-starring appends at the end — pinned muscle memory holds.
#[test]
fn star_order_is_stable_under_unstar_and_restar() {
    let mut rail = ToolRail::new();
    let tools: Vec<DrawingTool> = DRAWING_TOOLS.into_iter().take(3).collect();
    for tool in &tools {
        rail.toggle_favorite(*tool);
    }
    rail.toggle_favorite(tools[1]);
    assert_eq!(rail.favorites(), &[tools[0], tools[2]]);
    rail.toggle_favorite(tools[1]);
    assert_eq!(rail.favorites(), &[tools[0], tools[2], tools[1]]);
}

/// The saved-ids path: order kept, unknown ids dropped, duplicates kept
/// once — a stale or hand-edited file cannot corrupt the rail.
#[test]
fn set_favorites_round_trips_ids_and_survives_junk() {
    let mut rail = ToolRail::new();
    let tools: Vec<DrawingTool> = DRAWING_TOOLS.into_iter().take(2).collect();
    let ids = vec![
        tools[1].id().to_owned(),
        "no-such-tool".to_owned(),
        tools[0].id().to_owned(),
        tools[1].id().to_owned(),
    ];
    rail.set_favorites(&ids);
    assert_eq!(rail.favorites(), &[tools[1], tools[0]]);

    let saved: Vec<String> = rail
        .favorites()
        .iter()
        .map(|tool| tool.id().to_owned())
        .collect();
    let mut restored = ToolRail::new();
    restored.set_favorites(&saved);
    assert_eq!(
        restored.favorites(),
        rail.favorites(),
        "round-trip is exact"
    );
}

/// The favorites section costs rail length only when it exists, and the
/// stage math knows about it. Overflowing pins hand the rail to the
/// scrolling band — never to Compact, which is what used to make a
/// handful of stars swallow the whole toolbar.
#[test]
fn favorites_lengthen_the_full_stage_only_when_present() {
    let slots = tool_slots().len();
    assert!(full_length(slots, 2) > full_length(slots, 0));
    let extent = full_length(slots, 0);
    assert_eq!(stage_for(extent, slots, 0), RailStage::Full);
    assert_eq!(
        stage_for(extent, slots, 2),
        RailStage::Scroll,
        "two pins overflow the bare-full extent into the band, not into Compact"
    );
}

/// The scripted-validation hook (`QUANTICK_DRAWING_MAGNET`) sets the same
/// flag the button does; nothing may fork the two.
#[test]
fn the_hook_setter_moves_the_same_flag_the_button_does() {
    let mut rail = ToolRail::new();
    rail.set_magnet(true);
    assert!(rail.magnet());
    rail.set_magnet(false);
    assert!(!rail.magnet());
}

#[test]
fn stages_are_pure_functions_of_extent() {
    let slots = tool_slots().len();
    for extent in [100.0_f32, 200.0, 380.9, 381.0, 632.9, 633.0, 1000.0] {
        let first = stage_for(extent, slots, 0);
        let second = stage_for(extent, slots, 0);
        assert_eq!(first, second);
    }
    // The full stage grows one slot per registry addition; the anchored
    // VWAP took it from 597 to 633. Below it the band takes over, and
    // only below the band's own floor does the rail fall to Compact.
    let band_floor = scroll_length(0);
    assert_eq!(stage_for(633.0, slots, 0), RailStage::Full);
    assert_eq!(stage_for(632.9, slots, 0), RailStage::Scroll);
    assert_eq!(stage_for(band_floor, slots, 0), RailStage::Scroll);
    assert_eq!(stage_for(band_floor - 0.1, slots, 0), RailStage::Compact);
    assert_eq!(stage_for(381.0, slots, 0), RailStage::Compact);
    assert_eq!(stage_for(380.9, slots, 0), RailStage::Minimal);
}

#[test]
fn the_compact_rail_keeps_pointer_crosshair_armed_tool_and_objects() {
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    rail.set_dock(ToolboxDock::Top);
    rail.arm(Tool::Drawing(DRAWING_TOOLS[1]));
    let mut drawings = Drawings::default();

    // 400 px wide: Compact for a horizontal rail.
    let compact = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0));
    rail_frame_with(&mut rail, &mut drawings, &ctx, compact, Vec::new());
    assert!(rail.button_rect(Tool::Pointer).is_some());
    assert!(rail.button_rect(Tool::Crosshair).is_some());
    assert!(rail.button_rect(Tool::Drawing(DRAWING_TOOLS[1])).is_some());
    assert!(
        rail.button_rect(Tool::Drawing(DRAWING_TOOLS[0])).is_none(),
        "unarmed tools give up their slots at Compact"
    );
    assert!(rail.more_rect.is_some());
    assert!(rail.objects_rect.is_some());
    assert!(rail.hide_all_rect.is_some(), "trailing cluster survives");

    // 250 px: Minimal — Crosshair and the globals fold into More.
    let minimal = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(250.0, 600.0));
    rail_frame_with(&mut rail, &mut drawings, &ctx, minimal, Vec::new());
    assert!(rail.button_rect(Tool::Pointer).is_some());
    assert!(rail.button_rect(Tool::Crosshair).is_none());
    assert!(rail.button_rect(Tool::Drawing(DRAWING_TOOLS[1])).is_some());
    assert!(rail.hide_all_rect.is_none());
    assert!(rail.lock_all_rect.is_none());
    assert!(rail.objects_rect.is_some());

    // Wide again: every slot returns.
    let wide = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    rail_frame_with(&mut rail, &mut drawings, &ctx, wide, Vec::new());
    for slot in tool_slots() {
        let shown = match slot {
            RailSlot::Single(tool) => *tool,
            RailSlot::Family { family, members } => {
                rail.family_member(*family, members).unwrap_or(members[0])
            }
        };
        assert!(
            rail.button_rect(Tool::Drawing(shown)).is_some(),
            "{} lost its slot on a wide rail",
            shown.id()
        );
    }
}

#[test]
fn the_more_flyout_lists_exactly_what_each_stage_swallowed() {
    let mut rail = ToolRail::new();
    rail.arm(Tool::Drawing(DRAWING_TOOLS[1]));

    let compact = rail.swallowed_tools(RailStage::Compact);
    assert!(!compact.contains(&Tool::Crosshair));
    assert!(!compact.contains(&Tool::Drawing(DRAWING_TOOLS[1])));
    for tool in DRAWING_TOOLS {
        if tool != DRAWING_TOOLS[1] {
            assert!(compact.contains(&Tool::Drawing(tool)));
        }
    }

    let minimal = rail.swallowed_tools(RailStage::Minimal);
    assert!(minimal.contains(&Tool::Crosshair));
    assert!(!minimal.contains(&Tool::Drawing(DRAWING_TOOLS[1])));
}

#[test]
fn orientation_changes_positions_never_inventory() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let mut inventories: Vec<Vec<&'static str>> = Vec::new();
    for dock in [ToolboxDock::Left, ToolboxDock::Top, ToolboxDock::Bottom] {
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        rail.set_dock(dock);
        let mut drawings = Drawings::default();
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
        let mut tools: Vec<&'static str> = rail
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
        let mut rail = ToolRail::new();
        rail.set_dock(dock);
        let mut drawings = Drawings::default();
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

        let grip = rail.grip_rect.expect("grip rendered");
        let objects = rail.objects_rect.expect("objects rendered");
        let rail_rect = rail.rail_rect.expect("rail rect recorded");
        let along = |rect: egui::Rect| {
            if dock.is_vertical() {
                rect.top()
            } else {
                rect.left()
            }
        };
        for (_, rect) in rail.button_rects.iter().flatten() {
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
    let mut rail = ToolRail {
        tool: Tool::Pointer,
        visible,
        dock,
        ..ToolRail::default()
    };
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
    let mut rail = ToolRail::new();
    let mut drawings = Drawings::default();
    // Registry-order-proof: place whatever the first tool needs rather
    // than assuming it is a one-click tool.
    let tool = DRAWING_TOOLS[0];
    for anchor in 0..tool.required_points() {
        drawings.place(tool, ChartPoint::at(anchor as f32, 100.0 + anchor as f64));
    }
    assert_eq!(drawings.items().len(), 1, "the object was placed");
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

    let eye = rail.hide_all_rect.expect("hide-all rendered").center();
    click_at(&mut rail, &mut drawings, &ctx, screen, eye);
    assert!(drawings.all_hidden(), "hide-all engages the global layer");
    click_at(&mut rail, &mut drawings, &ctx, screen, eye);
    assert!(!drawings.all_hidden(), "hide-all toggles back off");

    let lock = rail.lock_all_rect.expect("lock-all rendered").center();
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
    let Some(band) = rail.band_rect else {
        return Vec::new();
    };
    rail.button_rects
        .iter()
        .flatten()
        .filter(|(_, rect)| band.contains_rect(*rect))
        .filter_map(|(tool, _)| tool.drawing_tool())
        .collect()
}

/// The offset can never leave the content: no blank band at either end,
/// which is the state a trader reads as "the toolbar is lost".
#[test]
fn the_band_offset_never_leaves_the_content() {
    let viewport = run_length(BAND_MIN_VISIBLE_ITEMS);
    assert_eq!(
        band_max_offset(viewport, BAND_MIN_VISIBLE_ITEMS),
        0.0,
        "content that fits cannot scroll"
    );
    assert_eq!(
        band_max_offset(viewport, 2),
        0.0,
        "less content than viewport still cannot scroll"
    );
    let max = band_max_offset(viewport, BAND_MIN_VISIBLE_ITEMS + 3);
    assert!(max > 0.0, "surplus content scrolls");
    assert_eq!(
        max,
        run_length(BAND_MIN_VISIBLE_ITEMS + 3) - viewport,
        "the last button lands flush with the band's far edge"
    );
}

/// The band never shows half a button. The viewport is whole slots, so
/// the travel divides into whole slots too and every position a chevron
/// can reach lands on a button boundary — no star badge floating beside
/// a sliced icon, which is what the first capture of the spill state
/// showed.
#[test]
fn the_band_never_shows_a_sliced_button() {
    let slot = TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX;
    for extent in [489.0_f32, 500.0, 517.3, 560.0, 599.9, 632.9] {
        for pins in 0..8 {
            let anchored = anchored_favorites(extent, pins);
            let viewport = band_viewport(extent, anchored);
            let items = band_visible_items(viewport);
            assert_eq!(
                viewport,
                run_length(items),
                "viewport is whole buttons at {extent} px with {pins} pins"
            );
            let max = band_max_offset(viewport, items + 5);
            let steps = max / slot;
            assert!(
                (steps - steps.round()).abs() < 1e-3,
                "travel divides into whole buttons at {extent} px with {pins} pins"
            );
        }
    }
}

/// A chevron click leaves one button of overlap, so the trader keeps a
/// landmark across the jump instead of a fresh screen of icons.
#[test]
fn a_chevron_step_keeps_one_button_of_overlap() {
    let slot = TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX;
    let viewport = run_length(5);
    assert_eq!(band_visible_items(viewport), 5);
    assert_eq!(band_scroll_step(viewport), 4.0 * slot);
    // Even a band down to its last button must still move on a click.
    assert!(band_scroll_step(TOOLRAIL_ICON.hit) >= slot);
    assert!(band_scroll_step(0.0) >= slot);
}

/// Pins are anchored while the band keeps its floor, and spill into the
/// band after that — a star can cost a pin its anchor, never its
/// existence.
#[test]
fn favorites_spill_into_the_band_instead_of_evicting_the_toolbar() {
    let extent = scroll_length(0);
    assert_eq!(anchored_favorites(extent, 0), 0);
    assert_eq!(
        anchored_favorites(extent, 4),
        0,
        "the band's floor outranks the anchor"
    );
    let roomy = scroll_length(3);
    assert_eq!(anchored_favorites(roomy, 3), 3, "all three fit anchored");
    assert_eq!(
        anchored_favorites(roomy, 6),
        3,
        "the surplus spills rather than shrinking the band past its floor"
    );
}

/// The bug this branch closes: starring past the rail's height used to
/// drop the entire tool run for the More menu. Now every tool is still
/// reachable by scrolling, and every pin still exists.
#[test]
fn no_tool_is_unreachable_once_favorites_overflow_the_rail() {
    let screen = scrolling_screen();
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    rail.set_favorites(&tool_ids(6));
    assert_eq!(rail.favorites().len(), 6, "six pins are live");

    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    assert!(
        rail.band_rect.is_some(),
        "the rail scrolls instead of collapsing to Compact"
    );
    assert!(rail.more_rect.is_none(), "no tool was swallowed into More");

    let mut seen: Vec<DrawingTool> = tools_on_screen(&rail);
    for _ in 0..40 {
        let Some((arrow, live)) = rail.band_trailing_arrow else {
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
    let mut rail = ToolRail::new();
    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

    let (_, up_live) = rail.band_leading_arrow.expect("leading chevron rendered");
    let (down, down_live) = rail.band_trailing_arrow.expect("trailing chevron rendered");
    assert!(!up_live, "nothing above the band at rest");
    assert!(down_live, "the run overflows, so the band can descend");

    click_chevron(&mut rail, &mut drawings, &ctx, screen, down);
    assert!(rail.band_offset > 0.0, "the click moved the band");
    let (_, up_live) = rail.band_leading_arrow.expect("leading chevron rendered");
    assert!(up_live, "the way back opens as soon as the band moves");

    for _ in 0..40 {
        let Some((arrow, live)) = rail.band_trailing_arrow else {
            break;
        };
        if !live {
            break;
        }
        click_chevron(&mut rail, &mut drawings, &ctx, screen, arrow);
    }
    let (_, down_live) = rail.band_trailing_arrow.expect("trailing chevron rendered");
    assert!(!down_live, "the chevron dies at the end of travel");
    assert!(
        rail.band_leading_arrow.is_some_and(|(_, live)| live),
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
    let mut rail = ToolRail::new();
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
    let settled = rail.band_offset;
    let up = rail.band_leading_arrow.expect("leading chevron").0;
    click_chevron(&mut rail, &mut drawings, &ctx, screen, up);
    assert!(
        rail.band_offset < settled,
        "the band scrolled away and was not dragged back"
    );
}

/// The wheel scrolls the same band the chevrons do. A trader whose hand
/// is already on the mouse should not have to find a 14 px arrow.
#[test]
fn the_wheel_scrolls_the_band() {
    let screen = scrolling_screen();
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    let over = rail.band_rect.expect("the band renders").center();
    assert_eq!(rail.band_offset, 0.0, "the band starts at the top");

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
        rail.band_offset > 0.0,
        "the wheel moved the band, offset={}",
        rail.band_offset
    );
}

/// "I cannot get it back to how it was": unstarring down to a rail that
/// fits must leave no scroll residue behind.
#[test]
fn unstarring_back_to_a_rail_that_fits_leaves_no_residue() {
    let screen = scrolling_screen();
    let ctx = egui::Context::default();
    let mut rail = ToolRail::new();
    rail.set_favorites(&tool_ids(6));
    let mut drawings = Drawings::default();
    rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
    let down = rail.band_trailing_arrow.expect("trailing chevron").0;
    click_chevron(&mut rail, &mut drawings, &ctx, screen, down);
    assert!(rail.band_offset > 0.0, "the band is scrolled");

    for tool in DRAWING_TOOLS.iter().take(6) {
        rail.toggle_favorite(*tool);
    }
    assert!(rail.favorites().is_empty(), "every pin was removed");

    let tall = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    rail_frame_with(&mut rail, &mut drawings, &ctx, tall, Vec::new());
    assert_eq!(rail.band_offset, 0.0, "the offset went home");
    assert!(
        rail.band_leading_arrow.is_none() && rail.band_trailing_arrow.is_none(),
        "a rail that fits shows no navigation chrome"
    );
    assert!(rail.band_rect.is_none(), "and no band");
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
        let mut rail = ToolRail::new();
        rail.set_dock(dock);
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
        let band = rail.band_rect.expect("the band renders in every dock");
        let (leading, _) = rail.band_leading_arrow.expect("leading chevron");
        let (trailing, _) = rail.band_trailing_arrow.expect("trailing chevron");
        // The band is a window onto the tools, not a claim on the rail:
        // it must never eat the far cluster's room. Measured along the
        // long axis only — the trailing cluster's short-axis placement
        // on a horizontal dock is wrong on `main` too, in every stage,
        // and fixing that is not this change's job.
        let objects = rail
            .objects_rect
            .unwrap_or_else(|| panic!("{dock:?} kept its Objects button"));
        let rail_rect = rail.rail_rect.expect("rail rendered");
        assert!(
            rail.magnet_rect.is_some() && rail.lock_all_rect.is_some(),
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
