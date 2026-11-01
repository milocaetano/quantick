//! The frame emitter's seat on the window: the window read as the
//! `quantick_control_handlers::semantic` source, and the frame hook that
//! hands it the gateway's journal and clock.
//!
//! What a change *is* and how it is journaled lives in the handler crate;
//! this file only reads the tabs and panes into that vocabulary, allocating
//! nothing on a quiet frame.

use quantick_control_handlers::analysis::AnalysisPane;
pub(super) use quantick_control_handlers::semantic::SemanticBaseline;
use quantick_control_handlers::semantic::{SelectionIdentity, SemanticSource, SemanticTab};
use quantick_control_schema::interaction::SelectionSnapshot;

use crate::app::ControlWindow;
use crate::metrics;
use crate::tab::Tab;

use super::super::feed::connection_state;
use super::super::interaction::{selection_identity, selection_snapshot};
use super::super::{PaneSideDto, pane_side_dto};
use super::ControlAccess;

/// One open tab with its id, lent for the walk.
struct TabRead<'a> {
    id: u64,
    tab: &'a Tab,
}

impl SemanticTab for TabRead<'_> {
    fn tab_id(&self) -> u64 {
        self.id
    }
    fn feed_id(&self) -> &str {
        &self.tab.active.0
    }
    fn symbol(&self) -> &str {
        &self.tab.active.1
    }
    fn connection(&self) -> &'static str {
        connection_state(self.tab.feed_connection)
    }
    fn replay(&self) -> Option<(bool, bool)> {
        self.tab
            .replay
            .as_ref()
            .map(|link| (link.status.is_playing(), link.status.is_finished()))
    }
    /// [`Tab::panes`] and not a local `Vec`: walked on every frame, in the
    /// order both analysis scopes publish.
    fn visit_panes(&self, visit: &mut dyn FnMut(PaneSideDto, &dyn AnalysisPane)) {
        for (pane, side) in self.tab.panes() {
            visit(pane_side_dto(side), pane);
        }
    }
}

impl SemanticSource for ControlWindow {
    fn active_tab_id(&self) -> u64 {
        self.tab_reads().tabs().active_id()
    }
    fn focused_pane_id(&self) -> u64 {
        let tabs = self.tab_reads().tabs();
        let active = &tabs[self
            .tab_reads()
            .active_tab_index()
            .min(tabs.len().saturating_sub(1))];
        active.pane(active.focused_side()).id
    }
    fn selection(&self) -> SelectionIdentity {
        selection_identity(self)
    }
    fn selection_snapshot(&self) -> SelectionSnapshot {
        selection_snapshot(self)
    }
    fn tab_count(&self) -> usize {
        self.tab_reads().tabs().len()
    }
    fn visit_tabs(&self, visit: &mut dyn FnMut(&dyn SemanticTab)) {
        for (id, tab) in self.tab_reads().tabs().iter_with_ids() {
            visit(&TabRead { id, tab });
        }
    }
}

impl ControlAccess {
    /// Record the semantic changes since the last frame; with access disabled
    /// nothing runs at all, the journal starts when the human opens the door
    /// and records changes, not the state it found.
    pub(super) fn emit_semantic_changes(&mut self, app: &ControlWindow) {
        let Some(mut baseline) = self.semantic_baseline.take() else {
            self.semantic_baseline = Some(SemanticBaseline::capture(app));
            return;
        };
        baseline.emit(app, &mut self.journal, metrics::wall_clock_ms());
        self.semantic_baseline = Some(baseline);
    }
}
