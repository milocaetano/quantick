use super::*;
use quantick_drawings::bar_placement::POPOVER_GAP_PX;

const PLAIN: Capabilities = Capabilities {
    stroke_width: true,
    authored: false,
    glyph_size: false,
    settings: true,
    magnet: true,
    position: false,
    draw_again: false,
};

#[test]
fn the_bar_never_borrows_the_provenance_amber() {
    // Amber promises "this data is not live". A drawing painted amber by
    // taste would spend that promise. Grep-guarded like the rail's own
    // rule and the indicators crate's libm rule.
    let source = include_str!("../context_bar.rs");
    assert!(
        !source.contains(concat!("theme::", "AM", "BER")),
        "the context bar's palette never includes the provenance amber"
    );
}

#[test]
fn the_bar_has_no_overflow_menu() {
    // Its overflow is the gear. A "more" button would mean the bar had
    // stopped being a bar, and the ceiling exists to make that visible
    // in review rather than three PRs later.
    let source = include_str!("../context_bar.rs");
    assert!(
        !source.contains(concat!("DOTS_", "THREE")),
        "the gear is the overflow; a more-menu is the failure mode"
    );
    // Every bar `capabilities` can ask for: width and glyph size are one
    // or the other, everything else may come or go. The fullest is an
    // assistant's pointer-anchored object, author chip and all.
    let functional = (0..128_u8)
        .map(|bits| {
            let caps = Capabilities {
                stroke_width: bits & 1 != 0,
                // A position is drawn with strokes: it has no type size.
                glyph_size: bits & 1 == 0 && bits & 64 == 0,
                authored: bits & 2 != 0,
                settings: bits & 4 != 0,
                magnet: bits & 16 != 0,
                draw_again: bits & 32 != 0,
                position: bits & 64 != 0,
            };
            slots(caps)
                .into_iter()
                .filter(|slot| !matches!(slot, Slot::Grip | Slot::Separator))
                .count()
        })
        .max()
        .unwrap_or_default();
    // Ten: the object's own magnet joined Lock, both switches a trader
    // flips on the object under their eye mid-session, on a bar that
    // already carried an assistant's author chip. Eleven: the pencil's
    // own draw-again, so a trader sketching stroke after stroke never
    // goes back to the rail between them. The ceiling is the fullest
    // bar today; a new slot has to argue past it here.
    const CEILING: usize = 11;
    assert!(
        functional <= CEILING,
        "the bar holds at most {CEILING} functional slots; found {functional}"
    );
    assert_eq!(functional, CEILING, "the ceiling is the fullest real bar");
}

#[test]
fn only_a_stroke_the_trader_keeps_drawing_offers_the_pencil_first() {
    assert!(!slots(PLAIN).contains(&Slot::DrawAgain));
    let cells = slots(Capabilities {
        draw_again: true,
        ..PLAIN
    });
    assert_eq!(&cells[..3], &[Slot::Grip, Slot::DrawAgain, Slot::Separator]);
}

#[test]
fn duplicate_names_the_two_key_copy_paste_workflow() {
    assert!(DUPLICATE_HOVER_TEXT.contains("Ctrl+C, Ctrl+V"));
}

#[test]
fn an_unsupported_property_is_absent_not_disabled() {
    let glyph_tool = slots(Capabilities {
        stroke_width: false,
        authored: false,
        glyph_size: true,
        settings: true,
        magnet: true,
        position: false,
        draw_again: false,
    });
    assert!(!glyph_tool.contains(&Slot::Width));
    assert!(glyph_tool.contains(&Slot::GlyphSize));

    let stroke_tool = slots(PLAIN);
    assert!(stroke_tool.contains(&Slot::Width));
    assert!(!stroke_tool.contains(&Slot::GlyphSize));
}

#[test]
fn a_pinned_inspector_takes_the_gear_off_the_bar() {
    // The destination is already on screen; a button that leads where
    // the eye already is would be the dead slot the contract forbids.
    let pinned = slots(Capabilities {
        settings: false,
        ..PLAIN
    });
    assert!(!pinned.contains(&Slot::Settings));
    assert!(
        pinned.contains(&Slot::Delete),
        "everything else survives the gear leaving"
    );
}

#[test]
fn the_trash_is_isolated_by_a_hairline() {
    let cells = slots(PLAIN);
    let trash = cells.iter().position(|slot| *slot == Slot::Delete).unwrap();
    assert_eq!(
        cells[trash - 1],
        Slot::Separator,
        "a hand that misses the neighbouring button must land on nothing"
    );
}

