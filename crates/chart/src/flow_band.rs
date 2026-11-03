//! The order-flow pane's identity: the band across its top, the frame around it,
//! one header per chart side under the band and the divider between the sides.
//! Where each stands, what each says and what a narrow pane gives up; the window
//! only paints the result.

use quantick_orderflow::{
    LaneView, LaneWindow, lane_window_label, lane_window_options, lane_window_pick,
    same_lane_window,
};

use crate::pane_header::{
    CAPSULE_FRAME_X_PX, Capsule, CapsuleSide, FlowHeaderState, LayerGlyph, item_px,
};

/// The band's height.
pub const BAND_HEIGHT_PX: f32 = 26.0;
/// A side header's height, its closing rule included.
pub const SIDE_HEADER_HEIGHT_PX: f32 = 28.0;
/// The frame's thickness: it closes the pane's left, right and bottom, and the
/// band closes its top.
pub const FRAME_PX: f32 = 3.0;
/// What a side header keeps clear at its left and right ends, and between items.
pub const SIDE_PAD_LEFT_PX: f32 = 10.0;
pub const SIDE_PAD_RIGHT_PX: f32 = 8.0;
pub const SIDE_GAP_PX: f32 = 7.0;
/// The second header's left end, clear of the divider standing on its edge.
pub const SECOND_SIDE_PAD_LEFT_PX: f32 = 14.0;
/// A docked capsule's slots: lower than a floating capsule's, so the capsule sits
/// in its header with room above and below.
pub const DOCKED_SLOT_HEIGHT_PX: f32 = 18.0;
/// A capsule docked in a header: one row of slots and its 1 px border.
pub const DOCKED_CAPSULE_HEIGHT_PX: f32 = DOCKED_SLOT_HEIGHT_PX + 2.0;
/// The divider's bar.
pub const DIVIDER_WIDTH_PX: f32 = 4.0;
/// The grip's three dots: their side, and the distance from one to the next.
pub const GRIP_DOT_PX: f32 = 2.0;
const GRIP_PITCH_PX: f32 = 5.0;

/// What the window selector's tooltip says it does.
pub const WINDOW_SELECTOR_HINT: &str = "How much market time the tape shows";
/// The selector's name in the scene.
pub const WINDOW_SELECTOR_NAME: &str = "Tape window";

/// An axis-aligned box, in the window's points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// A flow pane's area, carved: the band, the row of side headers under it and the
/// chart inside the frame. A pane too short for its chrome spends what it has on
/// the chrome, top down, and never inverts a box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowPaneLayout {
    pub band: FlowRect,
    pub side_row: FlowRect,
    pub chart: FlowRect,
}

/// Carve a flow pane spanning these edges.
#[must_use]
pub fn flow_pane_layout(left: f32, top: f32, right: f32, bottom: f32) -> FlowPaneLayout {
    let band_bottom = (top + BAND_HEIGHT_PX).min(bottom);
    let row_bottom = (band_bottom + SIDE_HEADER_HEIGHT_PX).min(bottom);
    let inner_left = (left + FRAME_PX).min(right);
    let inner_right = (right - FRAME_PX).max(inner_left);
    FlowPaneLayout {
        band: FlowRect {
            left,
            top,
            right,
            bottom: band_bottom,
        },
        side_row: FlowRect {
            left: inner_left,
            top: band_bottom,
            right: inner_right,
            bottom: row_bottom,
        },
        chart: FlowRect {
            left: inner_left,
            top: row_bottom,
            right: inner_right,
            bottom: (bottom - FRAME_PX).max(row_bottom),
        },
    }
}

