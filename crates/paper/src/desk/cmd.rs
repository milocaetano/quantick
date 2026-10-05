//! CMD entry: the modifier-and-click order the chart takes without a form.
//!
//! Holding the configured modifier turns the pointer into an order: the
//! preview says what a press would place and at what price, and the press
//! places exactly that. The preview and the press read the same
//! [`CmdPreview`], which is why what the trader was shown is what the venue
//! is told. The values and the rules live here; the host reads the keys and
//! paints the line.

use quantick_engine::Side;
use quantick_sim::{Bracket, EntryKind};
use rust_decimal::Decimal;

use super::geometry::{Bounds, Point, TAG_HEIGHT_PX, clamp_tag_center};
use crate::state::PaperState;

/// Shortest cmd-trading preview line: the pointer near the right edge
/// still gets a line long enough to read as one, by starting left of it.
pub const CMD_LINE_MIN_PX: f32 = 120.0;
/// The preview label's fixed width: paint and press share this exact
/// rect, so the two can never disagree (the overlay-controls rule).
pub const CMD_LABEL_WIDTH_PX: f32 = 116.0;
/// Clear space between the pointer and the label riding beside it. The
/// label must not sit under the crosshair it belongs to — the cursor and
/// the candle beneath it stay readable — while staying close enough to
/// read as one statement with the aim.
pub const CMD_LABEL_CURSOR_GAP_PX: f32 = 14.0;

/// A modifier key the cmd-trading gesture can bind to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdModifier {
    Shift,
    Ctrl,
    Alt,
}
impl CmdModifier {
    /// Every binding the selectors offer.
    pub const ALL: [Self; 3] = [Self::Shift, Self::Ctrl, Self::Alt];

    /// Stable token for the state file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shift => "shift",
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
        }
    }

    /// The inverse of [`Self::as_str`]; unknown tokens are refused.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "shift" => Some(Self::Shift),
            "ctrl" => Some(Self::Ctrl),
            "alt" => Some(Self::Alt),
            _ => None,
        }
    }

    /// Display label for the selector.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Shift => "Shift",
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
        }
    }

    /// Whether this modifier is among the held keys. `Ctrl` reads the
    /// platform command key, so the binding keeps meaning "the control-ish
    /// key" on every OS.
    #[must_use]
    pub fn is_down(self, keys: HeldKeys) -> bool {
        match self {
            Self::Shift => keys.shift,
            Self::Ctrl => keys.command,
            Self::Alt => keys.alt,
        }
    }
}

/// The modifier keys held this frame, as the window reported them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeldKeys {
    pub shift: bool,
    /// The platform command key: Ctrl, or ⌘ on a Mac.
    pub command: bool,
    pub alt: bool,
}

/// Which entry kind the aim places.
///
/// The fill model leaves exactly one *resting* kind valid at any price: a
/// buy above the market can only stop in (a buy limit there would fill at
/// once), and below it can only wait at a limit. So this is not a way to
/// place a stop where a limit belongs — no venue would take it. It is a way
/// to state **which order you came to place**, so the aim shows nothing
/// rather than quietly handing you the other kind when the market is on the
/// wrong side of your level.
///
/// That case is not hypothetical: the mark moves. A level a hand's breadth
/// above the last price is a buy stop now and a buy limit after two ticks
/// up, and under [`Self::Auto`] the same click at the same level places a
/// different order depending on when it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CmdEntryKind {
    /// Whichever kind can rest at the aimed price — the mark decides.
    #[default]
    Auto,
    /// Only a limit. Where a limit cannot rest, the aim stands down.
    Limit,
    /// Only a stop. Where a stop cannot arm, the aim stands down.
    Stop,
}
impl CmdEntryKind {
    /// Every choice the selector offers.
    pub const ALL: [Self; 3] = [Self::Auto, Self::Limit, Self::Stop];

    /// Stable token for the state file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Limit => "limit",
            Self::Stop => "stop",
        }
    }

    /// The inverse of [`Self::as_str`]; unknown tokens are refused.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "auto" => Some(Self::Auto),
            "limit" => Some(Self::Limit),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    /// Display label for the selector.
    ///
    /// The same words as [`Self::as_str`] today, and delegating rather than
    /// repeating them so it stays that way by accident only where it is
    /// harmless: written out twice, renaming the selector's "stop" would
    /// silently change the on-disk token and every remembered choice would
    /// fall back to `Auto` on the next launch. Give this its own `match`
    /// the day the label and the token should differ, which is what
    /// [`CmdModifier`] already does.
    #[must_use]
    pub fn label(self) -> &'static str {
        self.as_str()
    }
}

