//! The selected drawing's context bar: one floating row of icons that opens
//! where the object is — until a hand puts it somewhere else, after which
//! that point is where it opens for every object until it is given back — so
//! the actions a trader takes with their eye on the chart never cost a trip
//! to a panel.
//!
//! It is the second host of [`ActionBarIntent`] — the first being the
//! inspector's textual bar — so lock and delete keep exactly one set of
//! rules. What differs is the *form*, and that difference is the contract
//! this module adds:
//!
//! > A destructive action may be a bare glyph when, and only when, it is
//! > (a) undoable in one gesture, (b) announced by name when it happens, and
//! > (c) confirmed when the object is protected. Outside those three
//! > conditions it is textual, and [`super::action_bar`] stays the host that
//! > provides it.
//!
//! The bar carries no overflow menu. Its overflow *is* the settings gear:
//! anything that does not earn a slot here is already reachable, in words,
//! one click away. That is also the ceiling — a bar that needs a "more"
//! button has stopped being a bar.

use crate::drawings::StyleColor32;
use eframe::egui;
use egui_phosphor::regular as icons;
pub use quantick_drawings::context_bar::{Capabilities, Slot, slots};

use super::action_bar::ActionBarIntent;
use super::{DrawingStyle, GlyphSize, MAX_DRAWING_FILL_ALPHA};
use crate::theme;
use crate::widgets::{IconButton, TOOLRAIL_ICON};

/// Height of the bar: the rail's 32 px touch target with 4 px of air.
pub const BAR_HEIGHT_PX: f32 = 2.0 * BAR_PAD_Y_PX + TOOLRAIL_ICON.hit;
const BAR_PAD_X_PX: f32 = 6.0;
const BAR_PAD_Y_PX: f32 = 4.0;
const DUPLICATE_HOVER_TEXT: &str = "Duplicate (Ctrl+D) · Copy and paste (Ctrl+C, Ctrl+V)";
/// Gap between two neighbouring slots.
const ITEM_GAP_PX: f32 = 4.0;
/// Width of the hairline that isolates a group.
const SEPARATOR_WIDTH_PX: f32 = 1.0;
/// How far the hairline stops short of the bar's edges.
const SEPARATOR_INSET_Y_PX: f32 = 8.0;
/// Width of the drag grip.
const GRIP_WIDTH_PX: f32 = 18.0;
const GRIP_GLYPH_PX: f32 = 14.0;
/// Corner radius — one step rounder than the rail's flyout, because this
/// surface floats free instead of hanging off an edge.
const CORNER_RADIUS_PX: f32 = 8.0;
/// Gap between the object's bounding box and the bar.
pub const OBJECT_GAP_PX: f32 = 6.0;
/// Gap between the bar and a popover hanging off one of its slots.
const POPOVER_GAP_PX: f32 = 6.0;
/// Side of the colour swatch painted inside its 32 px slot.
const SWATCH_SIDE_PX: f32 = 20.0;
/// Side of one swatch inside the colour popover.
const PALETTE_SWATCH_PX: f32 = 22.0;
/// Gap between a palette swatch and the ring that marks it selected.
const SELECTED_RING_GAP_PX: f32 = 1.5;
/// Length of the stroke preview painted inside the width slot.
const WIDTH_PREVIEW_LEN_PX: f32 = 18.0;
/// Width of one row inside the width and size popovers.
///
/// Stated, not inherited. Those two popovers are a preview and a right-aligned
/// label — neither has an intrinsic width — so a row sized from
/// `ui.available_width()` reports back whatever the Area was laid out at and
/// the two agree on it forever: an Area egui has not seen starts at its
/// 600 px default, the rows fill it, and the measurement confirms it.
const POPOVER_ROW_WIDTH_PX: f32 = 132.0;
/// Height of one row inside the width and size popovers.
const POPOVER_ROW_HEIGHT_PX: f32 = 26.0;

/// Opacity while the pointer is elsewhere. It never reaches zero: the object
/// is selected, and this bar is the proof of that on screen.
const IDLE_OPACITY: f32 = 0.55;
/// How close the pointer must come before the bar wakes to full opacity.
const WAKE_RADIUS_PX: f32 = 24.0;
/// Seconds of the opacity cross-fade, both directions.
const FADE_SECONDS: f32 = 0.12;

/// How long a gesture with no release event — wheel zoom, window resize —
/// keeps the bar suppressed after its last event.
pub const TRANSIENT_SUPPRESS_MS: u64 = 200;

/// The stroke widths the bar offers. Four discrete steps, not the
/// inspector's continuous slider: nobody wants to dial 0.1 px during the
/// session, and a slider on a 32 px slot is a slider nobody can aim at.
pub const WIDTH_STEPS_PX: [f32; 4] = [1.0, 2.0, 3.0, 4.0];

/// Horizontal room a cell takes.
#[must_use]
pub fn slot_width(slot: Slot) -> f32 {
    match slot {
        Slot::Grip => GRIP_WIDTH_PX,
        Slot::Separator => SEPARATOR_WIDTH_PX,
        _ => TOOLRAIL_ICON.hit,
    }
}

/// Size of a bar made of these cells.
#[must_use]
pub fn bar_size(slots: &[Slot]) -> egui::Vec2 {
    let content: f32 = slots.iter().map(|slot| slot_width(*slot)).sum();
    let gaps = ITEM_GAP_PX * (slots.len().saturating_sub(1)) as f32;
    egui::vec2(2.0f32.mul_add(BAR_PAD_X_PX, content + gaps), BAR_HEIGHT_PX)
}

