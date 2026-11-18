//! The link's own bookkeeping: arrivals, latency, the entry pause's readings.

use super::*;
use quantick_sources::recovery::RollOffer;

/// A link over channels the test speaks on.
fn link() -> (FeedLink, mpsc::Sender<FeedNotice>) {
    let (handle, notices) = handle();
    (FeedLink::new(handle, 0), notices)
}

#[test]
fn a_live_print_books_its_delay_age_count_and_intake_sample() {
    let (mut link, _notices) = link();
    link.observe_live_trade_at(1_000, 600, 1_003);
    link.observe_live_trade_at(1_000, 1_200, 1_009);
    assert_eq!(
        link.latest_trade_latency_ms,
        Some(-200),
        "signed, as measured"
    );
    assert_eq!((link.latest_trade_ms, link.live_trades), (Some(1_200), 2));
    let lag = link.intake_lag.summary().expect("two samples");
    assert_eq!((lag.samples, lag.p50_ms, lag.max_ms), (2, 3, 9));
    link.attach(handle().0, 2_000);
    assert_eq!(
        link.intake_lag.summary(),
        None,
        "a new session, a new window"
    );
}

/// The pause ages the tape from when prints and book events arrived
/// here: a venue clock hours off moves nothing, a silent wire does.
#[test]
fn the_pause_ages_the_tape_by_local_arrival_not_the_venue_stamp() {
    use crate::sync::{SyncLimits, SyncReading};
    let (mut link, _notices) = link();
    streaming(&mut link);
    link.set_connection(FeedConnectionState::Connected, 999_000);
    let limits = SyncLimits::default();
    let at = |link: &mut FeedLink, now| link.sync_at("WINZ26", false, limits, now);
    assert_eq!(at(&mut link, 1_000_000), None, "connected a second ago");
    // A venue stamp three hours behind the local clock.
    link.observe_live_trade_at(1_000_000, 1_000_000 - 10_800_000, 1_000_000);
    assert_eq!(at(&mut link, 1_004_999), None, "an offset clock is not age");
    link.observe_book_arrival(1_004_000);
    assert_eq!(at(&mut link, 1_008_999), None, "the book still arrives");
    let paused = at(&mut link, 1_009_000).expect("five seconds of silence");
    assert_eq!(paused.reading, SyncReading::TapeAge);
    assert_eq!(paused.tape_age_ms, Some(5_000));
}

fn streaming(link: &mut FeedLink) {
    link.feed_capabilities = crate::fixed_capabilities(FeedCapabilities {
        transport: true,
        ..FeedCapabilities::none()
    });
}

/// A new session starts its own silence: the old session's arrivals do
/// not keep it fresh, and a connected feed that never streams pauses once
/// the tape-age limit has run from when it connected.
#[test]
fn a_connected_feed_that_never_streams_pauses_after_the_tape_age_limit() {
    use crate::sync::{SyncLimits, SyncReading};
    let limits = SyncLimits::default();
    let (mut link, _notices) = link();
    streaming(&mut link);
    link.set_connection(FeedConnectionState::Connected, 500);
    link.observe_book_arrival(1_000);
    link.attach(handle().0, 2_000);
    streaming(&mut link);
    link.set_connection(FeedConnectionState::Connected, 3_000);
    let at = |link: &mut FeedLink, now| link.sync_at("WINZ26", false, limits, now);
    assert_eq!(
        at(&mut link, 7_999),
        None,
        "the limit runs from the connect"
    );
    let paused = at(&mut link, 8_000).expect("connected and silent");
    assert_eq!(
        (paused.reading, paused.tape_age_ms),
        (SyncReading::TapeAge, Some(5_000))
    );
    link.observe_book_arrival(8_500);
    assert_eq!(at(&mut link, 9_000), None, "the first arrival ends it");
}

/// The link's word for the pause: the chart's offline latch, else the
/// feed's own connect. A book snapshot drained while it connects is no
/// arrival: it neither lifts the pause nor freshens the tape.
#[test]
fn the_pause_reads_the_transport_from_the_feed_and_the_charts_latch() {
    use crate::sync::{LinkState, SyncLimits};
    let limits = SyncLimits::default();
    let (mut link, _notices) = link();
    streaming(&mut link);
    let word = |link: &mut FeedLink, offline| {
        (link.sync_at("WINZ26", offline, limits, 10)).map(|paused| paused.link)
    };
    assert_eq!(word(&mut link, false), Some(LinkState::Attempting));
    link.observe_book_arrival(5);
    assert_eq!(
        word(&mut link, false),
        Some(LinkState::Attempting),
        "a snapshot"
    );
    link.set_connection(FeedConnectionState::Connected, 6);
    assert_eq!(word(&mut link, false), None, "connected, nothing old yet");
    let age = link
        .sync_at("WINZ26", false, limits, 5_006)
        .map(|paused| paused.tape_age_ms);
    assert_eq!(age, Some(Some(5_000)), "the snapshot never freshened it");
    assert_eq!(word(&mut link, true), Some(LinkState::Offline));
    link.set_connection(FeedConnectionState::Reconnecting, 8);
    assert_eq!(word(&mut link, false), Some(LinkState::Attempting));
}