/// Cmd trading: hold a key over the chart and a dashed line shows exactly
/// where the order will rest, with a label riding beside the cursor; the
/// click places it. Safer than the right-click menu because the price is
/// visible before anything commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmdTradingSettings {
    pub enabled: bool,
    pub buy: CmdModifier,
    pub sell: CmdModifier,
    /// Which entry kind the aim places; see [`CmdEntryKind`].
    pub kind: CmdEntryKind,
}
impl CmdTradingSettings {
    /// The settings the sidecar remembers, with the defaults wherever it
    /// never spoke — or spoke a token this build does not know.
    #[must_use]
    pub fn from_state(state: &PaperState) -> Self {
        let defaults = Self::default();
        Self {
            enabled: state.cmd_trading_enabled.unwrap_or(defaults.enabled),
            buy: state
                .cmd_buy_modifier
                .as_deref()
                .and_then(CmdModifier::parse)
                .unwrap_or(defaults.buy),
            sell: state
                .cmd_sell_modifier
                .as_deref()
                .and_then(CmdModifier::parse)
                .unwrap_or(defaults.sell),
            kind: state
                .cmd_entry_kind
                .as_deref()
                .and_then(CmdEntryKind::parse)
                .unwrap_or(defaults.kind),
        }
    }

    /// The side the held keys aim, or `None` when neither binding is held —
    /// or both are, which is ambiguous and so aims nothing.
    #[must_use]
    pub fn aimed_side(&self, keys: HeldKeys) -> Option<Side> {
        match (self.buy.is_down(keys), self.sell.is_down(keys)) {
            (true, false) => Some(Side::Buy),
            (false, true) => Some(Side::Sell),
            _ => None,
        }
    }
}
impl Default for CmdTradingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            buy: CmdModifier::Shift,
            sell: CmdModifier::Alt,
            // Auto is right at almost every price, and it is what shipped;
            // the choice exists for the trader who wants to be sure.
            kind: CmdEntryKind::Auto,
        }
    }
}

/// The kind the aim places at `price`, or `None` where nothing may rest
/// there.
///
/// [`CmdEntryKind::Auto`] reads the market: above the mark a buy stops in,
/// below it a buy waits at a limit; a sell mirrors. On the mark exactly,
/// nothing can rest — a resting order there would fill on the next print,
/// which is a market order wearing the wrong name.
///
/// A stated kind is honoured only where it is valid. Returning `None`
/// instead of the other kind is the point: the aim's promise is that the
/// label can never advertise an order the press will not make, and a
/// silent substitution would break it in the most expensive way — placing
/// a breakout stop for a trader who came to buy a pullback.
#[must_use]
pub fn resolve_cmd_kind(
    choice: CmdEntryKind,
    side: Side,
    price: Decimal,
    mark: Decimal,
) -> Option<EntryKind> {
    let available = match (price > mark, price < mark, side) {
        (true, _, Side::Buy) | (_, true, Side::Sell) => EntryKind::Stop,
        (true, _, Side::Sell) | (_, true, Side::Buy) => EntryKind::Limit,
        _ => return None,
    };
    match choice {
        CmdEntryKind::Auto => Some(available),
        CmdEntryKind::Limit => (available == EntryKind::Limit).then_some(EntryKind::Limit),
        CmdEntryKind::Stop => (available == EntryKind::Stop).then_some(EntryKind::Stop),
    }
}

/// The frame's cmd-trading preview: computed by the desk's input pass,
/// painted by the host, clicked through the same geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CmdPreview {
    pub side: Side,
    pub kind: EntryKind,
    /// Snapped price, for the label and the gutter chip.
    pub price: Decimal,
    /// Raw pointer price — what a click hands to `place_resting`, which
    /// snaps for itself (the armed-click path's contract).
    pub raw_price: f64,
    /// The aiming pointer, both coordinates: y is the price, x is where
    /// the label rides. Stored whole so paint and press lay out from the
    /// very same position.
    pub pointer: Point,
    /// This aim was invented by the capture hook, not by a held key. It
    /// paints, so a screenshot has something to show, and it never places:
    /// a run with nobody at the keyboard is holding no modifier, and a
    /// stray click during one must not write orders into a journal.
    pub forced: bool,
    /// The protection this order would carry: a strategy's ladder, the
    /// ruler's symmetric pair, or the ticket's typed offsets. Empty when the
    /// order would rest bare.
    ///
    /// One value, computed once, painted by the projection and placed by the
    /// click - a preview that promised one bracket while the order took
    /// another is the worst bug this surface can have.
    pub bracket: Bracket,
    /// How many ticks the ruler stands at; zero means it is not in use.
    pub ruler_ticks: u32,
}

