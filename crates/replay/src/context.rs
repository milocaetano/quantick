//! The context file: broker candles either side of a recorded session.
//!
//! A replay opens on the day the trader chose, and to its left there is
//! nothing — the chart starts at the first print with empty space before it.
//! That empty space is the problem this file solves: a trader needs to see
//! where the market came from before pressing play.
//!
//! The candles are *not* tape. They are the broker's own minute bars, and the
//! difference is carried rather than smoothed over — see [`ContextSeries`].
//! They live in a sibling file per session, following the rule the crate
//! documents for depth (`lib.rs`): data that is not a trade never gets
//! smuggled into the trade rows.
//!
//! ```text
//! <replay folder>/
//!     WINQ26/
//!         2026-08-12.csv           ← the tape, print by print
//!         2026-08-12.context.csv   ← the minute candles before it
//! ```
//!
//! ```text
//! # quantick-context 1
//! # symbol=WINQ26
//! # timezone=-03:00
//! # interval_ms=60000
//! # complete=true
//! # source=MetaTrader 5 broker candles
//! Date,Time,Open,High,Low,Close,Volume,Trades
//! 2026-08-11,09:00:00.000,167500,167600,167450,167580,1234,0
//! ```
//!
//! # One interval, folded locally
//!
//! Only the base interval is ever stored — a minute, as every provider
//! delivers. The 2m/5m/15m a trader switches between are folds over these bars,
//! computed in the app, so changing timeframe costs no download and the four
//! views can never disagree with each other. The interval is written down
//! rather than assumed: a consumer resampling must never guess what it is
//! resampling *from*.

use std::path::{Path, PathBuf};

use quantick_engine::Bar;
use rust_decimal::Decimal;

use crate::format::{
    self, FormatError, FormatErrorKind, UtcOffset, parse_date, parse_decimal, parse_time,
};

/// Marker on the first line of a context file.
pub const CONTEXT_FORMAT_NAME: &str = "quantick-context";

/// Version this build writes and reads.
pub const CONTEXT_FORMAT_VERSION: u32 = 1;

/// What a context file is called, relative to the session it belongs to:
/// `2026-08-12.csv` → `2026-08-12.context.csv`.
pub const CONTEXT_SUFFIX: &str = ".context.csv";

/// The column header this build writes. Columns are read by name, so the order
/// here is a convention and not a requirement.
pub const CANONICAL_CONTEXT_HEADER: &str = "Date,Time,Open,High,Low,Close,Volume,Trades";

/// Broker candles for the sessions around a recording.
///
/// # What these bars are, and are not
///
/// They are the broker's minute candles, carrying exactly what a broker
/// publishes — which is less than the tape does. Three consequences, recorded
/// here rather than discovered later:
///
/// - **No aggressor split exists.** `buy_volume` and `sell_volume` each carry
///   half the candle's volume, which keeps [`Bar::volume`] exact and makes
///   [`Bar::delta`] identically zero. That zero reads as *not measured* — never
///   as measured and found balanced. Anything showing order flow over this
///   stretch has to say it has none.
/// - **`open_time` is the bucket start**, and `close_time` the bucket end minus
///   one millisecond: the interval the candle *covers*. An engine bar instead
///   stamps its first and last trade, so its times sit strictly inside its
///   bucket. Joining the two series means reconciling that seam explicitly.
/// - **Empty intervals are absent, not flat.** A minute nothing traded in is
///   simply not there, following the engine's empty-interval rule. A
///   carried-forward price would be a fabricated one.
#[derive(Debug, Clone)]
pub struct ContextSeries {
    /// Instrument, from the `# symbol=` line.
    pub symbol: Option<String>,
    /// Offset the `Date`/`Time` columns are expressed in.
    pub timezone: UtcOffset,
    /// What one candle covers, in milliseconds. Declared, never inferred.
    pub interval_ms: i64,
    /// Whether the file covers the whole span its writer set out to fetch.
    ///
    /// `false` means the answer is short and *known* to be short — a download
    /// cancelled partway, a broker that stopped answering, a block clipped to a
    /// cap. It does not mean the same thing as a short series: an instrument
    /// younger than the span genuinely has fewer candles, and that answer is
    /// complete. The bar count cannot tell those two apart, which is why this
    /// is carried rather than derived.
    pub complete: bool,
    /// Where the candles came from, e.g. the broker and terminal that served
    /// them. Free text, shown to the trader.
    pub source: Option<String>,
    /// Candles ascending by `open_time`.
    pub bars: Vec<Bar>,
}

