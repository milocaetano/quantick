use super::*;

/// A monospace measure: every character six units wide.
fn mono(text: &str) -> f32 {
    text.chars().count() as f32 * 6.0
}

/// A detail line that outgrows its share of the row is cut with an
/// ellipsis, never clipped mid-glyph: a shortened "take prof" must not
/// be readable as a complete exit reason.
#[test]
fn a_detail_line_too_long_for_its_row_is_elided_not_clipped() {
    assert_eq!(elide_tail("take profit", 11), "take profit");
    assert_eq!(elide_tail("take profit", 12), "take profit");
    assert_eq!(elide_tail("take profit", 6), "take …");
    assert_eq!(elide_tail("take profit", 2), "t…");
    // Below the ellipsis plus a character there is nothing honest left
    // to say, so the line says nothing.
    assert_eq!(elide_tail("take profit", 1), "");
    assert_eq!(elide_tail("take profit", 0), "");
    // Multi-byte characters are counted as characters, not bytes.
    assert_eq!(elide_tail("WINV26 · 13:01", 8), "WINV26 …");
}

#[test]
fn eliding_to_a_width_keeps_the_longest_form_that_fits() {
    assert_eq!(
        elide_to_width("210350 → 210410", 90.0, mono),
        "210350 → 210410"
    );
    assert_eq!(elide_to_width("210350 → 210410", 60.0, mono), "210350 → …");
    assert_eq!(elide_to_width("210350 → 210410", 6.0, mono), "");
    assert_eq!(elide_to_width("210350 → 210410", -5.0, mono), "");
}

#[test]
fn the_first_fact_is_the_headline_figure() {
    assert_eq!(first_fact("+115.20 BRL · +205.5 pts/ct"), "+115.20 BRL");
    assert_eq!(first_fact("+205.5 pts/ct"), "+205.5 pts/ct");
}

/// The day header's order of retreat: the long date first, then the
/// points qualifier, and the count and money net never.
#[test]
fn a_day_header_shortens_its_date_before_its_net() {
    let lefts = ["Wed 21 Oct 2026", "21 Oct"];
    let rights = ["11 · +115.20 BRL · +205.5 pts/ct", "11 · +115.20 BRL"];
    let fit = |width| fit_line(width, 6.0, &lefts, &rights, mono);

    let wide = fit(400.0);
    assert_eq!(
        (wide.left.as_str(), wide.right.as_str()),
        (lefts[0], rights[0])
    );
    // 32 + 6 = 38 chars: the long date (15) no longer fits beside the net.
    let medium = fit(6.0 * 40.0 + 6.0);
    assert_eq!(
        (medium.left.as_str(), medium.right.as_str()),
        (lefts[1], rights[0])
    );
    // The full net no longer fits beside any date: the points go, and the
    // short date stays, because the long one was given up first.
    let narrow = fit(6.0 * 30.0 + 6.0);
    assert_eq!(
        (narrow.left.as_str(), narrow.right.as_str()),
        (lefts[1], rights[1])
    );
    let tight = fit(6.0 * 20.0 + 6.0);
    assert_eq!(
        (tight.left.as_str(), tight.right.as_str()),
        ("21 …", rights[1])
    );
    let starved = fit(6.0 * 17.0 + 6.0);
    assert_eq!(
        (starved.left.as_str(), starved.right.as_str()),
        ("", rights[1])
    );
}

/// Whatever the width, the two parts of a fitted line never overlap
/// unless the right part alone is wider than the line.
#[test]
fn a_fitted_line_never_overlaps_its_parts() {
    let lefts = ["76 trades · 87% win · 50 listed"];
    let rights = ["+5056.20 BRL · +9112.5 pts/ct", "+5056.20 BRL"];
    for chars in 13..80 {
        let width = chars as f32 * 6.0;
        let line = fit_line(width, 6.0, &lefts, &rights, mono);
        assert!(
            mono(&line.left) + 6.0 + mono(&line.right) <= width,
            "{chars} chars: {line:?}"
        );
        assert!(
            rights.contains(&line.right.as_str()),
            "the net is never cut"
        );
    }
}

