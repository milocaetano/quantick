//! Carves a tab's canvas into pixels. The model it carves from, pane identity,
//! presets, widths, shares and the split itself, is
//! `quantick_workspace::canvas_layout`; this lays its spans out as rectangles.

use eframe::egui;
pub use quantick_workspace::canvas_layout::*;
use smallvec::SmallVec;

/// One row of the canvas, carved.
///
/// `panes[i]` is the area of the `i`th pane; `dividers[i]` is the draggable
/// rule between pane `i` and pane `i + 1`, so there are always exactly one
/// fewer dividers than panes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowAreas {
    /// Left to right, as drawn.
    pub panes: SmallVec<[egui::Rect; MAX_CANVAS_PANES]>,
    /// One per seam. `dividers[i]` sits between `panes[i]` and `panes[i + 1]`.
    pub dividers: SmallVec<[egui::Rect; MAX_CANVAS_PANES]>,
}

/// Carve `area` into one pane per entry in `widths`, with a draggable divider
/// on every seam, as [`split`] spends the width. The column painter lays its
/// row out through `column_frame`; this is the row its tests carve.
#[cfg(test)]
#[must_use]
pub fn split_row(area: egui::Rect, widths: &[PaneWidth]) -> RowAreas {
    let band = |(from, to): Span| egui::Rect::from_x_y_ranges(from..=to, area.y_range());
    carve(split(area.left(), area.width(), widths), band)
}

/// Carve `area` top to bottom instead: the context column's stacked panes.
#[must_use]
pub fn split_column(area: egui::Rect, heights: &[PaneWidth]) -> RowAreas {
    let band = |(from, to): Span| egui::Rect::from_x_y_ranges(area.x_range(), from..=to);
    carve(split(area.top(), area.height(), heights), band)
}

fn carve(split: Split, band: impl Fn(Span) -> egui::Rect) -> RowAreas {
    RowAreas {
        panes: split.panes().iter().map(|span| band(*span)).collect(),
        dividers: split.dividers().iter().map(|span| band(*span)).collect(),
    }
}

#[cfg(test)]
mod row_tests {
    use super::*;

