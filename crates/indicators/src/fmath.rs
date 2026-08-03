//! The only transcendental call site in the crate.
//!
//! IEEE-754 `+ - * /`, `sqrt` and comparisons are bit-exact on every
//! platform, but `f64::powf`, `exp`, `ln`, `log10` are *not* — std defers to
//! the platform libm, and different platforms round differently. That would
//! break the determinism rule (same bars in, same plots out, bit-exact,
//! golden-tested in CI on every platform CI covers).
//!
//! So every transcendental goes through this module, which delegates to the
//! pure-Rust `libm` crate. A grep-guard test (`tests/fmath_guard.rs`) fails
//! the build the moment anyone calls the std versions elsewhere in `src/`.

/// `base` raised to `exponent` (`math.pow`).
#[must_use]
pub fn pow(base: f64, exponent: f64) -> f64 {
    libm::pow(base, exponent)
}

/// `e^x` (`math.exp`).
#[must_use]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// Natural logarithm (`math.log`).
#[must_use]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

/// Base-10 logarithm (`math.log10`).
#[must_use]
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrappers_agree_with_known_values() {
        assert_eq!(pow(2.0, 10.0), 1024.0);
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(ln(1.0), 0.0);
        assert_eq!(log10(1000.0), 3.0);
    }

    #[test]
    fn nan_propagates() {
        assert!(pow(f64::NAN, 2.0).is_nan());
        assert!(exp(f64::NAN).is_nan());
        assert!(ln(f64::NAN).is_nan());
        assert!(log10(f64::NAN).is_nan());
    }

    #[test]
    fn domain_errors_are_nan_not_panics() {
        assert!(ln(-1.0).is_nan());
        assert!(log10(-1.0).is_nan());
    }
}
