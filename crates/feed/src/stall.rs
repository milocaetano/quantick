//! When a feed that is *working on it* has been working on it for too long.
//!
//! [`FeedNotice`] is the feed's own voice: it says what the transport is doing
//! and, when a provider knows a specific reason, what to do about it. What it
//! cannot say is that nothing is happening, because nothing happening produces
//! no event to report. A first connection that never lands, a reconnect loop
//! that never closes, a socket that stays open while the terminal behind it
//! stops sending — all three look, from inside the feed, exactly like a step in
//! progress. The chart sat on "connecting to WINV26 …" for as long as the
//! trader let it, and their way out was to close the application.
//!
//! So this module is the chart's own judgement on top of the feed's report:
//! given how long the current state has stood and how old the newest event is,
//! decide whether progress has become a stall, and name the control that fixes
//! it. It is a pure function *told* the time — the caller reads the clock, the
//! decision never does — so the whole escalation is unit-testable across a
//! budget without waiting for one.
//!
//! It deliberately does **not** write into [`FeedNotice`]. An escalation is an
//! inference, and `FeedNotice::Attention` is always shown, over a working chart
//! included; a market that is simply closed overnight would then paint an
//! instruction across a chart with nothing wrong with it. A [`Stall`] is a
//! separate value the interface places by its own rules: on the empty pane that
//! is waiting, and in the status bar, which is where a full chart's trouble has
//! always been reported.

use crate::config::ProviderKind;

use super::{FeedConnectionState, FeedNotice};

/// How long a first connection may be in progress before the trader is told it
/// has failed rather than that it is working.
///
/// Generous on purpose: a MetaTrader bridge autostart waits three seconds for
/// an existing bridge and retries every five, and a terminal that is still
/// starting up resolves inside that. Past this the remaining causes all need a
/// human, and the useful thing to show is the reason, not the spinner.
pub const FIRST_CONNECT_BUDGET_MS: i64 = 30_000;

/// How long a reconnect loop may run before the same.
///
/// Shorter than the first-connect budget because the situation is different:
/// this transport worked a moment ago, so a reconnect that has not closed in
/// twenty seconds is not a slow start, it is something that changed.
pub const RECONNECT_BUDGET_MS: i64 = 20_000;

/// How long an established transport may deliver nothing before the chart calls
/// it stalled.
///
/// This is the frozen-terminal case, and the only one where the transport looks
/// perfectly healthy from both ends: MetaTrader stops publishing, the socket
/// stays open, and the chart shows bars that quietly stopped being current.
///
/// Two minutes rather than seconds, because silence is also what a closed
/// market looks like and the reading has to survive an overnight session
/// without crying wolf. That is also why the headline this produces states the
/// observation ("has sent nothing for 2 min") instead of a diagnosis: at 03:00
/// on a closed exchange it is still true.
pub const SILENT_BUDGET_MS: i64 = 120_000;

/// Which control gets the trader out of this particular stall.
///
/// The pair exists because the two acts have genuinely different costs, and the
/// application picks between them rather than asking the trader to diagnose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Respawn the transport and keep the timeline: the bars, drawings,
    /// indicators, armed strategies and any open paper position all survive.
    /// Nothing on screen moves. For a connection that never landed or dropped.
    Reconnect,
    /// Throw the timeline away and rebuild it from zero. Refetches history,
    /// flattens the paper position and disarms every strategy — so it is what
    /// the trader is offered only when the cheap act cannot help. For a
    /// transport that claims to be connected while nothing comes down it.
    Reload,
}

impl Recovery {
    /// The word on the button.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Reconnect => "Reconnect",
            Self::Reload => "Reload",
        }
    }

    /// The other one.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::Reconnect => Self::Reload,
            Self::Reload => Self::Reconnect,
        }
    }
}

