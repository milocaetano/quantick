//! Cumulative delivery observations and their immutable status-bar readout.

use std::sync::Arc;

use eframe::egui;
use quantick_feed::{FeedExclusion, FeedExclusions, FeedIntegrity};

/// Rebuilt only when a diagnostic, attachment, or reset changes the facts.
#[derive(Debug)]
pub(crate) struct DeliveryText(pub [String; 3]);

pub(crate) struct DeliveryView {
    pub exclusions: FeedExclusions,
    pub reporting: bool,
    local_fixture: bool,
    integrity: FeedIntegrity,
    text: Arc<DeliveryText>,
}

impl DeliveryView {
    pub fn new(reporting: bool) -> Self {
        let mut view = Self {
            exclusions: FeedExclusions::default(),
            reporting,
            local_fixture: false,
            integrity: FeedIntegrity::default(),
            text: Arc::new(DeliveryText(Default::default())),
        };
        view.rebuild();
        view
    }

    pub fn for_receiver(receiver: &quantick_feed::ObservedReceiver) -> Self {
        let mut view = Self::new(receiver.reports_exclusions());
        view.local_fixture = receiver.is_local_fixture();
        view.rebuild();
        view
    }

    pub fn attach_receiver(&mut self, receiver: &quantick_feed::ObservedReceiver, keep: bool) {
        self.local_fixture = receiver.is_local_fixture();
        self.attach(receiver.reports_exclusions(), keep);
    }

    pub fn text(&self) -> Arc<DeliveryText> {
        Arc::clone(&self.text)
    }

    pub fn attach(&mut self, reporting: bool, keep: bool) {
        self.reporting = reporting;
        if !keep {
            self.reset();
        } else {
            self.rebuild();
        }
    }

    pub fn reset(&mut self) {
        self.exclusions = FeedExclusions::default();
        self.integrity = FeedIntegrity::default();
        self.rebuild();
    }

    pub fn refresh(&mut self, integrity: FeedIntegrity) {
        if self.integrity != integrity {
            self.integrity = integrity;
            self.rebuild();
        }
    }

    pub fn observe_exclusion(&mut self, event: FeedExclusion, integrity: FeedIntegrity) {
        self.exclusions.observe(event);
        self.integrity = integrity;
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.text = Arc::new(DeliveryText([
            format!(
                "{}Source IDs: {} skipped / {} non-monotonic",
                if self.local_fixture {
                    "LOCAL SYNTHETIC | "
                } else {
                    ""
                },
                count(self.integrity.missing_messages),
                count(self.integrity.non_monotonic)
            ),
            format!(
                "Outages: {} (loss unknown; zero is not completeness)",
                count(self.integrity.unknown_loss)
            ),
            if self.reporting {
                format!(
                    "Received rows excluded: {} malformed / {} stale",
                    count(self.exclusions.malformed_rows),
                    count(self.exclusions.stale_rows)
                )
            } else {
                "Received-row exclusions: unavailable".to_owned()
            },
        ]));
    }
}

fn count(value: u64) -> String {
    if value == u64::MAX {
        format!(">={value}")
    } else {
        value.to_string()
    }
}

#[derive(Clone)]
struct CachedGalleys {
    text: Arc<DeliveryText>,
    font: egui::FontId,
    scale: f32,
    width: f32,
    color: egui::Color32,
    cells: [Arc<egui::Galley>; 3],
}

/// Existing text and galleys are shared across steady frames, including the
/// status-model transfer. String copies happen only on cache invalidation.
/// Reserve the exact cached text height before the chart chooses its area.
/// This prevents a first-frame wrapped-row resize from moving popup anchors.
pub(crate) fn draw_panel(ctx: &egui::Context, text: &Arc<DeliveryText>) {
    let width = (ctx.available_rect().width() - 20.0).max(1.0);
    let font = egui::TextStyle::Small.resolve(&ctx.style());
    let cache = cached_context(ctx, text, width, font);
    paint_panel(ctx, cache);
}

fn paint_panel(ctx: &egui::Context, cache: CachedGalleys) {
    let height = cache.cells.iter().map(|cell| cell.size().y).sum::<f32>()
        + 2.0 * ctx.style().spacing.item_spacing.y
        + 8.0;
    egui::TopBottomPanel::bottom("feed_delivery_status")
        .exact_height(height)
        .frame(
            egui::Frame::none()
                .fill(crate::theme::CHROME)
                .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
        )
        .show(ctx, |ui| {
            for cell in cache.cells {
                ui.add(egui::Label::new(cell));
            }
        });
}