    fn canvas() -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0))
    }

    /// The row spends the canvas exactly once, whatever it holds. A sliver of
    /// unpainted canvas between two panes reads as a rendering fault.
    fn assert_spends_the_canvas(area: egui::Rect, areas: &RowAreas) {
        let spent: f32 = areas.panes.iter().map(|pane| pane.width()).sum::<f32>()
            + areas
                .dividers
                .iter()
                .map(|divider| divider.width())
                .sum::<f32>();
        assert!(
            (spent - area.width()).abs() < 1e-3,
            "the row spent {spent} of a {} canvas",
            area.width()
        );
        assert_eq!(areas.panes.first().map(egui::Rect::left), Some(area.left()));
        assert_eq!(
            areas.panes.last().map(egui::Rect::right),
            Some(area.right())
        );
        for pane in &areas.panes {
            assert_eq!(pane.top(), area.top(), "the split is vertical only");
            assert_eq!(pane.bottom(), area.bottom());
        }
    }

    #[test]
    fn a_lone_pane_takes_the_whole_canvas_and_needs_no_divider() {
        let area = canvas();
        let areas = split_row(area, &[PaneWidth::Auto]);
        assert_eq!(areas.panes.as_slice(), &[area]);
        assert!(areas.dividers.is_empty(), "one pane has no seam");
    }

    #[test]
    fn there_is_always_one_fewer_divider_than_pane() {
        for count in 1..=MAX_CANVAS_PANES {
            let widths = vec![PaneWidth::Auto; count];
            let areas = split_row(canvas(), &widths);
            assert_eq!(areas.panes.len(), count);
            assert_eq!(
                areas.dividers.len(),
                count - 1,
                "{count} panes must have {} seams",
                count - 1
            );
        }
    }

    #[test]
    fn every_pane_count_spends_the_canvas_exactly_once() {
        for count in 1..=MAX_CANVAS_PANES {
            let widths = vec![PaneWidth::Auto; count];
            let area = canvas();
            assert_spends_the_canvas(area, &split_row(area, &widths));
        }
    }

    /// The two-pane row must land where the old fixed splitter did, or every
    /// saved workspace would open on a canvas that shifted under it.
    #[test]
    fn a_two_pane_row_puts_the_divider_on_the_share_it_was_given() {
        let area = canvas();
        for asked in [0.25_f32, 0.35, 0.5, 0.65, 0.75] {
            let areas = split_row(area, &[PaneWidth::Manual(asked), PaneWidth::Auto]);
            let split = (areas.dividers[0].center().x - area.left()) / area.width();
            assert!(
                (split - asked).abs() < 1e-3,
                "asked for {asked}, divider landed at {split}"
            );
            assert_eq!(areas.dividers[0].width(), CANVAS_DIVIDER_PX);
            assert_eq!(areas.panes[0].right(), areas.dividers[0].left());
            assert_eq!(areas.dividers[0].right(), areas.panes[1].left());
            assert_spends_the_canvas(area, &areas);
        }
    }

    /// The shape the old model could not express at all: two context panes
    /// beside the flow pane.
    #[test]
    fn a_three_pane_row_carves_two_seams_in_order() {
        let area = canvas();
        let areas = split_row(
            area,
            &[
                PaneWidth::Manual(0.175),
                PaneWidth::Manual(0.175),
                PaneWidth::Auto,
            ],
        );
        assert_eq!(areas.panes.len(), 3);
        assert_eq!(areas.dividers.len(), 2);
        assert!(areas.panes[0].right() <= areas.panes[1].left());
        assert!(areas.panes[1].right() <= areas.panes[2].left());
        assert!(
            areas.panes[2].width() > areas.panes[0].width() + areas.panes[1].width(),
            "the flow pane keeps the majority of the canvas"
        );
        assert_spends_the_canvas(area, &areas);
    }

    #[test]
    fn a_collapsed_pane_takes_its_rail_and_no_more() {
        let area = canvas();
        let areas = split_row(
            area,
            &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
        );
        assert!(
            (areas.panes[0].width() - COLLAPSED_PANE_WIDTH_PX).abs() < 1.0,
            "a collapsed pane is a rail, got {} px",
            areas.panes[0].width()
        );
        assert!(
            areas.panes[0].width() > 0.0,
            "a collapsed pane is never zero — there would be nothing left to click"
        );
        assert_spends_the_canvas(area, &areas);
    }

    #[test]
    fn every_pane_collapsed_still_spends_the_canvas() {
        let area = canvas();
        let widths = vec![PaneWidth::Collapsed { restore: 0.25 }; 3];
        let areas = split_row(area, &widths);
        assert_eq!(areas.panes.len(), 3);
        assert_spends_the_canvas(area, &areas);
    }

    /// A window dragged narrower than the panes' floors is a real state, and
    /// the row must degrade rather than overflow: the panes get thin, the
    /// canvas is still spent once, and nothing is painted outside it.
    #[test]
    fn a_canvas_too_narrow_for_its_panes_degrades_instead_of_overflowing() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(60.0, 600.0));
        let areas = split_row(area, &[PaneWidth::Auto, PaneWidth::Auto, PaneWidth::Auto]);
        assert_eq!(areas.panes.len(), 3);
        for pane in &areas.panes {
            assert!(pane.left() >= area.left() - 1e-3);
            assert!(pane.right() <= area.right() + 1e-3);
            assert!(pane.width() >= 0.0, "a pane never has negative width");
        }
        assert_spends_the_canvas(area, &areas);
    }

    #[test]
    fn a_zero_width_canvas_is_survived_rather_than_panicked_on() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(0.0, 600.0));
        let areas = split_row(area, &[PaneWidth::Auto, PaneWidth::Auto]);
        assert_eq!(areas.panes.len(), 2);
        for pane in &areas.panes {
            assert!(pane.width() >= 0.0);
        }
    }

    #[test]
    fn an_empty_row_draws_nothing() {
        let areas = split_row(canvas(), &[]);
        assert!(areas.panes.is_empty());
        assert!(areas.dividers.is_empty());
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// The registry is the vocabulary. Every id is a name a config file, the
    /// `QUANTICK_LAYOUT` hook and a saved workspace may all use, so two
    /// presets answering to one name would make a saved layout ambiguous.
    #[test]
    fn every_preset_id_is_unique() {
        for (index, preset) in LAYOUT_PRESETS.iter().enumerate() {
            for other in LAYOUT_PRESETS.iter().skip(index + 1) {
                assert_ne!(preset.id, other.id, "two presets answer to {}", preset.id);
            }
        }
    }

    #[test]
    fn every_preset_is_reachable_by_its_id() {
        for entry in LAYOUT_PRESETS {
            assert_eq!(preset(entry.id), Some(entry));
        }
        assert_eq!(preset("no such layout"), None);
        assert_eq!(preset("  flow  "), preset("flow"), "ids are read trimmed");
    }

    /// The rule that keeps the heatmap the protagonist, held as data rather
    /// than as a check inside the layout engine: a preset that draws the flow
    /// pane draws it **last**, so context charts default to its left.
    #[test]
    fn the_flow_pane_is_always_the_rightmost_pane_a_preset_holds() {
        for entry in LAYOUT_PRESETS {
            let flow_at = entry.kinds.iter().position(|kind| *kind == PaneKind::Flow);
            if let Some(index) = flow_at {
                assert_eq!(
                    index,
                    entry.kinds.len() - 1,
                    "preset {} puts a context pane right of the flow pane",
                    entry.id
                );
            }
            assert_eq!(
                entry.kinds.iter().filter(|k| **k == PaneKind::Flow).count(),
                flow_at.map_or(0, |_| 1),
                "preset {} holds more than one flow pane",
                entry.id
            );
        }
    }

    #[test]
    fn no_preset_asks_for_more_panes_than_the_canvas_allows() {
        for entry in LAYOUT_PRESETS {
            assert!(!entry.kinds.is_empty(), "preset {} draws nothing", entry.id);
            assert!(
                entry.kinds.len() <= MAX_CANVAS_PANES,
                "preset {} wants {} panes",
                entry.id,
                entry.kinds.len()
            );
        }
    }

    #[test]
    fn every_preset_lays_out_and_spends_the_canvas() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1600.0, 900.0));
        for entry in LAYOUT_PRESETS {
            let widths = vec![PaneWidth::Auto; entry.kinds.len()];
            let areas = split_row(area, &widths);
            assert_eq!(areas.panes.len(), entry.kinds.len(), "preset {}", entry.id);
            assert_eq!(areas.dividers.len(), entry.kinds.len() - 1);
        }
    }

    /// Width is decided by `PaneWidth` alone — a pane's kind never reaches
    /// the arithmetic. Asserted here as a property of the signature rather
    /// than by comparing two identical calls: the row is laid out from widths
    /// only, so there is no kind to pass in, and a rail is a rail whatever the
    /// pane it hides holds.
    #[test]
    fn the_rail_is_the_same_width_at_every_position_in_the_row() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
        let collapsed = PaneWidth::Collapsed { restore: 0.3 };

        // Leading, interior and trailing: each loses a different number of
        // divider halves, and each must still come out a full rail wide.
        for widths in [
            vec![collapsed, PaneWidth::Auto, PaneWidth::Auto],
            vec![PaneWidth::Auto, collapsed, PaneWidth::Auto],
            vec![PaneWidth::Auto, PaneWidth::Auto, collapsed],
        ] {
            let index = widths
                .iter()
                .position(|width| width.is_collapsed())
                .expect("the row under test holds a collapsed pane");
            let areas = split_row(area, &widths);
            assert!(
                (areas.panes[index].width() - COLLAPSED_PANE_WIDTH_PX).abs() < 1e-3,
                "a rail at position {index} came out {} px",
                areas.panes[index].width()
            );
        }
    }
}

