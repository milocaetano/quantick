//! The bundled surfaces as text, every byte a renderer reads off them, against
//! the dump taken at the commit before the flow pane's lights existed: the
//! bulb's off state, and Dark, paint exactly what they painted then.
use crate::{DARK, LIGHT, ResolvedTheme, Token};
use std::fmt::Write;

/// The tokens the lights added; they did not exist before them.
const ADDED: [&str; 1] = ["flow_bulb_lit"];
/// The settings panels' tokens came later still, under one prefix.
const ADDED_PREFIX: &str = "panel_";

fn dump(name: &str, theme: &ResolvedTheme, out: &mut String) {
    writeln!(out, "[{name}]").unwrap();
    for token in Token::ALL {
        if !ADDED.contains(&token.key()) && !token.key().starts_with(ADDED_PREFIX) {
            writeln!(out, "{} {:?}", token.key(), theme.color(token).to_array()).unwrap();
        }
    }
    writeln!(
        out,
        "candles {} {} {} {} {}",
        theme.candle_fill_opacity,
        theme.candle_outline_width,
        theme.candle_outline_opacity,
        theme.candle_wick_opacity,
        theme.chart_time_grid
    )
    .unwrap();
    writeln!(
        out,
        "vwap {:?}",
        theme.plot_ink("VWAP (cumulative)", "vwap")
    )
    .unwrap();
    writeln!(out, "heat {:?}", theme.footprint_heat).unwrap();
    writeln!(out, "{}", toml::to_string(&theme.orderflow).unwrap()).unwrap();
}

fn surfaces() -> String {
    let dark = ResolvedTheme::parse(DARK).unwrap();
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let mut out = String::new();
    dump("dark", &dark, &mut out);
    dump("dark.bookmap", dark.bookmap(), &mut out);
    dump("light", &light, &mut out);
    dump("light.bookmap", light.bookmap(), &mut out);
    dump("light.title_strip", light.title_strip(), &mut out);
    out
}

#[test]
fn dark_and_the_lights_off_paint_the_bytes_they_painted_before_the_lights() {
    let before = include_str!("../tests/fixtures/surfaces-before-lights.txt");
    let now = surfaces();
    for (line, (was, is)) in before.lines().zip(now.lines()).enumerate() {
        assert_eq!(was, is, "line {}", line + 1);
    }
    assert_eq!(before.lines().count(), now.lines().count());
}