#[test]
fn the_bar_is_one_row_of_touch_targets() {
    // Whatever the object's capabilities, the bar stays one row tall and
    // every slot keeps the rail's 32 px target: it is a strip, and a
    // strip that grew a second row would be a panel again.
    for caps in [
        PLAIN,
        Capabilities {
            stroke_width: false,
            authored: false,
            glyph_size: true,
            settings: false,
            magnet: true,
            position: false,
            draw_again: true,
        },
    ] {
        let cells = slots(caps);
        assert_eq!(bar_size(&cells).y, BAR_HEIGHT_PX);
        for cell in cells {
            if !matches!(cell, Slot::Grip | Slot::Separator) {
                assert_eq!(
                    slot_width(cell),
                    TOOLRAIL_ICON.hit,
                    "{cell:?} lost its target"
                );
            }
        }
    }
}

#[test]
fn sharing_sits_beside_hide_and_names_both_states() {
    let cells = slots(PLAIN);
    let hide = cells.iter().position(|slot| *slot == Slot::Hide).unwrap();
    assert_eq!(cells[hide + 1], Slot::Sharing);
    assert_eq!(
        SHARING_HOVER,
        ["Show on all timeframes", "Show only on this chart"]
    );
}

#[test]
fn the_magnet_sits_beside_lock_and_only_where_it_snaps() {
    let cells = slots(PLAIN);
    let lock = cells.iter().position(|slot| *slot == Slot::Lock).unwrap();
    assert_eq!(cells[lock + 1], Slot::Magnet);
    assert_eq!(magnet_control(false).1, "Snap this drawing to OHLC");
    assert_eq!(magnet_control(true).1, "Stop snapping this drawing to OHLC");
    let own_rule = slots(Capabilities {
        magnet: false,
        ..PLAIN
    });
    assert!(
        !own_rule.contains(&Slot::Magnet),
        "a tool that snaps by its own rule has no dead magnet"
    );
}

#[test]
fn every_offered_width_is_inside_the_models_range() {
    for step in WIDTH_STEPS_PX {
        assert!(
            (super::super::MIN_DRAWING_WIDTH_PX..=super::super::MAX_DRAWING_WIDTH_PX)
                .contains(&step),
            "{step} px is outside the drawing model's own range"
        );
    }
}

fn chart() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0))
}

/// The bug the user hit: a volume profile spans the price axis, so there
/// is no room above it and none below, and the bar used to hug the top
/// edge — which for a full-height object *is* the object. The bar sat on
/// the figure it edits.
///
/// It goes beside it now, on whichever side has room.
#[test]
fn a_full_height_object_gets_the_bar_beside_it_not_on_it() {
    let chart = chart();
    let size = egui::vec2(220.0, 32.0);
    // A profile down the left of the pane, floor to ceiling.
    let bbox = egui::Rect::from_min_max(
        egui::pos2(chart.left() + 40.0, chart.top()),
        egui::pos2(chart.left() + 240.0, chart.bottom()),
    );

    let position = place(chart, chart.right(), bbox, size);
    let rect = egui::Rect::from_min_size(position, size);
    assert!(
        !rect.intersect(bbox).is_positive(),
        "the bar must not sit on the object it edits: {rect:?} vs {bbox:?}"
    );
    assert!(
        chart.contains_rect(rect),
        "and it stays inside the pane: {rect:?}"
    );

    // Mirrored: an object hugging the right sends the bar to its left.
    let right_side = egui::Rect::from_min_max(
        egui::pos2(chart.right() - 240.0, chart.top()),
        egui::pos2(chart.right(), chart.bottom()),
    );
    let rect = egui::Rect::from_min_size(place(chart, chart.right(), right_side, size), size);
    assert!(
        !rect.intersect(right_side).is_positive(),
        "and beside it on the other side when that is where the room is: {rect:?}"
    );
    assert!(chart.contains_rect(rect), "{rect:?}");
}

/// An object that leaves room above still gets the bar above it: the
/// sideways placement is the fallback, not the rule.
#[test]
fn an_object_with_room_above_still_gets_the_bar_above_it() {
    let chart = chart();
    let size = egui::vec2(220.0, 32.0);
    let bbox = egui::Rect::from_min_max(
        egui::pos2(chart.left() + 300.0, chart.top() + 200.0),
        egui::pos2(chart.left() + 400.0, chart.top() + 260.0),
    );
    let position = place(chart, chart.right(), bbox, size);
    assert_eq!(position.x, bbox.left(), "left-aligned, as before");
    assert_eq!(
        position.y,
        bbox.top() - OBJECT_GAP_PX - size.y,
        "and above it, as before"
    );
}

