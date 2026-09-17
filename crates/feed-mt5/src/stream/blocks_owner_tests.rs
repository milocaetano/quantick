//! The rate cap is exercised at its existing owner, without a million TCP rows.

use super::*;

#[test]
fn a_full_rate_owner_clips_the_next_row_and_keeps_its_existing_bars() {
    let mut block = RatesBlock::new(60_000, 0);
    let protocol::BridgeMsg::Rate(chunk) =
        protocol::parse_line(r#"{"type":"rate","bars":[[60000,"10","12","9","11","3"]]}"#).unwrap()
    else {
        panic!("expected rate fixture")
    };
    block.absorb(&chunk);
    let bar = block.bars.values().next().unwrap().clone();
    for index in 0..MAX_BARS_PER_BLOCK {
        block.bars.insert(index as i64, bar.clone());
    }
    assert_eq!(block.len(), 1_000_000);
    block.absorb(&chunk);
    let (interval, bars, clipped) = block.finish();
    assert_eq!(interval, 60_000);
    assert_eq!(bars.len(), 1_000_000);
    assert!(clipped);
    assert_eq!(bars[0].close, rust_decimal::Decimal::from(11));
}
