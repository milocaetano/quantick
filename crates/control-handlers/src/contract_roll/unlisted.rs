//! A contract the venue no longer lists, and the configured roll off it.
//!
//! The feed names the missing contract as data; this decides what the tab
//! does about it. A hand gets the roll offered in Reconnect's place. A tab
//! restored from a saved workspace rolls itself, once per missing contract,
//! so a desk saved on an expired contract opens on the live one. Either way
//! the roll is [`super::apply`], the same act the palette and
//! `feed.contract.roll` run, and refused by the same rule.
use std::collections::BTreeSet;

use super::{ContractRollPort, RollRefusal, apply, resolve};

/// What tab `index` needs from the window to roll off an unlisted contract.
pub trait UnlistedContractPort: ContractRollPort {
    /// How many tabs are open.
    fn tab_count(&self) -> usize;
    /// The contract tab `index`'s feed reported it does not list, as the
    /// feed's structured report names it.
    fn unlisted_contract(&self, index: usize) -> Option<String>;
    /// Whether tab `index`'s feed connected since it last attached: a contract
    /// that streams is listed.
    fn feed_connected(&self, index: usize) -> bool;
    /// Tab `index`'s memory of the rolls it made by itself.
    fn auto_roll_mut(&mut self, index: usize) -> Option<&mut AutoRoll>;
}

/// A tab's licence to roll by itself, and the contracts it already decided.
///
/// Armed when the tab is opened from a saved workspace and disarmed once its
/// feed connects. Each missing contract is decided once, rolled or refused,
/// so a chain of unlisted contracts is walked one step per report and a
/// configured cycle cannot loop.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AutoRoll {
    armed: bool,
    decided: BTreeSet<String>,
}

impl AutoRoll {
    /// The tab was opened from a saved workspace.
    pub fn arm(&mut self) {
        self.armed = true;
    }

    /// Whether the tab still rolls by itself.
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.armed
    }
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

/// What the trader reads after a roll off an unlisted contract.
#[must_use]
pub fn unlisted_toast(from: &str, to: &str) -> String {
    format!("{from} is no longer listed; rolled to {to}")
}

/// The notice's Roll button on tab `index`: roll to `target` if it is still
/// the roll the notice offers. Answers the toast.
///
/// # Errors
/// The refusal, or a notice that no longer offers `target`.
pub fn roll_from_notice<H: UnlistedContractPort + ?Sized>(
    host: &mut H,
    index: usize,
    target: &str,
) -> Result<String, String> {
    let missing = host
        .unlisted_contract(index)
        .ok_or("The feed no longer reports an unlisted contract")?;
    match unlisted_roll(host, index, &missing) {
        Some(Ok(next)) if next == target => {}
        Some(Err(refusal)) => return Err(refusal.reason().to_owned()),
        _ => return Err(format!("{target} is no longer the roll on offer")),
    }
    apply(host, index, target).ok_or("The tab being rolled has closed")?;
    Ok(unlisted_toast(&missing, target))
}

/// Roll every armed tab whose feed reports its contract unlisted onto the
/// configured next one, and disarm the tabs whose feed connected. Answers one
/// toast per roll.
pub fn settle_auto_rolls<H: UnlistedContractPort + ?Sized>(host: &mut H) -> Vec<String> {
    (0..host.tab_count())
        .filter_map(|index| settle_tab(host, index))
        .collect()
}

fn settle_tab<H: UnlistedContractPort + ?Sized>(host: &mut H, index: usize) -> Option<String> {
    if !host.auto_roll_mut(index)?.armed {
        return None;
    }
    if host.feed_connected(index) {
        host.auto_roll_mut(index)?.armed = false;
        return None;
    }
    let missing = host.unlisted_contract(index)?;
    let roll = unlisted_roll(host, index, &missing)?;
    if !host.auto_roll_mut(index)?.decided.insert(missing.clone()) {
        return None;
    }
    let next = roll.ok()?;
    apply(host, index, &next)?;
    Some(unlisted_toast(&missing, &next))
}
