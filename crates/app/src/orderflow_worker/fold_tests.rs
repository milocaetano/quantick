//! [`fold_parked`]: only a layout request absorbs the one parked before it.

use super::*;
use quantick_engine::Side;

fn request(first_bar_index: usize) -> ProjectionRequest {
    ProjectionRequest {
        timeline_revision: 1,
        first_bar_index,
        closed: Vec::new(),
        partial: None,
        lane: false,
        on_newest_bar: true,
        lane_reference_ms: None,
        price_range: (90.0, 110.0),
    }
}

fn print() -> BookCommand {
    BookCommand::Trade(Trade {
        agg_id: 1,
        timestamp_ms: 1_000,
        price: Decimal::from(100),
        quantity: Decimal::ONE,
        side: Side::Buy,
    })
}

#[test]
fn the_newest_layout_replaces_a_parked_layout() {
    let mut older = BookCommand::Project(request(1));
    assert!(fold_parked(&mut older, BookCommand::Project(request(7))).is_none());
    assert!(matches!(&older, BookCommand::Project(r) if r.first_bar_index == 7));
}

#[test]
fn prints_depth_and_configuration_keep_their_place() {
    let mut older = print();
    assert!(
        fold_parked(&mut older, print()).is_some(),
        "prints never fold"
    );
    assert!(fold_parked(&mut older, BookCommand::Project(request(1))).is_some());
    let mut layout = BookCommand::Project(request(1));
    assert!(
        fold_parked(&mut layout, print()).is_some(),
        "a print after a layout"
    );
    let mut config = BookCommand::ApplyVisualConfig(HeatmapConfig::default());
    assert!(
        fold_parked(
            &mut config,
            BookCommand::ApplyVisualConfig(HeatmapConfig::default())
        )
        .is_some(),
        "a configuration change can prune history; the next one must not skip it"
    );
}
