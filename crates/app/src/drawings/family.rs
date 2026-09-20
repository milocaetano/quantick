//! The vocabulary the tool registry speaks: rail families, icons,
//! shortcuts and the two gestures every tool answers.

use eframe::egui;

/// What the trader is holding down while they shape an object.
///
/// Shift is free to take during a chart drag, and that is worth stating
/// because Shift is otherwise the trading modifier: every paper-trading
/// hotkey is Shift **plus a letter** (`docs/ux/paper-trading.md` §9) and the
/// rail's tool keys are letters too, so the modifier held on its own cannot
/// fire an order, flatten a position or arm a tool. Holding it while the hand
/// is on the mouse costs nothing and collides with nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Constrain {
    /// The pointer means exactly where it is.
    #[default]
    Free,
    /// Shift is down: hold the shape level.
    Level,
}

/// Hold `cursor` level with `anchor` — the same height, free to slide along
/// the tape.
///
/// Level, and not "the nearest of 0°/45°/90°", because a chart's two axes are
/// not the same kind of thing: one is a price and the other is time, their
/// ratio changes with every zoom, and a 45° line drawn today is a different
/// line after one scroll. Horizontal is the only angle that survives a zoom,
/// and it is the one that means something — a level *is* a price a trader is
/// holding constant. Vertical is an instant, which is what the vertical-line
/// tool is for.
pub fn level_with(anchor: egui::Pos2, cursor: egui::Pos2) -> egui::Pos2 {
    egui::pos2(cursor.x, anchor.y)
}

/// Where a tool wants its anchor to land on the bar under the pointer.
///
/// Almost every tool answers [`AnchorSnap::Pointer`]: the trader chose the
/// price by pointing at it, and the OHLC magnet is theirs to switch on. A
/// *mark* is the exception — it is a note about a bar, not a level, and it
/// only reads as a mark when it sits clear of the candle it belongs to. It
/// snaps whether or not the magnet is on, because a mark floating inside a
/// candle body is the failure the tool exists to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnchorSnap {
    #[default]
    Pointer,
    BarLow,
    BarHigh,
    /// Glued to the nearest of the bar's OHLC, whatever the distance and
    /// whether or not the magnet is on — and the bar itself clamps to the
    /// tape. For a tool whose anchor *means a bar* (the anchored VWAP): its
    /// price is presentation, and a ball floating in empty space far above
    /// any candle reads as a bug, not as a choice.
    NearestOhlc,
}

/// A tool's arming shortcut, declared by the tool itself so the keyboard
/// map never becomes a central match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolShortcut {
    pub key: egui::Key,
    pub shift: bool,
}

/// A family of related tools sharing one rail slot. Declared by each member,
/// never listed centrally — the rail folds consecutive registry entries with
/// equal `id` into a single split button. `PartialEq` only: the stroke
/// coordinates are `f32`, and nothing orders or hashes families.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolFamily {
    pub id: &'static str,
    /// Header of the family flyout.
    pub title: &'static str,
    /// Slot icon before any member has been armed.
    pub icon: &'static str,
    /// Vector icon for the slot, painted instead of `icon` when non-empty —
    /// same contract as [`DrawingTool::icon_strokes`].
    pub icon_strokes: IconStrokes,
    /// Anchor dots for the slot icon — same contract as
    /// [`DrawingTool::icon_dots`].
    pub icon_dots: IconDots,
    /// Letter set into the slot icon — same contract as
    /// [`DrawingTool::icon_letter`]. A family whose slot borrows one
    /// member's picture declares that member's letter, or the slot would be
    /// the only place that drawing appears unnamed.
    pub icon_letter: Option<IconLetter>,
}

/// A vector icon: polylines in the unit square (x right, y down), scaled to
/// the glyph box at paint time. `&[]` means "use the font glyph". A tool
/// declares one when no Phosphor glyph draws its meaning — a slanted
/// channel, Fibonacci levels — so the icon is registry data, not a special
/// case in the chrome.
pub type IconStrokes = &'static [&'static [(f32, f32)]];

/// The dots of a vector icon: points in the same unit square, painted as
/// small filled circles over the strokes.
///
/// They exist because the icons that needed them are icons *of a gesture*.
/// A Fib is not a stack of lines — it is a leg the trader dragged, with the
/// levels hung off it, and the two ends of that drag are what tells it apart
/// from the extension's three. Every drawing platform draws these tools with
/// their anchors marked for that reason. Registry data like the strokes, so
/// the chrome never learns which tool it is painting.
pub type IconDots = &'static [(f32, f32)];

/// A letter set into a vector icon: the character, where its centre sits in
/// the same unit square as the strokes, and its size as a fraction of the
/// glyph box.
///
/// It exists for the tools that draw the *same picture*. The two Fib tools
/// are one ladder of levels hung off one leg, and only the number of anchors
/// separates them — a difference that is two dots wide on a 32 px rail. `R`
/// and `P` say retracement and projection outright, so a trader picks the
/// one they meant without hovering for the tooltip first. Registry data like
/// the strokes: the chrome paints the letter it is handed and never learns
/// which tool it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconLetter {
    /// The letter itself, painted in the icon's own colour.
    pub text: &'static str,
    /// Centre of the letter in the unit square (x right, y down).
    pub at: (f32, f32),
    /// Font size as a fraction of the glyph box's height.
    pub height: f32,
}
