//! The quantick market-replay tick file: one executed trade per line, CSV.
//!
//! The shape follows the tick-data export every retail platform already speaks
//! (Sierra Chart ASCII, NinjaTrader tick import, the CSVs sold by tick-data
//! vendors): a `Date,Time,Price,Bid,Ask,Volume,Side` row per print, in
//! chronological order, preceded by `#` metadata lines.
//!
//! ```text
//! # quantick-replay 1
//! # symbol=WINJ26
//! # timezone=-03:00
//! # side_source=bid_ask
//! Date,Time,Price,Bid,Ask,Volume,Side
//! 2026-03-16,10:01:08.000,182035,182030,182035,12,B
//! 2026-03-16,10:01:09.000,182025,182025,182030,4,S
//! ```
//!
//! # Why the columns are read by name
//!
//! The header line is parsed into a [`ColumnMap`], so column *order* is free and
//! unknown columns are ignored instead of rejected. A file that also carries an
//! `Id` or an exchange-specific column still loads, and a future quantick can
//! start reading a new column without invalidating a single recorded session.
//!
//! # Timestamps
//!
//! `Date`/`Time` are wall-clock in the exchange's own timezone, which is what a
//! trader reads on a chart. The `# timezone=` metadata line declares the offset
//! (e.g. `-03:00` for B3); everything downstream works in epoch milliseconds
//! UTC. A file with no `timezone=` line is read as UTC — declared, not guessed.
//!
//! # Aggressor side
//!
//! `Side` is the **aggressor**: `B` when the buyer crossed the spread, `S` when
//! the seller did. It is deliberately not `A`/`B` for "at ask"/"at bid" — that
//! vocabulary inverts between vendors and would silently mirror the order flow.
//! Where the side had to be inferred, the recorder says so in `# side_source=`.

use std::fmt::Write as _;
use std::str::FromStr as _;

use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

/// Name written on the first metadata line, and looked for when identifying a
/// file as a quantick replay session.
pub const FORMAT_NAME: &str = "quantick-replay";

/// Version of the layout described in this module.
pub const FORMAT_VERSION: u32 = 1;

/// The only file extension scanned as a replay session.
pub const FILE_EXTENSION: &str = "csv";

/// The header line [`write_header`] emits. Reading accepts any column order.
pub const CANONICAL_HEADER: &str = "Date,Time,Price,Bid,Ask,Volume,Side";

/// The format, explained for a person who just picked the wrong folder.
///
/// Shown verbatim in the UI next to whatever went wrong, so the fix never
/// requires opening the documentation.
pub const FORMAT_HELP: &str = "\
Expected layout — one folder per instrument, one CSV per session day:

    <replay folder>/
        WINJ26/
            20260316.csv
            20260317.csv

Files named SYMBOL-YYYYMMDD.csv directly in the folder are read too.

Each file holds one executed trade per line, oldest first:

    # quantick-replay 1
    # symbol=WINJ26
    # timezone=-03:00
    Date,Time,Price,Bid,Ask,Volume,Side
    2026-03-16,10:01:08.000,182035,182030,182035,12,B

Required columns: Date, Time, Price, Volume, Side.
Optional columns: Bid, Ask, Id. Any other column is ignored.
Date is YYYY-MM-DD, Time is HH:MM:SS or HH:MM:SS.mmm, in the timezone declared
by the `# timezone=` line (UTC when absent).
Side is the aggressor: B when the buyer lifted the offer, S when the seller hit
the bid.

Recordings you already have can be converted with:
    cargo run -p quantick-replay --example import_mt5_ndjson -- \\
        --in <recording.ndjson> --out <replay folder>";

/// A fixed offset from UTC, in minutes, as declared by `# timezone=`.
///
/// Fixed on purpose: a replay session is a recording of one day, and applying a
/// DST rule to a timestamp that was already stamped by the exchange would move
/// prints the recorder never moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtcOffset(i32);

impl UtcOffset {
    /// No offset — the default when a file declares no timezone.
    pub const UTC: Self = Self(0);

    /// An offset of `minutes` east of UTC, if it is within ±24 h.
    #[must_use]
    pub fn from_minutes(minutes: i32) -> Option<Self> {
        (-1440..=1440).contains(&minutes).then_some(Self(minutes))
    }

    /// Parse `UTC`, `Z`, `+HH:MM`, `-HH:MM`, `-HHMM` or `-HH`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("utc") || text.eq_ignore_ascii_case("z") {
            return Some(Self::UTC);
        }
        let (sign, rest) = match text.as_bytes().first()? {
            b'+' => (1, &text[1..]),
            b'-' => (-1, &text[1..]),
            _ => (1, text),
        };
        let (hh, mm) = match rest.split_once(':') {
            Some((h, m)) => (h, m),
            None => match rest.len() {
                4 => rest.split_at(2),
                1 | 2 => (rest, "0"),
                _ => return None,
            },
        };
        let hours: i32 = hh.parse().ok()?;
        let minutes: i32 = mm.parse().ok()?;
        if !(0..=59).contains(&minutes) {
            return None;
        }
        Self::from_minutes(sign * (hours * 60 + minutes))
    }

    /// Minutes east of UTC.
    #[must_use]
    pub fn minutes(self) -> i32 {
        self.0
    }

    /// Milliseconds east of UTC, for epoch conversion.
    #[must_use]
    pub fn millis(self) -> i64 {
        i64::from(self.0) * 60_000
    }

    /// The canonical `±HH:MM` rendering (`UTC` for zero).
    #[must_use]
    pub fn label(self) -> String {
        if self.0 == 0 {
            return "UTC".to_string();
        }
        let sign = if self.0 < 0 { '-' } else { '+' };
        let abs = self.0.abs();
        format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
    }
}