/// Size of a contextual bar containing one ordinary icon action.
///
/// The temporary-range surface uses the same frame and touch target as this
/// bar, so the two pieces of chart chrome cannot drift apart.
#[must_use]
pub(crate) fn single_action_bar_size() -> egui::Vec2 {
    egui::vec2(
        2.0f32.mul_add(BAR_PAD_X_PX, TOOLRAIL_ICON.hit),
        BAR_HEIGHT_PX,
    )
}

/// The shared floating context-bar frame.
pub(crate) fn floating_frame(palette: &crate::theme::Palette) -> egui::Frame {
    egui::Frame::none()
        .fill(palette.chrome())
        .stroke(egui::Stroke::new(1.0_f32, palette.border()))
        .rounding(egui::Rounding::same(CORNER_RADIUS_PX))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 2.0),
            blur: 12.0,
            spread: 0.0,
            color: palette.float_shadow(),
        })
        .inner_margin(egui::Margin::symmetric(BAR_PAD_X_PX, BAR_PAD_Y_PX))
}

/// Where the bar opens for an object whose screen bounding box is `bbox`.
///
/// Above the object and aligned to the *left* of its box — not centred.
/// Centring looks tidier and is wrong: the bar's width changes with the
/// object's capabilities, so a centred bar puts the colour swatch in a
/// different place for every tool. Left-aligned, slot one is always in the
/// same place relative to the thing it edits.
///
/// `right_limit` is where the live lane starts, when the pane has one. The
/// bar never covers the live region: that is where the price the trader is
/// reading is being formed.
#[must_use]
pub fn place(
    chart: egui::Rect,
    right_limit: f32,
    bbox: egui::Rect,
    size: egui::Vec2,
) -> egui::Pos2 {
    let above = bbox.top() - OBJECT_GAP_PX - size.y;
    let below = bbox.bottom() + OBJECT_GAP_PX;
    if above < chart.top() && below + size.y > chart.bottom() {
        // Taller than the pane leaves no room above or below — a volume
        // profile spanning the price axis, a channel across the whole view.
        // The top edge of a full-height object *is* the object, and the bar
        // must never sit on the figure it edits.
        //
        // Beside it instead, on whichever side has room. Vertically it keeps
        // to the object's top, so the controls stay where the eye left them.
        let gap = OBJECT_GAP_PX;
        let y = (bbox.top()).clamp(chart.top(), (chart.bottom() - size.y).max(chart.top()));
        let right_of = bbox.right() + gap;
        let left_of = bbox.left() - gap - size.x;
        if right_of + size.x <= right_limit.min(chart.right()) {
            return egui::pos2(right_of, y);
        }
        if left_of >= chart.left() {
            return egui::pos2(left_of, y);
        }
        // The object leaves no strip on either side either. Nothing can be
        // placed clear of it; the top edge is the least-bad answer, and this
        // is now the only case that reaches it.
        return egui::pos2(
            bbox.left()
                .clamp(chart.left(), (chart.right() - size.x).max(chart.left())),
            chart.top() + gap,
        );
    }
    let y = if above >= chart.top() { above } else { below };
    // Prefer to stay clear of the live lane; if the bar is wider than the
    // history area itself, the chart's own edge wins over the preference.
    let right = right_limit.min(chart.right()).max(chart.left() + size.x);
    let max_x = (right - size.x).max(chart.left());
    let x = bbox.left().clamp(chart.left(), max_x);
    let max_y = (chart.bottom() - size.y).max(chart.top());
    egui::pos2(x, y.clamp(chart.top(), max_y))
}

/// Which popover the bar has open. Kept here rather than in egui memory so
/// the state machine below can close it as one step of a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Popover {
    #[default]
    None,
    Color,
    Width,
    GlyphSize,
}

/// Why the bar is currently not on screen.
///
/// Suppression exists to protect the bar from *gestures*, not from the
/// market moving. A drawing that drifts with the auto-scroll simply carries
/// the bar along, smoothly; suppressing that would make the bar blink all
/// session on a live tape. What must never happen is the bar measuring a
/// world the same gesture is still moving.
#[derive(Debug, Default)]
pub struct Suppression {
    /// A gesture with a release event — dragging the object, a handle, the
    /// canvas, a pane divider — is holding the bar down.
    gesture: bool,
    /// A gesture with no release — wheel zoom, window resize — expires.
    until_ms: Option<u64>,
}

/// The bar's own state: everything that must outlive a frame.
#[derive(Debug, Default)]
pub struct ContextBar {
    open: Popover,
    /// Where the trader dragged the bar. Absolute screen position, and it
    /// outlives the selection it was chosen under.
    ///
    /// Nobody drags this bar to a corner *for one object*: they drag it off
    /// the price action they are reading, and the next object is on the same
    /// chart with the same thing behind it.
    ///
    /// So the hand outranks the placement here exactly as it does for the
    /// properties popup, and by the same argument: a trader who chose where
    /// this goes has already answered the question the rule exists to guess.
    /// The way back is the grip's double-click, named in its own tooltip and
    /// aimed at the bar and nothing else — see [`Self::clear_manual`].
    ///
    /// The host clamps it into the chart before drawing — see
    /// `QuantickApp::draw_drawing_context_bar` — so a position parked under a
    /// wide layout can never leave the view under a narrow one.
    ///
    /// Session-only, and deliberately: the properties popup records its parked
    /// point in the workspace (`SavedChrome::inspector_position`) because it is
    /// a window of settings, while a *context* bar reappearing in a corner at
    /// the next launch, far from any object and with nothing to explain it,
    /// is a different decision and not one this change makes. The argument
    /// below is about which of hand and rule wins within a session.
    manual: Option<egui::Pos2>,
    suppression: Suppression,
    last_selection: Option<usize>,
    /// Screen rect of the slot each popover hangs from.
    anchor: Option<egui::Rect>,
    /// Last window size seen, so a resize can suppress like a zoom does.
    last_screen: Option<egui::Rect>,
    /// The delete tooltip, which names the object. Cached rather than
    /// formatted per frame: the bar is up for as long as something is
    /// selected, and this sentence only changes when the tool does.
    delete_hover: String,
    delete_hover_for: &'static str,
    /// Last resolved rect, for the wake radius and for tests.
    rect: Option<egui::Rect>,
    /// The open popover's Area — see [`Self::owns_press`].
    popover_layer: Option<egui::Id>,
    /// What [`Self::owns_press`] decided, keyed by the press's start time.
    press_owned: Option<(f64, bool)>,
    /// The colour popover shows the full picker inline: egui's own picker
    /// popup is a second Area, and a press inside it closed this popover.
    custom_open: bool,
    #[cfg(test)]
    probe: Probe,
}

