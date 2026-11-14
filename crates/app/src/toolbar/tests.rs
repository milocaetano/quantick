
use super::*;

fn test_capsule() -> quantick_ui_model::chrome_toolbar::Capsule {
    quantick_ui_model::chrome_toolbar::Capsule {
        tree: crate::canvas_layout::LAYOUT_PRESETS[0].tree.clone(),
        workspace: "Default".into(),
        unsaved: false,
        marks: Default::default(),
        menu_open: false,
        door_open: false,
        door_hover: String::new(),
    }
}

#[test]
fn trade_actions_keep_readable_labels() {
    let ctx = egui::Context::default();
    let mut appearance = quantick_appearance::Appearance::default();
    appearance.apply(quantick_appearance::LIGHT).unwrap();
    theme::sync(&ctx, &appearance);
    for ready in [false, true] {
        for state in 0..3 {
            let hovered = state == 1;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            let mut actions = Vec::new();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 100.0),
                    )),
                    events: if hovered {
                        vec![egui::Event::PointerMoved(egui::pos2(40.0, 20.0))]
                    } else if state == 2 {
                        vec![
                            egui::Event::PointerGone,
                            egui::Event::Key {
                                key: egui::Key::Tab,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]
                    } else {
                        vec![egui::Event::PointerGone]
                    },
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| draw_trade(ui, &flat_trade(ready), &mut actions));
                },
            );
            assert!(actions.is_empty());
            if state == 2 {
                let id = ctx
                    .data(|data| data.get_temp::<egui::Id>(egui::Id::new("toolbar_buy_response")))
                    .unwrap();
                assert_eq!(
                    ctx.memory(|memory| memory.has_focus(id)),
                    ready,
                    "BUY focus follows enabled state"
                );
            }

            let buy = output
                .shapes
                .iter()
                .rev()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "BUY 1" => Some(text),
                    _ => None,
                })
                .expect("BUY label");
            let ink = buy
                .override_text_color
                .unwrap_or_else(|| buy.galley.rows[0].visuals.mesh.vertices[0].color);
            let fill = output
                .shapes
                .iter()
                .rev()
                .find_map(|paint| match &paint.shape {
                    egui::Shape::Rect(rect)
                        if rect.rect.contains(buy.visual_bounding_rect().center())
                            && rect.fill.a() == 255 =>
                    {
                        Some(rect.fill)
                    }
                    _ => None,
                })
                .expect("painted button surface");
            let a = theme::relative_luminance(ink);
            let b = theme::relative_luminance(fill);
            assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
        }
    }
}

/// The row's horizontal padding, which the plan never sees.
const ROW_MARGINS: f32 = ROW_MARGIN.left + ROW_MARGIN.right;

/// What `draw` takes off for REC: the button and the gap before it.
const REC_WIDTH: f32 = crate::deal_recording_ui::REC_BUTTON_MIN_WIDTH_PX + MARKET_REC_GAP_PX;

/// Every group fits a 1280 px window and a 1920 px one with REC, at
/// TRADE's reserved width: the tray stays at the right edge.
#[test]
fn nothing_folds_at_the_common_window_widths() {
    for available in [
        1280.0 - ROW_MARGINS,
        1920.0 - ROW_MARGINS,
        1920.0 - ROW_MARGINS - REC_WIDTH,
    ] {
        assert_eq!(
            collapse_plan(available, W_TRADE),
            CollapsePlan::FULL,
            "{available} px"
        );
    }
}

/// The 1100 px first-launch window, and 1280 px with REC, no longer hold
/// the full row since the picker and the separators grew: PANELS and
/// HISTORY fold into `⋯` together and TRADE stays inline.
#[test]
fn a_narrow_window_folds_panels_and_history_but_keeps_trade() {
    for available in [1100.0 - ROW_MARGINS, 1280.0 - ROW_MARGINS - REC_WIDTH] {
        let plan = collapse_plan(available, W_TRADE);
        assert!(
            !plan.panels_inline && !plan.history_inline,
            "{available} px"
        );
        assert!(plan.trade_inline, "{available} px");
    }
}

