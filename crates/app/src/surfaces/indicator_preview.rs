//! The indicator-preview watermark, as a [`Surface`].
//!
//! While a settings dialog is previewing a draft, the chart under it is
//! showing numbers that are **not** the ones on disk. A full SELL triangle
//! drawn from a slider mid-drag must never wear the authority of a committed
//! signal (trader-ux review), and a trader glancing at the pane a second
//! later has no way to tell — so the pane says so itself: one line, over the
//! chart the preview is being judged on. The legend chip says the same thing
//! in the corner; this says it where the signals are.
//!
//! It owns no state at all — what to say is entirely a function of what the
//! host is previewing, which arrives through [`SurfaceEnv`]. That makes it
//! the smallest possible member of the registry, and a useful shape to have
//! in it: a surface may be a painter with nothing to remember, and still
//! belongs here rather than in the trunk, because the constants and the
//! placement rule are its own.

use eframe::egui;

use super::{Surface, SurfaceEnv, SurfaceResponse};
use crate::theme;

/// Distance below the pane's top edge, in pixels — below the top-centre toast
/// lane ("loading venue history"), so the two never overprint each other.
const WATERMARK_TOP_OFFSET_PX: f32 = 34.0;

/// Watermark font size, in pixels: larger than a legend chip, far from a
/// headline.
const WATERMARK_FONT_PX: f32 = 13.0;

/// What the watermark says. One sentence, and the dash rather than a colon
/// because the second half is the consequence, not a list.
const WATERMARK_TEXT: &str = "PREVIEW — settings not applied";

/// The banner over a pane whose indicator is showing an unapplied draft.
#[derive(Default)]
pub(crate) struct IndicatorPreviewSurface {
    /// The capture hook: paint the watermark over the focused pane even
    /// though no dialog is previewing.
    ///
    /// Without it this surface would be photographable only by driving an
    /// unrelated dialog into preview state, which is exactly the
    /// "reachable by hook" rule's reason for existing — a surface nobody can
    /// stage is a surface nobody checks.
    forced: bool,
}

impl Surface for IndicatorPreviewSurface {
    fn id(&self) -> &'static str {
        "indicator-preview-watermark"
    }

    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        self.forced = std::env::var("QUANTICK_INDICATOR_PREVIEW").is_ok_and(|value| value == "1");
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        // The real preview wins over the hook: a capture that stages one and
        // then also forces the banner must photograph the pane the
        // application actually chose, not the focused one.
        let area = env
            .indicator_preview_area
            .or_else(|| self.forced.then_some(env.focused_chart_area).flatten());
        let Some(rect) = area else {
            return SurfaceResponse::default();
        };
        // A painter on its own layer rather than an `Area`: the watermark is
        // a label over the chart with nothing to interact with, and a layer
        // painter costs no hit-test rectangle for the pointer to snag on.
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new(self.id()),
        ));
        painter.text(
            egui::pos2(rect.center().x, rect.top() + WATERMARK_TOP_OFFSET_PX),
            egui::Align2::CENTER_TOP,
            WATERMARK_TEXT,
            egui::FontId::proportional(WATERMARK_FONT_PX),
            theme::ACCENT,
        );
        SurfaceResponse::default()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn env(preview: Option<egui::Rect>, focused: Option<egui::Rect>) -> SurfaceEnv<'static> {
        SurfaceEnv {
            indicator_preview_area: preview,
            focused_chart_area: focused,
            ..SurfaceEnv::quiet(Instant::now())
        }
    }

    fn pane() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0))
    }

    /// Painting is a side effect on the context, so the test asks egui what
    /// actually landed rather than trusting the call was made — and asks for
    /// the *words*, which is what a trader reads, rather than for a layer id
    /// a refactor could keep while the text stopped being drawn.
    fn painted(surface: &mut IndicatorPreviewSurface, env: &SurfaceEnv<'_>) -> bool {
        let ctx = egui::Context::default();
        let output = ctx.run(Default::default(), |ctx| {
            surface.draw(ctx, env);
        });
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => text.galley.text() == WATERMARK_TEXT,
            _ => false,
        })
    }

    /// No preview, no hook: the pane is showing what is on disk, and a
    /// watermark saying otherwise would be the lie.
    #[test]
    fn a_pane_with_nothing_previewed_gets_no_watermark() {
        let mut surface = IndicatorPreviewSurface::default();
        assert!(!painted(&mut surface, &env(None, Some(pane()))));
    }

    /// A live preview marks the pane it is being judged on.
    #[test]
    fn a_previewed_pane_is_marked() {
        let mut surface = IndicatorPreviewSurface::default();
        assert!(painted(&mut surface, &env(Some(pane()), None)));
    }

    /// The hook stages the banner without a dialog, so a capture can
    /// photograph it.
    #[test]
    fn the_hook_marks_the_focused_pane() {
        let mut surface = IndicatorPreviewSurface { forced: true };
        assert!(painted(&mut surface, &env(None, Some(pane()))));
    }

    /// Forced or not, a pane that has never been laid out has no rectangle to
    /// paint on, and the surface returns rather than inventing one.
    #[test]
    fn a_pane_that_never_drew_is_left_alone() {
        let mut surface = IndicatorPreviewSurface { forced: true };
        assert!(!painted(&mut surface, &env(None, None)));
    }
}
