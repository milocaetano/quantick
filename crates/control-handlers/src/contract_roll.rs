//! The contract roll, one rule for the toolbar and the control plane.
//!
//! A roll is a source switch that keeps the tab's marks: the same mutation as
//! the selectors, after the drawings were copied to the contract they are
//! about to be looked up under. The next contract is only ever the config's.
use crate::{
    dock::ActionDock,
    feed_source::FeedSourcePort,
    notify::NotifyAccess,
    tabs::{tab_closed, tab_index},
};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::contract_roll::{
    CONTRACT_ROLL_EVENT_KIND, ContractRollInput, ContractRollResult, FROM_SYMBOL_PRECONDITION,
    LIVE_SOURCE_PRECONDITION, PAPER_FLAT_PRECONDITION, ROLL_CONFIGURED_PRECONDITION,
    TARGET_OFFERED_PRECONDITION,
};
use quantick_control_schema::feed::MODULE_ID;
use serde_json::{Value, json};

/// What a roll needs beyond a source switch.
pub trait ContractRollPort: FeedSourcePort {
    fn paper_position_open(&self, index: usize) -> bool;
    /// Copy the drawings on `from` to `to`; how many objects were added.
    fn carry_market_drawings(&mut self, feed_id: &str, from: &str, to: &str) -> usize;
}

/// Why a tab cannot roll right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RollRefusal {
    Replay,
    NotConfigured,
    TargetNotOffered,
    PaperPositionOpen,
}

impl RollRefusal {
    /// The sentence the trader and the caller both read.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::Replay => "A recorded session has no next contract",
            Self::NotConfigured => "The feed config names no contract after this one",
            Self::TargetNotOffered => "The feed does not offer the configured next contract",
            Self::PaperPositionOpen => "Close the paper position before rolling",
        }
    }
}

impl From<RollRefusal> for ControlError {
    fn from(refusal: RollRefusal) -> Self {
        let (precondition, next_step) = match refusal {
            RollRefusal::Replay => (
                LIVE_SOURCE_PRECONDITION,
                "close the recorded session and roll the live tab",
            ),
            RollRefusal::NotConfigured => (
                ROLL_CONFIGURED_PRECONDITION,
                "name the next contract in the feed config (contract_rolls)",
            ),
            RollRefusal::TargetNotOffered => (
                TARGET_OFFERED_PRECONDITION,
                "offer the next contract on this feed (symbols) or add it in Open market",
            ),
            RollRefusal::PaperPositionOpen => (
                PAPER_FLAT_PRECONDITION,
                "close the paper position, then roll",
            ),
        };
        crate::bubble_scale::refused_precondition(precondition, refusal.reason(), next_step)
    }
}

/// The contract the tab at `index` would roll to, or why it cannot.
pub fn target<H: ContractRollPort + ?Sized>(
    host: &H,
    index: usize,
) -> Option<Result<String, RollRefusal>> {
    let current = host.feed_selection(index)?;
    Some(resolve(
        host,
        index,
        &current.feed_id,
        &current.symbol,
        current.replay,
    ))
}

fn resolve<H: ContractRollPort + ?Sized>(
    host: &H,
    index: usize,
    feed_id: &str,
    symbol: &str,
    replay: bool,
) -> Result<String, RollRefusal> {
    if replay {
        return Err(RollRefusal::Replay);
    }
    let choices = host.feed_choices();
    let feed = choices.iter().find(|feed| feed.feed_id == feed_id);
    let next = feed
        .and_then(|feed| feed.contract_rolls.get(symbol))
        .ok_or(RollRefusal::NotConfigured)?;
    if !feed.is_some_and(|feed| feed.symbols.contains(next)) {
        return Err(RollRefusal::TargetNotOffered);
    }
    if host.paper_position_open(index) {
        return Err(RollRefusal::PaperPositionOpen);
    }
    Ok(next.clone())
}

/// Roll the tab at `index` to `next`, which [`target`] answered. How many
/// drawings the new contract gained, or `None` when the tab is gone.
pub fn apply<H: ContractRollPort + ?Sized>(
    host: &mut H,
    index: usize,
    next: &str,
) -> Option<usize> {
    let current = host.feed_selection(index)?;
    let carried = host.carry_market_drawings(&current.feed_id, &current.symbol, next);
    host.select_feed(index, &current.feed_id, next)?;
    Some(carried)
}

/// The roll the active tab's symbol list offers: the configured target, and
/// why it is greyed when it is. `None` when there is nothing to offer.
pub fn offer<H: ContractRollPort + ?Sized>(host: &H) -> Option<(String, Option<&'static str>)> {
    let index = host.active_tab_index();
    let current = host.feed_selection(index)?;
    if current.replay {
        return None;
    }
    let choices = host.feed_choices();
    let feed = choices
        .iter()
        .find(|feed| feed.feed_id == current.feed_id)?;
    let next = feed.contract_rolls.get(&current.symbol)?.clone();
    let refusal = target(host, index)?.err().map(RollRefusal::reason);
    Some((next, refusal))
}

/// The toolbar's act: the rule [`target`] asks, then the same [`apply`] the
/// capability runs. A refused roll changes nothing; the entry was greyed.
pub fn roll_active<H: ContractRollPort + ?Sized>(host: &mut H) {
    let index = host.active_tab_index();
    if let Some(Ok(next)) = target(&*host, index) {
        apply(host, index, &next);
    }
}

/// Dock the capability.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: ContractRollPort + ?Sized,
    A: NotifyAccess,
{
    registry.register(quantick_control_schema::contract_roll::descriptor(), roll)
}

fn roll<H: ContractRollPort + ?Sized, A: NotifyAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ContractRollInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(host, Some(input.tab_id))?;
    let current = host.feed_selection(index).ok_or_else(tab_closed)?;
    if current.replay {
        return Err(RollRefusal::Replay.into());
    }
    if current.symbol != input.from_symbol {
        // A dropped answer's retry finds the tab on the target already.
        let rolled = host
            .feed_choices()
            .iter()
            .find(|feed| feed.feed_id == current.feed_id)
            .and_then(|feed| feed.contract_rolls.get(&input.from_symbol))
            .is_some_and(|next| *next == current.symbol);
        if !rolled {
            return Err(crate::bubble_scale::refused_precondition(
                FROM_SYMBOL_PRECONDITION,
                "The tab no longer shows from_symbol",
                "read active_symbol in feed.status and name it as from_symbol",
            ));
        }
        return answer(ContractRollResult {
            tab_id: input.tab_id,
            feed_id: current.feed_id,
            symbol: current.symbol,
            from_symbol: input.from_symbol,
            changed: false,
            drawings_carried: 0,
        });
    }
    let next = resolve(host, index, &current.feed_id, &current.symbol, false)?;
    let carried = apply(host, index, &next).ok_or_else(tab_closed)?;
    let selected = host.feed_selection(index).ok_or_else(tab_closed)?;
    let result = answer(ContractRollResult {
        tab_id: input.tab_id,
        feed_id: selected.feed_id,
        symbol: selected.symbol,
        from_symbol: input.from_symbol,
        changed: true,
        drawings_carried: carried as u64,
    })?;
    access.record_event(NewEvent::by(
        MODULE_ID,
        CONTRACT_ROLL_EVENT_KIND,
        actor,
        json!({ "contract_roll": result.clone() }),
    ));
    Ok(result)
}

fn answer(result: ContractRollResult) -> Result<Value, ControlError> {
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

#[cfg(test)]
mod tests;