/// A feed that has stopped making progress, in words a trader can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stall {
    /// What was observed, stated as an observation rather than a diagnosis.
    pub headline: String,
    /// The one next step.
    pub next_step: String,
    /// The control that addresses this stall; the other stays available.
    pub primary: Recovery,
    /// Whether this is unambiguously wrong, or merely observed.
    ///
    /// A transport that never landed or dropped is broken however you read it,
    /// and the interface says so in amber. Silence is different: it is equally
    /// what a closed market looks like, and an exchange shuts every night. A
    /// permanently amber control is one the trader learns to stop seeing, so
    /// the quiet case offers the same two controls in the interface's ordinary
    /// colours and lets the tape cell — which already turns
    /// the theme's warning colour past ten seconds — carry the warning it has
    /// always carried.
    pub needs_attention: bool,
}

/// Everything the judgement needs, gathered by the caller so the decision
/// itself touches no clock and no application state.
#[derive(Debug, Clone, Copy)]
pub struct StallInput<'a> {
    /// The feed's own current notice.
    pub notice: &'a FeedNotice,
    /// Wall clock when the transport last changed state.
    pub connection_since_ms: i64,
    /// The provider-neutral transport state.
    pub connection: FeedConnectionState,
    /// Age of the newest event on the tape, or `None` when nothing has arrived.
    pub tape_age_ms: Option<i64>,
    /// Wall clock when this feed session was attached.
    pub attached_ms: i64,
    /// Which provider is behind it — for the one line that differs per venue.
    pub provider: ProviderKind,
    /// What the status bar calls this feed, e.g. `MetaTrader 5 — B3`.
    pub provider_name: &'a str,
    /// A recorded session is playing. There is no transport to recover.
    pub replaying: bool,
}

/// Decide whether `input` has stopped being progress, as of `now_ms`.
///
/// `None` means "still working, or nothing to add". The order of the arms is
/// the point: a provider that named its own reason is never overridden by this
/// module's generic one, and a replay is never diagnosed at all.
#[must_use]
pub fn assess(input: &StallInput<'_>, now_ms: i64) -> Option<Stall> {
    // A recording has no socket, no terminal and no reconnect. Its "silence" is
    // the gap between two recorded prints, which is data, not trouble.
    if input.replaying {
        return None;
    }
    // The provider knows something specific — the terminal is closed, the
    // package is missing, the contract does not exist. That reason is better
    // than anything inferred here, and the card already shows it with a
    // control on it.
    if matches!(input.notice, FeedNotice::Attention { .. }) {
        return None;
    }
    let provider = input.provider;
    let name = input.provider_name;
    match input.connection {
        FeedConnectionState::Connecting => {
            let waited = now_ms.saturating_sub(input.attached_ms);
            (waited >= FIRST_CONNECT_BUDGET_MS).then(|| never_connected(provider, name, waited))
        }
        FeedConnectionState::Reconnecting => {
            let waited = now_ms.saturating_sub(input.connection_since_ms);
            (waited >= RECONNECT_BUDGET_MS).then(|| never_came_back(provider, name, waited))
        }
        // Connected and quiet. Reconnecting a socket that is already open
        // fixes nothing; what the trader does by hand today is close the
        // application and start it again, which is this branch's control.
        FeedConnectionState::Connected => {
            // A tape that has *never* printed is silent in the way that
            // matters, and the reading this arm used to require is precisely
            // the one it does not have. Measured from when the transport
            // landed instead: a socket open for four minutes with nothing
            // down it says the same thing as one that went quiet four minutes
            // ago, and the trader needs the same way out of both.
            //
            // Before this it returned `None` forever, which was survivable
            // only because a card drew on any empty pane and the status bar
            // carried a recovery pair. Both are gone — the corner is the one
            // recovery surface now — so the branch that reports nothing is
            // the branch that leaves a chart with no way out.
            let silent = input
                .tape_age_ms
                .unwrap_or_else(|| now_ms.saturating_sub(input.connection_since_ms));
            (silent >= SILENT_BUDGET_MS).then(|| gone_quiet(provider, name, silent))
        }
    }
}

