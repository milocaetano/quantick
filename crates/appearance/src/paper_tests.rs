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

const KINDS: [RowKind; 5] = [
    RowKind::Day,
    RowKind::Session,
    RowKind::Trade {
        long: true,
        result: Some(Ordering::Greater),
    },
    RowKind::Trade {
        long: false,
        result: Some(Ordering::Less),
    },
    RowKind::Trade {
        long: true,
        result: None,
    },
];

/// Light and Dark draw the same cards, boxes and rows; only token values
/// (Light's own loss and replay inks, its day net, its lit session and its
/// trade stripe) differ.
#[test]
fn ledger_rows_share_one_structure_in_both_themes() {
    for kind in KINDS {
        let (dark, light) = (ledger_row(true, kind), ledger_row(false, kind));
        assert_eq!(
            (dark.fill, dark.ink, dark.detail),
            (light.fill, light.ink, light.detail),
            "{kind:?}"
        );
        assert_eq!(dark.edge.is_some(), light.edge.is_some(), "{kind:?}");
        assert_eq!(
            (dark.selected_fill, dark.radius),
            (light.selected_fill, light.radius),
            "{kind:?}"
        );
        if kind != RowKind::Session {
            assert_eq!(dark.hover_fill, light.hover_fill, "{kind:?}");
        }
        if kind != RowKind::Day {
            assert_eq!(dark.pnl[0], light.pnl[0], "{kind:?}");
        }
    }
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

/// Dark's stripe keeps its side rule, whatever the result; Light's tells a
/// win from a loss once the trade has closed and stays neutral while it is
/// open, so a position near its entry does not flip with every print.
#[test]
fn a_trade_stripe_is_dark_s_side_and_light_s_settled_result() {
    let stripe = |dark, long, result| ledger_row(dark, RowKind::Trade { long, result }).edge;
    let settled = [Ordering::Greater, Ordering::Less, Ordering::Equal].map(Some);
    for result in settled.into_iter().chain([None]) {
        assert_eq!(stripe(true, true, result), Some(Token::Buy));
        assert_eq!(stripe(true, false, result), Some(Token::Sell));
    }
    for long in [true, false] {
        for (result, edge) in [
            (Some(Ordering::Greater), Token::BuyInk),
            (Some(Ordering::Less), Token::PanelStripeLoss),
            (Some(Ordering::Equal), Token::PanelBlockEdge),
            (None, Token::PanelBlockEdge),
        ] {
            assert_eq!(stripe(false, long, result), Some(edge), "{result:?}");
        }
    }
    let dark = ResolvedTheme::default();
    assert_eq!(
        dark.color(Token::Buy),
        PremultipliedRgba::from_rgb(0x26, 0xA6, 0x9A)
    );
    assert_eq!(
        dark.color(Token::Sell),
        PremultipliedRgba::from_rgb(0xEF, 0x53, 0x50)
    );
}

/// Every word a ledger row writes keeps 4.5:1 on every surface it can sit
/// on: its own plate, the hover fill (in its lit inks) and, selected, the
/// selection - where the secondary words lift to the primary ink.
#[test]
fn ledger_words_read_on_every_surface_they_sit_on() {
    for theme in themes() {
        for kind in KINDS {
            let look = ledger_row(theme.dark, kind);
            let surfaces = [
                look.fill.map(|fill| (fill, false, false)),
                look.hover_fill.map(|fill| (fill, false, true)),
                look.selected_fill.map(|fill| (fill, true, true)),
            ];
            for (surface, selected, lit) in surfaces.into_iter().flatten() {
                let mut inks = vec![
                    look.ink,
                    look.detail_at(selected),
                    look.lit_if(lit).pnl_ink(Ordering::Greater),
                    look.lit_if(lit).pnl_ink(Ordering::Less),
                ];
                inks.extend(look.mark);
                for ink in inks {
                    assert_reads(&theme, ink, surface, 4.5);
                }
            }
        }
        // The borders still draw a line where they meet their fills.
        let frame = ledger_frame(theme.dark, Some([false, false]), Some([false, false]));
        let [card, session] = frame.layers.map(Option::unwrap);
        for (edge, fill) in [
            (card.stroke, card.fill),
            (session.stroke, session.fill),
            (card.stroke, Token::PanelGround),
        ] {
            assert_reads(&theme, edge, fill, 1.1);
        }
    }
}

/// No black plate under a trade in either theme: Light's card, box, live
/// row and selection are all pale; Dark's day card stands one step above
/// the panel's ground and the session box it paints one step above the card.
#[test]
fn ledger_surfaces_step_up_from_the_ground_and_none_is_a_black_plate() {
    let [dark, light] = themes();
    let luminance = |theme: &ResolvedTheme, token| relative_luminance(theme.color(token));
    let fills = |dark| {
        let frame = ledger_frame(dark, Some([false, false]), Some([false, false]));
        frame.layers.map(|layer| layer.unwrap().fill)
    };
    let rows = [
        Token::PanelLiveRow,
        Token::LedgerSelection,
        Token::HoverFill,
    ];
    for token in fills(false).into_iter().chain(rows) {
        assert!(luminance(&light, token) > 0.5, "{}", token.key());
    }
    for token in fills(true).into_iter().chain(rows) {
        assert!(
            luminance(&dark, token) > luminance(&dark, Token::PanelGround),
            "{}",
            token.key()
        );
    }
    let ([card, session], [light_card, _]) = (fills(true), fills(false));
    assert!(luminance(&dark, session) > luminance(&dark, card));
    assert!(luminance(&light, light_card) > luminance(&light, Token::PanelGround));
}

/// Dark paints the ledger's card and session box in the colours it painted
/// before the panels took their graphite blocks.
#[test]
fn dark_ledger_frame_keeps_its_card_and_box_colours() {
    let dark = ResolvedTheme::default();
    let frame = ledger_frame(true, Some([false, false]), Some([false, false]));
    let [card, session] = frame.layers.map(|layer| layer.unwrap());
    assert_eq!(
        [card.fill, card.stroke, session.fill, session.stroke],
        [
            Token::LedgerCard,
            Token::LedgerCardEdge,
            Token::LedgerSession,
            Token::LedgerSessionEdge
        ]
    );
    for (token, [r, g, b]) in [
        (card.fill, [0x1E, 0x23, 0x30]),
        (card.stroke, [0x2E, 0x36, 0x48]),
        (session.fill, [0x26, 0x2E, 0x40]),
        (session.stroke, [0x34, 0x40, 0x5A]),
    ] {
        assert_eq!(dark.color(token), PremultipliedRgba::from_rgb(r, g, b));
    }
}

/// Light paints its card as a panel block and rules its session box like a
/// caption, from the panels' own tokens: a saved theme that holds the old
/// ledger values still shows the blocks.
#[test]
fn light_ledger_frame_reads_the_panel_tokens() {
    let frame = ledger_frame(false, Some([false, false]), Some([false, false]));
    let [card, session] = frame.layers.map(|layer| layer.unwrap());
    assert_eq!(
        [card.fill, card.stroke, session.fill, session.stroke],
        [
            Token::PanelBlock,
            Token::PanelBlockEdge,
            Token::PanelBlock,
            Token::PanelCaptionRule
        ]
    );
}

/// Dark's day header writes its words, its count and its net in the inks
/// it wrote them in before the band: only the band under them is new.
#[test]
fn dark_day_words_keep_their_earlier_inks() {
    let dark = ResolvedTheme::default();
    let day = ledger_row(true, RowKind::Day);
    for (now, before) in [
        (day.ink, Token::TextPrimary),
        (day.detail, Token::TextMuted),
        (day.lit_if(false).pnl_ink(Ordering::Greater), Token::BuyInk),
        (day.lit_if(false).pnl_ink(Ordering::Less), Token::SellInk),
        (
            day.lit_if(false).pnl_ink(Ordering::Equal),
            Token::TextPrimary,
        ),
        (day.lit_if(true).pnl_ink(Ordering::Greater), Token::BuyInk),
        (day.lit_if(true).pnl_ink(Ordering::Less), Token::SellInk),
    ] {
        assert_eq!(dark.color(now), dark.color(before), "{now:?}");
    }
}

/// The session F2 and Del act on stands clearly off its live row in Light;
/// Dark keeps the hover fill it lit the row with before.
#[test]
fn a_lit_session_stands_off_its_live_row() {
    let [dark, light] = themes();
    let session = |dark| ledger_row(dark, RowKind::Session);
    let lit = session(false).fill_at(true, false).unwrap();
    let ratio = contrast_ratio(light.color(lit), light.color(Token::PanelLiveRow));
    assert!(ratio >= 1.2, "Light lit session: {ratio:.2}");
    let dark_lit = session(true).fill_at(true, false).unwrap();
    assert_eq!(dark.color(dark_lit), dark.color(Token::HoverFill));
    assert_ne!(dark.color(dark_lit), dark.color(Token::PanelLiveRow));
}

/// A session's gains keep the buy ink: Dark's always, Light's on its live
/// row, and only Light's lit row, on the footer grey where the buy ink
/// reads 4.0:1, takes the panels' deeper gain ink.
#[test]
fn only_light_s_lit_session_deepens_its_gains() {
    for lit in [false, true] {
        let dark = ledger_row(true, RowKind::Session);
        assert_eq!(dark.lit_if(lit).pnl_ink(Ordering::Greater), Token::BuyInk);
        assert_eq!(dark.lit_if(lit).pnl_ink(Ordering::Less), Token::SellInk);
    }
    let light = ledger_row(false, RowKind::Session);
    assert_eq!(
        light.lit_if(false).pnl_ink(Ordering::Greater),
        Token::BuyInk
    );
    assert_eq!(
        light.lit_if(true).pnl_ink(Ordering::Greater),
        Token::PanelGainInk
    );
    assert_eq!(light.lit_if(true).pnl_ink(Ordering::Less), Token::LossInk);
    assert_eq!(light.fill_at(true, false), Some(Token::PanelFooter));
}

/// A net written on the footer plate keeps 4.5:1 there in both themes:
/// Light's gains in the panels' gain ink, every other figure (and every
/// surface but the footer) in [`pnl_ink`]'s.
#[test]
fn signed_amounts_read_on_the_footer_plate() {
    for theme in themes() {
        for sign in [Ordering::Greater, Ordering::Less, Ordering::Equal] {
            let ink = plate_pnl_ink(&theme, Token::PanelFooter, sign);
            assert_reads(&theme, ink, Token::PanelFooter, 4.5);
            let inset = plate_pnl_ink(&theme, Token::Inset, sign);
            assert_eq!(inset, pnl_ink(&theme, Token::Inset, sign));
            if theme.dark || sign != Ordering::Greater {
                assert_eq!(ink, pnl_ink(&theme, Token::PanelFooter, sign));
            } else {
                assert_eq!(ink, Token::PanelGainInk);
            }
        }
    }
}

/// Gold, ochre and orange stay out of Light's ledger: every token a row or
/// a frame paints there sits outside the orange-to-gold hues.
#[test]
fn light_ledger_wears_no_gold_or_orange() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let mut tokens = vec![Token::LedgerSelection, Token::HoverFill];
    for kind in KINDS {
        let look = ledger_row(false, kind);
        tokens.extend([look.ink, look.detail]);
        tokens.extend(look.pnl.into_iter().chain(look.lit_pnl));
        tokens.extend(look.fill.into_iter().chain(look.edge).chain(look.mark));
    }
    let frame = ledger_frame(false, Some([false, false]), Some([false, false]));
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
    let bare = ledger_frame(true, None, None);
    assert_eq!(bare.layers, [None, None]);
    assert_eq!(bare.content, Margin::default());

    let open: Ends = [true, false];
    let middle: Ends = [false, false];
    let close: Ends = [false, true];
    let day = ledger_frame(true, Some(open), None);
    let card = day.layers[0].unwrap();
    assert_eq!(
        (card.fill, card.stroke),
        (Token::LedgerCard, Token::LedgerCardEdge)
    );
    assert_eq!(card.radius, CARD_RADIUS_PX);
    assert_eq!(card.inset.top, CARD_GAP_PX, "a gap above every card");
    assert_eq!(day.layers[1], None);
    assert_eq!((day.content.left, day.content.top), (0.0, CARD_GAP_PX));

    let head = ledger_frame(true, Some(middle), Some(open));
    let session = head.layers[1].unwrap();
    assert_eq!(
        (session.fill, session.stroke),
        (Token::LedgerSession, Token::LedgerSessionEdge)
    );
    assert_eq!(session.radius, SESSION_RADIUS_PX);
    assert_eq!(session.inset.left, SESSION_INSET_PX);
    assert_eq!(
        head.content.left, SESSION_INSET_PX,
        "the header spans its box"
    );

    let trade = ledger_frame(true, Some(middle), Some(middle));
    assert_eq!(trade.content.left, SESSION_INSET_PX + TRADE_INSET_PX);
    assert_eq!(trade.content.right, trade.content.left);
    assert_eq!((trade.content.top, trade.content.bottom), (0.0, 0.0));

    // Only a closed end is rounded; an open one carries its outline past
    // the row, so the clip leaves just the sides.
    assert_eq!(card.corners(), [CARD_RADIUS_PX, 0.0]);
    assert_eq!(card.reach(), [0.0, CARD_RADIUS_PX + 1.0]);

    let last = ledger_frame(true, Some(close), Some(close));
    assert_eq!(last.layers[0].unwrap().ends, close);
    assert_eq!(last.layers[1].unwrap().inset.bottom, SESSION_INSET_PX);
    assert_eq!(last.content.bottom, SESSION_INSET_PX + SESSION_PAD_PX);

    let loose = ledger_frame(true, Some(close), None);
    assert_eq!(loose.layers[1], None, "a trade no session holds has no box");
    assert_eq!(
        (loose.content.left, loose.content.bottom),
        (SESSION_INSET_PX, SESSION_INSET_PX)
    );
}

