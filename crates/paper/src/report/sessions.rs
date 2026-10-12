//! Sessions: one journal file each, and the ledger's questions about them.
//!
//! A session is exactly one file the account wrote — `<stamp>.csv`, or
//! `<stamp>.rerun-N.csv` when the same tape was traded again — so three
//! replays of one recording are three sessions, never one. This module names
//! a session ([`SessionKey`]), says what its file holds ([`SessionInfo`]),
//! decides which rows a ledger filter keeps ([`LedgerFilter`]) and groups rows
//! by civil day and then by session ([`group_by_day_and_session`]). The
//! painting stays in `app`; an operator with no window gets the same answers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quantick_civil::{CivilDate, DateRange, TzOffset, civil_utc};
use quantick_sim::ClosedTrade;
use quantick_sim::history::{self, SessionSource, TradeTimes};
use rust_decimal::Decimal;

use super::SourceFilter;

/// One session file, named relative to the trades folder:
/// `<symbol folder>/<file>.csv`. Stable across restarts and the identifier
/// the control plane speaks, so it never carries an absolute path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionKey(Arc<str>);

impl SessionKey {
    /// Read a key a caller sent. `None` unless it is exactly one symbol
    /// folder and one `.csv` file name: no absolute path, no `..`, no
    /// hidden folder — a key can only ever name a file the ledger lists.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (folder, file) = text.split_once('/')?;
        let plain = |part: &str| {
            !part.is_empty()
                && !part.starts_with('.')
                && !part.contains(['/', '\\', ':'])
                && !part.chars().any(char::is_control)
        };
        let csv = file
            .strip_suffix(history::FILE_EXTENSION)
            .is_some_and(|stem| stem.ends_with('.') && stem.len() > 1);
        (plain(folder) && plain(file) && csv).then(|| Self(Arc::from(text)))
    }

    /// The key of `path` when it is a session file directly inside a symbol
    /// folder of `dir`.
    #[must_use]
    pub fn of(dir: &Path, path: &Path) -> Option<Self> {
        let relative = path.strip_prefix(dir).ok()?;
        let mut parts = relative.components();
        let folder = parts.next()?.as_os_str().to_str()?;
        let file = parts.next()?.as_os_str().to_str()?;
        if parts.next().is_some() {
            return None;
        }
        Self::parse(&format!("{folder}/{file}"))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Where the file lives under `dir`.
    #[must_use]
    pub fn path(&self, dir: &Path) -> PathBuf {
        let (folder, file) = self.parts();
        dir.join(folder).join(file)
    }

    /// The symbol folder and the file name.
    #[must_use]
    pub fn parts(&self) -> (&str, &str) {
        self.0.split_once('/').unwrap_or(("", &self.0))
    }

    /// `N` for a `.rerun-N.csv` file — the same tape traded again — else
    /// `None`.
    #[must_use]
    pub fn rerun(&self) -> Option<u32> {
        let stem = self.parts().1.strip_suffix(".csv")?;
        stem.rsplit_once(".rerun-")?.1.parse().ok()
    }
}

impl std::fmt::Display for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What one session file holds, read with the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub key: SessionKey,
    pub symbol: String,
    /// `None` — a file from before the source was recorded.
    pub source: Option<SessionSource>,
    /// The trader's name for it; `None` until one is given.
    pub name: Option<String>,
    /// A file this app is still writing: its trades are in the simulator and
    /// listed as the current session, and it can never be deleted.
    pub live: bool,
    pub trades: usize,
    /// Net points over its trades.
    pub net: Decimal,
    /// When its first trade closed — the moment its file was named after.
    pub first_closed_ms: Option<i64>,
    /// Each tagged trade's tags.
    pub tags: BTreeMap<TradeTimes, Vec<String>>,
}

impl SessionInfo {
    /// The session as one file read it.
    #[must_use]
    pub fn of(key: SessionKey, symbol: String, parsed: &history::TradeHistory, live: bool) -> Self {
        Self {
            key,
            symbol,
            source: parsed.source,
            name: parsed.name.clone(),
            live,
            trades: parsed.trades.len(),
            net: parsed.trades.iter().fold(Decimal::ZERO, |net, trade| {
                net.saturating_add(trade.pnl_points)
            }),
            first_closed_ms: parsed.trades.iter().map(|trade| trade.closed_ms).min(),
            tags: parsed.tags.clone(),
        }
    }

    /// One session read straight from its file under `dir`; `None` when it
    /// is missing or not a quantick-trades file.
    #[must_use]
    pub fn read(dir: &Path, key: &SessionKey, live: bool) -> Option<Self> {
        let text = std::fs::read_to_string(key.path(dir)).ok()?;
        let parsed = history::parse(&text).ok()?;
        let symbol = parsed
            .symbol
            .clone()
            .unwrap_or_else(|| key.parts().0.to_owned());
        Some(Self::of(key.clone(), symbol, &parsed, live))
    }