impl ContextSeries {
    /// Timestamp of the first candle's bucket start, in epoch milliseconds.
    #[must_use]
    pub fn start_ms(&self) -> Option<i64> {
        self.bars.first().map(|b| b.open_time)
    }

    /// Timestamp of the last candle's bucket end, in epoch milliseconds.
    #[must_use]
    pub fn end_ms(&self) -> Option<i64> {
        self.bars.last().map(|b| b.close_time)
    }
}

/// The context file belonging to a session file.
///
/// `…/2026-08-12.csv` → `…/2026-08-12.context.csv`. A path whose name does not
/// end in the session extension gets the suffix appended, which keeps the
/// mapping total rather than optional.
#[must_use]
pub fn context_path(session: &Path) -> PathBuf {
    let name = session
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let stem = name
        .strip_suffix(&format!(".{}", format::FILE_EXTENSION))
        .unwrap_or(&name);
    session.with_file_name(format!("{stem}{CONTEXT_SUFFIX}"))
}

/// Whether a file name is a context file rather than a session.
///
/// The library scan uses this to pass over context files in silence: they are
/// not sessions that failed to load, they belong to one.
#[must_use]
pub fn is_context_file(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|n| n.ends_with(CONTEXT_SUFFIX))
}

/// Which column sits where, resolved once from the header.
struct ContextColumns {
    date: usize,
    time: usize,
    open: usize,
    high: usize,
    low: usize,
    close: usize,
    volume: usize,
    trades: Option<usize>,
    width: usize,
}

impl ContextColumns {
    fn parse(header: &str, line_no: usize) -> Result<Self, FormatError> {
        let names: Vec<String> = header
            .split(',')
            .map(|f| f.trim().to_ascii_lowercase())
            .collect();
        let find = |want: &str| names.iter().position(|n| n == want);
        let missing = |want: &str| {
            FormatError::at(
                line_no,
                FormatErrorKind::MissingColumn,
                format!("the header names no `{want}` column"),
            )
        };
        for want in ["date", "time", "open", "high", "low", "close", "volume"] {
            if names.iter().filter(|n| *n == want).count() > 1 {
                return Err(FormatError::at(
                    line_no,
                    FormatErrorKind::DuplicateColumn,
                    format!("the header names `{want}` more than once"),
                ));
            }
        }
        Ok(Self {
            date: find("date").ok_or_else(|| missing("Date"))?,
            time: find("time").ok_or_else(|| missing("Time"))?,
            open: find("open").ok_or_else(|| missing("Open"))?,
            high: find("high").ok_or_else(|| missing("High"))?,
            low: find("low").ok_or_else(|| missing("Low"))?,
            close: find("close").ok_or_else(|| missing("Close"))?,
            volume: find("volume").ok_or_else(|| missing("Volume"))?,
            trades: find("trades"),
            width: names.len(),
        })
    }
}