impl Default for UtcOffset {
    fn default() -> Self {
        Self::UTC
    }
}

/// What went wrong while reading a replay file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatErrorKind {
    /// The file holds no data rows at all.
    Empty,
    /// No header line naming the columns.
    MissingHeader,
    /// The header omits a column the format requires.
    MissingColumn,
    /// The header names the same column twice.
    DuplicateColumn,
    /// A row has fewer fields than the header declared.
    FieldCount,
    /// `Date` is not `YYYY-MM-DD`, or names a day that does not exist.
    Date,
    /// `Time` is not `HH:MM:SS[.mmm]`.
    Time,
    /// `Price` or `Volume` is not a number.
    Number,
    /// `Side` is not a recognised aggressor token.
    Side,
    /// A row is stamped before the row above it.
    OutOfOrder,
    /// `# timezone=` does not name a fixed UTC offset.
    Timezone,
}

impl FormatErrorKind {
    /// A short, stable token for logs and tests.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            FormatErrorKind::Empty => "empty",
            FormatErrorKind::MissingHeader => "missing_header",
            FormatErrorKind::MissingColumn => "missing_column",
            FormatErrorKind::DuplicateColumn => "duplicate_column",
            FormatErrorKind::FieldCount => "field_count",
            FormatErrorKind::Date => "bad_date",
            FormatErrorKind::Time => "bad_time",
            FormatErrorKind::Number => "bad_number",
            FormatErrorKind::Side => "bad_side",
            FormatErrorKind::OutOfOrder => "out_of_order",
            FormatErrorKind::Timezone => "bad_timezone",
        }
    }

    /// What to change to make the file load.
    #[must_use]
    pub fn advice(self) -> &'static str {
        match self {
            FormatErrorKind::Empty => "Add at least one trade row below the header.",
            FormatErrorKind::MissingHeader => {
                "Add the column header line above the rows, e.g. `Date,Time,Price,Bid,Ask,Volume,Side`."
            }
            FormatErrorKind::MissingColumn => {
                "Name every required column in the header: Date, Time, Price, Volume, Side."
            }
            FormatErrorKind::DuplicateColumn => {
                "Keep one column per name; rename or drop the repeated one."
            }
            FormatErrorKind::FieldCount => {
                "Give every row the same number of comma-separated fields as the header."
            }
            FormatErrorKind::Date => "Write dates as YYYY-MM-DD, e.g. 2026-03-16.",
            FormatErrorKind::Time => "Write times as HH:MM:SS or HH:MM:SS.mmm, e.g. 10:01:08.250.",
            FormatErrorKind::Number => "Use a plain decimal number, e.g. 182035 or 182035.5.",
            FormatErrorKind::Side => {
                "Use B for a buyer-initiated trade and S for a seller-initiated one."
            }
            FormatErrorKind::OutOfOrder => {
                "Sort the rows oldest first; a replay plays time forward only."
            }
            FormatErrorKind::Timezone => {
                "Declare the offset as `# timezone=-03:00`, or drop the line to mean UTC."
            }
        }
    }
}

/// A problem in a replay file, located at a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError {
    /// 1-based line number; `0` when the problem is the file as a whole.
    pub line: usize,
    /// The category of problem.
    pub kind: FormatErrorKind,
    /// What was found, in the file's own words.
    pub detail: String,
}

impl FormatError {
    /// Build an error at `line`.
    #[must_use]
    pub fn at(line: usize, kind: FormatErrorKind, detail: impl Into<String>) -> Self {
        Self {
            line,
            kind,
            detail: detail.into(),
        }
    }

    /// What to change to make the file load.
    #[must_use]
    pub fn advice(&self) -> &'static str {
        self.kind.advice()
    }
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.detail)
        } else {
            write!(f, "line {}: {}", self.line, self.detail)
        }
    }
}

impl std::error::Error for FormatError {}

/// Where each field sits on a row, resolved once from the header line.
///
/// Reading by index after this is a slice lookup, so a million-row file costs
/// one header parse instead of a million name comparisons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMap {
    date: usize,
    time: usize,
    price: usize,
    volume: usize,
    side: usize,
    bid: Option<usize>,
    ask: Option<usize>,
    id: Option<usize>,
    /// How many fields the header declared.
    width: usize,
    /// Header names that quantick does not read, kept so the UI can say what
    /// was ignored rather than silently dropping data.
    ignored: Vec<String>,
}

