//! [`TradeTape`]: the chart's retained tape, and what an append may cost.
//!
//! The oracle is the `Vec<Trade>` the chart held before the tape was chunked:
//! after every one of a sequence of irregular appends and prepends, every read
//! the tape offers must answer exactly as the same read over the vector
//! does, across several chunk boundaries. Three tests pin what the chunks buy
//! — no print moves when the tape grows, at most one chunk is held unused,
//! and a stretch comes back as one slice per chunk it touches.

use quantick_engine::trade_tape::{CHUNK_TRADES, TradeSeq, TradeTape};
use quantick_engine::{Side, Trade, fixture::parse_trades};
use rust_decimal::Decimal;

/// Print `i` of a synthetic tape: stamps that repeat in pairs (so a binary
/// search meets runs of equal keys), prices and sides that move.
fn trade(i: usize) -> Trade {
    Trade {
        agg_id: i as u64 + 1,
        timestamp_ms: 1_700_000_000_000 + (i / 2) as i64,
        price: Decimal::from(50_000 + (i * 7_919 % 101) as i64),
        quantity: Decimal::new(1 + (i % 9) as i64, 1),
        side: if i.is_multiple_of(3) {
            Side::Sell
        } else {
            Side::Buy
        },
    }
}

/// A deterministic stream of batch sizes: small, around a chunk, and larger.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % below as u64) as usize
    }
}

/// The positions worth reading at: both ends, and either side of every
/// chunk boundary the tape reaches.
fn positions(len: usize) -> Vec<usize> {
    let mut at = vec![0, 1, 2, len / 2];
    for boundary in (1..=len / CHUNK_TRADES + 1).map(|k| k * CHUNK_TRADES) {
        at.extend([boundary - 1, boundary, boundary + 1]);
    }
    at.extend([len.saturating_sub(2), len.saturating_sub(1), len]);
    at.retain(|&p| p <= len);
    at.sort_unstable();
    at.dedup();
    at
}

/// A positional reader with nothing but the two required methods, so the
/// trait's own default binary search is what answers.
struct Bare<'a>(&'a [Trade]);

impl TradeSeq for Bare<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<&Trade> {
        self.0.get(index)
    }
}

