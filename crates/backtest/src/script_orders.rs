//! A Pine script's `alertcondition`s trading a recorded session: the very
//! [`ScriptInstance`] a chart arms, armed to place orders.
//!
//! The chart's paper trading and this harness step the instance through the
//! same shared runner, so a backtest of a script order is the order the chart
//! would have placed on that tape. Two differences are the harness's, and the
//! report is read with them in mind: the placement is always
//! [`ScriptPlacement::Anywhere`], since no drawing exists here, and no
//! footprint is kept, so a script reading `poc` reads `na`.

use quantick_engine::{Bar, Side};
use quantick_sim::VenueEvent;
use quantick_strategy::runner;
use quantick_strategy::script::{Script, ScriptInstance, ScriptPlacement};
use quantick_strategy::{OrderQuantity, PriceBracket, SignalAction, SignalOrder};
use rust_decimal::Decimal;

use crate::strategy::{Signals, SimPort, Strategy};

/// One script, hunting one side, placing its order on every signal a flat
/// account can take. A recorded session has no ticket, so the size is fixed.
pub struct ScriptOrders {
    name: String,
    script: Script,
    side: Side,
    order: SignalOrder,
    instance: ScriptInstance<()>,
}

impl ScriptOrders {
    /// Arm `script` to enter `quantity` on its alerts naming `side`,
    /// protected by `bracket`.
    #[must_use]
    pub fn new(script: Script, side: Side, quantity: Decimal, bracket: PriceBracket) -> Self {
        let order = SignalOrder {
            quantity: OrderQuantity::Fixed(quantity),
            stop: bracket.stop,
            gain: bracket.gain,
        };
        let instance = arm(&script, side, order);
        Self {
            name: format!("script-orders({})", script.title()),
            script,
            side,
            order,
            instance,
        }
    }
}

fn arm(script: &Script, side: Side, order: SignalOrder) -> ScriptInstance<()> {
    let action = SignalAction::Order(order);
    ScriptInstance::arm(0, script, side, ScriptPlacement::Anywhere, &[], action)
}

impl Strategy for ScriptOrders {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_print_events(&mut self, events: &[VenueEvent], port: &mut dyn SimPort) {
        runner::deliver_print_events(std::slice::from_mut(&mut self.instance), events, port);
    }

    fn on_closed_bar(
        &mut self,
        bar: &Bar,
        _index: usize,
        _signals: Signals<'_>,
        port: &mut dyn SimPort,
    ) {
        let resolved = ScriptPlacement::Anywhere.resolve(|()| None);
        // No alarm sounds in a backtest, so the clock it is told is moot.
        let _ = self
            .instance
            .on_closed_bar(bar, None, resolved.as_ref(), port, 0);
    }

    /// The script starts over: one recorded day's history must not judge
    /// the next.
    fn end_of_session(&mut self) {
        self.instance = arm(&self.script, self.side, self.order);
    }
}
