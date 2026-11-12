use super::*;
use crate::{LIGHT, PremultipliedRgba, contrast::contrast_ratio};

fn themes() -> [ResolvedTheme; 2] {
    [
        ResolvedTheme::default(),
        ResolvedTheme::parse(LIGHT).unwrap(),
    ]
}

fn assert_reads(theme: &ResolvedTheme, ink: Token, fill: Token, least: f32) {
    let ratio = contrast_ratio(theme.color(ink), theme.color(fill));
    assert!(
        ratio >= least,
        "{}: {} on {}: {ratio}",
        theme.name,
        ink.key(),
        fill.key()
    );
}

/// Every plate's words, gains and losses keep 4.5:1 on it, in both themes.
#[test]
fn plate_inks_read_on_their_plates_in_both_themes() {
    for theme in themes() {
        for (ink, plate) in [
            (Token::ReplayPlateInk, Token::ReplayPlate),
            (Token::OrderPlateInk, Token::OrderPlate),
            (Token::PlateGain, Token::OrderPlate),
            (Token::PlateLoss, Token::OrderPlate),
        ] {
            assert_reads(&theme, ink, plate, 4.5);
        }
        let chip = drawdown_chip(theme.dark);
        assert_reads(&theme, chip.ink, chip.fill, 4.5);
    }
}

#[test]
fn light_writes_every_chart_level_on_the_order_plate_and_only_the_ink_changes() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    assert!(plates_on_chart(&light));
    for (line, ink) in [
        (Token::Accent, Token::OrderPlateInk),
        (Token::Sell, Token::PlateLoss),
        (Token::Buy, Token::PlateGain),
    ] {
        let plate = plate_for(&light, light.color(line)).unwrap();
        assert_eq!(plate.fill, Token::OrderPlate, "{}", line.key());
        assert_eq!(plate.ink, ink, "{}", line.key());
        assert_reads(&light, plate.ink, plate.fill, 4.5);
    }
    assert_eq!(
        light.color(Token::OrderPlate),
        PremultipliedRgba::from_rgb(0x15, 0x19, 0x1E)
    );
    // The plate is the line too: it has to stand off the white chart.
    assert!(
        contrast_ratio(
            light.color(Token::OrderPlate),
            light.color(Token::ChartCanvas)
        ) >= 3.0
    );
}

/// Dark, and Light's own slate flow pane, keep their accent and side colours.
#[test]
fn dark_surfaces_keep_their_accent_and_side_colours() {
    let dark = ResolvedTheme::default();
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    for surface in [&dark, light.bookmap()] {
        assert!(!plates_on_chart(surface), "{}", surface.name);
        for line in [Token::Accent, Token::Buy, Token::Sell] {
            assert_eq!(plate_for(surface, surface.color(line)), None);
        }
    }
    assert_eq!(equity_area(true, true), Token::Buy);
    assert_eq!(equity_area(true, false), Token::Sell);
    assert_eq!(
        drawdown_chip(true),
        Plate {
            fill: Token::Sell,
            ink: Token::ChipInk
        }
    );
}

/// Dark writes P&L in its candle colours; Light in its buy ink and a dark
/// red at 4.5:1 on every pale panel, bright on its slate flow pane.
#[test]
fn signed_amounts_take_each_surface_s_readable_ink() {
    let dark = ResolvedTheme::default();
    for surface in [Token::Inset, Token::ChartCanvas] {
        assert_eq!(pnl_ink(&dark, surface, Ordering::Greater), Token::Buy);
        assert_eq!(pnl_ink(&dark, surface, Ordering::Less), Token::Sell);
        assert_eq!(pnl_ink(&dark, surface, Ordering::Equal), Token::TextMuted);
    }
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    assert_eq!(
        pnl_ink(&light, Token::Inset, Ordering::Greater),
        Token::BuyInk
    );
    // The inks the Rust constants used to hold, now tokens a derived theme
    // can retheme: the same pixels.
    for (token, rgb) in [
        (Token::LossInk, (136, 24, 30)),
        (Token::BrightGainInk, (100, 220, 160)),
        (Token::BrightLossInk, (255, 145, 145)),
    ] {
        let expected = PremultipliedRgba::from_rgb(rgb.0, rgb.1, rgb.2);
        assert_eq!(light.color(token), expected, "{}", token.key());
    }
    for surface in [&light, light.bookmap()] {
        for sign in [Ordering::Greater, Ordering::Less, Ordering::Equal] {
            let ink = surface.color(pnl_ink(surface, Token::Inset, sign));
            for under in [Token::Chrome, Token::Inset, Token::Control, Token::TagBg] {
                let ratio = contrast_ratio(ink, surface.color(under));
                assert!(ratio >= 4.5, "{sign:?} on {}: {ratio}", under.key());
            }
        }
    }
}