fn handle() -> (FeedHandle, mpsc::Sender<FeedNotice>) {
    let (notices, notice_rx) = mpsc::channel(16);
    let handle = FeedHandle {
        events: mpsc::channel(1).1,
        book_events: mpsc::channel(1).1,
        notices: notice_rx,
        capabilities: crate::fixed_capabilities(FeedCapabilities::none()),
        latency: crate::unsplit_latency(),
        commands: mpsc::channel(1).0,
        replay: None,
    };
    (handle, notices)
}

fn unlisted(symbol: &str) -> FeedNotice {
    FeedNotice::attention(format!("MetaTrader does not list {symbol}"), "Add it.")
        .with_unlisted(Some(symbol.to_owned()))
}

fn say(link: &mut FeedLink, notices: &mpsc::Sender<FeedNotice>, notice: FeedNotice) {
    notices.try_send(notice).expect("room on the channel");
    link.drain_notices_at(1);
}

#[test]
fn an_unlisted_contract_stays_named_through_retries_and_the_give_up() {
    let (mut link, notices) = link();
    say(&mut link, &notices, unlisted("WINV26"));
    link.auto_roll
        .set_offer(Some(offer("WINV26", Ok("WINZ26"))));
    assert_eq!(link.quick_action(), NoticeAction::Roll("WINZ26".to_owned()));

    say(
        &mut link,
        &notices,
        FeedNotice::working("starting the MetaTrader bridge"),
    );
    say(
        &mut link,
        &notices,
        FeedNotice::reconnecting("MetaTrader 5 disconnected"),
    );
    say(&mut link, &notices, FeedNotice::Clear);
    assert_eq!(link.notice, unlisted("WINV26"), "a retry says nothing new");
    assert_eq!(link.feed_connection, FeedConnectionState::Reconnecting);

    // The bridge's give-up names the contract its last attempt reported.
    say(
        &mut link,
        &notices,
        FeedNotice::attention("the MetaTrader bridge would not stay running", "Fix it.")
            .with_unlisted(Some("WINV26".to_owned())),
    );
    assert_eq!(link.notice.unlisted_symbol(), Some("WINV26"));
    assert_eq!(link.roll_offer(), Some(Ok("WINZ26")));
    assert_eq!(link.roll_note(), Some(RollNote::To("WINZ26")));

    say(&mut link, &notices, FeedNotice::Connected);
    assert_eq!(
        link.notice,
        FeedNotice::Clear,
        "a session that connects ends it"
    );
    assert_eq!(link.roll_offer(), None);
    assert_eq!(link.quick_action(), NoticeAction::Reconnect);
}

fn offer(from: &str, to: Result<&str, &'static str>) -> RollOffer {
    RollOffer {
        from: from.to_owned(),
        to: to.map(str::to_owned),
    }
}

/// SELECT_FAILED proves the contract is listed: it takes the unlisted
/// contract, its roll and the licence's target away, whatever clears later.
#[test]
fn any_other_attention_drops_the_unlisted_contract_and_its_roll() {
    let (mut link, notices) = link();
    link.arm_auto_roll("WINV26");
    say(&mut link, &notices, unlisted("WINV26"));
    link.auto_roll.set_offer(Some(offer(
        "WINV26",
        Err("Finish the drawing gesture first"),
    )));
    assert_eq!(link.notice.unlisted_symbol(), Some("WINV26"));
    for other in [
        FeedNotice::attention("MetaTrader would not open WINV26", "Retry."),
        FeedNotice::attention("MetaTrader 5 is not running", "Start it."),
    ] {
        say(&mut link, &notices, unlisted("WINV26"));
        say(&mut link, &notices, other.clone());
        assert_eq!(link.notice, other);
        // The refusal clears: the next settle finds the roll again.
        link.auto_roll
            .set_offer(Some(offer("WINV26", Ok("WINZ26"))));
        assert_eq!(
            link.notice.unlisted_symbol(),
            None,
            "no contract to roll from"
        );
        assert_eq!(link.roll_offer(), None, "no roll_to");
        assert_eq!(link.quick_action(), NoticeAction::Reconnect);
        say(&mut link, &notices, FeedNotice::Clear);
        assert_eq!(link.notice, FeedNotice::Clear, "nothing holds it any more");
    }
}

