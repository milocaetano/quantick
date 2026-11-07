use super::*;

/// The rule the captures found: a chip and a gridline label on the same
/// row read as neither. The round number stands aside — it is the part
/// nobody needs, and the chip is the answer somebody asked for.
#[test]
fn a_label_under_a_chip_stands_aside() {
    let label = 14.0_f32;
    let chip = [400.0_f32];
    assert!(claimed(400.0, label, label, chip), "dead on the chip");
    assert!(
        claimed(408.0, label, label, chip),
        "half a label away, touching"
    );
    assert!(
        !claimed(430.0, label, label, chip),
        "two labels clear, so the round number is worth writing"
    );
    // The axis's own chips and the levels a drawing declared are one
    // question asked of two lists, chained rather than copied together.
    let levels = [520.0_f32, 700.0];
    assert!(claimed(521.0, label, label, chip.into_iter().chain(levels)));
    assert!(!claimed(
        600.0,
        label,
        label,
        chip.into_iter().chain(levels)
    ));
}

/// The time strip thins its own labels to `HH:MM` when it runs out of
/// room while the pointer's chip stays `HH:MM:SS`, so the two extents are
/// asked for separately. A rule that took the label's size for both let a
/// 30 px label sit a dozen pixels inside a 54 px chip — the interleaving
/// this exists to remove, back again on exactly the narrow strip that
/// needed it most.
#[test]
fn a_narrow_strips_short_label_still_clears_the_full_width_chip() {
    let (short, full) = (30.0_f32, 54.0_f32);
    let chip = [400.0_f32];
    assert!(
        claimed(358.0, short, full, chip),
        "42 px away, and the two boxes overlap by a dozen"
    );
    assert!(
        !claimed(358.0, short, short, chip),
        "which the old rule, measuring the label twice, allowed through"
    );
    assert!(claimed(400.0, short, full, chip));
    assert!(!claimed(340.0, short, full, chip), "clear of the chip");
}

/// No claims, nothing hidden: an axis with no chip on it labels every
/// gridline exactly as it always did.
#[test]
fn an_axis_nothing_claims_hides_nothing() {
    for at in [0.0_f32, 123.0, 999.0] {
        assert!(!claimed(at, 14.0, 14.0, []));
    }
}

/// The last-price chip grows down by its progress line, and the labels
/// under the grown part stand aside too: a claim on the price alone let
/// the round number below print through the second line.
#[test]
fn a_two_line_price_tag_claims_its_second_row() {
    let (label, second) = (14.0_f32, 12.0_f32);
    // The chip spans 392..420: the price's 14 px row, the 12 px line
    // under it, a pixel of padding each end.
    let one_line = || price_tag_claims(400.0, None);
    let two_lines = || price_tag_claims(400.0, Some(second));
    assert!(
        !claimed(424.0, label, label, one_line()),
        "the price's own row alone misses the second line"
    );
    assert!(
        claimed(424.0, label, label, two_lines()),
        "a label spanning 417..431 touches the chip's bottom"
    );
    assert!(
        !claimed(432.0, label, label, two_lines()),
        "one at 425..439 is clear of it"
    );
    assert!(claimed(386.0, label, label, two_lines()));
    assert!(
        !claimed(380.0, label, label, two_lines()),
        "the chip does not grow up"
    );
}

/// A progress line wider than the gutter is set smaller until it fits,
/// and one that fits keeps its size.
#[test]
fn a_second_line_too_wide_for_the_gutter_shrinks_to_fit() {
    assert_eq!(second_line_font_px(48.0), SECOND_LINE_PX, "`522/2000` fits");
    let size = second_line_font_px(72.0);
    assert!(
        size < SECOND_LINE_PX && 72.0 * size / SECOND_LINE_PX <= TAG_TEXT_ROOM_PX,
        "{size}"
    );
    assert_eq!(second_line_font_px(400.0), SECOND_LINE_MIN_PX);
    // The widest line the formatter writes fits at the floor: thirteen
    // monospace cells at 0.6 of the size.
    const { assert!(13.0 * 0.6 * SECOND_LINE_MIN_PX <= TAG_TEXT_ROOM_PX) };
}
