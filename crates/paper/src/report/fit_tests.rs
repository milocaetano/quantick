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

const SIDE_GAP: f32 = 8.0;

/// The trader's rows at the 320 px dock: a BTCUSDT row on the WINZ26
/// chart and a Replay row, as the capture wrote them.
fn capture_rows() -> [TradeRowText<'static>; 2] {
    [
        TradeRowText {
            head: "LONG 1",
            route: "75099.03 → 75070.01",
            lead: "-29.02",
            mark: "▷",
            symbol: "BTCUSDT",
            clock: "17:31:34",
            held: "36m 35s",
            reason: "take profit",
            behind: "-29.02 pts",
        },
        TradeRowText {
            head: "SHORT 6",
            route: "173603.33 → 173662",
            lead: "-70.40",
            mark: "▷",
            symbol: "",
            clock: "17:31:34",
            held: "36m 35s",
            reason: "take profit",
            behind: "-58.67 pts ×6",
        },
    ]
}

/// The line-one measure for a fitted row: the lead is piece 2 there, and
/// it stays in line one's font when it moves to line two.
fn lines_width(fit: &TradeRowFit) -> [f32; 2] {
    let (head, detail) = (row_measure(false), row_measure(true));
    let right = |fit: &FittedPieces, wide: f32| {
        if fit.right.is_empty() {
            0.0
        } else {
            SIDE_GAP + wide
        }
    };
    let one: f32 = fit.head.left.iter().map(|piece| head(0, piece)).sum();
    let two: f32 = fit.detail.left.iter().map(|piece| detail(0, piece)).sum();
    let lead = if fit.lead_below {
        head(2, &fit.detail.right)
    } else {
        detail(5, &fit.detail.right)
    };
    [
        one + right(&fit.head, head(2, &fit.head.right)),
        two + right(&fit.detail, lead),
    ]
}

/// The capture at 320 px read `75099.03 → 75070.0…` and cut BTCUSDT
/// away. Line one now holds side, size and the whole route; the mark, the
/// instrument and the clock open line two; at 280 the lead moves down
/// rather than cut a price.
#[test]
fn a_trade_row_never_cuts_a_price_or_its_instrument_at_the_docked_widths() {
    for (dock, width) in ROW_WORDS_PX {
        for text in capture_rows() {
            let fit = fit_trade_row(
                width,
                SIDE_GAP,
                &text,
                row_measure(false),
                row_measure(true),
            );
            assert_eq!(
                fit.head.left,
                [text.head.to_owned(), format!("  {}", text.route)],
                "side, size and the whole route at {dock}"
            );
            let opening = if text.symbol.is_empty() {
                format!("{} {}", text.mark, text.clock)
            } else {
                format!("{} {} · {}", text.mark, text.symbol, text.clock)
            };
            assert!(
                fit.detail.left.concat().starts_with(&opening),
                "{opening} opens line two whole at {dock}: {fit:?}"
            );
            assert_eq!(fit.lead_below, dock == 280.0, "at {dock}: {fit:?}");
            if fit.lead_below {
                assert_eq!(
                    (fit.head.right.as_str(), fit.detail.right.as_str()),
                    ("", text.lead)
                );
            } else {
                assert_eq!(
                    (fit.head.right.as_str(), fit.detail.right.as_str()),
                    (text.lead, text.behind)
                );
            }
            for (line, wide) in lines_width(&fit).into_iter().enumerate() {
                assert!(wide <= width, "line {line} is {wide} at {dock}: {fit:?}");
            }
        }
    }
    let [btc, replay] = capture_rows();
    let at_320 = |text| {
        fit_trade_row(
            256.0,
            SIDE_GAP,
            &text,
            row_measure(false),
            row_measure(true),
        )
    };
    assert_eq!(
        at_320(btc).detail.left.concat(),
        "▷ BTCUSDT · 17:31:34 · 36m 35s"
    );
    assert_eq!(
        at_320(replay).detail.left.concat(),
        "▷ 17:31:34 · 36m 35s · take…"
    );
    // Wide enough, nothing is cut or moved.
    let roomy = fit_trade_row(400.0, SIDE_GAP, &btc, row_measure(false), row_measure(true));
    assert!(!roomy.lead_below);
    assert_eq!(
        roomy.detail.left.concat(),
        "▷ BTCUSDT · 17:31:34 · 36m 35s · take profit"
    );
}

