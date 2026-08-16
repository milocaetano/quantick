//! Chart state: trades in (backfill + live), bars out — for any bar type.
//!
//! This is the app's side of the "one engine, four consumers" boundary. It
//! retains every trade and feeds them through whichever [`BarBuilder`] the user
//! has selected; switching the bar type rebuilds the bars from the retained
//! trades through a freshly configured builder — the same deterministic engine
//! code path, just a different measure. It also records where backfilled data
//! ends and live data begins so the two can be labelled honestly.
//!
//! No egui, no async here, so the ingest, dispatch and rebuild logic is
//! unit-tested in CI.

/// Re-exported so every consumer of the bar vocabulary finds the imbalance
/// unit next to [`BarSpec`], not off in the engine.
pub use quantick_engine::ImbalanceUnit;
use quantick_engine::{
    Bar, BarBuilder, BarFootprint, BarProgress, DollarBarBuilder, ImbalanceBarBuilder,
    TickBarBuilder, TimeBarBuilder, Trade, VolumeBarBuilder,
};
use rust_decimal::Decimal;

use crate::footprint_series::{self, FootprintSeries};

/// Which alternative bar type the chart is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    /// Close every N trades.
    Tick,
    /// Close every N units of traded quantity.
    Volume,
    /// Close every N notional (price × quantity).
    Dollar,
    /// Close every N milliseconds of trade time.
    Time,
    /// Close when aggressor imbalance beats an adaptive threshold
    /// (López de Prado tick imbalance bars).
    Imbalance,
}

impl BarKind {
    /// All bar kinds, for building a selector.
    pub const ALL: [BarKind; 5] = [
        BarKind::Tick,
        BarKind::Volume,
        BarKind::Dollar,
        BarKind::Time,
        BarKind::Imbalance,
    ];

    /// A short display label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            BarKind::Tick => "tick",
            BarKind::Volume => "volume",
            BarKind::Dollar => "dollar",
            BarKind::Time => "time",
            BarKind::Imbalance => "imbalance",
        }
    }

    /// Whether this rule measures traded size, and so needs a venue that
    /// prints one.
    ///
    /// Tick, time and imbalance bars all count *events*: imbalance in its
    /// default trades unit sums a signed ±1 per trade (López de Prado's tick
    /// imbalance bars), never a quantity. They stay meaningful on a
    /// quote-driven feed. Volume and dollar bars do not — fed one synthetic
    /// unit per tick, a "volume 500" bar is a 500-tick bar wearing a
    /// misleading label. The imbalance *kind* answers for that default: its
    /// volume/dollar units measure size too, and the toolbar's unit selector
    /// gates them per feed exactly as this method gates the kinds.
    #[must_use]
    pub fn needs_traded_volume(self) -> bool {
        match self {
            BarKind::Volume | BarKind::Dollar => true,
            BarKind::Tick | BarKind::Time | BarKind::Imbalance => false,
        }
    }

    /// The unit the closing rule counts in, for the forming bar's countdown.
    #[must_use]
    pub fn progress_unit(self) -> &'static str {
        match self {
            BarKind::Tick | BarKind::Imbalance => "ticks",
            BarKind::Volume => "vol",
            BarKind::Dollar => "notional",
            BarKind::Time => "ms",
        }
    }
}

/// A bar type together with its threshold parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarSpec {
    /// N trades per bar.
    Tick(u64),
    /// N units of quantity per bar.
    Volume(Decimal),
    /// N notional per bar.
    Dollar(Decimal),
    /// N milliseconds per bar. The interval a control may ask for is bounded
    /// by [`MIN_TIME_INTERVAL_MS`]..=[`MAX_TIME_INTERVAL_MS`].
    Time(i64),
    /// The adaptive imbalance rule: the measure θ accumulates (trades, volume
    /// or dollar — López de Prado's TIB/VIB/DIB) and the target trades per
    /// bar, which counts trades in every unit.
    Imbalance(ImbalanceUnit, u64),
}

impl BarSpec {
    /// The kind, discarding the parameter.
    #[must_use]
    pub fn kind(&self) -> BarKind {
        match self {
            BarSpec::Tick(_) => BarKind::Tick,
            BarSpec::Volume(_) => BarKind::Volume,
            BarSpec::Dollar(_) => BarKind::Dollar,
            BarSpec::Time(_) => BarKind::Time,
            BarSpec::Imbalance(..) => BarKind::Imbalance,
        }
    }