/// A first connection that never landed.
#[must_use]
fn never_connected(provider: ProviderKind, name: &str, waited_ms: i64) -> Stall {
    Stall {
        headline: format!("{name} has not connected in {}", spoken_ms(waited_ms)),
        next_step: provider.recovery_hint().to_owned(),
        primary: Recovery::Reconnect,
        needs_attention: true,
    }
}

/// A transport that worked, dropped, and has not come back.
#[must_use]
fn never_came_back(provider: ProviderKind, name: &str, waited_ms: i64) -> Stall {
    Stall {
        headline: format!(
            "{name} dropped and has not come back in {}",
            spoken_ms(waited_ms)
        ),
        next_step: provider.recovery_hint().to_owned(),
        primary: Recovery::Reconnect,
        needs_attention: true,
    }
}

/// A socket that stayed open while whatever is behind it stopped sending.
#[must_use]
fn gone_quiet(provider: ProviderKind, name: &str, silent_ms: i64) -> Stall {
    Stall {
        headline: format!("{name} has sent nothing for {}", spoken_ms(silent_ms)),
        next_step: format!(
            "If the market is open, {}",
            lower_first(provider.recovery_hint())
        ),
        primary: Recovery::Reload,
        // Silence alone is not a verdict: the exchange closes every night and
        // this branch fires two minutes later. The controls appear; the alarm
        // does not.
        needs_attention: false,
    }
}

/// Which stall `QUANTICK_FEED_STALL` is asking the chart to show.
///
/// Every recovery surface only appears once a feed has been failing for tens
/// of seconds, which makes it unreachable for a scripted run: `ui-harness`
/// cannot break a bridge, wait two minutes and photograph the result inside a
/// capture. So the hook reaches the state directly.
///
/// It produces the stall through the same three constructors [`assess`] uses,
/// so what a screenshot shows is the sentence a real stall writes rather than
/// a lookalike that can drift from it. It changes nothing else: the feed keeps
/// running, and pressing either control does exactly what it always does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedStall {
    /// A first connection that never landed.
    FirstConnect,
    /// A transport that dropped and has not come back.
    Reconnect,
    /// A connected transport delivering nothing — the frozen terminal.
    Silent,
}

impl ForcedStall {
    /// Read the hook, once. Unset or unrecognized means no forced stall: a
    /// typo must leave the real judgement running rather than pick a shape.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        match std::env::var("QUANTICK_FEED_STALL").ok()?.as_str() {
            "connecting" => Some(Self::FirstConnect),
            "reconnecting" => Some(Self::Reconnect),
            "silent" => Some(Self::Silent),
            _ => None,
        }
    }

    /// The stall this shape stands for, with each budget's own duration so the
    /// wording is the one the trader would really be reading.
    #[must_use]
    pub fn stall(self, provider: ProviderKind, provider_name: &str) -> Stall {
        match self {
            Self::FirstConnect => never_connected(provider, provider_name, FIRST_CONNECT_BUDGET_MS),
            Self::Reconnect => never_came_back(provider, provider_name, RECONNECT_BUDGET_MS),
            Self::Silent => gone_quiet(provider, provider_name, SILENT_BUDGET_MS),
        }
    }
}

/// Where a duration stops being read in seconds, in seconds.
///
/// Ninety rather than sixty so the reading does not flick to "1 min" the
/// instant it passes a minute, which reads as less precise than the seconds it
/// replaced.
const MINUTES_ABOVE_S: i64 = 90;

/// Where a duration stops being read in minutes, in minutes.
///
/// Ninety, for the same reason [`MINUTES_ABOVE_S`] is ninety seconds. It earns
/// its place on a different case, though: an overnight close is fourteen hours
/// of silence, and a chart that opens on the last session — which is now the
/// ordinary way to open one outside market hours — would otherwise report it
/// as `840 min`, a number nobody converts while looking for whether the data
/// in front of them is current.
const HOURS_ABOVE_MIN: i64 = 90;

