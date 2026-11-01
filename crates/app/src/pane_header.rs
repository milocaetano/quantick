//! Paints the header line every pane carries (the rule, the mark, the bars and
//! the indicators button on the left; the view and the strip on a flow pane's
//! right) and the capsule floating over each chart side. What they say and what
//! a press asks for are `quantick_chart::pane_header`'s call; this file holds
//! the shapes and the pointer.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_chart::pane_header::{
    BubbleSample, CAPSULE_HEIGHT_PX, CAPSULE_SLOT_HEIGHT_PX, Capsule, CapsuleItem, CapsuleSide,
    FLOW_CONTROLS_PX, FlowHeaderState, GLYPH_SLOT_PX, GROUP_GAP_PX, GlyphInk, HeaderControl,
    HeaderIntent, INDICATORS_GLYPH, INDICATORS_LABEL, LayerGlyph, LayerToggle, PaneHeader,
    SAMPLE_RESET_SCALE, SLOT_GAP_PX, VIEW_SLOT_PX, dragged_scale, flow_fit, sample_diameter_px,
    sample_value_text, view_name,
};
use quantick_engine::bar_selection::{BarInputAvailability, BarSelection};
use quantick_orderflow::LaneView;
use smallvec::SmallVec;

use crate::chart_layers::shortcuts;
use crate::theme;

/// Padding inside the strip: clears the pane's edge and keeps the controls off
/// the rule below.
pub const CONTENT_PADDING: egui::Vec2 = egui::vec2(10.0, 2.0);

/// The mark: 11 pt monospace with .08 em of letter spacing.
const MARK_SIZE: f32 = 11.0;
const MARK_LETTER_SPACING: f32 = 0.88;
/// The bars text and its faint caret.
const BARS_SIZE: f32 = 13.0;
const CARET_SIZE: f32 = 10.0;
/// egui's bundled fonts have one weight; the text painted again this far right
/// reads as the semibold the header asks for.
const SEMIBOLD_OFFSET_PX: f32 = 0.4;
/// The indicators button: 12 pt words, an icon and its gap, its padding.
const INDICATORS_SIZE: f32 = 12.0;
const INDICATORS_ICON_GAP_PX: f32 = 5.0;
const INDICATORS_PADDING: egui::Vec2 = egui::vec2(7.0, 3.0);

/// Corner radii: the capsule's, and a control's hover patch.
const CAPSULE_ROUNDING: f32 = 6.0;
const HOVER_ROUNDING: f32 = 4.0;
/// How far past the capsule its shadow paints.
const CAPSULE_SHADOW_ROOM_PX: f32 = 6.0;
/// Side of a layer glyph's drawing box, the unit its shapes are written in.
const GLYPH_BOX: f32 = 16.0;
/// The slash through a glyph switched off by hand, corner to corner.
const SLASH_PX: f32 = 1.8;
/// The outlined sample of bubbles switched off by hand.
const SAMPLE_OFF_DIAMETER_PX: f32 = 12.0;
const SAMPLE_OFF_STROKE_PX: f32 = 1.2;
/// The view glyph's drawing box: both sides of the pane, wider than tall.
const VIEW_BOX: egui::Vec2 = egui::vec2(30.0, 22.0);
/// How high the sample's value floats over the chart below it.
const VALUE_GAP_PX: f32 = 4.0;

/// What the header painted this frame, in reading order: the scene reads it,
/// so it lists a control only while a hand can reach it.
pub type PaintedControls = SmallVec<[(HeaderControl, egui::Rect); LayerToggle::ALL.len() + 3]>;