/// An import that adopts a tab whose feed already connected or streamed
/// does not license it to roll itself; a fresh restored tab is licensed.
#[test]
fn a_link_that_streamed_is_not_armed() {
    let (mut fresh, _notices) = link();
    fresh.arm_auto_roll("WINV26");
    assert!(fresh.auto_roll.armed_for("WINV26"));

    let (mut live, notices) = link();
    say(&mut live, &notices, FeedNotice::Connected);
    say(
        &mut live,
        &notices,
        FeedNotice::reconnecting("MetaTrader 5 disconnected"),
    );
    live.arm_auto_roll("WINV26");
    assert!(!live.auto_roll.is_armed(), "it connected once");

    let (mut streaming, _notices) = link();
    streaming.live_trades = 1;
    streaming.arm_auto_roll("WINV26");
    assert!(!streaming.auto_roll.is_armed(), "it streamed");
}

#[test]
fn an_offer_about_another_contract_is_not_offered() {
    let (mut link, notices) = link();
    say(&mut link, &notices, unlisted("WINV26"));
    link.auto_roll
        .set_offer(Some(offer("WINQ26", Ok("WINV26"))));
    assert_eq!(link.roll_offer(), None);
    link.auto_roll.set_offer(Some(offer(
        "WINV26",
        Err("Finish the drawing gesture first"),
    )));
    assert_eq!(
        link.roll_note(),
        Some(RollNote::Refused("Finish the drawing gesture first"))
    );
    assert_eq!(link.quick_action(), NoticeAction::Reconnect);
}

fn rolled_note() -> Option<Rolled> {
    Some(Rolled {
        from: "WINV26".to_owned(),
        to: "WINZ26".to_owned(),
    })
}

/// A link that just rolled WINV26 to WINZ26, speaking for the new feed.
fn rolled_link() -> (FeedLink, mpsc::Sender<FeedNotice>) {
    let (mut link, _old) = link();
    link.auto_roll.rolling("WINV26", "WINZ26", true);
    let (next, notices) = handle();
    link.attach(next, 5);
    assert_eq!(link.notice, FeedNotice::Clear, "the new feed starts clean");
    assert_eq!(link.rolled, rolled_note());
    (link, notices)
}

/// The note and the notice are two slots: every notice the new feed
/// says reaches the notice, before and after its connect, and none of
/// them takes the note away.
#[test]
fn a_roll_note_stands_beside_every_notice() {
    let (mut link, notices) = rolled_link();
    for notice in [
        FeedNotice::working("waiting for a MetaTrader bridge on 127.0.0.1:5555"),
        FeedNotice::working("waiting for the MetaTrader bridge to connect"),
        FeedNotice::Clear,
        FeedNotice::Connected,
        FeedNotice::working("loading WINZ26 history"),
        FeedNotice::reconnecting("MetaTrader 5 disconnected"),
        FeedNotice::attention("MetaTrader is not running", "Start it."),
        FeedNotice::Clear,
    ] {
        say(&mut link, &notices, notice.clone());
        let shown = match notice {
            FeedNotice::Connected => FeedNotice::Clear,
            other => other,
        };
        assert_eq!(link.notice, shown, "the notice says what the feed said");
        assert_eq!(link.rolled, rolled_note(), "the note stands");
    }
    assert_eq!(
        link.rolled.as_ref().map(Rolled::sentence).as_deref(),
        Some("WINV26 is no longer listed; rolled to WINZ26")
    );
    assert_eq!(link.notice.unlisted_symbol(), None, "the note is no report");
}

/// The next attach — a switch, a reconnect, a reload — takes the note
/// away; the attach after another roll explains that one.
#[test]
fn the_next_attach_replaces_the_roll_note() {
    let (mut link, _notices) = rolled_link();
    let (next, _notices) = handle();
    link.attach(next, 7);
    assert_eq!(link.rolled, None, "an attach that rolled nothing is clean");

    link.auto_roll.rolling("WINZ26", "WING27", true);
    let (next, _notices) = handle();
    link.attach(next, 9);
    assert_eq!(
        link.rolled,
        Some(Rolled {
            from: "WINZ26".to_owned(),
            to: "WING27".to_owned(),
        })
    );
}
