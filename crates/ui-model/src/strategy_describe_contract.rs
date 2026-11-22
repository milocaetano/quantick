//! Secret-free conversation and preview contracts.
use quantick_chart_interaction::drawing_model::DrawingId;
pub use quantick_session::script_draft::{Diagnostic, HistoricalClose, Marker};
use quantick_session::script_strategies::ScriptPlacement;
use std::sync::Arc;

pub const MAX_BARS: usize = 512;
pub const MAX_CONTEXT_ITEMS: usize = 32;
pub const MAX_TRANSCRIPT_BYTES: usize = 8192;
pub const MAX_MARKERS: usize = 128;
pub const MAX_TURNS: usize = 16;
pub use quantick_control_schema::strategy_describe::MAX_TEXT_BYTES;
pub const MAX_SOURCE_BYTES: usize = 32768;
pub const MAX_REPAIRS: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub workspace: quantick_workspace::lifetime::WorkspaceId,
    pub tab: u64,
    pub pane: u64,
    pub feed: String,
    pub symbol: String,
    pub bars: String,
    pub placement: ScriptPlacement<DrawingId>,
    pub indicators: Vec<String>,
    pub drawings: Vec<String>,
    pub availability: Vec<String>,
    pub history: Vec<HistoricalClose>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub source: String,
    pub summary: String,
    pub markers: Vec<Marker>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Turn {
    Trader(String),
    Assistant(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub revision: u64,
    pub context: Arc<Context>,
    pub transcript: Vec<Turn>,
    pub repair: Option<(String, Vec<Diagnostic>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Question(String),
    Candidate { source: String, summary: String },
}

pub use wire::Phase;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub revision: u64,
    pub phase: Phase,
    pub transcript: Vec<Turn>,
    pub message: String,
    pub draft: Option<Draft>,
    pub saved_id: Option<String>,
}

pub use quantick_control_schema::strategy_describe::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Command(Command),
    Opened(Arc<Context>),
    Replied {
        revision: u64,
        reply: Result<Reply, String>,
    },
    Previewed {
        revision: u64,
        result: Result<Draft, Vec<Diagnostic>>,
    },
    Saved {
        revision: u64,
        result: Result<String, String>,
    },
    /// The chart identity or conversation changed. New closes on the same chart retain its target.
    TargetChanged,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Effects {
    #[default]
    None,
    Dispatch(Command),
    Request(Request),
    Preview {
        revision: u64,
        /// Preview the loaded range captured when this conversation opened.
        context: Arc<Context>,
        source: String,
        summary: String,
    },
    Save {
        revision: u64,
        id: String,
    },
}

use quantick_control_schema::strategy_describe as wire;

impl From<Status> for wire::Status {
    fn from(state: Status) -> Self {
        let (summary, source, markers) = state.draft.map_or_else(Default::default, |draft| {
            let markers = draft
                .markers
                .into_iter()
                .map(|marker| wire::Marker {
                    slot: marker.slot,
                    close_time: marker.close_time,
                    side: match marker.side {
                        quantick_engine::Side::Buy => {
                            quantick_control_schema::trade::ActionSide::Buy
                        }
                        quantick_engine::Side::Sell => {
                            quantick_control_schema::trade::ActionSide::Sell
                        }
                    },
                })
                .collect();
            (Some(draft.summary), Some(draft.source), markers)
        });
        wire::Status {
            phase: state.phase,
            revision: quantick_control::wire::WireU64::new(state.revision),
            message: state.message,
            transcript: state
                .transcript
                .into_iter()
                .map(|turn| {
                    let (author, text) = match turn {
                        Turn::Trader(s) => ("trader", s),
                        Turn::Assistant(s) => ("assistant", s),
                    };
                    wire::Turn {
                        author: author.into(),
                        text,
                    }
                })
                .collect(),
            summary,
            source,
            markers,
            saved_id: state.saved_id,
        }
    }
}
