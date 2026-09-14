//! The paper account, driven by the backtest harness's loop — the second
//! consumer of `quantick-paper`.
//!
//! The chart's ticket drives a `PaperAccount` print by print. So does this
//! test, over the replay crate's own recorded opening auction, issuing its
//! orders on the same closed bars and at the same point in the loop that
//! `run_session` issues a strategy's. The harness's own run of the same
//! script is the reference: if the account the chart trades through ever
//! fills, closes or prices a trade differently from the harness, the two
//! lists below stop being equal and this test says which trade moved.

mod common;

use std::path::Path;

use quantick_backtest::bars::BarSpec;
use quantick_backtest::run::run_session;
use quantick_backtest::strategy::{BarView, Strategy};
use quantick_engine::Side;
use quantick_paper::PaperAccount;
use quantick_replay::format::ParseOptions;
use quantick_replay::session::Session;
use quantick_sim::{Bracket, Command, ExitReason, history};
use rust_decimal::Decimal;

use common::ScratchDir;

/// The 34-print opening auction the replay crate keeps as its own fixture.
/// Included rather than copied: one recording, one copy of the truth.
const OPENING_AUCTION: &str = include_str!("../../replay/tests/fixtures/WINQ26-2026-08-12.csv");

/// Three prints to a bar: eleven bars over the auction, enough for a round
/// trip closed by hand, one taken at its target, and one left open.
const SPEC: BarSpec = BarSpec::Tick(3);

/// Which command each closed bar issues. The prices are the auction's own:
/// it prints 168600 thirteen times, climbs to 168670, dips to 168650 and
/// ends at 168680.
fn script() -> Vec<(usize, Command)> {
    let price = |points: i64| Decimal::from(points);
    vec![
        // A long at the market on the first bar, closed by hand on the
        // fifth: in at 168600, out at the next print.
        (
            0,
            Command::PlaceMarket {
                side: Side::Buy,
                quantity: Decimal::ONE,
                bracket: Bracket::none(),
            },
        ),
        (4, Command::ClosePosition),
        // A protected long on the seventh bar: the climb reaches its
        // target before it comes back to its stop.
        (
            6,
            Command::PlaceMarket {
                side: Side::Buy,
                quantity: Decimal::ONE,
                bracket: Bracket::whole(Some(price(168_630)), Some(price(168_665))),
            },
        ),
        // A protected short on the tenth, still open when the recording
        // ends — no print proves its exit, so neither side may invent one.
        (
            9,
            Command::PlaceMarket {
                side: Side::Sell,
                quantity: Decimal::ONE,
                bracket: Bracket::whole(Some(price(168_700)), Some(price(168_600))),
            },
        ),
    ]
}

/// The script as a harness strategy: the commands for a bar, when it closes.
struct Scripted(Vec<(usize, Command)>);

impl Strategy for Scripted {
    fn name(&self) -> &str {
        "scripted-paper"
    }

    fn on_bar(&mut self, view: &BarView<'_>) -> Vec<Command> {
        self.0
            .iter()
            .filter(|(at, _)| *at == view.index)
            .map(|(_, command)| *command)
            .collect()
    }
}

fn auction() -> Session {
    Session::from_text(
        Path::new("fixtures/WINQ26/20260812.csv"),
        OPENING_AUCTION,
        ParseOptions::default(),
    )
    .expect("the replay crate's fixture parses")
}

#[test]
fn the_backtest_drives_the_same_paper_account_the_chart_trades_through() {
    let session = auction();
    let reference = run_session(&session, SPEC, &mut Scripted(script()));

    // The account, fed the way `run_session` feeds its venue: the print
    // first, then the bar it may close, then that bar's orders — queued
    // for the next print, because a market order never fills on the print
    // it was decided on.
    let dir = ScratchDir::new("paper-second-consumer");
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
    account.set_symbol(&session.symbol);
    account.set_session_source(history::SessionSource::Replay);
    let steps = script();
    let mut builder = SPEC.build();
    let mut bars = 0_usize;
    for trade in &session.trades {
        account.on_trade(trade);
        if builder.push(trade).is_none() {
            continue;
        }
        let index = bars;
        bars += 1;
        for (_, command) in steps.iter().filter(|(at, _)| *at == index) {
            let events = account.dispatch(*command);
            account.handle_events(events);
        }
    }

    assert_eq!(bars, reference.bars, "the same bars closed");
    assert_eq!(
        account.session_trades(),
        reference.trades.as_slice(),
        "the account and the harness filled and closed the same round trips"
    );
    let reasons: Vec<ExitReason> = reference
        .trades
        .iter()
        .map(|trade| trade.exit_reason)
        .collect();
    assert_eq!(
        reasons,
        [ExitReason::Manual, ExitReason::TakeProfit],
        "the script exercised a manual close and a target"
    );
    assert_eq!(
        account
            .venue()
            .position()
            .map(|position| (position.side, position.quantity)),
        reference.open_at_end,
        "both left the same position open, and neither invented its exit"
    );

    // What only the account adds to the harness's venue: the journal. It
    // is named from venue time and holds exactly the trades that closed.
    let folder = dir.path().join(&session.symbol);
    let files: Vec<_> = std::fs::read_dir(&folder)
        .expect("the account journaled under the instrument's folder")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(files.len(), 1, "one session, one file: {files:?}");
    let text = std::fs::read_to_string(&files[0]).expect("the journal is readable");
    let parsed = history::parse(&text).expect("the journal parses back");
    assert_eq!(parsed.source, Some(history::SessionSource::Replay));
    assert_eq!(
        parsed.trades, reference.trades,
        "the journal holds the harness's trades, byte-exact decimals"
    );
}