impl ColumnMap {
    /// Resolve the header line at `line_no` into field positions.
    ///
    /// # Errors
    ///
    /// [`FormatErrorKind::MissingColumn`] when a required column is absent,
    /// [`FormatErrorKind::DuplicateColumn`] when one is named twice.
    pub fn parse(header: &str, line_no: usize) -> Result<Self, FormatError> {
        let mut date = None;
        let mut time = None;
        let mut price = None;
        let mut volume = None;
        let mut side = None;
        let mut bid = None;
        let mut ask = None;
        let mut id = None;
        let mut ignored = Vec::new();
        let mut width = 0;

        for (index, raw) in header.split(',').enumerate() {
            width = index + 1;
            let name = raw.trim().trim_matches('"').to_ascii_lowercase();
            let slot = match name.as_str() {
                "date" => &mut date,
                "time" => &mut time,
                "price" | "last" => &mut price,
                "volume" | "size" | "qty" | "quantity" => &mut volume,
                "side" | "aggressor" => &mut side,
                "bid" | "bidprice" | "bid_price" => &mut bid,
                "ask" | "askprice" | "ask_price" | "offer" => &mut ask,
                "id" | "trade_id" | "tradeid" => &mut id,
                _ => {
                    if !name.is_empty() {
                        ignored.push(raw.trim().to_string());
                    }
                    continue;
                }
            };
            if slot.is_some() {
                return Err(FormatError::at(
                    line_no,
                    FormatErrorKind::DuplicateColumn,
                    format!("column `{}` appears more than once", raw.trim()),
                ));
            }
            *slot = Some(index);
        }

        // A file exported with a semicolon or tab separator reads as one long
        // column, and "header has no `Date` column" would send someone looking
        // for a column that is plainly there. Name the real problem instead.
        if !header.contains(',')
            && let Some(separator) = ['\t', ';', '|'].into_iter().find(|s| header.contains(*s))
        {
            let shown = if separator == '\t' {
                "a tab".to_string()
            } else {
                format!("`{separator}`")
            };
            return Err(FormatError::at(
                line_no,
                FormatErrorKind::MissingColumn,
                format!("this file separates columns with {shown}, not a comma"),
            ));
        }

        let required = |slot: Option<usize>, name: &str| -> Result<usize, FormatError> {
            slot.ok_or_else(|| {
                FormatError::at(
                    line_no,
                    FormatErrorKind::MissingColumn,
                    format!("header has no `{name}` column"),
                )
            })
        };

        Ok(Self {
            date: required(date, "Date")?,
            time: required(time, "Time")?,
            price: required(price, "Price")?,
            volume: required(volume, "Volume")?,
            side: required(side, "Side")?,
            bid,
            ask,
            id,
            width,
            ignored,
        })
    }

    /// The highest field index the map reads, for the row-width check.
    #[must_use]
    fn needed_width(&self) -> usize {
        [
            Some(self.date),
            Some(self.time),
            Some(self.price),
            Some(self.volume),
            Some(self.side),
            self.bid,
            self.ask,
            self.id,
        ]
        .into_iter()
        .flatten()
        .max()
        .map_or(0, |max| max + 1)
    }

    /// How many fields the header declared.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Header names quantick does not read.
    #[must_use]
    pub fn ignored(&self) -> &[String] {
        &self.ignored
    }

    /// Whether the file carries a bid/ask quote alongside each print.
    #[must_use]
    pub fn has_quotes(&self) -> bool {
        self.bid.is_some() && self.ask.is_some()
    }
}

/// The `#` metadata block plus the resolved column positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    /// Format version from `# quantick-replay <n>`, when present.
    pub version: Option<u32>,
    /// Instrument from `# symbol=`, when present.
    pub symbol: Option<String>,
    /// Offset the `Date`/`Time` columns are expressed in.
    pub timezone: UtcOffset,
    /// Whether the file actually declared the timezone (vs defaulting to UTC).
    pub timezone_declared: bool,
    /// How the recorder decided the aggressor side, from `# side_source=`.
    pub side_source: Option<String>,
    /// Where each field sits on a row.
    pub columns: ColumnMap,
    /// 1-based line number of the column header.
    pub header_line: usize,
}

/// Read the metadata block and column header from the start of a file.
///
/// Only reads far enough to find the header line, so scanning a folder of large
/// recordings does not parse a single trade.
///
/// # Errors
///
/// [`FormatErrorKind::MissingHeader`] when no column header is found,
/// [`FormatErrorKind::Timezone`] on an undecodable offset, plus anything
/// [`ColumnMap::parse`] rejects.
pub fn parse_header(text: &str) -> Result<FileHeader, FormatError> {
    let mut version = None;
    let mut symbol = None;
    let mut timezone = UtcOffset::UTC;
    let mut timezone_declared = false;
    let mut side_source = None;

    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let line_no = index + 1;
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            let comment = comment.trim();
            if let Some(rest) = comment
                .strip_prefix(FORMAT_NAME)
                .map(str::trim)
                .filter(|rest| !rest.starts_with('-'))
            {
                version = rest.parse().ok();
                continue;
            }
            let Some((key, value)) = comment.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim().to_ascii_lowercase().as_str() {
                "symbol" | "instrument" => symbol = Some(value.to_string()),
                "timezone" | "tz" | "utc_offset" => {
                    timezone = UtcOffset::parse(value).ok_or_else(|| {
                        FormatError::at(
                            line_no,
                            FormatErrorKind::Timezone,
                            format!("`{value}` is not a fixed UTC offset"),
                        )
                    })?;
                    timezone_declared = true;
                }
                "side_source" | "aggressor_source" => side_source = Some(value.to_string()),
                _ => {}
            }
            continue;
        }
        // First non-comment line is the column header.
        return Ok(FileHeader {
            version,
            symbol,
            timezone,
            timezone_declared,
            side_source,
            columns: ColumnMap::parse(line, line_no)?,
            header_line: line_no,
        });
    }

    Err(FormatError::at(
        0,
        FormatErrorKind::MissingHeader,
        "no column header line found".to_string(),
    ))
}

