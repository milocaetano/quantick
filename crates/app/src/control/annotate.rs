//! The window's adapter onto the annotate tier in
//! `quantick_control_handlers::annotate`: which tabs and panes there are, the
//! look a fresh object opens with, and each pane's series and marks. What a
//! call parses, resolves, places, refuses and answers is decided there.

use quantick_control::wire::ActorKind;
use quantick_control_handlers::annotate::{
    AnnotateAccess, AnnotatePort, AnnotatedPane, PaneSide as AnnotatedSide,
};
use quantick_control_host::journal::NewEvent;

use super::gateway::ControlAccess;
use crate::{
    app::control_host::ControlPort,
    drawings::{self, Drawings},
    pane::PaneSide,
    tab::Tab,
};

impl AnnotatePort for dyn ControlPort {
    fn tab_count(&self) -> usize {
        self.tab_reads().tabs().len()
    }
    fn drawing_side(&self, index: usize) -> AnnotatedSide {
        annotated_side(self.tab_reads().tabs()[index].drawing_side())
    }
    fn time_pane_count(&self, index: usize) -> usize {
        self.tab_reads().tabs()[index].time_panes.len()
    }
    fn sides(&self, index: usize) -> Vec<AnnotatedSide> {
        self.tab_reads().tabs()[index]
            .sides()
            .map(annotated_side)
            .collect()
    }
    fn new_drawing(&self, tool: drawings::DrawingTool) -> drawings::NewDrawing {
        self.tab_reads().new_drawing(tool)
    }
    fn pane_mut(&mut self, index: usize, side: AnnotatedSide) -> &mut dyn AnnotatedPane {
        let tab = self
            .tabs_mut()
            .tab_at_mut(index)
            .expect("annotate addresses a live tab");
        tab.annotated = pane_side(side);
        tab
    }
}

/// The tab, aimed at the pane `Tab::annotated` names: a pane reads its slots
/// through the series its tab session keeps for it, so the tab is what an
/// annotation writes through.
impl AnnotatedPane for Tab {
    fn pane_id(&self) -> u64 {
        self.pane(self.annotated).id
    }
    fn layout_id(&self) -> Option<u64> {
        self.pane(self.annotated).layout_id().map(|id| id.0)
    }
    fn pagination_revision(&self) -> u64 {
        self.pane(self.annotated).pagination_revision()
    }
    fn slots(&self) -> usize {
        self.pane(self.annotated).slots()
    }
    fn slot_at_time(&self, time_ms: i64) -> Option<usize> {
        self.pane(self.annotated).slot_at_time(time_ms)
    }
    fn slot_open_time(&self, slot: usize) -> Option<i64> {
        self.pane(self.annotated).slot_open_time(slot)
    }
    fn anchor_time(&self, bar: f32) -> Option<i64> {
        self.pane(self.annotated).series_read().anchor_time(bar)
    }
    fn drawings(&self) -> &Drawings {
        &self.pane(self.annotated).pane.drawings
    }
    fn drawings_mut(&mut self) -> &mut Drawings {
        &mut self.pane_mut(self.annotated).pane.drawings
    }
    /// The tab sweeps what rode a removed drawing on this same frame
    /// ([`crate::tab::Tab::apply_strategy_cleanup`]); a pane holds none.
    fn sweep_orphans(&mut self) {}
    fn band_refusal(&self, band: &drawings::DrawingBand) -> Option<&'static str> {
        self.pane(self.annotated).band_refusal(band)
    }
}

impl AnnotateAccess for ControlAccess {
    fn recorded_actor(&self) -> Option<(ActorKind, &str)> {
        self.recorded_author()
            .map(|recorded| (recorded.actor_kind, recorded.client_name.as_str()))
    }
    fn record_event(&mut self, event: NewEvent) {
        self.append_event(event);
    }
}

pub(crate) fn annotated_side(side: PaneSide) -> AnnotatedSide {
    match side {
        PaneSide::Flow => AnnotatedSide::Flow,
        PaneSide::Time(slot) => AnnotatedSide::Time(slot),
    }
}

fn pane_side(side: AnnotatedSide) -> PaneSide {
    match side {
        AnnotatedSide::Flow => PaneSide::Flow,
        AnnotatedSide::Time(slot) => PaneSide::Time(slot),
    }
}
