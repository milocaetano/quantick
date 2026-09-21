//! The named colours themselves. Every fixed surface of the chrome reads
//! one from here rather than inventing its own grey.

use eframe::egui::Color32;

/// `bg/canvas` — default chart canvas (user-configurable in the appearance
/// dialog; [`crate::style::CanvasStyle`] holds the editable copy, and the
/// parity test below keeps the two in agreement — the reason this token
/// exists without a chrome consumer).
#[allow(dead_code)]
pub const CANVAS: Color32 = Color32::from_rgb(0x13, 0x17, 0x22);

/// `bg/chrome` — menu bar, toolbar, tool rail, dock and status bar.
pub const CHROME: Color32 = Color32::from_rgb(0x17, 0x1B, 0x26);

/// `bg/inset` — sub-panes and wells sunk into the chrome.
pub const INSET: Color32 = Color32::from_rgb(0x10, 0x14, 0x1D);

/// `bg/control` — buttons, combos and inputs (also the default grid colour).
pub const CONTROL: Color32 = Color32::from_rgb(0x23, 0x29, 0x36);

/// `border` — panel and control borders.
pub const BORDER: Color32 = Color32::from_rgb(0x2E, 0x36, 0x48);

/// `text/primary` — labels and values.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xD2, 0xDA, 0xE2);

/// `text/muted` — secondary labels.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x96, 0xA0, 0xAF);

/// `text/faint` — hints, disabled text and the crosshair.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6E, 0x78, 0x87);

/// `buy` — bull candles, buy flow and the healthy/live status dot.
pub const BUY: Color32 = Color32::from_rgb(0x26, 0xA6, 0x9A);

/// `sell` — bear candles, sell flow, and the chrome's SELL affordances
/// (paper trading). Candles read their editable copy from [`crate::style`];
/// the parity test below pins the two together.
pub const SELL: Color32 = Color32::from_rgb(0xEF, 0x53, 0x50);

/// `poc` — the footprint's point-of-control line. Yellow, the user's call
/// after the reference charts (exocharts red would collide with the sell
/// side here). Close in hue to [`AMBER`] but a different surface: AMBER is
/// provenance *text* in the chrome, this is a line inside a candle — the
/// two never sit side by side.
pub const POC: Color32 = Color32::from_rgb(0xFF, 0xD5, 0x4F);

/// `accent/overlay` — selection accent and the future first indicator plot.
pub const ACCENT: Color32 = Color32::from_rgb(0x8A, 0xB4, 0xF8);

/// `honest/amber` — not-live provenance only: replay, backfill, inferred data.
pub const AMBER: Color32 = Color32::from_rgb(0xF0, 0xB9, 0x0B);

/// `seam/line` — the venue/prints boundary: white, and mostly transparent.
///
/// The one provenance mark that is *always* on screen, on every time-cutting
/// pane, for the whole session. [`AMBER`] earns its loudness where it says
/// "this is not live" about something the trader might act on; here it was
/// shouting a fact that never changes and never needs acting on, and it sat
/// permanently across a chart read for shape. White at low alpha keeps the
/// boundary findable when looked for and out of the way when not — it borrows
/// no hue that already means something on this chart, which is exactly why a
/// merely *dimmer* amber would not do: dimming a reserved colour keeps the
/// promise it makes while making the promise harder to read.
pub const SEAM_LINE: Color32 = Color32::from_rgba_premultiplied(0x3C, 0x3C, 0x3C, 0x3C);

/// `seam/label` — the "venue" caption beside [`SEAM_LINE`], at the same
/// weight as the line it names so the pair reads as one quiet mark rather
/// than a faint rule under a legible word.
pub const SEAM_LABEL: Color32 = Color32::from_rgba_premultiplied(0x50, 0x50, 0x50, 0x50);

/// `warn` — threshold breaches and errors.
pub const WARN: Color32 = Color32::from_rgb(0xFF, 0x63, 0x47);

/// The recording red: the REC control while the venue's deal counter is
/// being written down. Distinct from [`SELL`] on purpose — a sell candle
/// and a recorder are not the same kind of fact — and from [`WARN`], because
/// recording is the healthy state.
pub const REC: Color32 = Color32::from_rgb(0xFF, 0x4D, 0x4D);