/// Every read the tape offers, against the same read over the oracle.
fn assert_reads_like(tape: &TradeTape, oracle: &[Trade]) {
    let len = oracle.len();
    assert_eq!(tape.len(), len);
    assert_eq!(tape.is_empty(), oracle.is_empty());
    assert_eq!(tape.first(), oracle.first());
    assert_eq!(tape.last(), oracle.last());
    assert!(tape.capacity() >= len);
    assert!(tape.iter().eq(oracle.iter()), "front to back");
    assert!(tape.iter().rev().eq(oracle.iter().rev()), "back to front");
    assert_eq!(tape.iter().len(), len);
    assert!(tape.into_iter().eq(oracle.iter()));

    let at = positions(len);
    for &p in &at {
        assert_eq!(tape.get(p), oracle.get(p), "get({p})");
        if p < len {
            assert_eq!(&tape[p], &oracle[p], "tape[{p}]");
        }
        assert!(tape.since(p).eq(oracle[p..].iter()), "since({p})");
        let n = len - p;
        assert!(tape.last_n(n).eq(oracle[p..].iter()), "last_n({n})");
    }
    assert!(
        tape.last_n(len + 5).eq(oracle.iter()),
        "last_n past the start"
    );
    assert_eq!(tape.last_n(0).len(), 0);

    for &start in &at {
        for &end in at.iter().filter(|&&end| end >= start) {
            let want = &oracle[start..end];
            // Windows up to a chunk and a bit, and the whole tape, are read
            // whole; longer ones at both ends and at every chunk seam inside,
            // which is where the chunk arithmetic lives.
            let whole = want.len() <= CHUNK_TRADES + 8 || (start == 0 && end == len);
            let window = tape.range(start..end);
            assert_eq!(window.len(), want.len(), "range({start}..{end}).len()");
            if whole {
                assert!(window.clone().eq(want.iter()), "range({start}..{end})");
                assert!(window.rev().eq(want.iter().rev()), "range rev");
            } else {
                assert!(window.clone().take(8).eq(want.iter().take(8)));
                assert!(window.rev().take(8).eq(want.iter().rev().take(8)));
            }
            let mut ends = tape.range(start..end);
            let mut oracle_ends = want.iter();
            for step in 0..6 {
                if step % 2 == 0 {
                    assert_eq!(ends.next(), oracle_ends.next(), "alternating front");
                } else {
                    assert_eq!(
                        ends.next_back(),
                        oracle_ends.next_back(),
                        "alternating back"
                    );
                }
                assert_eq!(ends.len(), oracle_ends.len());
            }

            let slices: Vec<&[Trade]> = tape.slices(start..end).collect();
            assert!(
                slices.iter().all(|slice| !slice.is_empty()),
                "no empty slice"
            );
            assert_eq!(tape.slices(start..end).len(), slices.len());
            let mut offset = 0;
            for slice in &slices {
                let expected = &want[offset..offset + slice.len()];
                if whole {
                    assert_eq!(*slice, expected, "slices({start}..{end})");
                } else {
                    assert_eq!(slice.first(), expected.first(), "slices({start}..{end})");
                    assert_eq!(slice.last(), expected.last(), "slices({start}..{end})");
                }
                offset += slice.len();
            }
            assert_eq!(offset, want.len(), "slices({start}..{end}) cover the range");
            let backwards: Vec<&[Trade]> = tape.slices(start..end).rev().collect();
            assert!(
                backwards
                    .iter()
                    .rev()
                    .zip(&slices)
                    .all(|(back, front)| std::ptr::eq(*back, *front))
            );
            assert_eq!(backwards.len(), slices.len());
        }
    }
    assert!(tape.range(..).eq(oracle.iter()));
    if len > 3 {
        assert!(tape.range(1..=2).eq(oracle[1..=2].iter()));
    }

    assert_eq!(TradeSeq::len(tape), len);
    assert_eq!(TradeSeq::first(tape), oracle.first());
    assert_eq!(TradeSeq::is_empty(tape), oracle.is_empty());
    assert_eq!(TradeSeq::get(tape, len / 2), oracle.get(len / 2));
    assert_eq!(TradeSeq::len(oracle), len);
    assert_eq!(TradeSeq::get(oracle, len / 2), oracle.get(len / 2));

    // A binary search answers only for a tape sorted by the key it cuts on;
    // the chart's own tape is, and so are the synthetic ones here.
    if !oracle.is_sorted_by_key(|trade| trade.timestamp_ms) {
        return;
    }
    let first = oracle.first().map_or(0, |trade| trade.timestamp_ms);
    let last = oracle.last().map_or(0, |trade| trade.timestamp_ms);
    for cut in [
        first - 1,
        first,
        first + 1,
        (first + last) / 2,
        last,
        last + 1,
    ]
    .into_iter()
    .chain(
        at.iter()
            .filter_map(|&p| oracle.get(p))
            .map(|t| t.timestamp_ms),
    ) {
        let want = oracle.partition_point(|trade| trade.timestamp_ms < cut);
        assert_eq!(
            tape.partition_point(|trade| trade.timestamp_ms < cut),
            want,
            "partition_point at {cut}"
        );
        assert_eq!(
            TradeSeq::partition_point(tape, |trade| trade.timestamp_ms < cut),
            want
        );
        assert_eq!(
            TradeSeq::partition_point(oracle, |trade| trade.timestamp_ms < cut),
            want
        );
        assert_eq!(
            Bare(oracle).partition_point(|trade| trade.timestamp_ms < cut),
            want
        );
    }
}

/// The engine's golden trade tapes, end to end.
fn golden() -> Vec<Trade> {
    [
        include_str!("fixtures/sample_trades.csv"),
        include_str!("fixtures/tick_trades.csv"),
        include_str!("fixtures/time_trades.csv"),
        include_str!("fixtures/volume_trades.csv"),
        include_str!("fixtures/dollar_trades.csv"),
        include_str!("fixtures/imbalance_trades.csv"),
    ]
    .iter()
    .flat_map(|text| parse_trades(text).expect("a golden fixture parses"))
    .collect()
}