/// The head line drops the instrument tag before it touches the route,
/// and elides the route before it touches side, quantity or P&L.
#[test]
fn a_head_line_drops_the_tag_then_elides_the_route() {
    let route = "210350 → 210410"; // 15 chars, 90 units
    let fixed = 60.0;
    let tag = 42.0;
    assert_eq!(
        fit_head(200.0, fixed, tag, route, mono),
        HeadLine {
            tag: true,
            route: route.to_owned()
        }
    );
    assert_eq!(
        fit_head(160.0, fixed, tag, route, mono),
        HeadLine {
            tag: false,
            route: route.to_owned()
        }
    );
    assert_eq!(
        fit_head(120.0, fixed, tag, route, mono),
        HeadLine {
            tag: false,
            route: "210350 → …".to_owned()
        }
    );
    assert_eq!(
        fit_head(40.0, fixed, tag, route, mono),
        HeadLine {
            tag: false,
            route: String::new()
        }
    );
}

/// Bisection lands on the same form a scan from the longest down would.
#[test]
fn eliding_by_bisection_matches_the_longest_form_that_fits() {
    let text = "absorption-at-vwap · breakout · 13:01";
    for width in 0..260 {
        let width = width as f32;
        let scanned = (0..=text.chars().count())
            .rev()
            .map(|budget| elide_tail(text, budget))
            .find(|form| mono(form) <= width)
            .unwrap_or_default();
        assert_eq!(elide_to_width(text, width, mono), scanned, "at {width}");
    }
}

#[test]
fn an_empty_right_part_costs_no_gap() {
    let line = fit_line(60.0, 6.0, &["take profit"], &[""], mono);
    assert_eq!(line.left, "take prof…");
    let line = fit_line(66.0, 6.0, &["take profit"], &[""], mono);
    assert_eq!(line.left, "take profit");
}

#[test]
fn enlarged_readouts_fit_even_when_the_result_alone_exceeds_the_row() {
    for glyph in [6.0, 12.0, 18.0] {
        for width in [0.0, 60.0, 140.0, 300.0] {
            let measure = |text: &str| text.chars().count() as f32 * glyph;
            let line = fit_line(width, 8.0, &["SHORT 400"], &["+123456.78 USD"], measure);
            assert!(measure(&line.left) + measure(&line.right) <= width);
            if !line.left.is_empty() && !line.right.is_empty() {
                assert!(measure(&line.left) + measure(&line.right) + 8.0 <= width);
            }
        }
    }
}

/// The widths a trade row's words get in the docked panel: the dock less
/// the panel's margins (16), the session box and row insets (26) and the
/// side rule with the row's padding (22).
const ROW_WORDS_PX: [(f32, f32); 2] = [(280.0, 216.0), (320.0, 256.0)];

/// Egui's monospace at the row's sizes: 11 px on line one (piece 0 and 1
/// and the figure), 10 px for the detail line.
fn row_measure(detail: bool) -> impl Fn(usize, &str) -> f32 {
    let advance = if detail { 6.0 } else { 6.6 };
    move |_, text: &str| text.chars().count() as f32 * advance
}

fn fitted_width(fitted: &FittedPieces, measure: &impl Fn(usize, &str) -> f32) -> f32 {
    let words: f32 = fitted
        .left
        .iter()
        .enumerate()
        .map(|(i, piece)| measure(i, piece))
        .sum();
    words + SIDE_GAP + measure(fitted.left.len(), &fitted.right)
}

const SIDE_GAP: f32 = 8.0;