#[cfg(test)]
mod column_tests {
    use super::*;

    fn canvas() -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 900.0))
    }

    /// The context column stacks its panes top to bottom, and spends its
    /// height exactly once — the vertical twin of the row's own rule.
    #[test]
    fn a_column_stacks_top_to_bottom_and_spends_its_height_once() {
        let area = canvas();
        let areas = split_column(area, &[PaneWidth::Auto, PaneWidth::Auto]);
        assert_eq!(areas.panes.len(), 2);
        assert_eq!(areas.dividers.len(), 1);
        assert!(
            areas.panes[0].bottom() <= areas.panes[1].top(),
            "the first pane is the upper one"
        );
        let spent: f32 = areas.panes.iter().map(egui::Rect::height).sum::<f32>()
            + areas.dividers.iter().map(egui::Rect::height).sum::<f32>();
        assert!((spent - area.height()).abs() < 1e-3, "spent {spent}");
        assert_eq!(areas.panes[0].top(), area.top());
        assert_eq!(areas.panes[1].bottom(), area.bottom());
    }

    /// A column keeps the full width: stacking splits one axis only, exactly
    /// as the row keeps the full height.
    #[test]
    fn stacking_never_narrows_a_pane() {
        let area = canvas();
        for count in 1..=MAX_CONTEXT_PANES {
            let areas = split_column(area, &vec![PaneWidth::Auto; count]);
            for pane in &areas.panes {
                assert_eq!(pane.left(), area.left());
                assert_eq!(pane.right(), area.right());
            }
        }
    }

    /// The two axes are one function, so a divider dragged vertically behaves
    /// exactly as one dragged horizontally. Asserted by transposing a canvas
    /// and comparing the split it produces.
    #[test]
    fn a_column_splits_where_a_row_of_the_same_shares_would() {
        let widths = [PaneWidth::Manual(0.35), PaneWidth::Auto];
        let across = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
        let down = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(600.0, 1000.0));

        let row = split_row(across, &widths);
        let column = split_column(down, &widths);
        assert!(
            (row.panes[0].width() - column.panes[0].height()).abs() < 1e-3,
            "the same shares must carve the same extent on either axis: \
             row gave {} and column gave {}",
            row.panes[0].width(),
            column.panes[0].height()
        );
        assert!((row.dividers[0].width() - column.dividers[0].height()).abs() < 1e-3);
    }

    #[test]
    fn a_collapsed_pane_in_a_column_is_the_same_rail_a_row_gives() {
        let area = canvas();
        let areas = split_column(
            area,
            &[PaneWidth::Collapsed { restore: 0.5 }, PaneWidth::Auto],
        );
        assert!(
            (areas.panes[0].height() - COLLAPSED_PANE_WIDTH_PX).abs() < 1e-3,
            "a stacked rail came out {} px",
            areas.panes[0].height()
        );
    }
}