/// The selection outranks the hover, which outranks the row's own plate.
#[test]
fn a_selected_trade_wears_the_selection_and_lifts_its_detail() {
    let trade = ledger_row(
        false,
        RowKind::Trade {
            long: true,
            result: Some(Ordering::Greater),
        },
    );
    assert_eq!(trade.fill_at(false, false), Some(Token::PanelBlock));
    assert_eq!(trade.fill_at(true, false), Some(Token::HoverFill));
    assert_eq!(trade.fill_at(true, true), Some(Token::LedgerSelection));
    assert_eq!(trade.detail_at(false), Token::TextMuted);
    assert_eq!(trade.detail_at(true), Token::TextPrimary);
    let day = ledger_row(false, RowKind::Day);
    assert_eq!(
        day.fill_at(false, true),
        Some(Token::PanelBand),
        "a header is never selected"
    );
    assert_eq!(day.detail_at(true), Token::PanelBandState);
}

/// A day's header is its block's graphite band, a session's its live row:
/// the same surfaces the order-flow settings panel paints.
#[test]
fn a_day_wears_the_band_and_a_session_the_live_row() {
    let day = ledger_row(false, RowKind::Day);
    assert_eq!(
        (day.fill, day.ink),
        (Some(Token::PanelBand), Token::PanelPlateInk)
    );
    assert_eq!(day.pnl, [Token::PanelBandState; 2]);
    assert_eq!(
        ledger_row(false, RowKind::Session).fill,
        Some(Token::PanelLiveRow)
    );
}