/// Separator air is exact on both sides of every separator: the gap a
/// group draws inside it never trails into the air after it.
#[test]
fn every_separator_has_its_air_on_both_sides() {
    let shapes = toolbar_shapes(flat_trade(true), 1920.0);
    let rules: Vec<egui::Rect> = shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect)
                if rect.rect.width() == SEPARATOR_WIDTH_PX
                    && rect.rect.height() == TOOLBAR_HEIGHT - 1.0 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(rules.len(), 5, "five full-height separators: {rules:?}");
    let (buy, _) = entry_rects(&shapes);
    assert_eq!(
        rules[2].right() + SEPARATOR_AIR_PX,
        buy.expect("BUY").left()
    );
    let history = shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect)
                if rect.rect.left() > rules[1].right() && rect.rect.right() < rules[2].left() =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .map(|rect| rect.left())
        .fold(f32::INFINITY, f32::min);
    let market_right = shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect)
                if rect.rect.left() > rules[0].right() && rect.rect.right() < rules[1].left() =>
            {
                Some(rect.rect.right())
            }
            _ => None,
        })
        .fold(f32::NEG_INFINITY, f32::max);
    assert_eq!(
        rules[1].left() - market_right,
        SEPARATOR_AIR_PX,
        "air before the History rule"
    );
    assert_eq!(
        history - rules[1].right(),
        SEPARATOR_AIR_PX,
        "air after the History rule"
    );
}

/// The row draws the list door, market, history, trade, capsule and
/// panels, left to right: asserted on the painted shapes of a full-width
/// toolbar, the capsule by its workspace name.
#[test]
fn the_row_draws_door_market_history_trade_capsule_panels() {
    let shapes = toolbar_shapes(flat_trade(true), 1920.0);
    let (buy, sell) = entry_rects(&shapes);
    let (buy, sell) = (buy.expect("BUY inline"), sell.expect("SELL inline"));
    let text_x = |needle: &str| {
        shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text().contains(needle) => {
                    Some(text.pos.x)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle} was not painted"))
    };
    let order = [
        text_x(icons::LIST),
        text_x("BTCUSDT"),
        text_x("History"),
        buy.left(),
        sell.left(),
        text_x("Default"),
        text_x(icons::TRAY),
    ];
    assert!(
        order.is_sorted(),
        "door, market, history, BUY, SELL, capsule, tray: {order:?}"
    );
    assert_eq!(sell.left() - buy.right(), TRADE_GAP_PX);
    assert_eq!(buy.size(), egui::vec2(TRADE_ENTRY_WIDTH_PX, 24.0));
    assert!(
        sell.right() < 1920.0 / 2.0,
        "TRADE stands left, after History"
    );
    assert!(order[6] > 1920.0 - 40.0, "the tray sits at the right edge");
}

/// Opening a position never changes what folds: at every window width,
/// flat or open, the real toolbar paints BUY and SELL on the same
/// pixels, so the history group left of TRADE cannot fold out from
/// under a fill.
#[test]
fn opening_a_position_never_changes_the_collapse_plan() {
    let open = PaperTradeModel {
        close: Some("Close 88888 SHORT".to_owned()),
        ..flat_trade(true)
    };
    let full = CollapsePlan::FULL.width(W_TRADE);
    for width in [full + 200.0, full, full - 40.0, full - 200.0, full - 400.0] {
        let flat_rects = entry_rects(&toolbar_shapes(flat_trade(true), width));
        let open_rects = entry_rects(&toolbar_shapes(open.clone(), width));
        assert_eq!(flat_rects, open_rects, "BUY/SELL moved at width {width}");
        if width >= full {
            assert!(flat_rects.0.is_some(), "TRADE is inline at {width}");
        }
    }
}

