//! The window theme's tokens a mark paints with, as the chart's own colour
//! type. The window's theme owns every one of them; `app` pins each copy
//! here to its token with a test, so a theme change that forgets this file
//! fails rather than leaving a mark in last season's colour.

use quantick_chart::style::PremultipliedRgba;

use crate::color::rgba_premultiplied;

/// `canvas` — the chart's backdrop; a selection handle's hollow core.
pub const CANVAS: PremultipliedRgba = PremultipliedRgba::from_rgb(0x13, 0x17, 0x22);
/// `text/muted` — secondary labels.
pub const TEXT_MUTED: PremultipliedRgba = PremultipliedRgba::from_rgb(0x96, 0xA0, 0xAF);
/// `buy` — buy flow and the buy mark.
pub const BUY: PremultipliedRgba = PremultipliedRgba::from_rgb(0x26, 0xA6, 0x9A);
/// `sell` — sell flow and the sell mark.
pub const SELL: PremultipliedRgba = PremultipliedRgba::from_rgb(0xEF, 0x53, 0x50);
/// `poc` — the point of control.
pub const POC: PremultipliedRgba = PremultipliedRgba::from_rgb(0xFF, 0xD5, 0x4F);
/// `accent` — selection, and the ring of a selected anchor.
pub const ACCENT: PremultipliedRgba = PremultipliedRgba::from_rgb(0x8A, 0xB4, 0xF8);
/// The dark halo a label is knocked out of, so it reads over any backdrop.
pub const CASING: PremultipliedRgba = rgba_premultiplied(5, 7, 12, 235);
/// How far a casing extends past the ink it carries: one pixel a side.
pub const CASING_EXTRA_PX: f32 = 2.0;
/// `draw/cyan` — a drawing colour no chart element already owns.
pub const DRAW_CYAN: PremultipliedRgba = PremultipliedRgba::from_rgb(0x4D, 0xD0, 0xE1);