/// The rule closing the strip against the chart below it.
pub fn paint_rule(ui: &egui::Ui, strip: egui::Rect) {
    ui.painter().line_segment(
        [
            egui::pos2(strip.left(), strip.bottom()),
            egui::pos2(strip.right(), strip.bottom()),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
}

/// The `ƒx` icon and the words, as the button writes them.
fn indicators_text(words: egui::Color32) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    let icon = egui::TextFormat {
        italics: true,
        ..egui::TextFormat::simple(egui::FontId::monospace(INDICATORS_SIZE), theme::TEXT_MUTED)
    };
    text.append(INDICATORS_GLYPH, 0.0, icon);
    let font = egui::FontId::proportional(INDICATORS_SIZE);
    let words = egui::TextFormat::simple(font, words);
    text.append(INDICATORS_LABEL, INDICATORS_ICON_GAP_PX, words);
    text
}

/// How wide the indicators button is, before it is laid out.
pub fn indicators_width(ui: &egui::Ui) -> f32 {
    let galley = ui.fonts(|fonts| fonts.layout_job(indicators_text(theme::PANE_ACTION)));
    galley.size().x + 2.0 * INDICATORS_PADDING.x
}

/// The indicators button with its left edge at `left`, centred on the strip;
/// answers whether it was clicked.
pub fn indicators_button(ui: &egui::Ui, left: f32, strip: egui::Rect, salt: u64) -> bool {
    let size = egui::vec2(indicators_width(ui), 2.0 * (INDICATORS_SIZE - 1.0));
    let center = egui::pos2(left + size.x / 2.0, strip.center().y);
    let rect = egui::Rect::from_center_size(center, size);
    let id = ui.id().with(("pane_indicators", salt));
    let response = ui.interact(rect, id, egui::Sense::click());
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(indicators_id(salt), rect));
    let hovered = response.hovered();
    let words = if hovered {
        theme::TEXT_PRIMARY
    } else {
        theme::PANE_ACTION
    };
    if hovered {
        let rounding = egui::Rounding::same(HOVER_ROUNDING);
        ui.painter().rect_filled(rect, rounding, theme::BARE_HOVER);
    }
    let galley = ui.fonts(|fonts| fonts.layout_job(indicators_text(words)));
    let at = rect.center() - galley.size() / 2.0;
    ui.painter().galley(at, galley, words);
    response.clicked()
}

/// What a flow pane's row draws besides its bars: `None` without a tape.
pub struct FlowRow {
    /// The strip glyph, unless the data blocks it.
    pub strip: Option<LayerGlyph>,
    pub state: Option<FlowHeaderState>,
}

/// The flow pane's header line; answers what a press on it asked for, and
/// whether its indicators button was clicked.
#[allow(clippy::too_many_arguments)]
pub fn draw_marked(
    ui: &mut egui::Ui,
    strip: egui::Rect,
    header: &PaneHeader,
    salt: u64,
    selection: &mut BarSelection,
    inputs: BarInputAvailability,
    row: FlowRow,
    painted: &mut PaintedControls,
) -> (Option<HeaderIntent>, bool) {
    paint_rule(ui, strip);
    painted.clear();
    let inner = strip.shrink2(CONTENT_PADDING);
    let indicators_px = indicators_width(ui);
    // The indicators button outranks the right end: on a pane too narrow for
    // both, it is the one a hand can still reach.
    let state = row
        .state
        .filter(|_| inner.width() >= indicators_px + GROUP_GAP_PX + FLOW_CONTROLS_PX);
    let right_px = state.map_or(0.0, |_| FLOW_CONTROLS_PX + GROUP_GAP_PX);
    let left = inner.with_max_x(inner.right() - right_px);
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("pane_header", salt))
            .max_rect(left)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let limit = (left.right() - indicators_px).max(strip.left());
    content.set_clip_rect(strip.with_max_x(limit).intersect(ui.clip_rect()));
    content.spacing_mut().item_spacing.x = 0.0;
    let mark = header.mark.map(mark_text);
    let mark_px = mark.as_ref().map_or(0.0, |job| {
        content
            .fonts(|fonts| fonts.layout_job(job.clone()))
            .size()
            .x
    });
    let fit = flow_fit(
        inner.width(),
        mark_px,
        bars_width(&content, header),
        indicators_px,
    );
    if let Some(mark) = mark.filter(|_| fit.mark) {
        content.add(egui::Label::new(mark).selectable(false));
        content.add_space(GROUP_GAP_PX);
    }
    bars_control(&mut content, header, salt, selection, inputs);
    let button_left = (content.cursor().left() + GROUP_GAP_PX).min(limit);
    let indicators = indicators_button(ui, button_left, strip, salt);
    let mut intent = None;
    if let Some(state) = state {
        let room = inner.with_min_x(inner.right() - FLOW_CONTROLS_PX);
        let mut right = clipped_child(ui, room, strip, ("pane_header_right", salt));
        let height = inner.y_range();
        let view_right = room.left() + VIEW_SLOT_PX;
        let rect = egui::Rect::from_x_y_ranges(room.left()..=view_right, height);
        view_button(&mut right, rect, salt, state, painted, &mut intent);
        if let Some(glyph) = row.strip {
            let glyph_left = inner.right() - GLYPH_SLOT_PX;
            let rect = egui::Rect::from_x_y_ranges(glyph_left..=inner.right(), height);
            layer_glyph(&right, rect, salt, glyph, painted, &mut intent);
        }
    }
    (intent, indicators)
}