/// Lay the toolbar out for real, off-screen, live and replaying — a
/// duplicated widget id or a panicking widget fails here instead of on
/// the chart, and an un-clicked frame must ask nothing of the app.
#[test]
fn the_toolbar_lays_out_against_a_real_context() {
    let ctx = egui::Context::default();
    let mut history_menu_rect = None;
    for replaying in [false, true] {
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                let mut capsule_rects = capsule::CapsuleRects::default();
                let mut model = ToolbarModel {
                    window_menu: WindowMenuDoor {
                        open: false,
                        hover: String::new(),
                        rect: &mut None,
                    },
                    capsule: capsule::tests::model(),
                    capsule_rects: &mut capsule_rects,
                    deal_recording_menu: false,
                    history_menu_rect: &mut history_menu_rect,
                    snapshot: &quantick_ui_model::chrome_toolbar::Snapshot {
                        capsule: test_capsule(),
                        market: ('B', "BTCUSDT \u{b7} Binance".to_owned()),
                        replay: replaying.then(|| ReplaySource {
                            label: "BTCUSDT · 2026-03-16".to_owned(),
                            hover: "Replaying a recorded session".to_owned(),
                        }),
                        deal_recording: None,
                        focused_bars: <&quantick_engine::bar_registry::BarDefinition>::from(
                            quantick_engine::BarKind::Tick,
                        )
                        .default_config(),
                        history: HistoryButton {
                            main: HistoryReach::default(),
                            status: RunStatus::Idle,
                            tz: crate::timezone::TzOffset::default(),
                        },
                        history_candles: 0,
                        older_candles: crate::tab::OlderCandles::NotArrivedYet,
                        capabilities: FeedCapabilities {
                            transport: true,
                            book_capture: !replaying,
                            history_paging: !replaying,
                            history_boundary: None,
                            traded_volume: true,
                            deal_counter: false,
                            ohlcv_history: !replaying,
                            ohlcv_generation: 0,
                            ohlcv_daily_generation: 0,
                            ohlcv_aggressor_split: false,
                            native_candle_intervals: false,
                            liquidations: false,
                            open_interest: false,
                            funding: false,
                            spot_perp_pair: false,
                        },
                        dock_visible: true,
                        paper: flat_trade(true),
                    },
                };
                let actions = draw(ctx, &mut model);
                assert!(actions.is_empty(), "no clicks, no actions");
            });
        }
    }
}

/// The same layout for a venue that prints no volume — the shape a live
/// Tickmill US500 session produces. Every size-measuring affordance is
/// disabled rather than absent, so the toolbar must still lay out whole.
#[test]
fn a_quote_driven_feed_lays_the_toolbar_out_with_its_volume_widgets_disabled() {
    let ctx = egui::Context::default();
    let mut history_menu_rect = None;
    for _ in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let mut capsule_rects = capsule::CapsuleRects::default();
            let mut model = ToolbarModel {
                window_menu: WindowMenuDoor {
                    open: false,
                    hover: String::new(),
                    rect: &mut None,
                },
                capsule: capsule::tests::model(),
                capsule_rects: &mut capsule_rects,
                deal_recording_menu: false,
                history_menu_rect: &mut history_menu_rect,
                snapshot: &quantick_ui_model::chrome_toolbar::Snapshot {
                    capsule: test_capsule(),
                    market: ('B', "US500 \u{b7} MetaTrader 5".to_owned()),
                    replay: None,
                    deal_recording: None,
                    focused_bars: <&quantick_engine::bar_registry::BarDefinition>::from(
                        quantick_engine::BarKind::Tick,
                    )
                    .default_config(),
                    history: HistoryButton {
                        main: HistoryReach::default(),
                        status: RunStatus::Idle,
                        tz: crate::timezone::TzOffset::default(),
                    },
                    history_candles: 0,
                    older_candles: crate::tab::OlderCandles::NotArrivedYet,
                    capabilities: FeedCapabilities {
                        transport: true,
                        book_capture: false,
                        history_paging: false,
                        history_boundary: None,
                        traded_volume: false,
                        deal_counter: false,
                        ohlcv_history: false,
                        ohlcv_generation: 0,
                        ohlcv_daily_generation: 0,
                        ohlcv_aggressor_split: false,
                        native_candle_intervals: false,
                        liquidations: false,
                        open_interest: false,
                        funding: false,
                        spot_perp_pair: false,
                    },
                    dock_visible: true,
                    // Not yet ready: the TRADE pair must lay out in its
                    // disabled state without asking anything of the app.
                    paper: flat_trade(false),
                },
            };
            let actions = draw(ctx, &mut model);
            assert!(actions.is_empty(), "no clicks, no actions");
        });
    }
}

