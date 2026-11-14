//! The feed-status controls the scene lists, in the order the popup paints them.

use super::*;

/// What [`feed_status_controls`] lists, as the scene names each entry:
/// id, label, capability and the refusal code when it is refused.
fn named(view: FeedStatusView<'_>) -> Vec<(&'static str, String, &'static str, Option<&str>)> {
    feed_status_controls(view)
        .into_iter()
        .map(|entry| {
            let control = entry.control;
            (
                control.control_id(),
                entry.label,
                control.capability_id(),
                entry.refused,
            )
        })
        .collect()
}

fn view<'a>(
    roll: Option<Result<&'a str, &'a str>>,
    primary: Recovery,
    popup_open: bool,
) -> FeedStatusView<'a> {
    FeedStatusView {
        roll,
        primary,
        popup_open,
        offered: true,
    }
}

fn reconnect_entry() -> (&'static str, String, &'static str, Option<&'static str>) {
    (
        "feed_status.reconnect",
        "Reconnect".to_owned(),
        "feed.reconnect",
        None,
    )
}

fn reload_entry() -> (&'static str, String, &'static str, Option<&'static str>) {
    (
        "feed_status.reload",
        "Reload".to_owned(),
        "feed.reload",
        None,
    )
}

/// The scene names what the chip and the popup paint: a roll on offer stands
/// where Reconnect would and leads the popup, under the roll's capability.
#[test]
fn a_roll_on_offer_stands_in_the_scene_as_the_roll() {
    let roll = || {
        (
            "feed_status.roll",
            "Roll to WINZ26".to_owned(),
            "feed.contract.roll",
            None,
        )
    };
    let offered = Some(Ok("WINZ26"));
    assert_eq!(named(view(offered, Recovery::Reconnect, false)), [roll()]);
    assert_eq!(
        named(view(None, Recovery::Reconnect, false)),
        [reconnect_entry()]
    );
    let hidden = FeedStatusView {
        offered: false,
        ..view(offered, Recovery::Reconnect, false)
    };
    assert_eq!(named(hidden), []);
    assert_eq!(
        named(view(offered, Recovery::Reconnect, true)),
        [roll(), reconnect_entry(), reload_entry()]
    );
    assert_eq!(
        named(view(None, Recovery::Reconnect, true)),
        [reconnect_entry(), reload_entry()]
    );
}

/// The popup fills and paints the stall's own recovery first — Reload for a
/// connected feed gone silent — and the scene lists it first too.
#[test]
fn the_scene_lists_the_popup_s_primary_first() {
    assert_eq!(
        named(view(None, Recovery::Reload, true)),
        [reload_entry(), reconnect_entry()]
    );
    assert_eq!(
        named(view(Some(Ok("WINZ26")), Recovery::Reload, true))[1..],
        [reload_entry(), reconnect_entry()]
    );
    assert_eq!(
        named(view(None, Recovery::Reload, false)),
        [reconnect_entry()],
        "the one-click act outside the popup is Reconnect"
    );
}

/// A refused roll is in the popup as its reason; the scene lists it as a
/// roll that cannot run, with the precondition a call would fail on.
#[test]
fn a_refused_roll_is_listed_unavailable_with_its_reason() {
    let reason = RollRefusal::PaperNotFlat.reason();
    let entries = feed_status_controls(view(Some(Err(reason)), Recovery::Reconnect, true));
    let roll = &entries[0];
    assert_eq!(roll.control, FeedStatusControl::Roll);
    assert_eq!(roll.label, "Roll to the next contract");
    assert_eq!(roll.refused, Some("feed.contract.paper_flat"));
    assert_eq!(
        roll.description.as_deref(),
        Some(
            "Cannot roll to the next contract: \
             Close the paper position and cancel working orders first."
        )
    );
    assert_eq!(
        named(view(Some(Err(reason)), Recovery::Reconnect, true))[1..],
        [reconnect_entry(), reload_entry()]
    );
    assert_eq!(
        named(view(Some(Err(reason)), Recovery::Reconnect, false)),
        [reconnect_entry()],
        "outside the popup nothing paints the refused roll"
    );
    let unknown = feed_status_controls(view(Some(Err("Some reason")), Recovery::Reconnect, true));
    assert_eq!(unknown[0].refused, Some("roll_refused"));
}

/// Every refusal reads back from its own sentence.
#[test]
fn a_refusal_reads_back_from_its_sentence() {
    for refusal in [
        RollRefusal::Replay,
        RollRefusal::NotConfigured,
        RollRefusal::TargetNotOffered,
        RollRefusal::PaperNotFlat,
        RollRefusal::GestureInFlight,
    ] {
        assert_eq!(RollRefusal::from_reason(refusal.reason()), Some(refusal));
    }
    assert_eq!(RollRefusal::from_reason("no such refusal"), None);
}
