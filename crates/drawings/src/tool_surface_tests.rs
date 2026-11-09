//! A drawing born in its tool's default paints each surface's own colour for
//! that role; an authored colour keeps the plain legibility rule.
use quantick_appearance::{LIGHT, PremultipliedRgba, ResolvedTheme};

use crate::{DRAWING_TOOLS, DrawingTool};

fn tool(id: &str) -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .unwrap()
}

#[test]
fn a_new_anchored_vwap_paints_each_light_surface_its_own_vwap_colour() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let vwap = tool("anchored-vwap");
    let born = crate::ToolCatalog::themed_style(vwap, Some(&light));
    let paper = vwap.painted_style(born, &light).color.to_array();
    assert_eq!(paper, [0x2B, 0x36, 0x40, 255]);
    let flow = vwap.painted_style(born, light.bookmap()).color.to_array();
    assert_eq!(flow, [0x8A, 0xB4, 0xF8, 255]);
    // The width and fill are the drawing's own on every surface.
    let painted = vwap.painted_style(born, light.bookmap());
    assert_eq!(
        (painted.width_px, painted.fill_alpha),
        (born.width_px, born.fill_alpha)
    );
}

#[test]
fn an_authored_colour_on_the_flow_surface_keeps_the_legibility_rule() {
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    let flow = light.bookmap();
    let vwap = tool("anchored-vwap");
    let mut style = vwap.default_style();
    for authored in [
        PremultipliedRgba::from_rgb(0x24, 0x5D, 0xB1),
        PremultipliedRgba::from_rgb(0x2B, 0x36, 0x41),
    ] {
        style.color = authored;
        let painted = vwap.painted_style(style, flow).color;
        assert_eq!(painted, flow.legible_drawing(authored));
    }
    // A trend line born in Light's slate still reads as the flow default.
    let line = tool("trend-line");
    let born = crate::ToolCatalog::themed_style(line, Some(&light));
    let painted = line.painted_style(born, flow).color;
    assert_eq!(
        painted,
        flow.color(quantick_appearance::Token::DrawingDefault)
    );
}

#[test]
fn dark_paints_every_default_as_born() {
    let dark = ResolvedTheme::default();
    for tool in DRAWING_TOOLS {
        let born = crate::ToolCatalog::themed_style(tool, Some(&dark));
        let painted = tool.painted_style(born, dark.bookmap());
        assert_eq!(painted, born, "{}", tool.id());
    }
}