/// A duration in the words a status line uses: seconds while seconds still
/// mean something, then minutes, then hours.
///
/// Shared with the gap mark the chart draws and with the status line's own
/// staleness cell, so a four-minute silence is called the same thing by the
/// popup that offers to fix it, the seam that records it and the line that
/// measures it.
///
/// See [`MINUTES_ABOVE_S`] and [`HOURS_ABOVE_MIN`] for where the readings meet.
#[must_use]
pub fn spoken_ms(ms: i64) -> String {
    let seconds = ms.max(0) / 1_000;
    let minutes = seconds / 60;
    if seconds < MINUTES_ABOVE_S {
        format!("{seconds} s")
    } else if minutes < HOURS_ABOVE_MIN {
        format!("{minutes} min")
    } else {
        // Rounded, where the tiers above truncate. Each of those loses at
        // most one unit of its own scale; an hour truncated loses up to
        // fifty-nine minutes, which is the same order as the reading itself
        // — `1 h` for a tape nearly two hours old is the cell answering "is
        // my data current" with the wrong half of the answer.
        format!("{} h", (minutes + 30) / 60)
    }
}

/// Lower-case the first character, so a sentence written to stand alone can be
/// embedded after a clause. ASCII only by intent: every hint is an English
/// sentence starting with an ASCII letter, and a multi-byte first character
/// would be left exactly as it is rather than mangled.
#[must_use]
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_ascii_uppercase() => {
            first.to_ascii_lowercase().to_string() + chars.as_str()
        }
        _ => text.to_owned(),
    }
}

crate::hooks::declare_hooks!["QUANTICK_FEED_STALL"];

#[cfg(test)]
mod tests {
    use super::*;

