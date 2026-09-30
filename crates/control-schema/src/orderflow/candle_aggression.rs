//! Bounded readback of the exact frame used to paint candle aggression.
use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_control_host::wire::{canonical_decimal, wire_usize};
use quantick_orderflow::projection::{CandleDot, CandleDotFrame};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_CANDLE_MARKS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CandleAggressionSnapshot {
    pub candles_per_mark: WireU64,
    pub price_ticks_per_mark: WireU64,
    /// Largest visible eligible band quantity. When opening exclusion is
    /// effective, the reference excludes only exact opening contributions. All
    /// radii use sqrt(quantity / full_quantity) times maximum_radius_px,
    /// except opening overflow explicitly reported by size_capped.
    pub full_quantity: CanonicalDecimal,
    pub maximum_radius_px: CanonicalDecimal,
    pub ignore_opening_burst_in_scale: bool,
    pub opening_exclusion_effective: bool,
    /// First available recorded 100 ms windows per retained UTC date; an
    /// approximation, not evidence of an exchange auction.
    pub recorded_opening_windows_ms: Vec<i64>,
    /// Exact totals of every projected mark, including those omitted by the
    /// readback bound. Price-filtered and unavailable ladders are excluded.
    pub buy_quantity: CanonicalDecimal,
    pub sell_quantity: CanonicalDecimal,
    pub trade_count: WireU64,
    pub mark_count: WireU64,
    pub truncated: bool,
    /// At most 256 marks in absolute slot, then price-band order.
    pub marks: Vec<CandleMarkSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CandleMarkSnapshot {
    pub slot: WireU64,
    pub last_slot: WireU64,
    /// Quantity-weighted execution price within this price band.
    pub price: CanonicalDecimal,
    /// Actual contributing execution span, not synthetic band edges.
    pub price_low: CanonicalDecimal,
    pub price_high: CanonicalDecimal,
    pub buy_quantity: CanonicalDecimal,
    pub sell_quantity: CanonicalDecimal,
    pub trade_count: WireU64,
    /// Exact decimal representation of the painted f32 logical radius.
    pub radius_px: CanonicalDecimal,
    pub opening_quantity: CanonicalDecimal,
    /// True only for an opening-containing mark whose factual total exceeds
    /// the ordinary reference. Other marks retain proportional areas.
    pub size_capped: bool,
}

fn pixels(value: f32) -> CanonicalDecimal {
    canonical_decimal(Decimal::from_f32_retain(value).unwrap_or(Decimal::ZERO))
}

impl From<&CandleDotFrame> for CandleAggressionSnapshot {
    fn from(frame: &CandleDotFrame) -> Self {
        let (buy, sell, count) = frame.marks.iter().fold(
            (Decimal::ZERO, Decimal::ZERO, 0_u64),
            |(buy, sell, count), mark| {
                (
                    buy.saturating_add(mark.buy_quantity),
                    sell.saturating_add(mark.sell_quantity),
                    count.saturating_add(mark.trade_count),
                )
            },
        );
        Self {
            candles_per_mark: wire_usize(frame.candles_per_mark),
            price_ticks_per_mark: wire_usize(frame.price_ticks_per_mark),
            full_quantity: canonical_decimal(frame.full_quantity),
            maximum_radius_px: pixels(frame.maximum_radius_px),
            ignore_opening_burst_in_scale: frame.ignore_opening_burst_in_scale,
            opening_exclusion_effective: frame.opening_exclusion_effective,
            recorded_opening_windows_ms: frame.recorded_opening_windows_ms.clone(),
            buy_quantity: canonical_decimal(buy),
            sell_quantity: canonical_decimal(sell),
            trade_count: WireU64::new(count),
            mark_count: wire_usize(frame.marks.len()),
            truncated: frame.marks.len() > MAX_CANDLE_MARKS,
            marks: frame
                .marks
                .iter()
                .take(MAX_CANDLE_MARKS)
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<&CandleDot> for CandleMarkSnapshot {
    fn from(mark: &CandleDot) -> Self {
        Self {
            slot: wire_usize(mark.slot),
            last_slot: wire_usize(mark.last_slot),
            price: canonical_decimal(mark.price),
            price_low: canonical_decimal(mark.price_low),
            price_high: canonical_decimal(mark.price_high),
            buy_quantity: canonical_decimal(mark.buy_quantity),
            sell_quantity: canonical_decimal(mark.sell_quantity),
            trade_count: WireU64::new(mark.trade_count),
            radius_px: pixels(mark.radius_px),
            opening_quantity: canonical_decimal(mark.opening_quantity),
            size_capped: mark.size_capped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_marks_keep_full_exact_totals_and_tiny_painted_radii() {
        let mark = CandleDot {
            slot: 0,
            last_slot: 0,
            price: Decimal::ONE,
            price_low: Decimal::ONE,
            price_high: Decimal::ONE,
            buy_quantity: Decimal::new(1, 2),
            sell_quantity: Decimal::ZERO,
            trade_count: 1,
            radius_px: 0.02,
            opening_quantity: Decimal::ZERO,
            size_capped: false,
        };
        let frame = CandleDotFrame {
            marks: vec![mark; 300],
            full_quantity: Decimal::from(100),
            candles_per_mark: 1,
            price_ticks_per_mark: 2,
            maximum_radius_px: 4.0,
            ignore_opening_burst_in_scale: false,
            opening_exclusion_effective: false,
            recorded_opening_windows_ms: Vec::new(),
        };
        let snapshot = CandleAggressionSnapshot::from(&frame);
        assert_eq!(snapshot.marks.len(), 256);
        assert!(snapshot.truncated);
        assert_eq!(snapshot.mark_count, WireU64::new(300));
        assert_eq!(snapshot.buy_quantity, canonical_decimal(Decimal::from(3)));
        assert_eq!(snapshot.trade_count, WireU64::new(300));
        assert_eq!(
            snapshot.marks[0].radius_px,
            pixels(frame.marks[0].radius_px)
        );
        assert_eq!(
            snapshot.full_quantity,
            canonical_decimal(frame.full_quantity)
        );
    }
}
