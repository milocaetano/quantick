//! The window theme's tokens a mark paints with, as the chart's own colour
//! type. The window's theme owns every one of them; `app` pins each copy
//! here to its token with a test, so a theme change that forgets this file
//! fails rather than leaving a mark in last season's colour.

use quantick_chart::style::PremultipliedRgba;

/// `canvas` — the chart's backdrop; a selection handle's hollow core.
pub const CANVAS: PremultipliedRgba = quantick_appearance::palette::CANVAS;
/// `text/muted` — secondary labels.
pub const TEXT_MUTED: PremultipliedRgba = quantick_appearance::palette::TEXT_MUTED;
/// `buy` — buy flow and the buy mark.
pub const BUY: PremultipliedRgba = quantick_appearance::palette::BUY;
/// `sell` — sell flow and the sell mark.
pub const SELL: PremultipliedRgba = quantick_appearance::palette::SELL;
/// `poc` — the point of control.
pub const POC: PremultipliedRgba = quantick_appearance::palette::POC;
/// `accent` — selection, and the ring of a selected anchor.
pub const ACCENT: PremultipliedRgba = quantick_appearance::palette::ACCENT;
/// The dark halo a label is knocked out of, so it reads over any backdrop.
pub const CASING: PremultipliedRgba = quantick_appearance::palette::CASING;
/// How far a casing extends past the ink it carries: one pixel a side.
pub const CASING_EXTRA_PX: f32 = 2.0;
/// `draw/cyan` — a drawing colour no chart element already owns.
pub const DRAW_CYAN: PremultipliedRgba = quantick_appearance::palette::DRAW_CYAN;
