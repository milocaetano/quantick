use quantick_appearance::Token;
use rust_decimal::Decimal;

use super::big_trades::{BIG_TRADES_EMPTY, NotionalUnit, big_trades_view};
use super::depth::{DEPTH_EMPTY, depth_rows_per_side, depth_view};
use super::liquidations::{LIQUIDATIONS_NOT_PROVIDED, liquidations_view};
use super::open_interest::{OPEN_INTEREST_NOT_PROVIDED, READING_NOTE, open_interest_view};
use super::scene::{ROW_PX, ScenePrim, WidgetScene};
use super::spot_perp_cvd::{SPOT_PERP_NOT_PROVIDED, spot_perp_cvd_view};
use super::{big_trades_tests, depth_tests, derivatives_tests};

const SIZE: (f32, f32) = (400.0, 300.0);

fn texts(scene: &WidgetScene) -> Vec<&str> {
    scene.texts().collect()
}

/// Every item lies inside the body it was laid out for.
fn assert_inside(scene: &WidgetScene) {
    for prim in &scene.prims {
        let points: Vec<[f32; 2]> = match prim {
            ScenePrim::Rect { rect, .. } => {
                vec![[rect[0], rect[1]], [rect[0] + rect[2], rect[1] + rect[3]]]
            }
            ScenePrim::Text { at, .. } => vec![*at],
            ScenePrim::Line { points, .. } => points.clone(),
            ScenePrim::Circle { centre, .. } => vec![*centre],
        };
        for [x, y] in points {
            assert!((0.0..=SIZE.0).contains(&x), "{prim:?} leaves the body");
            assert!((0.0..=SIZE.1).contains(&y), "{prim:?} leaves the body");
        }
    }
}

#[test]
fn depth_paints_every_row_with_the_last_price_banded() {
    let view = depth_view(Some(&depth_tests::model()), Decimal::new(50, 2), 40.0, 60.0);
    let scene = view.paint(SIZE.0, SIZE.1);
    assert_eq!(scene.rows, 4);
    assert_eq!(scene.chips, vec![("tick 0.50".to_owned(), true)]);
    let texts = texts(&scene);
    assert!(texts.contains(&"76200.00"), "{texts:?}");
    assert!(texts.iter().any(|text| text.starts_with("spread 0.50")));
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with("unattributed L2 reduction: ask 0.75 at 76200.50")),
        "{texts:?}"
    );
    let accent = scene.prims.iter().filter(|prim| {
        matches!(prim, ScenePrim::Rect { token: Token::Accent, rect, .. } if rect[2] == SIZE.0)
    });
    assert_eq!(accent.count(), 1, "one banded last-price row");
    assert_inside(&scene);
    assert_eq!(depth_rows_per_side(SIZE.1), 6);
}

#[test]
fn depth_without_a_book_says_so_and_nothing_else() {
    let scene = depth_view(None, Decimal::ONE, 40.0, 60.0).paint(SIZE.0, SIZE.1);
    assert_eq!(scene.empty.as_deref(), Some(DEPTH_EMPTY));
    assert_eq!(texts(&scene), vec![DEPTH_EMPTY]);
    assert_eq!(scene.rows, 0);
}

#[test]
fn big_trades_paints_a_bubble_per_row_and_the_presets_as_chips() {
    let layout = big_trades_tests::layout("250000", true, NotionalUnit::USD);
    let scene = big_trades_view(Some(&big_trades_tests::model()), &layout).paint(SIZE.0, SIZE.1);
    let circles = scene
        .prims
        .iter()
        .filter(|prim| matches!(prim, ScenePrim::Circle { .. }))
        .count();
    assert_eq!(circles, scene.rows);
    assert!(scene.rows > 0);
    let chips: Vec<(&str, bool)> = scene
        .chips
        .iter()
        .map(|(text, on)| (text.as_str(), *on))
        .collect();
    assert_eq!(
        chips,
        [("50k", false), ("250k", true), ("1M", false), ("merge fills", true)]
    );
    assert!(texts(&scene).iter().any(|text| text.starts_with("last 5 min")));
    assert_inside(&scene);
}

#[test]
fn big_trades_takes_its_presets_from_the_layout() {
    let mut layout = big_trades_tests::layout("500000", false, NotionalUnit::QUOTE);
    layout.presets = &[100_000, 500_000];
    let scene = big_trades_view(None, &layout).paint(SIZE.0, SIZE.1);
    assert_eq!(scene.empty.as_deref(), Some(BIG_TRADES_EMPTY));
    assert_eq!(scene.chips[1], ("500k".to_owned(), true));
    assert_eq!(scene.chips.len(), 3);
}

#[test]
fn a_list_cut_by_the_body_counts_only_the_rows_drawn() {
    let layout = big_trades_tests::layout("0", false, NotionalUnit::USD);
    let scene = big_trades_view(Some(&big_trades_tests::model()), &layout).paint(SIZE.0, 3.0 * ROW_PX);
    assert_eq!(scene.rows, 1);
}

#[test]
fn derivative_panes_paint_their_data_inside_the_body() {
    let liquidations = liquidations_view(
        Some(&derivatives_tests::liquidations_model()),
        &derivatives_tests::liquidations_layout(),
    )
    .paint(SIZE.0, SIZE.1);
    assert!(liquidations.rows > 0);
    assert_inside(&liquidations);
    let oi = open_interest_view(
        Some(&derivatives_tests::oi_model()),
        &derivatives_tests::oi_layout(),
    )
    .paint(SIZE.0, SIZE.1);
    let reading = texts(&oi)
        .into_iter()
        .find(|text| text.contains(" = "))
        .expect("the reading rule is painted");
    assert!(reading.ends_with(READING_NOTE), "{reading}");
    let cvd = spot_perp_cvd_view(
        Some(&derivatives_tests::cvd_model()),
        &derivatives_tests::cvd_layout(),
    )
    .paint(SIZE.0, SIZE.1);
    assert!(cvd.rows > 0);
    assert!(texts(&cvd).iter().any(|text| text.contains("perp")));
}

#[test]
fn a_feed_without_derivatives_paints_the_not_provided_state() {
    let liquidations = liquidations_view(None, &derivatives_tests::liquidations_layout());
    let oi = open_interest_view(None, &derivatives_tests::oi_layout());
    let cvd = spot_perp_cvd_view(None, &derivatives_tests::cvd_layout());
    let scenes = [
        (liquidations.paint(SIZE.0, SIZE.1), LIQUIDATIONS_NOT_PROVIDED),
        (oi.paint(SIZE.0, SIZE.1), OPEN_INTEREST_NOT_PROVIDED),
        (cvd.paint(SIZE.0, SIZE.1), SPOT_PERP_NOT_PROVIDED),
    ];
    for (scene, text) in scenes {
        assert_eq!(scene.empty.as_deref(), Some(text));
        assert_eq!(texts(&scene), vec![text]);
        assert_eq!(scene.rows, 0);
    }
}