    /// Construct the matching engine builder. This is the whole "bar type →
    /// builder" dispatch: one place, four consumers of the same engine.
    #[must_use]
    pub fn build(&self) -> Box<dyn BarBuilder> {
        match self {
            BarSpec::Tick(n) => Box::new(TickBarBuilder::new(*n)),
            BarSpec::Volume(units) => Box::new(VolumeBarBuilder::new(*units)),
            BarSpec::Dollar(notional) => Box::new(DollarBarBuilder::new(*notional)),
            BarSpec::Time(ms) => Box::new(TimeBarBuilder::new(*ms)),
            BarSpec::Imbalance(unit, target) => {
                Box::new(ImbalanceBarBuilder::with_unit(*target, *unit))
            }
        }
    }

    /// The interval this spec cuts bars at, when it cuts by time at all.
    ///
    /// Only a time spec has one: a tick or volume bar covers whatever span its
    /// count happened to take, which is not an interval anything can be folded
    /// to.
    #[must_use]
    pub fn time_interval_ms(&self) -> Option<i64> {
        match self {
            Self::Time(ms) => Some(*ms),
            _ => None,
        }
    }

    /// A human-readable summary, e.g. `tick(50)` or `time(1m)`.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            BarSpec::Tick(n) => format!("tick({n})"),
            BarSpec::Volume(u) => format!("volume({u})"),
            BarSpec::Dollar(d) => format!("dollar({d})"),
            BarSpec::Time(ms) => format!("time({})", fmt_time_interval(*ms)),
            BarSpec::Imbalance(ImbalanceUnit::Trades, target) => format!("imbalance({target})"),
            BarSpec::Imbalance(unit, target) => format!("imbalance({} {target})", unit.as_str()),
        }
    }

    /// This spec in the `kind:parameter` vocabulary [`Self::parse`] reads —
    /// the form `default_bars` uses in the feeds configuration and the saved
    /// workspace ([`crate::ui_state`]) writes.
    ///
    /// The round trip is the point: whatever a chart is showing, a config or a
    /// workspace file can ask for by name, and the
    /// `every_bar_spec_survives_the_config_round_trip` test holds the two
    /// halves together.
    #[must_use]
    pub fn to_config_string(&self) -> String {
        match self {
            BarSpec::Tick(n) => format!("tick:{n}"),
            BarSpec::Volume(units) => format!("volume:{units}"),
            BarSpec::Dollar(notional) => format!("dollar:{notional}"),
            BarSpec::Time(ms) => format!("time:{}", fmt_time_interval(*ms)),
            // The trades unit keeps its historical short form, so every spec
            // a workspace saved before units existed still reads back as the
            // same chart.
            BarSpec::Imbalance(ImbalanceUnit::Trades, target) => format!("imbalance:{target}"),
            BarSpec::Imbalance(unit, target) => format!("imbalance:{}:{target}", unit.as_str()),
        }
    }

    /// Parse a `kind:parameter` spec string, the form `default_bars` uses in
    /// the feeds configuration: `tick:50`, `volume:5`, `dollar:500000`,
    /// `imbalance:100` (also `imbalance:volume:500` / `imbalance:dollar:500`
    /// to pick what θ accumulates), `time:1m` (also `time:30s`, `time:1h`,
    /// `time:1500ms` or a bare millisecond count).
    ///
    /// Every rule a UI control enforces holds here too — a positive
    /// parameter, and a time interval inside
    /// [`MIN_TIME_INTERVAL_MS`]..=[`MAX_TIME_INTERVAL_MS`] — so a config
    /// cannot open a chart no control could have produced.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message naming what is wrong, for the config
    /// loader to surface verbatim.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (kind, param) = text
            .split_once(':')
            .ok_or_else(|| format!("'{text}' is not a kind:parameter bar spec, like 'time:1m'"))?;
        let (kind, param) = (kind.trim(), param.trim());
        let positive_count = |what: &str| -> Result<u64, String> {
            match param.parse::<u64>() {
                Ok(n) if n > 0 => Ok(n),
                _ => Err(format!(
                    "{what} bars need a positive whole number, got '{param}'"
                )),
            }
        };
        let positive_decimal = |what: &str| -> Result<Decimal, String> {
            match param.parse::<Decimal>() {
                Ok(d) if d > Decimal::ZERO => Ok(d),
                _ => Err(format!("{what} bars need a positive number, got '{param}'")),
            }
        };
        match kind {
            "tick" => Ok(BarSpec::Tick(positive_count("tick")?)),
            "imbalance" => {
                // The parameter is `target` or `unit:target`. The unit picks
                // what θ accumulates; the target counts trades in every unit.
                let (unit, target) = match param.split_once(':') {
                    None => (ImbalanceUnit::Trades, param),
                    Some((token, target)) => {
                        let token = token.trim();
                        let unit = ImbalanceUnit::parse_token(token).ok_or_else(|| {
                            format!(
                                "unknown imbalance unit '{token}'; one of trades, \
                                 volume, dollar"
                            )
                        })?;
                        (unit, target.trim())
                    }
                };
                match target.parse::<u64>() {
                    Ok(n) if n > 0 => Ok(BarSpec::Imbalance(unit, n)),
                    _ => Err(format!(
                        "imbalance bars need a positive whole trade target, got '{target}'"
                    )),
                }
            }
            "volume" => Ok(BarSpec::Volume(positive_decimal("volume")?)),
            "dollar" => Ok(BarSpec::Dollar(positive_decimal("dollar")?)),
            "time" => {
                let ms = parse_time_interval(param)?;
                if !(MIN_TIME_INTERVAL_MS..=MAX_TIME_INTERVAL_MS).contains(&ms) {
                    return Err(format!(
                        "time interval '{param}' is outside {}..={} — the domain both \
                         time-bar controls accept",
                        fmt_time_interval(MIN_TIME_INTERVAL_MS),
                        fmt_time_interval(MAX_TIME_INTERVAL_MS),
                    ));
                }
                Ok(BarSpec::Time(ms))
            }
            _ => Err(format!(
                "unknown bar kind '{kind}'; one of tick, volume, dollar, time, imbalance"
            )),
        }
    }
}

