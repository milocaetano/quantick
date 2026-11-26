//! The Strategies tab: what the trader sets on an armed script from its
//! rail row.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{Mapping, Source, UiBehaviour};

/// The script-order controls behind a script row's action chip; one call
/// sets all of them.
pub const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "strategy.script.action.set",
        title: "Switch an armed script between ringing an alarm and placing paper orders",
        reach: "a script row's action chip in the Strategies tab: its order switch and Apply",
        keys: &[(
            Source::Authored,
            "the Strategies tab action chip order switch",
        )],
        mapping: capability!("strategy.script.action.set"),
    },
    UiBehaviour {
        id: "strategy.script.order.protection",
        title: "Set how far a script order's stop and gain sit from its entry",
        reach: "a script row's action chip in the Strategies tab: its stop and gain fields",
        keys: &[(
            Source::Authored,
            "the Strategies tab action chip stop and gain fields",
        )],
        mapping: capability!("strategy.script.action.set"),
    },
    UiBehaviour {
        id: "strategy.script.order.quantity",
        title: "Size a script's orders: a fixed contract count or the ticket's quantity",
        reach: "a script row's action chip in the Strategies tab: its ticket switch and quantity",
        keys: &[(
            Source::Authored,
            "the Strategies tab action chip quantity field",
        )],
        mapping: capability!("strategy.script.action.set"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_order_controls_map_to_named_calls() {
        assert!(!ROWS.is_empty(), "the action chip declares its controls");
        for control in ROWS {
            assert!(control.id.starts_with("strategy.script."), "{}", control.id);
            let Mapping::Capabilities(calls) = control.mapping else {
                panic!("{} is excluded or pending", control.id);
            };
            assert_eq!(calls, ["strategy.script.action.set"], "{}", control.id);
        }
    }
}
