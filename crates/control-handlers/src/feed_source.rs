//! Source selection policy over the same mutation used by the selectors.
//! `feed.source.set`, `tab.market.set` and the palette's switch mode all go
//! through [`set_source`]: one switch path.
//!
//! A changed source throws the tab's timeline away, closes its paper position
//! and disarms its strategies, so the switch is journaled with who made it and
//! the pair it left: an assistant's switch never reads as the trader's own.
use crate::{
    TabDirectory,
    dock::ActionDock,
    notify::NotifyAccess,
    tabs::{tab_closed, tab_index},
};
use quantick_control::wire::WireU64;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::feed::MODULE_ID;
use quantick_control_schema::feed_source::{
    FEED_SOURCE_EVENT_KIND, FeedSourceChoice, FeedSourceInput, FeedSourceResult, TabMarketSetInput,
    TabMarketSetResult, tab_market_descriptor,
};
use serde_json::{Value, json};

pub struct FeedSelection {
    pub feed_id: String,
    pub symbol: String,
    pub replay: bool,
}

pub trait FeedSourcePort: TabDirectory {
    fn feed_choices(&self) -> Vec<FeedSourceChoice>;
    fn feed_selection(&self, index: usize) -> Option<FeedSelection>;
    /// The validated pair, through the window's existing source switch.
    fn select_feed(&mut self, index: usize, feed_id: &str, symbol: &str) -> Option<FeedSelection>;
}

/// Dock `feed.source.set` and `tab.market.set`, one switch under two names.
/// A changed source journals through the access the notify handlers journal
/// through.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: FeedSourcePort + ?Sized,
    A: NotifyAccess,
{
    registry.register(quantick_control_schema::feed_source::descriptor(), select)?;
    registry.register(tab_market_descriptor(), set_market)
}

/// Put tab `tab_id` on the configured pair, through the window's own switch,
/// journaling nothing: the calls journal, the trader's hand does not.
///
/// # Errors
/// An unknown tab, a recording, or a pair the config does not list.
pub fn set_source<H: FeedSourcePort + ?Sized>(
    host: &mut H,
    tab_id: WireU64,
    feed_id: &str,
    symbol: &str,
) -> Result<FeedSourceResult, ControlError> {
    let index = tab_index(host, Some(tab_id))?;
    let current = host.feed_selection(index).ok_or_else(tab_closed)?;
    if current.replay {
        return Err(ControlError::invalid_request(
            "Close the recorded session before selecting a live source",
        ));
    }
    let valid = host.feed_choices().iter().any(|feed| {
        feed.feed_id == feed_id && feed.symbols.iter().any(|offered| offered == symbol)
    });
    if !valid {
        return Err(ControlError::invalid_request(
            "Select a configured feed_id and symbol pair from feed.status.sources",
        ));
    }
    let changed = current.feed_id != feed_id || current.symbol != symbol;
    let (from_feed_id, from_symbol) = (current.feed_id.clone(), current.symbol.clone());
    let selected = if changed {
        host.select_feed(index, feed_id, symbol)
            .ok_or_else(tab_closed)?
    } else {
        current
    };
    Ok(FeedSourceResult {
        tab_id,
        feed_id: selected.feed_id,
        symbol: selected.symbol,
        from_feed_id,
        from_symbol,
        changed,
        timeline_kept: !changed,
    })
}

/// Journal a changed source under the caller's name.
fn journal<A: NotifyAccess>(
    access: &mut A,
    actor: &ActorContext,
    result: &FeedSourceResult,
) -> Result<(), ControlError> {
    if result.changed {
        let payload = json!({ "feed_source": answer(result)? });
        access.record_event(NewEvent::by(
            MODULE_ID,
            FEED_SOURCE_EVENT_KIND,
            actor,
            payload,
        ));
    }
    Ok(())
}

fn answer<T: serde::Serialize>(result: &T) -> Result<Value, ControlError> {
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn select<H: FeedSourcePort + ?Sized, A: NotifyAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: FeedSourceInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let result = set_source(host, input.tab_id, &input.feed_id, &input.symbol)?;
    journal(access, actor, &result)?;
    answer(&result)
}

fn set_market<H: FeedSourcePort + ?Sized, A: NotifyAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TabMarketSetInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let result = set_source(host, input.tab, &input.feed, &input.symbol)?;
    journal(access, actor, &result)?;
    answer(&TabMarketSetResult::from(result))
}

#[cfg(test)]
mod tests;
