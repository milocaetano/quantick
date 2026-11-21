//! A fake window and a fake access: every port a family names, over plain
//! fields a test sets and reads back. The second implementation of each port,
//! after the application's own adapter.
//! Test-only even when inspected independently of the parent module.
#![cfg(test)]

use std::time::Duration;

use quantick_chart::state::ChartState;
use quantick_chart::style::ChartStyle;
use quantick_civil::TzOffset;
use quantick_control::wire::{ActorContext, ActorKind, CanonicalDecimal, WireU64};
use quantick_control_host::{
    journal::NewEvent,
    wire::{PaneSideDto, unavailable},
};
use quantick_control_schema::chart::{BarProvenanceContext, ViewportSnapshot};
use quantick_control_schema::interaction::CursorSnapshot;
use quantick_control_schema::notify::AgentPopup;
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration};
use quantick_engine::{Bar, BarSpec, Trade};
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError,
};
use quantick_paper::{PaperAccount, format::PositionSummary, risk_sizing::RiskState};
use quantick_session::paper::{SharedChange, TabPaper, TradeCommand, TradeOutcome};
use quantick_session::{HistoryAct, HistoryOutcome, SessionSnapshot};
use quantick_sim::Order;
use quantick_sources::history_reach::{HistoryReach, ReachProgress};
use quantick_sources::history_run::{Cancelled, Press, RunStatus};
use quantick_sources::recovery::Recovery;
use rust_decimal::Decimal;

use crate::JournalAccess;
use crate::addressed_pane::AddressedPanePort;
use crate::analysis::{AnalysisPaneEntry, AnalysisTab};
use crate::annotate::AnnotateAccess;
use crate::attention::MarkTab;
use crate::axis_pane::{AxisPane, NativeTape};
use crate::bubble_pane::{AssetSave, BubbleLook, BubblePane, PaneBubblesPort, TickSpherePane};
use crate::chart::{ChartPaneEntry, ChartPaneSource, ChartPort, MissingPane, PaneFraming};
use crate::chart_layer::{LayerBlocked, LayerRead};
use crate::chart_settings::ChartStylePort;
use crate::contract_roll::AutoRoll;
use crate::deal_recorder::{DealRecorder, DealRecordingPort};
use crate::drawing_magnet::RailMagnetPort;
use crate::indicator_host::{
    AttachedScript, IndicatorControlHost, IndicatorGuidePort, IndicatorOwner, IndicatorPickerPort,
    IndicatorRefusal, NativePort, ScriptEditorPort, ScriptsPort,
};
use crate::market_tab::FeedSelection;
use crate::market_tab::RecoveredFeed;
use crate::market_tab::{LentTab, MarketTab, MarketTabHost};
use crate::notify::{Attention, AttentionHost, NotifyAccess};
use crate::pane_marks::PaneMarks;
use crate::settings::{SettingsAct, SettingsOwner, SettingsReadings, SettleError};
use crate::tabs::{OpenTabs, TabDirectory};
use crate::trade::{PaperHost, PaperOwner, TradeDesk};
use quantick_control::error::ControlError;
use quantick_control_schema::bubble_preset::{ListInput, ListResult};
use quantick_control_schema::chart_settings::{ChartSettingsInput, ChartSettingsResult};
use quantick_control_schema::indicator_picker::{PickerInput, PickerResult};
use quantick_control_schema::layers::LayerSnapshot;
use quantick_control_schema::native_indicator as wire;
use quantick_control_schema::orderflow::BubblesStateSnapshot;
use quantick_control_schema::script_editor::{
    EditorResult, NumericValue, ScriptDocument, ScriptEditorAction,
};
use quantick_control_schema::tape_view::TapeViewSnapshot;
use quantick_drawings::{DrawingTool, Drawings};
use quantick_orderflow::LaneWindow;
use quantick_orderflow::tape_view::TapeEnd;
use quantick_orderflow::{HeatmapConfig, TickTuning};
use quantick_replay::recorder::{DealRecordingError, RecordingView};
use quantick_stores::bubble_asset_store::SaveSwitch;
use quantick_stores::candle_presets::PresetError;
use quantick_stores::config::AppConfig;
use quantick_stores::indicator_state::SavedIndicator;

mod access;
mod pane;
mod paper;
mod tab;
mod window;

pub(crate) use access::*;
pub(crate) use pane::*;
pub(crate) use paper::*;
pub(crate) use tab::*;
pub(crate) use window::*;
