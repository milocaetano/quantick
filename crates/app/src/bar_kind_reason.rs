//! Why a bar kind is listed but not offered on this source — the one owner
//! of the combo's disabled hover, so the rule and its reason cannot drift.

use crate::state::BarKind;

pub(crate) struct BarInputAvailability {
    pub traded_volume: bool,
    pub deal_counter: bool,
    pub deal_count: bool,
}

/// `None` when the kind is usable here; otherwise the hover a trader reads
/// off the disabled entry.
///
/// A count in hand — a recorded day, a resumed file — makes a deal-counted
/// kind usable whatever the hello said: the REC popover's *Show as trades*
/// offers it then, and the combo must not call the same kind impossible.
pub(crate) fn disabled_reason(
    kind: BarKind,
    inputs: &BarInputAvailability,
) -> Option<&'static str> {
    if kind.needs_traded_volume() && !inputs.traded_volume {
        Some("this source quotes prices but prints no traded volume")
    } else if kind.needs_deal_counter() && !inputs.deal_count {
        Some(if inputs.deal_counter {
            "no deal count yet — press REC to count from now, or load a recorded day"
        } else {
            "this source has no deal counter — MetaTrader B3 only"
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(traded_volume: bool, deal_counter: bool, deal_count: bool) -> BarInputAvailability {
        BarInputAvailability {
            traded_volume,
            deal_counter,
            deal_count,
        }
    }

    #[test]
    fn a_loaded_day_offers_trades_even_before_the_hello() {
        assert_eq!(
            disabled_reason(BarKind::Trades, &inputs(false, false, true)),
            None
        );
    }

    #[test]
    fn without_a_count_the_reason_names_what_is_missing() {
        assert!(
            disabled_reason(BarKind::Trades, &inputs(false, false, false))
                .is_some_and(|reason| reason.contains("no deal counter"))
        );
        assert!(
            disabled_reason(BarKind::Trades, &inputs(false, true, false))
                .is_some_and(|reason| reason.contains("press REC"))
        );
    }

    #[test]
    fn a_quoted_tape_refuses_the_kinds_that_count_size() {
        assert!(disabled_reason(BarKind::Volume, &inputs(false, true, true)).is_some());
        assert_eq!(
            disabled_reason(BarKind::Tick, &inputs(false, false, false)),
            None
        );
    }
}