/// `gap/line` — the dashed mark where the tape has a hole a reconnect left.
///
/// Louder than [`SEAM_LINE`] and quieter than [`AMBER`], because it sits
/// between the two things they mark. The venue seam is provenance a trader
/// reads once; a gap is *missing data* under the bars beside it, and the
/// honesty rule says it is labelled at the point of reading. It is still not
/// an alarm: nothing is wrong with the chart, there is simply a stretch of the
/// market it never saw.
pub const GAP_LINE: Color32 = Color32::from_rgba_premultiplied(0x78, 0x5C, 0x0A, 0x96);

/// `gap/label` — the caption beside [`GAP_LINE`], naming how long the silence
/// was. Readable without competing with price.
pub const GAP_LABEL: Color32 = Color32::from_rgb(0x9A, 0x82, 0x3A);

/// `tag/bg` — tooltips and the axis price tag.
pub const TAG_BG: Color32 = Color32::from_rgb(0x37, 0x3F, 0x50);

/// `text/support` — small explanatory lines that carry real information
/// (the inspector's locked/hidden notes). [`TEXT_FAINT`] stays reserved for
/// decoration and disabled states, where 4.5:1 contrast is not required.
pub const TEXT_SUPPORT: Color32 = Color32::from_rgb(0x86, 0x92, 0xA4);

/// `chip/ink` — dark text inside a solid semantic-color chip: the last-price
/// chip's ink, shared by every surface that speaks the chip language (trade
/// buttons, the position HUD's side tag, the jump-to-live chip).
pub const CHIP_INK: Color32 = Color32::from_rgb(0x0E, 0x12, 0x1A);

/// `casing` — the dark base under ink that has to survive whatever is behind
/// it: the liquidity map, a candle body at any of the four appearance
/// presets, or a canvas the trader switched off.
///
/// Deliberately *darker* than [`CANVAS`], because the heat ramp's floor is
/// black: a casing that matches that floor stops separating exactly where
/// separation is needed. Composed over the map's brightest band it still
/// lands near canvas-dark, which is the whole point — it makes the floor a
/// constant instead of a function of whatever happens to be underneath.
///
/// Born in the fixed-range profile, which needed a readable histogram over
/// the depth map; promoted here the day the footprint's ladder needed the
/// same guarantee. A colour copied by hand goes stale the day the theme
/// moves, with no test to notice.
pub const CASING: Color32 = Color32::from_rgba_premultiplied(5, 7, 12, 235);

/// How far a casing extends past the ink it carries: one pixel a side.
pub const CASING_EXTRA_PX: f32 = 2.0;

/// `draw/violet` and `draw/cyan` — the two drawing colours no chart element
/// already owns. They exist because the drawing palette needs hues that do
/// not collide with meaning: green and red are the candles, blue is
/// [`ACCENT`], yellow is [`POC`]. Without these two, every drawing colour
/// borrows something that already says something else.
pub const DRAW_VIOLET: Color32 = Color32::from_rgb(0xC5, 0x8A, 0xF9);

/// See [`DRAW_VIOLET`].
pub const DRAW_CYAN: Color32 = Color32::from_rgb(0x4D, 0xD0, 0xE1);

/// The drawing palette offered as one-click swatches on the context bar.
///
/// Eight, because that is what fits one row without the row becoming a grid
/// the trader has to read. Six are tokens that already mean something on
/// this chart, so a colour choice speaks the language the rest of the UI
/// speaks; two are the hues nothing else owns.
///
/// [`AMBER`] is deliberately absent and must stay absent: it is reserved for
/// provenance honesty, and a trader painting a line amber by taste would be
/// borrowing the one colour that promises "this data is not live".
/// Grep-guarded in the context bar, the way the rail guards itself.
pub const DRAWING_SWATCHES: [Color32; 8] = [
    ACCENT,
    TEXT_PRIMARY,
    TEXT_MUTED,
    BUY,
    SELL,
    POC,
    DRAW_VIOLET,
    DRAW_CYAN,
];

/// `shadow/float` — the drop shadow of a surface floating free over the
/// canvas. Docked chrome never uses it: a panel glued to an edge separates
/// itself with its hairline border. A floating strip has no anchored side,
/// and [`CHROME`] over [`CANVAS`] is too close in value to read alone.
pub const FLOAT_SHADOW: Color32 = Color32::from_black_alpha(96);
