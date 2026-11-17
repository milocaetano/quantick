// Where a capture's resting orders go, moved here from `app` with the
// decision: the rungs, their floor on a coarse quote, the bracket's sides,
// and the hook staying armed until something rests.

use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

use super::{CaptureOrders, MAX_CAPTURE_RUNGS};
use crate::PaperAccount;
use crate::scratch::ScratchDir;

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn account(dir: &ScratchDir) -> PaperAccount {
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
    account.set_symbol("CAPTX");
    account
}

fn rested(account: &PaperAccount) -> Vec<(Side, Decimal)> {
    account
        .working_orders()
        .iter()
        .map(|order| (order.side, order.price.expect("a limit has a price")))
        .collect()
}

const TWO: CaptureOrders = CaptureOrders {
    rungs: 2,
    bracket: false,
};

/// One order each side per rung, so whichever way the tape moves a tag is
/// still on screen for the shutter.
#[test]
fn the_rungs_step_out_either_side_of_the_mark() {
    let dir = ScratchDir::new("capture-rungs");
    let mut account = account(&dir);
    account.on_trade(&print(1, 100_000));
    assert!(account.rest_capture_orders(TWO));
    assert_eq!(
        rested(&account),
        vec![
            (Side::Buy, Decimal::from(99_940)),
            (Side::Sell, Decimal::from(100_060)),
            (Side::Buy, Decimal::from(99_880)),
            (Side::Sell, Decimal::from(100_120)),
        ],
        "two rungs, each side, stepping out from the mark"
    );
}

/// The bracket dresses every rung, each leg on the correct side of the
/// order's own price, which is what the venue validates.
#[test]
fn the_bracket_dresses_every_rung_on_its_own_side() {
    let dir = ScratchDir::new("capture-bracket");
    let mut account = account(&dir);
    account.on_trade(&print(1, 100_000));
    let one = CaptureOrders {
        rungs: 1,
        bracket: true,
    };
    assert!(account.rest_capture_orders(one));
    let orders = account.working_orders();
    assert_eq!(orders.len(), 2, "one rung, both sides");
    for order in orders {
        let price = order.price.expect("a limit has a price");
        let stop = order.bracket.stop_loss().expect("a stop rides along");
        let target = order.bracket.take_profit().expect("and a target");
        match order.side {
            Side::Buy => assert!(stop < price && target > price),
            Side::Sell => assert!(stop > price && target < price),
        }
    }
}

/// A coarsely quoted mark rounds 6 bp to nothing; the step floors at one
/// unit of the instrument's own precision and the rungs stay apart.
#[test]
fn an_integer_quoted_mark_still_rests_a_tick_per_rung() {
    let dir = ScratchDir::new("capture-coarse");
    let mut account = account(&dir);
    // 620 * 0.0006 = 0.372, which rounds to zero at scale 0.
    account.on_trade(&print(1, 620));
    assert!(account.rest_capture_orders(TWO));
    assert_eq!(
        rested(&account),
        vec![
            (Side::Buy, Decimal::from(619)),
            (Side::Sell, Decimal::from(621)),
            (Side::Buy, Decimal::from(618)),
            (Side::Sell, Decimal::from(622)),
        ]
    );
}

/// No mark, nothing rested: the hook stays armed.
#[test]
fn nothing_rests_before_the_first_mark() {
    let dir = ScratchDir::new("capture-unmarked");
    let mut account = account(&dir);
    assert!(!account.rest_capture_orders(TWO));
    assert!(account.working_orders().is_empty());
}

#[test]
fn the_hook_reads_a_rung_count_in_range_and_refuses_the_rest() {
    assert_eq!(
        CaptureOrders::parse(" 2 ", true),
        Some(CaptureOrders {
            rungs: 2,
            bracket: true
        })
    );
    let past = (MAX_CAPTURE_RUNGS + 1).to_string();
    for refused in ["0", past.as_str(), "two", ""] {
        assert_eq!(CaptureOrders::parse(refused, false), None, "{refused:?}");
    }
}