/// The sides on screen for this view over a chart spanning `left..right`, with the
/// tape's divider if there is one, leftmost first: the one split every flow surface
/// reads. A hidden side has none; a closed side's rail stays outside the open one;
/// a pane without a tape (`shown` absent) is all candles. A divider on or past an
/// edge splits nothing.
#[must_use]
pub fn flow_sides(
    shown: Option<LaneView>,
    left: f32,
    right: f32,
    divider: Option<f32>,
) -> [Option<(CapsuleSide, f32, f32)>; 2] {
    let candles = |end| Some((CapsuleSide::Candles, left, end));
    let tape = |start| Some((CapsuleSide::Tape, start, right));
    match (shown, divider.filter(|x| *x > left && *x < right)) {
        (Some(LaneView::Beside), Some(x)) => [candles(x), tape(x)],
        (Some(LaneView::CandlesOnly), Some(x)) => [candles(x), None],
        (Some(LaneView::TapeOnly), x) => [tape(x.unwrap_or(left)), None],
        _ => [candles(right), None],
    }
}

/// The headers of a row spanning `left..right`: [`flow_sides`], a lone side's
/// header taking the whole row, the closed side's rail included.
#[must_use]
pub fn side_headers(
    shown: Option<LaneView>,
    left: f32,
    right: f32,
    split: Option<f32>,
) -> [Option<(CapsuleSide, f32, f32)>; 2] {
    match flow_sides(shown, left, right, split) {
        [Some((side, ..)), None] => [Some((side, left, right)), None],
        sides => sides,
    }
}

/// The live strip's column on the trail, `width` wide against the tape's divider
/// and never past the chart's `left`: its left and right edges. Only beside the
/// tape; elsewhere the strip is the gutter's ([`gutter_strip_px`]).
#[must_use]
pub fn trail_strip(
    shown: Option<LaneView>,
    left: f32,
    divider: Option<f32>,
    width: f32,
) -> Option<(f32, f32)> {
    let divider = divider.filter(|_| shown == Some(LaneView::Beside) && width > 0.0)?;
    Some(((divider - width).max(left), divider))
}

/// The strip width the plot carves before the price gutter: the strip's own
/// unless the trail takes it beside the tape. The tape alone has no strip.
#[must_use]
pub fn gutter_strip_px(shown: Option<LaneView>, width: f32) -> f32 {
    match shown {
        Some(LaneView::Beside | LaneView::TapeOnly) => 0.0,
        _ => width,
    }
}

/// What one side header carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideParts {
    pub plate: bool,
    /// The pane's bars control and its indicators button.
    pub bars: bool,
    pub window: bool,
    pub capsule: bool,
    /// The room at its left end.
    pub pad_left: f32,
}

/// What the header for `side` carries, `first` in its row, on a pane with a tape
/// or without. The trail's carries the pane's bars and indicators, so a tape alone
/// has neither; the tape's carries the window; a pane without a tape names no side
/// and docks no capsule. The second starts further in, clear of the divider.
#[must_use]
pub fn side_parts(side: CapsuleSide, first: bool, has_tape: bool) -> SideParts {
    SideParts {
        plate: has_tape,
        bars: side == CapsuleSide::Candles,
        window: has_tape && side == CapsuleSide::Tape,
        capsule: has_tape,
        pad_left: if first {
            SIDE_PAD_LEFT_PX
        } else {
            SECOND_SIDE_PAD_LEFT_PX
        },
    }
}

/// The word on a side's plate.
#[must_use]
pub fn plate_word(side: CapsuleSide) -> &'static str {
    match side {
        CapsuleSide::Candles => "TRAIL",
        CapsuleSide::Tape => "TAPE",
        CapsuleSide::Time => "",
    }
}

/// How far the bars text clips before the capsule gives up its room.
pub const BARS_MIN_PX: f32 = 36.0;

/// The widths of a side header's items, left to right. An absent item is zero.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SideWidths {
    pub side: f32,
    /// The header's room at its left end ([`SideParts::pad_left`]).
    pub pad_left: f32,
    pub plate: f32,
    pub bars: f32,
    pub indicators_with_words: f32,
    pub indicators_icon: f32,
    pub window: f32,
    pub capsule: f32,
}

