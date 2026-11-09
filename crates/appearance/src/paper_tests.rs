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
        let session = ledger_row(theme.dark, RowKind::Session);
        let fill = session.fill.unwrap();
        for sign in [Ordering::Greater, Ordering::Less, Ordering::Equal] {
            assert_reads(&theme, session.pnl_ink(sign).unwrap(), fill, 4.5);
        }
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
    assert_eq!(pnl_ink(&dark, Ordering::Greater), dark.color(Token::Buy));
    assert_eq!(pnl_ink(&dark, Ordering::Less), dark.color(Token::Sell));
    assert_eq!(
        pnl_ink(&dark, Ordering::Equal),
        dark.color(Token::TextMuted)
    );
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    assert_eq!(
        pnl_ink(&light, Ordering::Greater),
        light.color(Token::BuyInk)
    );
    for surface in [&light, light.bookmap()] {
        for sign in [Ordering::Greater, Ordering::Less, Ordering::Equal] {
            let ink = pnl_ink(surface, sign);
            for under in [Token::Chrome, Token::Inset, Token::Control, Token::TagBg] {
                let ratio = contrast_ratio(ink, surface.color(under));
                assert!(ratio >= 4.5, "{sign:?} on {}: {ratio}", under.key());
            }
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

/// The ledger's scenario: a day, a session in it and a trade under that.
/// Each kind paints its own drawing - fill and left edge - in both themes,
/// so the three nest by look and not by indent alone.
#[test]
fn ledger_rows_paint_three_distinct_fills_and_edges() {
    for theme in themes() {
        let paint = |row| {
            let look = ledger_row(theme.dark, row);
            (
                look.fill.map(|token| theme.color(token)),
                look.edge.map(|token| theme.color(token)),
            )
        };
        let day = paint(RowKind::Day);
        let session = paint(RowKind::Session);
        let trade = paint(RowKind::Trade { long: true });
        assert_ne!(day, session, "{}", theme.name);
        assert_ne!(session, trade, "{}", theme.name);
        assert_ne!(day, trade, "{}", theme.name);
        let short = paint(RowKind::Trade { long: false });
        assert_ne!(trade.1, short.1, "the side rail tells the sides apart");
        for row in [
            RowKind::Day,
            RowKind::Session,
            RowKind::Trade { long: true },
        ] {
            let look = ledger_row(theme.dark, row);
            assert_reads(&theme, look.ink, look.fill.unwrap_or(Token::Chrome), 4.5);
            if let Some(hover) = look.hover_fill {
                assert_reads(&theme, look.ink, hover, 4.5);
            }
        }
    }
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let day = ledger_row(false, RowKind::Day);
    assert_eq!(light.color(day.fill.unwrap()), crate::color::WHITE);
    assert_eq!(day.edge, Some(Token::TextPrimary));
    let session = ledger_row(false, RowKind::Session);
    assert_eq!(
        light.color(session.fill.unwrap()),
        PremultipliedRgba::from_rgb(0x20, 0x20, 0x20)
    );
    let mark = session.mark.unwrap();
    assert_reads(&light, mark, session.fill.unwrap(), 4.5);
    let dark_session = ledger_row(true, RowKind::Session);
    assert_eq!(dark_session.mark, None, "Dark keeps the amber play glyph");
    assert_eq!(dark_session.fill, Some(Token::Control));
    assert_eq!(dark_session.pnl, Some([Token::BuyInk, Token::SellInk]));
}
