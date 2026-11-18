use super::*;

#[test]
fn a_one_switch_is_exactly_one() {
    assert!(one(Some("1")));
    for value in [None, Some(""), Some(" 1"), Some("true"), Some("0")] {
        assert!(!one(value), "{value:?}");
    }
}

#[test]
fn binary_and_flag_refuse_what_they_cannot_read() {
    assert_eq!((binary(" 1 "), binary("0")), (Some(true), Some(false)));
    assert_eq!(binary("on"), None);
    for (value, read) in [("on", Some(true)), ("false", Some(false)), ("yes", None)] {
        assert_eq!(flag(value), read, "{value:?}");
    }
}

#[test]
fn counts_refuse_zero_negatives_and_text() {
    assert_eq!(positive(" 8 "), Some(8));
    assert_eq!(
        (positive("0"), positive("-1"), positive("x")),
        (None, None, None)
    );
    assert_eq!(non_negative_ms("0"), Some(0));
    assert_eq!((non_negative_ms("-5"), non_negative_ms("8s")), (None, None));
}

#[test]
fn names_are_trimmed_and_blanks_dropped() {
    assert_eq!(names(" a, ,b ,").collect::<Vec<_>>(), ["a", "b"]);
}

#[test]
fn lane_layers_take_the_last_word_and_none_turns_both_off() {
    let read = lane_layers("heatmap, no-bubbles, typo, no-heatmap");
    assert_eq!((read.depth, read.bubbles), (Some(false), Some(false)));
    assert_eq!(lane_layers("bubbles").depth, None);
    let none = lane_layers("heatmap,none");
    assert_eq!((none.depth, none.bubbles), (Some(false), Some(false)));
}

#[test]
fn replay_autostart_speed_and_session_read_their_grammar() {
    assert_eq!(replay_autostart(Some(" 1 ")), Some(true));
    assert_eq!(replay_autostart(Some("paused")), Some(false));
    assert_eq!(
        (replay_autostart(Some("play")), replay_autostart(None)),
        (None, None)
    );
    assert!((replay_speed(Some(" 4 ")) - 4.0).abs() < f32::EPSILON);
    for value in [None, Some("0"), Some("-2"), Some("fast")] {
        assert!(
            (replay_speed(value) - 1.0).abs() < f32::EPSILON,
            "{value:?}"
        );
    }
    assert_eq!(
        non_blank(Some(" 2026-11-17 ")).as_deref(),
        Some("2026-11-17")
    );
    assert_eq!(non_blank(Some("  ")), None);
}

#[test]
fn a_context_fold_needs_exactly_one_side() {
    assert_eq!(context_fold(Some("1"), None), Some(true));
    assert_eq!(context_fold(None, Some("1")), Some(false));
    assert_eq!(context_fold(Some("1"), Some("1")), None);
    assert_eq!(context_fold(None, Some("0")), None);
}
