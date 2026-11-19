//! Session and civil-day report scopes shared by the ledger and control plane.
use std::path::Path;

use quantick_civil::{CivilDate, TzOffset};
use rust_decimal::Decimal;

use super::session_edit::{SessionEditError, resolve};
use super::sessions::SessionKey;
use super::{HistoryRow, LoadedHistory, SourceFilter, journals};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportRequest {
    pub session: SessionKey,
    /// None selects the whole session, even when it spans midnight.
    pub day: Option<CivilDate>,
}

impl ReportRequest {
    pub fn source(&self) -> SourceFilter {
        if self.session.is_broker() {
            SourceFilter::Broker
        } else {
            SourceFilter::Simulator
        }
    }

    pub fn admits(&self, row: &HistoryRow, tz: TzOffset) -> bool {
        match self.day {
            None => row.session.as_ref() == Some(&self.session),
            Some(day) => {
                row.session
                    .as_ref()
                    .is_some_and(|key| key.is_broker() == self.session.is_broker())
                    && CivilDate::from_ms(row.trade.closed_ms, tz) == day
            }
        }
    }
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

pub fn prepare(
    dir: &Path,
    mut request: ReportRequest,
    tz: TzOffset,
) -> Result<PreparedReport, SessionEditError> {
    request.session = resolve(dir, &request.session)?.0;
    let history = journals::load(dir, None, &[]);
    let info = history.sessions.get(&request.session).ok_or_else(|| {
        SessionEditError::Unreadable(
            request.session.clone(),
            "no readable session history".to_owned(),
        )
    })?;
    if let Some(day) = request.day
        && !history.rows.iter().any(|row| {
            row.session.as_ref() == Some(&request.session)
                && CivilDate::from_ms(row.trade.closed_ms, tz) == day
        })
    {
        return Err(SessionEditError::DayNotInSession(day));
    }
    let title = match request.day {
        Some(day) => format!("{} · {}", request.source().label(), day.iso()),
        None => format!("{} · {}", request.source().label(), info.label(tz)),
    };
    let mut trades = 0;
    let mut net_points = Decimal::ZERO;
    for row in history.rows.iter().filter(|row| request.admits(row, tz)) {
        trades += 1;
        net_points = net_points.saturating_add(row.trade.pnl_points);
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
