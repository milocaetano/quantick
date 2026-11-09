//! What the toolbar's TRADE group says this frame, decided in one read.
//!
//! The faces, hovers, the close label and the reason the group is locked
//! all hang on the same size and the same risk read. Taking that read once
//! here keeps the toolbar from asking the risk sizer once per string, and
//! keeps the entries and the reverse locked by the one rule.

use quantick_engine::Side;

use super::Desk;
use super::ticket::{ENTRY_FACE_MAX_CHARS, entry_face, entry_label};
use crate::PaperAccount;

// The toolbar paints these widths; they live here so the reserved width
// is decided beside the faces that must fit it.
/// One TRADE button's own horizontal padding, in pixels.
const TRADE_BUTTON_PAD_PX: f32 = 16.0;
/// Estimated width of one strong glyph in a TRADE button, in pixels: upper
/// case and digits, wider than the average label glyph.
const TRADE_GLYPH_PX: f32 = 8.0;
/// The one width both entry buttons wear, in every position state: room
/// for the longest face the paper desk ever writes
/// ([`ENTRY_FACE_MAX_CHARS`](super::ticket::ENTRY_FACE_MAX_CHARS)), so a press lands where it landed flat.
pub const TRADE_ENTRY_WIDTH_PX: f32 =
    TRADE_BUTTON_PAD_PX + ENTRY_FACE_MAX_CHARS as f32 * TRADE_GLYPH_PX;
/// Gap between two TRADE buttons, in pixels.
pub const TRADE_GAP_PX: f32 = 6.0;
/// Characters the close button shows before it cuts with `…`: room for
/// `× Close 999 SHORT`. The whole label is always in its hover.
const TRADE_CLOSE_FACE_CHARS: usize = 17;
/// The close button's fixed width, so the reverse beside it never moves on
/// a flip from LONG to SHORT or on a size change.
pub const TRADE_CLOSE_WIDTH_PX: f32 =
    TRADE_BUTTON_PAD_PX + TRADE_CLOSE_FACE_CHARS as f32 * TRADE_GLYPH_PX;
/// The reverse button's face.
pub const TRADE_REVERSE_LABEL: &str = "Reverse";
/// The reverse button's fixed width.
pub const TRADE_REVERSE_WIDTH_PX: f32 =
    TRADE_BUTTON_PAD_PX + TRADE_REVERSE_LABEL.len() as f32 * TRADE_GLYPH_PX;
/// TRADE's width in the collapse plan: the entry pair and the open-state
/// close and reverse, reserved flat too. Opening or closing a position then
/// never changes what folds, so nothing left of TRADE - and nothing in it -
/// moves when a fill lands.
pub const W_TRADE: f32 =
    2.0 * TRADE_ENTRY_WIDTH_PX + TRADE_CLOSE_WIDTH_PX + TRADE_REVERSE_WIDTH_PX + 3.0 * TRADE_GAP_PX;

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
