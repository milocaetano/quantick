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
