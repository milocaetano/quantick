//! The flow pane's view by the side it closes: the view a divider dragged to that
//! edge leaves, and a click on its rail undoes. An alias over
//! [`crate::tape_view_set`], with no state of its own.

use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use quantick_orderflow::LaneFold;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const TAPE_FOLD_CAPABILITY_ID: &str = "orderflow.tape.fold.set";

/// Which side of the pane is closed to a rail.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TapeFold {
    /// Candles and tape side by side: the view `beside`.
    #[default]
    Open,
    /// The candles closed to a rail on the left: the view `tape`, tape only.
    Candles,
    /// The tape closed to a rail on the right: the view `candles`, the tape off.
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
    /// `open` shows both sides at the default width share, from any view.
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
        "Close the tape or the candles",
        "Sets a flow pane's view by the side it closes, as dragging the divider to that edge does, an alias over orderflow.tape.view.set: candles closes the candles, which is tape only, tape closes the tape, which is the candles alone, open shows both at the default width share. Allowed from any view; a side keeps a thin rail at its edge either way. Open also stands down the market-closed hold, as a click on the rail does. Presentation only: executions are untouched.",
        "Stable tab and pane IDs select the pane; repeating a word has no further effect, and the result and orderflow.bubbles report the side closed.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[Readback] = &[snapshot(
    TAPE_FOLD_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].bubbles.tape_fold",
    "the addressed pane reports which side its view closes",
    &[
        "the_tape_fold_is_an_alias_over_the_view_retry_safe_and_read_back",
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
