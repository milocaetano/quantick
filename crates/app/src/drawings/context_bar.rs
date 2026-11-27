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
pub use quantick_drawings::context_bar::{Capabilities, Slot, UNSHAREABLE_REASON, slots};
use quantick_drawings::position_edit::{
    PositionAsk, PositionDefault, PositionEdit, PositionFace, PositionLeg,
};
use quantick_ui_model::drawing_chrome::bar_dismissal::{BarTarget, Press};
use quantick_ui_model::drawing_chrome::bar_state::BarState;

use super::action_bar::ActionBarIntent;
use super::geometry::{pos, px, px_rect};
use super::{DrawingStyle, GlyphSize, MAX_DRAWING_FILL_ALPHA};
use crate::theme;
use crate::widgets::{IconButton, TOOLRAIL_ICON};
use quantick_drawings::bar_placement as placement;

/// Height of the bar: the rail's 32 px touch target with 4 px of air.
pub const BAR_HEIGHT_PX: f32 = 2.0 * BAR_PAD_Y_PX + TOOLRAIL_ICON.hit;
const BAR_PAD_X_PX: f32 = 6.0;
const BAR_PAD_Y_PX: f32 = 4.0;
const DUPLICATE_HOVER_TEXT: &str = "Duplicate (Ctrl+D) · Copy and paste (Ctrl+C, Ctrl+V)";
/// The sharing slot's hover, while kept to this chart and while shared.
const SHARING_HOVER: [&str; 2] = ["Show on all timeframes", "Show only on this chart"];
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
pub use quantick_drawings::bar_placement::OBJECT_GAP_PX;
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
/// Room for the widest risk/reward button, `R/R 1:12.5`.
const RISK_REWARD_WIDTH_PX: f32 = 72.0;

/// Opacity while the pointer is elsewhere. It never reaches zero: the object
/// is selected, and this bar is the proof of that on screen.
const IDLE_OPACITY: f32 = 0.55;
/// How close the pointer must come before the bar wakes to full opacity.
const WAKE_RADIUS_PX: f32 = 24.0;
/// Seconds of the opacity cross-fade, both directions.
const FADE_SECONDS: f32 = 0.12;

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
        Slot::RiskReward => RISK_REWARD_WIDTH_PX,
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

/// [`quantick_drawings::bar_placement::place`], in egui's types.
#[must_use]
pub fn place(
    chart: egui::Rect,
    right_limit: f32,
    bbox: egui::Rect,
    size: egui::Vec2,
) -> egui::Pos2 {
    let (chart, bbox) = (px_rect(chart), px_rect(bbox));
    pos(placement::place(
        chart,
        right_limit,
        bbox,
        px(size.to_pos2()),
    ))
}

/// Which popover the bar has open. Kept here rather than in egui memory so
/// the state machine below can close it as one step of a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
enum Popover {
    #[default]
    None,
    Color,
    Width,
    GlyphSize,
    Leg(PositionLeg),
    RiskReward,
}

/// The bar's own state: the headless [`BarState`] plus what egui needs.
#[derive(Debug, Default)]
pub struct ContextBar {
    open: Popover,
    pub state: BarState,
    /// Screen rect of the slot each popover hangs from.
    anchor: Option<egui::Rect>,
    /// The delete tooltip, which names the object. Cached rather than
    /// formatted per frame: the bar is up for as long as something is
    /// selected, and this sentence only changes when the tool does.
    delete_hover: String,
    delete_hover_for: &'static str,
    /// The open popover's Area — see [`Self::owns_press`].
    popover_layer: Option<egui::Id>,
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
    /// [`BarState::suppress_gesture`]; a popover cannot outlive its bar.
    pub fn suppress_gesture(&mut self) {
        self.state.suppress_gesture();
        self.open = Popover::None;
    }

    /// [`BarState::note_screen`]; a resize closes the popover.
    pub fn note_screen(&mut self, screen: egui::Rect, now_ms: u64) {
        if self.state.note_screen(px_rect(screen), now_ms) {
            self.open = Popover::None;
        }
    }

    /// [`BarState::suppress_transient`], closing the popover.
    pub fn suppress_transient(&mut self, now_ms: u64) {
        self.state.suppress_transient(now_ms);
        self.open = Popover::None;
    }

    /// [`BarState::note_selection`]; a new object closes the popover.
    pub fn note_selection(&mut self, selection: Option<usize>) -> bool {
        let changed = self.state.note_selection(selection);
        if changed {
            self.open = Popover::None;
        }
        changed
    }

    /// [`BarState::step_dismissal`], answering whether the bar stands
    /// dismissed; dismissing closes the popover.
    pub fn step_dismissal(
        &mut self,
        target: Option<BarTarget>,
        focused: bool,
        press: Option<Press>,
    ) -> bool {
        if self.state.step_dismissal(target, focused, press) {
            self.open = Popover::None;
        }
        self.state.dismissed()
    }