#[test]
fn the_bar_opens_above_the_object_aligned_to_its_left_edge() {
    let size = bar_size(&slots(PLAIN));
    let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 200.0), egui::pos2(400.0, 260.0));
    let position = place(chart(), chart().right(), bbox, size);
    assert_eq!(position.x, bbox.left(), "left-aligned, never centred");
    assert_eq!(position.y, bbox.top() - OBJECT_GAP_PX - size.y);
}

#[test]
fn an_object_against_the_top_flips_the_bar_below_it() {
    let size = bar_size(&slots(PLAIN));
    let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 2.0), egui::pos2(400.0, 60.0));
    let position = place(chart(), chart().right(), bbox, size);
    assert_eq!(position.y, bbox.bottom() + OBJECT_GAP_PX);
}

/// An object taller than the pane used to send the bar to the top edge,
/// which for a full-height object *is* the object. It goes beside it now;
/// the top edge is reserved for the one object that leaves no side either.
#[test]
fn an_object_taller_than_the_pane_gets_the_bar_beside_it() {
    let size = bar_size(&slots(PLAIN));
    let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 0.0), egui::pos2(320.0, 600.0));
    let rect = egui::Rect::from_min_size(place(chart(), chart().right(), bbox, size), size);
    assert!(
        !rect.intersect(bbox).is_positive(),
        "beside the object, not on it: {rect:?}"
    );
    assert!(chart().contains_rect(rect), "{rect:?}");

    // Wall to wall: no side to go to, and the top edge is the honest
    // least-bad answer rather than a pretence.
    let wall = egui::Rect::from_min_max(chart().left_top(), chart().right_bottom());
    let position = place(chart(), chart().right(), wall, size);
    assert_eq!(position.y, chart().top() + OBJECT_GAP_PX);
}

#[test]
fn the_bar_never_covers_the_live_lane() {
    // The trader's first veto: if clicking a drawing costs sight of the
    // price forming, they stop clicking drawings.
    let size = bar_size(&slots(PLAIN));
    let lane_divider = 800.0;
    let bbox = egui::Rect::from_min_max(egui::pos2(780.0, 200.0), egui::pos2(860.0, 260.0));
    let position = place(chart(), lane_divider, bbox, size);
    assert!(
        position.x + size.x <= lane_divider,
        "the bar stops at the live lane: {} + {} > {lane_divider}",
        position.x,
        size.x
    );
}

#[test]
fn the_bar_stays_inside_the_pane_on_every_edge() {
    let size = bar_size(&slots(PLAIN));
    for bbox in [
        egui::Rect::from_min_max(egui::pos2(-50.0, 300.0), egui::pos2(10.0, 340.0)),
        egui::Rect::from_min_max(egui::pos2(980.0, 300.0), egui::pos2(1200.0, 340.0)),
        egui::Rect::from_min_max(egui::pos2(400.0, 560.0), egui::pos2(500.0, 640.0)),
    ] {
        let position = place(chart(), chart().right(), bbox, size);
        let rect = egui::Rect::from_min_size(position, size);
        assert!(
            chart().contains_rect(rect),
            "bar escaped the pane for {bbox:?}: {rect:?}"
        );
    }
}

#[test]
fn suppression_closes_any_open_popover() {
    let mut bar = ContextBar::default();
    bar.toggle(Popover::Color, egui::Rect::ZERO);
    assert!(bar.popover_open());
    bar.suppress_gesture();
    assert!(!bar.popover_open(), "a popover cannot outlive its bar");
}

#[test]
fn a_second_click_on_the_same_slot_closes_its_popover() {
    let mut bar = ContextBar::default();
    bar.toggle(Popover::Width, egui::Rect::ZERO);
    assert!(bar.popover_open());
    bar.toggle(Popover::Width, egui::Rect::ZERO);
    assert!(!bar.popover_open());
}

#[test]
fn a_popover_flips_above_the_bar_rather_than_off_the_pane() {
    // A bar parked low is the ordinary case, not the corner one: parking
    // it low is how a trader uncovers the price action above it. The
    // palette is several times the bar's own height, so downward-only
    // would put every colour swatch on the time axis for the session.
    let bounds = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
    let slot = egui::Rect::from_min_max(egui::pos2(100.0, 552.0), egui::pos2(132.0, 584.0));

    assert_eq!(
        popover_position(slot, egui::Vec2::ZERO, bounds).y,
        slot.bottom() + POPOVER_GAP_PX,
        "with nothing measured yet the answer is the usual downward one"
    );

    let tall = egui::vec2(160.0, 120.0);
    assert_eq!(
        popover_position(slot, tall, bounds).y,
        slot.top() - POPOVER_GAP_PX - tall.y,
        "no room below the pane, so it opens upward from the slot"
    );

    let short = egui::vec2(160.0, 10.0);
    assert_eq!(
        popover_position(slot, short, bounds).y,
        slot.bottom() + POPOVER_GAP_PX,
        "and one that does fit stays where the eye expects it"
    );
}

