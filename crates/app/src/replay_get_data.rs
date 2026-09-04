//! The **Get data** tab: pick a symbol, pick a day, download it.
//!
//! It lives inside the Market Replay browser rather than behind a menu entry
//! of its own, because for a trader downloading and replaying are one task:
//! "run me Wednesday". Splitting them across two places is how a feature gets
//! shipped and never found.
//!
//! Three inputs and one button, in the order the question is actually asked —
//! *which instrument, which day, how much of the run-up*. The day comes from a
//! calendar whose empty days are dead on arrival: the provider is asked what it
//! holds before the trader clicks, so a holiday cannot be chosen and then fail.
//! The estimate sits above the button, so nobody starts a 60 MB download to
//! find out how big it is.
//!
//! # No timeframe control, on purpose
//!
//! The run-up is fetched once as minute candles and folded to 2m/5m/15m by
//! [`crate::resample`]. Asking the trader to pick a timeframe *here* would be
//! asking the same question twice — the chart already has that control — and
//! would let the four views disagree with one another.

use std::collections::BTreeSet;
use std::ops::Bound;
use std::path::PathBuf;

use eframe::egui;
use egui_phosphor::regular as icons;

use quantick_replay::Library;
use quantick_replay::format::{CivilTime, UtcOffset};

use crate::config::ProviderKind;
use crate::theme::{AMBER, TEXT_MUTED, TEXT_PRIMARY, WARN};
use quantick_feed::replay_download::{
    self, DEFAULT_CONTEXT_SESSIONS, DEFAULT_JOIN_DAY_BEFORE, DownloadEvent, DownloadJob,
    DownloadRequest, Mt5SessionSource, SessionSource,
};

/// Context choices offered. A week is the default; the ends are there because
/// "just yesterday" and "the whole fortnight" are both real ways to prepare.
const CONTEXT_CHOICES: [u32; 5] = [1, 3, 5, 10, 20];

/// Roughly how many bytes one tick costs in the replay format, measured on a
/// real B3 export: 19,139,000 bytes for 389,622 prints. Used only to size the
/// estimate shown before a download starts, and labelled as approximate.
const BYTES_PER_TICK: u64 = 49;

/// Broker clocks a trader can state, when the provider cannot measure one.
///
/// Whole hours only, and only the ones a retail terminal is actually set to.
/// B3 brokers run at −03:00; the MetaQuotes demo servers and most European
/// brokers at +02:00/+03:00.
const BROKER_CLOCKS: [(i32, &str); 7] = [
    (-3 * 3600, "-03:00"),
    (-5 * 3600, "-05:00"),
    (0, "UTC"),
    (3600, "+01:00"),
    (2 * 3600, "+02:00"),
    (3 * 3600, "+03:00"),
    (8 * 3600, "+08:00"),
];

/// The month the calendar sits on before anything has been looked up.
///
/// It is never shown: the calendar only draws once a look-up has answered,
/// and the answer moves it to the newest month that has data — which is where
/// the trader wanted to be. A placeholder rather than "today" because this
/// panel owns no clock, and reading one here would be the only wall-clock read
/// on the path.
const PLACEHOLDER_MONTH: (i64, u32) = (1970, 1);

/// Rough prints per session for the estimate, when nothing better is known.
///
/// A busy WIN day is over a million; this is deliberately a round,
/// order-of-magnitude figure, and the interface says "about".
const TYPICAL_TICKS_PER_SESSION: u64 = 1_000_000;

/// What the panel asks the browser to do.
pub enum GetDataAction {
    /// A download finished and wrote this tape; rescan and select it.
    ///
    /// Always the day the trader asked for, never the day before it: the
    /// second tape is fetched for the chart, not to be selected in its place.
    Downloaded(PathBuf),
}

/// The day a download would fetch behind the chosen one.
struct DayBeforeTarget {
    /// The day itself, `YYYY-MM-DD`.
    day: String,
    /// Whether the replay folder already holds it.
    ///
    /// It still joins the chosen day on the chart — that is what makes the
    /// setting worth having on a folder the trader has been filling for weeks
    /// — but nothing is downloaded for it. Re-exporting a tape they already
    /// have costs tens of megabytes and minutes, and overwrites a recording
    /// they may have imported or hand-corrected.
    on_disk: bool,
}

/// How the second half of a two-day download is going.
///
/// Kept apart from [`Phase`], which speaks for the day the trader asked for.
/// That day is already on disk by the time any of this happens, and nothing
/// here may take it away from them: a refusal is a note beside a finished
/// download, never a failed one.
enum DayBeforeStage {
    /// Fetching this day's tape.
    Running {
        /// The day being fetched, `YYYY-MM-DD`.
        day: String,
        /// Prints written so far.
        ticks: u64,
    },
    /// It landed.
    Done {
        /// The day that landed.
        day: String,
        /// How many prints it holds.
        ticks: u64,
    },
    /// It did not, and the chosen day is untouched.
    Failed {
        /// The day that was refused.
        day: String,
        /// What the provider said, in the trader's terms.
        headline: String,
    },
}

/// How far along a request is.
enum Phase {
    /// Nothing asked yet.
    Idle,
    /// Asking the provider which days it holds.
    Probing,
    /// Downloading, with prints written so far.
    Running { ticks: u64 },
    /// Finished; the tape is on disk.
    Finished {
        ticks: u64,
        sessions: u32,
        complete: bool,
    },
    /// Refused or failed, with the one thing to do about it.
    Failed {
        headline: String,
        fix: Option<String>,
    },
}

/// State of the Get data tab.
pub struct GetDataPanel {
    symbol: String,
    /// The month the calendar is showing, as `(year, month)`.
    month: (i64, u32),
    /// The chosen day, `YYYY-MM-DD`.
    day: Option<String>,
    context_sessions: u32,
    /// Days the provider says it holds, or `None` before it was asked.
    ///
    /// A set, not a list: the calendar asks "is this day held?" once per cell
    /// per frame, and a linear scan of four months of days would be ~3,700
    /// string comparisons every frame to draw thirty-one numbers. Ordered
    /// rather than hashed, following the repo's rule for anything whose
    /// iteration can reach what is drawn.
    available: Option<BTreeSet<String>>,
    /// The symbol `available` was measured for, so a retyped symbol invalidates
    /// the calendar instead of quietly showing another instrument's days.
    available_for: String,
    job: Option<DownloadJob>,
    phase: Phase,
    /// The broker clock the trader stated, once the provider said it could not
    /// measure one. `None` means the provider is still doing the measuring.
    declared_clock: Option<i32>,
    /// Whether to show the clock control at all. Raised only after a request
    /// failed for want of one — nobody should have to think about broker
    /// timezones until the machine admits it cannot work one out.
    clock_needed: bool,
    /// Whether the day before the chosen one is fetched with it.
    ///
    /// A mirror of the browser's setting, refreshed on every draw: the
    /// browser owns the choice — the session list draws the same one — and
    /// this panel needs it in hand when a download finishes, which is frames
    /// after the tick was drawn.
    day_before: bool,
    /// Which day a download would fetch behind the chosen one, resolved once
    /// per draw while the library is in hand.
    ///
    /// Resolved there rather than at the moment it is needed because that
    /// moment — folding a finished download — has neither the library nor the
    /// folder, and re-deriving it from the broker's day list alone is how the
    /// panel came to re-export a tape the trader already had.
    day_before_target: Option<DayBeforeTarget>,
    /// The second half of a two-day download, once the first has landed.
    day_before_stage: Option<DayBeforeStage>,
    /// The folder the request in flight is writing into.
    ///
    /// Captured when the request was made, never read again from the field:
    /// the replay folder is a text box the trader can edit while a download
    /// runs, and the two tapes of one join have to land in the same folder or
    /// the join finds nothing beside the day it was made for.
    request_out_dir: Option<PathBuf>,
    /// A *day before* tick made on this tab, waiting for the browser that owns
    /// the setting to pick it up.
    ///
    /// A field rather than a return value: a tick and a finished download can
    /// land in the same frame, and only one action can be returned. The tape
    /// wins that race, so the tick has to survive it somewhere.
    day_before_change: Option<bool>,
    /// The chosen day's tape, held while the day before it is fetched.
    ///
    /// The browser is handed one finished download, not two: it rescans and
    /// selects the day the trader asked for, once everything that day needs is
    /// on disk.
    pending_tape: Option<PathBuf>,
    /// The day to fetch as soon as the finished job has been cleared away.
    ///
    /// Set while folding a `Done` event and acted on by the drain: starting a
    /// job mid-drain would have it thrown out as the finished one's remains.
    chain_day: Option<String>,
    /// Who serves downloads for this panel.
    ///
    /// Held rather than built per request, so the panel can answer *whose*
    /// instruments it is able to fetch — the interface has to know that before
    /// it offers a contract to click, not after the download refuses.
    source: Box<dyn SessionSource>,
}

