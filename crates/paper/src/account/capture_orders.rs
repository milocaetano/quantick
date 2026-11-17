//! A capture run's resting orders (`QUANTICK_PAPER_ORDERS`): limits either
//! side of the first mark, so a screenshot shows working orders, their tags
//! and their legs without a hand. The window reads the hook; where the rungs
//! go is decided here.

use quantick_engine::Side;
use quantick_trading::{Bracket, OrderIntent};
use rust_decimal::Decimal;

use super::PaperAccount;

/// Rungs past this are refused: a capture wants a tag or two, not a book.
pub const MAX_CAPTURE_RUNGS: u8 = 4;

/// How far the first rung sits from the mark, as a fraction of it (6 bp).
/// Small on purpose: a line outside the chart's autoscaled price range
/// paints no tag, so an order that cannot be reached also cannot be seen.
const STEP_FRACTION: Decimal = Decimal::from_parts(6, 0, 0, false, 4);

/// How far a dressed rung's legs sit from it, as a fraction of the mark
/// (15 bp): wider than the rung step so a leg never lands on a neighbouring
/// order's line, and far enough apart to read as two levels.
const BRACKET_FRACTION: Decimal = Decimal::from_parts(15, 0, 0, false, 4);

/// What a capture asked to rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureOrders {
    /// Rungs either side of the mark.
    pub rungs: u8,
    /// Dress each rung with a stop and a target
    /// (`QUANTICK_PAPER_ORDER_BRACKET`).
    pub bracket: bool,
}

impl CaptureOrders {
    /// Read the hook's rung count. Refused, never guessed, and said in the
    /// log: a typo that silently photographed an orderless chart would read
    /// as a defect in the thing being photographed.
    #[must_use]
    pub fn parse(value: &str, bracket: bool) -> Option<Self> {
        let rungs = value
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|rungs| (1..=MAX_CAPTURE_RUNGS).contains(rungs));
        if rungs.is_none() {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_ORDERS_AUTOSTART_UNKNOWN",
                value = %value,
                max = MAX_CAPTURE_RUNGS,
                "QUANTICK_PAPER_ORDERS wants a rung count from 1 to the maximum"
            );
        }
        Some(Self {
            rungs: rungs?,
            bracket,
        })
    }
}

impl PaperAccount {
    /// Rest a capture's orders around the mark. Answers whether any order
    /// rests now; until one does, the hook stays armed, because disarming
    /// before the simulator accepted anything is how a silent empty capture
    /// happens.
    ///
    /// The rung offsets snap to the instrument's own precision, and never
    /// to nothing: on a coarsely quoted mark 6 bp rounds to zero, both legs
    /// would price *at* the mark and the simulator would refuse them all. So
    /// the step floors at one unit of that precision.
    pub fn rest_capture_orders(&mut self, orders: CaptureOrders) -> bool {
        let Some(mark) = self.venue().mark_price() else {
            return false;
        };
        let tick = Decimal::ONE
            .checked_div(Decimal::from(10_u64.pow(mark.scale().min(18))))
            .unwrap_or(Decimal::ONE);
        for rung in 1..=u32::from(orders.rungs) {
            let step = (mark * STEP_FRACTION * Decimal::from(rung))
                .round_dp(mark.scale())
                .max(tick * Decimal::from(rung));
            for (side, price) in [
                (Side::Buy, mark.saturating_sub(step)),
                (Side::Sell, mark.saturating_add(step)),
            ] {
                let intent = self.capture_intent(side, price, (mark, tick), orders.bracket);
                let events = self.venue_mut().submit(intent);
                self.handle_events(events);
            }
        }
        if self.venue().working_orders().is_empty() {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_ORDERS_HOOK_REJECTED",
                %mark,
                action = "retry_next_print",
                "QUANTICK_PAPER_ORDERS rested nothing around this mark"
            );
            return false;
        }
        true
    }

    /// One rung's limit. A ticket armed with a ladder rests a laddered
    /// order, one contract per rung so the ladder photographs as written;
    /// otherwise the bracket hook dresses it with legs on the correct side
    /// of the *order's own* price, which is what the venue validates.
    fn capture_intent(
        &self,
        side: Side,
        price: Decimal,
        (mark, tick): (Decimal, Decimal),
        dress: bool,
    ) -> OrderIntent {
        let strategy = self.selected_order_strategy();
        let quantity = strategy.map_or(Decimal::ONE, |strategy| {
            Decimal::from(strategy.rows.len().max(1))
        });
        let laddered = strategy
            .and_then(|strategy| strategy.resolve(side, price, quantity, tick).ok())
            .filter(Bracket::is_laddered);
        let bracket = match laddered {
            Some(bracket) => bracket,
            // The same floor the rung step carries, for the same reason.
            None if dress => {
                let reach = (mark * BRACKET_FRACTION).round_dp(mark.scale()).max(tick);
                let (below, above) = (price.saturating_sub(reach), price.saturating_add(reach));
                match side {
                    Side::Buy => Bracket::whole(Some(below), Some(above)),
                    Side::Sell => Bracket::whole(Some(above), Some(below)),
                }
            }
            None => Bracket::none(),
        };
        OrderIntent::limit(side, quantity, price).with_bracket(bracket)
    }
}

#[cfg(test)]
#[path = "capture_orders_tests.rs"]
mod tests;