/// Where the bar's controls landed, so a test can press the real buttons
/// rather than trust arithmetic about slot order.
#[cfg(test)]
#[derive(Debug, Default)]
struct Probe {
    gear: Option<egui::Rect>,
    draw_again: Option<egui::Rect>,
    sharing: Option<egui::Rect>,
    magnet: Option<egui::Rect>,
    color: Option<egui::Rect>,
    colors: ColorProbe,
}

/// Where the colour popover's controls landed.
#[cfg(test)]
#[derive(Debug, Default)]
struct ColorProbe {
    swatches: Vec<egui::Rect>,
    fill: Option<egui::Rect>,
    custom: Option<egui::Rect>,
    picker: Option<egui::Rect>,
}

impl ContextBar {
    /// A gesture that has a release event starts here.
    pub fn suppress_gesture(&mut self) {
        self.suppression.gesture = true;
        self.open = Popover::None;
    }

    /// …and ends here. The bar comes back on the next frame, measuring the
    /// world *after* the gesture settled.
    pub fn release_gesture(&mut self) {
        self.suppression.gesture = false;
    }

    /// Watch the window for a resize, which is a gesture with no release of
    /// its own — the pointer is on a border the OS owns.
    pub fn note_screen(&mut self, screen: egui::Rect, now_ms: u64) {
        if self.last_screen.is_some_and(|last| last != screen) {
            self.suppress_transient(now_ms);
        }
        self.last_screen = Some(screen);
    }

    /// A gesture with no release — wheel zoom, resize — renews a deadline.
    pub fn suppress_transient(&mut self, now_ms: u64) {
        self.suppression.until_ms = Some(now_ms + TRANSIENT_SUPPRESS_MS);
        self.open = Popover::None;
    }

    #[must_use]
    pub fn suppressed(&self, now_ms: u64) -> bool {
        self.suppression.gesture
            || self
                .suppression
                .until_ms
                .is_some_and(|deadline| now_ms < deadline)
    }

    /// Called every frame with the current selection. Answers whether the
    /// selection changed.
    ///
    /// What a new object drops is what belongs to the *old* one: an open
    /// popover (a colour palette hanging off a bar that is about to move is
    /// pointing at nothing) and the last rect (the wake radius must not
    /// measure against geometry from the previous object). A hand-placed
    /// position is not one of those — see [`Self::manual`].
    pub fn note_selection(&mut self, selection: Option<usize>) -> bool {
        if self.last_selection == selection {
            return false;
        }
        self.last_selection = selection;
        self.open = Popover::None;
        self.rect = None;
        true
    }

    #[must_use]
    pub fn manual_position(&self) -> Option<egui::Pos2> {
        self.manual
    }

    /// The trader dragged the bar by the grip: it stops following the object
    /// and stays where they put it, across the selections that come after —
    /// see [`Self::manual`].
    ///
    /// It still hides through a pan, zoom or canvas drag like a following bar:
    /// suppression is what lets a drag reach the canvas underneath it.
    pub fn set_manual(&mut self, position: egui::Pos2) {
        self.manual = Some(position);
    }

    /// Give the placement back to the rule, reporting whether there was
    /// anything to give.
    ///
    /// The named door out of the parked state, for a hand and for an operator
    /// without one: the grip's double-click *calls* this rather than clearing
    /// the field itself, so there is one way back and not two.
    ///
    /// Not Escape: a key a trader presses many times an hour would quietly
    /// undo where they put the bar — see the escape stack in
    /// `QuantickApp::drawing_keys`.
    pub fn clear_manual(&mut self) -> bool {
        self.manual.take().is_some()
    }

    #[must_use]
    pub fn last_rect(&self) -> Option<egui::Rect> {
        self.rect
    }

    /// Whether the press (origin, start time) began on the bar or its open
    /// popover's layer — a drag there is not a chart drag. Decided once per
    /// press and held to the release: a grip drag moves the bar, and egui
    /// calls the release a drag. Keyed by start time, so a release this never
    /// saw cannot hand its answer to the next press.
    pub fn owns_press(
        &mut self,
        press: Option<(egui::Pos2, f64)>,
        current: Option<egui::Rect>,
        layer: Option<egui::LayerId>,
    ) -> bool {
        let Some((origin, started)) = press else {
            return self.press_owned.take().is_some_and(|(_, owned)| owned);
        };
        if let Some((at, owned)) = self.press_owned
            && at == started
        {
            return owned;
        }
        let popover = self.popover_layer.filter(|_| self.open != Popover::None);
        let owned = layer.is_some_and(|layer| Some(layer.id) == popover)
            || current.is_some_and(|rect| rect.contains(origin));
        self.press_owned = Some((started, owned));
        owned
    }