    /// A healthy connected feed whose tape is current: the baseline every test
    /// below perturbs one field of.
    fn healthy(notice: &FeedNotice) -> StallInput<'_> {
        StallInput {
            notice,
            connection_since_ms: 0,
            connection: FeedConnectionState::Connected,
            tape_age_ms: Some(50),
            attached_ms: 0,
            provider: ProviderKind::MetaTrader,
            provider_name: "MetaTrader 5 — B3",
            replaying: false,
        }
    }

    #[test]
    fn a_working_feed_says_nothing() {
        let notice = FeedNotice::Clear;
        assert_eq!(assess(&healthy(&notice), 10_000), None);
    }

    #[test]
    fn a_first_connection_escalates_only_after_its_budget() {
        let notice = FeedNotice::working("connecting to MetaTrader 5");
        let input = StallInput {
            connection: FeedConnectionState::Connecting,
            tape_age_ms: None,
            ..healthy(&notice)
        };
        assert_eq!(
            assess(&input, FIRST_CONNECT_BUDGET_MS - 1),
            None,
            "inside the budget this is still progress"
        );
        let stall = assess(&input, FIRST_CONNECT_BUDGET_MS).expect("the budget ran out");
        assert_eq!(stall.primary, Recovery::Reconnect);
        assert!(
            stall.headline.contains("has not connected"),
            "the headline states what was observed: {}",
            stall.headline
        );
        assert!(
            !stall.next_step.is_empty(),
            "an escalation without a next step is the mute card again"
        );
    }

    #[test]
    fn a_reconnect_loop_escalates_on_its_own_shorter_budget() {
        let notice = FeedNotice::reconnecting("MetaTrader 5 disconnected — reconnecting");
        let input = StallInput {
            connection: FeedConnectionState::Reconnecting,
            connection_since_ms: 1_000,
            ..healthy(&notice)
        };
        assert_eq!(assess(&input, 1_000 + RECONNECT_BUDGET_MS - 1), None);
        let stall = assess(&input, 1_000 + RECONNECT_BUDGET_MS).expect("the budget ran out");
        assert_eq!(stall.primary, Recovery::Reconnect);
        assert!(stall.headline.contains("dropped"), "{}", stall.headline);
    }

    /// The frozen terminal: the socket is open, the transport reports
    /// connected, and nothing comes down it. Before this it was reported as
    /// nothing at all.
    #[test]
    fn a_connected_transport_that_went_silent_is_a_stall() {
        let notice = FeedNotice::Clear;
        let input = StallInput {
            tape_age_ms: Some(SILENT_BUDGET_MS),
            ..healthy(&notice)
        };
        let stall = assess(&input, 0).expect("silence past the budget is a stall");
        assert_eq!(
            stall.primary,
            Recovery::Reload,
            "reconnecting a socket that is already open fixes nothing"
        );
        assert!(
            stall.headline.contains("has sent nothing"),
            "{}",
            stall.headline
        );
        assert!(
            !stall.headline.contains("connecting"),
            "a bridge that connected and went quiet is not connecting: {}",
            stall.headline
        );
    }

    /// Silence is also what a closed market looks like, so the next step is
    /// phrased as a check rather than an accusation.
    #[test]
    fn the_silent_next_step_survives_a_closed_market() {
        let notice = FeedNotice::Clear;
        let input = StallInput {
            tape_age_ms: Some(SILENT_BUDGET_MS * 30),
            ..healthy(&notice)
        };
        let stall = assess(&input, 0).expect("still a stall");
        assert!(
            stall.next_step.starts_with("If the market is open,"),
            "{}",
            stall.next_step
        );
    }

    /// The two stalls are not the same news. A broken transport is broken
    /// however you read it; a quiet tape is also a closed exchange, and an
    /// alarm every night is an alarm nobody sees.
    #[test]
    fn silence_offers_the_controls_without_the_alarm() {
        let notice = FeedNotice::Clear;
        let silent = StallInput {
            tape_age_ms: Some(SILENT_BUDGET_MS),
            ..healthy(&notice)
        };
        assert!(!assess(&silent, 0).expect("a stall").needs_attention);

        let connecting = StallInput {
            connection: FeedConnectionState::Connecting,
            tape_age_ms: None,
            ..healthy(&notice)
        };
        assert!(
            assess(&connecting, FIRST_CONNECT_BUDGET_MS)
                .expect("a stall")
                .needs_attention
        );
    }

    #[test]
    fn a_provider_that_named_its_own_reason_is_never_overridden() {
        let notice = FeedNotice::attention(
            "MetaTrader is not running",
            "Open MetaTrader 5 and attach QuantickBridge.",
        );
        let input = StallInput {
            connection: FeedConnectionState::Connecting,
            tape_age_ms: None,
            ..healthy(&notice)
        };
        assert_eq!(
            assess(&input, FIRST_CONNECT_BUDGET_MS * 10),
            None,
            "the feed's own diagnosis is better than an inferred one"
        );
    }

    #[test]
    fn a_replay_is_never_diagnosed() {
        let notice = FeedNotice::Clear;
        let input = StallInput {
            replaying: true,
            tape_age_ms: Some(SILENT_BUDGET_MS * 10),
            ..healthy(&notice)
        };
        assert_eq!(
            assess(&input, 0),
            None,
            "the gap between two recorded prints is data, not trouble"
        );
    }

    /// Nothing has ever arrived on a connected transport. A fresh one is not
    /// stalled — but one that has been open past the budget with nothing down
    /// it is, and it used to be the one shape that reported nothing at all.
    ///
    /// This reverses a deliberate call. When the budget was written, silence
    /// from birth was survivable without a report: the notice card drew on any
    /// empty pane and the status bar carried a recovery pair on a full one.
    /// The corner replaced both, so a branch that reports nothing is now a
    /// chart with no way out — a trader watching a spinner over an empty chart
    /// whose only exit is closing the application, which is the exact failure
    /// the card was built to end.
    ///
    /// No age is invented. "How long has this socket been open with nothing
    /// down it" is measured from the transport's own state change, and the
    /// budget is what keeps a fresh feed quiet.
    #[test]
    fn a_connected_feed_with_nothing_on_it_stalls_once_the_budget_runs_out() {
        let notice = FeedNotice::Clear;
        let fresh = StallInput {
            tape_age_ms: None,
            ..healthy(&notice)
        };
        assert_eq!(
            assess(&fresh, SILENT_BUDGET_MS - 1),
            None,
            "a feed that just landed is not stalled, it is new"
        );

        let stall = assess(&fresh, SILENT_BUDGET_MS).expect("an open socket with nothing on it");
        assert!(
            stall.headline.contains("sent nothing"),
            "the words are the ones a tape that went quiet gets: {}",
            stall.headline
        );
        assert_eq!(
            stall.primary,
            Recovery::Reload,
            "reconnecting a socket that is already open fixes nothing"
        );
        // And a tape that *has* printed is still judged on its own age rather
        // than on how long the socket has been up.
        let printed = StallInput {
            tape_age_ms: Some(0),
            ..healthy(&notice)
        };
        assert_eq!(assess(&printed, 10_000_000), None);
    }

    #[test]
    fn durations_read_as_a_status_line_writes_them() {
        assert_eq!(spoken_ms(0), "0 s");
        assert_eq!(spoken_ms(45_000), "45 s");
        assert_eq!(spoken_ms(MINUTES_ABOVE_S * 1_000 - 1), "89 s");
        assert_eq!(spoken_ms(MINUTES_ABOVE_S * 1_000), "1 min");
        assert_eq!(spoken_ms(600_000), "10 min");
        assert_eq!(spoken_ms(HOURS_ABOVE_MIN * 60_000 - 1), "89 min");
        assert_eq!(spoken_ms(HOURS_ABOVE_MIN * 60_000), "2 h");
        assert_eq!(
            spoken_ms(119 * 60_000),
            "2 h",
            "an hour truncated loses the same order as the reading"
        );
        // The case this tier exists for: a chart opened before the open, on
        // the session that closed the afternoon before.
        assert_eq!(spoken_ms(14 * 3_600_000), "14 h");
        assert_eq!(
            spoken_ms(-5),
            "0 s",
            "a clock that stepped back is not news"
        );
    }

    /// The hook has to photograph the real thing, so every shape it can ask
    /// for must produce exactly what the corresponding branch of `assess`
    /// produces.
    #[test]
    fn the_hook_shows_the_words_a_real_stall_writes() {
        let notice = FeedNotice::Clear;
        let name = "MetaTrader 5 — B3";

        let connecting = StallInput {
            connection: FeedConnectionState::Connecting,
            tape_age_ms: None,
            ..healthy(&notice)
        };
        assert_eq!(
            assess(&connecting, FIRST_CONNECT_BUDGET_MS),
            Some(ForcedStall::FirstConnect.stall(ProviderKind::MetaTrader, name))
        );

        let reconnecting = StallInput {
            connection: FeedConnectionState::Reconnecting,
            ..healthy(&notice)
        };
        assert_eq!(
            assess(&reconnecting, RECONNECT_BUDGET_MS),
            Some(ForcedStall::Reconnect.stall(ProviderKind::MetaTrader, name))
        );

        let silent = StallInput {
            tape_age_ms: Some(SILENT_BUDGET_MS),
            ..healthy(&notice)
        };
        assert_eq!(
            assess(&silent, 0),
            Some(ForcedStall::Silent.stall(ProviderKind::MetaTrader, name))
        );
    }

    #[test]
    fn recovery_names_itself_and_its_opposite() {
        assert_eq!(Recovery::Reconnect.label(), "Reconnect");
        assert_eq!(Recovery::Reload.label(), "Reload");
        assert_eq!(Recovery::Reconnect.other(), Recovery::Reload);
        assert_eq!(Recovery::Reload.other(), Recovery::Reconnect);
    }
}