/// A child ui over `rect` whose paint stays inside `clip`, so what it draws is
/// told from the chart below by its clip.
fn clipped_child(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    clip: egui::Rect,
    salt: impl std::hash::Hash,
) -> egui::Ui {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(salt).max_rect(rect));
    child.set_clip_rect(clip.intersect(ui.clip_rect()));
    child
}

/// One capsule with its corner at `corner`: its backdrop takes the pointer so a
/// press between the slots never reaches the chart. Registered after the chart,
/// so it wins the press. Answers the capsule's rect.
pub fn draw_capsule(
    ui: &mut egui::Ui,
    capsule: &Capsule,
    corner: (f32, f32),
    side: CapsuleSide,
    salt: u64,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) -> egui::Rect {
    let rect = egui::Rect::from_min_size(
        egui::pos2(corner.0, corner.1),
        egui::vec2(capsule.width_px(), CAPSULE_HEIGHT_PX),
    );
    let clip = rect.expand(CAPSULE_SHADOW_ROOM_PX);
    let ui = clipped_child(ui, rect, clip, ("pane_capsule", salt, side as u8));
    let _ = ui.interact(rect, ui.id(), egui::Sense::click_and_drag());
    let rounding = egui::Rounding::same(CAPSULE_ROUNDING);
    let shadow = egui::Shadow {
        offset: egui::vec2(0.0, 1.0),
        blur: 4.0,
        spread: 0.0,
        color: theme::CAPSULE_SHADOW,
    };
    let painter = ui.painter();
    painter.add(shadow.as_shape(rect, rounding));
    painter.rect(
        rect,
        rounding,
        theme::CAPSULE_FILL,
        egui::Stroke::new(1.0_f32, theme::CAPSULE_BORDER),
    );
    let (mut x, y) = Capsule::slot_origin(corner.0, corner.1);
    for item in capsule.items() {
        let slot = egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(item.slot_px(), CAPSULE_SLOT_HEIGHT_PX),
        );
        match item {
            CapsuleItem::Layer(glyph) => layer_glyph(&ui, slot, salt, glyph, painted, intent),
            CapsuleItem::Sample(which, scale) => {
                sample(&ui, slot, salt, which, scale, painted, intent);
            }
            CapsuleItem::SampleOff(_) => {
                painter.circle_stroke(
                    slot.center(),
                    SAMPLE_OFF_DIAMETER_PX / 2.0,
                    egui::Stroke::new(SAMPLE_OFF_STROKE_PX, theme::OFF),
                );
            }
        }
        x = slot.right() + SLOT_GAP_PX;
    }
    rect
}

/// One sample bubble at its current size: drag up to grow, double click for
/// `1x`, the value on show only while dragging.
fn sample(
    ui: &egui::Ui,
    rect: egui::Rect,
    salt: u64,
    which: BubbleSample,
    scale: f32,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let id = ui.id().with(("pane_header_sample", salt, which as u8));
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.drag_started() {
        ui.ctx().data_mut(|data| data.insert_temp(id, scale));
    }
    let mut shown = scale;
    if response.dragged() {
        let pressed_at = ui.ctx().data(|data| data.get_temp(id)).unwrap_or(scale);
        let up_px = ui.input(|input| {
            let pointer = &input.pointer;
            pointer
                .press_origin()
                .zip(pointer.latest_pos())
                .map_or(0.0, |(from, now)| from.y - now.y)
        });
        shown = dragged_scale(pressed_at, up_px);
        paint_value(ui, id, rect, shown);
    }
    if response.double_clicked() {
        shown = SAMPLE_RESET_SCALE;
    }
    if shown != scale {
        *intent = Some(HeaderIntent::SetSample(which, shown));
    }
    let colour = match which {
        BubbleSample::Candle => theme::ACCENT,
        BubbleSample::Tape => theme::BUY,
    };
    ui.painter()
        .circle_filled(rect.center(), sample_diameter_px(shown) / 2.0, colour);
    record(ui, HeaderControl::Sample(which), rect, painted);
    response.on_hover_text(which.name());
}

