//! What a broker said, in the log: every refusal and every notice once, at
//! the one place the account handles the venue's events. A toast is gone in
//! seconds; the log is what is left to read after the session.

use quantick_trading::{RejectReason, VenueEvent, VenueNotice};

/// The log line one broker event earns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrokerLog {
    /// The venue, or its adapter before it, refused.
    Refused {
        /// The venue's own return code; `None` when it never answered one.
        retcode: Option<i64>,
        /// The venue's words, or the adapter's sentence, verbatim.
        comment: String,
        /// Which request was refused, when the refusal answers one.
        request: Option<&'static str>,
        /// A protective price dropped at the fill rather than a request
        /// refused outright.
        at_fill: bool,
    },
    /// The venue said something about itself: its link, its account.
    Notice(String),
}

/// The line `event` earns, or `None` for an event that is no refusal and
/// no notice (fills and closes have their own journal; an acknowledgement
/// is every order's routine).
pub(crate) fn broker_log(event: &VenueEvent) -> Option<BrokerLog> {
    let (reason, at_fill) = match event {
        VenueEvent::Rejected(reason) => (reason, false),
        VenueEvent::BracketDropped { reason } => (reason, true),
        VenueEvent::Notice(VenueNotice::Acknowledged { .. }) => return None,
        VenueEvent::Notice(notice) => return Some(BrokerLog::Notice(notice.to_string())),
        _ => return None,
    };
    Some(match reason {
        RejectReason::Venue {
            code,
            message,
            request,
        } => BrokerLog::Refused {
            retcode: *code,
            comment: message.to_string(),
            request: request.map(|request| request.as_str()),
            at_fill,
        },
        other => BrokerLog::Refused {
            retcode: None,
            comment: other.to_string(),
            request: None,
            at_fill,
        },
    })
}

/// Log `event` from the broker on `route` trading `symbol`, if it earns a
/// line.
pub(crate) fn log_broker_event(event: &VenueEvent, route: &str, symbol: &str) {
    match broker_log(event) {
        Some(BrokerLog::Refused {
            retcode,
            comment,
            request,
            at_fill,
        }) => tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "BROKER_ORDER_REFUSED",
            route,
            symbol,
            retcode,
            comment = %comment,
            request,
            at_fill,
            "the broker refused"
        ),
        Some(BrokerLog::Notice(notice)) => tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "BROKER_NOTICE",
            route,
            symbol,
            notice = %notice,
            "the broker said"
        ),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use quantick_trading::{OrderId, VenueRequest, VenueText};

    use super::*;

    #[test]
    fn a_venue_refusal_is_logged_with_its_code_words_and_request() {
        let refused = VenueEvent::Rejected(RejectReason::Venue {
            code: Some(10016),
            message: VenueText::new("Invalid stops"),
            request: Some(VenueRequest::Stops),
        });
        assert_eq!(
            broker_log(&refused),
            Some(BrokerLog::Refused {
                retcode: Some(10016),
                comment: "Invalid stops".to_owned(),
                request: Some("stops"),
                at_fill: false,
            })
        );
    }

    #[test]
    fn an_adapter_refusal_and_a_dropped_bracket_are_refusals_too() {
        let blocked = VenueEvent::Rejected(RejectReason::Blocked("no link with the broker"));
        assert_eq!(
            broker_log(&blocked),
            Some(BrokerLog::Refused {
                retcode: None,
                comment: "no link with the broker".to_owned(),
                request: None,
                at_fill: false,
            })
        );
        let dropped = VenueEvent::BracketDropped {
            reason: RejectReason::NoPosition,
        };
        assert!(matches!(
            broker_log(&dropped),
            Some(BrokerLog::Refused { at_fill: true, .. })
        ));
    }

    #[test]
    fn a_notice_is_logged_and_an_acknowledgement_is_not() {
        assert_eq!(
            broker_log(&VenueEvent::Notice(VenueNotice::LinkLost)),
            Some(BrokerLog::Notice(VenueNotice::LinkLost.to_string()))
        );
        let ack = VenueEvent::Notice(VenueNotice::Acknowledged {
            id: OrderId(1),
            venue_ref: 501,
        });
        assert_eq!(broker_log(&ack), None);
    }
}
