//! Readback must describe the addressed surface, including inherited fields.
use super::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn target() -> (StyleTarget, ChartSettingsTarget) {
    let target = StyleTarget {
        tab_id: 7,
        pane_id: 19,
    };
    (
        target,
        ChartSettingsTarget {
            tab_id: WireU64(7),
            pane_id: WireU64(19),
        },
    )
}
fn settings() -> CandleSettings {
    // These operations never save. An empty path has no preset document and
    // creates no temporary files, so no scratch directory is required.
    CandleSettings::open(PathBuf::new())
}
fn document(style: ChartStyle) -> Value {
    let document = style_wire::encode(&StyleFile::from(&style));
    quantick_control::canonical::validate_control_value(&document).unwrap();
    document
}

#[test]
fn effective_style_readback_resolves_stale_theme_values_for_time_and_bookmap_updates() {
    let descriptor = quantick_control_schema::chart_settings::descriptor();
    let bytes = serde_json::to_vec(&descriptor).unwrap().len();
    assert!(
        bytes <= quantick_control::limits::CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES,
        "chart settings descriptor has {bytes} bytes"
    );
    quantick_control::canonical::validate_control_value(&descriptor.input_schema).unwrap();
    quantick_control::canonical::validate_control_value(&descriptor.output_schema).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let light =
        quantick_stores::appearance::load(&root.join("../appearance/themes/light.toml")).unwrap();
    for bookmap in [false, true] {
        let surface = StyleSurface::new(light.resolved(), bookmap);
        let (target, wire_target) = target();
        let mut session = ChartSettingsSession::default();
        let mut candles = settings();
        let current = surface.resolve(ChartStyle::default());
        apply(
            &mut session,
            &mut candles,
            ChartSettingsInput::Open {
                tab: ChartSettingsTab::Candles,
                target: Some(wire_target),
            },
            Some(target),
            Some(current),
            surface,
        )
        .unwrap();
        let mut supplied = ChartStyle::default();
        supplied.candles.bull_fill = [251, 1, 249];
        supplied.candles.bear_outline = [2, 250, 3];
        supplied.canvas.background = [248, 4, 247];
        supplied.candles.wick_width = 2.5;
        let expected = surface.resolve(supplied);
        assert_ne!(supplied.candles.bull_fill, expected.candles.bull_fill);
        let input = ChartSettingsInput::UpdateCandles {
            target: Some(wire_target),
            style: document(supplied),
        };
        assert_eq!(requested(&input), Some(target));
        let updated = apply(
            &mut session,
            &mut candles,
            input,
            Some(target),
            Some(current),
            surface,
        )
        .unwrap();
        assert_eq!(updated.result.target, Some(wire_target));
        quantick_control::canonical::validate_control_value(
            &serde_json::to_value(&updated.result).unwrap(),
        )
        .unwrap();
        assert_eq!(updated.result.effective_style, Some(document(expected)));
        assert_eq!(updated.style, Some(expected));
        assert!(updated.result.changed);
        assert_eq!(session.editor.target, Some(target));
        let inspected = apply(
            &mut session,
            &mut candles,
            ChartSettingsInput::Inspect {
                target: Some(wire_target),
            },
            Some(target),
            updated.style,
            surface,
        )
        .unwrap();
        assert_eq!(inspected.result.target, updated.result.target);
        assert_eq!(
            inspected.result.effective_style,
            updated.result.effective_style
        );
        assert!(!inspected.result.changed);
        assert_eq!(
            inspected.result.effective_style.unwrap()["ownership"]["bull_fill"],
            json!("theme")
        );
    }
}