    /// The trader's name, or the default every unnamed session wears.
    #[must_use]
    pub fn label(&self, tz: TzOffset) -> String {
        self.name.clone().unwrap_or_else(|| self.default_label(tz))
    }

    /// `Replay · 13:05 · rerun 2`: where the trades came from, when the
    /// first one closed in the display timezone, and which run of the same
    /// tape this file is.
    #[must_use]
    pub fn default_label(&self, tz: TzOffset) -> String {
        let mut label = source_word(self.source).to_owned();
        if let Some(first) = self.first_closed_ms {
            let (.., hour, minute, _) = civil_utc(first.saturating_add(tz.offset_ms()));
            label.push_str(&format!(" · {hour:02}:{minute:02}"));
        }
        if let Some(rerun) = self.key.rerun() {
            label.push_str(&format!(" · rerun {rerun}"));
        }
        label
    }

    /// The tags this session's file gives one trade.
    #[must_use]
    pub fn tags_of(&self, trade: &ClosedTrade) -> &[String] {
        self.tags
            .get(&(trade.opened_ms, trade.closed_ms))
            .map_or(&[], Vec::as_slice)
    }
}

/// The word a session's source reads as.
#[must_use]
pub fn source_word(source: Option<SessionSource>) -> &'static str {
    match source {
        Some(SessionSource::Live) => "Live",
        Some(SessionSource::Replay) => "Replay",
        None => "Unrecorded",
    }
}

/// The ledger's search box, read: `#tag` tokens must all be on the trade,
/// every other word must appear in its session's label. Case-insensitive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LedgerQuery {
    pub tags: Vec<String>,
    pub words: Vec<String>,
}

impl LedgerQuery {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut query = Self::default();
        for token in text.split_whitespace() {
            if token.starts_with('#') {
                // A lone `#` is a tag still being typed, not a filter.
                if let Some(tag) = history::normalize_tag(token) {
                    query.tags.push(tag);
                }
            } else {
                query.words.push(token.to_lowercase());
            }
        }
        query
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.words.is_empty()
    }

    /// Whether a trade carrying `tags`, in a session labelled `label`, is
    /// what was asked for.
    #[must_use]
    pub fn admits(&self, tags: &[String], label: &str) -> bool {
        if !self.tags.iter().all(|wanted| tags.contains(wanted)) {
            return false;
        }
        if self.words.is_empty() {
            return true;
        }
        let label = label.to_lowercase();
        self.words.iter().all(|word| label.contains(word.as_str()))
    }
}

/// The ledger's filter row: source pills, a civil date span and the search
/// box. The default keeps everything, which is the ledger as it always was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerFilter {
    pub source: SourceFilter,
    pub range: Option<DateRange>,
    /// The search box's text, kept verbatim; [`LedgerQuery::parse`] reads it.
    pub search: String,
}

impl Default for LedgerFilter {
    fn default() -> Self {
        Self {
            source: SourceFilter::All,
            range: None,
            search: String::new(),
        }
    }
}

impl LedgerFilter {
    /// Whether this filter keeps every row.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.source == SourceFilter::All
            && self.range.is_none()
            && LedgerQuery::parse(&self.search).is_empty()
    }

    /// The filter, parsed once for a walk over many rows.
    #[must_use]
    pub fn matcher(&self, tz: TzOffset) -> LedgerMatcher {
        LedgerMatcher {
            source: self.source,
            range: self.range,
            query: LedgerQuery::parse(&self.search),
            tz,
        }
    }
}

/// A [`LedgerFilter`] ready to test rows.
pub struct LedgerMatcher {
    source: SourceFilter,
    range: Option<DateRange>,
    query: LedgerQuery,
    tz: TzOffset,
}

impl LedgerMatcher {
    /// Whether one trade stays in the list.
    #[must_use]
    pub fn admits(
        &self,
        source: Option<SessionSource>,
        trade: &ClosedTrade,
        tags: &[String],
        session_label: &str,
    ) -> bool {
        self.source.admits(source)
            && self
                .range
                .is_none_or(|range| range.contains_ms(trade.closed_ms, self.tz))
            && self.query.admits(tags, session_label)
    }
}

/// One session as a listing reports it: what its file says, and the trades
/// of it a filter kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSummary<'a> {
    pub info: &'a SessionInfo,
    /// The name, or the default label.
    pub label: String,
    /// Whether a tab is still journaling to it.
    pub live: bool,
    /// Its trades the filter kept, and their net points.
    pub trades: usize,
    pub net: Decimal,
    /// When the newest kept trade closed — the listing's order.
    pub newest_closed_ms: i64,
}

