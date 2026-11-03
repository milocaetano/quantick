//! The order-flow pane's identity: the band across its top, the frame around it,
//! one header per chart side under the band and the divider between the sides.
//! Where each stands, what each says and what a narrow pane gives up; the window
//! only paints the result.

use quantick_orderflow::{LaneView, LaneWindow, lane_window_label, lane_window_options};

use crate::pane_header::{
    CAPSULE_FRAME_X_PX, CAPSULE_SLOT_HEIGHT_PX, CapsuleSide, FlowHeaderState,
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
/// A capsule docked in a side header: one row of slots and its 1 px border.
pub const DOCKED_CAPSULE_HEIGHT_PX: f32 = CAPSULE_SLOT_HEIGHT_PX + 2.0;
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

/// The headers of a row spanning `left..right` for this view, the chart's split at
/// `split` if it has one. Two sides meet at the split; a closed side, or a pane
/// without a tape (`shown` absent), leaves one header at full width.
#[must_use]
pub fn side_headers(
    shown: Option<LaneView>,
    left: f32,
    right: f32,
    split: Option<f32>,
) -> [Option<(CapsuleSide, f32, f32)>; 2] {
    let beside = split.filter(|x| shown == Some(LaneView::Beside) && *x > left && *x < right);
    match (shown, beside) {
        (_, Some(x)) => [
            Some((CapsuleSide::Candles, left, x)),
            Some((CapsuleSide::Tape, x, right)),
        ],
        (Some(LaneView::TapeOnly), None) => [Some((CapsuleSide::Tape, left, right)), None],
        _ => [Some((CapsuleSide::Candles, left, right)), None],
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

/// The widths of the trail's header. An absent capsule is zero.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrailWidths {
    pub side: f32,
    pub plate: f32,
    pub bars: f32,
    pub indicators_with_words: f32,
    pub indicators_icon: f32,
    pub capsule: f32,
}

/// What the trail's header keeps at a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrailFit {
    pub plate: bool,
    pub indicators_words: bool,
    pub capsule: bool,
}

/// The narrow rule of the trail's header, in the order things go: the word beside
/// the indicators icon, then the plate, then the bars text clips down to
/// [`BARS_MIN_PX`], then the capsule. The icon and the clipped bars always stay.
#[must_use]
pub fn trail_fit(widths: &TrailWidths) -> TrailFit {
    let item = |width: f32| {
        if width > 0.0 {
            width + SIDE_GAP_PX
        } else {
            0.0
        }
    };
    let (plate, capsule) = (item(widths.plate), item(widths.capsule));
    let icon = widths.indicators_icon;
    let fits = |bars: f32, indicators: f32, plate: f32| {
        let core = SIDE_PAD_LEFT_PX + bars + SIDE_GAP_PX + indicators + SIDE_PAD_RIGHT_PX;
        core + plate + capsule <= widths.side
    };
    TrailFit {
        indicators_words: fits(widths.bars, widths.indicators_with_words, plate),
        plate: fits(widths.bars, icon, plate),
        capsule: widths.capsule > 0.0 && fits(widths.bars.min(BARS_MIN_PX), icon, 0.0),
    }
}

/// The widths of the tape's header. An absent capsule is zero.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TapeWidths {
    pub side: f32,
    pub plate: f32,
    pub selector: f32,
    pub capsule: f32,
}

/// What the tape's header keeps at a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapeFit {
    pub plate: bool,
    pub selector: bool,
    pub capsule: bool,
}

/// The narrow rule of the tape's header, in the order things go: the plate, then
/// the selector (the tape's menu still sets the window), then the capsule.
#[must_use]
pub fn tape_fit(widths: &TapeWidths) -> TapeFit {
    let room = widths.side - SIDE_PAD_LEFT_PX - SIDE_PAD_RIGHT_PX;
    let item = |width: f32| {
        if width > 0.0 {
            width + SIDE_GAP_PX
        } else {
            0.0
        }
    };
    let capsule = widths.capsule;
    let selector = item(widths.selector) + capsule;
    TapeFit {
        plate: item(widths.plate) + selector <= room,
        selector: selector <= room,
        capsule: capsule > 0.0 && capsule <= room,
    }
}

/// A docked capsule's corner in a side ending at `side_right`, the row's top at
/// `row_top`: against the side's far end, centred above the row's rule.
#[must_use]
pub fn docked_capsule_corner(side_right: f32, row_top: f32, capsule_px: f32) -> (f32, f32) {
    let spare = SIDE_HEADER_HEIGHT_PX - 1.0 - DOCKED_CAPSULE_HEIGHT_PX;
    (
        side_right - SIDE_PAD_RIGHT_PX - capsule_px,
        row_top + spare / 2.0,
    )
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
    pub window: LaneWindow,
    pub label: String,
    pub selected: bool,
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
            window,
            label: lane_window_label(window, state.window_reference_ms),
            selected: quantick_orderflow::same_lane_window(state.window, window),
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

    /// The split is the chart's; a closed side or a tape alone is one whole header.
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
            headers(Some(LaneView::TapeOnly), Some(0.0)),
            [tape(0.0), None]
        );
        assert_eq!(headers(None, Some(70.0)), [candles(100.0), None]);
        assert_eq!(headers(beside, Some(0.0)), [candles(100.0), None]);
    }

    fn trail(side: f32) -> TrailWidths {
        TrailWidths {
            side,
            plate: 40.0,
            bars: 80.0,
            indicators_with_words: 90.0,
            indicators_icon: 30.0,
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
            let fit = trail_fit(&trail(side));
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

    /// A capsule the data emptied asks for no room and is not listed as kept.
    #[test]
    fn an_empty_capsule_takes_no_room() {
        let fit = trail_fit(&TrailWidths {
            capsule: 0.0,
            ..trail(500.0)
        });
        assert!(fit.plate && fit.indicators_words && !fit.capsule);
    }

    /// The plate, then the selector, then the capsule.
    #[test]
    fn a_narrowing_tape_gives_things_up_in_order() {
        let pads = SIDE_PAD_LEFT_PX + SIDE_PAD_RIGHT_PX;
        let widths = |side| TapeWidths {
            side,
            plate: 40.0,
            selector: 60.0,
            capsule: 78.0,
        };
        let keeps = |side: f32| {
            let fit = tape_fit(&widths(side));
            (fit.plate, fit.selector, fit.capsule)
        };
        let selector = 60.0 + SIDE_GAP_PX + 78.0;
        let all = 40.0 + SIDE_GAP_PX + selector;
        assert_eq!(keeps(pads + all), (true, true, true));
        assert_eq!(keeps(pads + all - 1.0), (false, true, true));
        assert_eq!(keeps(pads + selector - 1.0), (false, false, true));
        assert_eq!(keeps(pads + 78.0 - 1.0), (false, false, false));
    }

    /// Docked 8 px off its side's end, centred above the rule, slots inside.
    #[test]
    fn a_docked_capsule_stands_against_its_sides_end() {
        let corner = docked_capsule_corner(500.0, 40.0, 100.0);
        assert_eq!(corner, (392.0, 42.5));
        assert_eq!(docked_slot_origin(corner), (397.0, 43.5));
        let spare = SIDE_HEADER_HEIGHT_PX - 1.0 - DOCKED_CAPSULE_HEIGHT_PX;
        assert!(spare >= 0.0, "the capsule fits the row");
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
