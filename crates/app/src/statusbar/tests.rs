use super::*;

#[test]
fn lag_cell_reports_replay_live_and_waiting() {
    let replay = Some(ReplayFigures {
        speed: 10.0,
        progress: 0.45,
    });
    assert_eq!(tape_text(replay, None, None, None), "10×  45%");
    assert_eq!(tape_text(None, Some(230), Some(300), None), "clock 230 ms");
    // The tape has gone quiet: the stored arrival figure is stale itself.
    assert_eq!(
        tape_text(None, Some(42), Some(12_000), None),
        "stale 12 s",
        "a wedged socket keeps a healthy-looking arrival forever"
    );
    // A chart opened before the open, on the session that closed the
    // afternoon before. This cell is the always-visible half of how old
    // the tape is; the corner is the other half.
    assert_eq!(
        tape_text(None, Some(42), Some(14 * 3_600_000), None),
        "stale 14 h",
        "an overnight close is not fifty thousand seconds"
    );
    assert_eq!(tape_text(None, None, None, None), "arrival —");
}

/// A split as a bridge reports one.
fn split(arrival: i64, source: i64, transport: i64, hop: &'static str) -> FeedLatency {
    FeedLatency {
        arrival_lag_ms: arrival,
        source_lag_ms: Some(source),
        source_lag_peak_ms: Some(source + 100),
        transport_lag_ms: Some(transport),
        hop: Some(hop),
        prints: 64,
    }
}

#[test]
fn a_late_tape_names_the_hop_and_a_healthy_one_stays_quiet() {
    // One word is what a trader mid-session can act on: look at the
    // terminal, or look at the chart. It appears only when there is
    // something to act on — a cell that always carries a second word stops
    // being read, and a healthy tape naming its nominally slowest hop is
    // noise dressed as a diagnosis.
    let late = split(9_000, 8_800, 200, "MT5");
    assert_eq!(
        tape_text(None, Some(9_600), Some(9_000), Some(late)),
        "MT5 9000 ms"
    );
    let healthy = split(120, 90, 30, "MT5");
    assert_eq!(
        tape_text(None, Some(140), Some(120), Some(healthy)),
        "arrival 120 ms"
    );
}

#[test]
fn the_cell_shows_the_total_its_own_halves_add_up_to() {
    // The regression this shape exists to prevent: the cell showing the
    // chart's end-to-end figure while the hover broke down the feed's, so
    // the trader read a total that its own halves did not sum to. The
    // chart's figure is larger — it also counts the queue and the frame —
    // and the hover is where it is named, as its own measurement.
    let late = split(9_000, 8_800, 200, "MT5");
    assert_eq!(tape_arrival_ms(Some(9_600), Some(late)), Some(9_000));
    assert_eq!(
        late.source_lag_ms.unwrap() + late.transport_lag_ms.unwrap(),
        late.arrival_lag_ms
    );
    // A provider with no split to publish is untouched: the chart's own
    // figure is the only one there is.
    assert_eq!(tape_arrival_ms(Some(120), None), Some(120));
}

#[test]
fn naming_the_hop_never_widens_the_cell() {
    // The three sections of this row share one width with no budget between
    // them, so a cell that grows pushes the machinery readouts off the end.
    // Measured at 1000 px: appending the hop overlapped `bars`/`trades`
    // outright, and swapping in an eight-character name still grazed them.
    // `LatencyHop::MAX_LABEL_CHARS` is therefore the width of `arrival`
    // itself; this is the other half of that bargain, asserted where the
    // cell is built, and it is `<=` with no slack on purpose.
    let plain = tape_text(None, Some(18_112), Some(100), None);
    for hop in quantick_feed_mt5::LatencyHop::ALL.map(quantick_feed_mt5::LatencyHop::label) {
        let named = tape_text(
            None,
            Some(18_112),
            Some(100),
            Some(split(18_112, 17_980, 132, hop)),
        );
        assert!(
            named.chars().count() <= plain.chars().count(),
            "{named:?} is wider than {plain:?}"
        );
    }
}

#[test]
fn a_stale_tape_still_reports_staleness_over_any_split() {
    // A wedged socket is the one state where the arrival figure lies, and
    // a hop name beside it would make the lie more convincing.
    assert_eq!(
        tape_text(
            None,
            Some(9_000),
            Some(60_000),
            Some(split(9_000, 8_800, 200, "MT5"))
        ),
        "stale 60 s"
    );
}

#[test]
fn the_hover_says_where_each_figure_was_taken() {
    // The feed measures at the socket and the chart at the frame, so the
    // two totals are different numbers about different things. Printing
    // them as one is what produced three figures that did not reconcile;
    // naming the gap turns it into the one hop neither side sees alone.
    let text = tape_tooltip(Some(9_600), Some(split(9_000, 8_800, 200, "MT5")));
    assert!(text.starts_with(TAPE_TOOLTIP_BASE), "the base always leads");
    assert!(text.contains("the feed measured 9 s from the venue's stamp"));
    assert!(text.contains("8 s before it was handed over"));
    assert!(text.contains("200 ms from there to the socket"));
    assert!(text.contains("most of it in MT5"));
    assert!(text.contains("slowest handover in the last 64 prints"));
    assert!(
        text.contains("the chart's own queue and drawing"),
        "the hop neither side can see alone is named: {text}"
    );
}

