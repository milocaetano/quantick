//! What the account's P&L reads as on the chrome: the ticket, the status
//! cell and the mode switch all ask here, so they cannot disagree.
//!
//! The figures and their wording are [`crate::pnl`]'s; this file only knows
//! which symbol and which trades the account holds.

use quantick_sim::ClosedTrade;

use super::PaperAccount;
use crate::pnl::{PnlPrimary, PnlReader};

impl PaperAccount {
    /// Every P&L readout's lookups: the declared book, the catalog and the
    /// display choice.
    #[must_use]
    pub fn pnl_reader(&self) -> PnlReader<'_> {
        PnlReader {
            book: self.pnl_book(),
            settings: &self.pnl,
        }
    }

    /// The figure this chart's readouts lead with.
    #[must_use]
    pub fn pnl_primary(&self) -> PnlPrimary {
        self.pnl_reader().primary(&self.symbol)
    }

    /// Pick the figure readouts lead with. App-wide once the host fans the
    /// settings out, like the risk per trade beside them.
    pub fn set_pnl_primary(&mut self, primary: PnlPrimary) {
        self.pnl.chosen = Some(primary);
    }

    /// The switch's choices, labelled for this chart's instrument: money
    /// reads as its currency code.
    #[must_use]
    pub fn pnl_modes(&self) -> [(PnlPrimary, String); 3] {
        let currency = self.symbol_money().map(|money| &money.currency);
        PnlPrimary::ALL.map(|primary| (primary, primary.label(currency)))
    }

    /// One closed trade of this chart's symbol, as a row reads it.
    #[must_use]
    pub fn trade_pnl_text(&self, trade: &ClosedTrade) -> String {
        self.pnl_reader()
            .trade(&self.symbol, trade.pnl_points, trade.quantity)
    }

    /// This session's realized trades, totalled.
    #[must_use]
    pub fn realized_pnl_text(&self) -> String {
        let symbol = self.symbol.as_str();
        self.pnl_reader().total(
            self.venue
                .closed_trades()
                .iter()
                .map(|trade| (symbol, trade)),
        )
    }

    /// The open position's profit at the last print, `None` while flat or
    /// before a print.
    #[must_use]
    pub fn open_pnl_text(&self) -> Option<String> {
        let position = self.venue.position()?;
        let open = position.open_points(self.venue.mark_price()?);
        Some(
            self.pnl_reader()
                .trade(&self.symbol, open, position.quantity),
        )
    }
}