#[test]
fn a_tape_reads_like_the_vec_it_replaces() {
    let mut tape = TradeTape::new();
    let mut oracle: Vec<Trade> = Vec::new();
    assert_reads_like(&tape, &oracle);
    let mut sizes = Lcg(0x5eed);
    let mut next = 0;
    // Irregular appends: a print at a time, small batches, a chunk and a
    // half at once, and one batch ending exactly on a boundary.
    for round in 0..24 {
        match round % 6 {
            0 => {
                for _ in 0..1 + sizes.next(300) {
                    tape.push(trade(next));
                    oracle.push(trade(next));
                    next += 1;
                }
            }
            1 | 4 => {
                let batch: Vec<Trade> = (next..next + 1 + sizes.next(5_000)).map(trade).collect();
                next += batch.len();
                tape.extend_from_slice(&batch);
                oracle.extend_from_slice(&batch);
            }
            2 => {
                let batch: Vec<Trade> = (next..next + CHUNK_TRADES + CHUNK_TRADES / 2)
                    .map(trade)
                    .collect();
                next += batch.len();
                tape.extend_from_slice(&batch);
                oracle.extend_from_slice(&batch);
            }
            3 => {
                let to_boundary = CHUNK_TRADES - oracle.len() % CHUNK_TRADES;
                let batch: Vec<Trade> = (next..next + to_boundary).map(trade).collect();
                next += batch.len();
                tape.extend_from_slice(&batch);
                oracle.extend_from_slice(&batch);
            }
            _ => {
                tape.extend_from_slice(&[]);
            }
        }
        assert_reads_like(&tape, &oracle);
    }
    assert!(
        oracle.len() > 5 * CHUNK_TRADES,
        "the run crossed several chunks"
    );
}

#[test]
fn prepending_reads_like_joining_the_vecs() {
    let mut sizes = Lcg(0xbeef);
    for (held, older) in [
        (0, 10),
        (10, 0),
        (CHUNK_TRADES - 1, 1),
        (1, CHUNK_TRADES - 1),
        (CHUNK_TRADES, CHUNK_TRADES),
        (2 * CHUNK_TRADES + 17, CHUNK_TRADES + 3),
        (sizes.next(3 * CHUNK_TRADES), sizes.next(2 * CHUNK_TRADES)),
    ] {
        // Older prints carry smaller ids and stamps, as a history page does.
        let oldest: Vec<Trade> = (0..older).map(trade).collect();
        let newest: Vec<Trade> = (older..older + held).map(trade).collect();
        let mut tape: TradeTape = newest.iter().cloned().collect();
        tape.prepend(&oldest);
        let oracle: Vec<Trade> = oldest.iter().chain(&newest).cloned().collect();
        assert_reads_like(&tape, &oracle);
        // And the joined tape keeps growing like the vector would.
        tape.push(trade(older + held));
        let mut grown = oracle;
        grown.push(trade(older + held));
        assert_reads_like(&tape, &grown);
    }
}

#[test]
fn the_golden_tapes_read_back_unchanged_across_chunks() {
    let golden = golden();
    assert!(!golden.is_empty());
    // Repeated, with ids and stamps moved on each time, until the tape
    // crosses three chunk boundaries.
    let span = golden.last().expect("not empty").timestamp_ms - golden[0].timestamp_ms + 1;
    let mut oracle = Vec::new();
    let mut lap = 0;
    while oracle.len() <= 3 * CHUNK_TRADES {
        oracle.extend(golden.iter().map(|trade| Trade {
            agg_id: trade.agg_id + lap * 1_000_000,
            timestamp_ms: trade.timestamp_ms + lap as i64 * span,
            ..trade.clone()
        }));
        lap += 1;
    }
    let mut tape = TradeTape::new();
    for batch in oracle.chunks(golden.len()) {
        tape.extend_from_slice(batch);
    }
    assert_reads_like(&tape, &oracle);
    let pushed: TradeTape = oracle.iter().cloned().collect();
    assert_reads_like(&pushed, &oracle);
}