/// Parse a time interval in the same vocabulary [`fmt_time_interval`] emits:
/// `1h`, `5m`, `90s`, `1500ms`, or a bare millisecond count. The round trip is
/// deliberate — whatever the status bar can say, a config can ask for.
fn parse_time_interval(text: &str) -> Result<i64, String> {
    let parse_scaled = |digits: &str, scale: i64| -> Result<i64, String> {
        digits
            .parse::<i64>()
            .ok()
            .and_then(|n| n.checked_mul(scale))
            .filter(|ms| *ms > 0)
            .ok_or_else(|| format!("'{text}' is not a time interval, like '1m' or '30s'"))
    };
    // `ms` before `m` and `s`: the longest suffix owns the string.
    if let Some(digits) = text.strip_suffix("ms") {
        parse_scaled(digits, 1)
    } else if let Some(digits) = text.strip_suffix('h') {
        parse_scaled(digits, 3_600_000)
    } else if let Some(digits) = text.strip_suffix('m') {
        parse_scaled(digits, 60_000)
    } else if let Some(digits) = text.strip_suffix('s') {
        parse_scaled(digits, 1_000)
    } else {
        parse_scaled(text, 1)
    }
}

/// A time-bar interval for humans: `1m`, `5m`, `1h` for round units, `90s`
/// for whole seconds, raw milliseconds otherwise. The same vocabulary the
/// timeframe chips speak, so the status bar, the toolbar and the chips can
/// never disagree about what `60000` means (the audit's MAJOR-6: two time
/// controls speaking different languages).
#[must_use]
pub fn fmt_time_interval(ms: i64) -> String {
    if ms >= 3_600_000 && ms % 3_600_000 == 0 {
        format!("{}h", ms / 3_600_000)
    } else if ms >= 60_000 && ms % 60_000 == 0 {
        format!("{}m", ms / 60_000)
    } else if ms >= 1_000 && ms % 1_000 == 0 {
        format!("{}s", ms / 1_000)
    } else {
        format!("{ms}ms")
    }
}

/// The bars derived from the retained trade stream, plus the backfill/live
/// boundary, for the currently selected [`BarSpec`].
/// Smallest interval a time-bar control may ask for, in milliseconds.
///
/// A tenth of a second is already finer than any venue's own bar; below it
/// the series is a tick chart wearing a clock.
pub const MIN_TIME_INTERVAL_MS: i64 = 100;
/// Largest interval a time-bar control may ask for, in milliseconds — one
/// day, the coarsest that still fits inside a session.
///
/// The two time-bar controls (the toolbar's BARS group and the time pane's
/// own header) read the same bounds: they set the same `BarSpec::Time`, and a
/// preset one of them offers has to be a value the other accepts.
pub const MAX_TIME_INTERVAL_MS: i64 = 86_400_000;
/// Milliseconds per drag point, shared by both controls so the same gesture
/// moves the same amount wherever it is made.
///
/// Kept at the fine end of the domain — the BARS group's existing feel —
/// because that is where dragging is the right gesture. Crossing to the
/// coarse end is what the time pane's presets and click-to-type are for, and
/// a speed that made an hour a short drag would make a second unreachable.
pub const TIME_INTERVAL_DRAG_SPEED: f64 = 100.0;