/// What a side header keeps at a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideFit {
    pub plate: bool,
    pub indicators_words: bool,
    pub window: bool,
    pub capsule: bool,
}

/// The narrow rule of a side header, in the order things go. The trail's: the
/// word beside the indicators icon, then the plate, then the bars text clips down
/// to [`BARS_MIN_PX`], then the capsule; the icon and the clipped bars never go.
/// The tape's: the plate, then the window selector (the tape's menu still sets
/// the window), then the capsule.
#[must_use]
pub fn side_fit(widths: &SideWidths) -> SideFit {
    let fits = |items: &[f32]| {
        let items: f32 = items.iter().map(|px| item_px(*px, SIDE_GAP_PX)).sum();
        let pads = widths.pad_left + SIDE_PAD_RIGHT_PX;
        pads + (items - SIDE_GAP_PX).max(0.0) <= widths.side
    };
    let SideWidths {
        plate,
        bars,
        window,
        capsule,
        ..
    } = *widths;
    let icon = widths.indicators_icon;
    SideFit {
        indicators_words: fits(&[plate, bars, widths.indicators_with_words, capsule]),
        plate: fits(&[plate, bars, icon, window, capsule]),
        window: fits(&[window, capsule]),
        capsule: capsule > 0.0 && fits(&[bars.min(BARS_MIN_PX), icon, capsule]),
    }
}

/// A docked capsule's corner in a header ending at `side_right`, spanning
/// `top..bottom`: against the header's far end, centred above its 1 px rule. A
/// flow side's header and a time pane's dock it alike. `None` when a short pane
/// clamped the header below the capsule's height: the capsule goes rather than
/// rise over the band above or sink into the chart below.
#[must_use]
pub fn docked_capsule_corner(
    side_right: f32,
    top: f32,
    bottom: f32,
    capsule_px: f32,
) -> Option<(f32, f32)> {
    let spare = bottom - top - 1.0 - DOCKED_CAPSULE_HEIGHT_PX;
    (spare >= 0.0).then_some((
        side_right - SIDE_PAD_RIGHT_PX - capsule_px,
        top + spare / 2.0,
    ))
}

/// What a header keeps free at its right end for `side`'s capsule with every
/// layer in it, the gap before it included: what a time pane's header lays out
/// around before its capsule paints.
#[must_use]
pub fn docked_reserve_px(side: CapsuleSide) -> f32 {
    let every = |toggle| LayerGlyph {
        toggle,
        on: true,
        blocked: None,
    };
    Capsule::of(side, every, Some(1.0)).width_px() + SIDE_GAP_PX
}

/// Where a docked capsule's first slot starts, from the capsule's own corner.
#[must_use]
pub fn docked_slot_origin(corner: (f32, f32)) -> (f32, f32) {
    (corner.0 + CAPSULE_FRAME_X_PX, corner.1 + 1.0)
}

/// The top of each of the grip's dots on a divider spanning `top..bottom`.
#[must_use]
pub fn grip_dots(top: f32, bottom: f32) -> [f32; 3] {
    let middle = f32::midpoint(top, bottom) - GRIP_DOT_PX / 2.0;
    [middle - GRIP_PITCH_PX, middle, middle + GRIP_PITCH_PX]
}

/// One line of the window selector's list.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowOption {
    pub label: String,
    pub selected: bool,
    /// What a click asks for ([`lane_window_pick`]): nothing on `auto` while it is
    /// in force and filed, so its zoom is kept; a fixed window in force is filed again.
    pub pick: Option<LaneWindow>,
}

/// What the selector reads this frame.
#[must_use]
pub fn window_label(state: &FlowHeaderState) -> String {
    lane_window_label(state.window, state.window_reference_ms)
}