    /// [`BarState::owns_press`] against the last rect, or the open popover's
    /// `layer`.
    pub fn owns_press(
        &mut self,
        press: Option<(egui::Pos2, f64)>,
        layer: Option<egui::LayerId>,
    ) -> bool {
        let popover = self.popover_layer.filter(|_| self.open != Popover::None);
        let on_popover = layer.is_some_and(|layer| Some(layer.id) == popover);
        (self.state).owns_press(press.map(|(at, t)| (px(at), t)), on_popover)
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
    /// A position's colour, opacity, ratio or default, from its own menus.
    pub position: Option<PositionAsk>,
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
    /// A long or short position's colours and risk/reward menu.
    pub position: Option<PositionFace>,
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
        magnet: object.magnet.is_some(),
        draw_again: object.draw_again.is_some(),
        position: object.position.is_some(),
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
    bar.state.set_rect(px_rect(rect));
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
        Slot::TargetColor => leg_slot(ui, bar, object, PositionLeg::Target, "Target colour"),
        Slot::StopColor => leg_slot(ui, bar, object, PositionLeg::Stop, "Stop colour"),
        Slot::RiskReward => draw_ratio_slot(ui, bar, object),
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
            let sharing = IconButton::new(icons::GRID_FOUR, TOOLRAIL_ICON)
                .active(object.shared)
                .enabled(object.shareable)
                .hover_text(SHARING_HOVER[usize::from(object.shared)])
                .disabled_explanation(UNSHAREABLE_REASON)
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
    let hover = hover_text(
        object,
        "Colour",
        "Colour - the lock protects position, not style",
    );
    swatch_slot(ui, bar, object.style.color32(), Popover::Color, hover);
}

fn swatch_slot(
    ui: &mut egui::Ui,
    bar: &mut ContextBar,
    color: egui::Color32,
    popover: Popover,
    hover: &'static str,
) {
    let palette = theme::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(TOOLRAIL_ICON.hit, TOOLRAIL_ICON.hit),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let swatch =
            egui::Rect::from_center_size(rect.center(), egui::vec2(SWATCH_SIDE_PX, SWATCH_SIDE_PX));
        let painter = ui.painter();
        painter.rect_filled(swatch, egui::Rounding::same(4.0), color);
        painter.rect_stroke(
            swatch,
            egui::Rounding::same(4.0),
            egui::Stroke::new(1.0_f32, palette.border()),
        );
    }
    #[cfg(test)]
    if popover == Popover::Color {
        bar.probe.color = Some(rect);
    }
    if response.on_hover_text(hover).clicked() {
        bar.toggle(popover, rect);
    }
}

fn leg_slot(
    ui: &mut egui::Ui,
    bar: &mut ContextBar,
    object: &BarObject<'_>,
    leg: PositionLeg,
    hover: &'static str,
) {
    if let Some(face) = object.position.as_ref() {
        swatch_slot(ui, bar, leg_color(face, leg), Popover::Leg(leg), hover);
    }
}

fn leg_color(face: &PositionFace, leg: PositionLeg) -> egui::Color32 {
    let [r, g, b] = match leg {
        PositionLeg::Target => face.target_rgb,
        PositionLeg::Stop => face.stop_rgb,
    };
    egui::Color32::from_rgb(r, g, b)
}

