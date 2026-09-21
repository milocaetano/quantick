//! The price plates and the level lines have to name the same price.
//!
//! Issue #157: the POC line was drawn at the centre of its row and the POC
//! plate printed the row's low edge, so the plate named a price exactly half a
//! row below the mark the chart had drawn — and a trader who bids the printed
//! number bids somewhere the chart never marked.
//!
//! These tests do not compare the plate with a second copy of the arithmetic;
//! they take the price the plate prints and ask the drawing whether a line is
//! there, through the pointer hit test, which is the one surface that reads the
//! painted geometry back. The pointer is put to the right of every histogram
//! row, so only a level line can answer, and each level is switched off again
//! to prove the answer came from that line and not from its neighbour.
use super::super::paint::level_plates;
use super::*;

/// Where the range is on screen. Rows occupy the left half (`width_frac`), so
/// x = 400 is past every row and only the level lines reach it.
const RANGE: [egui::Pos2; 2] = [egui::pos2(100.0, 0.0), egui::pos2(500.0, 400.0)];
const CLEAR_OF_THE_ROWS_X: f32 = 400.0;

/// A profile at a real B3 price with a group of 10, the shape issue #157 was
/// reproduced against: the heaviest row is bucket 9501, low edge 95010, centre
/// 95015.
fn grouped_cache(rows: &[(i64, i64)], group: Decimal) -> FrvpCache {
    use quantick_anchored_studies::{ProfileInputs, ProfileRequest};
    use quantick_engine::{BarSpec, Side, Trade, bar_registry::BarConfiguration};
    let mut builder = BarConfiguration::from(BarSpec::Tick(1)).build();
    let mut footprint = quantick_engine::FootprintBuilder::new(group, 4096);
    let mut last = None;
    for &(price, quantity) in rows {
        let trade = Trade {
            agg_id: 1,
            timestamp_ms: 0,
            price: Decimal::from(price),
            quantity: Decimal::from(quantity),
            side: Side::Buy,
        };
        last = builder.push(&trade);
        footprint.push(&trade);
    }
    let bars = vec![last.expect("fixture trade closes a bar"); 20];
    let ladder = footprint.close().expect("the fixture traded");
    let mut owned = None;
    FrvpCache::refresh(
        &mut owned,
        ProfileRequest {
            min_bar: 0.0,
            max_bar: f32::MAX,
            extend_right: true,
            approximate_history: false,
            value_area_pct: DEFAULT_VALUE_AREA_PCT,
        },
        &ProfileInputs {
            closed: &bars,
            ladders: &[ladder],
            prefix: &[],
            partial_ladder: None,
            group,
            series_revision: 0,
            partial_version: 0,
            blocked: false,
            side_inferred: false,
            partial_bucket_slot: None,
            budget: 1500,
        },
    );
    owned.expect("refresh installs a result")
}

fn payload(show_poc: bool, show_value_area: bool) -> FrvpPayload {
    FrvpPayload {
        width_frac: 0.5,
        show_labels: true,
        show_poc,
        show_value_area,
        cache: Some(grouped_cache(
            &[(95_010, 10), (95_020, 8), (95_000, 7), (94_990, 2)],
            Decimal::from(10),
        )),
        ..FrvpPayload::default()
    }
}

fn context<'a>(payload: &'a FrvpPayload, scale: &'a PriceScale) -> DrawContext<'a> {
    DrawContext {
        payload,
        anchors: &[],
        scale,
        px_per_bar: 20.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: false,
        halo: false,
        content_editing: false,
    }
}

/// Is a level line drawn at `price`, as the pointer sees it?
fn line_at(payload: &FrvpPayload, price: Decimal) -> bool {
    // 0.25 price units per pixel, so the half-row the bug moved the plate by
    // is twenty pixels — far outside any slop the selector allows.
    let scale = PriceScale::from_range(94_950.0, 95_070.0, 0.0, 480.0);
    TOOL.hit_test(
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 500.0)),
        &RANGE,
        egui::pos2(CLEAR_OF_THE_ROWS_X, scale.y(to_f64(price))),
        3.0,
        &context(payload, &scale),
    )
}

/// The plate a payload prints for `name`, or `None` where it is hidden.
fn plate(payload: &FrvpPayload, name: &str) -> Option<Decimal> {
    let cache = payload.cache.as_ref().expect("the fixture has a cache");
    let output = cache.output();
    let (profile, area) = output.profile.expect("the fixture folded a profile");
    let area = area.expect("the fixture has a value area");
    level_plates(profile, area, payload)
        .into_iter()
        .flatten()
        .find(|(plate, ..)| *plate == name)
        .map(|(_, price, _)| price)
}

#[test]
fn profile_poc_plate_names_the_price_the_poc_line_marks() {
    let marked = payload(true, false);
    let poc = plate(&marked, "POC").expect("the POC plate is shown");
    assert!(
        line_at(&marked, poc),
        "the POC plate names a price where no POC line is drawn"
    );
    assert!(
        !line_at(&payload(false, false), poc),
        "with the POC hidden nothing else answers at that price, so the hit was the POC line"
    );
    // The regression in words, after the pin that would have caught it: the
    // plate used to read 95010, the low edge of bucket 9501, while the line
    // was drawn at 95015, the row's centre.
    assert_eq!(poc.to_string(), "95015", "the POC plate names the row centre");
}

#[test]
fn profile_value_area_plates_name_the_prices_their_dashes_mark() {
    let marked = payload(false, true);
    let hidden = payload(false, false);
    for name in ["VAH", "VAL"] {
        let price = plate(&marked, name).expect("the value-area plates are shown");
        assert!(
            line_at(&marked, price),
            "the {name} plate names a price where no {name} dash is drawn"
        );
        assert!(
            !line_at(&hidden, price),
            "with the value area hidden nothing answers at {price}, so the hit was the {name} dash"
        );
    }
}

#[test]
fn profile_plates_follow_the_payload_toggles() {
    assert!(plate(&payload(true, false), "POC").is_some());
    assert!(plate(&payload(true, false), "VAH").is_none());
    assert!(plate(&payload(false, true), "POC").is_none());
    assert!(plate(&payload(false, true), "VAL").is_some());
}
