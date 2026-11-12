//! A contract the venue no longer lists, and the configured roll off it.
//!
//! The feed names the missing contract as data; this decides what the tab
//! does about it. A hand gets the roll offered ahead of Reconnect and Reload.
//! A tab restored from a saved workspace rolls itself off the contract it was
//! restored on, and along the chain while each next contract is unlisted too,
//! so a desk saved on an expired contract opens on the live one. Either way
//! the roll is [`super::apply`], the same act the palette and
//! `feed.contract.roll` run, refused by the same rule and journaled under the
//! same event kind.
pub use quantick_sources::recovery::{AutoRoll, RollOffer, rolled_sentence as unlisted_toast};

use super::{ContractRollPort, RollRefusal, apply, resolve};
use crate::JournalAccess;
use crate::workspace_book::Journal;
use quantick_control::wire::WireU64;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::contract_roll::{CONTRACT_ROLL_EVENT_KIND, ContractRollResult};
use quantick_control_schema::feed::MODULE_ID;
use serde_json::{Value, json};

/// The journal's `trigger` for a roll the notice's Roll button asked for.
pub const PRESSED_TRIGGER: &str = "notice_roll";
/// The journal's `trigger` for a roll a restored tab made by itself.
pub const AUTOMATIC_TRIGGER: &str = "workspace_restore";

/// What tab `index` needs from the window to roll off an unlisted contract.
pub trait UnlistedContractPort: ContractRollPort {
    /// How many tabs are open.
    fn tab_count(&self) -> usize;
    /// The contract tab `index`'s feed reported it does not list, as the
    /// feed's structured report names it.
    fn unlisted_contract(&self, index: usize) -> Option<String>;
    /// Tab `index`'s roll memory: armed, decided, pressed and on offer.
    fn auto_roll_mut(&mut self, index: usize) -> Option<&mut AutoRoll>;
}

/// The roll tab `index` can make off `missing`: the configured next contract,
/// or why it cannot roll right now. `None` when nothing changes from a plain
/// notice — the tab shows another contract, is a recording, or has no
/// offered successor.
pub fn unlisted_roll<H: ContractRollPort + ?Sized>(
    host: &H,
    index: usize,
    missing: &str,
) -> Option<Result<String, RollRefusal>> {
    let current = host.feed_selection(index)?;
    if current.symbol != missing {
        return None;
    }
    match resolve(host, index, &current.feed_id, missing, current.replay) {
        Ok(next) => Some(Ok(next)),
        Err(refusal @ (RollRefusal::PaperNotFlat | RollRefusal::GestureInFlight)) => {
            Some(Err(refusal))
        }
        Err(RollRefusal::Replay | RollRefusal::NotConfigured | RollRefusal::TargetNotOffered) => {
            None
        }
    }
}

/// The roll the notice of tab `index` offers, read from its own report.
pub fn notice_roll<H: UnlistedContractPort + ?Sized>(
    host: &H,
    index: usize,
) -> Option<Result<String, RollRefusal>> {
    unlisted_roll(host, index, &host.unlisted_contract(index)?)
}

/// What a restored tab says, once, when its roll has to wait.
#[must_use]
pub fn waiting_toast(from: &str, reason: &str) -> String {
    format!("{from} is no longer listed; the roll waits: {reason}")
}

/// What the trader reads when the switch a roll ran did not land.
#[must_use]
pub fn switch_failed_toast(to: &str) -> String {
    format!("Could not switch to {to}")
}

/// What one settle did: the words for the trader, and the rolls for the
/// journal.
#[derive(Debug, Default, PartialEq)]
pub struct Settled {
    /// One toast per roll, refused press, or refusal a restored tab waits on.
    pub notes: Vec<String>,
    /// One `feed.contract.rolled` payload per roll.
    pub rolls: Vec<Value>,
}

impl Settled {
    /// Nothing to say and nothing to journal.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.rolls.is_empty()
    }

    /// Journal each roll as `feed.contract.rolled` under the actor `journal`
    /// names — a window with no journal journals nothing — and answer the
    /// toasts.
    pub fn journal<A: JournalAccess>(self, journal: Journal<'_, A>) -> Vec<String> {
        if let Some((access, actor)) = journal {
            for payload in self.rolls {
                access.record_event(NewEvent::by(
                    MODULE_ID,
                    CONTRACT_ROLL_EVENT_KIND,
                    actor,
                    payload,
                ));
            }
        }
        self.notes
    }
}

