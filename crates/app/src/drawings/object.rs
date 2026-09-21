//! The object model: one drawn mark, its anchors, its identity and the
//! axis it belongs to. The file a feature reads to name a drawing.

use super::{DrawingPayload, DrawingStyle, DrawingTool};

/// One anchor of a drawing.
///
/// `bar` is the pane's own fractional slot — the coordinate the chart draws
/// from, and the one pan, zoom and history prepends keep meaningful.
/// `time_ms` is the same instant said in market time, captured when the
/// anchor was placed. Two panes of one symbol disagree completely about bar
/// indices and agree exactly about market time, which is what lets a drawing
/// cross from the timeframe chart to the tick chart
/// (`docs/ux/drawing-tools-2026-08.md` §D7).
///
/// It is an `Option` because a pane cannot always name the time: an anchor
/// dropped past the newest bar, or on a pane with no bars yet, has no instant
/// behind it. A drawing whose anchors have no time simply cannot be shared —
/// it is never guessed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartPoint {
    pub bar: f32,
    pub price: f64,
    pub time_ms: Option<i64>,
}

impl ChartPoint {
    /// An anchor with no market time behind it. Test-only on purpose: every
    /// production anchor comes from a pane, and a pane always knows whether
    /// the slot under the pointer has an instant behind it. A production
    /// caller reaching for this would be dropping that answer on the floor.
    #[cfg(test)]
    #[must_use]
    pub const fn at(bar: f32, price: f64) -> Self {
        Self {
            bar,
            price,
            time_ms: None,
        }
    }

    #[must_use]
    pub const fn at_time(bar: f32, price: f64, time_ms: Option<i64>) -> Self {
        Self {
            bar,
            price,
            time_ms,
        }
    }
}

/// Which charts a drawing appears on
/// (`docs/ux/drawing-tools-2026-08.md` §D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawingScope {
    /// The pane it was drawn on, and only that one. Today's behaviour, and
    /// the default, so nothing that exists changes.
    #[default]
    ThisChart,
    /// Every pane of the same tab — one symbol, one feed, two bar types. The
    /// anchors are re-expressed through each pane's own market clock.
    ///
    /// Never across tabs: a price level drawn on BTC means nothing on a WIN
    /// chart, and a mark that says otherwise is the data-honesty failure this
    /// repo refuses.
    AllCharts,
}

/// Durable identity of one indicator pane inside a chart pane.
///
/// `kind` is the constructor the indicator was added through (`native.cvd`,
/// `script.zigzag.pine`), `ordinal` distinguishes two instances of the same
/// kind in add order. Deliberately *not* the `SlotId`: slots are a monotonic
/// counter, so removing an indicator and adding it back always yields a new
/// one, and every drawing on that pane would orphan on the most common
/// indicator action there is. Ordinal-within-kind is also why the key can
/// never re-adopt a drawing onto a *different* indicator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneKey {
    /// Shared, not cloned: a key is copied on every band carve, which runs
    /// twice per chart pane per frame.
    pub kind: std::sync::Arc<str>,
    pub ordinal: u8,
}

/// Which value axis of the chart pane an object's anchors live on.
///
/// A *band* is a region of one chart pane owning a value axis: the candles'
/// price band, plus one per expanded indicator pane. A drawing belongs to
/// exactly one band, or — for the time-only tools — to none of them and
/// therefore to all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DrawingBand {
    /// The candles' price axis. Today's behaviour, and the default, so
    /// nothing that exists changes.
    #[default]
    Price,
    /// One indicator pane's own value axis. Painted, hit-tested and dragged
    /// only there: a CVD level drawn through the candles would read as a
    /// price, which is the data-honesty failure this repo refuses.
    Indicator(PaneKey),
    /// No value axis at all: the object marks an instant, so it paints as a
    /// clipped segment in every band while remaining one object.
    AllBands,
}

/// Stable identity of one drawn object, unique within its pane's store for
/// the life of the session.
///
/// The `Vec` index is a *position* — `bring_to_front` and deletes reorder
/// it under anything that remembers it. Everything that must keep pointing
/// at "that drawing" across frames (an armed strategy on a rectangle, a
/// future alert) holds this id instead and resolves it through
/// [`Drawings::index_of`] each time. Ids are never reused; an undone delete
/// restores the object under the id it always had, so a reference held
/// across the undo keeps meaning the same object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DrawingId(pub u64);

/// What [`Drawings::duplicate_selected`] made: the object it copied, and the
/// copy.
///
/// The pair is returned rather than acted on because everything that rides a
/// drawing without living in it — an armed strategy today, whatever docks
/// next — is owned a layer up. `Drawings` stays a store of marks and learns
/// nothing about strategies; the pane that owns both reads this and carries
/// the passengers across.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duplicated {
    pub source: DrawingId,
    pub copy: DrawingId,
}