/// The ink follows the surface the figure is written on, not the panel's:
/// a light theme with a dark inset over its white chart writes the panel's
/// figures bright and the chart's dark.
#[test]
fn signed_amounts_follow_the_surface_they_are_written_on() {
    let source = LIGHT.replacen("inset = \"#F7F7F7\"", "inset = \"#10141D\"", 1);
    let theme = ResolvedTheme::parse(&source).unwrap();
    for (sign, panel, chart) in [
        (Ordering::Greater, Token::BrightGainInk, Token::BuyInk),
        (Ordering::Less, Token::BrightLossInk, Token::LossInk),
    ] {
        assert_eq!(pnl_ink(&theme, Token::Inset, sign), panel);
        assert_eq!(pnl_ink(&theme, Token::ChartCanvas, sign), chart);
        for surface in [Token::Inset, Token::ChartCanvas] {
            assert_reads(&theme, pnl_ink(&theme, surface, sign), surface, 4.5);
        }
    }
}

#[test]
fn light_report_paints_gains_in_buy_ink_losses_in_warn_and_plates_the_drawdown() {
    assert_eq!(equity_area(false, true), Token::BuyInk);
    assert_eq!(equity_area(false, false), Token::Warn);
    assert_eq!(
        drawdown_chip(false),
        Plate {
            fill: Token::OrderPlate,
            ink: Token::OrderPlateInk
        }
    );
}

const KINDS: [RowKind; 4] = [
    RowKind::Day,
    RowKind::Session,
    RowKind::Trade { long: true },
    RowKind::Trade { long: false },
];

/// Light and Dark draw the same cards, boxes and rows; only token values
/// (and Light's own loss and replay inks) differ.
#[test]
fn ledger_rows_share_one_structure_in_both_themes() {
    for kind in KINDS {
        let (dark, light) = (ledger_row(true, kind), ledger_row(false, kind));
        assert_eq!(
            (dark.fill, dark.edge, dark.ink, dark.detail),
            (light.fill, light.edge, light.ink, light.detail),
            "{kind:?}"
        );
        assert_eq!(
            (dark.hover_fill, dark.selected_fill, dark.radius),
            (light.hover_fill, light.selected_fill, light.radius),
            "{kind:?}"
        );
        assert_eq!(dark.pnl[0], light.pnl[0]);
    }
    assert_ne!(
        ledger_row(true, RowKind::Trade { long: true }).edge,
        ledger_row(true, RowKind::Trade { long: false }).edge,
        "the side rule tells the sides apart"
    );
    assert_eq!(
        ledger_row(true, RowKind::Session).mark,
        None,
        "Dark keeps the amber play glyph"
    );
    assert_eq!(
        ledger_row(false, RowKind::Session).mark,
        Some(Token::ReplayInk)
    );
}