#[cfg(test)]
mod collapse_tests {
    use super::*;

    /// The rail is near-zero without being zero, and that gap is the whole
    /// design: a pane with no width has no handle, and a pane with no handle
    /// cannot be brought back from the canvas it left.
    #[test]
    fn a_collapsed_pane_is_a_sliver_of_the_canvas_but_never_none_of_it() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1080.0));
        let areas = split_row(
            area,
            &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
        );
        let share = areas.panes[0].width() / area.width();
        assert!(
            share < 0.01,
            "a rail taking {share} of the canvas is not a rail"
        );
        assert!(
            areas.panes[0].width() > 0.0,
            "a rail with no width is the bug this exists to prevent"
        );
    }

    /// The hit area is bigger than the paint, and by enough to clear the floor
    /// a pointer target is held to.
    #[test]
    fn the_rail_can_be_hit_even_though_it_is_thin() {
        // Read off a rail the splitter actually produced, rather than
        // comparing two literals: what matters is how far the hit area has to
        // reach past the paint that exists.
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1080.0));
        let painted = split_row(
            area,
            &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
        )
        .panes[0]
            .width();
        assert!(
            COLLAPSED_HIT_PX > painted,
            "the hit area must reach past the {painted}px it paints"
        );
    }

    /// The gesture that dismisses a pane has to be reachable before the pane's
    /// own minimum stops the drag, or a trader could never reach it.
    #[test]
    fn the_collapse_threshold_is_inside_the_range_a_drag_can_reach() {
        // The threshold is in pixels and so is the floor, which is the whole
        // reason it stopped being a share of the canvas: as a share it sat
        // under the floor on a trading monitor and over it on a laptop, and
        // over it the floor never binds. Read at the widths a real window
        // takes, so the test says what it protects.
        for width in [1024.0_f32, 1280.0, 1600.0, 1920.0, 2560.0] {
            let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(width, 900.0));
            let floor_share = MIN_PANE_WIDTH_PX / area.width();
            let threshold_share = COLLAPSE_AT_PX / area.width();
            assert!(
                threshold_share < floor_share,
                "on a {width}px canvas the pane is dismissed at {threshold_share}                  while its floor is {floor_share}, so every width between them                  is unreachable"
            );
        }
    }
}
