//! Provenance markers share the chart's viewport and current trade-derived bars.
use super::super::{
    SEAM_DASH_PX, SEAM_GAP_PX, SEAM_LABEL_INSET_PX, SEAM_LABEL_PT, draw_dashed_vertical,
};
use super::{Contribution, Package};
use crate::{theme, viewport::Viewport};
use eframe::egui;
const GAP_CAPTION_BOTTOM_CLEARANCE_PX: f32 = 3.0 * SEAM_LABEL_PT;
pub(super) const PACKAGE: Package = Package {
    layers: &[
        quantick_layers::ChartLayer::BackfillDivider,
        quantick_layers::ChartLayer::SeamDivider,
    ],
    contributions: &[
        Contribution::Seam(seam),
        Contribution::Backfill(backfill),
        Contribution::FeedGaps(feed_gaps),
    ],
};
pub(in crate::pane) struct DividerPass<'a> {
    pub painter: &'a egui::Painter,
    pub pane: egui::Rect,
    pub total: usize,
    pub candle_width: f32,
    pub viewport: &'a Viewport,
    pub seam: usize,
    pub boundary: Option<usize>,
    pub bars: &'a [quantick_engine::Bar],
    pub gaps: &'a [quantick_feed::FeedGap],
}
fn seam(p: &mut DividerPass<'_>) {
    let DividerPass {
        painter,
        pane,
        total,
        candle_width,
        viewport,
        ..
    } = *p;

    let seam = p.seam;
    if seam == 0 || seam >= total {
        return;
    }
    let x = viewport.x_center(seam, pane.right(), total) - candle_width / 2.0;
    if x < pane.left() || x > pane.right() {
        return; // off-screen
    }
    draw_dashed_vertical(
        painter,
        x,
        pane,
        SEAM_DASH_PX,
        SEAM_GAP_PX,
        theme::SEAM_LINE,
    );
    painter.text(
        egui::pos2(x - SEAM_LABEL_INSET_PX, pane.top() + SEAM_LABEL_INSET_PX),
        egui::Align2::RIGHT_TOP,
        "venue",
        egui::FontId::proportional(SEAM_LABEL_PT),
        theme::SEAM_LABEL,
    );
}
fn backfill(p: &mut DividerPass<'_>) {
    let DividerPass {
        painter,
        pane,
        total,
        candle_width,
        viewport,
        ..
    } = *p;

    let Some(boundary) = p.boundary else {
        return;
    };
    if boundary == 0 {
        return; // nothing backfilled
    }
    // The engine counts its own bars; the venue prefix sits in front of
    // them, so the slot is offset by however many bars that is.
    let boundary = boundary + p.seam;
    // The divider sits at the left edge of the first live bar.
    let x = viewport.x_center(boundary, pane.right(), total) - candle_width / 2.0;
    if x < pane.left() || x > pane.right() {
        return; // off-screen
    }
    painter.line_segment(
        [egui::pos2(x, pane.top()), egui::pos2(x, pane.bottom())],
        egui::Stroke::new(1.0_f32, theme::AMBER),
    );
    let font = egui::FontId::proportional(11.0);
    painter.text(
        egui::pos2(x - 4.0, pane.bottom() - 4.0),
        egui::Align2::RIGHT_BOTTOM,
        "backfill",
        font.clone(),
        theme::TEXT_MUTED,
    );
    painter.text(
        egui::pos2(x + 4.0, pane.bottom() - 4.0),
        egui::Align2::LEFT_BOTTOM,
        "live",
        font,
        theme::AMBER,
    );
}
fn gap_slot(
    bars: &[quantick_engine::Bar],
    prefix: usize,
    gap: quantick_feed::FeedGap,
) -> Option<usize> {
    let index = bars.partition_point(|bar| bar.open_time < gap.to_ms);
    (index < bars.len()).then_some(prefix + index)
}
fn feed_gaps(p: &mut DividerPass<'_>) {
    let DividerPass {
        painter,
        pane,
        total,
        candle_width,
        viewport,
        gaps,
        ..
    } = *p;

    for gap in gaps {
        let Some(slot) = gap_slot(p.bars, p.seam, *gap) else {
            continue;
        };
        if slot == 0 || slot >= total {
            continue;
        }
        let x = viewport.x_center(slot, pane.right(), total) - candle_width / 2.0;
        if x < pane.left() || x > pane.right() {
            continue; // off-screen
        }
        draw_dashed_vertical(painter, x, pane, SEAM_DASH_PX, SEAM_GAP_PX, theme::GAP_LINE);
        // Above the bottom footer/backfill labels, away from the top
        // foreground loading overlay and flow legend. Keep the caption
        // to the right of its line, opposite the venue seam's caption.
        painter.text(
            egui::pos2(
                x + SEAM_LABEL_INSET_PX,
                pane.bottom() - GAP_CAPTION_BOTTOM_CLEARANCE_PX.min(pane.height() / 2.0),
            ),
            egui::Align2::LEFT_BOTTOM,
            format!("{} gap", gap.duration_label()),
            egui::FontId::proportional(SEAM_LABEL_PT),
            theme::GAP_LABEL,
        );
    }
}
