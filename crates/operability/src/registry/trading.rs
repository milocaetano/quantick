//! The paper ticket: what a trader does to an order or a position.
//!
//! Read [`super`] first: it says why the rows are written, how they are
//! joined, and what the drift guard does with them.

use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// Every row this family owns.
pub(super) const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "trade.aim.bracket",
        title: "Send an entry with its stop and target attached",
        reach: "the ticket's bracket fields, and the aim on the chart",
        keys: &[(
            Source::Authored,
            "the ticket bracket fields and the aim on the chart",
        )],
        mapping: capability!("trade.order.bracket"),
    },
    UiBehaviour {
        id: "trade.instrument.money.set",
        title: "Declare what one point of an instrument is worth",
        reach: "the Trading panel's instrument section",
        keys: &[(Source::Authored, "the Trading panel instrument section")],
        mapping: capability!("trade.instrument.set_money"),
    },
    UiBehaviour {
        id: "trade.market.buy",
        title: "Buy at market, simulated",
        reach: "toolbar trade group, Trading panel, Shift+B",
        keys: &[
            (Source::ToolbarAction, "PaperBuy"),
            (Source::Hotkey, "PAPER_BUY_SHORTCUT"),
        ],
        mapping: capability!("trade.order.place"),
    },
    UiBehaviour {
        id: "trade.market.sell",
        title: "Sell at market, simulated",
        reach: "toolbar trade group, Trading panel, Shift+S",
        keys: &[
            (Source::ToolbarAction, "PaperSell"),
            (Source::Hotkey, "PAPER_SELL_SHORTCUT"),
        ],
        mapping: capability!("trade.order.place"),
    },
    UiBehaviour {
        id: "trade.order.place_at_price",
        title: "Rest an order at the price under the pointer",
        reach: "the canvas right-click menu's trade section",
        keys: &[(
            Source::Authored,
            "the canvas right-click menu trade section, resolved per click",
        )],
        mapping: capability!("trade.order.place"),
    },
    UiBehaviour {
        id: "trade.orders.cancel_all",
        title: "Cancel every working order without trading",
        reach: "Trading panel, Shift+X",
        keys: &[(Source::Hotkey, "PAPER_CANCEL_SHORTCUT")],
        mapping: capability!("trade.order.cancel"),
    },
    UiBehaviour {
        id: "trade.position.close",
        title: "Exit the open position at the next print",
        reach: "toolbar trade group, Trading panel",
        keys: &[(Source::ToolbarAction, "PaperClose")],
        mapping: capability!("trade.order.place"),
    },
    UiBehaviour {
        id: "trade.position.flatten",
        title: "Close the position and cancel every working order",
        reach: "Trading panel, Shift+F",
        keys: &[(Source::Hotkey, "PAPER_FLATTEN_SHORTCUT")],
        mapping: excluded!(
            PendingCapability,
            "`trade.order.place` and `trade.order.cancel` do the two halves, and a caller that \
             runs them in sequence is not flat between them. The one-shot has no capability. \
             Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "trade.position.reverse",
        title: "Reverse the simulated position",
        reach: "Trading panel, Shift+R",
        keys: &[(Source::Hotkey, "PAPER_REVERSE_SHORTCUT")],
        mapping: excluded!(
            PendingCapability,
            "no capability reverses; a caller would have to size the flip itself from a read \
             that may already be stale. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "trade.ticket.risk.set",
        title: "Say what one trade may lose",
        reach: "the Trading panel's risk section",
        keys: &[(Source::Authored, "the Trading panel risk field")],
        mapping: capability!("trade.risk.set"),
    },
    UiBehaviour {
        id: "trade.ticket.ruler.set",
        title: "Walk the projected stop and target out from the aim",
        reach: "the ruler wheel on the chart's aim",
        keys: &[(Source::Authored, "the ruler wheel on the chart aim")],
        mapping: capability!("trade.ruler.set"),
    },
    UiBehaviour {
        id: "trade.ticket.strategy.select",
        title: "Choose the ticket's exit ladder",
        reach: "the Trading panel's strategy selector",
        keys: &[(Source::Authored, "the Trading panel strategy selector")],
        mapping: capability!("trade.strategy.select"),
    },
];