/// The notice's Roll button on tab `index`: roll to `target` if it is still
/// the roll the notice offers. Answers the toast and the journal payload.
///
/// # Errors
/// The refusal, a notice that no longer offers `target`, or a switch that did
/// not land.
pub fn roll_from_notice<H: UnlistedContractPort + ?Sized>(
    host: &mut H,
    index: usize,
    target: &str,
) -> Result<(String, Value), String> {
    let missing = host
        .unlisted_contract(index)
        .ok_or("The feed no longer reports an unlisted contract")?;
    match unlisted_roll(host, index, &missing) {
        Some(Ok(next)) if next == target => {}
        Some(Err(refusal)) => return Err(refusal.reason().to_owned()),
        _ => return Err(format!("{target} is no longer the roll on offer")),
    }
    let payload = roll_off(host, index, &missing, target, PRESSED_TRIGGER)
        .ok_or_else(|| switch_failed_toast(target))?;
    Ok((unlisted_toast(&missing, target), payload))
}

/// Settle every tab: record the roll each one offers, run a pressed Roll,
/// then roll every tab armed for the contract its feed reports unlisted onto
/// the configured next one. A refused automatic roll decides nothing: it runs
/// on the settle after its refusal clears, and says the refusal once.
pub fn settle_auto_rolls<H: UnlistedContractPort + ?Sized>(host: &mut H) -> Settled {
    let mut settled = Settled::default();
    for index in 0..host.tab_count() {
        settle_tab(host, index, &mut settled);
    }
    settled
}

fn settle_tab<H: UnlistedContractPort + ?Sized>(
    host: &mut H,
    index: usize,
    settled: &mut Settled,
) -> Option<()> {
    let missing = host.unlisted_contract(index);
    let roll = missing
        .as_deref()
        .and_then(|missing| unlisted_roll(host, index, missing));
    let offer = missing
        .clone()
        .zip(roll.clone())
        .map(|(from, to)| RollOffer {
            from,
            to: to.map_err(RollRefusal::reason),
        });
    host.auto_roll_mut(index)?.set_offer(offer);
    if let Some(target) = host.auto_roll_mut(index)?.take_press() {
        match roll_from_notice(host, index, &target) {
            Ok((note, payload)) => {
                settled.notes.push(note);
                settled.rolls.push(payload);
            }
            Err(refusal) => settled.notes.push(refusal),
        }
        return Some(());
    }
    let missing = missing?;
    if !host.auto_roll_mut(index)?.armed_for(&missing) {
        return None;
    }
    match roll? {
        Ok(next) => match roll_off(host, index, &missing, &next, AUTOMATIC_TRIGGER) {
            Some(payload) => {
                settled.notes.push(unlisted_toast(&missing, &next));
                settled.rolls.push(payload);
            }
            // Said once: the licence ends, and the roll stays on offer.
            None => {
                host.auto_roll_mut(index)?.disarm();
                settled.notes.push(switch_failed_toast(&next));
            }
        },
        Err(refusal) => {
            if host.auto_roll_mut(index)?.refusal_is_news(refusal.reason()) {
                settled
                    .notes
                    .push(waiting_toast(&missing, refusal.reason()));
            }
        }
    }
    Some(())
}

/// Roll tab `index` off `from` onto `to`, leaving the attach that follows the
/// explanation, and answer the journal payload: `feed.contract.roll`'s result
/// and what asked for it. `None` when the switch did not land, and then the
/// roll memory is as it was: nothing decided, no licence moved, the offer
/// kept. The caller resolved `to` first, so the feed offers it and the carry
/// [`apply`] makes before the switch runs only where the switch can land.
fn roll_off<H: UnlistedContractPort + ?Sized>(
    host: &mut H,
    index: usize,
    from: &str,
    to: &str,
    trigger: &str,
) -> Option<Value> {
    // The attach inside the switch reads the roll, so it is staged first and
    // put back when the switch does not land.
    let before = host.auto_roll_mut(index)?.clone();
    host.auto_roll_mut(index)?
        .rolling(from, to, trigger == AUTOMATIC_TRIGGER);
    let carried = apply(host, index, to);
    let selected = host.feed_selection(index).filter(|now| now.symbol == to);
    let (Some(carried), Some(selected)) = (carried, selected) else {
        *host.auto_roll_mut(index)? = before;
        return None;
    };
    let result = ContractRollResult {
        tab_id: WireU64::new(host.tab_id_at(index)),
        feed_id: selected.feed_id,
        symbol: selected.symbol,
        from_symbol: from.to_owned(),
        changed: true,
        drawings_carried: carried as u64,
    };
    Some(json!({ "contract_roll": result, "trigger": trigger }))
}
