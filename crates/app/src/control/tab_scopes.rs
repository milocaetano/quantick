//! The window's part in the session, analysis, mark, paper and chart-context
//! calls: it lends its open tabs, the config, the rail's magnet and the
//! workspace's paper owner; each tab answers the read scopes itself.
use quantick_control_handlers::OpenTabs;
use quantick_control_handlers::analysis::{AnalysisPaneEntry, AnalysisTab};
use quantick_control_handlers::attention::MarkTab;
use quantick_control_handlers::session::{ReplayRead, SessionTab};
use quantick_control_handlers::trade::{PaperHost, PaperOwner, TradeDesk};
use quantick_control_schema::interaction::CursorSnapshot;

use crate::app::control_host::ControlPort;
use crate::config::AppConfig;
use crate::tab::Tab;
use crate::toolrail::ToolRailExt as _;

impl OpenTabs for dyn ControlPort {
    type Tab = Tab;
    fn tab_count(&self) -> usize {
        self.tab_reads().tabs().len()
    }
    fn open_tab(&self, index: usize) -> Option<&Tab> {
        self.tab_reads().tab_at(index)
    }
    fn config(&self) -> &AppConfig {
        self.tab_reads().config()
    }
    fn rail_magnet(&self) -> bool {
        self.chrome_reads().tool_rail().magnet()
    }
}

impl PaperHost for dyn ControlPort {
    fn paper(&mut self) -> Box<dyn PaperOwner + '_> {
        Box::new(self.workspace_calls())
    }
}

impl SessionTab for Tab {
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn desk(&self) -> &dyn TradeDesk {
        &self.paper
    }
    fn replay(&self) -> Option<Box<dyn ReplayRead + '_>> {
        let link = self.link.replay.as_ref()?;
        Some(Box::new(ReplayLinkRead(link)))
    }
}

impl AnalysisTab for Tab {
    fn analysis_panes(&self) -> Vec<AnalysisPaneEntry<'_>> {
        self.panes()
            .map(|(pane, side)| AnalysisPaneEntry {
                side: super::pane_side_dto(side),
                pane,
            })
            .collect()
    }
}

impl MarkTab for Tab {
    fn cursor_snapshot(&self, tab_id: u64, config: &AppConfig) -> CursorSnapshot {
        super::interaction::cursor_snapshot(self, tab_id, config)
    }
}

impl quantick_control_handlers::chart_context::ArmedTab for Tab {
    fn armed_on(
        &self,
        pane_id: u64,
    ) -> Vec<quantick_session::strategy_reading::ArmedReading<crate::drawings::DrawingId>> {
        self.market.strategies.readings(pane_id)
    }
}

/// One linked recording, lent for the read.
struct ReplayLinkRead<'a>(&'a quantick_feed::replay::ReplayLink);

impl ReplayRead for ReplayLinkRead<'_> {
    fn session(&self) -> &quantick_replay::Session {
        &self.0.session
    }
    fn playing(&self) -> bool {
        self.0.status.is_playing()
    }
    fn finished(&self) -> bool {
        self.0.status.is_finished()
    }
    fn position_ms(&self) -> i64 {
        self.0.status.position_ms()
    }
    fn start_ms(&self) -> i64 {
        self.0.status.start_ms()
    }
    fn end_ms(&self) -> i64 {
        self.0.status.end_ms()
    }
    fn elapsed_ms(&self) -> i64 {
        self.0.status.elapsed_ms()
    }
    fn progress(&self) -> f32 {
        self.0.status.progress()
    }
    fn speed(&self) -> f32 {
        self.0.status.speed()
    }
    fn rewinds(&self) -> u64 {
        self.0.status.rewinds()
    }
    fn day_prints(&self) -> (usize, usize) {
        self.0.day_prints()
    }
    fn trace_file_name(&self) -> String {
        quantick_control_schema::session::file_name(&super::trace::ReplayTraceFile::path_for(
            &self.0.session.path,
        ))
    }
}
