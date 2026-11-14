//! Watermark painting over the host's authoritative preview rectangle.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use crate::theme;
use eframe::egui;
use quantick_ui_model::indicator_preview::{Intent, Model, State, Target};

const WATERMARK_TOP_OFFSET_PX: f32 = 34.0;
const WATERMARK_FONT_PX: f32 = 13.0;
const WATERMARK_TEXT: &str = "PREVIEW — settings not applied";

#[derive(Default)]
pub(crate) struct View;

impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "indicator-preview-watermark"
    }

    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _: &SurfaceEnv<'_>) -> Option<Intent> {
        Some(Intent::Force(
            crate::hooks::captured::var("QUANTICK_INDICATOR_PREVIEW")
                .is_some_and(|value| value == "1"),
        ))
    }

    fn paint_updated(
        &mut self,
        session: &mut quantick_ui_model::UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) {
        let Some(env) = env else {
            return;
        };
        let preview_available = env.indicator_preview_area.is_some();
        let focused_available = env.focused_chart_area.is_some();
        if (
            session.state().preview_available,
            session.state().focused_available,
        ) != (preview_available, focused_available)
        {
            session.update(Intent::Refresh {
                preview_available,
                focused_available,
            });
        }
        self.paint(session.state(), target, Some(env));
    }

    fn paint(
        &mut self,
        state: &State,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let env = env?;
        let rect = match state.target {
            Target::None => None,
            Target::Preview => env.indicator_preview_area,
            Target::Focused => env.focused_chart_area,
        }?;
        let ctx = target.context();
        let palette = theme::get(ctx);
        // A label-only layer introduces no pointer hit-test rectangle.
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new(self.id()),
        ));
        painter.text(
            egui::pos2(rect.center().x, rect.top() + WATERMARK_TOP_OFFSET_PX),
            egui::Align2::CENTER_TOP,
            WATERMARK_TEXT,
            palette.body_font(WATERMARK_FONT_PX),
            palette.accent(),
        );
        None
    }
}

pub(crate) fn effects(_: ()) -> SurfaceResponse {
    SurfaceResponse::default()
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_INDICATOR_PREVIEW"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn registered_preview_paints_words_and_authoritative_geometry_in_same_frame() {
        let focused = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        let preview = focused.translate(egui::vec2(400.0, 0.0));
        // Preserve all four original painted-word cases, plus real-over-forced precedence.
        for (forced, real, focus, expected) in [
            (false, None, Some(focused), None),
            (false, Some(preview), None, Some(preview)),
            (true, None, Some(focused), Some(focused)),
            (true, None, None, None),
            (true, Some(preview), Some(focused), Some(preview)),
        ] {
            let ctx = egui::Context::default();
            let mut registry = super::super::Surfaces::default();
            crate::hooks::captured::set_on_this_thread(Some(&[(
                "QUANTICK_INDICATOR_PREVIEW",
                if forced { "1" } else { "0" },
            )]));
            let output = ctx.run(Default::default(), |ctx| {
                registry.draw_all(
                    ctx,
                    &SurfaceEnv {
                        indicator_preview_area: real,
                        focused_chart_area: focus,
                        ..SurfaceEnv::quiet(Instant::now())
                    },
                );
            });
            crate::hooks::captured::set_on_this_thread(None);
            let painted = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == WATERMARK_TEXT => Some(text),
                    _ => None,
                });
            assert_eq!(painted.is_some(), expected.is_some());
            if let Some(rect) = expected {
                let text = painted.unwrap();
                let bounds = text.galley.rect.translate(text.pos.to_vec2());
                assert!((bounds.center().x - rect.center().x).abs() < 1.0);
                assert!((bounds.top() - rect.top() - WATERMARK_TOP_OFFSET_PX).abs() < 1.0);
            }
        }
    }
}
