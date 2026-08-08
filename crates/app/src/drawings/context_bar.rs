//! The selected drawing's context bar: one floating row of icons that opens
//! where the object is, so the actions a trader takes with their eye on the
//! chart never cost a trip to a panel.
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

use eframe::egui;
use egui_phosphor::regular as icons;
use smallvec::SmallVec;

use super::action_bar::ActionBarIntent;
use super::{DrawingStyle, GlyphSize, MAX_DRAWING_FILL_ALPHA};
use crate::theme;
use crate::widgets::{IconButton, TOOLRAIL_ICON};

/// Height of the bar: the rail's 32 px touch target with 4 px of air.
pub const BAR_HEIGHT_PX: f32 = 2.0 * BAR_PAD_Y_PX + TOOLRAIL_ICON.hit;
const BAR_PAD_X_PX: f32 = 6.0;
const BAR_PAD_Y_PX: f32 = 4.0;
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
/// Length of the stroke preview painted inside the width slot.
const WIDTH_PREVIEW_LEN_PX: f32 = 18.0;
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

/// One cell of the bar, in paint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Grip,
    Color,
    Width,
    /// Type/glyph size, for the tools drawn as glyphs rather than strokes.
    GlyphSize,
    Separator,
    Duplicate,
    Lock,
    Hide,
    Settings,
    Delete,
}

impl Slot {
    /// Horizontal room this cell takes.
    #[must_use]
    pub fn width(self) -> f32 {
        match self {
            Self::Grip => GRIP_WIDTH_PX,
            Self::Separator => SEPARATOR_WIDTH_PX,
            _ => TOOLRAIL_ICON.hit,
        }
    }
}

/// What the selected object can actually be asked for. The bar is built from
/// this and nothing else — an unsupported property is absent, never a grey
/// button, which is the rule [`super::DrawingToolImpl`] already states for
/// the inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub stroke_width: bool,
    pub glyph_size: bool,
    /// Whether the gear earns its slot. It does not when the inspector is
    /// pinned: its destination is already on screen.
    pub settings: bool,
}

/// The cells of a bar for an object with these capabilities.
///
/// A `SmallVec`, not a `Vec`: this is rebuilt on every frame something is
/// selected, and the bar never holds more cells than fit inline.
#[must_use]
pub fn slots(caps: Capabilities) -> SmallVec<[Slot; 12]> {
    let mut slots = SmallVec::from_slice(&[Slot::Grip, Slot::Color]);
    if caps.stroke_width {
        slots.push(Slot::Width);
    }
    if caps.glyph_size {
        slots.push(Slot::GlyphSize);
    }
    slots.push(Slot::Separator);
    slots.push(Slot::Duplicate);
    slots.push(Slot::Lock);
    slots.push(Slot::Hide);
    if caps.settings {
        slots.push(Slot::Settings);
    }
    // The hairline before the trash is not decoration. It turns the 4 px gap
    // into 9 px, so a hand that misses the gear by a few pixels lands on
    // nothing instead of on the one irreversible button.
    slots.push(Slot::Separator);
    slots.push(Slot::Delete);
    slots
}

/// Size of a bar made of these cells.
#[must_use]
pub fn bar_size(slots: &[Slot]) -> egui::Vec2 {
    let content: f32 = slots.iter().map(|slot| slot.width()).sum();
    let gaps = ITEM_GAP_PX * (slots.len().saturating_sub(1)) as f32;
    egui::vec2(2.0f32.mul_add(BAR_PAD_X_PX, content + gaps), BAR_HEIGHT_PX)
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
    let y = if above >= chart.top() {
        above
    } else if below + size.y <= chart.bottom() {
        below
    } else {
        // An object taller than the pane — a vertical line, a channel across
        // the whole view. Hug the top edge rather than sit in the middle of
        // the price.
        chart.top() + OBJECT_GAP_PX
    };
    // Prefer to stay clear of the live lane; if the bar is wider than the
    // history area itself, the chart's own edge wins over the preference.
    let right = right_limit.min(chart.right()).max(chart.left() + size.x);
    let max_x = (right - size.x).max(chart.left());
    let x = bbox.left().clamp(chart.left(), max_x);
    let max_y = (chart.bottom() - size.y).max(chart.top());
    egui::pos2(x, y.clamp(chart.top(), max_y))
}