/// Every word a ledger row writes keeps 4.5:1 on every surface it can sit
/// on: its card, its session box, the hover fill and, selected, the
/// selection - where the secondary words lift to the primary ink.
#[test]
fn ledger_words_read_on_every_surface_they_sit_on() {
    for theme in themes() {
        for kind in KINDS {
            let look = ledger_row(theme.dark, kind);
            for (surface, selected) in [
                (Token::LedgerCard, false),
                (Token::LedgerSession, false),
                (Token::HoverFill, false),
                (Token::LedgerSelection, true),
            ] {
                if selected && look.selected_fill.is_none() {
                    continue;
                }
                let mut inks = vec![
                    look.ink,
                    look.detail_at(selected),
                    look.pnl_ink(Ordering::Greater),
                    look.pnl_ink(Ordering::Less),
                ];
                inks.extend(look.mark);
                for ink in inks {
                    assert_reads(&theme, ink, surface, 4.5);
                }
            }
        }
        // The borders still draw a line where they meet their fills.
        for (edge, fill) in [
            (Token::LedgerCardEdge, Token::LedgerCard),
            (Token::LedgerSessionEdge, Token::LedgerSession),
            (Token::LedgerCardEdge, Token::Chrome),
        ] {
            assert_reads(&theme, edge, fill, 1.1);
        }
    }
}

/// No black plate in either theme: Light's card, box and selection are all
/// pale; Dark's card stands one step above the panel and its session box one
/// step above the card.
#[test]
fn ledger_surfaces_step_up_from_the_panel_and_none_is_a_black_plate() {
    let [dark, light] = themes();
    let luminance = |theme: &ResolvedTheme, token| relative_luminance(theme.color(token));
    for token in [
        Token::LedgerCard,
        Token::LedgerSession,
        Token::LedgerSelection,
        Token::HoverFill,
    ] {
        assert!(luminance(&light, token) > 0.5, "{}", token.key());
        assert!(
            luminance(&dark, token) > luminance(&dark, Token::Chrome),
            "{}",
            token.key()
        );
    }
    assert!(luminance(&dark, Token::LedgerSession) > luminance(&dark, Token::LedgerCard));
    assert!(luminance(&light, Token::LedgerCard) > luminance(&light, Token::Chrome));
}

/// Gold, ochre and orange stay out of Light's ledger: every token a row or
/// a frame paints there sits outside the orange-to-gold hues.
#[test]
fn light_ledger_wears_no_gold_or_orange() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let mut tokens = vec![Token::LedgerSelection, Token::HoverFill];
    for kind in KINDS {
        let look = ledger_row(false, kind);
        tokens.extend([look.ink, look.detail, look.pnl[0], look.pnl[1]]);
        tokens.extend(look.fill.into_iter().chain(look.edge).chain(look.mark));
    }
    let frame = ledger_frame(Some([false, false]), Some([false, false]));
    for layer in frame.layers.into_iter().flatten() {
        tokens.extend([layer.fill, layer.stroke]);
    }
    for token in tokens {
        let [r, g, b, _] = light.color(token).to_array().map(f32::from);
        let (high, low) = (r.max(g).max(b), r.min(g).min(b));
        let warm = high == r && r - low > 0.25 * high && g > b;
        let hue = if warm {
            60.0 * (g - b) / (r - low)
        } else {
            0.0
        };
        assert!(
            !(warm && (15.0..=65.0).contains(&hue)),
            "{} reads gold or orange: hue {hue}",
            token.key()
        );
    }
}

