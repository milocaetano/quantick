use super::bar::*;
const PROFILE_COLOR: egui::Color32 = egui::Color32::from_gray(0xD8);
use super::*;

/// The text floors are a typography budget and must be re-derived, never
/// nudged: a floor under what its own text measures draws digits across
/// the neighbouring candle, which is worse than making the trader zoom.
/// This is that derivation, run against whatever the constants say today.
/// Compose a translucent colour over an opaque one, the way the painter
/// does — so a test measures the pixel the trader sees, not the token.
fn over(fg: egui::Color32, bg: egui::Color32) -> egui::Color32 {
    let alpha = f32::from(fg.a()) / 255.0;
    // `Color32` is premultiplied, so the source term is already scaled.
    let channel = |f: u8, b: u8| -> u8 {
        (f32::from(f) + f32::from(b) * (1.0 - alpha))
            .round()
            .min(255.0) as u8
    };
    egui::Color32::from_rgb(
        channel(fg.r(), bg.r()),
        channel(fg.g(), bg.g()),
        channel(fg.b(), bg.b()),
    )
}

/// The floor WCAG calls readable for body text. Every number this layer
/// draws is measured against it — no exemptions, because a number a trader
/// cannot read is a number that is not there.
const AA: f32 = 4.5;

/// The plate is what makes the ladder's contrast a constant. Proven
/// against every background the layer can actually sit on: the canvas, a
/// candle body at each of the four appearance presets, and the depth map's
/// brightest bands.
///
/// Two of those presets used to fail — `Glass` at 4.07:1 and `Classic` at
/// 2.18:1 — because the ladder had no plate and inherited whatever the
/// trader's taste in candles left behind. That is the defect this test
/// exists to keep out.
#[test]
fn the_ladder_plate_makes_every_background_readable() {
    let candle_at = |fill: f32, side: egui::Color32| -> egui::Color32 {
        over(side.gamma_multiply(fill), theme::CANVAS)
    };
    let backgrounds = [
        ("canvas", theme::CANVAS),
        ("orderflow buy", candle_at(0.20, theme::BUY)),
        ("orderflow sell", candle_at(0.20, theme::SELL)),
        ("glass buy", candle_at(0.35, theme::BUY)),
        ("glass sell", candle_at(0.35, theme::SELL)),
        ("classic buy", candle_at(1.0, theme::BUY)),
        ("classic sell", candle_at(1.0, theme::SELL)),
        ("heat cyan", egui::Color32::from_rgb(0x00, 0xC2, 0xC4)),
        ("heat amber", egui::Color32::from_rgb(0xFA, 0x9E, 0x2C)),
        ("heat peak", egui::Color32::from_rgb(0xFF, 0xFA, 0xE8)),
    ];
    for (name, background) in backgrounds {
        let plate = over(theme::CASING, background);
        for (ink_name, ink) in [
            ("ordinary", theme::TEXT_PRIMARY),
            ("buy", theme::ink(Side::Buy)),
            ("sell", theme::ink(Side::Sell)),
        ] {
            let ratio = contrast_ratio(ink, plate);
            assert!(
                ratio >= AA,
                "{ink_name} ink over the plate on {name}: {ratio:.2}:1"
            );
        }
        // And on an imbalanced row, where the cell sinks below the plate.
        for side in [Side::Buy, Side::Sell] {
            let cell = over(
                theme::side_color(side).gamma_multiply(IMBALANCE_CELL_ALPHA),
                plate,
            );
            let ratio = contrast_ratio(theme::ink(side), cell);
            assert!(
                ratio >= AA,
                "{side:?} ink over its imbalanced cell on {name}: {ratio:.2}:1"
            );
        }
    }
}

/// The cluster's total column carries a silhouette instead of a heat step,
/// and that is what buys it one ink with no flip rule. The ceiling is
/// load-bearing: past it the column re-enters the unreadable band.
#[test]
fn the_total_columns_silhouette_stays_under_its_ceiling() {
    let plate = over(theme::CASING, theme::CANVAS);
    let filled = over(
        PROFILE_COLOR.gamma_multiply(CLUSTER_TOTAL_SILHOUETTE_ALPHA),
        plate,
    );
    let ratio = contrast_ratio(theme::TEXT_PRIMARY, filled);
    assert!(
        ratio >= AA,
        "primary ink over the total column's silhouette: {ratio:.2}:1"
    );
    const {
        assert!(
            CLUSTER_TOTAL_SILHOUETTE_ALPHA <= 0.42,
            "past 0.42 the silhouette needs a flip rule of its own"
        );
    }
}
