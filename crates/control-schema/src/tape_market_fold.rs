//! The asset's setting that folds the tape while the market is closed: the
//! checkbox in the Bubbles settings, by name.

use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const TAPE_MARKET_FOLD_CAPABILITY_ID: &str = "orderflow.tape.market_fold.set";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TapeMarketFoldInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// On, the tape folds by itself while the feed is quiet or offline.
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeMarketFoldResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub enabled: bool,
    pub changed: bool,
}

impl TapeMarketFoldInput {
    #[must_use]
    pub fn result(self, changed: bool) -> TapeMarketFoldResult {
        TapeMarketFoldResult {
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            enabled: self.enabled,
            changed,
        }
    }
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<
        TapeMarketFoldInput,
        TapeMarketFoldResult,
    >(
        TAPE_MARKET_FOLD_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Fold the tape while the market is closed",
        "Sets whether a pane showing the tape beside its candles folds the tape to its rail by itself while the feed is quiet or offline. The first print brings it back. Turning it off brings it back at once and leaves a retained tape up to a seam at the last print. A replay never folds. Presentation only. One of the asset's bubble settings (orderflow.bubbles names it), saved for it while its save switch is on.",
        "Stable tab and pane IDs select the pane; repeating a value has no further effect, and the result and orderflow.bubbles report the setting kept.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[Readback] = &[snapshot(
    TAPE_MARKET_FOLD_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].bubbles.fold_market_closed",
    "the addressed pane reports whether its tape folds while the market is closed",
    &[EVERY_OPTIONAL_TEST],
)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_input_names_its_pane_and_refuses_anything_else() {
        let input: TapeMarketFoldInput =
            serde_json::from_str(r#"{"tab_id":"1","pane_id":"0","enabled":false}"#).unwrap();
        assert!(!input.clone().result(true).enabled);
        for bad in [
            r#"{"tab_id":"1","pane_id":"0"}"#,
            r#"{"tab_id":"1","pane_id":"0","enabled":"no"}"#,
            r#"{"tab_id":"1","pane_id":"0","enabled":true,"view":"tape"}"#,
        ] {
            assert!(
                serde_json::from_str::<TapeMarketFoldInput>(bad).is_err(),
                "{bad}"
            );
        }
    }
}
