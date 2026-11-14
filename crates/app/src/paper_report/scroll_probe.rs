//! Report-owned, feature-gated wheel input over rectangles from the real draw.
use eframe::egui;
use quantick_paper::report::scroll_probe::ReportScroll;

#[derive(Default)]
pub(super) struct Probe(
    ReportScroll,
    #[cfg(test)] pub(super) Option<(egui::Rect, egui::Rect)>,
);

impl Probe {
    pub(super) fn arm(&mut self, value: &str) {
        if !self.0.arm(value) {
            tracing::warn!(
                event_code = "PAPER_REPORT_SCROLL_REJECTED",
                "expected up to four comma-separated report:<signed notches> or rows:<signed notches> stages, nonzero with at most 120 total notches"
            );
        }
    }
    pub(super) fn report(&mut self, ui: &egui::Ui) {
        self.0
            .report(edges(ui.max_rect().intersect(ui.clip_rect())));
        ui.ctx()
            .data_mut(|data| data.remove::<egui::Rect>(egui::Id::new("report_trade_viewport")));
    }
    pub(super) fn rows(&mut self, ui: &egui::Ui) {
        if let Some(rect) = ui
            .ctx()
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("report_trade_viewport")))
        {
            self.0.rows(edges(rect));
        }
    }
    pub(super) fn footer(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        #[cfg(test)]
        {
            self.1 = Some((rect, ui.clip_rect()));
        }
        if self.0.active() {
            tracing::info!(
                event_code = "PAPER_REPORT_SCROLL_VIEW",
                footer_visible = ui.clip_rect().contains_rect(rect),
                footer_top = rect.top(),
                viewport_bottom = ui.clip_rect().bottom(),
                "report footer after normal layout"
            );
        }
    }
    pub(super) fn push(&mut self, open: bool, input: &mut egui::RawInput) {
        let Some((x, y, delta)) = self.0.input(open) else {
            return;
        };
        let at = egui::pos2(x, y);
        input.events.extend([
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                modifiers: egui::Modifiers::default(),
            },
        ]);
        tracing::info!(
            event_code = "PAPER_REPORT_SCROLL_WHEEL",
            ?at,
            delta,
            "report-scoped wheel input"
        );
    }
}

fn edges(rect: egui::Rect) -> [f32; 4] {
    [rect.left(), rect.top(), rect.right(), rect.bottom()]
}

impl super::ReportState {
    pub(crate) fn arm_scroll_probe(&mut self, value: &str) {
        self.adapter.view.scroll_probe.arm(value);
    }
    pub(crate) fn push_scroll_probe(&mut self, input: &mut egui::RawInput) {
        let open = self.report_open();
        self.adapter.view.scroll_probe.push(open, input);
    }
}