/// With a position open, the exit control reaches actual toolbar pixels
/// — the audit's pain 1 was precisely this button not existing anywhere
/// the user looks.
#[test]
fn an_open_position_paints_the_close_button() {
    let paper = PaperTradeModel {
        close: Some("Close 2 LONG".to_owned()),
        ..flat_trade(true)
    };
    let mut painted = String::new();
    for shape in toolbar_shapes(paper, 1900.0) {
        if let egui::epaint::Shape::Text(text) = shape.shape {
            painted.push_str(text.galley.text());
            painted.push(' ');
        }
    }
    assert!(
        painted.contains("Close 2 LONG"),
        "the exit control never reached the toolbar; painted: {painted}"
    );
    assert!(
        painted.contains("SELL 1") && !painted.contains("(closes"),
        "the face names side and size only; painted: {painted}"
    );
}

/// A flat, ready TRADE group — the tests' baseline. Not ready reads as
/// the simulator before its first print.
fn flat_trade(ready: bool) -> PaperTradeModel {
    let texts = |face: &str, hover: &str| quantick_paper::desk::EntryTexts {
        face: face.to_owned(),
        label: face.to_owned(),
        hover: hover.to_owned(),
    };
    PaperTradeModel {
        blocked: (!ready).then(|| quantick_paper::desk::NO_MARKET_YET.to_owned()),
        buy: texts("BUY 1", "simulated market buy"),
        sell: texts("SELL 1", "simulated market sell"),
        close: None,
        reverse_hover: "reverse · Shift+R".to_owned(),
    }
}

/// The whole toolbar painted at `width`, settled, over a tick chart.
fn toolbar_shapes(paper: PaperTradeModel, width: f32) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut history_menu_rect = None;
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 400.0),
        )),
        ..Default::default()
    };
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let output = ctx.run(input(), |ctx| {
            let mut capsule_rects = capsule::CapsuleRects::default();
            let mut model = ToolbarModel {
                window_menu: WindowMenuDoor {
                    open: false,
                    hover: String::new(),
                    rect: &mut None,
                },
                capsule: capsule::tests::model(),
                capsule_rects: &mut capsule_rects,
                deal_recording_menu: false,
                history_menu_rect: &mut history_menu_rect,
                snapshot: &quantick_ui_model::chrome_toolbar::Snapshot {
                    capsule: test_capsule(),
                    market: ('B', "BTCUSDT \u{b7} Binance".to_owned()),
                    replay: None,
                    deal_recording: None,
                    focused_bars: <&quantick_engine::bar_registry::BarDefinition>::from(
                        quantick_engine::BarKind::Tick,
                    )
                    .default_config(),
                    history: HistoryButton {
                        main: HistoryReach::default(),
                        status: RunStatus::Idle,
                        tz: crate::timezone::TzOffset::default(),
                    },
                    history_candles: 0,
                    older_candles: crate::tab::OlderCandles::NotArrivedYet,
                    capabilities: FeedCapabilities {
                        transport: true,
                        book_capture: true,
                        history_paging: true,
                        history_boundary: None,
                        traded_volume: true,
                        deal_counter: false,
                        ohlcv_history: true,
                        ohlcv_generation: 0,
                        ohlcv_daily_generation: 0,
                        ohlcv_aggressor_split: false,
                        native_candle_intervals: false,
                        liquidations: false,
                        open_interest: false,
                        funding: false,
                        spot_perp_pair: false,
                    },
                    dock_visible: true,
                    paper: paper.clone(),
                },
            };
            let actions = draw(ctx, &mut model);
            assert!(actions.is_empty(), "no clicks, no actions");
        });
        shapes = output.shapes;
    }
    shapes
}

