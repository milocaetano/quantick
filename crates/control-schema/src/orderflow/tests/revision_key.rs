//! The engine revision key moves when the order-flow config changes, and
//! only then: a capture that compares keys must neither miss a change nor
//! report one that did not happen.
use super::EngineRevisionKey;
use quantick_orderflow::{
    HeatmapConfig, IntensityMode, LaneWindow, LiveLaneStyle, TickBubbleStyle, VolumeDotStyle,
    engine::CaptureStatus,
};
use rust_decimal::Decimal;

/// A named edit of a config.
type Change = (&'static str, fn(&mut HeatmapConfig));

fn key(config: &HeatmapConfig) -> EngineRevisionKey {
    EngineRevisionKey::from_config(config, &CaptureStatus::Connecting, Decimal::new(1, 2))
}

#[test]
fn an_unchanged_config_keeps_the_engine_revision_key() {
    let config = HeatmapConfig::default();
    assert_eq!(key(&config), key(&config.clone()));
}

#[test]
fn every_kind_of_config_change_moves_the_engine_revision_key() {
    let base = HeatmapConfig {
        min_unattributed_reduction: 0.0,
        price_grouping: Decimal::new(1, 1),
        ..HeatmapConfig::default()
    };
    let changes: [Change; 9] = [
        ("a switch", |c| c.show_legend = !c.show_legend),
        ("a float", |c| c.opacity /= 2.0),
        ("a nested float", |c| c.bubbles.min_radius += 1.0),
        ("a wide float", |c| c.volume_dots.full_quantity += 1.0),
        ("the lane window", |c| {
            c.live_lane.window = LaneWindow::Fixed { ms: 60_000 };
        }),
        ("tick spheres", |c| {
            c.volume_dots.tick_bubbles = Some(TickBubbleStyle {
                reference_quantity: 1.0,
                max_radius: 32.0,
                label_min_radius: 12.0,
                opacity: 0.9,
            });
        }),
        ("a fixed intensity", |c| {
            c.intensity_mode = IntensityMode::Fixed(Decimal::ONE);
        }),
        // 0.1 and 0.10 are equal numbers but not the same setting.
        ("the grouping's scale alone", |c| {
            c.price_grouping = Decimal::new(10, 2);
        }),
        ("the sign of a zero", |c| {
            c.min_unattributed_reduction = -0.0
        }),
    ];
    for (what, change) in changes {
        let mut changed = base.clone();
        change(&mut changed);
        assert_ne!(key(&base), key(&changed), "{what} must move the key");
    }
}

/// A NaN equals no number, itself included; a key that compared it as a
/// number would report a change at every capture.
#[test]
fn a_config_holding_nan_keeps_its_engine_revision_key() {
    let defaults = HeatmapConfig::default();
    let config = HeatmapConfig {
        opacity: f32::NAN,
        volume_dots: VolumeDotStyle {
            full_quantity: f64::NAN,
            ..defaults.volume_dots
        },
        live_lane: LiveLaneStyle {
            window: LaneWindow::Auto { zoom: f32::NAN },
            ..defaults.live_lane.clone()
        },
        ..defaults
    };
    assert_eq!(key(&config), key(&config.clone()));
}
