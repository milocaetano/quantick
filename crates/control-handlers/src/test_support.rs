//! A fake window and a fake access: every port a family names, over plain
//! fields a test sets and reads back. The second implementation of each port,
//! after the application's own adapter.
//! Test-only even when inspected independently of the parent module.
#![cfg(test)]

use quantick_engine::{Side, Trade};
use quantick_sources::history_reach::{HistoryReach, ReachProgress};
use quantick_sources::history_run::RunStatus;
use quantick_stores::config::AppConfig;
use rust_decimal::Decimal;

mod access;
mod chart;
mod desk;
mod indicators;
mod pane;
mod paper;
mod tab;
mod window;

pub(crate) use access::FakeAccess;
pub(crate) use indicators::FakeIndicators;
pub(crate) use pane::{FakeBubbles, FakePane, FakeTape};
pub(crate) use paper::{NoDesk, PaperDesk};
pub(crate) use tab::FakeTab;
pub(crate) use window::{FakeWindow, TabsWindow};

/// The main-click default a fake window opens with.
pub(crate) const FAKE_DEFAULT_REACH: HistoryReach = HistoryReach::Sessions(9);

/// One fake feed: its id, the symbols it offers and the contract each of
/// those rolls onto.
pub(crate) type FakeFeed<'a> = (&'a str, &'a [&'a str], &'a [(&'a str, &'a str)]);

/// A config over `feeds`.
pub(crate) fn fake_config(feeds: &[FakeFeed<'_>]) -> AppConfig {
    let feeds: Vec<serde_json::Value> = feeds
        .iter()
        .map(|(id, symbols, rolls)| {
            let rolls: std::collections::BTreeMap<_, _> = rolls.iter().copied().collect();
            serde_json::json!({
                "id": id,
                "name": id,
                "provider": "binance",
                "symbols": symbols,
                "contract_rolls": rolls,
            })
        })
        .collect();
    let config =
        serde_json::json!({ "default_feed": "fake", "default_symbol": "FAKE", "feeds": feeds });
    serde_json::from_value(config).expect("a valid config")
}

/// The fixed time a fake access tells a handler it runs at.
pub(crate) const FAKE_NOW_UNIX_MS: i64 = 100;

/// A run paging toward `reach`, nothing pulled yet.
pub(crate) fn loading(reach: HistoryReach) -> RunStatus {
    RunStatus::Loading(ReachProgress {
        reach,
        oldest_ms: 0,
        sessions_reached: 0,
        traded_ms: 0,
        prints_pulled: 0,
        pages: 0,
    })
}

/// A buy print at `price`, the `id`th second of the epoch.
pub(crate) fn trade(id: u64, price: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: i64::try_from(id).expect("small id") * 1_000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}