#[test]
fn a_popover_with_no_room_either_way_still_opens_downward() {
    // Above is the fallback, not a second rule: if it does not fit there
    // either, the popover keeps the position the eye is already on.
    let bounds = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 200.0));
    let slot = egui::Rect::from_min_max(egui::pos2(100.0, 150.0), egui::pos2(132.0, 182.0));
    let tall = egui::vec2(160.0, 400.0);
    assert_eq!(
        popover_position(slot, tall, bounds).y,
        slot.bottom() + POPOVER_GAP_PX
    );
}

#[test]
fn only_an_open_popover_owns_a_press_on_its_layer() {
    let mut bar = ContextBar::default();
    let id = egui::Id::new("popover");
    let layer = Some(egui::LayerId::new(egui::Order::Foreground, id));
    bar.popover_layer = Some(id);
    let elsewhere = Some((egui::pos2(500.0, 500.0), 3.0));
    assert!(
        !bar.owns_press(elsewhere, layer),
        "a closed popover owns nothing"
    );
    bar.open = Popover::Color;
    assert!(bar.owns_press(Some((egui::pos2(500.0, 500.0), 4.0)), layer));
    assert!(
        bar.owns_press(None, None),
        "the release frame is still owned"
    );
}

#[test]
fn a_popover_that_fits_the_pane_but_neither_side_covers_the_bar() {
    let bounds = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
    let slot = egui::Rect::from_min_max(egui::pos2(100.0, 180.0), egui::pos2(132.0, 212.0));
    let at = popover_position(slot, egui::vec2(160.0, 300.0), bounds);
    assert_eq!(at.y, 100.0, "bottom edge on the pane's bottom edge");
}

#[test]
fn a_popover_never_runs_off_the_right_of_the_pane() {
    let bounds = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
    let slot = egui::Rect::from_min_max(egui::pos2(960.0, 100.0), egui::pos2(992.0, 132.0));
    let wide = egui::vec2(200.0, 60.0);
    let at = popover_position(slot, wide, bounds);
    assert!(
        at.x + wide.x <= bounds.right(),
        "the palette stays inside the pane: {} + {} > {}",
        at.x,
        wide.x,
        bounds.right()
    );
    assert_eq!(
        at.x + wide.x,
        slot.right(),
        "and it right-aligns on the slot, so it still hangs off the control"
    );
}

#[test]
fn a_popover_that_fits_hangs_from_the_slot_it_belongs_to() {
    let bounds = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
    let slot = egui::Rect::from_min_max(egui::pos2(120.0, 100.0), egui::pos2(152.0, 132.0));
    let at = popover_position(slot, egui::vec2(200.0, 60.0), bounds);
    assert_eq!(
        at.x,
        slot.left(),
        "left edge to left edge is the resting rule"
    );
}

#[test]
fn each_popover_remembers_only_a_size_that_is_its_own() {
    // The size read back from egui decides which way a popover opens, so
    // the id it is stored under has to split on everything that changes
    // that size — and on nothing else, because a fresh id costs a frame
    // of egui's invisible sizing pass right where the trader clicked.
    assert_ne!(
        popover_id(Popover::Color, false, false),
        popover_id(Popover::Width, false, false),
        "one kind never answers for another"
    );
    assert_ne!(
        popover_id(Popover::Width, false, false),
        popover_id(Popover::GlyphSize, false, false)
    );
    assert_ne!(
        popover_id(Popover::Color, true, false),
        popover_id(Popover::Color, false, false),
        "the palette with a fill row is a different height"
    );
    assert_eq!(
        popover_id(Popover::Width, true, false),
        popover_id(Popover::Width, false, false),
        "but the width rows do not read fill, so they must not split on it"
    );
    assert_eq!(
        popover_id(Popover::GlyphSize, true, false),
        popover_id(Popover::GlyphSize, false, false)
    );
}

#[test]
fn a_new_selection_still_drops_what_belonged_to_the_old_object() {
    // The popover hangs off a slot of the bar as it stood for the object
    // that is no longer selected, and the rect is what the wake radius
    // measures against. Both are stale the instant the selection moves.
    let mut bar = ContextBar::default();
    bar.note_selection(Some(0));
    bar.toggle(Popover::Width, egui::Rect::ZERO);
    assert!(bar.popover_open());
    assert!(bar.note_selection(Some(1)));
    assert!(!bar.popover_open(), "a popover cannot outlive its object");
}