/// The bid/ask quote recorded alongside a print, when the file carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote {
    /// Best bid at the time of the print.
    pub bid: Decimal,
    /// Best ask at the time of the print.
    pub ask: Decimal,
}

/// Everything one file yielded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFile {
    /// Metadata and column positions.
    pub header: FileHeader,
    /// Trades, in file order (oldest first).
    pub trades: Vec<Trade>,
    /// Quotes parallel to `trades`, when requested and available.
    pub quotes: Vec<Quote>,
}

/// Tuning for [`parse_file`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParseOptions {
    /// Also collect the `Bid`/`Ask` columns into [`ParsedFile::quotes`].
    ///
    /// Off by default: the chart replays trades, and keeping quotes for a
    /// multi-million-row session costs memory nothing reads yet.
    pub keep_quotes: bool,
}

/// Parse a whole replay file.
///
/// Trades come out in file order with a synthetic `agg_id` — the sequence
/// number of the print within the file, unless the file carries an `Id` column.
/// Rows must be chronologically non-decreasing; the first row that steps
/// backwards is an error rather than a silently reordered print.
///
/// # Errors
///
/// The first [`FormatError`] encountered, carrying its 1-based line number.
pub fn parse_file(text: &str, options: ParseOptions) -> Result<ParsedFile, FormatError> {
    let header = parse_header(text)?;
    let columns = &header.columns;
    let needed = columns.needed_width();
    let tz_ms = header.timezone.millis();
    let want_quotes = options.keep_quotes && columns.has_quotes();

    // One allocation sized from the file: ~60 bytes per row is the shape of a
    // real B3 session, and over-reserving beats re-growing a 250k-row vector.
    let estimate = text.len() / 60;
    let mut trades = Vec::with_capacity(estimate);
    let mut quotes = if want_quotes {
        Vec::with_capacity(estimate)
    } else {
        Vec::new()
    };

    let mut fields: Vec<&str> = Vec::with_capacity(columns.width().max(needed));
    let mut last_ms = i64::MIN;
    let mut sequence: u64 = 0;

    for (index, raw) in text.lines().enumerate().skip(header.header_line) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line_no = index + 1;

        fields.clear();
        fields.extend(line.split(','));
        if fields.len() < needed {
            return Err(FormatError::at(
                line_no,
                FormatErrorKind::FieldCount,
                format!(
                    "row has {} field(s); the header declares {}",
                    fields.len(),
                    columns.width()
                ),
            ));
        }

        let date_ms = parse_date(fields[columns.date].trim(), line_no)?;
        let time_ms = parse_time(fields[columns.time].trim(), line_no)?;
        let timestamp_ms = date_ms.saturating_add(time_ms).saturating_sub(tz_ms);
        if timestamp_ms < last_ms {
            return Err(FormatError::at(
                line_no,
                FormatErrorKind::OutOfOrder,
                format!(
                    "{} {} is older than the row above it",
                    fields[columns.date].trim(),
                    fields[columns.time].trim()
                ),
            ));
        }
        last_ms = timestamp_ms;

        let price = parse_decimal(fields[columns.price].trim(), "Price", line_no)?;
        let quantity = parse_decimal(fields[columns.volume].trim(), "Volume", line_no)?;
        let side = parse_side(fields[columns.side].trim(), line_no)?;

        sequence += 1;
        let agg_id = match columns.id {
            Some(at) => fields[at].trim().parse::<u64>().unwrap_or(sequence),
            None => sequence,
        };

        if want_quotes {
            let bid = columns
                .bid
                .map(|at| parse_decimal(fields[at].trim(), "Bid", line_no))
                .transpose()?;
            let ask = columns
                .ask
                .map(|at| parse_decimal(fields[at].trim(), "Ask", line_no))
                .transpose()?;
            if let (Some(bid), Some(ask)) = (bid, ask) {
                quotes.push(Quote { bid, ask });
            }
        }

        trades.push(Trade {
            agg_id,
            timestamp_ms,
            price,
            quantity,
            side,
        });
    }

    if trades.is_empty() {
        return Err(FormatError::at(
            0,
            FormatErrorKind::Empty,
            "file has a header but no trade rows".to_string(),
        ));
    }

    Ok(ParsedFile {
        header,
        trades,
        quotes,
    })
}

/// Milliseconds at midnight UTC of a `YYYY-MM-DD` date.
fn parse_date(text: &str, line_no: usize) -> Result<i64, FormatError> {
    let err = || {
        FormatError::at(
            line_no,
            FormatErrorKind::Date,
            format!("`{text}` is not a YYYY-MM-DD date"),
        )
    };
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(err());
    }
    let year: i64 = text[0..4].parse().map_err(|_| err())?;
    let month: u32 = text[5..7].parse().map_err(|_| err())?;
    let day: u32 = text[8..10].parse().map_err(|_| err())?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return Err(err());
    }
    Ok(days_from_civil(year, month, day) * 86_400_000)
}