/// The four type sizes a glyph tool offers, spread across its own range.
#[must_use]
pub fn glyph_steps(size: GlyphSize) -> [f32; 4] {
    let step = (size.max - size.min) / 3.0;
    [
        size.min,
        (step.mul_add(1.0, size.min)).round(),
        (step.mul_add(2.0, size.min)).round(),
        size.max,
    ]
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
    /// Where the trader dragged the bar. Absolute screen position, cleared
    /// on every selection change: a remembered position belongs to the
    /// object it was chosen for, and reappearing far from the *next* object
    /// is the failure mode of remembering it globally.
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
    /// Where the gear and the colour controls landed, so a test can press
    /// the real buttons rather than trust arithmetic about slot order.
    #[cfg(test)]
    gear_rect: Option<egui::Rect>,
    #[cfg(test)]
    color_rect: Option<egui::Rect>,
    #[cfg(test)]
    swatch_rects: Vec<egui::Rect>,
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
    /// selection changed, and drops any manual position with it.
    pub fn note_selection(&mut self, selection: Option<usize>) -> bool {
        if self.last_selection == selection {
            return false;
        }
        self.last_selection = selection;
        self.manual = None;
        self.open = Popover::None;
        self.rect = None;
        true
    }

    #[must_use]
    pub fn manual_position(&self) -> Option<egui::Pos2> {
        self.manual
    }

    /// The trader dragged the bar by the grip: it stops following the object
    /// and stops being suppressed by pan and zoom — it is where they put it,
    /// on purpose.
    pub fn set_manual(&mut self, position: egui::Pos2) {
        self.manual = Some(position);
    }

    /// Escape with a hand-placed bar puts it back on the object before it
    /// touches the selection.
    pub fn clear_manual(&mut self) -> bool {
        self.manual.take().is_some()
    }

    #[must_use]
    pub fn last_rect(&self) -> Option<egui::Rect> {
        self.rect
    }

    #[cfg(test)]
    #[must_use]
    pub fn gear_rect(&self) -> Option<egui::Rect> {
        self.gear_rect
    }

    #[cfg(test)]
    #[must_use]
    pub fn color_rect(&self) -> Option<egui::Rect> {
        self.color_rect
    }

    #[cfg(test)]
    #[must_use]
    pub fn swatch_rect(&self, index: usize) -> Option<egui::Rect> {
        self.swatch_rects.get(index).copied()
    }

    fn toggle(&mut self, popover: Popover, anchor: egui::Rect) {
        self.open = if self.open == popover {
            Popover::None
        } else {
            popover
        };
        self.anchor = Some(anchor);
    }

    #[cfg(test)]
    pub fn popover_open(&self) -> bool {
        self.open != Popover::None
    }
}

/// What the bar asked for this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ContextBarIntent {
    /// Lock and delete, in the one shape every host reports them in.
    pub actions: ActionBarIntent,
    pub toggle_hidden: bool,
    pub duplicate: bool,
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
    pub hidden: bool,
    pub supports_fill: bool,
    /// Whether the gear earns a slot — false while the inspector is pinned.
    pub settings_available: bool,
    /// A delete was asked for on a protected object and is waiting for the
    /// second step. This is the one state where the bar speaks in words: a
    /// locked object is one the trader declared important, and condition (c)
    /// of this module's contract is what buys the bare glyph everywhere else.
    pub confirming_delete: bool,
    /// Name of the tool, for the delete tooltip: a trader who cannot tell
    /// *what* was deleted cannot use undo to get it back. `'static` because
    /// the registry's names are, which is what lets the tooltip be cached
    /// instead of formatted every frame.
    pub tool_name: &'static str,
}

/// Draw the bar at `position` and report what was asked of it.
///
/// The caller owns every mutation except the style and glyph size, which are
/// `Copy` values edited in place and written back as one undo entry.
pub fn show(
    bar: &mut ContextBar,
    ctx: &egui::Context,
    position: egui::Pos2,
    object: &mut BarObject<'_>,
) -> ContextBarIntent {
    let mut intent = ContextBarIntent::default();
    let caps = Capabilities {
        // A glyph object has type size, not stroke width: a width control on
        // a text note moves nothing.
        stroke_width: object.glyph_size.is_none(),
        glyph_size: object.glyph_size.is_some(),
        settings: object.settings_available,
    };
    let cells = slots(caps);
    let size = bar_size(&cells);
    let rect = egui::Rect::from_min_size(position, size);
    bar.rect = Some(rect);
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

    let frame = egui::Frame::none()
        .fill(theme::CHROME)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
        .rounding(egui::Rounding::same(CORNER_RADIUS_PX))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 2.0),
            blur: 12.0,
            spread: 0.0,
            color: theme::FLOAT_SHADOW,
        })
        .inner_margin(egui::Margin::symmetric(BAR_PAD_X_PX, BAR_PAD_Y_PX));

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
    draw_popover(bar, ctx, object, &mut intent);
    intent
}

