//! Which of its three views the flow pane shows, and the older fold call that
//! now names the same views by the side it hides.

use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_orderflow::LaneView;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const TAPE_VIEW_CAPABILITY_ID: &str = "orderflow.tape.view.set";
pub const TAPE_FOLD_CAPABILITY_ID: &str = "orderflow.tape.fold.set";

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

/// The fold call's words: names the side hidden, so `candles` is a pane of tape.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TapeFold {
    /// Nothing hidden: candles and tape side by side.
    #[default]
    Open,
    /// The candles hidden: the pane shows the tape alone.
    Candles,
    /// The tape hidden: the pane shows the candles alone.
    Tape,
}

impl From<TapeFold> for TapeView {
    fn from(fold: TapeFold) -> Self {
        match fold {
            TapeFold::Open => Self::Beside,
            TapeFold::Candles => Self::Tape,
            TapeFold::Tape => Self::Candles,
        }
    }
}

impl From<TapeView> for TapeFold {
    fn from(view: TapeView) -> Self {
        match view {
            TapeView::Beside => Self::Open,
            TapeView::Tape => Self::Candles,
            TapeView::Candles => Self::Tape,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TapeViewInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub view: TapeView,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeViewResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub view: TapeView,
    pub changed: bool,
}

impl TapeViewInput {
    #[must_use]
    pub fn result(self, changed: bool) -> TapeViewResult {
        TapeViewResult {
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            view: self.view,
            changed,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TapeFoldInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// `open` shows both sides at the width the tape had.
    pub fold: TapeFold,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeFoldResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub fold: TapeFold,
    pub changed: bool,
}

impl TapeFoldInput {
    #[must_use]
    pub fn result(self, changed: bool) -> TapeFoldResult {
        TapeFoldResult {
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            fold: self.fold,
            changed,
        }
    }
}

pub fn view_descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<TapeViewInput, TapeViewResult>(
        TAPE_VIEW_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Show the candles, the tape, or both",
        "Sets what a flow pane shows, as its view button, Ctrl+Shift+T or letting go of the divider at an edge do: beside shows candles and tape at the tape's width share, candles shows the candles alone, tape shows the tape alone. Allowed from any view. Presentation only: executions, grouping and the width share are untouched, and the layer switches keep their state.",
        "Stable tab and pane IDs select the pane; repeating a view has no further effect, and the result and orderflow.bubbles report the view on screen.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<TapeFoldInput, TapeFoldResult>(
        TAPE_FOLD_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Hide the tape or the candles",
        "The older name of orderflow.tape.view.set, naming the side hidden rather than the side shown: open is view beside, candles is view tape, tape is view candles. Allowed from any view; prefer orderflow.tape.view.set.",
        "Stable tab and pane IDs select the pane; repeating a fold has no further effect, and the result and orderflow.bubbles report the side hidden.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[Readback] = &[
    snapshot(
        TAPE_VIEW_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        crate::orderflow::BUBBLES_SCOPE_ID,
        "tabs[].panes[].bubbles.tape_view",
        "the addressed pane reports which of its three views it shows",
        &[
            "the_tape_view_is_named_retry_safe_refused_without_a_lane_and_read_back",
            EVERY_OPTIONAL_TEST,
        ],
    ),
    snapshot(
        TAPE_FOLD_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        crate::orderflow::BUBBLES_SCOPE_ID,
        "tabs[].panes[].bubbles.tape_fold",
        "the addressed pane reports which side of its view is hidden",
        &[
            "the_tape_fold_is_named_retry_safe_refused_without_a_lane_and_read_back",
            EVERY_OPTIONAL_TEST,
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A fold names the side hidden and a view the side shown, so the two
    /// words `candles` and `tape` cross over and `open` is `beside`.
    #[test]
    fn a_fold_names_the_side_hidden_and_a_view_the_side_shown() {
        for (fold, view, lane) in [
            (TapeFold::Open, TapeView::Beside, LaneView::Beside),
            (TapeFold::Candles, TapeView::Tape, LaneView::TapeOnly),
            (TapeFold::Tape, TapeView::Candles, LaneView::CandlesOnly),
        ] {
            assert_eq!(TapeView::from(fold), view);
            assert_eq!(TapeFold::from(view), fold);
            assert_eq!(LaneView::from(view), lane);
            assert_eq!(TapeView::from(lane), view);
        }
    }

    #[test]
    fn the_wire_words_are_the_documented_ones_and_nothing_else() {
        let input: TapeFoldInput =
            serde_json::from_str(r#"{"tab_id":"1","pane_id":"0","fold":"candles"}"#).unwrap();
        assert_eq!(input.fold, TapeFold::Candles);
        for (word, view) in [
            ("beside", TapeView::Beside),
            ("candles", TapeView::Candles),
            ("tape", TapeView::Tape),
        ] {
            let text = format!(r#"{{"tab_id":"1","pane_id":"0","view":"{word}"}}"#);
            let input: TapeViewInput = serde_json::from_str(&text).unwrap();
            assert_eq!(input.view, view);
        }
        for bad in [
            r#"{"tab_id":"1","pane_id":"0","fold":"left"}"#,
            r#"{"tab_id":"1","pane_id":"0"}"#,
            r#"{"tab_id":"1","pane_id":"0","fold":"open","width":1}"#,
        ] {
            assert!(serde_json::from_str::<TapeFoldInput>(bad).is_err(), "{bad}");
        }
        for bad in [
            r#"{"tab_id":"1","pane_id":"0","view":"open"}"#,
            r#"{"tab_id":"1","pane_id":"0"}"#,
            r#"{"tab_id":"1","pane_id":"0","view":"tape","width":1}"#,
        ] {
            assert!(serde_json::from_str::<TapeViewInput>(bad).is_err(), "{bad}");
        }
    }
}
