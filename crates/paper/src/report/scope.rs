//! Session, civil-day and tag report scopes shared by the ledger and control plane.
use std::path::Path;

use quantick_civil::{CivilDate, TzOffset};
use quantick_sim::history::normalize_tag;
use rust_decimal::Decimal;

use super::session_edit::{SessionEditError, resolve};
use super::sessions::SessionKey;
use super::{HistoryRow, LoadedHistory, SourceFilter, journals};

/// Which book a day or tag report reads. Simulator and broker results are
/// never summed into one figure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReportOrigin {
    Simulator,
    Broker,
}

impl ReportOrigin {
    #[must_use]
    pub fn of(session: &SessionKey) -> Self {
        if session.is_broker() {
            Self::Broker
        } else {
            Self::Simulator
        }
    }

    #[must_use]
    pub fn source(self) -> SourceFilter {
        match self {
            Self::Simulator => SourceFilter::Simulator,
            Self::Broker => SourceFilter::Broker,
        }
    }

    /// The stable wire token: `simulator` or `broker`.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Simulator => "simulator",
            Self::Broker => "broker",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportRequest {
    /// One session. `day` narrows it to a civil day it holds a trade on,
    /// and then reads every session of its origin on that day.
    Session {
        session: SessionKey,
        day: Option<CivilDate>,
    },
    /// Every trade of the origin closed on the civil day, in any session.
    Day {
        origin: ReportOrigin,
        day: CivilDate,
    },
    /// Every trade of the origin carrying the tag, over all history. The tag
    /// is normalized as the journal stores it: no `#`, lowercased.
    Tag { origin: ReportOrigin, tag: String },
}

impl ReportRequest {
    #[must_use]
    pub fn origin(&self) -> ReportOrigin {
        match self {
            Self::Session { session, .. } => ReportOrigin::of(session),
            Self::Day { origin, .. } | Self::Tag { origin, .. } => *origin,
        }
    }

    pub fn source(&self) -> SourceFilter {
        self.origin().source()
    }

    /// The civil day the scope is pinned to, if any.
    #[must_use]
    pub fn day(&self) -> Option<CivilDate> {
        match self {
            Self::Session { day, .. } => *day,
            Self::Day { day, .. } => Some(*day),
            Self::Tag { .. } => None,
        }
    }

    /// A session header's reports: the whole session, then the day it heads.
    #[must_use]
    pub fn sessions(session: &SessionKey, day: CivilDate) -> [Self; 2] {
        [None, Some(day)].map(|day| Self::Session {
            session: session.clone(),
            day,
        })
    }

    /// A day header's reports: one per origin it holds.
    pub fn days(day: CivilDate, origins: &[ReportOrigin]) -> impl Iterator<Item = Self> + '_ {
        origins.iter().map(move |&origin| Self::Day { origin, day })
    }

    /// A trade's reports: one per tag it carries, over its session's origin.
    pub fn tags<'a>(session: &SessionKey, tags: &'a [String]) -> impl Iterator<Item = Self> + 'a {
        let origin = ReportOrigin::of(session);
        tags.iter().map(move |tag| Self::Tag {
            origin,
            tag: tag.clone(),
        })
    }

    /// What a context menu's button for this scope reads.
    #[must_use]
    pub fn menu_label(&self) -> String {
        match self {
            Self::Session { day: None, .. } => "Session report".to_owned(),
            Self::Session { day: Some(_), .. } => "Day report".to_owned(),
            Self::Day { origin, .. } => format!("Day report · {}", origin.source().label()),
            Self::Tag { tag, .. } => format!("Report #{tag}"),
        }
    }

    pub fn admits(&self, row: &HistoryRow, tz: TzOffset) -> bool {
        match self {
            Self::Session { session, day: None } => row.session.as_ref() == Some(session),
            Self::Session {
                session,
                day: Some(day),
            } => {
                of_origin(row, ReportOrigin::of(session))
                    && CivilDate::from_ms(row.trade.closed_ms, tz) == *day
            }
            Self::Day { origin, day } => {
                of_origin(row, *origin) && CivilDate::from_ms(row.trade.closed_ms, tz) == *day
            }
            Self::Tag { origin, tag } => of_origin(row, *origin) && row.tags.contains(tag),
        }
    }

    fn title(&self, history: &LoadedHistory, tz: TzOffset) -> String {
        let origin = self.source().label();
        match self {
            Self::Session { day: Some(day), .. } | Self::Day { day, .. } => {
                format!("{origin} · {}", day.iso())
            }
            Self::Session { session, day: None } => {
                let label = history
                    .sessions
                    .get(session)
                    .map(|info| info.label(tz))
                    .unwrap_or_default();
                format!("{origin} · {label}")
            }
            Self::Tag { tag, .. } => format!("{origin} · #{tag}"),
        }
    }
}

fn of_origin(row: &HistoryRow, origin: ReportOrigin) -> bool {
    row.session
        .as_ref()
        .is_some_and(|key| ReportOrigin::of(key) == origin)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportReceipt {
    pub request: ReportRequest,
    pub title: String,
    pub trades: usize,
    pub net_points: Decimal,
}

pub struct PreparedReport {
    pub receipt: ReportReceipt,
    pub history: LoadedHistory,
    pub symbols: Vec<String>,
}

/// Resolves the request against the journal under `dir` and counts what it
/// admits. A session must be readable (and hold a trade on its day, when one
/// is given); a day or tag scope must admit at least one trade.
pub fn prepare(
    dir: &Path,
    mut request: ReportRequest,
    tz: TzOffset,
) -> Result<PreparedReport, SessionEditError> {
    match &mut request {
        ReportRequest::Session { session, .. } => *session = resolve(dir, session)?.0,
        ReportRequest::Tag { tag, .. } => {
            *tag = normalize_tag(tag)
                .ok_or_else(|| SessionEditError::InvalidTag(format!("`{tag}` is not one tag")))?;
        }
        ReportRequest::Day { .. } => {}
    }
    let history = journals::load(dir, None, &[]);
    if let ReportRequest::Session { session, day } = &request {
        if !history.sessions.contains_key(session) {
            return Err(SessionEditError::Unreadable(
                session.clone(),
                "no readable session history".to_owned(),
            ));
        }
        if let Some(day) = *day
            && !history.rows.iter().any(|row| {
                row.session.as_ref() == Some(session)
                    && CivilDate::from_ms(row.trade.closed_ms, tz) == day
            })
        {
            return Err(SessionEditError::DayNotInSession(day));
        }
    }
    let mut trades = 0;
    let mut net_points = Decimal::ZERO;
    for row in history.rows.iter().filter(|row| request.admits(row, tz)) {
        trades += 1;
        net_points = net_points.saturating_add(row.trade.pnl_points);
    }
    let title = request.title(&history, tz);
    if trades == 0 && !matches!(request, ReportRequest::Session { .. }) {
        return Err(SessionEditError::EmptyScope(title));
    }
    Ok(PreparedReport {
        receipt: ReportReceipt {
            request,
            title,
            trades,
            net_points,
        },
        history,
        symbols: journals::symbols(dir),
    })
}