#[test]
fn effective_style_readback_resolves_dark_opening_look_on_light_surface_reset() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dark =
        quantick_stores::appearance::load(&root.join("../appearance/themes/dark.toml")).unwrap();
    let light =
        quantick_stores::appearance::load(&root.join("../appearance/themes/light.toml")).unwrap();
    for bookmap in [false, true] {
        let (target, wire_target) = target();
        let dark_surface = StyleSurface::new(dark.resolved(), bookmap);
        let light_surface = StyleSurface::new(light.resolved(), bookmap);
        let mut session = ChartSettingsSession::default();
        let mut candles = settings();
        let opened = dark_surface.resolve(ChartStyle::default());
        apply(
            &mut session,
            &mut candles,
            ChartSettingsInput::Open {
                tab: ChartSettingsTab::Candles,
                target: Some(wire_target),
            },
            Some(target),
            Some(opened),
            dark_surface,
        )
        .unwrap();
        let edited = apply(
            &mut session,
            &mut candles,
            ChartSettingsInput::ApplyCandlePreset {
                name: "glass".into(),
                target: Some(wire_target),
            },
            Some(target),
            Some(opened),
            dark_surface,
        )
        .unwrap()
        .style
        .unwrap();
        let current = light_surface.resolve(edited);
        let other = StyleTarget {
            tab_id: target.tab_id,
            pane_id: target.pane_id + 1,
        };
        assert!(
            apply(
                &mut session,
                &mut candles,
                ChartSettingsInput::ResetCandles {
                    target: Some(ChartSettingsTarget {
                        tab_id: WireU64(other.tab_id),
                        pane_id: WireU64(other.pane_id)
                    })
                },
                Some(other),
                Some(current),
                light_surface
            )
            .is_err()
        );
        assert_eq!(session.editor.target, Some(target));
        let expected = light_surface.resolve(opened);
        let reset = apply(
            &mut session,
            &mut candles,
            ChartSettingsInput::ResetCandles {
                target: Some(wire_target),
            },
            Some(target),
            Some(current),
            light_surface,
        )
        .unwrap();
        assert_eq!(reset.result.target, Some(wire_target));
        assert_eq!(reset.result.effective_style, Some(document(expected)));
        assert_eq!(reset.style, Some(expected));
        assert_eq!(session.editor.target, Some(target));
        let inspected = apply(
            &mut session,
            &mut candles,
            ChartSettingsInput::Inspect {
                target: Some(wire_target),
            },
            Some(target),
            reset.style,
            light_surface,
        )
        .unwrap();
        assert_eq!(
            inspected.result.effective_style,
            reset.result.effective_style
        );
        assert_eq!(inspected.result.candle_preset, reset.result.candle_preset);
        assert!(!inspected.result.changed);
        let expected_theme = if bookmap {
            light.resolved().bookmap()
        } else {
            light.resolved()
        };
        let mut expected_defaults = ChartStyle::default();
        expected_defaults.apply_theme(expected_theme);
        assert_eq!(expected.candles, expected_defaults.candles);
        assert_eq!(expected.canvas, expected_defaults.canvas);
    }
}

/// The candle contour's thickness is the outline width the same call edits,
/// drawn in whole device px: Light's default 1 px is two device px at the
/// trader's 1.5 scale (TradingView's thickness on that screen), and 0.5 px
/// is the thin single pixel. Both are reachable through `update_candles`.
#[test]
fn update_candles_reaches_both_the_default_and_the_thin_contour() {
    use quantick_chart::candle_pixels::{CandleRun, PixelBodies};
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let light =
        quantick_stores::appearance::load(&root.join("../appearance/themes/light.toml")).unwrap();
    let surface = StyleSurface::new(light.resolved(), false);
    let contour = |style: &ChartStyle| {
        let run = CandleRun::for_frame(PixelBodies::Zoom, 10.0, 0.0, &style.candles, 1.5);
        (run.widths().border, run.widths().wick)
    };
    let current = surface.resolve(ChartStyle::default());
    assert_eq!(contour(&current), (2, 2), "Light's default at 1.5 px/pt");

    let (target, wire_target) = target();
    let mut session = ChartSettingsSession::default();
    let mut candles = settings();
    for (width, device_px) in [(0.5, 1), (1.0, 2)] {
        let mut supplied = current;
        supplied.candles.outline_width = width;
        // The trader's own value, not the theme's: what the settings
        // window's thickness slider records when it moves.
        let mut style = document(supplied);
        style["ownership"]["outline_width"] = json!("explicit");
        let updated = apply(
            &mut session,
            &mut candles,
            ChartSettingsInput::UpdateCandles {
                target: Some(wire_target),
                style,
            },
            Some(target),
            Some(current),
            surface,
        )
        .unwrap();
        let style = updated.style.expect("the call returns the applied style");
        assert_eq!(style.candles.outline_width, width);
        assert_eq!(contour(&style).0, device_px, "outline width {width}");
    }
}
