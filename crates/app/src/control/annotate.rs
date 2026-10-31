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
    pane::{ChartPane, PaneSide},
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
        self.tabs_mut().pane_mut(index, pane_side(side))
    }
}

impl AnnotatedPane for ChartPane {
    fn pane_id(&self) -> u64 {
        self.id
    }
    fn layout_id(&self) -> Option<u64> {
        ChartPane::layout_id(self).map(|id| id.0)
    }
    fn pagination_revision(&self) -> u64 {
        ChartPane::pagination_revision(self)
    }
    fn slots(&self) -> usize {
        ChartPane::slots(self)
    }
    fn slot_at_time(&self, time_ms: i64) -> Option<usize> {
        ChartPane::slot_at_time(self, time_ms)
    }
    fn slot_open_time(&self, slot: usize) -> Option<i64> {
        ChartPane::slot_open_time(self, slot)
    }
    fn anchor_time(&self, bar: f32) -> Option<i64> {
        self.series_read().anchor_time(bar)
    }
    fn drawings(&self) -> &Drawings {
        &self.drawings
    }
    fn drawings_mut(&mut self) -> &mut Drawings {
        &mut self.drawings
    }
    fn sweep_orphans(&mut self) {
        self.strategies.sweep_orphans(&self.drawings);
    }
    fn band_refusal(&self, band: &drawings::DrawingBand) -> Option<&'static str> {
        ChartPane::band_refusal(self, band)
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
