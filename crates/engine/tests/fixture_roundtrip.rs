//! Integration test: the committed sample fixture parses and round-trips.
//!
//! Reads the real file from disk (not an inline string) so the on-disk fixture
//! format itself is covered, and asserts `parse -> write -> parse` is stable.

use quantick_engine::fixture;

const SAMPLE: &str = include_str!("fixtures/sample_trades.csv");

#[test]
fn sample_fixture_parses() {
    let trades = fixture::parse_trades(SAMPLE).expect("sample fixture should parse");
    assert_eq!(trades.len(), 6, "sample has six trades");
    assert_eq!(trades[0].agg_id, 1);
    assert_eq!(trades[5].agg_id, 6);
    // agg_ids are strictly increasing in the sample.
    for pair in trades.windows(2) {
        assert!(pair[1].agg_id > pair[0].agg_id, "agg_ids increase");
    }
}

#[test]
fn sample_fixture_round_trips() {
    let trades = fixture::parse_trades(SAMPLE).expect("sample fixture should parse");
    let rewritten = fixture::write_trades(&trades);
    let reparsed = fixture::parse_trades(&rewritten).expect("rewritten fixture should parse");
    assert_eq!(trades, reparsed, "parse -> write -> parse is lossless");
}