pub struct ChartState {
    spec: BarSpec,
    /// O(1) identity of the current temporal bar partition.
    timeline_revision: u64,
    builder: Box<dyn BarBuilder>,
    trades: Vec<Trade>,
    backfill_trade_count: usize,
    backfill_done: bool,
    bars: Vec<Bar>,
    partial: Option<Bar>,
    backfill_boundary: Option<usize>,
    /// Per-bar footprint ladders, index-aligned with `bars`; fed the same
    /// trades the bar builder folds (see [`FootprintSeries`]).
    footprints: FootprintSeries,
    /// Whether the ladders are being accumulated at all. Off (the default)
    /// costs nothing per trade and holds nothing per bar — a capability
    /// nobody asked for must not tax every ingest. Enabling refolds the
    /// retained trades, so nothing is lost by having been off.
    footprint_enabled: bool,
}

impl ChartState {
    /// A fresh chart building bars per `spec`.
    #[must_use]
    pub fn new(spec: BarSpec) -> Self {
        let builder = spec.build();
        Self {
            spec,
            timeline_revision: 0,
            builder,
            trades: Vec::new(),
            backfill_trade_count: 0,
            backfill_done: false,
            bars: Vec::new(),
            partial: None,
            backfill_boundary: None,
            footprints: FootprintSeries::new(footprint_series::default_group()),
            footprint_enabled: false,
        }
    }

    /// Ingest the backfilled history as one batch (call once, before any live
    /// trades), then mark the boundary.
    pub fn ingest_backfill(&mut self, trades: &[Trade]) {
        self.trades.extend_from_slice(trades);
        self.backfill_trade_count = self.trades.len();
        self.backfill_done = true;
        for trade in trades {
            let closed = self.builder.push(trade);
            if self.footprint_enabled {
                self.footprints.observe(trade, closed.as_ref());
            }
            if let Some(bar) = closed {
                self.bars.push(bar);
            }
        }
        self.backfill_boundary = Some(self.bars.len());
        self.refresh_partial();
        self.bump_timeline_revision();
    }

    /// Prepend older backfilled history to the front of the retained stream.
    ///
    /// `trades` must be strictly older than everything already retained (the
    /// feed guarantees this by paging backward from the earliest known
    /// `agg_id`). Because count-based bars (tick/volume/dollar) are grouped from
    /// the first trade, adding older trades re-aligns every bar — so this rebuilds
    /// the whole series through the same deterministic engine path rather than
    /// pretending the existing bars are untouched (data-honesty rule). The
    /// backfill/live boundary is recomputed. Returns how many net bars were added
    /// so the caller can keep the visible window steady.
    pub fn prepend_history(&mut self, trades: &[Trade]) -> usize {
        if trades.is_empty() {
            return 0;
        }
        let bars_before = self.bars.len();
        let mut combined = Vec::with_capacity(trades.len() + self.trades.len());
        combined.extend_from_slice(trades);
        combined.append(&mut self.trades);
        self.trades = combined;
        self.backfill_trade_count += trades.len();
        self.rebuild();
        self.bars.len().saturating_sub(bars_before)
    }

    /// Ingest one live trade, incrementally (no full rebuild).
    pub fn ingest_live(&mut self, trade: &Trade) {
        self.trades.push(trade.clone());
        let closed = self.builder.push(trade);
        if self.footprint_enabled {
            self.footprints.observe(trade, closed.as_ref());
        }
        if let Some(bar) = closed {
            self.bars.push(bar);
        }
        self.refresh_partial();
        self.bump_timeline_revision();
    }

    /// Switch the bar type/parameter, rebuilding all bars from the retained
    /// trades. A no-op if `spec` is unchanged.
    pub fn set_spec(&mut self, spec: BarSpec) {
        if spec == self.spec {
            return;
        }
        self.spec = spec;
        self.rebuild();
    }

    /// Replay every retained trade through a fresh builder for the current spec,
    /// recomputing the bars and the backfill/live boundary.
    fn rebuild(&mut self) {
        let mut builder = self.spec.build();
        let mut bars = Vec::new();
        let mut boundary = None;
        self.footprints.reset(self.footprints.base_group());
        for (i, trade) in self.trades.iter().enumerate() {
            if self.backfill_done && i == self.backfill_trade_count {
                boundary = Some(bars.len());
            }
            let closed = builder.push(trade);
            if self.footprint_enabled {
                self.footprints.observe(trade, closed.as_ref());
            }
            if let Some(bar) = closed {
                bars.push(bar);
            }
        }
        // Backfill covered every retained trade (no live yet): boundary is the
        // end of the bar list.
        if self.backfill_done && boundary.is_none() {
            boundary = Some(bars.len());
        }
        self.partial = builder.partial().cloned();
        self.builder = builder;
        self.bars = bars;
        self.backfill_boundary = boundary;
        self.bump_timeline_revision();
    }