    fn toggle(&mut self, popover: Popover, anchor: egui::Rect) {
        self.open = if self.open == popover {
            Popover::None
        } else {
            popover
        };
        self.custom_open = false;
        self.anchor = Some(anchor);
    }
}

/// What the bar asked for this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ContextBarIntent {
    /// Lock and delete, in the one shape every host reports them in.
    pub actions: ActionBarIntent,
    pub toggle_hidden: bool,
    pub toggle_shared: bool,
    pub toggle_magnet: bool,
    pub duplicate: bool,
    /// Arm this object's tool again: the pencil after a finished stroke.
    pub draw_again: bool,
    pub open_settings: bool,
    /// The second step of a protected delete was taken.
    pub force_delete: bool,
    /// …or abandoned.
    pub cancel_delete: bool,
    /// The style or glyph size changed this frame — the caller folds it into
    /// its undo coalescing, exactly as it does for the inspector.
    pub edited: bool,
    /// How far the grip was dragged this frame.
    pub drag_delta: egui::Vec2,
    /// The grip was double-clicked: back to automatic placement.
    pub reset_position: bool,
}

/// Everything the bar needs to know about the object it is editing.
pub struct BarObject<'a> {
    pub style: &'a mut DrawingStyle,
    /// `Some` for a tool drawn as a glyph; the bar offers type size instead
    /// of stroke width, and writes the chosen size back here.
    pub glyph_size: Option<GlyphSize>,
    pub locked: bool,
    /// This object's own magnet; `None` for a tool the magnet does not move.
    pub magnet: Option<bool>,
    pub hidden: bool,
    pub shared: bool,
    pub shareable: bool,
    pub supports_fill: bool,
    /// Whether the gear earns a slot — false while the inspector is pinned.
    pub settings_available: bool,
    /// A delete was asked for on a protected object and is waiting for the
    /// second step. This is the one state where the bar speaks in words: a
    /// locked object is one the trader declared important, and condition (c)
    /// of this module's contract is what buys the bare glyph everywhere else.
    pub confirming_delete: bool,
    /// Who placed this object, when it was not the trader — "Claude Code
    /// (agent)". `None` is the trader's own hand and shows nothing: the
    /// chip exists to break the tie, not to decorate every object.
    pub author: Option<&'a str>,
    /// Name of the tool, for the delete tooltip: a trader who cannot tell
    /// *what* was deleted cannot use undo to get it back. `'static` because
    /// the registry's names are, which is what lets the tooltip be cached
    /// instead of formatted every frame.
    pub tool_name: &'static str,
    /// The tool that drew this object, offered again on the bar as its own
    /// rail glyph and hover text; `None` for a tool drawn once at a time.
    pub draw_again: Option<DrawAgain>,
}

/// The rail's own face for the tool the bar offers again, so the bar and
/// the rail cannot name one tool two ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawAgain {
    pub glyph: &'static str,
    pub hover: &'static str,
}

/// Which controls this object earns a slot for.
///
/// One derivation, not two. The host has to know the bar's width before it can
/// repair a parked position into the pane — and a second `Capabilities` literal
/// written beside this one would agree today and drift on the PR that adds a
/// capability to one of them, leaving the bar drawn wider than the rectangle it
/// was repaired into, out over the live lane, with nothing failing.
#[must_use]
pub fn capabilities(object: &BarObject<'_>) -> Capabilities {
    Capabilities {
        // A glyph object has type size, not stroke width: a width control on
        // a text note moves nothing.
        stroke_width: object.glyph_size.is_none(),
        glyph_size: object.glyph_size.is_some(),
        authored: object.author.is_some(),
        settings: object.settings_available,
        sharing: object.shareable,
        magnet: object.magnet.is_some(),
        draw_again: object.draw_again.is_some(),
    }
}

/// Draw the bar at `position` and report what was asked of it.
///
/// The caller owns every mutation except the style and glyph size, which are
/// `Copy` values edited in place and written back as one undo entry.
pub fn show(
    bar: &mut ContextBar,
    ctx: &egui::Context,
    position: egui::Pos2,
    popover_bounds: egui::Rect,
    object: &mut BarObject<'_>,
) -> ContextBarIntent {
    let palette = theme::get(ctx);
    let mut intent = ContextBarIntent::default();
    let cells = slots(capabilities(object));
    let size = bar_size(&cells);
    let rect = egui::Rect::from_min_size(position, size);
    bar.rect = Some(rect);
    #[cfg(test)]
    {
        bar.probe.draw_again = None;
    }
    if bar.delete_hover_for != object.tool_name {
        bar.delete_hover_for = object.tool_name;
        bar.delete_hover = format!("Delete {} (Del)", object.tool_name.to_lowercase());
    }

    // One opacity for the whole surface: near the pointer it is a tool, away
    // from it it steps back behind the price without disappearing.
    let awake = bar.open != Popover::None
        || ctx
            .pointer_latest_pos()
            .is_some_and(|pointer| rect.expand(WAKE_RADIUS_PX).contains(pointer));
    let opacity = ctx.animate_value_with_time(
        egui::Id::new("drawing_context_bar_opacity"),
        if awake { 1.0 } else { IDLE_OPACITY },
        FADE_SECONDS,
    );

    let frame = floating_frame(&palette);

    egui::Area::new(egui::Id::new("drawing_context_bar"))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            frame.show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = ITEM_GAP_PX;
                ui.horizontal(|ui| {
                    for cell in &cells {
                        draw_slot(ui, bar, *cell, object, &mut intent);
                    }
                });
            });
        });
    draw_popover(bar, ctx, popover_bounds, object, &mut intent);
    intent
}