/// Milliseconds since midnight of a `HH:MM:SS[.mmm]` time.
fn parse_time(text: &str, line_no: usize) -> Result<i64, FormatError> {
    let err = || {
        FormatError::at(
            line_no,
            FormatErrorKind::Time,
            format!("`{text}` is not a HH:MM:SS[.mmm] time"),
        )
    };
    let (clock, fraction) = match text.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (text, None),
    };
    let bytes = clock.as_bytes();
    if bytes.len() != 8 || bytes[2] != b':' || bytes[5] != b':' {
        return Err(err());
    }
    let hours: i64 = clock[0..2].parse().map_err(|_| err())?;
    let minutes: i64 = clock[3..5].parse().map_err(|_| err())?;
    let seconds: i64 = clock[6..8].parse().map_err(|_| err())?;
    // 60 is accepted as a leap second and clamped, the way exchange feeds stamp
    // them; 61 is a typo.
    if hours > 23 || minutes > 59 || seconds > 60 {
        return Err(err());
    }
    let millis = match fraction {
        None => 0,
        Some(fraction) => {
            if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
                return Err(err());
            }
            // Accept any precision; anything finer than a millisecond is
            // truncated, never rounded, so a re-export cannot drift forward.
            let digits = &fraction[..fraction.len().min(3)];
            let scale = 10i64.pow(3 - digits.len() as u32);
            digits.parse::<i64>().map_err(|_| err())? * scale
        }
    };
    Ok(((hours * 60 + minutes) * 60 + seconds.min(59)) * 1000 + millis)
}

fn parse_decimal(text: &str, column: &str, line_no: usize) -> Result<Decimal, FormatError> {
    Decimal::from_str(text).map_err(|e| {
        FormatError::at(
            line_no,
            FormatErrorKind::Number,
            format!("{column} `{text}`: {e}"),
        )
    })
}

fn parse_side(text: &str, line_no: usize) -> Result<Side, FormatError> {
    match text {
        "B" | "b" | "1" => return Ok(Side::Buy),
        "S" | "s" | "-1" => return Ok(Side::Sell),
        _ => {}
    }
    if text.eq_ignore_ascii_case("buy") {
        return Ok(Side::Buy);
    }
    if text.eq_ignore_ascii_case("sell") {
        return Ok(Side::Sell);
    }
    // `A`/`ASK` reads as "traded at the ask" in some exports and "ask-side
    // aggressor" in others, which are opposite sides. Guessing would mirror the
    // whole session's order flow, so it is rejected by name.
    let detail = if text.eq_ignore_ascii_case("a")
        || text.eq_ignore_ascii_case("ask")
        || text.eq_ignore_ascii_case("bid")
    {
        format!(
            "Side `{text}` is ambiguous — it names a price level, not the aggressor \
             (write B for a buyer-initiated trade, S for a seller-initiated one)"
        )
    } else if text.is_empty() {
        "Side is empty; every row must state the aggressor".to_string()
    } else {
        format!("Side `{text}` is not B or S")
    };
    Err(FormatError::at(line_no, FormatErrorKind::Side, detail))
}

/// Days since the Unix epoch for a proleptic-Gregorian date (Howard Hinnant's
/// `days_from_civil`). Pure integer maths — no calendar dependency, and the
/// same answer on every platform.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`], for writing files.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted + 2) / 5 + 1) as u32;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// A wall-clock reading: what a `Date`/`Time` pair says, with no timezone of its
/// own. Pair it with a [`UtcOffset`] to get an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilTime {
    /// Four-digit year.
    pub year: i64,
    /// Month, 1–12.
    pub month: u32,
    /// Day of month, 1–31.
    pub day: u32,
    /// Hour, 0–23.
    pub hour: u32,
    /// Minute, 0–59.
    pub minute: u32,
    /// Second, 0–59.
    pub second: u32,
    /// Millisecond, 0–999.
    pub milli: u32,
}

impl CivilTime {
    /// The instant this reading names when read at `offset`.
    ///
    /// The inverse of [`civil_parts`]. Out-of-range parts are not rejected here
    /// — callers reading untrusted text validate first (see [`parse_file`]);
    /// this is the arithmetic, shared so recorders and readers agree to the
    /// millisecond.
    #[must_use]
    pub fn epoch_millis(self, offset: UtcOffset) -> i64 {
        let days = days_from_civil(self.year, self.month, self.day);
        let in_day =
            (i64::from(self.hour) * 3600 + i64::from(self.minute) * 60 + i64::from(self.second))
                * 1000
                + i64::from(self.milli);
        days.saturating_mul(86_400_000)
            .saturating_add(in_day)
            .saturating_sub(offset.millis())
    }

    /// The `Date` field, `YYYY-MM-DD`.
    #[must_use]
    pub fn date_field(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// The `Time` field, `HH:MM:SS.mmm`.
    #[must_use]
    pub fn time_field(self) -> String {
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            self.hour, self.minute, self.second, self.milli
        )
    }
}