/// Who placed an object, when it was not the trader's own hand.
///
/// Data honesty, at the level the eye works: an object an assistant put on
/// the chart must never be indistinguishable from one the trader drew. The
/// two strings are the control plane's own vocabulary — the actor kind and
/// the client's name from its handshake — carried as plain text so the
/// drawings layer stays free of control types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingAuthor {
    /// `agent`, `automation` — the wire's actor kind.
    pub actor_kind: String,
    /// The client's own name, as it introduced itself.
    pub client_name: String,
}

impl DrawingAuthor {
    /// One line for a panel: "Claude Code (agent)".
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} ({})", self.client_name, self.actor_kind)
    }
}

#[derive(Debug, Clone)]
pub struct Drawing {
    /// See [`DrawingId`]: identity, where the index is only position.
    pub id: DrawingId,
    /// Set when something other than the trader's hand placed this object.
    /// `None` is the trader's own; anything else is shown as its author's
    /// wherever the object is named, and is the only thing the annotate tier
    /// of the control plane is allowed to remove.
    pub author: Option<DrawingAuthor>,
    /// The trader's own name for the object ("congestão 108k"). `None`
    /// falls back to the derived `"<tool> <n>"` label everywhere a label is
    /// shown; empty strings are normalised to `None` on edit.
    pub name: Option<String>,
    pub tool: DrawingTool,
    pub points: Vec<ChartPoint>,
    /// The value axis the anchors were placed against.
    pub band: DrawingBand,
    pub style: DrawingStyle,
    /// A locked drawing keeps rejecting geometry edits and unforced deletes;
    /// its style stays editable.
    pub locked: bool,
    /// A hidden drawing neither paints nor hit-tests, and stays recoverable.
    pub hidden: bool,
    /// Whether the other panes of this tab show it too.
    pub scope: DrawingScope,
    /// Set when the tab changed the instrument under this mark.
    ///
    /// Time survives a symbol switch and price does not: BTC traded at the
    /// same instants the index did, so the anchors resolve perfectly and the
    /// level lands at a price that means nothing on the chart it is now over.
    /// `off_series` cannot catch it — the series *does* reach those instants.
    ///
    /// Marks are never deleted by a state change, so this is what keeps that
    /// honest: the object stays, and it says it belongs to another market
    /// rather than pretending to be a level on this one.
    pub foreign_market: bool,
    /// Set by [`Drawings::reanchor`] when this pane's series does not reach
    /// the market instant an anchor was placed at — the mark survived a
    /// re-cut, a rewind or a symbol switch, but it is no longer sitting on
    /// the data it was drawn against.
    ///
    /// Derived state, never edited: it is what the honesty fade and the
    /// object manager's off-series badge read, and it is deliberately absent
    /// from [`PartialEq`] so re-anchoring can never look like a user edit to
    /// the undo history.
    pub off_series: bool,
    /// Tool-owned state (Fib levels, a future tool's own properties). The
    /// registry creates it; the shared envelope never learns its fields.
    pub payload: Box<dyn DrawingPayload>,
}

impl Drawing {
    /// The label every list and menu shows: the trader's name when one was
    /// given, the tool name plus the 1-based position otherwise.
    #[must_use]
    pub fn display_label(&self, index: usize) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("{} {}", self.tool.name(), index + 1),
        }
    }

    /// Whether this object *can* be shared: every anchor has to name a market
    /// instant, because that is the only coordinate two panes agree on. An
    /// anchor dropped past the newest bar has none, and no time is invented
    /// to make the checkbox available.
    #[must_use]
    pub fn shareable(&self) -> bool {
        !self.points.is_empty() && self.points.iter().all(|point| point.time_ms.is_some())
    }

    /// Whether the other panes of this tab paint it this frame.
    #[must_use]
    pub fn shared(&self) -> bool {
        self.scope == DrawingScope::AllCharts && !self.hidden && self.shareable()
    }
}

impl PartialEq for Drawing {
    fn eq(&self, other: &Self) -> bool {
        // `id` stays out: identity is not content, and an undo snapshot
        // holding the same objects under the same ids must compare equal to
        // the live store by their *edits* alone. `name` is content — a
        // rename is an edit the undo history records.
        self.name == other.name
            && self.author == other.author
            && self.tool == other.tool
            && self.points == other.points
            && self.band == other.band
            && self.style == other.style
            && self.locked == other.locked
            && self.hidden == other.hidden
            && self.scope == other.scope
            && self.payload.eq_dyn(other.payload.as_ref())
    }
}

/// The look a freshly placed object opens with: the trader's saved default
/// for the tool when there is one, the built-in start otherwise. Both halves
/// travel together because a saved look is one thing to a trader, not a style
/// and a payload.
pub struct NewDrawing {
    pub style: DrawingStyle,
    pub payload: Box<dyn DrawingPayload>,
}

/// What a delete request did. Locked objects demand an explicit `force`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    NeedsConfirmation,
    NothingSelected,
}