fn draw_slot(
    ui: &mut egui::Ui,
    bar: &mut ContextBar,
    slot: Slot,
    object: &mut BarObject<'_>,
    intent: &mut ContextBarIntent,
) {
    let palette = theme::get(ui.ctx());
    match slot {
        Slot::Grip => draw_grip(ui, intent),
        Slot::Author => {
            // An indicator, not a control: the glyph says an operator placed
            // this, the tooltip says which one, and the inspector and the
            // object manager spell it out in words.
            let author = object.author.unwrap_or_default();
            IconButton::new(icons::ROBOT, TOOLRAIL_ICON)
                .accent(palette.text_support())
                .hover_text(&format!("Placed by {author}, not by you"))
                .show(ui);
        }
        Slot::Separator => draw_separator(ui),
        Slot::DrawAgain => {
            // `slots` offers this cell only when the object names its tool.
            let Some(face) = object.draw_again else {
                return;
            };
            let pencil = IconButton::new(face.glyph, TOOLRAIL_ICON)
                .hover_text(face.hover)
                .show(ui);
            #[cfg(test)]
            {
                bar.probe.draw_again = Some(pencil.rect);
            }
            intent.draw_again |= pencil.clicked();
        }
        Slot::Color => draw_color_slot(ui, bar, object),
        Slot::Width => draw_width_slot(ui, bar, object),
        Slot::GlyphSize => draw_glyph_size_slot(ui, bar),
        Slot::Duplicate => {
            if IconButton::new(icons::COPY_SIMPLE, TOOLRAIL_ICON)
                .hover_text(DUPLICATE_HOVER_TEXT)
                .show(ui)
                .clicked()
            {
                intent.duplicate = true;
            }
        }
        Slot::Lock => {
            let (glyph, hover) = if object.locked {
                (icons::LOCK_SIMPLE, "Unlock drawing")
            } else {
                (icons::LOCK_SIMPLE_OPEN, "Lock drawing")
            };
            if IconButton::new(glyph, TOOLRAIL_ICON)
                .active(object.locked)
                .hover_text(hover)
                .show(ui)
                .clicked()
            {
                intent.actions.toggle_lock = true;
            }
        }
        Slot::Magnet => {
            let on = object.magnet.unwrap_or_default();
            let (glyph, hover) = magnet_control(on);
            let magnet = IconButton::new(glyph, TOOLRAIL_ICON)
                .active(on)
                .hover_text(hover)
                .show(ui);
            #[cfg(test)]
            {
                bar.probe.magnet = Some(magnet.rect);
            }
            if magnet.clicked() {
                intent.toggle_magnet = true;
            }
        }
        Slot::Hide => {
            let (glyph, hover) = if object.hidden {
                (icons::EYE_SLASH, "Show this drawing again")
            } else {
                (icons::EYE, "Hide drawing")
            };
            if IconButton::new(glyph, TOOLRAIL_ICON)
                .active(object.hidden)
                .hover_text(hover)
                .show(ui)
                .clicked()
            {
                intent.toggle_hidden = true;
            }
        }
        Slot::Sharing => {
            let (glyph, hover) = sharing_control(object.shared);
            let sharing = IconButton::new(glyph, TOOLRAIL_ICON)
                .active(object.shared)
                .hover_text(hover)
                .show(ui);
            #[cfg(test)]
            {
                bar.probe.sharing = Some(sharing.rect);
            }
            if sharing.clicked() {
                intent.toggle_shared = true;
            }
        }
        Slot::Settings => {
            let gear = IconButton::new(icons::GEAR, TOOLRAIL_ICON)
                .hover_text("Settings - every property of this drawing")
                .show(ui);
            #[cfg(test)]
            {
                bar.probe.gear = Some(gear.rect);
            }
            if gear.clicked() {
                intent.open_settings = true;
            }
        }
        Slot::Delete if object.confirming_delete => {
            // Words, here and only here. The bar grows for the length of the
            // question, which is a visible, deliberate transition rather
            // than the silent no-op a locked object would otherwise give.
            if ui
                .button(egui::RichText::new("Delete locked drawing?").color(palette.warn()))
                .clicked()
            {
                intent.force_delete = true;
            }
            if IconButton::new(icons::X, TOOLRAIL_ICON)
                .hover_text("Keep it")
                .show(ui)
                .clicked()
            {
                intent.cancel_delete = true;
            }
        }
        Slot::Delete => {
            // The glyph earns its place through the tooltip that names the
            // object, the toast that names it again on the way out, and the
            // undo behind both.
            if IconButton::new(icons::TRASH_SIMPLE, TOOLRAIL_ICON)
                .accent(palette.warn())
                .hover_text(&bar.delete_hover)
                .show(ui)
                .clicked()
            {
                intent.actions.delete = true;
            }
        }
    }
}

/// The rail's magnet glyph, named for this object alone: the rail's switch
/// is a different one and stays where it is.
fn magnet_control(on: bool) -> (&'static str, &'static str) {
    if on {
        (icons::MAGNET, "Stop snapping this drawing to OHLC")
    } else {
        (icons::MAGNET, "Snap this drawing to OHLC")
    }
}

fn sharing_control(shared: bool) -> (&'static str, &'static str) {
    if shared {
        (icons::GRID_FOUR, "Show only on this chart")
    } else {
        (icons::GRID_FOUR, "Show on all timeframes")
    }
}

