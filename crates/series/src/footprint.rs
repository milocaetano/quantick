//! One forming ladder, with boundary ownership inferred from trade counts.
use quantick_engine::{Bar, BarFootprint, DEFAULT_LEVEL_CAP, FootprintBuilder, Trade};
use rust_decimal::Decimal;

pub(crate) struct FormingFootprint {
    builder: FootprintBuilder,
    pending: u64,
}

impl FormingFootprint {
    pub(crate) fn new(group: Decimal) -> Self {
        Self {
            builder: FootprintBuilder::new(group, DEFAULT_LEVEL_CAP),
            pending: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn observe(&mut self, trade: &Trade, closed: Option<&Bar>) -> Option<BarFootprint> {
        let Some(bar) = closed else {
            self.builder.push(trade);
            self.pending = self.pending.saturating_add(1);
            return None;
        };
        let closing_trade_included = bar.trade_count == self.pending.saturating_add(1);
        debug_assert!(
            closing_trade_included || bar.trade_count == self.pending,
            "footprint trade counter drifted from the bar builder's"
        );
        if closing_trade_included {
            self.builder.push(trade);
        }
        let Some(ladder) = self.builder.close() else {
            // Preserve the original alignment fallback. A valid closed
            // bar contains a print, so normal builders never take it.
            self.builder.push(trade);
            self.pending = 0;
            return Some(self.builder.close().expect("pushed just above"));
        };
        if closing_trade_included {
            self.pending = 0;
        } else {
            // The boundary print opens the next bar, not the closed one.
            self.builder.push(trade);
            self.pending = 1;
        }
        Some(ladder)
    }

    pub(crate) fn close_without(&mut self, bar: &Bar) -> Option<BarFootprint> {
        debug_assert!(
            bar.trade_count == self.pending,
            "footprint trade counter drifted from the bar builder's"
        );
        self.pending = 0;
        self.builder.close()
    }

    pub(crate) fn partial(&self) -> Option<&BarFootprint> {
        self.builder.partial()
    }
}