/// A trade sits inside its session's box inside its day's card: the box
/// stands in from the card, the trade from the box, and only a container's
/// first and last rows add its top and bottom space.
#[test]
fn the_ledger_nests_a_trade_in_its_session_box_in_its_day_card() {
    let bare = ledger_frame(None, None);
    assert_eq!(bare.layers, [None, None]);
    assert_eq!(bare.content, Margin::default());

    let open: Ends = [true, false];
    let middle: Ends = [false, false];
    let close: Ends = [false, true];
    let day = ledger_frame(Some(open), None);
    let card = day.layers[0].unwrap();
    assert_eq!(card.fill, Token::LedgerCard);
    assert_eq!(card.radius, CARD_RADIUS_PX);
    assert_eq!(card.inset.top, CARD_GAP_PX, "a gap above every card");
    assert_eq!(day.layers[1], None);
    assert_eq!((day.content.left, day.content.top), (0.0, CARD_GAP_PX));

    let head = ledger_frame(Some(middle), Some(open));
    let session = head.layers[1].unwrap();
    assert_eq!(session.fill, Token::LedgerSession);
    assert_eq!(session.radius, SESSION_RADIUS_PX);
    assert_eq!(session.inset.left, SESSION_INSET_PX);
    assert_eq!(
        head.content.left, SESSION_INSET_PX,
        "the header spans its box"
    );

    let trade = ledger_frame(Some(middle), Some(middle));
    assert_eq!(trade.content.left, SESSION_INSET_PX + TRADE_INSET_PX);
    assert_eq!(trade.content.right, trade.content.left);
    assert_eq!((trade.content.top, trade.content.bottom), (0.0, 0.0));

    // Only a closed end is rounded; an open one carries its outline past
    // the row, so the clip leaves just the sides.
    assert_eq!(card.corners(), [CARD_RADIUS_PX, 0.0]);
    assert_eq!(card.reach(), [0.0, CARD_RADIUS_PX + 1.0]);

    let last = ledger_frame(Some(close), Some(close));
    assert_eq!(last.layers[0].unwrap().ends, close);
    assert_eq!(last.layers[1].unwrap().inset.bottom, SESSION_INSET_PX);
    assert_eq!(last.content.bottom, SESSION_INSET_PX + SESSION_PAD_PX);

    let loose = ledger_frame(Some(close), None);
    assert_eq!(loose.layers[1], None, "a trade no session holds has no box");
    assert_eq!(
        (loose.content.left, loose.content.bottom),
        (SESSION_INSET_PX, SESSION_INSET_PX)
    );
}

/// The selection outranks the hover, which outranks the row's own plate.
#[test]
fn a_selected_trade_wears_the_selection_and_lifts_its_detail() {
    let trade = ledger_row(false, RowKind::Trade { long: true });
    assert_eq!(trade.fill_at(false, false), Some(Token::LedgerCard));
    assert_eq!(trade.fill_at(true, false), Some(Token::HoverFill));
    assert_eq!(trade.fill_at(true, true), Some(Token::LedgerSelection));
    assert_eq!(trade.detail_at(false), Token::TextMuted);
    assert_eq!(trade.detail_at(true), Token::TextPrimary);
    let day = ledger_row(false, RowKind::Day);
    assert_eq!(day.fill_at(false, true), None, "a header is never selected");
    assert_eq!(day.detail_at(true), Token::TextMuted);
}

const STATES: [LevelState; 4] = [
    LevelState::Resting,
    LevelState::Pending,
    LevelState::Moving,
    LevelState::Aimed,
];

/// Light writes a level under the hand, and the aim, on the paper plate:
/// the pending leg's white plate and side edge, dark words, an edge dashed
/// only while the order is unsent. A resting level keeps the dark plate.
#[test]
fn light_lays_the_paper_plate_under_a_moving_or_aimed_level() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    for (line, edge) in [
        (Token::Buy, Token::PlateGain),
        (Token::Sell, Token::PlateLoss),
    ] {
        let line = light.color(line);
        assert_eq!(paper_plate(&light, line, LevelState::Resting), None);
        for state in [LevelState::Pending, LevelState::Moving, LevelState::Aimed] {
            let plate = paper_plate(&light, line, state).unwrap();
            assert_eq!(
                (plate.fill, plate.edge, plate.ink),
                (Token::Inset, edge, Token::TextPrimary),
                "{state:?}"
            );
            assert_eq!(plate.dashed, state == LevelState::Aimed, "{state:?}");
            assert!(outlined_tag(&light, state));
            assert_reads(&light, plate.ink, plate.fill, 4.5);
            assert!(
                relative_luminance(light.color(plate.fill)) > 0.9,
                "a white plate"
            );
        }
    }
    assert!(!outlined_tag(&light, LevelState::Resting));
    // An entry's edge is the order plate, never its white ink on white.
    let accent = paper_plate(&light, light.color(Token::Accent), LevelState::Aimed).unwrap();
    assert_eq!(accent.edge, Token::OrderPlate);
}

