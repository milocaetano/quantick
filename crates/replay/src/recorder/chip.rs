//! The chart-corner chip: what a pane's recording note says, worded from
//! [`RecordingView`] alone so the chip, the REC button and its popover
//! cannot disagree. The window paints it; every word is decided here.

use quantick_engine::bar_registry::BarDefinition;

use super::{RecState, RecordingView, STALE_AFTER_MS, fmt_count, fmt_hms};

/// What the status bar's recording note says for one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealChip {
    pub text: String,
    pub hover: String,
    pub tone: RecState,
}

/// The chip for a pane, or none when the pane has nothing to say — a pane
/// not drawing `trades` on a feed that is not recording.
#[must_use]
pub fn chip_for(
    view: &RecordingView,
    pane_kind: impl Into<&'static BarDefinition>,
    reading_in_pane: bool,
    uncounted_prints: u64,
) -> Option<DealChip> {
    let pane_kind = pane_kind.into();
    let since = view
        .since_ms
        .map(|ms| fmt_hms(ms, view.tz_minutes))
        .unwrap_or_else(|| "—".to_owned());
    let (text, hover) = match view.state {
        RecState::Recording if view.since_ms.is_none() => (
            "recording · waiting for the counter".to_owned(),
            format!(
                "{} is being recorded; no reading has arrived yet this session",
                view.symbol
            ),
        ),
        RecState::Recording => (
            format!("recording · {since} →"),
            format!(
                "{} deals counted since {since} and written to disk; the counter reads \
                 every ~31 s, and the deals of each print in between are estimated",
                view.symbol
            ),
        ),
        RecState::Stale => (
            "recording · counter stale".to_owned(),
            format!(
                "the tape flows but the deal counter has not moved for {} s; prints are still \
                 estimated at the last rate, and have no count after {} min",
                STALE_AFTER_MS / 1000,
                quantick_engine::READING_MAX_AGE_MS / 60_000
            ),
        ),
        RecState::Recorded => (
            format!("recorded · {}", view.loaded_days.join(", ")),
            "the deal counts on this chart come from a recording; nothing is being written"
                .to_owned(),
        ),
        RecState::Off | RecState::Unsupported => {
            if !pane_kind.needs_deal_counter() {
                return None;
            }
            if view.state == RecState::Unsupported {
                // A trades pane on a feed with no counter — a spec restored
                // from a workspace or a config before the hello, or a quoted
                // instrument — is a blank chart unless something says why.
                (
                    "no deal count · this source has no deal counter".to_owned(),
                    "trades bars need MetaTrader B3's session deal counter; this feed declares none"
                        .to_owned(),
                )
            } else if view.counter_stale {
                // The counter stands still while prints keep coming, REC on
                // or off: the trades pane is the one that suffers, so it is
                // the one that says so.
                if view.reading == Some(0) {
                    (
                        "no deal count · counter stuck at 0".to_owned(),
                        "the venue's deal counter reads 0 while the tape moves; this broker may \
                         not count, and trades bars cannot cut"
                            .to_owned(),
                    )
                } else {
                    (
                        "counting · counter stale".to_owned(),
                        format!(
                            "the tape flows but the deal counter has not moved for {} s; prints are \
                             still estimated at the last rate, and have no count after {} min",
                            STALE_AFTER_MS / 1000,
                            quantick_engine::READING_MAX_AGE_MS / 60_000
                        ),
                    )
                }
            } else if view.reading.is_some() {
                // Readings arrive and cut bars whether or not REC is on;
                // what REC adds is the file. Say exactly that.
                (
                    "counting · not written to disk".to_owned(),
                    "bars are cut from the live counter; press REC to keep them for a restart"
                        .to_owned(),
                )
            } else {
                (
                    "no deal count".to_owned(),
                    "press REC to count from now, or load a recorded day".to_owned(),
                )
            }
        }
    };
    let mut text = text;
    let mut hover = hover;
    if pane_kind.needs_deal_counter() && uncounted_prints > 0 {
        text = format!(
            "{text} · {} prints have no count",
            fmt_count(uncounted_prints)
        );
        hover = format!(
            "{hover}. {} prints came before the first two readings, or more than {} min \
             after the newest one before them, and form no trades bar",
            fmt_count(uncounted_prints),
            quantick_engine::READING_MAX_AGE_MS / 60_000
        );
    }
    if pane_kind.needs_deal_counter() && !reading_in_pane && view.state == RecState::Recording {
        hover = format!("{hover}. No reading has reached this pane yet");
    }
    Some(DealChip {
        text,
        hover,
        tone: view.state,
    })
}

#[cfg(test)]
#[path = "chip_tests.rs"]
mod tests;
