//! The values the headless drawings carry as copies of the window's own:
//! the theme tokens a mark paints with, the icon font's codepoints and the
//! letters a tool's shortcut names. Each is pinned here to its owner, so a
//! theme or font change that forgets `quantick_drawings` fails rather than
//! leaving a mark in last season's colour.

use eframe::egui;
use egui_phosphor::regular as phosphor;
use quantick_drawings::{icons, palette};

use super::{DRAWING_TOOLS, shortcut_key, to_color32};
use crate::theme;

#[test]
fn the_mark_palette_is_the_theme() {
    for (name, mark, token) in [
        ("canvas", palette::CANVAS, theme::CANVAS),
        ("text muted", palette::TEXT_MUTED, theme::TEXT_MUTED),
        ("buy", palette::BUY, theme::BUY),
        ("sell", palette::SELL, theme::SELL),
        ("poc", palette::POC, theme::POC),
        ("accent", palette::ACCENT, theme::ACCENT),
        ("casing", palette::CASING, theme::CASING),
        ("draw cyan", palette::DRAW_CYAN, theme::DRAW_CYAN),
    ] {
        assert_eq!(to_color32(mark), token, "{name}");
    }
    assert_eq!(palette::CASING_EXTRA_PX, theme::CASING_EXTRA_PX);
}

#[test]
fn the_tool_glyphs_are_the_icon_font_s() {
    for (mark, font) in [
        (icons::ANCHOR_SIMPLE, phosphor::ANCHOR_SIMPLE),
        (icons::ARROWS_HORIZONTAL, phosphor::ARROWS_HORIZONTAL),
        (
            icons::ARROWS_OUT_LINE_HORIZONTAL,
            phosphor::ARROWS_OUT_LINE_HORIZONTAL,
        ),
        (icons::ARROWS_VERTICAL, phosphor::ARROWS_VERTICAL),
        (icons::ARROW_FAT_DOWN, phosphor::ARROW_FAT_DOWN),
        (icons::ARROW_FAT_UP, phosphor::ARROW_FAT_UP),
        (icons::ARROW_LINE_RIGHT, phosphor::ARROW_LINE_RIGHT),
        (icons::ARROW_UP_RIGHT, phosphor::ARROW_UP_RIGHT),
        (icons::CHART_BAR_HORIZONTAL, phosphor::CHART_BAR_HORIZONTAL),
        (icons::CIRCLE, phosphor::CIRCLE),
        (icons::FLOW_ARROW, phosphor::FLOW_ARROW),
        (icons::LINE_SEGMENT, phosphor::LINE_SEGMENT),
        (icons::LINE_VERTICAL, phosphor::LINE_VERTICAL),
        (icons::MINUS, phosphor::MINUS),
        (icons::PARALLELOGRAM, phosphor::PARALLELOGRAM),
        (icons::RECTANGLE, phosphor::RECTANGLE),
        (icons::ROWS, phosphor::ROWS),
        (icons::ROWS_PLUS_TOP, phosphor::ROWS_PLUS_TOP),
        (icons::RULER, phosphor::RULER),
        (icons::SCRIBBLE, phosphor::SCRIBBLE),
        (icons::SHAPES, phosphor::SHAPES),
        (icons::TEXT_T, phosphor::TEXT_T),
        (icons::TREND_UP, phosphor::TREND_UP),
        (icons::TRIANGLE, phosphor::TRIANGLE),
    ] {
        assert_eq!(mark, font);
    }
}

#[test]
fn every_tool_shortcut_names_a_key_the_window_has() {
    for tool in DRAWING_TOOLS {
        if let Some(shortcut) = tool.shortcut() {
            let key = shortcut_key(shortcut);
            assert!(
                key.is_some(),
                "{}: no key for {:?}",
                tool.id(),
                shortcut.key
            );
            assert_eq!(
                key.map(egui::Key::name),
                Some(shortcut.key.to_string().as_str()),
                "{}: the rail labels the key it arms",
                tool.id()
            );
        }
    }
}
