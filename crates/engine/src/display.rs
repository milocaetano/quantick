//! How a number is spelled where a person reads it.
//!
//! Every painted value — a price, a quantity, a point count, an indicator
//! reading — shows at most [`DISPLAY_DECIMALS`] places, trailing zeros
//! stripped, so a computed `1.3333333333` reads `1.33` and never widens a
//! label. Presentation only: the stored and transmitted values stay exact.

use rust_decimal::{Decimal, RoundingStrategy};

/// Most decimal places a painted number shows.
pub const DISPLAY_DECIMALS: u32 = 2;

/// `value` rounded half away from zero to [`DISPLAY_DECIMALS`] places.
pub fn round_for_display(value: Decimal) -> Decimal {
    value
        .round_dp_with_strategy(DISPLAY_DECIMALS, RoundingStrategy::MidpointAwayFromZero)
        .normalize()
}

/// A decimal for display: at most two places, trailing zeros stripped.
pub fn display_decimal(value: Decimal) -> String {
    let rounded = round_for_display(value);
    if rounded.is_zero() {
        return "0".to_owned();
    }
    rounded.to_string()
}

/// A float for display: at most two places, trailing zeros stripped;
/// non-finite values keep their own spelling.
pub fn display_f64(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let text = format!("{value:.prec$}", prec = DISPLAY_DECIMALS as usize);
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    match text {
        "-0" => "0".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_carry_at_most_two_places() {
        let third = Decimal::from(4) / Decimal::from(3);
        assert_eq!(display_decimal(third), "1.33");
        assert_eq!(display_decimal(Decimal::new(-5, 3)), "-0.01");
        assert_eq!(display_decimal(Decimal::new(-4, 3)), "0");
        assert_eq!(display_decimal(Decimal::new(510, 2)), "5.1");
        assert_eq!(display_decimal(Decimal::from(12)), "12");
        assert_eq!(display_decimal(Decimal::ZERO), "0");
    }

    #[test]
    fn floats_carry_at_most_two_places() {
        assert_eq!(display_f64(4.0 / 3.0), "1.33");
        assert_eq!(display_f64(5.10), "5.1");
        assert_eq!(display_f64(12.0), "12");
        assert_eq!(display_f64(-0.001), "0");
        assert_eq!(display_f64(1500.0), "1500");
        assert_eq!(display_f64(f64::NAN), "NaN");
    }
}