/// Split an epoch-millisecond instant into the wall-clock reading seen at
/// `offset`.
#[must_use]
pub fn civil_parts(timestamp_ms: i64, offset: UtcOffset) -> CivilTime {
    let local = timestamp_ms.saturating_add(offset.millis());
    let days = local.div_euclid(86_400_000);
    let in_day = local.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let seconds = in_day / 1000;
    CivilTime {
        year,
        month,
        day,
        hour: (seconds / 3600) as u32,
        minute: ((seconds % 3600) / 60) as u32,
        second: (seconds % 60) as u32,
        milli: (in_day % 1000) as u32,
    }
}

/// Format the `Date` and `Time` fields for one instant.
#[must_use]
pub fn format_datetime(timestamp_ms: i64, offset: UtcOffset) -> (String, String) {
    let civil = civil_parts(timestamp_ms, offset);
    (civil.date_field(), civil.time_field())
}

/// Metadata written above the column header by [`write_header`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteHeader {
    /// Instrument the session belongs to.
    pub symbol: String,
    /// Offset the `Date`/`Time` columns are written in.
    pub timezone: UtcOffset,
    /// How the aggressor side was decided, recorded honestly.
    pub side_source: String,
    /// Where the rows came from, e.g. the recording file name.
    pub source: Option<String>,
}

/// Write the metadata block and column header.
#[must_use]
pub fn write_header(header: &WriteHeader) -> String {
    let mut out = String::with_capacity(256);
    let _ = writeln!(out, "# {FORMAT_NAME} {FORMAT_VERSION}");
    let _ = writeln!(out, "# symbol={}", header.symbol);
    let _ = writeln!(out, "# timezone={}", header.timezone.label());
    let _ = writeln!(out, "# side_source={}", header.side_source);
    if let Some(source) = &header.source {
        let _ = writeln!(out, "# source={source}");
    }
    out.push_str(CANONICAL_HEADER);
    out.push('\n');
    out
}

