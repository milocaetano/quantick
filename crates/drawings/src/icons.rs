//! The glyphs the tool rail shows for each tool: codepoints of the Phosphor
//! icon font the window ships. A glyph is data here — the window owns the
//! font and paints it — and `app` pins each codepoint to the font crate's
//! own constant with a test.

pub const ANCHOR_SIMPLE: &str = "\u{E5D8}";
pub const ARROWS_HORIZONTAL: &str = "\u{EB06}";
pub const ARROWS_IN_LINE_HORIZONTAL: &str = "\u{E530}";
pub const ARROWS_OUT_LINE_HORIZONTAL: &str = "\u{E534}";
pub const ARROWS_VERTICAL: &str = "\u{EB04}";
pub const ARROW_FAT_DOWN: &str = "\u{E518}";
pub const ARROW_FAT_UP: &str = "\u{E52E}";
pub const ARROW_LINE_LEFT: &str = "\u{E062}";
pub const ARROW_LINE_RIGHT: &str = "\u{E064}";
pub const ARROW_UP_RIGHT: &str = "\u{E092}";
pub const CHART_BAR_HORIZONTAL: &str = "\u{E152}";
pub const CIRCLE: &str = "\u{E18A}";
pub const FLOW_ARROW: &str = "\u{E6EC}";
pub const LINE_SEGMENT: &str = "\u{E6D2}";
pub const LINE_VERTICAL: &str = "\u{ED70}";
pub const MINUS: &str = "\u{E32A}";
pub const PARALLELOGRAM: &str = "\u{ECC6}";
pub const RECTANGLE: &str = "\u{E3F0}";
pub const ROWS: &str = "\u{E5A2}";
pub const ROWS_PLUS_TOP: &str = "\u{E5A0}";
pub const RULER: &str = "\u{E6B8}";
pub const SCRIBBLE: &str = "\u{E806}";
pub const SHAPES: &str = "\u{EC5E}";
pub const TEXT_T: &str = "\u{E48A}";
pub const TREND_UP: &str = "\u{E4AE}";
pub const TRIANGLE: &str = "\u{E4B0}";
