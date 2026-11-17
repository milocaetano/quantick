//! Each ground keeps its own saved defaults, and a file from before grounds
//! lands on the one the trader was saving on.

use super::*;
use crate::new_drawing_from_defaults;

/// The trader's complaint: a colour saved on the light time chart showed
/// up on the dark order-flow pane beside it. Each ground keeps its own.
#[test]
fn a_default_saved_on_one_ground_never_reaches_the_other() {
    let path = crate::scratch::thread_dir("ground-drawing-defaults").join(PRESETS_FILE);
    let _ = std::fs::remove_file(&path);
    let mut store = PresetStore::load_from(&path);
    let mut appearance = quantick_appearance::Appearance::default();
    appearance.apply(quantick_appearance::LIGHT).unwrap();
    store.sync_theme(&appearance);
    let line = crate::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "trend-line")
        .unwrap();
    let (light, dark) = (ChartSurface::Paper, ChartSurface::Flow);
    let built_in_dark = new_drawing_from_defaults(&store, dark, line).style;
    let mut drawings = crate::Drawings::default();
    assert!(!drawings.place(line, crate::ChartPoint::at(1.0, 100.0)));
    assert!(drawings.place(line, crate::ChartPoint::at(9.0, 200.0)));
    let mut red = drawings.items()[0].clone();
    red.style.color = quantick_appearance::PremultipliedRgba::from_rgb(220, 30, 30);
    crate::save_tool_default(&mut store, light, &red);

    assert_eq!(
        new_drawing_from_defaults(&store, light, line).style,
        red.style
    );
    assert_eq!(
        new_drawing_from_defaults(&store, dark, line).style,
        built_in_dark
    );
    assert!(!crate::has_saved_default(&store, dark, line));

    let mut blue = red.clone();
    blue.style.color = quantick_appearance::PremultipliedRgba::from_rgb(30, 90, 220);
    crate::save_tool_default(&mut store, dark, &blue);
    assert_eq!(
        new_drawing_from_defaults(&store, dark, line).style,
        blue.style
    );
    assert_eq!(
        new_drawing_from_defaults(&store, light, line).style,
        red.style
    );

    crate::reset_tool_default(&mut store, dark, line);
    assert_eq!(
        new_drawing_from_defaults(&store, dark, line).style,
        built_in_dark
    );
    let mut reloaded = PresetStore::load_from(&path);
    reloaded.sync_theme(&appearance);
    assert_eq!(
        new_drawing_from_defaults(&reloaded, light, line).style,
        red.style
    );
}

/// A file written before grounds held one default per tool; it stays the
/// light ground's, and the dark ground starts empty.
#[test]
fn a_file_from_before_grounds_keeps_its_defaults_on_the_light_ground() {
    let path = crate::scratch::thread_dir("legacy-ground-defaults").join(PRESETS_FILE);
    let legacy = "version = 1\n[tools.trend-line]\nconfig = { marker = 'old' }\n[tools.trend-line.style]\ncolor = '#dc1e1e'\nwidth_px = 2.0\nfill_alpha = 0\n";
    std::fs::write(&path, legacy).unwrap();
    let mut store = PresetStore::load_from(&path);
    let red = quantick_appearance::PremultipliedRgba::from_rgb(220, 30, 30);
    let light = store.default_style("trend-line", Ground::Light).unwrap();
    assert_eq!(light.color, red);
    assert!(store.has_default_config("trend-line", Ground::Light));
    assert_eq!(store.default_style("trend-line", Ground::Dark), None);
    assert!(!store.has_default_config("trend-line", Ground::Dark));

    store.set_default_style("trend-line", Ground::Dark, Some(light));
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("dark_style"), "{written}");
    let reloaded = PresetStore::load_from(&path);
    assert_eq!(
        reloaded.default_style("trend-line", Ground::Light),
        Some(light)
    );
    assert_eq!(
        reloaded.default_style("trend-line", Ground::Dark),
        Some(light)
    );
}

/// The trader saved those defaults on the chart his theme painted: on the
/// dark theme they were dark-ground defaults, and they stay his there.
#[test]
fn a_file_from_before_grounds_follows_the_theme_it_opens_under_once() {
    let path = crate::scratch::thread_dir("legacy-dark-defaults").join(PRESETS_FILE);
    let legacy = "version = 1
[tools.trend-line.style]
color = '#dc1e1e'
width_px = 2.0
fill_alpha = 0
";
    std::fs::write(&path, legacy).unwrap();
    let red = quantick_appearance::PremultipliedRgba::from_rgb(220, 30, 30);
    let mut store = PresetStore::load_from(&path);
    store.sync_theme(&quantick_appearance::Appearance::default());
    let dark = store
        .default_style("trend-line", Ground::Dark)
        .map(|style| style.color);
    assert_eq!(dark, Some(red));
    assert_eq!(store.default_style("trend-line", Ground::Light), None);

    let mut light = quantick_appearance::Appearance::default();
    light.apply(quantick_appearance::LIGHT).unwrap();
    let mut reopened = PresetStore::load_from(&path);
    reopened.sync_theme(&light);
    let dark = reopened
        .default_style("trend-line", Ground::Dark)
        .map(|style| style.color);
    assert_eq!(
        dark,
        Some(red),
        "the file says grounds now; a theme change moves nothing"
    );
    assert_eq!(reopened.default_style("trend-line", Ground::Light), None);
}