/// The selector's list: the one the tape's menu offers, the window in force
/// marked. A stored window no preset names is in force with no line marked, and
/// the selector's own words still say what it is.
#[must_use]
pub fn window_options(state: &FlowHeaderState) -> Vec<WindowOption> {
    lane_window_options()
        .into_iter()
        .map(|window| WindowOption {
            label: lane_window_label(window, state.window_reference_ms),
            selected: same_lane_window(state.window, window),
            pick: lane_window_pick(state.window, state.window_setting, window),
        })
        .collect()
}

#[cfg(test)]
mod flow_band_tests {
    use super::*;

    const OPEN: FlowHeaderState = FlowHeaderState {
        view: LaneView::Beside,
        shown_view: LaneView::Beside,
        market_held: false,
        candle_sample: None,
        tape_sample: None,
        window: LaneWindow::Auto { zoom: 1.0 },
        window_reference_ms: None,
        window_setting: LaneWindow::Auto { zoom: 1.0 },
    };

    /// The band is the mock's 26 px, the side headers its 28, and the frame's 3 px
    /// closes the left, the right and the bottom around them.
    #[test]
    fn the_pane_carves_band_row_and_chart_inside_the_frame() {
        let layout = flow_pane_layout(100.0, 50.0, 900.0, 650.0);
        assert_eq!(
            layout.band,
            FlowRect {
                left: 100.0,
                top: 50.0,
                right: 900.0,
                bottom: 76.0
            }
        );
        assert_eq!(
            layout.side_row,
            FlowRect {
                left: 103.0,
                top: 76.0,
                right: 897.0,
                bottom: 104.0
            }
        );
        assert_eq!(
            layout.chart,
            FlowRect {
                left: 103.0,
                top: 104.0,
                right: 897.0,
                bottom: 647.0
            }
        );
    }

    /// A pane shorter than its chrome keeps every box the right way up.
    #[test]
    fn a_short_pane_never_inverts_a_box() {
        let layout = flow_pane_layout(0.0, 0.0, 4.0, 30.0);
        assert_eq!(layout.band.bottom, 26.0);
        assert_eq!(layout.side_row.bottom, 30.0);
        assert_eq!(layout.chart.top, 30.0);
        assert_eq!(layout.chart.bottom, 30.0);
        assert!(layout.chart.left <= layout.chart.right);
    }

    /// A hidden side has no room, and a side keeps its own edge; a divider on an
    /// edge splits nothing.
    #[test]
    fn the_view_decides_the_sides() {
        let candles = |end| Some((CapsuleSide::Candles, 0.0, end));
        let tape = |start| Some((CapsuleSide::Tape, start, 100.0));
        let sides = |view, divider| flow_sides(Some(view), 0.0, 100.0, divider);
        assert_eq!(
            sides(LaneView::Beside, Some(70.0)),
            [candles(70.0), tape(70.0)]
        );
        assert_eq!(sides(LaneView::Beside, None), [candles(100.0), None]);
        assert_eq!(sides(LaneView::Beside, Some(0.0)), [candles(100.0), None]);
        assert_eq!(sides(LaneView::CandlesOnly, None), [candles(100.0), None]);
        assert_eq!(
            sides(LaneView::CandlesOnly, Some(92.0)),
            [candles(92.0), None]
        );
        assert_eq!(sides(LaneView::TapeOnly, Some(8.0)), [tape(8.0), None]);
        assert_eq!(sides(LaneView::TapeOnly, None), [tape(0.0), None]);
        assert_eq!(
            flow_sides(None, 0.0, 100.0, Some(70.0)),
            [candles(100.0), None]
        );
    }

    /// The headers split where the sides do; a lone side's header, rail and all,
    /// is the whole row.
    #[test]
    fn the_headers_split_where_the_chart_does() {
        let candles = |end| Some((CapsuleSide::Candles, 0.0, end));
        let tape = |start| Some((CapsuleSide::Tape, start, 100.0));
        let beside = Some(LaneView::Beside);
        let headers = |shown, split| side_headers(shown, 0.0, 100.0, split);
        assert_eq!(headers(beside, Some(70.0)), [candles(70.0), tape(70.0)]);
        assert_eq!(headers(beside, None), [candles(100.0), None]);
        assert_eq!(
            headers(Some(LaneView::CandlesOnly), Some(92.0)),
            [candles(100.0), None],
            "the closed tape's rail is not a side"
        );
        assert_eq!(
            headers(Some(LaneView::TapeOnly), Some(8.0)),
            [tape(0.0), None]
        );
        assert_eq!(headers(None, Some(70.0)), [candles(100.0), None]);
    }