impl Default for GetDataPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl GetDataPanel {
    /// An empty panel, opening on the current month.
    #[must_use]
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            month: PLACEHOLDER_MONTH,
            day: None,
            context_sessions: DEFAULT_CONTEXT_SESSIONS,
            available: None,
            available_for: String::new(),
            job: None,
            phase: Phase::Idle,
            declared_clock: None,
            clock_needed: false,
            // Overwritten by the browser on every draw; this is only what the
            // panel believes before it has been drawn once.
            day_before: DEFAULT_JOIN_DAY_BEFORE,
            day_before_target: None,
            day_before_stage: None,
            request_out_dir: None,
            day_before_change: None,
            pending_tape: None,
            chain_day: None,
            // The interpreter is the bridge's default, which already falls
            // back from `python` to `py`. A machine that can stream
            // MetaTrader can download from it.
            source: Box::new(Mt5SessionSource::new(
                "python",
                crate::paper_home::shelf_dir().as_deref(),
            )),
        }
    }

    /// The provider whose instruments this panel can actually fetch.
    #[must_use]
    pub fn provider(&self) -> ProviderKind {
        self.source.provider()
    }

    /// Type a symbol into the field, as the keyboard would.
    pub fn set_symbol(&mut self, symbol: &str) {
        self.symbol = symbol.to_string();
    }

    /// The trader just arrived on this tab: put the chart's own instrument in
    /// the field and ask the terminal what it holds, unless something is
    /// already showing or running.
    ///
    /// "Which days can I get?" is the only reason to open this tab, so asking
    /// it is the tab's job, not a button the trader must find first. Repeat
    /// arrivals are free: a calendar already answered for this symbol is left
    /// alone, so switching between the two halves never re-runs the look-up.
    pub fn arrive(&mut self, out_dir: &str, chart_symbol: Option<&str>) {
        if self.on_arrival(chart_symbol) {
            self.probe(out_dir);
        }
    }

    /// What arriving means, decided without touching a process: adopt the
    /// chart's instrument when the field is empty, and answer whether the
    /// terminal is worth asking.
    ///
    /// Split from [`Self::arrive`] so every one of these refusals is reachable
    /// from a test with no MetaTrader, no Python and no network — the same
    /// reason [`Self::absorb`] is split from the channel drain.
    fn on_arrival(&mut self, chart_symbol: Option<&str>) -> bool {
        if self.symbol.trim().is_empty()
            && let Some(symbol) = chart_symbol.map(str::trim).filter(|s| !s.is_empty())
        {
            self.symbol = symbol.to_string();
        }
        if self.is_busy() || self.symbol.trim().is_empty() {
            return false;
        }
        // A calendar already measured for this symbol is the answer; switching
        // between the two halves of the browser must not re-run the look-up.
        // A stale one — measured for another symbol — is exactly the case
        // worth re-asking, and the only one.
        if self.available.is_some() && self.symbol.trim() == self.available_for {
            return false;
        }
        // A refusal is not retried on arrival: the trader is reading the
        // reason, and hammering the terminal would only replace it with itself.
        !matches!(self.phase, Phase::Failed { .. })
    }

    /// Put `symbol` in the field and ask for its days at once — what clicking
    /// one of the offered contracts means.
    fn pick_symbol(&mut self, symbol: &str, out_dir: &str) {
        self.adopt_pick(symbol);
        self.probe(out_dir);
    }

    /// What a pick changes, decided without touching a process — split out for
    /// the same reason [`Self::on_arrival`] is: the state a click produces has
    /// to be reachable from a test with no MetaTrader behind it.
    fn adopt_pick(&mut self, symbol: &str) {
        self.symbol = symbol.to_string();
        // Another contract's day is not this one's; the calendar answers again
        // before anything can be downloaded.
        self.day = None;
        // A pick is an explicit ask, so it clears a previous refusal rather
        // than being blocked by it.
        self.phase = Phase::Idle;
    }

    /// Whether a download or probe is running.
    ///
    /// The second half of a two-day download counts: the tab's spinner and the
    /// disabled Download button both mean "the provider is still working", and
    /// the day before is the provider still working.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        matches!(self.phase, Phase::Probing | Phase::Running { .. })
            || matches!(self.day_before_stage, Some(DayBeforeStage::Running { .. }))
    }

    /// The day the provider holds immediately before `day`, when it holds one.
    ///
    /// The broker's own list of days is the calendar: a Monday reaches Friday
    /// and a holiday is skipped without this panel owning a table of market
    /// closures it would have to keep right for every venue. `None` before the
    /// look-up has answered, and on the oldest day the broker has.
    fn day_before_of(&self, day: &str) -> Option<&str> {
        // Bounds rather than `..day`: a `RangeTo<&str>` cannot address a set of
        // `String` without an allocation, and this is read every frame the tab
        // is open.
        self.available
            .as_ref()?
            .range::<str, _>((Bound::Unbounded, Bound::Excluded(day)))
            .next_back()
            .map(String::as_str)
    }

    /// Which day sits behind the chosen one, and whether it is already here.
    ///
    /// `None` when the trader did not ask for it, when no day is picked, when
    /// the look-up has not answered, or when the broker holds nothing earlier.
    fn resolve_day_before(&self, library: Option<&Library>) -> Option<DayBeforeTarget> {
        if !self.day_before {
            return None;
        }
        let day = self.day.as_deref()?;
        let earlier = self.day_before_of(day)?;
        let symbol = self.symbol.trim();
        let on_disk = library.is_some_and(|library| {
            library.sessions.iter().any(|held| {
                held.symbol == symbol && held.date.is_some_and(|d| d.label() == earlier)
            })
        });
        Some(DayBeforeTarget {
            day: earlier.to_owned(),
            on_disk,
        })
    }

    /// The day to *download* after the chosen one: the target above, unless
    /// the folder already holds it.
    fn day_before_to_fetch(&self) -> Option<String> {
        self.day_before_target
            .as_ref()
            .filter(|target| !target.on_disk)
            .map(|target| target.day.clone())
    }

    /// Drain whatever the running job has said since the last frame.
    ///
    /// Non-blocking by construction: the download runs on its own thread and
    /// this only ever reads what has already arrived, so a busy export cannot
    /// cost a frame.
    fn poll(&mut self) -> Option<GetDataAction> {
        let job = self.job.as_ref()?;
        let mut action = None;
        let mut ended = false;
        let mut absorbed = Vec::new();
        loop {
            match job.events.try_recv() {
                Ok(event) => absorbed.push(event),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    ended = true;
                    break;
                }
            }
        }
        let disconnected = ended;
        for event in absorbed {
            if let Some(finished) = self.absorb(event) {
                action = Some(finished);
                ended = true;
            } else if !matches!(self.phase, Phase::Probing | Phase::Running { .. }) {
                ended = true;
            }
        }
        // The worker is gone and never said why. Say so rather than leaving a
        // spinner turning forever.
        if disconnected && matches!(self.phase, Phase::Probing | Phase::Running { .. }) {
            self.phase = Phase::Failed {
                headline: "the download stopped without finishing".to_string(),
                fix: Some(
                    "Check the MetaTrader 5 terminal is still open, then try again.".to_string(),
                ),
            };
        }
        // The second half's worker is gone without a word — cancelled, killed,
        // or dead. The day the trader actually asked for is already on disk,
        // and it must reach the browser: stranding `pending_tape` here would
        // leave them with a downloaded day the session list never scanned,
        // which looks exactly like a download that did nothing.
        if disconnected
            && let Some(DayBeforeStage::Running { day, .. }) = self.day_before_stage.as_ref()
        {
            let day = day.clone();
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_DAY_BEFORE_STOPPED",
                day = day.as_str(),
                "the day before stopped before finishing; the chosen day is on disk"
            );
            self.day_before_stage = Some(DayBeforeStage::Failed {
                day,
                headline: "it stopped before finishing".to_string(),
            });
            action = action.or_else(|| self.pending_tape.take().map(GetDataAction::Downloaded));
        }
        if ended {
            self.job = None;
        }
        // Only now. `start_day_before` installs a job, and the drain above
        // would otherwise throw it away as the finished one's remains.
        if let Some(day) = self.chain_day.take() {
            action = action.or_else(|| self.start_day_before(&day));
        }
        action
    }

    /// Fold one event into the panel's state.
    ///
    /// Split out from the channel drain so every state this interface can be
    /// in is reachable from a test with no process, no MetaTrader and no
    /// network — which is the only way the failure paths get exercised at all.
    fn absorb(&mut self, event: DownloadEvent) -> Option<GetDataAction> {
        if matches!(self.day_before_stage, Some(DayBeforeStage::Running { .. })) {
            return self.absorb_day_before(event);
        }
        match event {
            DownloadEvent::DaysAvailable { symbol, days } => {
                if let Some(newest) = days.last().and_then(|d| parse_day(d)) {
                    self.month = (newest.0, newest.1);
                }
                self.available_for = symbol;
                self.available = Some(days.into_iter().collect());
                self.phase = Phase::Idle;
                None
            }
            DownloadEvent::Progress { ticks } | DownloadEvent::TapeWritten { ticks, .. } => {
                self.phase = Phase::Running { ticks };
                None
            }
            DownloadEvent::ContextWritten { sessions, complete } => {
                let ticks = match self.phase {
                    Phase::Running { ticks } => ticks,
                    _ => 0,
                };
                self.phase = Phase::Finished {
                    ticks,
                    sessions,
                    complete,
                };
                None
            }
            DownloadEvent::Done { tape } => {
                if let Phase::Running { ticks } = self.phase {
                    self.phase = Phase::Finished {
                        ticks,
                        sessions: 0,
                        complete: true,
                    };
                }
                // The trader's day is on disk. When they asked for the day
                // before it as well, that is fetched behind this one and the
                // browser hears about the pair together — a rescan in the
                // middle would land them on a chart missing half of what is
                // still coming.
                if let Some(day) = self.day_before_to_fetch() {
                    self.pending_tape = Some(tape);
                    self.chain_day = Some(day);
                    return None;
                }
                Some(GetDataAction::Downloaded(tape))
            }
            DownloadEvent::Failed { headline, mut fix } => {
                // The one failure the trader can answer here rather than in a
                // terminal: raise the control instead of handing them advice
                // about a command-line flag they have nowhere to type.
                if headline.contains("clock offset") {
                    self.clock_needed = true;
                    if self.declared_clock.is_none() {
                        self.declared_clock = Some(BROKER_CLOCKS[0].0);
                    }
                    fix = Some(
                        "Pick the broker clock above, then look up the days again.".to_string(),
                    );
                }
                self.phase = Phase::Failed { headline, fix };
                None
            }
        }
    }

    /// Fold one event from the second half of a two-day download.
    ///
    /// Every ending hands the browser the *chosen* day's tape, because that is
    /// the download that finished; whether the day before it came too is said
    /// beside it rather than by succeeding or failing.
    fn absorb_day_before(&mut self, event: DownloadEvent) -> Option<GetDataAction> {
        let Some(DayBeforeStage::Running { day, ticks }) = self.day_before_stage.as_ref() else {
            return None;
        };
        let (day, ticks) = (day.clone(), *ticks);
        match event {
            DownloadEvent::Progress { ticks } | DownloadEvent::TapeWritten { ticks, .. } => {
                self.day_before_stage = Some(DayBeforeStage::Running { day, ticks });
                None
            }
            DownloadEvent::Done { .. } => {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "REPLAY_DAY_BEFORE_DOWNLOADED",
                    symbol = self.symbol.trim(),
                    day = day.as_str(),
                    ticks,
                    "the day before the chosen session is on disk"
                );
                self.day_before_stage = Some(DayBeforeStage::Done { day, ticks });
                self.pending_tape.take().map(GetDataAction::Downloaded)
            }
            DownloadEvent::Failed { headline, .. } => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "REPLAY_DAY_BEFORE_REFUSED",
                    symbol = self.symbol.trim(),
                    day = day.as_str(),
                    detail = headline.as_str(),
                    "the day before could not be fetched; the chosen day is unaffected"
                );
                self.day_before_stage = Some(DayBeforeStage::Failed { day, headline });
                self.pending_tape.take().map(GetDataAction::Downloaded)
            }
            // The second half asks for neither a run-up nor a day list, so
            // nothing it could say about them is about this download.
            DownloadEvent::ContextWritten { .. } | DownloadEvent::DaysAvailable { .. } => None,
        }
    }

    /// Start the second half of a two-day download.
    ///
    /// Deliberately not [`Self::start`]: that one owns [`Self::phase`], and
    /// the chosen day's finished summary has to survive this. A provider that
    /// cannot even be launched for the second day hands the first one over at
    /// once, with the reason kept beside it.
    fn start_day_before(&mut self, day: &str) -> Option<GetDataAction> {
        // The folder the chosen day went into, not the one the field holds
        // now. A trader who edits the path mid-download would otherwise get
        // the two tapes of one join in two folders, and both reported as
        // downloaded.
        let Some(out_dir) = self.request_out_dir.clone() else {
            return self.pending_tape.take().map(GetDataAction::Downloaded);
        };
        let request = DownloadRequest {
            symbol: self.symbol.trim().to_string(),
            day: Some(day.to_string()),
            // No run-up of its own: the candles in front of the chosen day
            // already reach back over this one, and a second copy would be the
            // same minutes downloaded twice.
            context_sessions: 0,
            out_dir,
            utc_offset_s: self.declared_clock,
        };
        match replay_download::run(self.source.as_ref(), &request) {
            Ok(job) => {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "REPLAY_DAY_BEFORE_STARTED",
                    source = self.source.label().as_str(),
                    symbol = request.symbol.as_str(),
                    day,
                    "fetching the day before the chosen session"
                );
                self.job = Some(job);
                self.day_before_stage = Some(DayBeforeStage::Running {
                    day: day.to_string(),
                    ticks: 0,
                });
                None
            }
            Err(e) => {
                self.day_before_stage = Some(DayBeforeStage::Failed {
                    day: day.to_string(),
                    headline: e.to_string(),
                });
                self.pending_tape.take().map(GetDataAction::Downloaded)
            }
        }
    }

    /// Ask the provider which days it holds for the typed symbol.
    fn probe(&mut self, out_dir: &str) {
        if self.symbol.trim().is_empty() {
            return;
        }
        self.available = None;
        self.start(
            DownloadRequest {
                symbol: self.symbol.trim().to_string(),
                day: None,
                context_sessions: 0,
                out_dir: PathBuf::from(out_dir),
                utc_offset_s: self.declared_clock,
            },
            Phase::Probing,
        );
    }

    /// Start the download the three inputs describe.
    fn download(&mut self, out_dir: &str) {
        let Some(day) = self.day.clone() else {
            return;
        };
        self.start(
            DownloadRequest {
                symbol: self.symbol.trim().to_string(),
                day: Some(day),
                context_sessions: self.context_sessions,
                out_dir: PathBuf::from(out_dir),
                utc_offset_s: self.declared_clock,
            },
            Phase::Running { ticks: 0 },
        );
    }

    fn start(&mut self, request: DownloadRequest, phase: Phase) {
        // Whatever the last request said about a day before is about that
        // request. Cleared here rather than at the button, so a probe cannot
        // leave the note from a download standing under it.
        self.day_before_stage = None;
        self.pending_tape = None;
        self.chain_day = None;
        self.request_out_dir = Some(request.out_dir.clone());
        let source = &self.source;
        match replay_download::run(source.as_ref(), &request) {
            Ok(job) => {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "REPLAY_DOWNLOAD_STARTED",
                    source = source.label().as_str(),
                    symbol = request.symbol.as_str(),
                    day = if request.is_probe() {
                        "(probe)"
                    } else {
                        request.day.as_deref().unwrap_or_default()
                    },
                    context_sessions = request.context_sessions,
                    "downloading a replay session"
                );
                self.job = Some(job);
                self.phase = phase;
            }
            Err(e) => {
                self.phase = Phase::Failed {
                    headline: e.to_string(),
                    fix: Some(e.advice()),
                };
            }
        }
    }

    /// Draw the tab. Returns an action when a download finished.
    ///
    /// `offered` are the instruments worth one click — the chart's own first,
    /// then the catalogue, then whatever the replay folder already holds.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        out_dir: &str,
        library: Option<&Library>,
        offered: &[String],
        day_before: bool,
    ) -> Option<GetDataAction> {
        // Before the drain: a download finishing this frame has to read the
        // setting as it stands, not as it stood when the tick was last drawn.
        self.day_before = day_before;
        self.day_before_target = self.resolve_day_before(library);
        let action = self.poll();

        self.draw_source_row(ui);
        ui.add_space(8.0);
        self.draw_symbol_row(ui, out_dir);
        self.draw_offered_row(ui, out_dir, offered);
        ui.add_space(8.0);
        self.draw_calendar(ui, library);
        ui.add_space(8.0);
        self.draw_context_row(ui);
        self.draw_day_before_row(ui, library);
        self.draw_clock_row(ui);
        ui.add_space(10.0);
        self.draw_estimate_and_button(ui, out_dir);

        action
    }

    /// A *day before* tick made on this tab, for the browser that owns the
    /// setting. `None` when nothing was clicked.
    ///
    /// Drained separately from [`Self::draw`]'s action so a tick and a landed
    /// tape in one frame do not compete: only one action can be returned, the
    /// tape wins it, and a click that vanished with no trace would leave the
    /// box visibly flipping back next frame.
    pub fn take_day_before_change(&mut self) -> Option<bool> {
        self.day_before_change.take()
    }

    fn draw_source_row(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Source").color(TEXT_MUTED).small());
            ui.label(egui::RichText::new("MetaTrader 5").color(TEXT_PRIMARY));
            ui.label(
                egui::RichText::new("— the terminal you are logged in to")
                    .color(TEXT_MUTED)
                    .small(),
            );
        });
    }

    fn draw_symbol_row(&mut self, ui: &mut egui::Ui, out_dir: &str) {
        ui.label(egui::RichText::new("Symbol").color(TEXT_MUTED).small());
        let mut look_up = false;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 120.0).max(100.0);
            let response = ui.add_sized(
                [width, 24.0],
                egui::TextEdit::singleline(&mut self.symbol)
                    .hint_text("WINQ26")
                    .desired_width(width),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                look_up = true;
            }
            let busy = self.is_busy();
            if ui
                .add_enabled(
                    !busy && !self.symbol.trim().is_empty(),
                    egui::Button::new("Look up days"),
                )
                .on_hover_text("Ask the terminal which days it holds for this symbol")
                .on_disabled_hover_text(if busy {
                    "Waiting for the terminal"
                } else {
                    "Type a symbol first"
                })
                .clicked()
            {
                look_up = true;
            }
        });
        // The same guard the button beside it carries. Without it, Enter in
        // this field starts a probe mid-chain — and `start` clears the tape
        // being held for the day before, stranding a day that is already on
        // disk and that the browser is then never told about.
        if look_up && !self.is_busy() {
            self.probe(out_dir);
        }
        if matches!(self.phase, Phase::Probing) {
            crate::loading::inline(ui, "asking the terminal which days it holds");
        }
    }

    /// The instruments one click away.
    ///
    /// Typing `WINV26` correctly is a thing a trader should never have to do
    /// to answer "what can I replay?" — the app already knows which contract
    /// is on the chart, which ones the catalogue carries, and which ones are
    /// sitting in the replay folder. The rolling contract changes every couple
    /// of months, so this row is where "the current one, or the one before it"
    /// actually lives.
    fn draw_offered_row(&mut self, ui: &mut egui::Ui, out_dir: &str, offered: &[String]) {
        if offered.is_empty() {
            return;
        }
        let busy = self.is_busy();
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Contracts").color(TEXT_MUTED).small());
            for symbol in offered {
                let on = self.symbol.trim() == symbol;
                let response = ui.add_enabled(
                    !busy,
                    egui::SelectableLabel::new(on, egui::RichText::new(symbol).monospace()),
                );
                if response
                    .on_hover_text("Ask the terminal which days it holds for this contract")
                    .clicked()
                {
                    picked = Some(symbol.clone());
                }
            }
        });
        if let Some(symbol) = picked {
            self.pick_symbol(&symbol, out_dir);
        }
    }

    /// The calendar. Days the provider does not hold are dead — visibly so, and
    /// not clickable — because finding out after a failed download is the worst
    /// possible time to learn a day was a holiday.
    fn draw_calendar(&mut self, ui: &mut egui::Ui, library: Option<&Library>) {
        ui.label(egui::RichText::new("Day").color(TEXT_MUTED).small());
        // A retyped symbol invalidates the calendar rather than showing another
        // instrument's days under the new name — the fastest way to download
        // the wrong contract.
        let stale = self.symbol.trim() != self.available_for;
        // Taken out and put back rather than cloned: this runs every frame,
        // and copying four months of day strings to draw one month of numbers
        // is an allocation per frame for nothing.
        let Some(available) = self.available.take().filter(|_| !stale) else {
            egui::Frame::none()
                .fill(egui::Color32::from_black_alpha(60))
                .inner_margin(10.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Type a symbol and look up its days — quantick asks the \
                             terminal what it holds before you pick.",
                        )
                        .color(TEXT_MUTED)
                        .small(),
                    );
                });
            return;
        };

        egui::Frame::none()
            .fill(egui::Color32::from_black_alpha(60))
            .inner_margin(8.0)
            .rounding(4.0)
            .show(ui, |ui| {
                let (year, month) = self.month;
                ui.horizontal(|ui| {
                    if ui.small_button(icons::CARET_LEFT).clicked() {
                        self.month = previous_month(year, month);
                    }
                    ui.label(
                        egui::RichText::new(format!("{} {year}", month_name(month)))
                            .color(TEXT_PRIMARY),
                    );
                    if ui.small_button(icons::CARET_RIGHT).clicked() {
                        self.month = next_month(year, month);
                    }
                });
                ui.add_space(4.0);

                let (year, month) = self.month;
                egui::Grid::new("replay_get_data_calendar")
                    .num_columns(7)
                    .spacing([2.0, 2.0])
                    .show(ui, |ui| {
                        for name in ["M", "T", "W", "T", "F", "S", "S"] {
                            ui.label(egui::RichText::new(name).color(TEXT_MUTED).small());
                        }
                        ui.end_row();

                        // Monday-first, which is how a trading week reads.
                        let lead = weekday_monday_first(year, month, 1);
                        for _ in 0..lead {
                            ui.label("");
                        }
                        // Built once for the month, not once per cell: the old
                        // shape called `SessionDate::label` inside the scan,
                        // which allocates a String per comparison — thirty-one
                        // days times every session in the folder, every frame.
                        let mut on_disk: BTreeSet<String> = BTreeSet::new();
                        if let Some(library) = library {
                            on_disk.extend(
                                library
                                    .sessions
                                    .iter()
                                    .filter_map(|s| s.date.map(|d| d.label())),
                            );
                        }

                        let mut column = lead;
                        for day in 1..=days_in_month(year, month) {
                            let iso = format!("{year:04}-{month:02}-{day:02}");
                            let held = available.contains(&iso);
                            let downloaded = on_disk.contains(&iso);
                            draw_day_cell(ui, &mut self.day, day, &iso, held, downloaded);
                            column += 1;
                            if column.is_multiple_of(7) {
                                ui.end_row();
                            }
                        }
                    });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} already downloaded", icons::CHECK))
                            .color(AMBER)
                            .small(),
                    );
                    ui.label(
                        egui::RichText::new("· greyed days have no data at this broker")
                            .color(TEXT_MUTED)
                            .small(),
                    );
                });
            });
        self.available = Some(available);
    }

    fn draw_context_row(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Run-up before that day")
                .color(TEXT_MUTED)
                .small(),
        );
        ui.horizontal(|ui| {
            for choice in CONTEXT_CHOICES {
                let selected = self.context_sessions == choice;
                if ui
                    .selectable_label(selected, format!("{choice}"))
                    .on_hover_text(format!(
                        "{choice} trading session(s) of minute candles before the tape"
                    ))
                    .clicked()
                {
                    self.context_sessions = choice;
                }
            }
            ui.label(
                egui::RichText::new("sessions, as minute candles")
                    .color(TEXT_MUTED)
                    .small(),
            );
        });
        ui.label(
            egui::RichText::new(
                "1m, 2m, 5m and 15m all come from these. Broker candles carry no \
                 order flow, so the chart shows none before the tape starts.",
            )
            .color(TEXT_MUTED)
            .small(),
        );
    }

    /// The *day before* tick: whether the session before the chosen one is
    /// fetched as a full tape, not just as the run-up's minute candles.
    ///
    /// It sits directly under the run-up because it answers the sentence the
    /// run-up ends on — *broker candles carry no order flow* — and a trader
    /// reading that has just been told what is missing. Returns the new value
    /// when it was changed here; the browser owns it, and draws the same
    /// setting beside **Play session**.
    fn draw_day_before_row(&mut self, ui: &mut egui::Ui, library: Option<&Library>) {
        ui.add_space(8.0);
        let mut wanted = self.day_before;
        let changed = ui
            .checkbox(&mut wanted, "The day before, as a full tape")
            .on_hover_text(
                "Downloads the previous session's prints too, and replays them in front of the day you picked",
            )
            .changed();
        // The mirror follows the tick on the frame it was clicked. The browser
        // that owns the setting is told below and answers next frame; without
        // this the box would draw its old state for one frame, which is a tick
        // that visibly does not take.
        if changed {
            self.day_before = wanted;
            // Resolved again on the spot: the caption under the box has to
            // answer for the tick that was just clicked, not for the one the
            // frame started with.
            self.day_before_target = self.resolve_day_before(library);
            self.day_before_change = Some(wanted);
        }
        ui.label(
            egui::RichText::new(self.day_before_note())
                .color(TEXT_MUTED)
                .small(),
        );
    }

    /// The sentence under the tick: which day the download would actually
    /// reach for, or why it would reach for none.
    ///
    /// The broker's own list of days is the calendar, so this names the day
    /// that *would* be fetched rather than guessing at weekends. The tick
    /// decides which sentence is true — never the chosen day, which only
    /// decides how precise it can be. Split from the draw for the reason
    /// [`Self::on_arrival`] is: a caption that contradicts the box above it is
    /// a defect nobody catches except by looking, and this is a shape a test
    /// can look at.
    fn day_before_note(&self) -> String {
        if !self.day_before {
            return "Only the day you pick is downloaded; the run-up above stays broker candles."
                .to_string();
        }
        // "The broker holds no earlier day" is a fact about the record, and
        // before the look-up has answered this panel has not been told it.
        // Saying it anyway would be the interface inventing the one thing it
        // is here to report.
        if self.available.is_none() {
            return "The session before the day you pick comes with it, once the terminal has \
                    said which days it holds."
                .to_string();
        }
        if self.day.is_none() {
            return "The session before the day you pick comes with it.".to_string();
        }
        match &self.day_before_target {
            Some(target) if target.on_disk => format!(
                "{} is already in this folder, and replays in front of the day you pick.",
                target.day
            ),
            Some(target) => format!(
                "{} comes too, so the open you rehearse has real order flow behind it.",
                target.day
            ),
            None => "The broker holds no earlier day for this contract.".to_string(),
        }
    }

    /// The broker-clock control, shown only once the provider has said it
    /// cannot measure one.
    ///
    /// Measuring needs a fresh tick, which needs an open market — and a replay
    /// is downloaded after the close. Rather than send the trader to a command
    /// line for a flag, the question is asked here, in the one place they are
    /// already standing, and the answer is labelled as *stated* rather than
    /// measured: the timestamps in the file are only as right as this is.
    fn draw_clock_row(&mut self, ui: &mut egui::Ui) {
        if !self.clock_needed {
            return;
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Broker clock")
                .color(TEXT_MUTED)
                .small(),
        );
        ui.horizontal(|ui| {
            for (offset, label) in BROKER_CLOCKS {
                if ui
                    .selectable_label(self.declared_clock == Some(offset), label)
                    .clicked()
                {
                    self.declared_clock = Some(offset);
                }
            }
        });
        ui.label(
            egui::RichText::new(
                "quantick could not measure it — the market is closed. This is what \
                 the times in the file will be written in, so it is stated, not measured. \
                 B3 brokers run at -03:00.",
            )
            .color(TEXT_MUTED)
            .small(),
        );
    }

    fn draw_estimate_and_button(&mut self, ui: &mut egui::Ui, out_dir: &str) {
        match &self.phase {
            Phase::Failed { headline, fix } => {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(90, 30, 25, 120))
                    .inner_margin(8.0)
                    .rounding(4.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(headline).color(WARN));
                        if let Some(fix) = fix {
                            ui.label(egui::RichText::new(fix).color(TEXT_MUTED).small());
                        }
                    });
                ui.add_space(6.0);
            }
            Phase::Finished {
                ticks,
                sessions,
                complete,
            } => {
                ui.label(
                    egui::RichText::new(format!(
                        "{} {} prints downloaded, with {sessions} session(s) of run-up",
                        icons::CHECK,
                        thousands(*ticks)
                    ))
                    .color(AMBER),
                );
                if !complete {
                    ui.label(
                        egui::RichText::new(
                            "The broker held fewer sessions than asked for; the run-up is short.",
                        )
                        .color(TEXT_MUTED)
                        .small(),
                    );
                }
                ui.add_space(6.0);
            }
            _ => {}
        }

        // Said beside the chosen day's result, never in place of it: the day
        // before is an addition, and its absence is not this download failing.
        match &self.day_before_stage {
            Some(DayBeforeStage::Running { day, ticks }) => {
                // Its own row, with its own Cancel, and then out — exactly as
                // the first half does. Falling through to the button would
                // draw it disabled with a reason that is not the reason
                // ("pick a day from the calendar", with a day picked), and
                // would leave 70 MB downloading with nothing to stop it.
                ui.horizontal(|ui| {
                    crate::loading::inline(
                        ui,
                        &format!(
                            "{day} — the day before, {} prints so far",
                            thousands(*ticks)
                        ),
                    );
                    if ui
                        .button("Cancel")
                        .on_hover_text("The day you picked is already downloaded and stays")
                        .clicked()
                        && let Some(job) = self.job.as_ref()
                    {
                        job.cancel();
                    }
                });
                return;
            }
            Some(DayBeforeStage::Done { day, ticks }) => {
                ui.label(
                    egui::RichText::new(format!(
                        "{} {day} came too, {} prints — it replays in front of the day you picked",
                        icons::CHECK,
                        thousands(*ticks)
                    ))
                    .color(AMBER)
                    .small(),
                );
                ui.add_space(6.0);
            }
            Some(DayBeforeStage::Failed { day, headline }) => {
                ui.label(
                    egui::RichText::new(format!("The day before ({day}) did not come: {headline}"))
                        .color(WARN)
                        .small(),
                );
                ui.label(
                    egui::RichText::new(
                        "The day you picked is downloaded and will replay on its own.",
                    )
                    .color(TEXT_MUTED)
                    .small(),
                );
                ui.add_space(6.0);
            }
            None => {}
        }

        if let Phase::Running { ticks } = self.phase {
            ui.horizontal(|ui| {
                crate::loading::inline(
                    ui,
                    &format!("downloading — {} prints so far", thousands(ticks)),
                );
                if ui.button("Cancel").clicked()
                    && let Some(job) = self.job.as_ref()
                {
                    job.cancel();
                }
            });
            return;
        }

        let ready = self.day.is_some() && !self.symbol.trim().is_empty();
        ui.horizontal(|ui| {
            if let Some(day) = &self.day {
                let bytes = TYPICAL_TICKS_PER_SESSION * BYTES_PER_TICK;
                // Two tapes cost twice one tape, and nobody may find that out
                // from a progress bar that keeps going after they expected it
                // to stop. Both days are named, because "2 tapes from the
                // 28th" is not what is about to be downloaded.
                let (days, tapes) = match self.day_before_to_fetch() {
                    Some(earlier) => (format!("{day} + {earlier}"), 2),
                    None => (day.clone(), 1),
                };
                ui.label(
                    egui::RichText::new(format!(
                        "Tape {days} · about {} · run-up {} session(s) · under 1 MB",
                        size_label(bytes * tapes),
                        self.context_sessions
                    ))
                    .color(TEXT_MUTED)
                    .small(),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button =
                    egui::Button::new(egui::RichText::new("Download").color(egui::Color32::BLACK))
                        .fill(AMBER);
                if ui
                    .add_enabled(ready && !self.is_busy(), button)
                    .on_disabled_hover_text(if self.symbol.trim().is_empty() {
                        "Type a symbol first"
                    } else if self.available.is_none() {
                        "Look up the symbol's days first"
                    } else {
                        "Pick a day from the calendar"
                    })
                    .clicked()
                {
                    self.download(out_dir);
                }
            });
        });
    }
}

