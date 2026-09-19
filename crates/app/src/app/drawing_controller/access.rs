//! Drawing-only access to the workspace's existing stores. No ownership of
//! tabs, feeds, layouts or paper state moves into the controller.
use crate::app::arrangement_host::ArrangementHost;
use crate::drawings;
use crate::pane::{ChartPane, PaneSide};

pub(crate) struct DrawingReadAccess<'a> {
    tabs: &'a ArrangementHost,
}
impl<'a> DrawingReadAccess<'a> {
    pub(crate) fn new(tabs: &'a ArrangementHost) -> Self {
        Self { tabs }
    }
    fn pane(&self) -> &ChartPane {
        self.tabs[self.tabs.active_index()].drawing_pane()
    }
    fn focused(&self) -> &ChartPane {
        self.tabs[self.tabs.active_index()].focused_pane()
    }
    pub(super) fn current_range_owner(
        &self,
        requested: Option<crate::surfaces::drawing_chrome::QuickRangeOwner>,
    ) -> Option<crate::surfaces::drawing_chrome::QuickRangeOwner> {
        let requested = requested?;
        let tab = &self.tabs[self.tabs.active_index()];
        tab.sides().find_map(|side| {
            let pane = tab.pane(side);
            (pane.id == requested.pane).then_some(
                crate::surfaces::drawing_chrome::QuickRangeOwner {
                    tab: self.tabs.active_id(),
                    side,
                    pane: pane.id,
                    revision: pane.pagination_revision(),
                    layout: pane.layout_id().map(|id| id.0),
                },
            )
        })
    }
    pub(crate) fn authored_count(&self) -> usize {
        self.tabs
            .iter()
            .map(|tab| {
                tab.panes()
                    .map(|(pane, _)| pane.drawings.authored_count())
                    .sum::<usize>()
            })
            .sum()
    }
}
pub(super) struct ChromeFacts {
    pub tab: u64,
    pub side: PaneSide,
    pub pane_id: u64,
    pub chart_area: Option<eframe::egui::Rect>,
    pub focused_chart_area: Option<eframe::egui::Rect>,
    pub lane_divider_x: Option<f32>,
    pub legends: Option<eframe::egui::Rect>,
    pub auto_range: Option<(f64, f64)>,
}
impl DrawingReadAccess<'_> {
    pub(super) fn drawings(&self) -> &drawings::Drawings {
        &self.pane().drawings
    }
    pub(super) fn chart_area(&self) -> Option<eframe::egui::Rect> {
        self.pane().frame.chart_area
    }
    pub(super) fn slots(&self) -> usize {
        self.pane().slots()
    }
    pub(super) fn history_right(&self, chart: eframe::egui::Rect) -> f32 {
        self.pane().frame.lane_divider_x.unwrap_or(chart.right())
    }
    pub(super) fn band(&self, drawing: &drawings::Drawing) -> Option<&crate::bands::Band> {
        crate::bands::band_of(self.pane().frame.cached_bands(), drawing)
    }
    pub(super) fn band_label(&self, drawing: &drawings::Drawing) -> crate::bands::BandLabel {
        crate::bands::label_for(&self.focused().indicators, drawing)
    }
    pub(super) fn projected_points(
        &self,
        drawing: &drawings::Drawing,
        right: f32,
        total: usize,
        scale: &crate::chart::PriceScale,
    ) -> smallvec::SmallVec<[eframe::egui::Pos2; 4]> {
        self.pane()
            .drawing_projection()
            .projected_drawing_points(drawing, right, total, scale)
    }
    pub(super) fn chrome_facts(&self) -> ChromeFacts {
        let pane = self.pane();
        ChromeFacts {
            tab: self.tabs.active_id(),
            side: self.tabs[self.tabs.active_index()].drawing_side(),
            pane_id: pane.id,
            chart_area: pane.frame.chart_area,
            focused_chart_area: self.focused().frame.chart_area,
            lane_divider_x: pane.frame.lane_divider_x,
            legends: pane
                .frame
                .flow_legend
                .into_iter()
                .chain(pane.frame.indicator_legend)
                .reduce(|a, b| a.union(b)),
            auto_range: pane.frame.auto_range,
        }
    }
}
pub(crate) struct DrawingAccess<'a> {
    tabs: &'a mut ArrangementHost,
}
impl<'a> DrawingAccess<'a> {
    pub(crate) fn new(tabs: &'a mut ArrangementHost) -> Self {
        Self { tabs }
    }
    fn pane(&self) -> &ChartPane {
        self.tabs[self.tabs.active_index()].drawing_pane()
    }
    fn pane_mut(&mut self) -> &mut ChartPane {
        let active = self.tabs.active_index();
        self.tabs.runtime_mut(active).drawing_pane_mut()
    }
    pub(super) fn target(&self) -> (u64, PaneSide) {
        (
            self.tabs.active_id(),
            self.tabs[self.tabs.active_index()].drawing_side(),
        )
    }
    pub(super) fn cancel_paper(&mut self) -> bool {
        let active = self.tabs.active_index();
        self.tabs.runtime_mut(active).paper.cancel_interaction()
    }
    pub(super) fn record(
        &mut self,
        tab: u64,
        side: PaneSide,
        index: usize,
        before: drawings::Drawing,
    ) {
        if let Some(tab) = self.tabs.by_id_mut(tab) {
            tab.pane_mut(side).drawings.record_edit_of(index, before);
        }
    }
    pub(super) fn commit_inline(
        &mut self,
        chrome: &mut crate::surfaces::drawing_chrome::DrawingChromeSurface,
    ) {
        chrome.commit_inline_text(self.tabs);
    }
    pub(super) fn sync_inline(
        &mut self,
        chrome: &crate::surfaces::drawing_chrome::DrawingChromeSurface,
    ) {
        chrome.sync_content_editing(self.tabs);
    }
    pub(super) fn paste(&mut self, drawing: &drawings::Drawing, offset: f32) {
        let active = self.tabs.active_index();
        let tab = self.tabs.runtime_mut(active);
        let focused = tab.focused_side();
        for pane in tab.panes_mut() {
            pane.drawings.select(None);
        }
        let _ = tab.pane_mut(focused).drawings.paste(drawing, offset);
    }
    pub(super) fn arm_copy(
        &mut self,
        side: PaneSide,
        copy: drawings::DrawingId,
        spec: &crate::strategy_presets::StoredPreset,
        label: String,
        alerts: &mut dyn crate::audio::AlertSink,
    ) -> Result<(), String> {
        let active = self.tabs.active_index();
        self.tabs
            .runtime_mut(active)
            .arm_strategy_instance(alerts, side, copy, spec, label)
    }
    pub(crate) fn remove_authored(&mut self) -> usize {
        let mut removed = 0;
        for tab in self.tabs.iter_mut() {
            for pane in tab.panes_mut() {
                let taken = pane.drawings.remove_authored();
                if taken > 0 {
                    pane.strategies.sweep_orphans(&pane.drawings);
                    removed += taken;
                }
            }
        }
        removed
    }
}

