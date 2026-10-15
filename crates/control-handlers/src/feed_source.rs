//! Source selection policy over the same mutation used by the selectors.
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
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::feed::MODULE_ID;
use quantick_control_schema::feed_source::{
    FEED_SOURCE_EVENT_KIND, FeedSourceChoice, FeedSourceInput, FeedSourceResult,
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

/// Dock the capability. A changed source journals through the access the
/// notify handlers journal through.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: FeedSourcePort + ?Sized,
    A: NotifyAccess,
{
    registry.register(quantick_control_schema::feed_source::descriptor(), select)
}

fn select<H: FeedSourcePort + ?Sized, A: NotifyAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: FeedSourceInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(host, Some(input.tab_id))?;
    let current = host.feed_selection(index).ok_or_else(tab_closed)?;
    if current.replay {
        return Err(ControlError::invalid_request(
            "Close the recorded session before selecting a live source",
        ));
    }
    let valid = host
        .feed_choices()
        .iter()
        .any(|feed| feed.feed_id == input.feed_id && feed.symbols.contains(&input.symbol));
    if !valid {
        return Err(ControlError::invalid_request(
            "Select a configured feed_id and symbol pair from feed.status.sources",
        ));
    }
    let changed = current.feed_id != input.feed_id || current.symbol != input.symbol;
    let (from_feed_id, from_symbol) = (current.feed_id.clone(), current.symbol.clone());
    let selected = if changed {
        host.select_feed(index, &input.feed_id, &input.symbol)
            .ok_or_else(tab_closed)?
    } else {
        current
    };
    let result = serde_json::to_value(FeedSourceResult {
        tab_id: input.tab_id,
        feed_id: selected.feed_id,
        symbol: selected.symbol,
        from_feed_id,
        from_symbol,
        changed,
        timeline_kept: !changed,
    })
    .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    if changed {
        let payload = json!({ "feed_source": result.clone() });
        access.record_event(NewEvent::by(
            MODULE_ID,
            FEED_SOURCE_EVENT_KIND,
            actor,
            payload,
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
