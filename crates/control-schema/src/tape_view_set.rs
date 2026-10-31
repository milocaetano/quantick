//! Which of its three views the flow pane shows. Not to be confused with
//! `chart.tape_view.set`, which moves the tape's end and window ([`crate::tape_view`]),
//! nor with a fold ([`crate::tape_fold`]), which this neither reads nor writes.

use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_orderflow::LaneView;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const TAPE_VIEW_SET_CAPABILITY_ID: &str = "orderflow.tape.view.set";

/// What the flow pane shows: names the side shown.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TapeView {
    /// Candles and tape side by side at the tape's width share.
    #[default]
    Beside,
    /// The candles alone; the tape is switched off.
    Candles,
    /// The tape alone; no candle is drawn.
    Tape,
}

impl From<TapeView> for LaneView {
    fn from(view: TapeView) -> Self {
        match view {
            TapeView::Beside => Self::Beside,
            TapeView::Candles => Self::CandlesOnly,
            TapeView::Tape => Self::TapeOnly,
        }
    }
}

impl From<LaneView> for TapeView {
    fn from(view: LaneView) -> Self {
        match view {
            LaneView::Beside => Self::Beside,
            LaneView::CandlesOnly => Self::Candles,
            LaneView::TapeOnly => Self::Tape,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TapeViewSetInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub view: TapeView,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeViewSetResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub view: TapeView,
    pub changed: bool,
}

impl TapeViewSetInput {
    #[must_use]
    pub fn result(self, changed: bool) -> TapeViewSetResult {
        TapeViewSetResult {
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            view: self.view,
            changed,
        }
    }
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<TapeViewSetInput, TapeViewSetResult>(
        TAPE_VIEW_SET_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Show the candles, the tape, or both",
        "Sets what a flow pane shows, as its view button and Ctrl+Shift+T do: beside shows candles and tape at the tape's width share, candles shows the candles alone, tape shows the tape alone. Allowed from any view. Presentation only: executions, grouping, the width share and a fold of the divider (orderflow.tape.fold.set) are untouched, and the layer switches keep their state.",
        "Stable tab and pane IDs select the pane; repeating a view has no further effect, and the result and orderflow.bubbles report the view kept.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[Readback] = &[snapshot(
    TAPE_VIEW_SET_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].bubbles.tape_view",
    "the addressed pane reports which of its three views it shows",
    &[
        "the_tape_view_is_named_retry_safe_refused_without_a_lane_and_read_back",
        EVERY_OPTIONAL_TEST,
    ],
)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_view_is_the_lane_view() {
        for (view, lane) in [
            (TapeView::Beside, LaneView::Beside),
            (TapeView::Candles, LaneView::CandlesOnly),
            (TapeView::Tape, LaneView::TapeOnly),
        ] {
            assert_eq!(LaneView::from(view), lane);
            assert_eq!(TapeView::from(lane), view);
        }
    }

    #[test]
    fn the_wire_words_are_the_documented_ones_and_nothing_else() {
        for (word, view) in [
            ("beside", TapeView::Beside),
            ("candles", TapeView::Candles),
            ("tape", TapeView::Tape),
        ] {
            let text = format!(r#"{{"tab_id":"1","pane_id":"0","view":"{word}"}}"#);
            let input: TapeViewSetInput = serde_json::from_str(&text).unwrap();
            assert_eq!(input.view, view);
        }
        for bad in [
            r#"{"tab_id":"1","pane_id":"0","view":"open"}"#,
            r#"{"tab_id":"1","pane_id":"0"}"#,
            r#"{"tab_id":"1","pane_id":"0","view":"tape","width":1}"#,
        ] {
            assert!(
                serde_json::from_str::<TapeViewSetInput>(bad).is_err(),
                "{bad}"
            );
        }
    }
}