    /// Beside the tape the strip stands on the trail against the divider, paid
    /// for by the trail; the trail alone keeps it at the gutter; the tape alone
    /// has none.
    #[test]
    fn the_strip_is_the_trails() {
        let (beside, alone) = (Some(LaneView::Beside), Some(LaneView::TapeOnly));
        let candles = Some(LaneView::CandlesOnly);
        assert_eq!(
            trail_strip(beside, 0.0, Some(70.0), 10.0),
            Some((60.0, 70.0))
        );
        assert_eq!(
            trail_strip(beside, 65.0, Some(70.0), 10.0),
            Some((65.0, 70.0))
        );
        assert_eq!(trail_strip(beside, 0.0, Some(70.0), 0.0), None);
        assert_eq!(trail_strip(beside, 0.0, None, 10.0), None);
        for view in [candles, alone, None] {
            assert_eq!(trail_strip(view, 0.0, Some(70.0), 10.0), None, "{view:?}");
        }
        assert_eq!(gutter_strip_px(beside, 10.0), 0.0);
        assert_eq!(gutter_strip_px(alone, 10.0), 0.0);
        assert_eq!(gutter_strip_px(candles, 10.0), 10.0);
        assert_eq!(gutter_strip_px(None, 10.0), 10.0);
    }

    fn trail(side: f32) -> SideWidths {
        SideWidths {
            side,
            pad_left: SIDE_PAD_LEFT_PX,
            plate: 40.0,
            bars: 80.0,
            indicators_with_words: 90.0,
            indicators_icon: 30.0,
            window: 0.0,
            capsule: 100.0,
        }
    }

    /// The word, then the plate, then the bars clip, then the capsule.
    #[test]
    fn a_narrowing_trail_gives_things_up_in_order() {
        let core = SIDE_PAD_LEFT_PX + 80.0 + SIDE_PAD_RIGHT_PX + SIDE_GAP_PX;
        let plate = 40.0 + SIDE_GAP_PX;
        let capsule = 100.0 + SIDE_GAP_PX;
        let keeps = |side: f32| {
            let fit = side_fit(&trail(side));
            (fit.indicators_words, fit.plate, fit.capsule)
        };
        assert_eq!(keeps(core + 90.0 + plate + capsule), (true, true, true));
        assert_eq!(
            keeps(core + 90.0 + plate + capsule - 1.0),
            (false, true, true)
        );
        assert_eq!(keeps(core + 30.0 + plate + capsule), (false, true, true));
        assert_eq!(keeps(core + 30.0 + capsule), (false, false, true));
        let clipped = core - 80.0 + BARS_MIN_PX;
        assert_eq!(keeps(clipped + 30.0 + capsule), (false, false, true));
        assert_eq!(keeps(clipped + 30.0 + capsule - 1.0), (false, false, false));
    }

    /// The bars clip away first; only then does the button lose its word. Its
    /// icon is in every sum the rule makes, so nothing it drops takes the button.
    #[test]
    fn the_indicators_button_keeps_its_icon_when_its_word_cannot_fit() {
        let bare = SideWidths {
            plate: 0.0,
            capsule: 0.0,
            ..trail(0.0)
        };
        let least = SIDE_PAD_LEFT_PX + 80.0 + SIDE_GAP_PX + 90.0 + SIDE_PAD_RIGHT_PX;
        let words = |side| side_fit(&SideWidths { side, ..bare }).indicators_words;
        assert!(words(least));
        assert!(!words(least - 1.0));
    }