/// The grip: the one part of the bar that is a drag surface. The slots are
/// never draggable, so a click that lands slightly off a button can only
/// miss — it can never smear the bar across the chart.
fn draw_grip(ui: &mut egui::Ui, intent: &mut ContextBarIntent) {
    let palette = theme::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(GRIP_WIDTH_PX, TOOLRAIL_ICON.hit),
        egui::Sense::click_and_drag(),
    );
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() {
            palette.text_muted()
        } else {
            palette.text_faint()
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::DOTS_SIX_VERTICAL,
            palette.body_font(GRIP_GLYPH_PX),
            color,
        );
    }
    let response = response.on_hover_text("Drag to move · double-click to snap back");
    if response.double_clicked() {
        intent.reset_position = true;
    } else if response.dragged() {
        intent.drag_delta = response.drag_delta();
    }
}

fn draw_separator(ui: &mut egui::Ui) {
    let palette = theme::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(SEPARATOR_WIDTH_PX, TOOLRAIL_ICON.hit),
        egui::Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        ui.painter().line_segment(
            [
                egui::pos2(rect.center().x, rect.top() + SEPARATOR_INSET_Y_PX),
                egui::pos2(rect.center().x, rect.bottom() - SEPARATOR_INSET_Y_PX),
            ],
            egui::Stroke::new(SEPARATOR_WIDTH_PX, palette.border()),
        );
    }
}

/// The colour slot shows the object's colour rather than a paint-can glyph:
/// the control *is* the current value, which is what lets it answer "what
/// colour is this?" without being clicked.
fn draw_color_slot(ui: &mut egui::Ui, bar: &mut ContextBar, object: &BarObject<'_>) {
    let palette = theme::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(TOOLRAIL_ICON.hit, TOOLRAIL_ICON.hit),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let swatch =
            egui::Rect::from_center_size(rect.center(), egui::vec2(SWATCH_SIDE_PX, SWATCH_SIDE_PX));
        let painter = ui.painter();
        painter.rect_filled(swatch, egui::Rounding::same(4.0), object.style.color32());
        painter.rect_stroke(
            swatch,
            egui::Rounding::same(4.0),
            egui::Stroke::new(1.0_f32, palette.border()),
        );
    }
    #[cfg(test)]
    {
        bar.probe.color = Some(rect);
    }
    let response = response.on_hover_text(hover_text(
        object,
        "Colour",
        "Colour - the lock protects position, not style",
    ));
    if response.clicked() {
        bar.toggle(Popover::Color, rect);
    }
}

/// Same idea as the colour slot: the button is a stroke drawn at the width
/// it sets.
fn draw_width_slot(ui: &mut egui::Ui, bar: &mut ContextBar, object: &BarObject<'_>) {
    let palette = theme::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(TOOLRAIL_ICON.hit, TOOLRAIL_ICON.hit),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() {
            palette.text_primary()
        } else {
            palette.text_muted()
        };
        ui.painter().line_segment(
            [
                egui::pos2(
                    rect.center().x - WIDTH_PREVIEW_LEN_PX / 2.0,
                    rect.center().y,
                ),
                egui::pos2(
                    rect.center().x + WIDTH_PREVIEW_LEN_PX / 2.0,
                    rect.center().y,
                ),
            ],
            egui::Stroke::new(object.style.width_px, color),
        );
    }
    let response = response.on_hover_text(hover_text(
        object,
        "Line width",
        "Line width - the lock protects position, not style",
    ));
    if response.clicked() {
        bar.toggle(Popover::Width, rect);
    }
}

fn draw_glyph_size_slot(ui: &mut egui::Ui, bar: &mut ContextBar) {
    let response = IconButton::new(icons::TEXT_AA, TOOLRAIL_ICON)
        .hover_text("Text size")
        .show(ui);
    if response.clicked() {
        bar.toggle(Popover::GlyphSize, response.rect);
    }
}