/// Read a context file.
///
/// # Errors
///
/// [`FormatError`] naming the line and what to change, on any malformed
/// header, column or row.
pub fn parse_context(text: &str) -> Result<ContextSeries, FormatError> {
    let text = format::strip_bom(text);
    let mut symbol = None;
    let mut timezone = UtcOffset::UTC;
    let mut interval_ms = None;
    let mut complete = true;
    let mut source = None;
    let mut columns = None;
    let mut header_line = 0;

    let mut lines = text.lines().enumerate();
    for (index, raw) in lines.by_ref() {
        let line = raw.trim();
        let line_no = index + 1;
        if line.is_empty() {
            continue;
        }
        if let Some(meta) = line.strip_prefix('#') {
            let meta = meta.trim();
            let Some((key, value)) = meta.split_once('=') else {
                continue;
            };
            match key.trim() {
                "symbol" => symbol = Some(value.trim().to_string()),
                "timezone" => {
                    timezone = UtcOffset::parse(value.trim()).ok_or_else(|| {
                        FormatError::at(
                            line_no,
                            FormatErrorKind::Timezone,
                            format!("`{}` is not a fixed UTC offset", value.trim()),
                        )
                    })?;
                }
                "interval_ms" => {
                    let parsed: i64 = value.trim().parse().map_err(|_| {
                        FormatError::at(
                            line_no,
                            FormatErrorKind::Interval,
                            format!("`{}` is not a whole number of milliseconds", value.trim()),
                        )
                    })?;
                    if parsed <= 0 {
                        return Err(FormatError::at(
                            line_no,
                            FormatErrorKind::Interval,
                            format!("an interval of {parsed} ms covers no time"),
                        ));
                    }
                    interval_ms = Some(parsed);
                }
                "complete" => complete = !value.trim().eq_ignore_ascii_case("false"),
                "source" => source = Some(value.trim().to_string()),
                _ => {}
            }
            continue;
        }
        header_line = line_no;
        columns = Some(ContextColumns::parse(line, line_no)?);
        break;
    }

    let Some(columns) = columns else {
        return Err(FormatError::at(
            0,
            FormatErrorKind::MissingHeader,
            format!("no column header; expected `{CANONICAL_CONTEXT_HEADER}`"),
        ));
    };
    // Declared, never guessed: a consumer folding these to 5m has to know what
    // it is folding from, and a wrong assumption would misplace every bucket.
    let Some(interval_ms) = interval_ms else {
        return Err(FormatError::at(
            header_line,
            FormatErrorKind::Interval,
            "no `# interval_ms=` line; the candle interval must be declared".to_string(),
        ));
    };

    let mut bars: Vec<Bar> = Vec::new();
    for (index, raw) in lines {
        let line = raw.trim();
        let line_no = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() < columns.width {
            return Err(FormatError::at(
                line_no,
                FormatErrorKind::FieldCount,
                format!(
                    "{} fields, but the header declared {}",
                    fields.len(),
                    columns.width
                ),
            ));
        }
        let date_ms = parse_date(fields[columns.date], line_no)?;
        let time_ms = parse_time(fields[columns.time], line_no)?;
        // Wall clock in the file's declared offset, back to epoch UTC — the
        // same arithmetic the tape rows go through.
        let open_time = date_ms
            .saturating_add(time_ms)
            .saturating_sub(timezone.millis());
        let volume = parse_decimal(fields[columns.volume], "Volume", line_no)?;
        // Halved rather than left whole: `Bar` has no undivided volume field,
        // and half each is what keeps `Bar::volume` exact while making
        // `Bar::delta` identically zero — the shape that reads as "not
        // measured". See `ContextSeries`.
        let half = volume / Decimal::TWO;
        let trade_count = match columns.trades {
            // 0 means "not reported", not "no trades": a candle with no trades
            // is never written at all.
            None => 0,
            Some(at) if fields[at].is_empty() => 0,
            Some(at) => fields[at].parse::<u64>().map_err(|e| {
                FormatError::at(
                    line_no,
                    FormatErrorKind::Number,
                    format!("Trades `{}`: {e}", fields[at]),
                )
            })?,
        };
        let bar = Bar {
            open_time,
            close_time: open_time + interval_ms - 1,
            open: parse_decimal(fields[columns.open], "Open", line_no)?,
            high: parse_decimal(fields[columns.high], "High", line_no)?,
            low: parse_decimal(fields[columns.low], "Low", line_no)?,
            close: parse_decimal(fields[columns.close], "Close", line_no)?,
            buy_volume: half,
            sell_volume: volume - half,
            trade_count,
        };
        if let Some(previous) = bars.last()
            && bar.open_time <= previous.open_time
        {
            return Err(FormatError::at(
                line_no,
                FormatErrorKind::OutOfOrder,
                format!(
                    "this candle opens at {}, the one above it at {}",
                    bar.open_time, previous.open_time
                ),
            ));
        }
        bars.push(bar);
    }

    Ok(ContextSeries {
        symbol,
        timezone,
        interval_ms,
        complete,
        source,
        bars,
    })
}