/// The BUY and SELL buttons' rectangles in painted shapes, if inline.
fn entry_rects(shapes: &[egui::epaint::ClippedShape]) -> (Option<egui::Rect>, Option<egui::Rect>) {
    let fill = |colour| {
        shapes.iter().find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == colour => Some(rect.rect),
            _ => None,
        })
    };
    (fill(theme::BUY), fill(theme::SELL))
}

/// Paint the TRADE group alone, settled: the rectangles of BUY, SELL,
/// Close and Reverse (the last two `None` while flat), with every text
/// it painted and whether the paint had to cut that text to fit.
fn paint_trade(paper: &PaperTradeModel) -> (TradeRects, Vec<(String, bool)>) {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 200.0),
        )),
        ..Default::default()
    };
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let output = ctx.run(input(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // Left to right, as in the toolbar's left run.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    let mut actions = Vec::new();
                    draw_trade(ui, paper, &mut actions);
                    assert!(actions.is_empty(), "no clicks, no actions");
                });
            });
        });
        shapes = output.shapes;
    }
    let (buy, sell) = entry_rects(&shapes);
    // The outlined pair, in paint order: Close then Reverse.
    let outlined: Vec<egui::Rect> = shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == theme::CONTROL => Some(rect.rect),
            _ => None,
        })
        .collect();
    let texts = shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => {
                Some((text.galley.text().to_owned(), text.galley.elided))
            }
            _ => None,
        })
        .collect();
    let rects = TradeRects {
        buy: buy.expect("buy painted"),
        sell: sell.expect("sell painted"),
        close: outlined.first().copied(),
        reverse: outlined.get(1).copied(),
    };
    (rects, texts)
}

/// Where the TRADE group's buttons landed.
#[derive(Debug, PartialEq)]
struct TradeRects {
    buy: egui::Rect,
    sell: egui::Rect,
    close: Option<egui::Rect>,
    reverse: Option<egui::Rect>,
}

