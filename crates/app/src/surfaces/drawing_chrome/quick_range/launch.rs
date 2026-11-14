//! Capture and staging adapter proofs for the feature-gated quick-range host.
#![cfg(any(feature = "quick-range-harness", test))]
#[cfg(test)]
use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_modes_reach_the_recipient_once_without_another_read() {
        for (value, expected) in [
            ("active", (false, false)),
            ("1", (true, false)),
            (" ready ", (true, false)),
            ("future", (true, true)),
        ] {
            let mut reads = Vec::new();
            let input = QuickRangeLaunch::capture(|name| {
                reads.push(name.to_owned());
                Some(value.into())
            });
            assert_eq!(reads, ["QUANTICK_QUICK_RANGE_DEMO"]);
            let mut calls = Vec::new();
            input.deliver(|request| calls.push((request.ready, request.future)));
            assert_eq!(calls, [expected]);
        }
    }

    #[test]
    fn absent_or_unknown_modes_do_not_invent_a_request() {
        for value in [None, Some(""), Some("READY"), Some("unknown")] {
            let input = QuickRangeLaunch::capture(|_| value.map(OsString::from));
            input.deliver(|_| panic!("an invalid input must not request a scenario"));
        }
    }
}

#[cfg(test)]
mod stage_tests;