/// The trader's capture at the default dock: rows ran to three and four
/// lines. Each line now stays one line at 280 and 320 px: the figure
/// whole and flush right, the words cut at their end - side, size and
/// prices first on line one, the clock and duration first on line two.
#[test]
fn a_trade_row_keeps_two_lines_at_the_docked_widths() {
    for (dock, width) in ROW_WORDS_PX {
        let head = ["SHORT 6", "  173603.33 → 173662"];
        let measure = row_measure(false);
        let first = fit_pieces(width, SIDE_GAP, &head, "-70.40", false, &measure);
        assert_eq!(first.right, "-70.40", "the figure is never cut at {dock}");
        assert!(!first.stacked, "never under the words at {dock}");
        assert!(
            fitted_width(&first, &measure) <= width,
            "{first:?} at {dock}"
        );
        assert_eq!(first.left[0], "SHORT 6", "side and size first at {dock}");

        let detail = ["17:31:34 · 36m 35s · take profit"];
        let measure = row_measure(true);
        let second = fit_pieces(width, SIDE_GAP, &detail, "-58.67 pts ×6", false, &measure);
        assert_eq!(second.right, "-58.67 pts ×6");
        assert!(
            fitted_width(&second, &measure) <= width,
            "{second:?} at {dock}"
        );
        assert!(
            second.left[0].starts_with("17:31:34 · 36m 35s"),
            "the clock and duration stay at {dock}: {second:?}"
        );
        assert!(second.left[0].ends_with('…'), "the reason is cut, visibly");
    }
    // Wide enough, nothing is cut.
    let roomy = fit_pieces(
        400.0,
        SIDE_GAP,
        &["SHORT 6", "  173603.33 → 173662"],
        "-70.40",
        false,
        row_measure(false),
    );
    assert_eq!(roomy.left, ["SHORT 6", "  173603.33 → 173662"]);
}

/// A cut lands on the piece it falls in; the pieces after it are dropped,
/// so a cut route never leaves a source mark dangling after it.
#[test]
fn pieces_are_cut_at_their_end_and_the_rest_dropped() {
    let pieces = ["SHORT 1", "  173290 → 173840", "  ▷"];
    let fits = |budget: f32| elide_pieces(&pieces, budget, |_, text: &str| mono(text));
    assert_eq!(fits(200.0), ["SHORT 1", "  173290 → 173840", "  ▷"]);
    assert_eq!(fits(90.0), ["SHORT 1", "  17329…", ""]);
    assert_eq!(fits(42.0), ["SHORT…", "", ""]);
    // Spaces before the cut go, so it never reads `SHORT 1  …`.
    assert_eq!(fits(60.0), ["SHORT 1", "…", ""]);
    assert_eq!(fits(6.0), ["", "", ""], "below two characters, nothing");
}

/// A session header stays one line: its title is cut, its count and figure
/// never. A day header may set its figures under the date instead.
#[test]
fn a_header_is_one_line_unless_it_may_stack() {
    let left = ["▾ ", "▷ ", "Replay · rerun 2 · 09:04–12:02"];
    let session = fit_pieces(
        200.0,
        SIDE_GAP,
        &left,
        "12 · -316.09 BRL",
        false,
        |_, text| mono(text),
    );
    assert!(!session.stacked);
    assert_eq!(session.right, "12 · -316.09 BRL");
    assert_eq!(session.left[..2], ["▾ ", "▷ "]);
    assert!(session.left[2].starts_with("Replay") && session.left[2].ends_with('…'));

    let day = fit_pieces(
        240.0,
        SIDE_GAP,
        &["▾ ", "Mon 09 Nov 2026"],
        "13 · -491.09 BRL · -1427.85 pts/ct",
        true,
        |_, text| mono(text),
    );
    assert!(day.stacked, "the figures go under the date");
    assert_eq!(day.left, ["▾ ", "Mon 09 Nov 2026"], "the date stays whole");
    assert_eq!(day.right, "13 · -491.09 BRL · -1427.85 pts/ct");
}