#[test]
fn a_provider_that_cannot_split_says_only_what_it_measured() {
    // Data honesty: no invented zeros, and no breakdown of a chain nobody
    // cut. The hover is exactly what it always was.
    assert!(tape_tooltip(Some(120), None).starts_with(TAPE_TOOLTIP_BASE));
    let unsplit = FeedLatency {
        arrival_lag_ms: 9_000,
        source_lag_ms: None,
        source_lag_peak_ms: None,
        transport_lag_ms: None,
        hop: None,
        prints: 3,
    };
    assert!(tape_tooltip(Some(9_000), Some(unsplit)).starts_with(TAPE_TOOLTIP_BASE));
    assert_eq!(
        tape_text(None, Some(9_000), Some(9_000), Some(unsplit)),
        "clock 9000 ms"
    );
}

#[test]
fn replay_progress_is_clamped_to_a_percentage() {
    let over = Some(ReplayFigures {
        speed: 1.0,
        progress: 1.7,
    });
    assert_eq!(tape_text(over, None, None, None), "1× 100%");
}

/// Lay the bar out for real, off-screen, in its live and replay shapes —
/// every section drawn, no duplicated widget id, no panicking widget.
#[test]
fn the_status_bar_lays_out_against_a_real_context() {
    let ctx = egui::Context::default();
    let tz = TzOffset::default();
    for replaying in [false, true] {
        let model = StatusModel {
            venue: "Binance".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            replay: replaying.then_some(ReplayFigures {
                speed: 10.0,
                progress: 0.4,
            }),
            feed_arrival_ms: (!replaying).then_some(120),
            feed_latency: None,
            tape_age_ms: (!replaying).then_some(200),
            deal_recording: None,
            deal_chip: None,
            side_note: replaying.then(|| "side: inferred (tick rule)".to_owned()),
            side_detail: None,
            sim_pnl: Some(("SIM +12.5 pts".to_owned(), std::cmp::Ordering::Greater)),
            price_auto: false,
            fps: Some(60.0),
            frame_avg_ms: Some(16.7),
            frame_cpu_ms: Some(4.2),
            show_perf: true,
            saves_off: None,
        };
        for width in [650.0, 1000.0, 1500.0] {
            for frame in 0..2 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        let response = draw(ctx, &model, tz, &StatusLead::default());
                        assert_eq!(
                            response,
                            StatusResponse::default(),
                            "an un-clicked frame asks nothing of the app"
                        );
                    },
                );
                let labels: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                            if frame > 0 {
                                assert!(
                                    shape.clip_rect.contains_rect(rect),
                                    "clipped {}: {rect:?} outside {:?}",
                                    text.galley.text(),
                                    shape.clip_rect
                                );
                            }
                            Some((text.galley.text(), rect))
                        } else {
                            None
                        }
                    })
                    .collect();
                assert!(
                    labels
                        .iter()
                        .any(|(label, _)| label.contains("SIM +12.5 pts"))
                );
                for (index, (label, rect)) in labels.iter().enumerate() {
                    assert!(
                        rect.left() >= 0.0 && rect.right() <= width,
                        "clipped {label}: {rect:?}"
                    );
                    for (other, other_rect) in &labels[index + 1..] {
                        assert!(
                            !rect.intersects(*other_rect),
                            "overlap {label} / {other}: {rect:?} / {other_rect:?}"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(
        tz,
        TzOffset::default(),
        "an un-clicked frame changes nothing"
    );
}

/// The quote-driven disclosure has to reach actual pixels, not just the
/// model: a chart of one-unit prints that says nothing reads as a real
/// tape where every trade happened to be the same size.
#[test]
fn the_quote_driven_disclosure_is_really_painted() {
    let ctx = egui::Context::default();
    let model = StatusModel {
            venue: "MetaTrader 5".to_owned(),
            symbol: "US500".to_owned(),
            replay: None,
            feed_arrival_ms: Some(106),
            feed_latency: None,
            tape_age_ms: Some(150),
            deal_recording: None,
            deal_chip: None,
            side_note: Some("prints: quote-derived".to_owned()),
            side_detail: Some(
                "this venue quotes prices but prints no trades: every candle is built \n                 from one synthetic print per tick"
                    .to_owned(),
            ),
            sim_pnl: None,
            price_auto: true,
            fps: Some(60.0),
            frame_avg_ms: Some(16.7),
            frame_cpu_ms: Some(4.2),
            show_perf: false,
            saves_off: None,
        };
    let painted = painted_text(&ctx, &model);
    assert!(
        painted.contains("prints: quote-derived"),
        "the honesty label never reached the screen; painted: {painted}"
    );
    assert!(painted.contains("US500"), "painted: {painted}");
}

/// Every text the bar paints, after egui settles its layout on the
/// second pass.
fn painted_text(ctx: &egui::Context, model: &StatusModel) -> String {
    let mut painted = String::new();
    for _ in 0..2 {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let _ = draw(ctx, model, TzOffset::default(), &StatusLead::default());
        });
        painted.clear();
        for shape in output.shapes {
            if let egui::epaint::Shape::Text(text) = shape.shape {
                painted.push_str(text.galley.text());
                painted.push(' ');
            }
        }
    }
    painted
}

/// The bar spec, the forming bar, the bar counts, the history hint and the
/// trade count left the line: the price tag carries the forming bar now.
/// What stays — workspace, feed, clock, perf, the manual-price hint — still
/// paints.
#[test]
fn the_trimmed_segments_are_gone_and_the_rest_stays() {
    let ctx = egui::Context::default();
    let model = super::native_layout_tests::model(false);
    let model = StatusModel {
        price_auto: false,
        ..model
    };
    let painted = painted_text(&ctx, &model);
    for gone in [
        "tick(50)",
        "37/50",
        " bars",
        "trades",
        "double-click for live",
    ] {
        assert!(
            !painted.contains(gone),
            "{gone:?} is still painted: {painted}"
        );
    }
    for kept in ["fps", "cpu", "price: manual", "WDOU26"] {
        assert!(painted.contains(kept), "{kept:?} is missing: {painted}");
    }
}