fn draw_slot(
    ui: &mut egui::Ui,
    bar: &mut ContextBar,
    slot: Slot,
    object: &mut BarObject<'_>,
    intent: &mut ContextBarIntent,
) {
    match slot {
        Slot::Grip => draw_grip(ui, intent),
        Slot::Separator => draw_separator(ui),
        Slot::Color => draw_color_slot(ui, bar, object),
        Slot::Width => draw_width_slot(ui, bar, object),
        Slot::GlyphSize => draw_glyph_size_slot(ui, bar),
        Slot::Duplicate => {
            if IconButton::new(icons::COPY_SIMPLE, TOOLRAIL_ICON)
                .hover_text("Duplicate (Ctrl+D)")
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
        Slot::Settings => {
            let gear = IconButton::new(icons::GEAR, TOOLRAIL_ICON)
                .hover_text("Settings - every property of this drawing")
                .show(ui);
            #[cfg(test)]
            {
                bar.gear_rect = Some(gear.rect);
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
                .button(egui::RichText::new("Delete locked drawing?").color(theme::WARN))
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
                .accent(theme::WARN)
                .hover_text(&bar.delete_hover)
                .show(ui)
                .clicked()
            {
                intent.actions.delete = true;
            }
        }
    }
}

/// The grip: the one part of the bar that is a drag surface. The slots are
/// never draggable, so a click that lands slightly off a button can only
/// miss — it can never smear the bar across the chart.
fn draw_grip(ui: &mut egui::Ui, intent: &mut ContextBarIntent) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(GRIP_WIDTH_PX, TOOLRAIL_ICON.hit),
        egui::Sense::click_and_drag(),
    );
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() {
            theme::TEXT_MUTED
        } else {
            theme::TEXT_FAINT
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::DOTS_SIX_VERTICAL,
            egui::FontId::proportional(GRIP_GLYPH_PX),
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
            egui::Stroke::new(SEPARATOR_WIDTH_PX, theme::BORDER),
        );
    }
}