/// One day in the calendar grid.
///
/// A free function taking the selection by reference rather than a method:
/// the grid runs inside a closure that already borrows the panel's day set,
/// and this is what lets the set be borrowed instead of copied every frame.
fn draw_day_cell(
    ui: &mut egui::Ui,
    selected_day: &mut Option<String>,
    day: u32,
    iso: &str,
    held: bool,
    on_disk: bool,
) {
    if !held {
        // Not a disabled button: there is nothing here to press, and a greyed
        // number reads as "no data" faster than a dead control does.
        ui.label(
            egui::RichText::new(format!("{day:2}"))
                .color(TEXT_MUTED)
                .weak(),
        );
        return;
    }
    let selected = selected_day.as_deref() == Some(iso);
    let colour = if on_disk { AMBER } else { TEXT_PRIMARY };
    let response = ui
        .selectable_label(
            selected,
            egui::RichText::new(format!("{day:2}")).color(colour),
        )
        .on_hover_text(if on_disk {
            format!("{iso} — already on disk; downloading again replaces it")
        } else {
            iso.to_string()
        });
    if response.clicked() {
        *selected_day = Some(iso.to_string());
    }
}

/// `2026-08-12` → `(2026, 8, 12)`.
fn parse_day(iso: &str) -> Option<(i64, u32, u32)> {
    let mut parts = iso.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

fn previous_month(year: i64, month: u32) -> (i64, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

fn next_month(year: i64, month: u32) -> (i64, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    NAMES.get((month as usize).wrapping_sub(1)).unwrap_or(&"—")
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// Which column a date falls in, with Monday at 0.
///
/// Derived from the replay crate's own civil-date arithmetic rather than a
/// second calendar implementation: a date means one thing in this app.
fn weekday_monday_first(year: i64, month: u32, day: u32) -> usize {
    let ms = CivilTime {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
        second: 0,
        milli: 0,
    }
    .epoch_millis(UtcOffset::UTC);
    let days = ms.div_euclid(86_400_000);
    // 1970-01-01 was a Thursday, which is column 3 when Monday is 0.
    usize::try_from((days + 3).rem_euclid(7)).unwrap_or(0)
}

/// `1234567` → `1,234,567`.
fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A size a person reads at a glance.
fn size_label(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{} kB", bytes / 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calendar_lines_up_with_a_known_week() {
        // 2026-08-12 was a Wednesday — the day WINQ26 expired.
        assert_eq!(weekday_monday_first(2026, 8, 12), 2);
        assert_eq!(weekday_monday_first(2026, 8, 10), 0, "Monday is column 0");
        assert_eq!(weekday_monday_first(2026, 8, 16), 6, "Sunday is column 6");
        // 1970-01-01 was a Thursday.
        assert_eq!(weekday_monday_first(1970, 1, 1), 3);
    }

    #[test]
    fn months_have_the_right_number_of_days() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29, "2024 is a leap year");
        assert_eq!(days_in_month(2000, 2), 29, "so is 2000");
        assert_eq!(days_in_month(1900, 2), 28, "but 1900 is not");
        assert_eq!(days_in_month(2026, 8), 31);
    }

    #[test]
    fn month_navigation_crosses_the_year() {
        assert_eq!(previous_month(2026, 1), (2025, 12));
        assert_eq!(next_month(2026, 12), (2027, 1));
        assert_eq!(next_month(2026, 8), (2026, 9));
    }

    #[test]
    fn an_unmeasurable_clock_raises_the_control_that_answers_it() {
        // The provider's own advice names a command-line flag. Someone
        // standing in a window cannot act on that, so the question is asked
        // where they are — and the advice is rewritten to point at it.
        let mut panel = GetDataPanel::new();
        assert!(
            !panel.clock_needed,
            "nobody thinks about this until it fails"
        );

        panel.absorb(DownloadEvent::Failed {
            headline: "quantick does not know this broker's clock offset yet".to_string(),
            fix: Some("pass --utc-offset-s (B3 brokers use -10800)".to_string()),
        });

        assert!(panel.clock_needed, "the control comes up");
        assert_eq!(
            panel.declared_clock,
            Some(-10_800),
            "defaulting to B3, the market this was built for"
        );
        let Phase::Failed { fix, .. } = &panel.phase else {
            panic!("the failure is still shown");
        };
        let fix = fix.as_deref().unwrap_or_default();
        assert!(
            !fix.contains("--utc-offset-s"),
            "no command-line flags in a window: {fix}"
        );
        assert!(fix.contains("above"), "it points at the control: {fix}");
    }

    #[test]
    fn retyping_the_symbol_invalidates_the_calendar() {
        // Showing WINQ26's days under the name WINV26 is the fastest possible
        // way to download the wrong contract.
        let mut panel = GetDataPanel::new();
        panel.symbol = "WINQ26".to_string();
        panel.available_for = "WINQ26".to_string();
        panel.available = Some(BTreeSet::from(["2026-08-12".to_string()]));
        assert_eq!(panel.symbol.trim(), panel.available_for);

        panel.symbol = "WINV26".to_string();
        assert_ne!(
            panel.symbol.trim(),
            panel.available_for,
            "the calendar is stale and must be looked up again"
        );
    }

    #[test]
    fn a_download_needs_a_day_that_was_offered() {
        let mut panel = GetDataPanel::new();
        panel.symbol = "WINQ26".to_string();
        assert!(panel.day.is_none(), "nothing is picked before a look-up");
        // `download` is a no-op without a day, so a stray click cannot start
        // a request the calendar never authorised.
        panel.download("/replay");
        assert!(panel.job.is_none());
        assert!(matches!(panel.phase, Phase::Idle));
    }

    #[test]
    fn a_probe_reply_moves_the_calendar_to_where_the_data_is() {
        // The panel owns no clock, so it opens on a placeholder month. The
        // first answer has to land the trader on the month that has data,
        // or every download starts with two clicks of month navigation.
        let mut panel = GetDataPanel::new();
        assert_ne!(panel.month, (2026, 8));
        if let Some((year, month, _)) = parse_day("2026-08-12") {
            panel.month = (year, month);
        }
        assert_eq!(panel.month, (2026, 8));
    }

    #[test]
    fn counts_and_sizes_read_at_a_glance() {
        assert_eq!(thousands(389_622), "389,622");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(thousands(7), "7");
        assert_eq!(size_label(49_000_000), "46 MB");
    }

    #[test]
    fn a_day_is_parsed_out_of_the_provider_s_own_spelling() {
        assert_eq!(parse_day("2026-08-12"), Some((2026, 8, 12)));
        assert_eq!(parse_day("nonsense"), None);
    }

    /// Opening the tab *is* the question "what can I download?". The chart's
    /// own instrument answers "which one", so the trader types nothing.
    #[test]
    fn arriving_adopts_the_chart_s_instrument_and_asks() {
        let mut panel = GetDataPanel::new();
        assert!(panel.on_arrival(Some("WINV26")));
        assert_eq!(panel.symbol, "WINV26");
    }

    /// A symbol already typed is the trader's, not the chart's.
    #[test]
    fn arriving_never_overwrites_a_typed_symbol() {
        let mut panel = GetDataPanel::new();
        panel.set_symbol("WINQ26");
        assert!(panel.on_arrival(Some("WINV26")));
        assert_eq!(panel.symbol, "WINQ26");
    }

    /// Switching between the two halves of the browser must not re-run the
    /// look-up that already answered.
    #[test]
    fn arriving_on_an_answered_calendar_asks_nothing() {
        let mut panel = GetDataPanel::new();
        panel.absorb(DownloadEvent::DaysAvailable {
            symbol: "WINV26".to_owned(),
            days: vec!["2026-08-13".to_owned()],
        });
        assert!(!panel.on_arrival(Some("WINV26")));
    }

    /// A calendar measured for another contract is stale, and stale is the one
    /// case worth asking again — showing another instrument's days under this
    /// name is the fastest way to download the wrong contract.
    #[test]
    fn arriving_after_the_chart_changed_contract_asks_again() {
        let mut panel = GetDataPanel::new();
        panel.absorb(DownloadEvent::DaysAvailable {
            symbol: "WINQ26".to_owned(),
            days: vec!["2026-07-21".to_owned()],
        });
        panel.set_symbol("WINV26");
        assert!(panel.on_arrival(Some("WINV26")));
    }

    /// The trader is reading why it failed; re-asking on arrival would replace
    /// that explanation with itself.
    #[test]
    fn arriving_does_not_retry_a_refusal() {
        let mut panel = GetDataPanel::new();
        panel.set_symbol("WINV26");
        panel.absorb(DownloadEvent::Failed {
            headline: "the terminal does not serve WINV26".to_owned(),
            fix: None,
        });
        assert!(!panel.on_arrival(Some("WINV26")));
    }

    /// Nothing on the chart and nothing typed: there is no question to ask.
    #[test]
    fn arriving_with_no_instrument_anywhere_asks_nothing() {
        let mut panel = GetDataPanel::new();
        assert!(!panel.on_arrival(None));
        assert!(panel.symbol.is_empty());
    }

    /// Clicking an offered contract is the one explicit ask that clears a
    /// refusal — the trader has answered "which instrument" themselves, so
    /// leaving the old failure on screen would refuse a question nobody asked.
    #[test]
    fn picking_a_contract_clears_the_refusal_it_replaces() {
        let mut panel = GetDataPanel::new();
        panel.set_symbol("WINQ26");
        panel.day = Some("2026-07-21".to_owned());
        panel.absorb(DownloadEvent::Failed {
            headline: "the terminal does not serve WINQ26".to_owned(),
            fix: None,
        });
        assert!(matches!(panel.phase, Phase::Failed { .. }));

        panel.adopt_pick("WINV26");

        assert_eq!(panel.symbol, "WINV26");
        assert_eq!(panel.day, None, "another contract's day means nothing here");
        assert!(
            !matches!(panel.phase, Phase::Failed { .. }),
            "an explicit pick is not blocked by the last refusal"
        );
    }

    /// The panel has to know whose instruments it can fetch *before* it offers
    /// one to click: a contract the source cannot serve is a click that can
    /// only end in a refusal.
    #[test]
    fn the_panel_answers_for_the_provider_its_source_serves() {
        assert_eq!(GetDataPanel::new().provider(), ProviderKind::MetaTrader);
    }

    /// A panel that has been told which days the broker holds, and which one
    /// the trader picked. A week with a weekend in it, so the day before
    /// Monday has to be Friday and not Sunday.
    fn panel_on(day: &str) -> GetDataPanel {
        let mut panel = GetDataPanel::new();
        panel.absorb(DownloadEvent::DaysAvailable {
            symbol: "WINV26".to_string(),
            days: [
                "2026-08-25",
                "2026-08-26",
                "2026-08-27",
                "2026-08-28",
                "2026-08-31",
            ]
            .iter()
            .map(|d| (*d).to_string())
            .collect(),
        });
        panel.symbol = "WINV26".to_string();
        panel.day = Some(day.to_string());
        after_a_frame(&mut panel);
        panel
    }

    /// Resolve the day before, as drawing the tab does once per frame.
    ///
    /// The panel reads the target rather than deriving it on demand, because
    /// the moment it is needed — folding a finished download — has neither the
    /// library nor the folder in hand. Tests that change what the answer
    /// depends on have to stand a frame in for the same reason.
    fn after_a_frame(panel: &mut GetDataPanel) {
        panel.day_before_target = panel.resolve_day_before(None);
    }

    /// A scanned replay folder holding these days of WINV26 and nothing else.
    fn library_holding(days: &[&str]) -> Library {
        Library {
            root: PathBuf::from("replay"),
            problems: Vec::new(),
            sessions: days
                .iter()
                .map(|day| quantick_replay::SessionEntry {
                    path: PathBuf::from(format!("replay/WINV26/{day}.csv")),
                    symbol: "WINV26".to_string(),
                    date: quantick_replay::SessionDate::parse(day),
                    size_bytes: 0,
                    timezone: UtcOffset::UTC,
                    timezone_declared: true,
                    side_source: None,
                    has_quotes: false,
                    notes: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_day_before_is_the_one_the_broker_holds_not_the_one_the_calendar_has() {
        // Monday: the day before it at this broker is the Friday.
        let monday = panel_on("2026-08-31");
        assert_eq!(monday.day_before_of("2026-08-31"), Some("2026-08-28"));
        assert_eq!(
            monday.day_before_to_fetch().as_deref(),
            Some("2026-08-28"),
            "and that is the day a download would ask for"
        );

        // The oldest day it holds has nothing before it, which is not a
        // failure — it is the honest end of the record.
        let oldest = panel_on("2026-08-25");
        assert_eq!(oldest.day_before_of("2026-08-25"), None);
        assert_eq!(oldest.day_before_to_fetch(), None);
    }

    #[test]
    fn with_the_tick_off_nothing_is_fetched_behind_the_chosen_day() {
        let mut panel = panel_on("2026-08-31");
        panel.day_before = false;
        after_a_frame(&mut panel);
        assert_eq!(
            panel.day_before_of("2026-08-31"),
            Some("2026-08-28"),
            "the broker still holds it"
        );
        assert_eq!(
            panel.day_before_to_fetch(),
            None,
            "the trader simply did not ask for it"
        );
    }

    #[test]
    fn a_finished_download_holds_the_tape_back_until_the_day_before_is_in() {
        let mut panel = panel_on("2026-08-31");
        panel.phase = Phase::Running { ticks: 1_200 };
        let tape = PathBuf::from("replay/WINV26/20260831.csv");

        let action = panel.absorb(DownloadEvent::Done { tape: tape.clone() });

        assert!(
            action.is_none(),
            "the browser is told once, when both days are on disk"
        );
        assert_eq!(panel.pending_tape.as_ref(), Some(&tape));
        assert_eq!(
            panel.chain_day.as_deref(),
            Some("2026-08-28"),
            "and the day before is queued behind it"
        );
        assert!(
            matches!(panel.phase, Phase::Finished { ticks: 1_200, .. }),
            "the day the trader asked for is finished either way"
        );
    }

    #[test]
    fn with_nothing_to_fetch_behind_it_a_download_lands_at_once() {
        let mut panel = panel_on("2026-08-25");
        panel.phase = Phase::Running { ticks: 7 };
        let tape = PathBuf::from("replay/WINV26/20260825.csv");

        let action = panel.absorb(DownloadEvent::Done { tape: tape.clone() });

        assert!(matches!(action, Some(GetDataAction::Downloaded(p)) if p == tape));
        assert!(panel.chain_day.is_none());
        assert!(panel.pending_tape.is_none());
    }

    /// The report this half exists to prevent: a trader who asked for two days
    /// and lost the one they actually picked because the extra one failed.
    #[test]
    fn a_refused_day_before_still_hands_over_the_day_that_was_asked_for() {
        let mut panel = panel_on("2026-08-31");
        let tape = PathBuf::from("replay/WINV26/20260831.csv");
        panel.pending_tape = Some(tape.clone());
        panel.day_before_stage = Some(DayBeforeStage::Running {
            day: "2026-08-28".to_string(),
            ticks: 0,
        });

        let action = panel.absorb(DownloadEvent::Failed {
            headline: "the terminal holds no ticks for that day".to_string(),
            fix: None,
        });

        assert!(matches!(action, Some(GetDataAction::Downloaded(p)) if p == tape));
        assert!(
            matches!(panel.day_before_stage, Some(DayBeforeStage::Failed { .. })),
            "and the interface can say the day before did not come"
        );
        assert!(
            !matches!(panel.phase, Phase::Failed { .. }),
            "the chosen day's download did not fail"
        );
    }

    /// The caption under the tick and the tick itself are one statement. They
    /// disagreed once — the caption keyed off whether a day had been picked,
    /// so a ticked box sat over "only the day you pick is downloaded" — and a
    /// trader reading that has been told the feature is off while it is on.
    #[test]
    fn the_note_under_the_tick_never_contradicts_it() {
        let mut panel = panel_on("2026-08-31");
        assert!(
            panel.day_before_note().contains("2026-08-28"),
            "{}",
            panel.day_before_note()
        );

        // No day picked yet: still coming, it just cannot be named yet.
        panel.day = None;
        after_a_frame(&mut panel);
        assert!(
            panel.day_before_note().contains("comes with it"),
            "{}",
            panel.day_before_note()
        );

        panel.day_before = false;
        after_a_frame(&mut panel);
        assert!(
            panel.day_before_note().starts_with("Only the day you pick"),
            "{}",
            panel.day_before_note()
        );

        // The oldest day the broker holds has nothing behind it, and says so
        // rather than promising a day that will never arrive.
        let oldest = panel_on("2026-08-25");
        assert!(
            oldest.day_before_note().contains("no earlier day"),
            "{}",
            oldest.day_before_note()
        );
    }

    /// Cancelling the second half, or losing it to a killed terminal, must
    /// still hand over the day the trader asked for. It is already on disk;
    /// a browser that is never told never rescans, and a downloaded day that
    /// does not appear in the list is a download that did nothing.
    #[test]
    fn a_second_half_that_stops_without_a_word_still_hands_the_day_over() {
        let mut panel = panel_on("2026-08-31");
        let tape = PathBuf::from("replay/WINV26/20260831.csv");
        panel.pending_tape = Some(tape.clone());
        panel.day_before_stage = Some(DayBeforeStage::Running {
            day: "2026-08-28".to_string(),
            ticks: 400,
        });
        // A job whose sender is already gone: what cancel, a killed terminal
        // and a crashed exporter all look like from here.
        let (_, events) = std::sync::mpsc::channel::<DownloadEvent>();
        panel.job = Some(DownloadJob::stopped_for_test(events));

        let action = panel.poll();

        assert!(matches!(action, Some(GetDataAction::Downloaded(p)) if p == tape));
        assert!(
            matches!(panel.day_before_stage, Some(DayBeforeStage::Failed { .. })),
            "and the row says the day before did not come"
        );
        assert!(panel.pending_tape.is_none(), "handed over exactly once");
        assert!(
            !matches!(panel.phase, Phase::Failed { .. }),
            "the chosen day's download did not fail"
        );
    }

    /// A tape the trader already has is not downloaded again. Re-exporting a
    /// day costs them tens of megabytes and minutes, and overwrites a
    /// recording they may have imported or corrected by hand — while the day
    /// still joins the chart, which is the whole point of the setting on a
    /// folder that has been filling up for weeks.
    #[test]
    fn a_day_before_already_in_the_folder_is_joined_but_never_re_downloaded() {
        let mut panel = panel_on("2026-08-31");
        let library = library_holding(&["2026-08-28"]);
        panel.day_before_target = panel.resolve_day_before(Some(&library));

        assert!(
            panel.day_before_target.as_ref().is_some_and(|t| t.on_disk),
            "the folder holds it"
        );
        assert_eq!(
            panel.day_before_to_fetch(),
            None,
            "so nothing is downloaded for it"
        );
        assert!(
            panel.day_before_note().contains("already in this folder"),
            "{}",
            panel.day_before_note()
        );

        // And a folder that does not hold it downloads it as before.
        let empty = library_holding(&[]);
        panel.day_before_target = panel.resolve_day_before(Some(&empty));
        assert_eq!(panel.day_before_to_fetch().as_deref(), Some("2026-08-28"));
    }

    /// Before the terminal has answered, "the broker holds no earlier day" is
    /// a claim about a record this panel has not been shown.
    #[test]
    fn a_look_up_still_running_never_reports_what_the_broker_holds() {
        let mut panel = panel_on("2026-08-31");
        panel.available = None;
        after_a_frame(&mut panel);

        let note = panel.day_before_note();
        assert!(note.contains("once the terminal has said"), "{note}");
        assert!(
            !note.contains("no earlier day"),
            "a fact it has not been told: {note}"
        );
    }

    #[test]
    fn a_new_request_forgets_the_last_one_s_day_before() {
        let mut panel = panel_on("2026-08-31");
        panel.pending_tape = Some(PathBuf::from("replay/WINV26/20260831.csv"));
        panel.day_before_stage = Some(DayBeforeStage::Done {
            day: "2026-08-28".to_string(),
            ticks: 9,
        });

        panel.probe("replay");

        assert!(panel.day_before_stage.is_none());
        assert!(panel.pending_tape.is_none());
        assert!(panel.chain_day.is_none());
    }
}