/// A locked object keeps its shape *and* its style: the lock protects the
/// geometry from an accidental drag, which is the only accident the trader
/// asked to be protected from — so the style slots stay live, and say so.
///
/// Both strings are `'static`: this is read on every frame the bar is up,
/// and a `format!` here would allocate sixty times a second to say a
/// sentence that never changes.
fn hover_text(object: &BarObject<'_>, idle: &'static str, locked: &'static str) -> &'static str {
    if object.locked { locked } else { idle }
}

/// Where a popover opens: hanging off the slot it belongs to — under it,
/// flipped above when there is no room, right-aligned on it rather than
/// pushed off it — and never outside `bounds`.
///
/// Downward is the default because that is where the eye goes from a control
/// it just pressed. It cannot be the only answer: the bar can sit one bar's
/// height off the bottom of the pane — automatic placement reaches that for
/// an object at the bottom, and a *parked* bar stays there for every object
/// after it — while the colour palette and the width rows are several times
/// that tall. A palette painted past the pane lands on the time axis, on the
/// neighbouring chart of a split, or off the window, and rows a trader cannot
/// reach read as rows that do not exist.
///
/// `size` is what egui measured last time this popover was open — zero the
/// first time, so that frame goes downward and the flip lands on the next
/// one. That is one frame of a surface the trader just clicked open, and the
/// alternative is measuring a panel before laying it out.
///
/// egui keeps the last word, against the *window*: its constraint rectangle
/// is also the clip rectangle, so handing it the pane would silently cut the
/// lower rows of a popover taller than the pane. Covering the bar is the
/// better failure, and the one this rule tries hardest to avoid.
fn popover_position(anchor: egui::Rect, size: egui::Vec2, bounds: egui::Rect) -> egui::Pos2 {
    let below = anchor.left_bottom() + egui::vec2(0.0, POPOVER_GAP_PX);
    let y = if below.y + size.y > bounds.bottom()
        && anchor.top() - POPOVER_GAP_PX - size.y >= bounds.top()
    {
        anchor.top() - POPOVER_GAP_PX - size.y
    } else if size.y <= bounds.height() {
        // Fits the pane, but on neither side of the slot: covering the bar
        // beats rows on the time axis or a neighbouring pane.
        below.y.min(bounds.bottom() - size.y)
    } else {
        below.y
    };
    // Horizontally it hangs from the slot's left edge, and when that would run
    // past the pane it right-aligns on the slot instead of being clamped to an
    // arbitrary column: a palette that no longer touches the control that
    // opened it reads as a panel that belongs to nothing. The clamp stays as
    // the last word, for a popover wider than the pane itself.
    let x = if below.x + size.x > bounds.right() {
        anchor.right() - size.x
    } else {
        below.x
    };
    let max_x = (bounds.right() - size.x).max(bounds.left());
    egui::pos2(x.clamp(bounds.left(), max_x), y)
}

/// The Area id a popover is laid out under.
///
/// Keyed on everything that changes its measured size — the kind, the colour
/// palette's fill row and its inline picker — because egui reads that size
/// back a frame late and it decides which way the popover opens: a shared id
/// flips a popover that had room below. Keyed on *only* that, since a fresh
/// id costs a frame of egui's invisible sizing pass where the trader clicked.
fn popover_id(popover: Popover, fill: bool, custom: bool) -> egui::Id {
    let color = matches!(popover, Popover::Color);
    egui::Id::new((
        "drawing_context_bar_popover",
        popover as u8,
        fill && color,
        custom && color,
    ))
}

fn draw_popover(
    bar: &mut ContextBar,
    ctx: &egui::Context,
    bounds: egui::Rect,
    object: &mut BarObject<'_>,
    intent: &mut ContextBarIntent,
) {
    let palette = theme::get(ctx);
    let (popover, anchor) = match (bar.open, bar.anchor) {
        (Popover::None, _) | (_, None) => {
            bar.popover_layer = None;
            bar.custom_open = false;
            return;
        }
        (popover, Some(anchor)) => (popover, anchor),
    };
    let id = popover_id(popover, object.supports_fill, bar.custom_open);
    // The size egui measured last time this kind was open. Zero the first
    // time, which is what makes the default answer the downward one.
    let size = ctx
        .memory(|memory| memory.area_rect(id))
        .map_or(egui::Vec2::ZERO, |rect| rect.size());
    let at = popover_position(anchor, size, bounds);
    #[cfg(test)]
    let mut probe = ColorProbe::default();
    let custom_open = &mut bar.custom_open;
    let response = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(palette.control())
                .stroke(egui::Stroke::new(1.0_f32, palette.border()))
                .show(ui, |ui| match popover {
                    Popover::Color => draw_color_popover(
                        ui,
                        object,
                        intent,
                        custom_open,
                        #[cfg(test)]
                        &mut probe,
                    ),
                    Popover::Width => draw_width_popover(ui, object, intent),
                    Popover::GlyphSize => draw_glyph_size_popover(ui, object, intent),
                    Popover::None => {}
                });
        })
        .response;
    bar.popover_layer = Some(id);
    #[cfg(test)]
    {
        bar.probe.colors = probe;
    }
    // Click anywhere else closes the popover and keeps the selection: the
    // bar is never modal, and dismissing a colour list is not a reason to
    // lose the object you were styling.
    if ctx.input(|input| input.pointer.any_click())
        && !response
            .rect
            .contains(ctx.pointer_latest_pos().unwrap_or_default())
        && !bar
            .rect
            .is_some_and(|rect| rect.contains(ctx.pointer_latest_pos().unwrap_or_default()))
    {
        bar.open = Popover::None;
    }
}

fn draw_color_popover(
    ui: &mut egui::Ui,
    object: &mut BarObject<'_>,
    intent: &mut ContextBarIntent,
    custom_open: &mut bool,
    #[cfg(test)] probe: &mut ColorProbe,
) {
    let palette = theme::get(ui.ctx());
    // Both axes: the selected ring sits outside its swatch.
    ui.spacing_mut().item_spacing = egui::vec2(ITEM_GAP_PX, ITEM_GAP_PX);
    let primaries = quantick_appearance::palette::DRAWING_PRIMARIES.map(super::to_color32);
    for row in [&palette.drawing_swatches()[..], &primaries[..]] {
        draw_swatch_row(
            ui,
            row,
            object,
            intent,
            #[cfg(test)]
            &mut probe.swatches,
        );
    }
    ui.separator();
    // The full picker stays one row away rather than one panel away: sending
    // the trader to another surface to choose a violet trades one friction
    // for a larger one.
    ui.horizontal(|ui| {
        let custom = ui.selectable_label(
            *custom_open,
            egui::RichText::new("Custom")
                .small()
                .color(palette.text_support()),
        );
        #[cfg(test)]
        {
            probe.custom = Some(custom.rect);
        }
        if custom.clicked() {
            *custom_open = !*custom_open;
        }
    });
    if *custom_open {
        let before = object.style.color32();
        let mut color = before;
        let picker = ui.scope(|ui| {
            egui::color_picker::color_picker_color32(
                ui,
                &mut color,
                egui::color_picker::Alpha::Opaque,
            )
        });
        #[cfg(test)]
        {
            probe.picker = Some(picker.response.rect);
        }
        // The picker is opaque, so it keeps the drawing's own alpha, and it
        // reports a change on opening: only a different colour is an edit.
        let [r, g, b, _] = color.to_srgba_unmultiplied();
        let color = egui::Color32::from_rgba_unmultiplied(r, g, b, before.a());
        if picker.inner && color != before {
            object.style.set_color32(color);
            intent.edited = true;
        }
    }
    if object.supports_fill {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Fill")
                    .small()
                    .color(palette.text_support()),
            );
            let fill = ui.add(egui::Slider::new(
                &mut object.style.fill_alpha,
                0..=MAX_DRAWING_FILL_ALPHA,
            ));
            #[cfg(test)]
            {
                probe.fill = Some(fill.rect);
            }
            if fill.changed() {
                intent.edited = true;
            }
        });
    }
}