/// Every session holding at least one trade `filter` keeps, newest first,
/// and how many trades the filter kept out. `live` names the sessions a tab
/// is still writing; read `history` with nothing excluded, so their trades
/// are counted from the file like every other session's.
#[must_use]
pub fn summarize_sessions<'a>(
    history: &'a super::LoadedHistory,
    filter: &LedgerFilter,
    tz: TzOffset,
    live: &std::collections::BTreeSet<SessionKey>,
) -> (Vec<SessionSummary<'a>>, usize) {
    let labels: BTreeMap<&SessionKey, String> = history
        .sessions
        .iter()
        .map(|(key, info)| (key, info.label(tz)))
        .collect();
    let matcher = filter.matcher(tz);
    let mut kept: BTreeMap<&SessionKey, (usize, Decimal, i64)> = BTreeMap::new();
    let mut hidden = 0usize;
    for row in &history.rows {
        let Some(key) = row.session.as_ref() else {
            continue;
        };
        let label = labels.get(key).map_or("", String::as_str);
        if !matcher.admits(row.source, &row.trade, &row.tags, label) {
            hidden += 1;
            continue;
        }
        let entry = kept.entry(key).or_insert((0, Decimal::ZERO, i64::MIN));
        entry.0 += 1;
        entry.1 = entry.1.saturating_add(row.trade.pnl_points);
        entry.2 = entry.2.max(row.trade.closed_ms);
    }
    let mut sessions: Vec<SessionSummary<'a>> = kept
        .into_iter()
        .filter_map(|(key, (trades, net, newest_closed_ms))| {
            let info = history.sessions.get(key)?;
            Some(SessionSummary {
                info,
                label: labels.get(key).cloned().unwrap_or_default(),
                live: live.contains(key),
                trades,
                net,
                newest_closed_ms,
            })
        })
        .collect();
    sessions.sort_by(|a, b| {
        b.newest_closed_ms
            .cmp(&a.newest_closed_ms)
            .then_with(|| a.info.key.cmp(&b.info.key))
    });
    (sessions, hidden)
}

/// The keys of the files `live` names under `dir`.
#[must_use]
pub fn live_keys(dir: &Path, live: &[PathBuf]) -> std::collections::BTreeSet<SessionKey> {
    live.iter()
        .filter_map(|path| SessionKey::of(dir, path))
        .collect()
}

/// One civil day of the ledger, and the sessions that closed trades on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayGroup {
    pub day: CivilDate,
    pub count: usize,
    pub net: Decimal,
    /// In the order their newest trade appears.
    pub sessions: Vec<SessionGroup>,
}

/// One session's trades inside one day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionGroup {
    /// `None` for trades no file under the trades folder holds.
    pub key: Option<SessionKey>,
    pub count: usize,
    pub net: Decimal,
    /// Positions in the grouped input, in input order.
    pub items: Vec<usize>,
}

/// Group rows already in ledger order (newest first) by the civil day they
/// closed on, then by session. Sessions of one day may interleave in time —
/// three replays of one tape close trades at the same venue times — so a
/// session collects its rows wherever they fall in the day rather than
/// splitting at each interleave.
pub fn group_by_day_and_session<'a>(
    items: impl IntoIterator<Item = (Option<&'a SessionKey>, &'a ClosedTrade)>,
    tz: TzOffset,
) -> Vec<DayGroup> {
    let mut days: Vec<DayGroup> = Vec::new();
    for (index, (key, trade)) in items.into_iter().enumerate() {
        let day = CivilDate::from_ms(trade.closed_ms, tz);
        if days.last().is_none_or(|last| last.day != day) {
            days.push(DayGroup {
                day,
                count: 0,
                net: Decimal::ZERO,
                sessions: Vec::new(),
            });
        }
        let group = days.last_mut().expect("pushed above");
        group.count += 1;
        group.net = group.net.saturating_add(trade.pnl_points);
        let session = match group
            .sessions
            .iter()
            .position(|session| session.key.as_ref() == key)
        {
            Some(position) => &mut group.sessions[position],
            None => {
                group.sessions.push(SessionGroup {
                    key: key.cloned(),
                    count: 0,
                    net: Decimal::ZERO,
                    items: Vec::new(),
                });
                group.sessions.last_mut().expect("pushed above")
            }
        };
        session.count += 1;
        session.net = session.net.saturating_add(trade.pnl_points);
        session.items.push(index);
    }
    days
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod sessions_tests;
