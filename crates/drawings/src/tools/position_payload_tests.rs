use super::*;

fn custom_look() -> PositionPayload {
    PositionPayload {
        ratio: 1.3,
        projected_offset: Some(31.5),
        target_rgb: [0x12, 0x34, 0x56],
        stop_rgb: [0xAB, 0xCD, 0xEF],
        zone_opacity_pct: 40,
        span_bars: Some([-2.0, 30.0]),
    }
}

/// A preset is the look — ratio, both colours, opacity — and never one
/// chart's geometry; a saved drawing is all of it.
#[test]
fn colours_opacity_and_ratio_round_trip_through_a_preset_and_a_save() {
    let source = custom_look();
    let mut preset = PositionPayload::default();
    assert!(preset.import_preset(&source.export_preset().unwrap()));
    assert_eq!(
        preset,
        PositionPayload {
            projected_offset: None,
            span_bars: None,
            ..custom_look()
        }
    );
    let mut restored = PositionPayload::default();
    assert!(restored.import_saved(&source.export_saved().unwrap()));
    assert_eq!(restored, source);
}

/// A drawing saved before colours existed: `ratio = 2.0` and nothing else.
/// It keeps its ratio and takes the factory colours.
#[test]
fn a_payload_saved_before_colours_still_loads() {
    let old: toml::Value = toml::from_str("version = 1\nratio = 2.0\n").unwrap();
    let mut saved = custom_look();
    assert!(saved.import_saved(&old));
    assert_eq!(
        saved,
        PositionPayload {
            ratio: 2.0,
            ..PositionPayload::default()
        }
    );
    let pinned: toml::Value =
        toml::from_str("version = 1\nratio = 2.0\nprojected_offset = 12.5\n").unwrap();
    let mut saved = PositionPayload::default();
    assert!(saved.import_saved(&pinned));
    assert_eq!(saved.projected_offset, Some(12.5));
    let mut preset = custom_look();
    assert!(preset.import_preset(&old));
    assert_eq!((preset.ratio, preset.target_rgb), (2.0, FACTORY_TARGET_RGB));
}

#[test]
fn an_unreadable_look_is_refused_and_changes_nothing() {
    let unreadable = [
        "version = 1\nratio = 0.0\n",
        "version = 1\nratio = 25.0\n",
        "version = 2\nratio = 1.0\n",
        "version = 1\nratio = 1.0\ntarget_color = \"green\"\n",
        "version = 1\nratio = 1.0\nzone_opacity_pct = 101\n",
    ];
    for text in unreadable {
        let value: toml::Value = toml::from_str(text).unwrap();
        let mut payload = PositionPayload::default();
        assert!(!payload.import_preset(&value), "{text}");
        assert!(!payload.import_saved(&value), "{text}");
        assert_eq!(payload, PositionPayload::default(), "{text}");
    }
    let narrow: toml::Value =
        toml::from_str("version = 1\nratio = 1.0\nspan_bars = [0.0, 3.0]\n").unwrap();
    assert!(!PositionPayload::default().import_saved(&narrow));
}

#[test]
fn colours_read_and_write_as_hex() {
    assert_eq!(rgb_hex(FACTORY_TARGET_RGB), "#2EA064");
    assert_eq!(parse_rgb_hex("#d63031"), Some(FACTORY_STOP_RGB));
    for bad in ["d63031", "#d6303", "#d630311", "#zz3031", "#d63é1"] {
        assert_eq!(parse_rgb_hex(bad), None, "{bad}");
    }
}

#[test]
fn a_preset_ratio_is_no_custom_plan_and_any_other_is() {
    for preset in RATIO_PRESETS {
        assert_eq!(preset_of(preset), Some(preset));
    }
    let custom = PositionPayload {
        ratio: 1.3,
        ..PositionPayload::default()
    };
    assert!(crate::position_preset_of(custom.ratio).is_none());
    assert_eq!(preset_of(1.5), None, "1.5 left the menu");
}

#[test]
fn opacity_and_width_refuse_what_cannot_be_drawn() {
    let anchors = [
        crate::ChartPoint::at(10.0, 100.0),
        crate::ChartPoint::at(30.0, 90.0),
    ];
    let mut payload = PositionPayload::default();
    assert_eq!(payload.width_bars(&anchors), 20.0);
    assert_eq!(
        payload.set_zone_opacity(101),
        Err(PositionRefusal::OpacityOutOfRange)
    );
    for bars in [13.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            payload.set_width_bars(&anchors, bars),
            Err(PositionRefusal::WidthOutOfRange)
        );
    }
    assert_eq!(payload, PositionPayload::default());
    payload.set_width_bars(&anchors, 40.0).unwrap();
    assert_eq!(payload.span_bars, Some([0.0, 40.0]));
    assert_eq!(payload.width_bars(&anchors), 40.0);
    payload.set_zone_opacity(0).unwrap();
    assert_eq!(payload.zone_alpha(), 0);
}