/// The metadata a context file is written with.
pub struct ContextHeader {
    /// Instrument the candles belong to.
    pub symbol: String,
    /// Offset the `Date`/`Time` columns are written in.
    pub timezone: UtcOffset,
    /// What one candle covers, in milliseconds.
    pub interval_ms: i64,
    /// Whether the writer got the whole span it asked for.
    pub complete: bool,
    /// Where the candles came from.
    pub source: Option<String>,
}

/// Write a context file: metadata block, column header, one row per candle.
#[must_use]
pub fn write_context(header: &ContextHeader, bars: &[Bar]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(128 + bars.len() * 56);
    let _ = writeln!(out, "# {CONTEXT_FORMAT_NAME} {CONTEXT_FORMAT_VERSION}");
    let _ = writeln!(out, "# symbol={}", header.symbol);
    let _ = writeln!(out, "# timezone={}", header.timezone.label());
    let _ = writeln!(out, "# interval_ms={}", header.interval_ms);
    let _ = writeln!(out, "# complete={}", header.complete);
    if let Some(source) = &header.source {
        let _ = writeln!(out, "# source={source}");
    }
    let _ = writeln!(out, "{CANONICAL_CONTEXT_HEADER}");
    for bar in bars {
        let (date, time) = format::format_datetime(bar.open_time, header.timezone);
        let _ = writeln!(
            out,
            "{date},{time},{},{},{},{},{},{}",
            bar.open,
            bar.high,
            bar.low,
            bar.close,
            bar.buy_volume + bar.sell_volume,
            bar.trade_count
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# quantick-context 1
# symbol=WINQ26
# timezone=-03:00
# interval_ms=60000
# complete=true
# source=MetaTrader 5 broker candles
Date,Time,Open,High,Low,Close,Volume,Trades
2026-08-11,09:00:00.000,167500,167600,167450,167580,1234,0
2026-08-11,09:01:00.000,167580,167640,167570,167600,801,0
";

    #[test]
    fn a_context_file_parses_into_candles() {
        let series = parse_context(SAMPLE).expect("the sample is well formed");
        assert_eq!(series.symbol.as_deref(), Some("WINQ26"));
        assert_eq!(series.timezone.minutes(), -180);
        assert_eq!(series.interval_ms, 60_000);
        assert!(series.complete);
        assert_eq!(series.bars.len(), 2);

        let first = &series.bars[0];
        assert_eq!(first.open, Decimal::from(167_500));
        assert_eq!(first.high, Decimal::from(167_600));
        assert_eq!(first.low, Decimal::from(167_450));
        assert_eq!(first.close, Decimal::from(167_580));
        // The bucket the candle covers, not the stamps of trades inside it.
        assert_eq!(first.close_time, first.open_time + 59_999);
        assert_eq!(series.bars[1].open_time, first.open_time + 60_000);
    }

    #[test]
    fn volume_is_kept_exact_and_delta_reads_as_unmeasured() {
        let series = parse_context(SAMPLE).unwrap();
        let first = &series.bars[0];
        assert_eq!(first.volume(), Decimal::from(1_234), "volume stays exact");
        assert_eq!(
            first.delta(),
            Decimal::ZERO,
            "a broker candle has no aggressor split; zero means not measured"
        );
    }

    #[test]
    fn an_odd_volume_still_adds_back_up() {
        let text = SAMPLE.replace(",1234,0", ",1235,0");
        let series = parse_context(&text).unwrap();
        assert_eq!(series.bars[0].volume(), Decimal::from(1_235));
        assert_eq!(series.bars[0].delta(), Decimal::ZERO);
    }

    #[test]
    fn the_interval_must_be_declared() {
        let text = SAMPLE.replace("# interval_ms=60000\n", "");
        let err = parse_context(&text).expect_err("an undeclared interval is a rejection");
        assert_eq!(err.kind, FormatErrorKind::Interval);
        assert!(!err.advice().is_empty());
    }

    #[test]
    fn a_nonsense_interval_is_named_not_defaulted() {
        for bad in ["# interval_ms=0\n", "# interval_ms=minute\n"] {
            let text = SAMPLE.replace("# interval_ms=60000\n", bad);
            let err = parse_context(&text).expect_err("{bad} must not load");
            assert_eq!(err.kind, FormatErrorKind::Interval);
        }
    }

    #[test]
    fn a_short_download_says_so() {
        let text = SAMPLE.replace("# complete=true", "# complete=false");
        let series = parse_context(&text).unwrap();
        assert!(
            !series.complete,
            "a clipped fetch is carried, not inferred from the bar count"
        );
    }

    #[test]
    fn columns_are_read_by_name_and_extras_ignored() {
        let text = "\
# quantick-context 1
# timezone=+00:00
# interval_ms=60000
Time,Date,Close,Open,Spread,High,Low,Volume
09:00:00.000,2026-08-11,167580,167500,5,167600,167450,1234
";
        let series = parse_context(text).expect("order is free, unknown columns ignored");
        assert_eq!(series.bars.len(), 1);
        assert_eq!(series.bars[0].open, Decimal::from(167_500));
        assert_eq!(series.bars[0].close, Decimal::from(167_580));
        assert_eq!(
            series.bars[0].trade_count, 0,
            "no Trades column: 0 reported"
        );
    }

    #[test]
    fn a_missing_column_names_itself() {
        let text = "\
# interval_ms=60000
Date,Time,Open,High,Low,Volume
2026-08-11,09:00:00.000,1,2,3,4
";
        let err = parse_context(text).expect_err("Close is required");
        assert_eq!(err.kind, FormatErrorKind::MissingColumn);
        assert!(err.to_string().contains("Close"), "{err}");
    }

    #[test]
    fn candles_must_run_forwards() {
        let text = "\
# interval_ms=60000
# timezone=+00:00
Date,Time,Open,High,Low,Close,Volume
2026-08-11,09:01:00.000,1,2,3,4,5
2026-08-11,09:00:00.000,1,2,3,4,5
";
        let err = parse_context(text).expect_err("a replay plays time forward only");
        assert_eq!(err.kind, FormatErrorKind::OutOfOrder);
        assert_eq!(err.line, 5);
    }

    #[test]
    fn a_file_with_no_header_says_which_one_it_wanted() {
        let err = parse_context("# interval_ms=60000\n").expect_err("no columns, no file");
        assert_eq!(err.kind, FormatErrorKind::MissingHeader);
        assert!(err.to_string().contains(CANONICAL_CONTEXT_HEADER), "{err}");
    }

    #[test]
    fn no_candles_is_a_valid_file() {
        let text = "# interval_ms=60000\nDate,Time,Open,High,Low,Close,Volume\n";
        let series = parse_context(text).expect("an empty context is an honest absence");
        assert!(series.bars.is_empty());
    }

    #[test]
    fn a_written_file_reads_back_identical() {
        let series = parse_context(SAMPLE).unwrap();
        let text = write_context(
            &ContextHeader {
                symbol: "WINQ26".to_string(),
                timezone: series.timezone,
                interval_ms: series.interval_ms,
                complete: series.complete,
                source: series.source.clone(),
            },
            &series.bars,
        );
        let round_tripped = parse_context(&text).expect("what we write, we read");
        assert_eq!(round_tripped.bars.len(), series.bars.len());
        for (a, b) in round_tripped.bars.iter().zip(&series.bars) {
            assert_eq!(a, b);
        }
        assert_eq!(round_tripped.interval_ms, series.interval_ms);
        assert_eq!(round_tripped.symbol.as_deref(), Some("WINQ26"));
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_first_line() {
        let with_bom = format!("\u{feff}{SAMPLE}");
        let series = parse_context(&with_bom).expect("Windows puts a BOM on everything");
        assert_eq!(series.bars.len(), 2);
        assert_eq!(series.symbol.as_deref(), Some("WINQ26"));
    }

    #[test]
    fn a_context_file_sits_beside_its_session() {
        assert_eq!(
            context_path(Path::new("replay/WINQ26/2026-08-12.csv")),
            PathBuf::from("replay/WINQ26/2026-08-12.context.csv")
        );
        assert_eq!(
            context_path(Path::new("replay/WINQ26-20260812.csv")),
            PathBuf::from("replay/WINQ26-20260812.context.csv")
        );
        assert!(is_context_file(Path::new("a/2026-08-12.context.csv")));
        assert!(!is_context_file(Path::new("a/2026-08-12.csv")));
    }
}