    /// A capsule the data emptied asks for no room and is not listed as kept.
    #[test]
    fn an_empty_capsule_takes_no_room() {
        let fit = side_fit(&SideWidths {
            capsule: 0.0,
            ..trail(500.0)
        });
        assert!(fit.plate && fit.indicators_words && !fit.capsule);
    }

    /// The plate, then the selector, then the capsule; the second header measures
    /// from its own, wider, left end.
    #[test]
    fn a_narrowing_tape_gives_things_up_in_order() {
        let pads = SECOND_SIDE_PAD_LEFT_PX + SIDE_PAD_RIGHT_PX;
        let widths = |side| SideWidths {
            side,
            pad_left: side_parts(CapsuleSide::Tape, false, true).pad_left,
            plate: 40.0,
            window: 60.0,
            capsule: 78.0,
            ..SideWidths::default()
        };
        let keeps = |side: f32| {
            let fit = side_fit(&widths(side));
            (fit.plate, fit.window, fit.capsule)
        };
        let selector = 60.0 + SIDE_GAP_PX + 78.0;
        let all = 40.0 + SIDE_GAP_PX + selector;
        assert_eq!(keeps(pads + all), (true, true, true));
        assert_eq!(keeps(pads + all - 1.0), (false, true, true));
        assert_eq!(keeps(pads + selector - 1.0), (false, false, true));
        assert_eq!(keeps(pads + 78.0 - 1.0), (false, false, false));
    }

    /// The trail's header carries the bars; the tape's the window, alone on the
    /// pane too, where it has no bars and no indicators; a pane with no tape names
    /// no side and docks nothing.
    #[test]
    fn each_header_carries_its_parts() {
        let parts = |side, first, tape| {
            let p = side_parts(side, first, tape);
            (p.plate, p.bars, p.window, p.capsule, p.pad_left)
        };
        let (candles, tape) = (CapsuleSide::Candles, CapsuleSide::Tape);
        let (first, second) = (SIDE_PAD_LEFT_PX, SECOND_SIDE_PAD_LEFT_PX);
        assert_eq!(parts(candles, true, true), (true, true, false, true, first));
        assert_eq!(parts(tape, false, true), (true, false, true, true, second));
        assert_eq!(parts(tape, true, true), (true, false, true, true, first));
        assert_eq!(
            parts(candles, true, false),
            (false, true, false, false, first)
        );
    }

    /// Docked 8 px off its header's end, centred above the rule, slots inside: 3.5
    /// under the band in a side header, which with the band's own 3 below its
    /// switches leaves 6.5 between them.
    #[test]
    fn a_docked_capsule_stands_against_its_sides_end() {
        let corner = docked_capsule_corner(500.0, 40.0, 40.0 + SIDE_HEADER_HEIGHT_PX, 100.0);
        assert_eq!(corner, Some((392.0, 43.5)));
        assert_eq!(docked_slot_origin((392.0, 43.5)), (397.0, 44.5));
        assert_eq!(DOCKED_CAPSULE_HEIGHT_PX, 20.0);
        let time = docked_capsule_corner(500.0, 0.0, 34.0, 100.0);
        assert_eq!(
            time.map(|c| c.1),
            Some(6.5),
            "a time pane's taller header centres it too"
        );
    }

    /// A pane too short for its side row clamps the row; the capsule then fits it
    /// exactly or goes, and never stands over the band or below the row.
    #[test]
    fn a_short_pane_keeps_its_capsule_inside_the_row() {
        for height in [0.0, 6.0, 12.0, 20.0, 21.0, 24.0, 40.0, 54.0, 90.0] {
            let row = flow_pane_layout(100.0, 50.0, 900.0, 50.0 + height).side_row;
            let corner = docked_capsule_corner(row.right, row.top, row.bottom, 100.0);
            let fits = row.bottom - row.top >= DOCKED_CAPSULE_HEIGHT_PX + 1.0;
            assert_eq!(corner.is_some(), fits, "{height}");
            if let Some((_, top)) = corner {
                assert!(top >= row.top, "{height}: over the band");
                assert!(
                    top + DOCKED_CAPSULE_HEIGHT_PX <= row.bottom,
                    "{height}: below the row"
                );
            }
        }
    }

