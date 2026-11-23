//! Inputs shared by a historical preview and its explicit alarm publication.
use quantick_engine::Side;
use quantick_script_trigger::{ScriptInstance, ScriptPlacement};
use quantick_strategy::Region;

use crate::strategies::LiveClose;

/// Oldest first. A region is resolved at this slot, including its time span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalClose {
    pub close: LiveClose,
    pub region: Option<(Region, bool)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub slot: usize,
    pub close_time: i64,
    pub side: Side,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: Option<usize>,
    pub code: String,
    pub message: String,
}

/// Warming and admission finish before the store is touched.
pub struct PreparedAlarm<D>(pub(super) Vec<ScriptInstance<D>>);

pub struct DraftInput<'a, D> {
    pub id: &'a str,
    pub source: &'a str,
    pub placement: ScriptPlacement<D>,
    pub history: &'a [HistoricalClose],
}
