//! What the toolbar's TRADE group says this frame, decided in one read.
//!
//! The faces, hovers, the close label and the reason the group is locked
//! all hang on the same size and the same risk read. Taking that read once
//! here keeps the toolbar from asking the risk sizer once per string, and
//! keeps the entries and the reverse locked by the one rule.

use quantick_engine::Side;

use super::Desk;
use super::ticket::{entry_face, entry_label};
use crate::PaperAccount;

/// Why the entries are off before the simulator has seen a price.
pub const NO_MARKET_YET: &str = "waiting for the first print - there is no market yet";

/// One entry button's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryTexts {
    /// The fixed-width face: side and size only (`SELL 5`).
    pub face: String,
    /// The state-aware label (`SELL 5 (reverses to short 4)`), for surfaces
    /// with room for it.
    pub label: String,
    /// The hover: the label where it says more than the face, then the
    /// quantity and bracket details.
    pub hover: String,
}

/// The TRADE group's words for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeBar {
    /// Why the entries and the reverse are off, when they are: no market
    /// yet, or the risk lock's own sentence. `None` enables them.
    pub blocked: Option<String>,
    /// The buy button.
    pub buy: EntryTexts,
    /// The sell button.
    pub sell: EntryTexts,
    /// `Close 1 LONG` while a position is open; `None` removes the close
    /// and the reverse buttons.
    pub close: Option<String>,
    /// What the reverse button does, ending in its shortcut.
    pub reverse_hover: String,
}

impl Desk {
    /// The TRADE group's words, from one risk read. `reverse_key` is the
    /// reverse shortcut as the host spells it (`Shift+R`), named once where
    /// the key is bound.
    #[must_use]
    pub fn trade_bar(&self, account: &PaperAccount, reverse_key: &str) -> TradeBar {
        let (state, locked) = self.risk_report(account);
        let quantity = state
            .derived_quantity()
            .or_else(|| self.ticket.quantity_preview());
        let blocked = if !account.ready() {
            Some(NO_MARKET_YET.to_owned())
        } else if locked {
            Some(format!("the risk lock is on - {}", state.sentence()))
        } else {
            None
        };
        let position = account.venue().position();
        let texts = |side: Side| {
            let face = entry_face(side, quantity);
            let label = entry_label(side, quantity, position);
            let details = self.ticket.entry_hover(side);
            let hover = if label == face {
                details
            } else {
                format!("{label}\n{details}")
            };
            EntryTexts { face, label, hover }
        };
        TradeBar {
            blocked,
            buy: texts(Side::Buy),
            sell: texts(Side::Sell),
            close: account.close_button_label(),
            reverse_hover: format!(
                "reverse the open simulated position: one market order closes it and opens the \
                 same size the other way at the next print · {reverse_key}"
            ),
        }
    }
}
