//! Folding the tape lane against an edge of its pane: the state a divider
//! dragged to that edge leaves, and a click on its rail undoes.

use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_orderflow::LaneFold;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bubble_scale::precondition;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const TAPE_FOLD_CAPABILITY_ID: &str = "orderflow.tape.fold.set";

/// The tape shares the pane with the candles: switched on and not shown
/// alone. Folding either side needs it; opening never does.
pub const BESIDE_CANDLES_PRECONDITION: &str = "orderflow.tape.beside_candles";

/// Which side of the pane is folded to a rail.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TapeFold {
    /// Candles and tape side by side at the tape's width share.
    #[default]
    Open,
    /// The candles fold to a rail on the left; the tape takes the pane.
    Candles,
    /// The tape folds to a rail on the right; the candles take the pane.
    Tape,
}

impl From<TapeFold> for LaneFold {
    fn from(fold: TapeFold) -> Self {
        match fold {
            TapeFold::Open => Self::Open,
            TapeFold::Candles => Self::Candles,
            TapeFold::Tape => Self::Tape,
        }
    }
}

impl From<LaneFold> for TapeFold {
    fn from(fold: LaneFold) -> Self {
        match fold {
            LaneFold::Open => Self::Open,
            LaneFold::Candles => Self::Candles,
            LaneFold::Tape => Self::Tape,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TapeFoldInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// `open` unfolds at the width the tape had before it was folded.
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

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<TapeFoldInput, TapeFoldResult>(
        TAPE_FOLD_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Fold the tape or the candles",
        "Folds one side of a pane that shows the tape beside its candles to a thin rail, as dragging the divider to that edge does: candles leaves the tape the whole pane, tape leaves the candles it. Open unfolds at the width share the tape had before the fold, which folding never changes. Presentation only: executions, grouping and the tape's own switches (tape on, tape only) are untouched, and tape only wins over a fold while it is on. One of the asset's bubble settings (orderflow.bubbles names it), saved for it while its save switch is on.",
        "Stable tab and pane IDs select the pane; repeating a fold has no further effect, and the result and orderflow.bubbles report the fold kept.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor.preconditions = vec![precondition(
        BESIDE_CANDLES_PRECONDITION,
        "Folding a side needs the tape switched on beside the candles, not off and not tape only.",
    )];
    descriptor
}

pub const READBACKS: &[Readback] = &[snapshot(
    TAPE_FOLD_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].bubbles.tape_fold",
    "the addressed pane reports which side of its tape lane is folded",
    &[
        "the_tape_fold_is_named_retry_safe_refused_without_a_lane_and_read_back",
        EVERY_OPTIONAL_TEST,
    ],
)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_fold_is_the_lane_fold_and_refuses_anything_else() {
        for fold in [TapeFold::Open, TapeFold::Candles, TapeFold::Tape] {
            assert_eq!(TapeFold::from(LaneFold::from(fold)), fold);
        }
        let input: TapeFoldInput =
            serde_json::from_str(r#"{"tab_id":"1","pane_id":"0","fold":"candles"}"#).unwrap();
        assert_eq!(input.fold, TapeFold::Candles);
        for bad in [
            r#"{"tab_id":"1","pane_id":"0","fold":"left"}"#,
            r#"{"tab_id":"1","pane_id":"0"}"#,
            r#"{"tab_id":"1","pane_id":"0","fold":"open","width":1}"#,
        ] {
            assert!(serde_json::from_str::<TapeFoldInput>(bad).is_err(), "{bad}");
        }
    }
}