    fn refresh_partial(&mut self) {
        self.partial = self.builder.partial().cloned();
    }

    fn bump_timeline_revision(&mut self) {
        self.timeline_revision = self.timeline_revision.saturating_add(1);
    }

    /// Monotonic identity for order-flow projections keyed by these bar bounds.
    #[must_use]
    pub fn timeline_revision(&self) -> u64 {
        self.timeline_revision
    }

    /// The current bar spec.
    #[must_use]
    pub fn spec(&self) -> &BarSpec {
        &self.spec
    }

    /// The closed bars.
    #[must_use]
    pub fn bars(&self) -> &[Bar] {
        &self.bars
    }

    /// The forming (in-progress) bar, if any.
    #[must_use]
    pub fn partial(&self) -> Option<&Bar> {
        self.partial.as_ref()
    }

    /// The number of purely-backfilled bars (the backfill/live divider index).
    #[must_use]
    pub fn backfill_boundary(&self) -> Option<usize> {
        self.backfill_boundary
    }

    /// One footprint ladder per closed bar, same indices as [`Self::bars`].
    #[must_use]
    pub fn bar_footprints(&self) -> &[BarFootprint] {
        self.footprints.closed()
    }

    /// The forming bar's footprint ladder, the counterpart of
    /// [`Self::partial`].
    #[must_use]
    pub fn partial_footprint(&self) -> Option<&BarFootprint> {
        self.footprints.partial()
    }

    /// The row width footprints are captured at. Rendering reads the width
    /// off each ladder ([`BarFootprint::group`]); this is the capture side of
    /// that round trip — what the range-profile cache keys on to notice a
    /// refold, and what the tests assert against.
    #[must_use]
    pub fn footprint_group(&self) -> Decimal {
        self.footprints.base_group()
    }

    /// Switch footprint accumulation on or off. Off is free; turning it on
    /// refolds the retained trades so the ladders appear fully populated,
    /// and turning it off drops them (the trades can always rebuild them).
    /// A no-op when nothing changes.
    pub fn set_footprint_enabled(&mut self, enabled: bool) {
        if enabled == self.footprint_enabled {
            return;
        }
        self.footprint_enabled = enabled;
        if enabled {
            self.refold_footprints(self.footprints.base_group());
        } else {
            self.footprints.reset(self.footprints.base_group());
        }
    }

    /// Re-capture the footprints at row width `group` (normally the
    /// instrument's `price_step` once the feed reports it), replaying the
    /// retained trades through a scratch builder of the current spec so the
    /// ladders re-align with the very same bar boundaries. The bars — and the
    /// timeline revision projections key on — are untouched. A no-op when the
    /// group is unchanged or not positive; while accumulation is off only
    /// the width is stored, for the refold that happens on enable.
    pub fn set_footprint_group(&mut self, group: Decimal) {
        if group <= Decimal::ZERO || group == self.footprints.base_group() {
            return;
        }
        if self.footprint_enabled {
            self.refold_footprints(group);
        } else {
            self.footprints.reset(group);
        }
    }

    /// Replay every retained trade through a scratch builder of the current
    /// spec, rebuilding the ladders on `group`-wide rows against the very
    /// same bar boundaries the real builder produced.
    fn refold_footprints(&mut self, group: Decimal) {
        self.footprints.reset(group);
        let mut builder = self.spec.build();
        for trade in &self.trades {
            let closed = builder.push(trade);
            self.footprints.observe(trade, closed.as_ref());
        }
    }

    /// Every trade this chart still holds, oldest first — what a rebuild
    /// replays, and what a second view of the same market is seeded from.
    #[must_use]
    pub fn trades(&self) -> &[Trade] {
        &self.trades
    }

    /// How many of [`Self::trades`] arrived as backfilled history rather than
    /// live. Seeding another chart needs this: a trade that was streamed live
    /// must not become "history" just because the second view opened late.
    #[must_use]
    pub fn backfill_trade_count(&self) -> usize {
        self.backfill_trade_count
    }

    /// When the bar in chart slot `index` opened — a closed bar, or the forming
    /// bar in the slot right after them (the chart draws it as one more).
    ///
    /// Bar indices are only meaningful for one spec at a time; the market time
    /// under them survives a rebuild, so this is how a caller remembers where
    /// the user was looking across one. `None` for a slot the series has not
    /// got.
    #[must_use]
    pub fn slot_open_time(&self, index: usize) -> Option<i64> {
        match self.bars.get(index) {
            Some(bar) => Some(bar.open_time),
            None if index == self.bars.len() => self.partial.as_ref().map(|bar| bar.open_time),
            None => None,
        }
    }

