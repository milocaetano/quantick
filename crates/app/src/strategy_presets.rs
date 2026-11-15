//! The strategy preset bank: the document is `quantick_strategy::presets`;
//! this is where its path is resolved.

use std::path::PathBuf;

pub(crate) use quantick_strategy::presets::*;

/// Resolve the bank file: the env override first, then the durable cockpit
/// home. Under test, a scratch file per store.
#[must_use]
pub fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(STRATEGIES_FILE);
    }
    crate::store_home::resolve(STRATEGIES_FILE)
}

/// Cache the domain bank's ordered rows after load or a real mutation.
pub(crate) fn snapshot(bank: &StrategyBank) -> Vec<(String, StoredPreset)> {
    bank.names()
        .filter_map(|name| bank.get(name).map(|form| (name.to_owned(), form.clone())))
        .collect()
}
/// Execute the popup's requested IO through the authoritative bank.
pub(crate) fn apply(
    bank: &mut StrategyBank,
    effect: quantick_ui_model::strategy_popup::BankEffect,
) -> quantick_ui_model::strategy_popup::Intent {
    use quantick_ui_model::strategy_popup::{BankEffect, Intent};
    match effect {
        BankEffect::Save { name, form } => bank.save(&name, *form),
        BankEffect::Remove(name) => bank.remove(&name),
    }
    Intent::BankSnapshot(snapshot(bank))
}