/// The sample's value, floating just below its slot so nothing shifts.
fn paint_value(ui: &egui::Ui, id: egui::Id, rect: egui::Rect, scale: f32) {
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(egui::Order::Tooltip, id));
    let galley = painter.layout_no_wrap(
        sample_value_text(scale),
        egui::FontId::monospace(11.0),
        theme::TEXT_PRIMARY,
    );
    let at = egui::pos2(
        rect.center().x - galley.size().x / 2.0,
        rect.bottom() + VALUE_GAP_PX,
    );
    painter.rect_filled(
        egui::Rect::from_min_size(at, galley.size()).expand(2.0),
        egui::Rounding::same(2.0),
        theme::CHROME,
    );
    painter.galley(at, galley, theme::TEXT_PRIMARY);
}

/// One layer's borderless toggle in `rect`: its shape, lit, or outlined and
/// slashed. Left click switches, right click opens what configures it: looking
/// is not enabling.
pub fn layer_glyph(
    ui: &egui::Ui,
    rect: egui::Rect,
    salt: u64,
    glyph: LayerGlyph,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let id = ui.id().with(("pane_layer_glyph", salt, glyph.toggle as u8));
    let response = ui.interact(rect, id, egui::Sense::click());
    if response.clicked()
        && let Some(asked) = glyph.click()
    {
        *intent = Some(asked);
    }
    if response.secondary_clicked() {
        *intent = Some(HeaderIntent::OpenLayerSettings(glyph.toggle));
    }
    if response.hovered() {
        let patch = egui::Rect::from_center_size(
            rect.center(),
            egui::Vec2::splat(GLYPH_SLOT_PX.min(rect.height())),
        );
        ui.painter().rect_filled(
            patch,
            egui::Rounding::same(HOVER_ROUNDING),
            theme::BARE_HOVER,
        );
    }
    let boxed = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(GLYPH_BOX));
    paint_layer(ui.painter(), boxed, glyph.toggle, glyph.ink());
    record(ui, HeaderControl::Layer(glyph.toggle), rect, painted);
    response.on_hover_ui(|ui| {
        let key = shortcuts::binding(glyph.toggle.layer());
        ui.label(with_key(ui.ctx(), glyph.toggle.name(), key));
    });
}

/// The button that walks the stored views; its glyph is the side on screen.
fn view_button(
    ui: &egui::Ui,
    rect: egui::Rect,
    salt: u64,
    state: FlowHeaderState,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let id = ui.id().with(("pane_view_button", salt));
    let response = ui.interact(rect, id, egui::Sense::click());
    if response.clicked() {
        *intent = Some(HeaderIntent::SetView(state.view.next()));
    }
    paint_view(ui.painter(), rect, state.shown_view, theme::ACCENT);
    record(ui, HeaderControl::View, rect, painted);
    response.on_hover_ui(|ui| {
        let key = Some(shortcuts::TAPE_VIEW_SHORTCUT);
        ui.label(with_key(ui.ctx(), view_name(state.shown_view), key));
    });
}

fn record(ui: &egui::Ui, control: HeaderControl, rect: egui::Rect, painted: &mut PaintedControls) {
    // A control the strip clipped, even in part, is not listed: a hand or a
    // script aiming at the middle of its rect could miss it.
    if ui.is_visible() && ui.clip_rect().contains_rect(rect) {
        painted.push((control, rect));
    }
}

fn with_key(ctx: &egui::Context, name: &str, key: Option<egui::KeyboardShortcut>) -> String {
    key.map_or_else(
        || name.to_owned(),
        |key| format!("{name} ({})", ctx.format_shortcut(&key)),
    )
}

