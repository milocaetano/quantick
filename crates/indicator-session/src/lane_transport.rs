//! [`LaneTransport`] — the producer side of the forming run the session
//! retains: which of the tape's prints the session has not been sent yet.

use crate::IndicatorCommand;
use quantick_engine::trade_tape::TradeTape;
use quantick_indicators::FootprintBar;

/// Producer cursor for the session-owned forming run. No trade storage lives
/// here.
#[derive(Debug, Default)]
pub struct LaneTransport {
    rungs: usize,
    sent: usize,
}

impl LaneTransport {
    /// Forget what was sent: the next command cold-seeds the whole run.
    pub fn reset(&mut self) {
        self.sent = 0;
    }

    /// How many rungs the chart's lane shows; `0` turns the lane off.
    /// Returns whether the count changed.
    pub fn set_rungs(&mut self, rungs: usize) -> bool {
        let changed = self.rungs != rungs;
        if rungs == 0 {
            self.reset();
        }
        self.rungs = rungs;
        changed
    }

    /// The forming-bar update for this frame: the partial, and the forming
    /// run's prints not yet sent.
    ///
    /// Rate: **per frame**. The copy is the unsent suffix of the tape only —
    /// the prints since the previous command, or the forming bar's whole run
    /// after a reset — taken chunk slice by chunk slice, never the tape.
    pub fn command(
        &mut self,
        partial: Option<FootprintBar>,
        trades: &TradeTape,
    ) -> IndicatorCommand {
        let count = partial
            .as_ref()
            .filter(|_| self.rungs > 0)
            .map_or(0, |forming| {
                usize::try_from(forming.bar.trade_count)
                    .unwrap_or(usize::MAX)
                    .min(trades.len())
            });
        let start = trades.len() - count + self.sent.min(count);
        let mut run = Vec::with_capacity(trades.len() - start);
        for slice in trades.slices(start..) {
            run.extend_from_slice(slice);
        }
        self.sent = count;
        IndicatorCommand::PartialUpdated {
            partial,
            run,
            rungs: self.rungs,
        }
    }
}
