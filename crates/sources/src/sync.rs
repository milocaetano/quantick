//! A chart out of sync with its feed, in the words that pause a new entry.
//!
//! The judgement is the feed host's (`quantick_feed::sync`), told the time.
//! This is the vocabulary it answers in, kept here because the tab's paper
//! desk (`quantick_session`) refuses entries on it and the control plane
//! reports it, and neither may name the feed host.

use serde::Deserialize;

/// Arrival latency, in milliseconds, at which new entries pause by default.
pub const DEFAULT_MAX_LATENCY_MS: i64 = 500;
/// Tape age, in milliseconds, at which new entries pause by default.
pub const DEFAULT_MAX_TAPE_AGE_MS: i64 = 5_000;

/// How far behind its feed a chart may fall before new entries pause: a
/// `[[feeds]]` entry's `sync` table. Each limit is reached at, not past.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SyncLimits {
    /// The provider's measured arrival latency.
    pub max_latency_ms: i64,
    /// Wall clock minus the newest event on the tape.
    pub max_tape_age_ms: i64,
}

impl Default for SyncLimits {
    fn default() -> Self {
        Self {
            max_latency_ms: DEFAULT_MAX_LATENCY_MS,
            max_tape_age_ms: DEFAULT_MAX_TAPE_AGE_MS,
        }
    }
}

impl SyncLimits {
    /// Refuse a limit no reading could stay under.
    ///
    /// # Errors
    ///
    /// The offending key, in the config's own words.
    pub fn validate(&self, feed_id: &str) -> Result<(), String> {
        for (key, value) in [
            ("max_latency_ms", self.max_latency_ms),
            ("max_tape_age_ms", self.max_tape_age_ms),
        ] {
            if value < 1 {
                return Err(format!(
                    "feed '{feed_id}' sets sync.{key} to {value} - a limit under 1 ms \
                     would pause every entry"
                ));
            }
        }
        Ok(())
    }
}

/// The chart's link to the feed, in the three words the gate reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// Connected: live or quiet.
    Up,
    /// The transport is trying.
    Attempting,
    /// The chart's stall judgement fired and latched.
    Offline,
}

impl LinkState {
    /// The stable wire word.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Attempting => "attempting",
            Self::Offline => "offline",
        }
    }
}

/// Which reading paused entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncReading {
    Link,
    Latency,
    TapeAge,
}

impl SyncReading {
    /// The stable wire word.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Latency => "latency",
            Self::TapeAge => "tape_age",
        }
    }
}

/// Why new entries are paused: the reading that tripped first and every
/// reading beside it, as measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desync {
    pub reading: SyncReading,
    pub link: LinkState,
    /// The provider's measured arrival latency; `None` without a split.
    pub latency_ms: Option<i64>,
    /// The provider's name for the hop that owns most of it.
    pub latency_hop: Option<&'static str>,
    pub tape_age_ms: Option<i64>,
    pub limits: SyncLimits,
    /// What the feed's own notice says, when it says anything.
    pub notice: Option<String>,
}

impl Desync {
    /// Whether the pause offers its own Reconnect beside the tape: only on a
    /// link that is up, late or stale. A link that is down already has the
    /// status word's and the overlay's, and an attempt shows no control.
    #[must_use]
    pub fn offers_reconnect(&self) -> bool {
        self.link == LinkState::Up
    }

    /// The refusal a paused entry answers with: the cause, then what still
    /// works and where the way out is.
    #[must_use]
    pub fn sentence(&self) -> String {
        let cause = match self.reading {
            SyncReading::Link if self.link == LinkState::Offline => {
                "the feed is offline".to_owned()
            }
            SyncReading::Link => "the feed is not connected".to_owned(),
            SyncReading::Latency => {
                let hop = self
                    .latency_hop
                    .map(|hop| format!(", most of it in {hop}"))
                    .unwrap_or_default();
                format!(
                    "prints arrive {} late (limit {}){hop}",
                    spoken(self.latency_ms.unwrap_or_default()),
                    spoken(self.limits.max_latency_ms)
                )
            }
            SyncReading::TapeAge => format!(
                "the newest print is {} old (limit {})",
                spoken(self.tape_age_ms.unwrap_or_default()),
                spoken(self.limits.max_tape_age_ms)
            ),
        };
        let notice = self
            .notice
            .as_deref()
            .map(|notice| format!(" - {notice}"))
            .unwrap_or_default();
        format!(
            "new entries paused, the chart is out of sync with the feed: {cause}{notice}. \
             Exits still work; Reconnect sits beside the tape"
        )
    }
}

/// Milliseconds under a second, else seconds to one decimal.
fn spoken(ms: i64) -> String {
    if ms.abs() < 1_000 {
        format!("{ms} ms")
    } else {
        format!("{}.{} s", ms / 1_000, (ms % 1_000).abs() / 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desync(reading: SyncReading) -> Desync {
        Desync {
            reading,
            link: LinkState::Up,
            latency_ms: Some(900),
            latency_hop: Some("MT5"),
            tape_age_ms: Some(7_250),
            limits: SyncLimits::default(),
            notice: None,
        }
    }

    #[test]
    fn each_sentence_names_its_reading_value_and_limit() {
        let latency = desync(SyncReading::Latency).sentence();
        assert!(
            latency.contains("prints arrive 900 ms late (limit 500 ms), most of it in MT5"),
            "{latency}"
        );
        let age = desync(SyncReading::TapeAge).sentence();
        assert!(
            age.contains("the newest print is 7.2 s old (limit 5.0 s)"),
            "{age}"
        );
        assert!(desync(SyncReading::Latency).offers_reconnect());
        let offline = Desync {
            link: LinkState::Offline,
            notice: Some("MetaTrader 5 disconnected".to_owned()),
            ..desync(SyncReading::Link)
        };
        let said = offline.sentence();
        assert!(
            said.contains("the feed is offline - MetaTrader 5 disconnected. Exits still work"),
            "{said}"
        );
        assert!(!offline.offers_reconnect(), "offline offers its own");
    }

    #[test]
    fn a_limit_under_a_millisecond_is_refused_at_load() {
        assert_eq!(SyncLimits::default().validate("mt5"), Ok(()));
        let zero = SyncLimits {
            max_tape_age_ms: 0,
            ..SyncLimits::default()
        };
        let refused = zero.validate("mt5").expect_err("zero pauses everything");
        assert!(refused.contains("sync.max_tape_age_ms"), "{refused}");
    }
}