/// Shapes written in a glyph's own drawing box, scaled onto the slot. Each
/// glyph is a small copy of what its layer draws; a lit one fills its marks and
/// an off one is outlined and slashed, so on and off survive a greyscale reading.
struct Pen<'a> {
    painter: &'a egui::Painter,
    origin: egui::Pos2,
    unit: f32,
    colour: egui::Color32,
    filled: bool,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> egui::Pos2 {
        self.origin + egui::vec2(x, y) * self.unit
    }

    fn stroke(&self) -> egui::Stroke {
        egui::Stroke::new(1.0_f32, self.colour)
    }

    fn line(&self, from: (f32, f32), to: (f32, f32)) {
        self.painter.line_segment(
            [self.at(from.0, from.1), self.at(to.0, to.1)],
            self.stroke(),
        );
    }

    fn dot(&self, x: f32, y: f32, radius: f32) {
        let (center, radius) = (self.at(x, y), radius * self.unit);
        if self.filled {
            self.painter.circle_filled(center, radius, self.colour);
        } else {
            self.painter.circle_stroke(center, radius, self.stroke());
        }
    }

    fn rect(&self, x: f32, y: f32, width: f32, height: f32) -> egui::Rect {
        egui::Rect::from_min_max(self.at(x, y), self.at(x + width, y + height))
    }

    fn outline(&self, x: f32, y: f32, width: f32, height: f32) {
        self.painter.rect_stroke(
            self.rect(x, y, width, height),
            egui::Rounding::same(1.0),
            self.stroke(),
        );
    }

    /// A band of the given strength, drawn only while the glyph is lit.
    fn band(&self, x: f32, y: f32, width: f32, height: f32, strength: f32) {
        if self.filled {
            self.painter.rect_filled(
                self.rect(x, y, width, height),
                egui::Rounding::ZERO,
                self.colour.gamma_multiply(strength),
            );
        }
    }

    /// A candle body: the only mark the view glyphs share with the chart.
    fn body(&self, x: f32, y: f32, width: f32, height: f32) {
        self.outline(x, y, width, height);
        self.band(x, y, width, height, 1.0);
    }
}

fn paint_layer(painter: &egui::Painter, boxed: egui::Rect, toggle: LayerToggle, ink: GlyphInk) {
    let lit = ink == GlyphInk::Lit;
    let colour = if lit { theme::ACCENT } else { theme::OFF };
    let pen = Pen {
        painter,
        origin: boxed.min,
        unit: boxed.width() / GLYPH_BOX,
        colour,
        filled: lit,
    };
    match toggle {
        // Three circles: the prints themselves.
        LayerToggle::Bubbles | LayerToggle::TapeBubbles => {
            pen.dot(5.0, 9.5, 3.2);
            pen.dot(10.5, 4.5, 2.1);
            pen.dot(11.5, 11.0, 1.3);
        }
        // A block with three fading bands: the book's rows.
        LayerToggle::Heatmap | LayerToggle::TapeHeatmap => {
            pen.band(1.5, 1.5, 12.0, 3.0, 1.0);
            pen.band(1.5, 6.0, 12.0, 3.0, 0.45);
            pen.band(1.5, 10.5, 12.0, 3.0, 0.2);
            pen.outline(1.5, 1.5, 12.0, 12.0);
        }
        // A candle split in two columns of ticks: the footprint inside a bar.
        LayerToggle::Footprint => {
            pen.line((7.5, 1.0), (7.5, 14.0));
            pen.outline(3.0, 2.5, 9.0, 10.0);
            for y in [5.0, 7.5, 10.0] {
                pen.line((4.5, y), (6.0, y));
                pen.line((9.0, y), (10.5, y));
            }
        }
        // Four narrow columns: the strip beside the price axis.
        LayerToggle::LiveStrip => {
            for x in [3.0, 6.0, 9.0, 12.0] {
                pen.line((x, 2.0), (x, 14.0));
            }
        }
    }
    if !lit {
        painter.line_segment(
            [pen.at(2.0, 14.0), pen.at(14.0, 2.0)],
            egui::Stroke::new(SLASH_PX, colour),
        );
    }
}