    /// The slot showing market time `timestamp_ms`: the newest bar that opened
    /// at or before it, or slot 0 when the whole series is younger than it.
    ///
    /// Bars are pushed in trade order, so their open times are non-decreasing
    /// and this is a binary search. The forming bar counts as the slot after
    /// the closed ones, matching [`Self::slot_open_time`]. `None` when there is
    /// no series to point into.
    #[must_use]
    pub fn slot_at_time(&self, timestamp_ms: i64) -> Option<usize> {
        let slots = self.bars.len() + usize::from(self.partial.is_some());
        if slots == 0 {
            return None;
        }
        if self
            .partial
            .as_ref()
            .is_some_and(|bar| bar.open_time <= timestamp_ms)
        {
            return Some(self.bars.len());
        }
        let after = self
            .bars
            .partition_point(|bar| bar.open_time <= timestamp_ms);
        Some(after.saturating_sub(1))
    }

    /// How far the forming bar is from closing, in the rule's own measure, and
    /// the unit to print it in.
    ///
    /// Straight from the builder that owns the closing rule — the chart never
    /// re-derives "a tick bar closes at N". `None` for a rule with no fixed
    /// threshold to count toward.
    #[must_use]
    pub fn progress(&self) -> Option<(BarProgress, &'static str)> {
        Some((self.builder.progress()?, self.spec.kind().progress_unit()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use std::str::FromStr as _;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    /// Whatever a chart is showing, a config or a saved workspace can name —
    /// and naming it gets that chart back. Both halves of the vocabulary live
    /// in this file, and this is what stops one of them drifting.
    #[test]
    fn every_bar_spec_survives_the_config_round_trip() {
        for spec in [
            BarSpec::Tick(50),
            BarSpec::Imbalance(ImbalanceUnit::Trades, 100),
            BarSpec::Imbalance(ImbalanceUnit::Volume, 500),
            BarSpec::Imbalance(ImbalanceUnit::Dollar, 2500),
            BarSpec::Volume(dec("5.25")),
            BarSpec::Dollar(dec("500000")),
            BarSpec::Time(60_000),
            BarSpec::Time(3_600_000),
            // A parameter with no round unit: the suffix ladder has to fall
            // through to bare milliseconds rather than rounding it away.
            BarSpec::Time(1_500),
        ] {
            let text = spec.to_config_string();
            assert_eq!(
                BarSpec::parse(&text),
                Ok(spec.clone()),
                "'{text}' did not come back as the spec that wrote it"
            );
        }
    }

    /// `default_bars` speaks the same vocabulary the UI does — every kind,
    /// every interval suffix, padding tolerated.
    #[test]
    fn bar_specs_parse_in_the_config_vocabulary() {
        assert_eq!(BarSpec::parse("tick:50"), Ok(BarSpec::Tick(50)));
        assert_eq!(
            BarSpec::parse("imbalance:100"),
            Ok(BarSpec::Imbalance(ImbalanceUnit::Trades, 100)),
            "the pre-units short form still names tick imbalance bars"
        );
        assert_eq!(
            BarSpec::parse("imbalance:trades:100"),
            Ok(BarSpec::Imbalance(ImbalanceUnit::Trades, 100))
        );
        assert_eq!(
            BarSpec::parse("imbalance:volume:500"),
            Ok(BarSpec::Imbalance(ImbalanceUnit::Volume, 500))
        );
        assert_eq!(
            BarSpec::parse("imbalance:dollar:2500"),
            Ok(BarSpec::Imbalance(ImbalanceUnit::Dollar, 2500))
        );
        assert_eq!(BarSpec::parse("volume:5"), Ok(BarSpec::Volume(dec("5"))));
        assert_eq!(
            BarSpec::parse("volume:0.5"),
            Ok(BarSpec::Volume(dec("0.5")))
        );
        assert_eq!(
            BarSpec::parse("dollar:500000"),
            Ok(BarSpec::Dollar(dec("500000")))
        );
        assert_eq!(BarSpec::parse("time:1m"), Ok(BarSpec::Time(60_000)));
        assert_eq!(BarSpec::parse("time:90s"), Ok(BarSpec::Time(90_000)));
        assert_eq!(BarSpec::parse("time:1h"), Ok(BarSpec::Time(3_600_000)));
        assert_eq!(BarSpec::parse("time:1500ms"), Ok(BarSpec::Time(1_500)));
        assert_eq!(BarSpec::parse("time:60000"), Ok(BarSpec::Time(60_000)));
        assert_eq!(BarSpec::parse(" time : 5m "), Ok(BarSpec::Time(300_000)));
    }

    /// Whatever the status bar can say, a config can ask for: the interval
    /// formatter and the parser are inverses over the whole domain shape.
    #[test]
    fn time_interval_labels_round_trip_through_the_parser() {
        for ms in [
            MIN_TIME_INTERVAL_MS,
            1_500,
            60_000,
            300_000,
            3_600_000,
            MAX_TIME_INTERVAL_MS,
        ] {
            let label = fmt_time_interval(ms);
            assert_eq!(
                BarSpec::parse(&format!("time:{label}")),
                Ok(BarSpec::Time(ms)),
                "{label}"
            );
        }
    }

    /// A spec no live control could produce must not come in through the
    /// config either — its only symptom would be a chart nobody asked for.
    #[test]
    fn a_spec_no_control_could_produce_does_not_parse() {
        for bad in [
            "",
            "tick",
            "tick:",
            "tick:0",
            "tick:-5",
            "volume:0",
            "dollar:nope",
            "imbalance:1.5",
            "imbalance:volume:0",
            "imbalance:volume:1.5",
            "imbalance:notional:500",
            "imbalance:volume:",
            "time:0",
            "time:50ms",
            "time:25h",
            "time:1w",
            "grid:1",
        ] {
            let error = BarSpec::parse(bad).expect_err(bad);
            assert!(!error.is_empty(), "{bad} must explain itself");
        }
    }

    fn trade(agg_id: u64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1000 + agg_id as i64 * 100,
            price: dec("100"),
            quantity: dec("1.0"),
            side: Side::Buy,
        }
    }

    #[test]
    fn only_the_size_measuring_rules_need_a_traded_volume() {
        // Volume and dollar bars measure size, so a venue that prints none can
        // only fake them.
        assert!(BarKind::Volume.needs_traded_volume());
        assert!(BarKind::Dollar.needs_traded_volume());
        // The other three count events, not quantity — imbalance answering
        // for its default trades unit, which sums a signed ±1 per trade; the
        // unit selector gates its volume/dollar units per feed.
        assert!(!BarKind::Tick.needs_traded_volume());
        assert!(!BarKind::Time.needs_traded_volume());
        assert!(!BarKind::Imbalance.needs_traded_volume());
    }

    /// One vocabulary for every surface that names a timeframe: the summary
    /// speaks the chips' own labels, falling back to finer units only where
    /// no coarser one writes the value back exactly.
    #[test]
    fn time_summaries_speak_the_chips_language() {
        assert_eq!(fmt_time_interval(60_000), "1m");
        assert_eq!(fmt_time_interval(300_000), "5m");
        assert_eq!(fmt_time_interval(900_000), "15m");
        assert_eq!(fmt_time_interval(3_600_000), "1h");
        assert_eq!(
            fmt_time_interval(90_000),
            "90s",
            "90s is not a round minute"
        );
        assert_eq!(fmt_time_interval(1_000), "1s");
        assert_eq!(fmt_time_interval(1_500), "1500ms");
        assert_eq!(BarSpec::Time(60_000).summary(), "time(1m)");
        assert_eq!(BarSpec::Time(500).summary(), "time(500ms)");
    }

    #[test]
    fn build_dispatches_every_kind() {
        // Tick(1) and Imbalance(1) close on the first trade; the others simply
        // must build and accept a trade without panicking.
        for spec in [
            BarSpec::Tick(1),
            BarSpec::Volume(dec("1.0")),
            BarSpec::Dollar(dec("100")),
            BarSpec::Time(1),
            BarSpec::Imbalance(ImbalanceUnit::Trades, 1),
            BarSpec::Imbalance(ImbalanceUnit::Volume, 1),
            BarSpec::Imbalance(ImbalanceUnit::Dollar, 1),
        ] {
            let kind = spec.kind();
            let mut builder = spec.build();
            let closed = builder.push(&trade(1));
            if matches!(kind, BarKind::Tick | BarKind::Imbalance) {
                assert!(closed.is_some(), "{kind:?}(1) closes immediately");
            }
        }
    }

    #[test]
    fn backfill_and_live_go_through_the_same_builder() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2), trade(3)]);
        assert_eq!(s.bars().len(), 1);
        assert_eq!(s.backfill_boundary(), Some(1));

        s.ingest_live(&trade(4));
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(1), "boundary does not move");
    }

    #[test]
    fn switching_bar_type_rebuilds_from_retained_trades() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        let trades: Vec<Trade> = (1..=6).map(trade).collect();
        s.ingest_backfill(&trades); // tick(2): 6 trades -> 3 bars
        assert_eq!(s.bars().len(), 3);

        s.set_spec(BarSpec::Tick(3)); // rebuild: 6 trades -> 2 bars
        assert_eq!(s.bars().len(), 2);
        assert_eq!(
            s.backfill_boundary(),
            Some(2),
            "all six are backfill -> boundary at the end"
        );
    }

    #[test]
    fn timeline_revision_tracks_ingest_prepend_and_rebuilds() {
        let mut state = ChartState::new(BarSpec::Tick(2));
        assert_eq!(state.timeline_revision(), 0);

        state.ingest_backfill(&(5..=8).map(trade).collect::<Vec<_>>());
        assert_eq!(state.timeline_revision(), 1);
        state.ingest_live(&trade(9));
        assert_eq!(state.timeline_revision(), 2);
        state.prepend_history(&(1..=4).map(trade).collect::<Vec<_>>());
        assert_eq!(state.timeline_revision(), 3);

        state.set_spec(BarSpec::Tick(2));
        assert_eq!(state.timeline_revision(), 3, "an unchanged spec is a no-op");
        state.set_spec(BarSpec::Tick(3));
        assert_eq!(state.timeline_revision(), 4);
    }

    #[test]
    fn boundary_is_recomputed_across_a_switch() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2), trade(3)]); // 1 bar + partial
        s.ingest_live(&trade(4)); // closes bar 2 (backfill 3 + live 4)
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(1));

        // tick(4): 4 trades -> 1 bar. The first 3 (backfill) close 0 bars.
        s.set_spec(BarSpec::Tick(4));
        assert_eq!(s.bars().len(), 1);
        assert_eq!(s.backfill_boundary(), Some(0));
    }

    #[test]
    fn prepend_history_adds_older_bars_and_keeps_boundary() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(5), trade(6), trade(7), trade(8)]); // 2 bars
        s.ingest_live(&trade(9)); // opens a partial, still 2 closed bars
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(2));

        // Pull in the four older trades 1..=4.
        let added = s.prepend_history(&[trade(1), trade(2), trade(3), trade(4)]);
        // tick(2) over 1..=8 backfill = 4 closed bars; trade 9 is the partial.
        assert_eq!(s.bars().len(), 4);
        assert_eq!(added, 2, "two net bars were prepended");
        assert_eq!(
            s.backfill_boundary(),
            Some(4),
            "all eight retained backfill trades are history"
        );
    }

    #[test]
    fn prepend_empty_history_is_a_noop() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2)]);
        let before = s.bars().len();
        let added = s.prepend_history(&[]);
        assert_eq!(added, 0);
        assert_eq!(s.bars().len(), before);
    }

    /// Bar indices mean a different thing per spec; market time does not. This
    /// is the lookup that carries the user's position across a rebuild.
    #[test]
    fn a_slot_is_found_by_the_market_time_it_shows() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        // trade(n) is stamped at 1000 + n*100, so tick(2) bars open at 1100,
        // 1300, 1500 and the partial (trade 7) at 1700.
        let trades: Vec<Trade> = (1..=7).map(trade).collect();
        s.ingest_backfill(&trades);
        assert_eq!(s.bars().len(), 3);
        assert!(s.partial().is_some());

        assert_eq!(s.slot_open_time(0), Some(1100));
        assert_eq!(s.slot_open_time(3), Some(1700), "the forming bar's slot");
        assert_eq!(s.slot_open_time(4), None, "no such slot");

        assert_eq!(s.slot_at_time(1300), Some(1), "exactly on an open");
        assert_eq!(s.slot_at_time(1400), Some(1), "inside a bar");
        assert_eq!(s.slot_at_time(9_999), Some(3), "past the end: the newest");
        assert_eq!(s.slot_at_time(0), Some(0), "before the start: the oldest");
    }

    #[test]
    fn an_empty_series_has_no_slot_to_point_at() {
        let s = ChartState::new(BarSpec::Tick(2));
        assert_eq!(s.slot_at_time(1_000), None);
        assert_eq!(s.slot_open_time(0), None);
    }

    /// The rebuild is what makes the lookup necessary: the same market time
    /// lands on a different index once the bars are re-cut.
    #[test]
    fn the_slot_of_a_time_moves_when_the_spec_changes() {
        let mut s = ChartState::new(BarSpec::Tick(1));
        let trades: Vec<Trade> = (1..=8).map(trade).collect();
        s.ingest_backfill(&trades);
        let slot = s.slot_at_time(1500).expect("a slot for trade 5");
        assert_eq!(slot, 4, "tick(1): one bar per trade");

        s.set_spec(BarSpec::Tick(4));
        assert_eq!(s.slot_at_time(1500), Some(1), "tick(4): the second bar");
    }

    #[test]
    fn setting_the_same_spec_is_a_noop() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2)]);
        let before = s.bars().len();
        s.set_spec(BarSpec::Tick(2));
        assert_eq!(s.bars().len(), before);
    }
}