/// Append one trade row in [`CANONICAL_HEADER`] order.
///
/// `quote` supplies the `Bid`/`Ask` columns; without one they are written
/// empty, which reads back as "not recorded" rather than as a zero quote.
pub fn write_trade(out: &mut String, trade: &Trade, quote: Option<Quote>, offset: UtcOffset) {
    let (date, time) = format_datetime(trade.timestamp_ms, offset);
    let (bid, ask) = match quote {
        Some(q) => (q.bid.to_string(), q.ask.to_string()),
        None => (String::new(), String::new()),
    };
    let _ = writeln!(
        out,
        "{date},{time},{},{bid},{ask},{},{}",
        trade.price,
        trade.quantity,
        match trade.side {
            Side::Buy => 'B',
            Side::Sell => 'S',
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(text: &str) -> Decimal {
        Decimal::from_str(text).unwrap()
    }

    const SAMPLE: &str = "\
# quantick-replay 1
# symbol=WINJ26
# timezone=-03:00
# side_source=bid_ask
Date,Time,Price,Bid,Ask,Volume,Side
2026-03-16,10:01:08.000,182035,182030,182035,12,B
2026-03-16,10:01:09.250,182025,182025,182030,4,S
";

    #[test]
    fn reads_metadata_and_columns() {
        let header = parse_header(SAMPLE).unwrap();
        assert_eq!(header.version, Some(1));
        assert_eq!(header.symbol.as_deref(), Some("WINJ26"));
        assert_eq!(header.timezone, UtcOffset::from_minutes(-180).unwrap());
        assert!(header.timezone_declared);
        assert_eq!(header.side_source.as_deref(), Some("bid_ask"));
        assert_eq!(header.header_line, 5);
        assert!(header.columns.has_quotes());
    }

    #[test]
    fn parses_trades_into_utc_epoch_millis() {
        let parsed = parse_file(SAMPLE, ParseOptions::default()).unwrap();
        assert_eq!(parsed.trades.len(), 2);
        // 2026-03-16 10:01:08 at UTC-03:00 is 13:01:08 UTC.
        let civil = civil_parts(parsed.trades[0].timestamp_ms, UtcOffset::UTC);
        assert_eq!(civil.date_field(), "2026-03-16");
        assert_eq!(civil.time_field(), "13:01:08.000");
        assert_eq!(parsed.trades[0].price, dec("182035"));
        assert_eq!(parsed.trades[0].quantity, dec("12"));
        assert_eq!(parsed.trades[0].side, Side::Buy);
        assert_eq!(parsed.trades[1].side, Side::Sell);
        assert_eq!(
            parsed.trades[1].timestamp_ms - parsed.trades[0].timestamp_ms,
            1250
        );
        // Synthetic ids number the prints in file order.
        assert_eq!(parsed.trades[0].agg_id, 1);
        assert_eq!(parsed.trades[1].agg_id, 2);
        assert!(parsed.quotes.is_empty(), "quotes are opt-in");
    }

    #[test]
    fn collects_quotes_when_asked() {
        let parsed = parse_file(SAMPLE, ParseOptions { keep_quotes: true }).unwrap();
        assert_eq!(parsed.quotes.len(), 2);
        assert_eq!(parsed.quotes[0].bid, dec("182030"));
        assert_eq!(parsed.quotes[0].ask, dec("182035"));
    }

    #[test]
    fn column_order_is_free_and_extra_columns_are_ignored() {
        let text = "\
Side,Volume,Time,Date,Exchange,Price
B,7,09:00:00,2026-03-16,BVMF,182000
";
        let parsed = parse_file(text, ParseOptions::default()).unwrap();
        assert_eq!(parsed.trades[0].quantity, dec("7"));
        assert_eq!(parsed.trades[0].price, dec("182000"));
        assert_eq!(parsed.header.columns.ignored(), ["Exchange"]);
        assert!(!parsed.header.columns.has_quotes());
    }

    #[test]
    fn an_id_column_replaces_the_synthetic_sequence() {
        let text = "\
Id,Date,Time,Price,Volume,Side
9001,2026-03-16,09:00:00,100,1,B
9002,2026-03-16,09:00:01,101,1,S
";
        let parsed = parse_file(text, ParseOptions::default()).unwrap();
        assert_eq!(parsed.trades[0].agg_id, 9001);
        assert_eq!(parsed.trades[1].agg_id, 9002);
    }

    #[test]
    fn no_timezone_line_means_utc_and_says_so() {
        let text = "Date,Time,Price,Volume,Side\n2026-03-16,00:00:00,1,1,B\n";
        let parsed = parse_file(text, ParseOptions::default()).unwrap();
        assert_eq!(parsed.header.timezone, UtcOffset::UTC);
        assert!(!parsed.header.timezone_declared);
        assert_eq!(parsed.trades[0].timestamp_ms, 1_773_619_200_000);
    }

    #[test]
    fn missing_required_column_names_it() {
        let err = parse_file("Date,Time,Price,Volume\n", ParseOptions::default()).unwrap_err();
        assert_eq!(err.kind, FormatErrorKind::MissingColumn);
        assert!(err.detail.contains("Side"), "{}", err.detail);
        assert!(err.advice().contains("Side"));
    }

    #[test]
    fn a_semicolon_export_is_told_what_is_wrong_with_it() {
        for text in [
            "Date;Time;Price;Volume;Side\n2026-03-16;10:00:00;1;1;B\n",
            "Date\tTime\tPrice\tVolume\tSide\n2026-03-16\t10:00:00\t1\t1\tB\n",
        ] {
            let err = parse_file(text, ParseOptions::default()).unwrap_err();
            assert_eq!(err.kind, FormatErrorKind::MissingColumn);
            assert!(
                err.detail.contains("not a comma"),
                "should name the separator, said: {}",
                err.detail
            );
        }
    }

    #[test]
    fn a_file_without_a_header_is_rejected() {
        let err = parse_file("# only comments\n\n", ParseOptions::default()).unwrap_err();
        assert_eq!(err.kind, FormatErrorKind::MissingHeader);
        assert_eq!(err.line, 0);
    }

    #[test]
    fn a_header_without_rows_is_rejected() {
        let err = parse_file(CANONICAL_HEADER, ParseOptions::default()).unwrap_err();
        assert_eq!(err.kind, FormatErrorKind::Empty);
    }

    #[test]
    fn duplicate_columns_are_rejected() {
        let err = parse_file(
            "Date,Time,Price,Price,Volume,Side\n",
            ParseOptions::default(),
        )
        .unwrap_err();
        assert_eq!(err.kind, FormatErrorKind::DuplicateColumn);
    }

    #[test]
    fn short_rows_report_the_line() {
        let text =
            "Date,Time,Price,Volume,Side\n2026-03-16,09:00:00,100,1,B\n2026-03-16,09:00:01\n";
        let err = parse_file(text, ParseOptions::default()).unwrap_err();
        assert_eq!(err.kind, FormatErrorKind::FieldCount);
        assert_eq!(err.line, 3);
    }

    #[test]
    fn impossible_dates_are_rejected() {
        for date in ["2026-02-30", "2026-13-01", "16/03/2026", "2026-3-16"] {
            let text = format!("Date,Time,Price,Volume,Side\n{date},09:00:00,1,1,B\n");
            let err = parse_file(&text, ParseOptions::default()).unwrap_err();
            assert_eq!(err.kind, FormatErrorKind::Date, "{date}");
        }
        // A real leap day still loads.
        let text = "Date,Time,Price,Volume,Side\n2028-02-29,09:00:00,1,1,B\n";
        assert!(parse_file(text, ParseOptions::default()).is_ok());
    }

    #[test]
    fn malformed_times_are_rejected() {
        for time in ["9:00:00", "09:00", "09:00:61", "24:00:00", "09:00:00."] {
            let text = format!("Date,Time,Price,Volume,Side\n2026-03-16,{time},1,1,B\n");
            let err = parse_file(&text, ParseOptions::default()).unwrap_err();
            assert_eq!(err.kind, FormatErrorKind::Time, "{time}");
        }
    }

    #[test]
    fn sub_millisecond_precision_truncates() {
        let text = "Date,Time,Price,Volume,Side\n2026-03-16,09:00:00.123456,1,1,B\n";
        let parsed = parse_file(text, ParseOptions::default()).unwrap();
        let civil = civil_parts(parsed.trades[0].timestamp_ms, UtcOffset::UTC);
        assert_eq!(civil.milli, 123);
    }

    #[test]
    fn ambiguous_side_tokens_are_refused_by_name() {
        let text = "Date,Time,Price,Volume,Side\n2026-03-16,09:00:00,1,1,A\n";
        let err = parse_file(text, ParseOptions::default()).unwrap_err();
        assert_eq!(err.kind, FormatErrorKind::Side);
        assert!(err.detail.contains("ambiguous"), "{}", err.detail);
    }

    #[test]
    fn rows_must_move_forward_in_time() {
        let text = "\
Date,Time,Price,Volume,Side
2026-03-16,09:00:01,1,1,B
2026-03-16,09:00:00,1,1,B
";
        let err = parse_file(text, ParseOptions::default()).unwrap_err();
        assert_eq!(err.kind, FormatErrorKind::OutOfOrder);
        assert_eq!(err.line, 3);
        // Equal timestamps are normal — several prints share a millisecond.
        let same = "\
Date,Time,Price,Volume,Side
2026-03-16,09:00:01,1,1,B
2026-03-16,09:00:01,1,1,S
";
        assert!(parse_file(same, ParseOptions::default()).is_ok());
    }

    #[test]
    fn a_bad_timezone_line_is_an_error_not_a_guess() {
        let text =
            "# timezone=Brazil/East\nDate,Time,Price,Volume,Side\n2026-03-16,09:00:00,1,1,B\n";
        let err = parse_file(text, ParseOptions::default()).unwrap_err();
        assert_eq!(err.kind, FormatErrorKind::Timezone);
        assert_eq!(err.line, 1);
    }

    #[test]
    fn utc_offsets_parse_in_every_common_spelling() {
        assert_eq!(UtcOffset::parse("UTC"), Some(UtcOffset::UTC));
        assert_eq!(UtcOffset::parse("Z"), Some(UtcOffset::UTC));
        assert_eq!(UtcOffset::parse("-03:00").unwrap().minutes(), -180);
        assert_eq!(UtcOffset::parse("-0300").unwrap().minutes(), -180);
        assert_eq!(UtcOffset::parse("-3").unwrap().minutes(), -180);
        assert_eq!(UtcOffset::parse("+05:30").unwrap().minutes(), 330);
        assert_eq!(UtcOffset::parse("+05:30").unwrap().label(), "+05:30");
        assert_eq!(UtcOffset::parse("nonsense"), None);
        assert_eq!(UtcOffset::parse("-03:99"), None);
    }

    #[test]
    fn written_files_round_trip() {
        let header = WriteHeader {
            symbol: "WINJ26".to_string(),
            timezone: UtcOffset::parse("-03:00").unwrap(),
            side_source: "bid_ask".to_string(),
            source: Some("mt5_orderflow_20260316.ndjson".to_string()),
        };
        let trades = vec![
            Trade {
                agg_id: 1,
                timestamp_ms: 1_773_666_068_000,
                price: dec("182035"),
                quantity: dec("12"),
                side: Side::Buy,
            },
            Trade {
                agg_id: 2,
                timestamp_ms: 1_773_666_069_250,
                price: dec("182025"),
                quantity: dec("4"),
                side: Side::Sell,
            },
        ];
        let quotes = [
            Quote {
                bid: dec("182030"),
                ask: dec("182035"),
            },
            Quote {
                bid: dec("182025"),
                ask: dec("182030"),
            },
        ];

        let mut text = write_header(&header);
        for (trade, quote) in trades.iter().zip(quotes) {
            write_trade(&mut text, trade, Some(quote), header.timezone);
        }

        let parsed = parse_file(&text, ParseOptions { keep_quotes: true }).unwrap();
        assert_eq!(parsed.trades, trades);
        assert_eq!(parsed.quotes, quotes);
        assert_eq!(parsed.header.symbol.as_deref(), Some("WINJ26"));
        assert_eq!(parsed.header.timezone, header.timezone);
        assert_eq!(parsed.header.side_source.as_deref(), Some("bid_ask"));
    }

    #[test]
    fn missing_quotes_are_written_empty_not_zero() {
        let mut text = String::new();
        write_trade(
            &mut text,
            &Trade {
                agg_id: 1,
                timestamp_ms: 0,
                price: dec("1"),
                quantity: dec("1"),
                side: Side::Buy,
            },
            None,
            UtcOffset::UTC,
        );
        assert_eq!(text, "1970-01-01,00:00:00.000,1,,,1,B\n");
    }

    #[test]
    fn epoch_millis_is_the_inverse_of_civil_parts() {
        let offset = UtcOffset::parse("-03:00").unwrap();
        let civil = CivilTime {
            year: 2026,
            month: 3,
            day: 16,
            hour: 10,
            minute: 1,
            second: 8,
            milli: 250,
        };
        let ms = civil.epoch_millis(offset);
        assert_eq!(civil_parts(ms, offset), civil);
        // Same instant, read in UTC, is three hours later on the clock.
        assert_eq!(civil_parts(ms, UtcOffset::UTC).time_field(), "13:01:08.250");
        // And it agrees with what the row parser produces.
        let text =
            "# timezone=-03:00\nDate,Time,Price,Volume,Side\n2026-03-16,10:01:08.250,1,1,B\n";
        let parsed = parse_file(text, ParseOptions::default()).unwrap();
        assert_eq!(parsed.trades[0].timestamp_ms, ms);
    }

    #[test]
    fn civil_days_round_trip_across_boundaries() {
        for (y, m, d) in [
            (1970, 1, 1),
            (1999, 12, 31),
            (2000, 3, 1),
            (2026, 3, 16),
            (2028, 2, 29),
            (2100, 3, 1),
        ] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d), "{y}-{m}-{d}");
        }
    }
}