impl DrawingAccess<'_> {
    pub(super) fn drawings(&self) -> &drawings::Drawings {
        &self.pane().drawings
    }
    pub(super) fn drawings_mut(&mut self) -> &mut drawings::Drawings {
        &mut self.pane_mut().drawings
    }
    pub(super) fn remove_strategy(&mut self, id: drawings::DrawingId) {
        self.pane_mut().strategies.remove_for_drawing(id);
    }
    // Each destructive operation keeps its original owner even if it clears
    // the selection. The next independent command resolves its owner afresh.
    pub(super) fn undo_drawings(&mut self) {
        let pane = self.pane_mut();
        pane.drawings.undo();
        pane.strategies.sweep_orphans(&pane.drawings);
    }
    pub(super) fn redo_drawings(&mut self) {
        let pane = self.pane_mut();
        pane.drawings.redo();
        pane.strategies.sweep_orphans(&pane.drawings);
    }
    pub(super) fn delete_all_drawings(&mut self) -> usize {
        let pane = self.pane_mut();
        let deleted = pane.drawings.delete_all();
        pane.strategies.sweep_orphans(&pane.drawings);
        deleted
    }
    pub(super) fn selected_value_per_px(&self) -> Option<f64> {
        let pane = self.pane();
        crate::bands::selected_value_per_px(&pane.drawings, pane.frame.cached_bands())
    }
    pub(super) fn retime_selected(&mut self) {
        self.pane_mut().retime_selected();
    }
    pub(super) fn center_on_drawing(&mut self, index: usize) {
        let pane = self.pane_mut();
        let slots = pane.slots();
        if let Some(chart) = pane.frame.chart_area {
            let points = &pane.drawings.items()[index].points;
            if !points.is_empty() {
                let mid = points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32;
                pane.viewport.center_on_bar(mid, chart.width(), slots);
            }
        }
    }
    pub(super) fn watching_strategy(
        &self,
        source: drawings::DrawingId,
    ) -> Option<(crate::strategy_presets::StoredPreset, String)> {
        use quantick_strategy::ArmedState;
        self.pane()
            .strategies
            .anchors
            .for_drawing(source)
            .filter(|instance| {
                matches!(
                    instance.armed.state(),
                    ArmedState::Armed | ArmedState::Fired { .. } | ArmedState::InPosition
                )
            })
            .map(|instance| (instance.spec.clone(), instance.preset.clone()))
    }
    #[cfg(any(feature = "drawing-harness", test))]
    pub(super) fn text_note_point(&self) -> Option<drawings::ChartPoint> {
        let pane = self.pane();
        let slots = pane.slots();
        let close = pane
            .closed_bar(slots.saturating_sub(1))
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close));
        let plan = crate::surfaces::drawing_chrome::launch::NotePlacement::plan(
            pane.frame.chart_area.is_some(),
            slots,
            close,
            pane.frame.auto_range,
        )?;
        Some(plan.point(pane.slot_open_time(plan.slot)))
    }
}