/// Line two gives way in a fixed order as the row narrows: the reason is
/// cut, then dropped, then the duration goes whole, then the qualifier;
/// the mark, the instrument and the clock never.
#[test]
fn line_two_cuts_the_reason_then_drops_the_duration_then_the_qualifier() {
    let text = TradeRowText {
        head: "L 1",
        route: "1 → 2",
        mark: "▷",
        symbol: "BTCUSDT",
        clock: "17:31:34",
        held: "36m 35s",
        reason: "take profit",
        behind: "+1 pts",
        ..TradeRowText::default()
    };
    let line = |chars: usize| {
        let fit = fit_trade_row(
            chars as f32 * 6.0,
            6.0,
            &text,
            |_, t| mono(t),
            |_, t| mono(t),
        );
        (fit.detail.left.concat(), fit.detail.right)
    };
    // 20 protected + 10 duration + 14 reason, a 1-char gap, 6 qualifier.
    assert_eq!(line(51).0, "▷ BTCUSDT · 17:31:34 · 36m 35s · take profit");
    assert_eq!(line(45).0, "▷ BTCUSDT · 17:31:34 · 36m 35s · take…");
    assert_eq!(line(40).0, "▷ BTCUSDT · 17:31:34 · 36m 35s");
    assert_eq!(
        line(30),
        ("▷ BTCUSDT · 17:31:34".to_owned(), "+1 pts".to_owned())
    );
    assert_eq!(line(22), ("▷ BTCUSDT · 17:31:34".to_owned(), String::new()));
}

/// A Replay header at the docked widths cut its own span. The span never
/// goes: the net goes first, the count stays, then the stem is cut.
#[test]
fn a_session_header_keeps_its_span_and_count_before_its_stem() {
    let left = ["▾ ", "▷ ", "Replay", " · 09:04–12:02"];
    let head =
        |width: f32| fit_session_head(width, SIDE_GAP, &left, (26, "+1901.80 BRL"), |_, t| mono(t));
    // 24 chars of words, 17 of figures, 8 px of gap.
    assert_eq!(head(254.0).right, "26 · +1901.80 BRL");
    assert_eq!(head(254.0).left, left);
    assert_eq!(head(200.0).right, "26", "the net goes first");
    assert_eq!(head(200.0).left, left, "the stem whole while it fits");
    assert_eq!(head(160.0).left, ["▾ ", "▷ ", "Repl…", " · 09:04–12:02"]);
    assert_eq!(head(125.0).left, ["▾ ", "▷ ", "", "09:04–12:02"]);
    for (dock, width) in ROW_WORDS_PX {
        let left = ["▾ ", "▷ ", "Replay · rerun 2", " · 09:04–12:02"];
        let fit = fit_session_head(
            width,
            SIDE_GAP,
            &left,
            (26, "+1901.80 BRL"),
            row_measure(false),
        );
        assert!(
            fit.left.concat().ends_with(" · 09:04–12:02"),
            "the span is whole at {dock}: {fit:?}"
        );
        assert!(fit.right.starts_with("26"), "and the count at {dock}");
    }
}

#[test]
fn a_session_title_splits_into_its_stem_and_its_span() {
    use crate::report::sessions::title_span;
    assert_eq!(title_span("Live · 12:16–13:56"), ("Live", " · 12:16–13:56"));
    assert_eq!(
        title_span("Replay · rerun 2 · 09:04–12:02"),
        ("Replay · rerun 2", " · 09:04–12:02")
    );
    assert_eq!(title_span("Replay"), ("Replay", ""));
    assert_eq!(title_span("desk · notes"), ("desk · notes", ""));
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