fn cached_context(
    ctx: &egui::Context,
    text: &Arc<DeliveryText>,
    width: f32,
    font: egui::FontId,
) -> CachedGalleys {
    let id = egui::Id::new("feed.delivery.readout");
    let scale = ctx.pixels_per_point();
    let color = crate::theme::TEXT_MUTED;
    let previous = ctx.data(|data| data.get_temp::<CachedGalleys>(id));
    previous
        .filter(|cache| {
            Arc::ptr_eq(&cache.text, text)
                && cache.font == font
                && cache.width == width
                && cache.scale == scale
                && cache.color == color
        })
        .unwrap_or_else(|| {
            let cache = CachedGalleys {
                text: Arc::clone(text),
                font: font.clone(),
                scale,
                width,
                color,
                cells: std::array::from_fn(|i| {
                    ctx.fonts(|fonts| fonts.layout(text.0[i].clone(), font.clone(), color, width))
                }),
            };
            ctx.data_mut(|data| data.insert_temp(id, cache.clone()));
            cache
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_render_reuses_galleys_and_invalidates_for_width_font_scale_and_facts() {
        let ctx = egui::Context::default();
        let mut view = DeliveryView::new(true);
        let mut prior: Option<CachedGalleys> = None;
        for (width, font_size, scale, unchanged) in [
            (900.0, 12.0, 1.0, false),
            (900.0, 12.0, 1.0, true),
            (220.0, 12.0, 1.0, false),
            (220.0, 12.0, 1.0, true),
            (220.0, 16.0, 1.0, false),
            (220.0, 16.0, 2.0, false),
        ] {
            ctx.set_pixels_per_point(scale);
            ctx.style_mut(|style| {
                style.text_styles.insert(
                    egui::TextStyle::Small,
                    egui::FontId::proportional(font_size),
                );
            });
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    draw_panel(ctx, &view.text());
                    {
                        let cache = ctx
                            .data(|data| {
                                data.get_temp::<CachedGalleys>(egui::Id::new(
                                    "feed.delivery.readout",
                                ))
                            })
                            .unwrap();
                        if let Some(previous) = &prior {
                            if unchanged {
                                assert!(Arc::ptr_eq(&previous.cells[0], &cache.cells[0]));
                                let before = crate::work_meter::tally();
                                for _ in 0..50 {
                                    std::hint::black_box(cached_context(
                                        ctx,
                                        &view.text(),
                                        cache.width,
                                        cache.font.clone(),
                                    ));
                                }
                                let cost = crate::work_meter::tally().since(before);
                                assert_eq!(
                                    (cost.allocs, cost.reallocs),
                                    (0, 0),
                                    "steady render cache must not clone Strings"
                                );
                            } else {
                                assert!(
                                    cache.width != previous.width
                                        || cache.font != previous.font
                                        || cache.scale != previous.scale
                                );
                            }
                        }
                        for cell in &cache.cells {
                            assert!(cell.size().x <= cache.width + 0.1);
                        }
                        prior = Some(cache);
                    }
                },
            );
        }
        let old = view.text();
        view.observe_exclusion(
            quantick_feed::FeedExclusion {
                reason: quantick_feed::ExclusionReason::MalformedRow,
                rows: std::num::NonZeroU64::new(1).unwrap(),
            },
            FeedIntegrity::default(),
        );
        assert!(!Arc::ptr_eq(&old, &view.text()));
        assert!(view.text().0[2].contains("1 malformed"));
        view.attach(true, true);
        assert_eq!(view.exclusions.malformed_rows, 1);
        view.attach(false, false);
        assert_eq!(view.exclusions.malformed_rows, 0);
        assert!(view.text().0[2].contains("unavailable"));
    }

    #[test]
    fn actual_steady_panel_adds_no_string_allocation_over_cached_galley_painting() {
        let ctx = egui::Context::default();
        let view = DeliveryView::new(true);
        let mut costs = Vec::new();
        for frame in 0..6 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                let previous = ctx.data(|data| {
                    data.get_temp::<CachedGalleys>(egui::Id::new("feed.delivery.readout"))
                });
                let before = crate::work_meter::tally();
                if frame % 2 == 1 {
                    paint_panel(ctx, previous.unwrap());
                } else {
                    draw_panel(ctx, &view.text());
                }
                costs.push(crate::work_meter::tally().since(before));
            });
        }
        let (actual, reference) = (costs[4], costs[5]);
        assert_eq!(
            (actual.allocs, actual.alloc_bytes, actual.reallocs),
            (reference.allocs, reference.alloc_bytes, reference.reallocs),
            "actual steady frame must add no allocation over painting the retained galleys"
        );
    }

    #[test]
    fn cached_model_transfer_allocates_nothing_and_zero_is_not_completeness() {
        let view = DeliveryView::new(true);
        let before = crate::work_meter::tally();
        for _ in 0..100 {
            std::hint::black_box(view.text());
        }
        let cost = crate::work_meter::tally().since(before);
        assert_eq!((cost.allocs, cost.reallocs), (0, 0));
        assert!(view.text.0[1].contains("zero is not completeness"));
        assert!(DeliveryView::new(false).text.0[2].contains("unavailable"));
        assert_eq!(count(u64::MAX), format!(">={}", u64::MAX));
    }
}
