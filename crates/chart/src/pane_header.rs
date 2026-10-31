//! What a pane's header line says about the pane: its identity and its bars.
//!
//! Built from that pane's own bar selection and nothing else, so the line reads
//! the same wherever focus sits. The window only paints the result.

use quantick_engine::bar_registry::BarConfiguration;

/// The mark naming the order-flow pane.
pub const ORDER_FLOW_MARK: &str = "ORDER FLOW";

/// What a pane is for, as its header line names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneIdentity {
    /// The order-flow pane: heatmap, bubbles and tape.
    OrderFlow,
    /// A timeframe pane: candles, indicators and drawings.
    Time,
}

/// One pane's header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneHeader {
    /// The pane's name, when it needs one. A time pane's presets already say
    /// what it is.
    pub mark: Option<&'static str>,
    /// The bars this pane builds, as the bars control reads.
    pub bars_text: String,
    /// Whether the timeframe presets stand beside the bars control.
    pub presets: bool,
}

impl PaneHeader {
    /// The header line of a pane with this identity building these bars.
    #[must_use]
    pub fn of(identity: PaneIdentity, bars: BarConfiguration) -> Self {
        let presets = identity == PaneIdentity::Time && bars.time_interval_ms().is_some();
        Self {
            mark: (identity == PaneIdentity::OrderFlow).then_some(ORDER_FLOW_MARK),
            // The presets beside it name the interval, so the kind says enough.
            bars_text: if presets {
                kind_name(bars)
            } else {
                format!("{} · {}", kind_name(bars), bars.parameter_summary())
            },
            presets,
        }
    }
}

/// The bar kind as a header line writes it: its registered label, capitalised.
fn kind_name(bars: BarConfiguration) -> String {
    let label = bars.definition().label();
    let mut letters = label.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::BarSpec;
    use rust_decimal::Decimal;

    fn header(identity: PaneIdentity, spec: BarSpec) -> PaneHeader {
        PaneHeader::of(identity, spec.into())
    }

    fn expected(mark: Option<&'static str>, bars_text: &str, presets: bool) -> PaneHeader {
        PaneHeader {
            mark,
            bars_text: bars_text.to_owned(),
            presets,
        }
    }

    #[test]
    fn the_flow_pane_is_marked_and_writes_its_bars_in_full() {
        assert_eq!(
            header(PaneIdentity::OrderFlow, BarSpec::Tick(2000)),
            expected(Some("ORDER FLOW"), "Tick · 2000", false)
        );
        assert_eq!(
            header(PaneIdentity::OrderFlow, BarSpec::Time(60_000)),
            expected(Some("ORDER FLOW"), "Time · 1m", false),
            "the flow pane has no presets to name the interval"
        );
        assert_eq!(
            header(PaneIdentity::OrderFlow, BarSpec::Volume(Decimal::from(50))),
            expected(Some("ORDER FLOW"), "Volume · 50.0", false)
        );
    }

    #[test]
    fn a_time_pane_on_time_bars_keeps_its_presets_and_says_only_the_kind() {
        assert_eq!(
            header(PaneIdentity::Time, BarSpec::Time(60_000)),
            expected(None, "Time", true)
        );
        assert_eq!(
            header(PaneIdentity::Time, BarSpec::Time(3_600_000)),
            expected(None, "Time", true),
            "the interval belongs to the presets"
        );
    }

    #[test]
    fn a_time_pane_on_other_bars_drops_the_presets_it_would_misstate() {
        assert_eq!(
            header(PaneIdentity::Time, BarSpec::Tick(4)),
            expected(None, "Tick · 4", false)
        );
    }
}