/// The colour slot shows the object's colour rather than a paint-can glyph:
/// the control *is* the current value, which is what lets it answer "what
/// colour is this?" without being clicked.
fn draw_color_slot(ui: &mut egui::Ui, bar: &mut ContextBar, object: &BarObject<'_>) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(TOOLRAIL_ICON.hit, TOOLRAIL_ICON.hit),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let swatch =
            egui::Rect::from_center_size(rect.center(), egui::vec2(SWATCH_SIDE_PX, SWATCH_SIDE_PX));
        let painter = ui.painter();
        painter.rect_filled(swatch, egui::Rounding::same(4.0), object.style.color);
        painter.rect_stroke(
            swatch,
            egui::Rounding::same(4.0),
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
    }
    #[cfg(test)]
    {
        bar.color_rect = Some(rect);
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
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(TOOLRAIL_ICON.hit, TOOLRAIL_ICON.hit),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_MUTED
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

fn draw_popover(
    bar: &mut ContextBar,
    ctx: &egui::Context,
    object: &mut BarObject<'_>,
    intent: &mut ContextBarIntent,
) {
    let (popover, anchor) = match (bar.open, bar.anchor) {
        (Popover::None, _) | (_, None) => return,
        (popover, Some(anchor)) => (popover, anchor),
    };
    let id = egui::Id::new("drawing_context_bar_popover");
    #[cfg(test)]
    let mut swatch_rects = Vec::new();
    let response = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(anchor.left_bottom() + egui::vec2(0.0, POPOVER_GAP_PX))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::CONTROL)
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .show(ui, |ui| match popover {
                    Popover::Color => draw_color_popover(
                        ui,
                        object,
                        intent,
                        #[cfg(test)]
                        &mut swatch_rects,
                    ),
                    Popover::Width => draw_width_popover(ui, object, intent),
                    Popover::GlyphSize => draw_glyph_size_popover(ui, object, intent),
                    Popover::None => {}
                });
        })
        .response;
    #[cfg(test)]
    {
        bar.swatch_rects = swatch_rects;
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
    #[cfg(test)] swatch_rects: &mut Vec<egui::Rect>,
) {
    ui.spacing_mut().item_spacing.x = ITEM_GAP_PX;
    ui.horizontal(|ui| {
        for swatch in theme::DRAWING_SWATCHES {
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(PALETTE_SWATCH_PX, PALETTE_SWATCH_PX),
                egui::Sense::click(),
            );
            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                painter.rect_filled(rect, egui::Rounding::same(4.0), swatch);
                if object.style.color == swatch {
                    // The ring is TEXT_PRIMARY, not ACCENT: ACCENT is itself
                    // one of the eight, and a ring would vanish on it.
                    painter.rect_stroke(
                        rect,
                        egui::Rounding::same(4.0),
                        egui::Stroke::new(2.0_f32, theme::TEXT_PRIMARY),
                    );
                } else if response.hovered() {
                    painter.rect_stroke(
                        rect,
                        egui::Rounding::same(4.0),
                        egui::Stroke::new(1.0_f32, theme::TEXT_MUTED),
                    );
                }
            }
            #[cfg(test)]
            {
                swatch_rects.push(rect);
            }
            if response.clicked() {
                object.style.color = swatch;
                intent.edited = true;
            }
        }
    });
    ui.separator();
    // The full picker stays one row away rather than one panel away: sending
    // the trader to another surface to choose a violet trades one friction
    // for a larger one.
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Custom")
                .small()
                .color(theme::TEXT_SUPPORT),
        );
        if ui
            .color_edit_button_srgba(&mut object.style.color)
            .changed()
        {
            intent.edited = true;
        }
    });
    if object.supports_fill {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Fill")
                    .small()
                    .color(theme::TEXT_SUPPORT),
            );
            if ui
                .add(egui::Slider::new(
                    &mut object.style.fill_alpha,
                    0..=MAX_DRAWING_FILL_ALPHA,
                ))
                .changed()
            {
                intent.edited = true;
            }
        });
    }
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
    for step in glyph_steps(size) {
        let active = (size.px - step).abs() < f32::EPSILON;
        if draw_preview_row(
            ui,
            active,
            &format!("{step:.0} px"),
            |painter, rect, color| {
                painter.text(
                    egui::pos2(rect.left() + 10.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Aa",
                    egui::FontId::proportional(step.min(POPOVER_ROW_HEIGHT_PX - 6.0)),
                    color,
                );
            },
        ) {
            object.glyph_size = Some(GlyphSize { px: step, ..size });
            intent.edited = true;
        }
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
    let width = ui.available_width().max(120.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, POPOVER_ROW_HEIGHT_PX),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if active {
            painter.rect_filled(
                rect,
                egui::Rounding::same(4.0),
                theme::active_tint(theme::ACCENT),
            );
        } else if response.hovered() {
            painter.rect_filled(rect, egui::Rounding::same(4.0), theme::BORDER);
        }
        let ink = if active {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        };
        preview(painter, rect, ink);
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::proportional(11.0),
            theme::TEXT_FAINT,
        );
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: Capabilities = Capabilities {
        stroke_width: true,
        glyph_size: false,
        settings: true,
    };

    #[test]
    fn the_bar_never_borrows_the_provenance_amber() {
        // Amber promises "this data is not live". A drawing painted amber by
        // taste would spend that promise. Grep-guarded like the rail's own
        // rule and the indicators crate's libm rule.
        let source = include_str!("context_bar.rs");
        assert!(
            !source.contains(concat!("theme::", "AM", "BER")),
            "the context bar's palette never includes the provenance amber"
        );
    }

    #[test]
    fn the_bar_has_no_overflow_menu() {
        // Its overflow is the gear. A "more" button would mean the bar had
        // stopped being a bar, and the ceiling exists to make that visible
        // in review rather than three PRs later.
        let source = include_str!("context_bar.rs");
        assert!(
            !source.contains(concat!("DOTS_", "THREE")),
            "the gear is the overflow; a more-menu is the failure mode"
        );
        let functional = slots(PLAIN)
            .into_iter()
            .filter(|slot| !matches!(slot, Slot::Grip | Slot::Separator))
            .count();
        assert!(
            functional <= 8,
            "the bar holds at most 8 functional slots; found {functional}"
        );
    }

    #[test]
    fn an_unsupported_property_is_absent_not_disabled() {
        let glyph_tool = slots(Capabilities {
            stroke_width: false,
            glyph_size: true,
            settings: true,
        });
        assert!(!glyph_tool.contains(&Slot::Width));
        assert!(glyph_tool.contains(&Slot::GlyphSize));

        let stroke_tool = slots(PLAIN);
        assert!(stroke_tool.contains(&Slot::Width));
        assert!(!stroke_tool.contains(&Slot::GlyphSize));
    }

    #[test]
    fn a_pinned_inspector_takes_the_gear_off_the_bar() {
        // The destination is already on screen; a button that leads where
        // the eye already is would be the dead slot the contract forbids.
        let pinned = slots(Capabilities {
            settings: false,
            ..PLAIN
        });
        assert!(!pinned.contains(&Slot::Settings));
        assert!(
            pinned.contains(&Slot::Delete),
            "everything else survives the gear leaving"
        );
    }

    #[test]
    fn the_trash_is_isolated_by_a_hairline() {
        let cells = slots(PLAIN);
        let trash = cells.iter().position(|slot| *slot == Slot::Delete).unwrap();
        assert_eq!(
            cells[trash - 1],
            Slot::Separator,
            "a hand that misses the neighbouring button must land on nothing"
        );
    }

    #[test]
    fn the_bar_is_one_row_of_touch_targets() {
        // Whatever the object's capabilities, the bar stays one row tall and
        // every slot keeps the rail's 32 px target: it is a strip, and a
        // strip that grew a second row would be a panel again.
        for caps in [
            PLAIN,
            Capabilities {
                stroke_width: false,
                glyph_size: true,
                settings: false,
            },
        ] {
            let cells = slots(caps);
            assert_eq!(bar_size(&cells).y, BAR_HEIGHT_PX);
            for cell in cells {
                if !matches!(cell, Slot::Grip | Slot::Separator) {
                    assert_eq!(cell.width(), TOOLRAIL_ICON.hit, "{cell:?} lost its target");
                }
            }
        }
    }

    #[test]
    fn every_offered_width_is_inside_the_models_range() {
        for step in WIDTH_STEPS_PX {
            assert!(
                (super::super::MIN_DRAWING_WIDTH_PX..=super::super::MAX_DRAWING_WIDTH_PX)
                    .contains(&step),
                "{step} px is outside the drawing model's own range"
            );
        }
    }

    #[test]
    fn the_glyph_steps_span_the_tools_own_range() {
        let steps = glyph_steps(GlyphSize {
            px: 12.0,
            min: 8.0,
            max: 28.0,
        });
        assert_eq!(steps[0], 8.0);
        assert_eq!(steps[3], 28.0);
        for pair in steps.windows(2) {
            assert!(pair[1] > pair[0], "steps must increase: {steps:?}");
        }
    }

    fn chart() -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0))
    }

    #[test]
    fn the_bar_opens_above_the_object_aligned_to_its_left_edge() {
        let size = bar_size(&slots(PLAIN));
        let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 200.0), egui::pos2(400.0, 260.0));
        let position = place(chart(), chart().right(), bbox, size);
        assert_eq!(position.x, bbox.left(), "left-aligned, never centred");
        assert_eq!(position.y, bbox.top() - OBJECT_GAP_PX - size.y);
    }

    #[test]
    fn an_object_against_the_top_flips_the_bar_below_it() {
        let size = bar_size(&slots(PLAIN));
        let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 2.0), egui::pos2(400.0, 60.0));
        let position = place(chart(), chart().right(), bbox, size);
        assert_eq!(position.y, bbox.bottom() + OBJECT_GAP_PX);
    }

    #[test]
    fn an_object_taller_than_the_pane_hugs_the_top_edge() {
        let size = bar_size(&slots(PLAIN));
        let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 0.0), egui::pos2(320.0, 600.0));
        let position = place(chart(), chart().right(), bbox, size);
        assert_eq!(position.y, chart().top() + OBJECT_GAP_PX);
    }

    #[test]
    fn the_bar_never_covers_the_live_lane() {
        // The trader's first veto: if clicking a drawing costs sight of the
        // price forming, they stop clicking drawings.
        let size = bar_size(&slots(PLAIN));
        let lane_divider = 800.0;
        let bbox = egui::Rect::from_min_max(egui::pos2(780.0, 200.0), egui::pos2(860.0, 260.0));
        let position = place(chart(), lane_divider, bbox, size);
        assert!(
            position.x + size.x <= lane_divider,
            "the bar stops at the live lane: {} + {} > {lane_divider}",
            position.x,
            size.x
        );
    }

    #[test]
    fn the_bar_stays_inside_the_pane_on_every_edge() {
        let size = bar_size(&slots(PLAIN));
        for bbox in [
            egui::Rect::from_min_max(egui::pos2(-50.0, 300.0), egui::pos2(10.0, 340.0)),
            egui::Rect::from_min_max(egui::pos2(980.0, 300.0), egui::pos2(1200.0, 340.0)),
            egui::Rect::from_min_max(egui::pos2(400.0, 560.0), egui::pos2(500.0, 640.0)),
        ] {
            let position = place(chart(), chart().right(), bbox, size);
            let rect = egui::Rect::from_min_size(position, size);
            assert!(
                chart().contains_rect(rect),
                "bar escaped the pane for {bbox:?}: {rect:?}"
            );
        }
    }

    #[test]
    fn a_gesture_suppresses_the_bar_until_it_is_released() {
        let mut bar = ContextBar::default();
        assert!(!bar.suppressed(0));
        bar.suppress_gesture();
        assert!(bar.suppressed(0));
        assert!(bar.suppressed(10_000), "a held gesture never times out");
        bar.release_gesture();
        assert!(!bar.suppressed(0), "the bar returns on release");
    }

    #[test]
    fn a_gesture_without_a_release_expires_on_its_own() {
        // Wheel zoom and window resize have no button to let go of.
        let mut bar = ContextBar::default();
        bar.suppress_transient(1_000);
        assert!(bar.suppressed(1_000));
        assert!(bar.suppressed(1_000 + TRANSIENT_SUPPRESS_MS - 1));
        assert!(!bar.suppressed(1_000 + TRANSIENT_SUPPRESS_MS));
    }

    #[test]
    fn a_renewed_transient_gesture_pushes_its_own_deadline() {
        let mut bar = ContextBar::default();
        bar.suppress_transient(1_000);
        bar.suppress_transient(1_150);
        assert!(
            bar.suppressed(1_000 + TRANSIENT_SUPPRESS_MS),
            "a continuing zoom keeps the bar down"
        );
        assert!(!bar.suppressed(1_150 + TRANSIENT_SUPPRESS_MS));
    }

    #[test]
    fn suppression_closes_any_open_popover() {
        let mut bar = ContextBar::default();
        bar.toggle(Popover::Color, egui::Rect::ZERO);
        assert!(bar.popover_open());
        bar.suppress_gesture();
        assert!(!bar.popover_open(), "a popover cannot outlive its bar");
    }

    #[test]
    fn a_second_click_on_the_same_slot_closes_its_popover() {
        let mut bar = ContextBar::default();
        bar.toggle(Popover::Width, egui::Rect::ZERO);
        assert!(bar.popover_open());
        bar.toggle(Popover::Width, egui::Rect::ZERO);
        assert!(!bar.popover_open());
    }

    #[test]
    fn a_new_selection_drops_the_hand_placed_position() {
        // A position chosen for one object reappearing beside the next one
        // is the failure mode of remembering it globally.
        let mut bar = ContextBar::default();
        bar.note_selection(Some(0));
        bar.set_manual(egui::pos2(120.0, 40.0));
        assert_eq!(bar.manual_position(), Some(egui::pos2(120.0, 40.0)));
        assert!(bar.note_selection(Some(1)));
        assert_eq!(bar.manual_position(), None);
    }

    #[test]
    fn the_same_selection_keeps_the_hand_placed_position() {
        let mut bar = ContextBar::default();
        bar.note_selection(Some(0));
        bar.set_manual(egui::pos2(120.0, 40.0));
        assert!(!bar.note_selection(Some(0)));
        assert_eq!(bar.manual_position(), Some(egui::pos2(120.0, 40.0)));
    }
}