fn draw_swatch_row(
    ui: &mut egui::Ui,
    swatches: &[egui::Color32],
    object: &mut BarObject<'_>,
    intent: &mut ContextBarIntent,
    #[cfg(test)] rects: &mut Vec<egui::Rect>,
) {
    let palette = theme::get(ui.ctx());
    ui.horizontal(|ui| {
        for &swatch in swatches {
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(PALETTE_SWATCH_PX, PALETTE_SWATCH_PX),
                egui::Sense::click(),
            );
            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                painter.rect_filled(rect, egui::Rounding::same(4.0), swatch);
                if object.style.color32() == swatch {
                    // The ring is TEXT_PRIMARY, not ACCENT: ACCENT is itself
                    // one of the swatches. It sits just outside the swatch,
                    // on the popover's own fill, so white keeps it too.
                    painter.rect_stroke(
                        rect.expand(SELECTED_RING_GAP_PX),
                        egui::Rounding::same(5.0),
                        egui::Stroke::new(2.0_f32, palette.text_primary()),
                    );
                } else if response.hovered() {
                    painter.rect_stroke(
                        rect,
                        egui::Rounding::same(4.0),
                        egui::Stroke::new(1.0_f32, palette.text_muted()),
                    );
                }
            }
            #[cfg(test)]
            {
                rects.push(rect);
            }
            if response.clicked() {
                object.style.set_color32(swatch);
                intent.edited = true;
            }
        }
    });
}

fn draw_width_popover(
    ui: &mut egui::Ui,
    object: &mut BarObject<'_>,
    intent: &mut ContextBarIntent,
) {
    for step in WIDTH_STEPS_PX {
        let active = (object.style.width_px - step).abs() < f32::EPSILON;
        if draw_preview_row(
            ui,
            active,
            &format!("{step:.0} px"),
            |painter, rect, color| {
                painter.line_segment(
                    [
                        egui::pos2(rect.left() + 8.0, rect.center().y),
                        egui::pos2(
                            rect.left() + 8.0 + WIDTH_PREVIEW_LEN_PX * 2.0,
                            rect.center().y,
                        ),
                    ],
                    egui::Stroke::new(step, color),
                );
            },
        ) {
            object.style.width_px = step;
            intent.edited = true;
        }
    }
}

fn draw_glyph_size_popover(
    ui: &mut egui::Ui,
    object: &mut BarObject<'_>,
    intent: &mut ContextBarIntent,
) {
    let Some(size) = object.glyph_size else {
        return;
    };
    let mut px = size.px;
    if ui
        .add(
            egui::Slider::new(&mut px, size.min..=size.max)
                .suffix(" px")
                .text("Size"),
        )
        .changed()
    {
        object.glyph_size = Some(GlyphSize { px, ..size });
        intent.edited = true;
    }
}

/// One row of a picker popover: a preview of the value on the left, the
/// value in words on the right. The preview is the point — a trader picking
/// a width should see widths, not read numbers.
fn draw_preview_row(
    ui: &mut egui::Ui,
    active: bool,
    label: &str,
    preview: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> bool {
    let palette = theme::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(POPOVER_ROW_WIDTH_PX, POPOVER_ROW_HEIGHT_PX),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if active {
            painter.rect_filled(
                rect,
                egui::Rounding::same(4.0),
                theme::active_tint(palette.accent()),
            );
        } else if response.hovered() {
            painter.rect_filled(rect, egui::Rounding::same(4.0), palette.hover_fill());
        }
        let ink = if active {
            palette.accent()
        } else {
            palette.text_muted()
        };
        preview(painter, rect, ink);
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            label,
            palette.body_font(11.0),
            palette.text_faint(),
        );
    }
    response.clicked()
}

#[cfg(test)]
impl ContextBar {
    #[must_use]
    pub fn gear_rect(&self) -> Option<egui::Rect> {
        self.probe.gear
    }

    #[must_use]
    pub fn draw_again_rect(&self) -> Option<egui::Rect> {
        self.probe.draw_again
    }

    #[must_use]
    pub fn sharing_rect(&self) -> Option<egui::Rect> {
        self.probe.sharing
    }

    #[must_use]
    pub fn magnet_rect(&self) -> Option<egui::Rect> {
        self.probe.magnet
    }

    #[must_use]
    pub fn color_rect(&self) -> Option<egui::Rect> {
        self.probe.color
    }

    #[must_use]
    pub fn swatch_rect(&self, index: usize) -> Option<egui::Rect> {
        self.probe.colors.swatches.get(index).copied()
    }

    #[must_use]
    pub fn fill_slider_rect(&self) -> Option<egui::Rect> {
        self.probe.colors.fill
    }

    #[must_use]
    pub fn custom_toggle_rect(&self) -> Option<egui::Rect> {
        self.probe.colors.custom
    }

    #[must_use]
    pub fn custom_picker_rect(&self) -> Option<egui::Rect> {
        self.probe.colors.picker
    }

    pub fn popover_open(&self) -> bool {
        self.open != Popover::None
    }
}

#[cfg(test)]
mod tests;
