//! The layout picker (alternative A): which token each part of its chrome,
//! words and cell states wears, and the workspace colour's inks clamped per
//! theme. The ring, a pane's edge and the ✓ hold [`RING_CONTRAST`] on what
//! they stand on; the flow pane's fill holds [`FILL_CONTRAST`] on an empty
//! chart cell, or gives way to a wider edge. Graded headless; the painter
//! only reads the bytes.

use crate::color::{RgbaExt, with_opacity};
use crate::contrast::{contrast_ratio, legible_at, over};
use crate::{PremultipliedRgba, ResolvedTheme, Token};

/// The ring, the flow pane's edge and the ✓ badge on the popover's ground.
pub const RING_CONTRAST: f32 = 3.0;
/// The flow pane's fill on an empty chart cell.
pub const FILL_CONTRAST: f32 = 1.5;
/// Every word on its ground.
pub const TEXT_CONTRAST: f32 = 4.5;
/// The timeframe panes' edges: the colour at this opacity, out of 255 (60%).
pub const TIME_EDGE_ALPHA: u8 = 153;
/// The flow pane's edge, and its edge without a fill, in points.
pub const FLOW_EDGE_PT: f32 = 1.0;
pub const FLOW_EDGE_BARE_PT: f32 = 2.0;
/// The flow fill's share of the way from the ground to the colour: the first
/// tried, the step, and the most.
const FILL_SHARE: [f32; 3] = [0.25, 0.05, 0.6];
/// The current cell's plate: its share toward the colour in Light and Dark.
const PLATE_SHARE: [f32; 2] = [0.1, 0.18];

/// The token of each part of the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerChrome {
    /// Behind the grid, and the header block with its rule: menu B's.
    pub ground: Token,
    pub block: Token,
    pub block_edge: Token,
    /// The 1 px border.
    pub edge: Token,
    /// A name, a cell's label.
    pub ink: Token,
    /// The captions, the hint and the status words: 4.5:1 on the ground and
    /// the block in both themes, where menu B's faint is not.
    pub support: Token,
    /// A cell under the pointer.
    pub hover: Token,
    /// The keyboard focus ring: never the workspace colour of the current ring.
    pub focus: Token,
    /// What a press lays over the hover plate to sink it.
    pub press: Token,
}

/// The picker as `dark` or Light paints it.
#[must_use]
pub const fn chrome(dark: bool) -> PickerChrome {
    let menu = crate::workspace_menu::chrome(dark);
    PickerChrome {
        ground: menu.ground,
        block: menu.block,
        block_edge: menu.block_edge,
        edge: menu.edge,
        ink: menu.ink,
        support: Token::TextSupport,
        hover: menu.hover,
        focus: Token::TextPrimary,
        press: Token::BareHover,
    }
}

/// The pressed plate: the hover plate under the press overlay, a step deeper.
#[must_use]
pub fn pressed(theme: &ResolvedTheme) -> PremultipliedRgba {
    let look = chrome(theme.dark);
    over(theme.color(look.press), theme.color(look.hover))
}

/// A workspace colour's inks on one ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColourInks {
    /// The current cell's ring, the ✓ badge and the flow pane's edge: the
    /// colour, shaded just far enough to hold [`RING_CONTRAST`].
    pub ring: PremultipliedRgba,
    /// The current cell's plate.
    pub plate: PremultipliedRgba,
    /// The flow pane's fill; `None` when no tint holds [`FILL_CONTRAST`], and
    /// the edge widens to [`FLOW_EDGE_BARE_PT`] instead.
    pub fill: Option<PremultipliedRgba>,
    /// The flow pane's edge width, in points.
    pub flow_edge: f32,
    /// A timeframe pane's edge: the ring at [`TIME_EDGE_ALPHA`].
    pub time_edge: PremultipliedRgba,
    /// The ✓ on the badge: black or white, whichever reads more.
    pub check: PremultipliedRgba,
}

impl ColourInks {
    /// A miniature's panes as the workspace menu paints them: the flow pane
    /// in `line` over its pressed-accent tint, a timeframe pane edged in `time`.
    #[must_use]
    pub fn panes(line: PremultipliedRgba, time: PremultipliedRgba) -> Self {
        let fill = Some(with_opacity(line, crate::contrast::ACTIVE_TINT_ALPHA));
        let (ring, plate, check, flow_edge, time_edge) = (line, line, line, FLOW_EDGE_PT, time);
        Self {
            ring,
            plate,
            fill,
            flow_edge,
            time_edge,
            check,
        }
    }
}

/// `[r, g, b]`'s inks on `ground`, an opaque colour.
#[must_use]
pub fn colour_inks(rgb: [u8; 3], ground: PremultipliedRgba, dark: bool) -> ColourInks {
    let [r, g, b] = rgb;
    let raw = PremultipliedRgba::from_rgb(r, g, b);
    let ring = legible_at(raw, ground, RING_CONTRAST);
    let [start, step, most] = FILL_SHARE;
    let mut share = start;
    let mut fill = None;
    while share <= most + f32::EPSILON {
        let tint = mix(ground, raw, share);
        if contrast_ratio(tint, ground) >= FILL_CONTRAST {
            fill = Some(tint);
            break;
        }
        share += step;
    }
    let flow_edge = if fill.is_some() {
        FLOW_EDGE_PT
    } else {
        FLOW_EDGE_BARE_PT
    };
    let (black, white) = (
        PremultipliedRgba::from_rgb(0, 0, 0),
        PremultipliedRgba::from_rgb(255, 255, 255),
    );
    let check = if contrast_ratio(black, ring) >= contrast_ratio(white, ring) {
        black
    } else {
        white
    };
    ColourInks {
        ring,
        plate: mix(ground, raw, PLATE_SHARE[usize::from(dark)]),
        fill,
        flow_edge,
        time_edge: with_opacity(ring, TIME_EDGE_ALPHA),
        check,
    }
}

/// `share` of the way from `from` to `to`, both opaque.
fn mix(from: PremultipliedRgba, to: PremultipliedRgba, share: f32) -> PremultipliedRgba {
    let channel = |a: u8, b: u8| -> u8 {
        let (a, b) = (f32::from(a), f32::from(b));
        (a + (b - a) * share).round().clamp(0.0, 255.0) as u8
    };
    PremultipliedRgba::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

#[cfg(test)]
#[path = "layout_picker_tests.rs"]
mod tests;
