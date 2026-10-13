//! What the account's P&L reads as on the chrome: the ticket, the status
//! cell and the mode switch all ask here, so they cannot disagree.
//!
//! The figures and their wording are [`crate::pnl`]'s; this file only knows
//! which symbol and which trades the account holds.

use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;

use super::PaperAccount;
use crate::pnl::{PnlPrimary, PnlReader, PnlTotal, SymbolPnl};

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

    /// The reader bound to this chart's symbol.
    #[must_use]
    pub fn symbol_pnl(&self) -> SymbolPnl<'_> {
        self.pnl_reader().of(&self.symbol)
    }

    fn realized_total(&self) -> PnlTotal {
        let symbol = self.symbol.as_str();
        self.pnl_reader().sum(
            self.venue
                .closed_trades()
                .iter()
                .map(|trade| (symbol, trade)),
        )
    }

    /// This session's realized trades, totalled.
    #[must_use]
    pub fn realized_pnl_text(&self) -> String {
        self.pnl_reader().word(&self.realized_total())
    }

    /// The sign of [`Self::realized_pnl_text`]'s leading figure: what it
    /// is coloured by.
    #[must_use]
    pub fn realized_pnl_tint(&self) -> Decimal {
        self.pnl_reader().tint(&self.realized_total())
    }

    /// One figure of this chart's symbol in the mode the ticket leads with,
    /// alone: the HUD and the chart's position tag.
    #[must_use]
    pub fn pnl_compact(&self, pnl_points: Decimal, quantity: Decimal) -> String {
        self.pnl_reader()
            .compact(&self.symbol, pnl_points, quantity)
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

    /// The open position's stop and target, each with what the position
    /// would read at that level; `None` for a leg not set, or while flat.
    #[must_use]
    pub fn bracket_pnl(&self) -> [Option<(Decimal, String)>; 2] {
        let Some(position) = self.venue.position() else {
            return [None, None];
        };
        [position.stop_loss, position.take_profit].map(|level| {
            level.map(|level| {
                let at = position.open_points(level);
                let text = self.pnl_reader().trade(&self.symbol, at, position.quantity);
                (level, text)
            })
        })
    }
}