fn paint_view(painter: &egui::Painter, slot: egui::Rect, view: LaneView, colour: egui::Color32) {
    let unit = (slot.width() / VIEW_BOX.x).min(slot.height() / VIEW_BOX.y);
    let boxed = egui::Rect::from_center_size(slot.center(), VIEW_BOX * unit);
    let pen = Pen {
        painter,
        origin: boxed.min,
        unit,
        colour,
        filled: true,
    };
    match view {
        LaneView::Beside => {
            pen.line((7.0, 2.0), (7.0, 20.0));
            pen.body(4.0, 5.0, 6.0, 11.0);
            pen.line((15.0, 2.0), (15.0, 20.0));
            pen.dot(23.0, 4.0, 1.5);
            pen.dot(23.0, 9.5, 2.4);
            pen.dot(23.0, 15.5, 1.8);
            pen.dot(23.0, 19.5, 1.1);
        }
        LaneView::CandlesOnly => {
            pen.line((9.0, 2.0), (9.0, 20.0));
            pen.body(6.0, 5.0, 6.0, 11.0);
            pen.line((19.0, 4.0), (19.0, 18.0));
            pen.body(16.0, 6.5, 6.0, 7.5);
        }
        LaneView::TapeOnly => {
            pen.dot(9.0, 4.0, 1.5);
            pen.dot(15.0, 9.5, 2.6);
            pen.dot(21.0, 5.5, 1.4);
            pen.dot(10.0, 16.0, 2.0);
            pen.dot(20.0, 15.5, 1.7);
        }
    }
}

/// The mark as the line writes it: gold monospace, spaced out.
fn mark_text(mark: &str) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        mark,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::monospace(MARK_SIZE),
            color: theme::PANE_MARK,
            extra_letter_spacing: MARK_LETTER_SPACING,
            ..Default::default()
        },
    );
    text
}

/// The bars as words in full ink with a faint caret, as the control lays them
/// out.
fn bars_text(header: &PaneHeader) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        &header.bars_text,
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(BARS_SIZE), theme::TEXT_PRIMARY),
    );
    text.append(
        icons::CARET_DOWN,
        3.0,
        egui::TextFormat::simple(
            egui::FontId::proportional(CARET_SIZE),
            theme::TEXT_FAINT.gamma_multiply(0.6),
        ),
    );
    text
}

/// How wide the bars control will be, before it is laid out.
fn bars_width(ui: &egui::Ui, header: &PaneHeader) -> f32 {
    let galley = ui.fonts(|fonts| fonts.layout_job(bars_text(header)));
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

/// The bars as words with a faint caret; a click opens this pane's bars menu.
pub fn bars_control(
    ui: &mut egui::Ui,
    header: &PaneHeader,
    salt: u64,
    selection: &mut BarSelection,
    inputs: BarInputAvailability,
) {
    let galley = ui.fonts(|fonts| fonts.layout_job(bars_text(header)));
    let padding = ui.spacing().button_padding;
    let (rect, control) = ui.allocate_exact_size(
        galley.size() + egui::vec2(2.0 * padding.x, 2.0 * padding.y),
        egui::Sense::click(),
    );
    if control.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::same(HOVER_ROUNDING),
            theme::BARE_HOVER,
        );
    }
    let at = rect.min + padding;
    ui.painter().galley(
        at + egui::vec2(SEMIBOLD_OFFSET_PX, 0.0),
        galley.clone(),
        theme::TEXT_PRIMARY,
    );
    ui.painter().galley(at, galley, theme::TEXT_PRIMARY);
    let control = control.on_hover_text("Change this chart's bars");
    let popup = menu_id(salt);
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(popup, control.rect));
    if control.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup));
    }
    egui::popup_below_widget(
        ui,
        popup,
        &control,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(control.rect.width().max(120.0));
            crate::toolbar::bar_parameters::draw_kinds(ui, selection, inputs);
            ui.separator();
            ui.horizontal(|ui| crate::toolbar::bar_parameters::draw(ui, selection, inputs));
        },
    );
}

/// The popup id of the bars menu of the pane keyed by `salt`.
pub(crate) fn menu_id(salt: u64) -> egui::Id {
    egui::Id::new(("pane_bars_menu", salt))
}

/// Where the tests read back the indicators button of the pane keyed by `salt`.
#[cfg(test)]
pub(crate) fn indicators_id(salt: u64) -> egui::Id {
    egui::Id::new(("pane_indicators_rect", salt))
}