/// `R/R 1:2` in one colour.
fn draw_ratio_slot(ui: &mut egui::Ui, bar: &mut ContextBar, object: &BarObject<'_>) {
    let Some(face) = object.position.as_ref() else {
        return;
    };
    let palette = theme::get(ui.ctx());
    let size = egui::vec2(RISK_REWARD_WIDTH_PX, TOOLRAIL_ICON.hit);
    let Some(button) = face.ratio.as_ref().map(|menu| &menu.button) else {
        ui.add_sized(size, egui::Label::new("R/R –"))
            .on_hover_text("A flat plan states no risk/reward");
        return;
    };
    let text = egui::RichText::new(button)
        .font(palette.body_font(12.0))
        .color(palette.text_primary());
    let response = ui
        .add_sized(size, egui::Button::new(text).frame(false))
        .on_hover_text("Risk/reward");
    if response.clicked() {
        bar.toggle(Popover::RiskReward, response.rect);
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

fn popover_position(anchor: egui::Rect, size: egui::Vec2, bounds: egui::Rect) -> egui::Pos2 {
    let (anchor, bounds) = (px_rect(anchor), px_rect(bounds));
    pos(placement::popover_position(
        anchor,
        px(size.to_pos2()),
        bounds,
    ))
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
        popover,
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
                    Popover::Leg(leg) => draw_leg_popover(ui, object, leg, intent),
                    Popover::RiskReward => draw_ratio_popover(ui, object, intent),
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
        && !(bar.state.last_rect())
            .is_some_and(|rect| rect.contains(px(ctx.pointer_latest_pos().unwrap_or_default())))
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
    let current = object.style.color32();
    if let Some(swatch) = draw_palette(
        ui,
        current,
        #[cfg(test)]
        &mut probe.swatches,
    ) {
        object.style.set_color32(swatch);
        intent.edited = true;
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

/// The theme's swatches over the primaries, answering the one clicked.
fn draw_palette(
    ui: &mut egui::Ui,
    current: egui::Color32,
    #[cfg(test)] rects: &mut Vec<egui::Rect>,
) -> Option<egui::Color32> {
    let palette = theme::get(ui.ctx());
    // Both axes: the selected ring sits outside its swatch.
    ui.spacing_mut().item_spacing = egui::vec2(ITEM_GAP_PX, ITEM_GAP_PX);
    let primaries = quantick_appearance::palette::DRAWING_PRIMARIES.map(super::to_color32);
    let rows = [&palette.drawing_swatches()[..], &primaries[..]];
    rows.into_iter().fold(None, |picked, row| {
        let clicked = draw_swatch_row(
            ui,
            row,
            current,
            #[cfg(test)]
            rects,
        );
        clicked.or(picked)
    })
}

/// One row of swatches, answering the one clicked.
fn draw_swatch_row(
    ui: &mut egui::Ui,
    swatches: &[egui::Color32],
    current: egui::Color32,
    #[cfg(test)] rects: &mut Vec<egui::Rect>,
) -> Option<egui::Color32> {
    let palette = theme::get(ui.ctx());
    let mut picked = None;
    ui.horizontal(|ui| {
        for &swatch in swatches {
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(PALETTE_SWATCH_PX, PALETTE_SWATCH_PX),
                egui::Sense::click(),
            );
            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                painter.rect_filled(rect, egui::Rounding::same(4.0), swatch);
                if current == swatch {
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
                picked = Some(swatch);
            }
        }
    });
    picked
}

/// A position zone's palette and opacity, then the tool's default.
fn draw_leg_popover(
    ui: &mut egui::Ui,
    object: &BarObject<'_>,
    leg: PositionLeg,
    intent: &mut ContextBarIntent,
) {
    let Some(face) = object.position.as_ref() else {
        return;
    };
    if let Some(swatch) = draw_palette(
        ui,
        leg_color(face, leg),
        #[cfg(test)]
        &mut Vec::new(),
    ) {
        let [r, g, b, _] = swatch.to_srgba_unmultiplied();
        intent.position = Some(PositionAsk::Edit(PositionEdit::Colour(leg, [r, g, b])));
    }
    let mut pct = face.zone_opacity_pct;
    let max = quantick_drawings::MAX_ZONE_OPACITY_PCT;
    let opacity = egui::Slider::new(&mut pct, 0..=max)
        .suffix(" %")
        .text("Opacity");
    if ui.add(opacity).changed() {
        intent.position = Some(PositionAsk::Edit(PositionEdit::ZoneOpacity(pct)));
    }
    draw_default_rows(ui, intent);
}

/// The risk/reward rows: a checked, read-only custom row on top while the
/// plan is no preset, then the presets.
fn draw_ratio_popover(ui: &mut egui::Ui, object: &BarObject<'_>, intent: &mut ContextBarIntent) {
    let Some(face) = object.position.as_ref() else {
        return;
    };
    for row in face.ratio.iter().flat_map(|menu| &menu.rows) {
        let label = egui::SelectableLabel::new(row.checked, row.label.as_str());
        // A lock keeps the prices: the presets move the target.
        if ui
            .add_enabled(row.pick.is_some() && !object.locked, label)
            .clicked()
            && let Some(ratio) = row.pick
        {
            intent.position = Some(PositionAsk::Edit(PositionEdit::Ratio(ratio)));
        }
    }
    draw_default_rows(ui, intent);
}

fn draw_default_rows(ui: &mut egui::Ui, intent: &mut ContextBarIntent) {
    ui.separator();
    for (label, hover, action) in [
        (
            "Save as default",
            "New positions of this side open with these colours, opacity and risk/reward",
            PositionDefault::Save,
        ),
        (
            "Reset",
            "New positions of this side open with the factory look again",
            PositionDefault::Reset,
        ),
    ] {
        if ui.button(label).on_hover_text(hover).clicked() {
            intent.position = Some(PositionAsk::Default(action));
        }
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

    #[must_use]
    pub fn manual_position(&self) -> Option<egui::Pos2> {
        self.state.manual().map(pos)
    }

    pub fn set_manual(&mut self, position: egui::Pos2) {
        self.state.set_manual(px(position));
    }

    pub fn clear_manual(&mut self) -> bool {
        self.state.clear_manual()
    }

    #[must_use]
    pub fn last_rect(&self) -> Option<egui::Rect> {
        self.state.last_rect().map(super::geometry::egui_rect)
    }
}

#[cfg(test)]
mod tests;
