//! How a number is spelled where a person reads it.
//!
//! Every painted value — a price, a quantity, a point count, an indicator
//! reading — shows at most [`DISPLAY_DECIMALS`] places, trailing zeros
//! stripped, so a computed `1.3333333333` reads `1.33` and never widens a
//! label. One exception keeps data honest: a non-zero value too small to
//! survive that rounding shows [`SMALL_SIGNIFICANT_DIGITS`] significant
//! digits (`0.004`), never a `0` that would hide a real size or move.
//! Presentation only: the stored and transmitted values stay exact.

use rust_decimal::Decimal;

/// Most decimal places a painted number shows.
pub const DISPLAY_DECIMALS: u32 = 2;

/// Significant digits kept by a non-zero value below one display step.
pub const SMALL_SIGNIFICANT_DIGITS: u32 = 2;

/// Most decimal places a sub-step float is searched to before it counts
/// as zero.
const F64_MAX_DECIMALS: usize = 12;

/// `value` as displayed: rounded to [`DISPLAY_DECIMALS`] places, or to
/// [`SMALL_SIGNIFICANT_DIGITS`] significant digits when that rounding
/// would erase a non-zero value.
pub fn round_for_display(value: Decimal) -> Decimal {
    let mut places = DISPLAY_DECIMALS;
    let mut rounded = value.round_dp(places);
    if rounded.is_zero() && !value.is_zero() {
        while value.round_dp(places).is_zero() && places < value.scale() {
            places += 1;
        }
        rounded = value.round_dp(places + SMALL_SIGNIFICANT_DIGITS - 1);
    }
    rounded.normalize()
}

/// A decimal for display: at most two places, trailing zeros stripped.
pub fn display_decimal(value: Decimal) -> String {
    let rounded = round_for_display(value);
    if rounded.is_zero() {
        return "0".to_owned();
    }
    rounded.to_string()
}

/// A float for display, by the same rule as [`display_decimal`];
/// non-finite values keep their own spelling.
pub fn display_f64(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let mut places = DISPLAY_DECIMALS as usize;
    let mut text = fixed(value, places);
    if text == "0" && value != 0.0 {
        while fixed(value, places) == "0" && places < F64_MAX_DECIMALS {
            places += 1;
        }
        if places < F64_MAX_DECIMALS {
            text = fixed(value, places + SMALL_SIGNIFICANT_DIGITS as usize - 1);
        }
    }
    text
}

/// `value` with `places` decimals, trailing zeros and a signed zero dropped.
fn fixed(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
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

/// [`display_f64`] with an explicit `+` on gains, for a signed move.
pub fn display_signed_f64(value: f64) -> String {
    let text = display_f64(value);
    if value > 0.0 && text != "0" {
        format!("+{text}")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_carry_at_most_two_places() {
        let third = Decimal::from(4) / Decimal::from(3);
        assert_eq!(display_decimal(third), "1.33");
        assert_eq!(display_decimal(Decimal::new(510, 2)), "5.1");
        assert_eq!(display_decimal(Decimal::from(12)), "12");
        assert_eq!(display_decimal(Decimal::ZERO), "0");
        assert_eq!(display_decimal(Decimal::new(-2345, 3)), "-2.34");
    }

    #[test]
    fn a_small_non_zero_decimal_is_never_painted_as_zero() {
        assert_eq!(display_decimal(Decimal::new(4, 3)), "0.004");
        assert_eq!(display_decimal(Decimal::new(12_345, 8)), "0.00012");
        assert_eq!(display_decimal(Decimal::new(-5, 3)), "-0.005");
    }

    #[test]
    fn floats_carry_at_most_two_places() {
        assert_eq!(display_f64(4.0 / 3.0), "1.33");
        assert_eq!(display_f64(5.10), "5.1");
        assert_eq!(display_f64(12.0), "12");
        assert_eq!(display_f64(1500.0), "1500");
        assert_eq!(display_f64(0.0), "0");
        assert_eq!(display_f64(-0.0), "0");
        assert_eq!(display_f64(f64::NAN), "NaN");
    }

    #[test]
    fn a_small_non_zero_float_is_never_painted_as_zero() {
        assert_eq!(display_f64(0.004), "0.004");
        assert_eq!(display_f64(0.000_123_4), "0.00012");
        assert_eq!(display_f64(-0.001), "-0.001");
        assert_eq!(display_f64(1e-15), "0");
    }

    #[test]
    fn signed_moves_carry_their_sign() {
        assert_eq!(display_signed_f64(0.5), "+0.5");
        assert_eq!(display_signed_f64(-0.001), "-0.001");
        assert_eq!(display_signed_f64(0.0), "0");
    }
}