    /// A time pane's header keeps its capsule's widest room: the footprint glyph in
    /// its frame, and the gap before it.
    #[test]
    fn a_time_header_reserves_its_capsule() {
        let glyph = crate::pane_header::GLYPH_SLOT_PX;
        let frame = 2.0 * CAPSULE_FRAME_X_PX;
        assert_eq!(
            docked_reserve_px(CapsuleSide::Time),
            glyph + frame + SIDE_GAP_PX
        );
    }

    /// Three dots around the divider's middle.
    #[test]
    fn the_grip_is_centred() {
        let [a, b, c] = grip_dots(100.0, 300.0);
        assert_eq!(b + GRIP_DOT_PX / 2.0, 200.0);
        assert_eq!((b - a, c - b), (5.0, 5.0));
    }

    /// Auto, then the presets, one list with the menu; the words are the label's.
    #[test]
    fn the_selector_lists_auto_and_the_presets() {
        let state = FlowHeaderState {
            window_reference_ms: Some(8_000),
            ..OPEN
        };
        assert_eq!(window_label(&state), "auto (≈ 8 s)");
        let words: Vec<_> = window_options(&state)
            .into_iter()
            .map(|option| (option.label, option.selected))
            .collect();
        let picks: Vec<_> = window_options(&state)
            .into_iter()
            .map(|option| option.pick)
            .collect();
        assert_eq!(picks[0], None, "auto in force picks nothing, zoom kept");
        assert_eq!(picks[1], Some(LaneWindow::Fixed { ms: 10_000 }));
        assert_eq!(
            words,
            [
                ("auto (≈ 8 s)".to_owned(), true),
                ("10 s".to_owned(), false),
                ("30 s".to_owned(), false),
                ("1 min".to_owned(), false),
                ("2 min".to_owned(), false),
                ("5 min".to_owned(), false),
            ]
        );
    }

    /// A fixed window in force is marked and still filed when picked: the wheel may
    /// have reached it without making it the setting.
    #[test]
    fn a_fixed_window_in_force_is_filed_again() {
        let minute = LaneWindow::Fixed { ms: 60_000 };
        let state = FlowHeaderState {
            window: minute,
            ..OPEN
        };
        let options = window_options(&state);
        assert!(options[3].selected);
        assert_eq!(options[3].pick, Some(minute));
        assert_eq!(options[0].pick, Some(LaneWindow::default()));
    }

    /// A tape navigated to `auto` over a filed fixed window files `auto` when the
    /// selector picks it; filed too, it keeps the zoom.
    #[test]
    fn auto_over_a_filed_fixed_window_is_filed() {
        let state = FlowHeaderState {
            window: LaneWindow::Auto { zoom: 2.0 },
            window_setting: LaneWindow::Fixed { ms: 60_000 },
            ..OPEN
        };
        assert!(window_options(&state)[0].selected);
        assert_eq!(window_options(&state)[0].pick, Some(LaneWindow::default()));
        let filed = FlowHeaderState {
            window_setting: LaneWindow::Auto { zoom: 2.0 },
            ..state
        };
        assert_eq!(window_options(&filed)[0].pick, None);
    }

    /// A 15 s stored before the presets moved loads, reads and stays what it is.
    #[test]
    fn a_stored_window_no_preset_names_still_reads() {
        let state = FlowHeaderState {
            window: LaneWindow::Fixed { ms: 15_000 },
            ..OPEN
        };
        assert_eq!(window_label(&state), "15 s");
        assert!(window_options(&state).iter().all(|option| !option.selected));
    }
}