/// The `QUANTICK_CMD_PREVIEW` hook, parsed: which side to aim and, when
/// stated, where along the band to park the virtual pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CmdPreviewForce {
    pub side: Side,
    /// 0.0 is the band's left edge, 1.0 its right; `None` leaves the
    /// pointer mid-band, which is what the hook meant before the label
    /// followed it.
    pub x_fraction: Option<f32>,
}

impl CmdPreviewForce {
    /// `buy`, `sell`, `buy@0.15`. An unparseable fraction degrades to the
    /// mid-band park rather than killing the whole preview — a capture run
    /// that paints nothing is the hardest failure to read.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let (side, fraction) = match value.split_once('@') {
            Some((side, fraction)) => (side, Some(fraction)),
            None => (value, None),
        };
        let side = match side.trim().to_ascii_lowercase().as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return None,
        };
        Some(Self {
            side,
            x_fraction: fraction
                .and_then(|text| text.trim().parse::<f32>().ok())
                // `"NaN"` and `"inf"` parse, and `clamp` passes NaN
                // straight through — which would poison the pointer's x
                // and paint nothing at all, the one outcome this fallback
                // exists to rule out.
                .filter(|fraction: &f32| fraction.is_finite())
                .map(|fraction| fraction.clamp(0.0, 1.0)),
        })
    }
}

/// The cmd preview's geometry from the interactive band and the pointer:
/// the dashed line under the cursor running out to the right edge, and the
/// clickable label riding beside the cursor. One function for paint and
/// press alike, so a painted label and its hit-test can never disagree
/// (the overlay-controls rule).
///
/// The label follows the pointer rather than parking against the right
/// edge: aiming at a price on the left of the plot used to mean crossing
/// the whole chart to click the thing you were already pointing at. It
/// sits *beside* the cursor, never under it, so the crosshair and the
/// candle it rests on stay readable. Left is the preferred side (the line
/// and the price chip run off to the right, so the label completes the
/// sentence from its start); it flips right when the left edge leaves no
/// room, and in a band too narrow for either — well under any window this
/// app opens — it parks against the closer edge.
#[must_use]
pub fn cmd_preview_layout(band: Bounds, axis_x: f32, pointer: Point) -> (Point, Point, Bounds) {
    let need = CMD_LABEL_CURSOR_GAP_PX + CMD_LABEL_WIDTH_PX;
    let left = if pointer.x - band.left() >= need {
        pointer.x - need
    } else if band.right() - pointer.x >= need {
        pointer.x + CMD_LABEL_CURSOR_GAP_PX
    } else {
        // Narrower than the label and its gap on either side: the two
        // cannot both hold, so it parks at the left edge and the cursor
        // may cross it. Reaching this needs a band under 260 px — no
        // window this app opens is that small.
        band.left()
    };
    let center_y = clamp_tag_center(pointer.y, band.top(), band.bottom());
    let half = TAG_HEIGHT_PX / 2.0;
    let label = Bounds::from_min_max(
        Point::new(left, center_y - half),
        Point::new(left + CMD_LABEL_WIDTH_PX, center_y + half),
    );
    // The line starts under the cursor and reaches the axis, which is what
    // ties the label beside the hand to the price on the gutter. Close to
    // that edge it starts further left instead, so there is always a line
    // to read.
    let start = Point::new(
        pointer
            .x
            .min(band.right() - CMD_LINE_MIN_PX)
            .max(band.left()),
        pointer.y,
    );
    // The *band* stops at the live lane's divider, because that is where a
    // click can still be pressed; the line does not, because it is a read
    // and not a control. Stopping it there left the tape lane — the widest
    // thing on the chart — as a blank gap between the aim and its own price
    // on the axis, so the one place a trader watches the order arrive was
    // the one place the order was invisible. Every other level here already
    // spans to the axis (`level_line`); this now says the same.
    (
        start,
        Point::new(axis_x.max(band.right()), pointer.y),
        label,
    )
}