/// The chip beside a paper plate is filled in its side's tag colour with
/// white digits at 4.5:1; a resting or pending level keeps the dark plate.
#[test]
fn light_fills_a_moving_level_s_chip_in_its_side_colour() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    for (line, fill) in [
        (Token::Buy, Token::PaperTagGain),
        (Token::Sell, Token::PaperTagLoss),
    ] {
        let line = light.color(line);
        for state in [LevelState::Moving, LevelState::Aimed] {
            let chip = level_chip(&light, line, state).unwrap();
            assert_eq!(
                chip,
                Plate {
                    fill,
                    ink: Token::ChipInk
                }
            );
            assert_reads(&light, chip.ink, chip.fill, 4.5);
            assert_eq!(
                light.color(chip.ink),
                PremultipliedRgba::from_rgb(255, 255, 255)
            );
        }
        for state in [LevelState::Resting, LevelState::Pending] {
            assert_eq!(level_chip(&light, line, state), plate_for(&light, line));
        }
    }
    assert_ne!(
        light.color(Token::PaperTagGain),
        light.color(Token::PaperTagLoss)
    );
    let accent = light.color(Token::Accent);
    assert_eq!(
        level_chip(&light, accent, LevelState::Aimed),
        plate_for(&light, accent)
    );
}

/// Dark, and Light's slate flow pane, keep every level as it was: no paper
/// plate, no filled tag colour, no readout edge; only a pending leg is
/// outlined, as before.
#[test]
fn dark_surfaces_lay_no_paper_plate() {
    let dark = ResolvedTheme::default();
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    for surface in [&dark, light.bookmap()] {
        for state in STATES {
            for line in [Token::Accent, Token::Buy, Token::Sell] {
                let line = surface.color(line);
                assert_eq!(paper_plate(surface, line, state), None);
                assert_eq!(level_chip(surface, line, state), None);
            }
            assert_eq!(
                outlined_tag(surface, state),
                state == LevelState::Pending,
                "{state:?}"
            );
        }
        assert_eq!(paper_readout(surface), None);
    }
}

/// Light's ruler readout: a grey edge on its plate, the price line in the
/// readable gain or loss ink of the chart, the rest in the primary ink.
#[test]
fn light_readout_is_edged_in_grey_and_inks_only_the_move() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let readout = paper_readout(&light).unwrap();
    assert_eq!(
        readout,
        PaperReadout {
            edge: Token::ReadoutEdge,
            ink: Token::TextPrimary,
            gain: Token::BuyInk,
            loss: Token::LossInk,
        }
    );
    for ink in [readout.ink, readout.gain, readout.loss] {
        assert_reads(&light, ink, Token::ChartCanvas, 4.5);
    }
    let edge = light.color(readout.edge);
    let [r, g, b, a] = edge.to_array();
    assert_eq!(a, 255);
    assert!(
        r.abs_diff(b) < 24 && g.abs_diff(b) < 24,
        "a neutral grey edge"
    );
    assert!(contrast_ratio(edge, light.color(Token::ChartCanvas)) > 1.5);
}

/// A saved Light copy written before these tokens (the trader's
/// `appearance.toml.choice.toml`) still resolves them to bundled Light's.
#[test]
fn a_light_copy_saved_before_the_paper_tokens_takes_bundled_light_s() {
    let bundled = ResolvedTheme::parse(LIGHT).unwrap();
    let older: String = LIGHT
        .lines()
        .filter(|line| {
            !["paper_tag_gain", "paper_tag_loss", "readout_edge"]
                .iter()
                .any(|key| line.starts_with(key))
        })
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(older, LIGHT.replace("\r\n", "\n"));
    let saved = ResolvedTheme::parse(&older).unwrap();
    for token in [Token::PaperTagGain, Token::PaperTagLoss, Token::ReadoutEdge] {
        assert_eq!(saved.color(token), bundled.color(token), "{}", token.key());
    }
}