/// Every TRADE button keeps one width and one place in every position
/// state — flat, long, short, a reverse-sized order and the longest
/// faces the desk writes — so a press lands where it landed before, and
/// no entry face is cut by its button.
#[test]
fn the_trade_buttons_keep_one_place_in_every_state() {
    use quantick_engine::Side;
    use quantick_paper::desk::EntryTexts;
    use quantick_paper::desk::ticket::entry_face;
    use rust_decimal::Decimal;
    let face = |side, qty: Decimal| {
        let face = entry_face(side, Some(qty));
        EntryTexts {
            label: face.clone(),
            hover: face.clone(),
            face,
        }
    };
    let state = |buy: Decimal, sell: Decimal, close: Option<&str>| PaperTradeModel {
        buy: face(Side::Buy, buy),
        sell: face(Side::Sell, sell),
        close: close.map(str::to_owned),
        ..flat_trade(true)
    };
    let one = Decimal::ONE;
    let states = [
        state(one, one, None),
        state(one, one, Some("Close 2 LONG")),
        state(one, one, Some("Close 2 SHORT")),
        state(
            Decimal::from(999),
            Decimal::from(999),
            Some("Close 999 SHORT"),
        ),
        state(
            Decimal::new(888, 5),
            Decimal::from(999_999),
            Some("Close 999 LONG"),
        ),
        state(
            Decimal::from(88_888_888),
            Decimal::from(12_345_678),
            Some("Close 88888888 SHORT"),
        ),
    ];
    let (flat, _) = paint_trade(&states[0]);
    assert_eq!(flat.buy.width(), TRADE_ENTRY_WIDTH_PX);
    assert_eq!(flat.sell.width(), TRADE_ENTRY_WIDTH_PX);
    assert_eq!((flat.close, flat.reverse), (None, None));
    let (long, _) = paint_trade(&states[1]);
    let close = long.close.expect("close painted");
    let reverse = long.reverse.expect("reverse painted");
    assert_eq!(close.width(), TRADE_CLOSE_WIDTH_PX);
    assert_eq!(reverse.width(), TRADE_REVERSE_WIDTH_PX);
    for paper in &states {
        let (rects, texts) = paint_trade(paper);
        assert_eq!(rects.buy, flat.buy, "BUY moved or resized for {paper:?}");
        assert_eq!(rects.sell, flat.sell, "SELL moved or resized for {paper:?}");
        if paper.close.is_some() {
            assert_eq!(rects.close, Some(close), "Close moved for {paper:?}");
            assert_eq!(rects.reverse, Some(reverse), "Reverse moved for {paper:?}");
        }
        for face in [&paper.buy.face, &paper.sell.face] {
            let (_, elided) = texts
                .iter()
                .find(|(text, _)| text == face)
                .unwrap_or_else(|| panic!("{face} never painted"));
            assert!(!elided, "{face} does not fit the fixed width");
        }
    }
    let (_, texts) = paint_trade(&states[3]);
    assert!(
        texts
            .iter()
            .any(|(text, elided)| text == "× Close 999 SHORT" && !elided),
        "a three-digit close fits its fixed width uncut"
    );
}

/// Reverse sits beside Close exactly while a position is open, and is
/// held by the lock that holds the entries.
#[test]
fn reverse_shows_only_beside_an_open_position() {
    let painted = |paper: &PaperTradeModel| {
        let (_, texts) = paint_trade(paper);
        texts.iter().any(|(text, _)| text == TRADE_REVERSE_LABEL)
    };
    assert!(!painted(&flat_trade(true)), "flat has no reverse");
    let open = PaperTradeModel {
        close: Some("Close 1 LONG".to_owned()),
        ..flat_trade(true)
    };
    assert!(painted(&open), "an open position wears the reverse");
    let (rects, _) = paint_trade(&open);
    let reverse = rects.reverse.expect("reverse painted").center();
    assert_eq!(
        click_trade(&open, reverse),
        vec![ToolbarAction::PaperReverse]
    );
    let locked = PaperTradeModel {
        blocked: Some("the risk lock is on".to_owned()),
        ..open
    };
    assert!(
        click_trade(&locked, reverse).is_empty(),
        "the lock that holds the entries holds the reverse"
    );
    assert!(click_trade(&locked, rects.buy.center()).is_empty());
    let close = rects.close.expect("close painted").center();
    assert_eq!(
        click_trade(&locked, close),
        vec![ToolbarAction::PaperClose],
        "leaving is never locked"
    );
}

/// One primary click at `at` on the TRADE group laid out as
/// [`paint_trade`] lays it, and what it asked for.
fn click_trade(paper: &PaperTradeModel, at: egui::Pos2) -> Vec<ToolbarAction> {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let mut actions = Vec::new();
    for events in [
        vec![egui::Event::PointerMoved(at)],
        vec![button(true)],
        vec![button(false)],
        Vec::new(),
    ] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 200.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let row = egui::Layout::left_to_right(egui::Align::Min);
                ui.with_layout(row, |ui| draw_trade(ui, paper, &mut actions));
            });
        });
    }
    actions
}