#[test]
#[should_panic(expected = "out of range")]
fn indexing_past_the_end_panics_like_a_vec() {
    let tape: TradeTape = (0..3).map(trade).collect();
    let _ = &tape[3];
}

#[test]
#[should_panic(expected = "out of range")]
fn a_range_past_the_end_panics_like_a_vec() {
    let tape: TradeTape = (0..3).map(trade).collect();
    let _ = tape.range(1..4);
}

#[test]
#[should_panic(expected = "starts at 2 but ends at 1")]
fn a_range_out_of_order_panics_like_a_vec() {
    let tape: TradeTape = (0..3).map(trade).collect();
    #[allow(clippy::reversed_empty_ranges)]
    let _ = tape.range(2..1);
}

/// The stall this type exists to remove: growing the tape must never move a
/// print it already holds — a moved print is a copied tape.
#[test]
fn appending_never_moves_a_trade_already_held() {
    let mut tape = TradeTape::new();
    let mut seen: Vec<(usize, *const Trade)> = Vec::new();
    let mut sizes = Lcg(0xfeed);
    let total = 3 * CHUNK_TRADES + 17;
    while tape.len() < total {
        let start = tape.len();
        if sizes.next(2) == 0 {
            tape.push(trade(start));
        } else {
            let batch: Vec<Trade> = (start..(start + 1 + sizes.next(20_000)).min(total))
                .map(trade)
                .collect();
            tape.extend_from_slice(&batch);
        }
        // Where each print just appended lives, sampled, and every chunk's
        // first and last position.
        for p in start..tape.len() {
            if p % 4_096 == 0 || p % CHUNK_TRADES == CHUNK_TRADES - 1 || p + 1 == tape.len() {
                seen.push((p, &raw const tape[p]));
            }
        }
    }
    for (p, address) in seen {
        assert!(
            std::ptr::eq(&raw const tape[p], address),
            "print {p} moved when the tape grew to {}",
            tape.len()
        );
    }
}

/// What the chunks cost in memory: at most one chunk held and unused, where
/// a doubling vector holds up to the whole tape again.
#[test]
fn the_tape_reserves_at_most_one_chunk_it_does_not_use() {
    for len in [
        1,
        CHUNK_TRADES - 1,
        CHUNK_TRADES,
        CHUNK_TRADES + 1,
        2 * CHUNK_TRADES + 1,
        3 * CHUNK_TRADES + 17,
    ] {
        let pushed: TradeTape = (0..len).map(trade).collect();
        let mut extended = TradeTape::new();
        extended.extend_from_slice(&(0..len).map(trade).collect::<Vec<_>>());
        let mut prepended: TradeTape = (len / 2..len).map(trade).collect();
        prepended.prepend(&(0..len / 2).map(trade).collect::<Vec<_>>());
        for (how, tape) in [
            ("pushed", pushed),
            ("extended", extended),
            ("prepended", prepended),
        ] {
            assert_eq!(tape.len(), len);
            assert!(
                tape.capacity() - len < CHUNK_TRADES,
                "{how} to {len}: {} prints reserved and unused",
                tape.capacity() - len
            );
        }
    }
    assert_eq!(
        TradeTape::new().capacity(),
        0,
        "an empty tape holds nothing"
    );
}

/// A bulk reader gets the stretch it asked for as one slice per chunk the
/// stretch touches — contiguous memory, without the tape ever being one.
#[test]
fn a_stretch_comes_back_as_one_slice_per_chunk() {
    let len = 3 * CHUNK_TRADES + 17;
    let tape: TradeTape = (0..len).map(trade).collect();
    for &start in &positions(len) {
        for &end in positions(len).iter().filter(|&&end| end >= start) {
            let slices: Vec<&[Trade]> = tape.slices(start..end).collect();
            let touched = if start == end {
                0
            } else {
                (end - 1) / CHUNK_TRADES - start / CHUNK_TRADES + 1
            };
            assert_eq!(slices.len(), touched, "slices({start}..{end})");
            assert!(slices.iter().all(|slice| slice.len() <= CHUNK_TRADES));
        }
    }
}
