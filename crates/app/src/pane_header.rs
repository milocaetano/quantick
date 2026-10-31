//! Paints the header line every pane carries: the rule, the mark, the bars
//! control, and the glyphs and sample bubbles beside it. What the line says and
//! what a press asks for are `quantick_chart::pane_header`'s call; this file
//! holds the shapes and the pointer.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_chart::pane_header::{
    BubbleSample, FlowHeaderState, GLYPH_SLOT_PX, GROUP_GAP_PX, GlyphInk, HeaderControl,
    HeaderIntent, LayerGlyph, LayerToggle, PaneHeader, SAMPLE_RESET_SCALE, SAMPLE_SLOT_PX,
    SLOT_GAP_PX, VIEW_SLOT_PX, dragged_scale, flow_fit, sample_diameter_px, sample_value_text,
    view_name,
};
use quantick_engine::bar_selection::{BarInputAvailability, BarSelection};
use quantick_orderflow::LaneView;
use smallvec::SmallVec;

use crate::chart_layers::shortcuts;
use crate::theme;

/// Padding inside the strip: clears the pane's edge and keeps the controls off
/// the rule below.
pub const CONTENT_PADDING: egui::Vec2 = egui::vec2(8.0, 2.0);

/// Side of a layer glyph's drawing box, the unit its shapes are written in.
const GLYPH_BOX: f32 = 15.0;
/// The view glyph's drawing box: both sides of the pane, wider than tall.
const VIEW_BOX: egui::Vec2 = egui::vec2(30.0, 22.0);
/// How a slot's ink dims: off and resting, off under the pointer, blocked.
const OFF_INK: f32 = 0.45;
const OFF_HOVER_INK: f32 = 0.85;
const BLOCKED_INK: f32 = 0.22;
/// How high the sample's value floats over the chart below it.
const VALUE_GAP_PX: f32 = 4.0;

/// What the header painted this frame, in reading order: the scene reads it,
/// so it lists a control only while a hand can reach it.
pub type PaintedControls = SmallVec<[(HeaderControl, egui::Rect); 6]>;

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

/// What a flow pane's row draws besides its bars.
pub struct FlowRow<'a> {
    pub glyphs: &'a [LayerGlyph],
    pub state: Option<FlowHeaderState>,
}

/// The flow pane's header line; answers what a press on it asked for.
#[allow(clippy::too_many_arguments)]
pub fn draw_marked(
    ui: &mut egui::Ui,
    strip: egui::Rect,
    header: &PaneHeader,
    salt: u64,
    selection: &mut BarSelection,
    inputs: BarInputAvailability,
    row: FlowRow<'_>,
    painted: &mut PaintedControls,
) -> Option<HeaderIntent> {
    paint_rule(ui, strip);
    painted.clear();
    let inner = strip.shrink2(CONTENT_PADDING);
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("pane_header", salt))
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    content.set_clip_rect(strip.intersect(ui.clip_rect()));
    content.spacing_mut().item_spacing.x = SLOT_GAP_PX;
    let mark_font = egui::TextStyle::Small.resolve(content.style());
    let mark_font = egui::FontId::monospace(mark_font.size);
    let mark_px = header.mark.map_or(0.0, |mark| {
        let colour = theme::PANE_MARK;
        let galley = content
            .painter()
            .layout_no_wrap(mark.to_owned(), mark_font, colour);
        galley.size().x
    });
    let bars_px = bars_width(&content, header);
    let fit = flow_fit(inner.width(), mark_px, bars_px);
    if let Some(mark) = header.mark.filter(|_| fit.mark) {
        content.label(
            egui::RichText::new(mark)
                .monospace()
                .small()
                .color(theme::PANE_MARK),
        );
        content.add_space(GROUP_GAP_PX);
    }
    bars_control(&mut content, header, salt, selection, inputs);
    let mut intent = None;
    if let Some(state) = row.state {
        content.add_space(GROUP_GAP_PX);
        view_button(&mut content, state.view, painted, &mut intent);
        content.add_space(GROUP_GAP_PX);
        for glyph in row.glyphs {
            layer_glyph(&mut content, *glyph, painted, &mut intent);
            if glyph.toggle == LayerToggle::Bubbles && fit.samples {
                candle_slot(
                    &mut content,
                    salt,
                    state.candle_sample,
                    painted,
                    &mut intent,
                );
            }
        }
        if let Some(scale) = state.tape_sample.filter(|_| fit.samples) {
            let rect = egui::Rect::from_min_max(
                egui::pos2(inner.right() - SAMPLE_SLOT_PX, strip.top()),
                egui::pos2(inner.right(), strip.bottom()),
            );
            let tape = BubbleSample::Tape;
            sample(&mut content, rect, salt, tape, scale, painted, &mut intent);
        }
    }
    intent
}

/// The slot beside the bubbles glyph: kept whether or not the sample shows, so
/// the glyphs after it never shift.
fn candle_slot(
    ui: &mut egui::Ui,
    salt: u64,
    scale: Option<f32>,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(SAMPLE_SLOT_PX, ui.available_height()),
        egui::Sense::hover(),
    );
    if let Some(scale) = scale {
        let candle = BubbleSample::Candle;
        sample(ui, rect, salt, candle, scale, painted, intent);
    }
}

/// One sample bubble at its current size: drag up to grow, double click for
/// `1x`, the value on show only while dragging.
fn sample(
    ui: &mut egui::Ui,
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

/// The sample's value, floating just below the strip so nothing shifts.
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

/// One layer's borderless toggle: its shape, lit or grey. Left click switches,
/// right click opens what configures it: looking is not enabling.
pub fn layer_glyph(
    ui: &mut egui::Ui,
    glyph: LayerGlyph,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(GLYPH_SLOT_PX, ui.available_height()),
        egui::Sense::click(),
    );
    if response.clicked()
        && let Some(asked) = glyph.click()
    {
        *intent = Some(asked);
    }
    if response.secondary_clicked() {
        *intent = Some(HeaderIntent::OpenLayerSettings(glyph.toggle));
    }
    let ink = glyph.ink();
    let colour = ink_colour(ink, response.hovered());
    let boxed = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(GLYPH_BOX));
    paint_layer(
        ui.painter(),
        boxed,
        glyph.toggle,
        ink == GlyphInk::Lit,
        colour,
    );
    record(ui, HeaderControl::Layer(glyph.toggle), rect, painted);
    response.on_hover_ui(|ui| {
        match glyph.blocked {
            Some(block) => ui.label(block.explanation),
            None => {
                let key = shortcuts::binding(glyph.toggle.layer());
                ui.label(with_key(ui.ctx(), glyph.toggle.name(), key))
            }
        };
    });
}

/// The button that walks the three views; its glyph is the view on screen.
fn view_button(
    ui: &mut egui::Ui,
    view: LaneView,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(VIEW_SLOT_PX, ui.available_height()),
        egui::Sense::click(),
    );
    if response.clicked() {
        *intent = Some(HeaderIntent::SetView(view.next()));
    }
    paint_view(ui.painter(), rect, view, theme::ACCENT);
    record(ui, HeaderControl::View, rect, painted);
    response.on_hover_ui(|ui| {
        let key = Some(shortcuts::TAPE_VIEW_SHORTCUT);
        ui.label(with_key(ui.ctx(), view_name(view), key));
    });
}

fn record(ui: &egui::Ui, control: HeaderControl, rect: egui::Rect, painted: &mut PaintedControls) {
    // A control the strip clipped away is not on screen, so it is not listed.
    if ui.is_rect_visible(rect) {
        painted.push((control, rect));
    }
}

fn with_key(ctx: &egui::Context, name: &str, key: Option<egui::KeyboardShortcut>) -> String {
    key.map_or_else(
        || name.to_owned(),
        |key| format!("{name} ({})", ctx.format_shortcut(&key)),
    )
}

fn ink_colour(ink: GlyphInk, hovered: bool) -> egui::Color32 {
    match ink {
        GlyphInk::Lit => theme::ACCENT,
        GlyphInk::Off if hovered => theme::TEXT_MUTED.gamma_multiply(OFF_HOVER_INK),
        GlyphInk::Off => theme::TEXT_MUTED.gamma_multiply(OFF_INK),
        GlyphInk::Blocked => theme::TEXT_MUTED.gamma_multiply(BLOCKED_INK),
    }
}

/// Shapes written in a glyph's own drawing box, scaled onto the slot. Each
/// glyph is a small copy of what its layer draws; a lit one fills its marks, so
/// on and off survive a screenshot read in greyscale.
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

fn paint_layer(
    painter: &egui::Painter,
    boxed: egui::Rect,
    toggle: LayerToggle,
    lit: bool,
    colour: egui::Color32,
) {
    let pen = Pen {
        painter,
        origin: boxed.min,
        unit: boxed.width() / GLYPH_BOX,
        colour,
        filled: lit,
    };
    match toggle {
        // Three circles: the prints themselves.
        LayerToggle::Bubbles => {
            pen.dot(5.0, 9.5, 3.2);
            pen.dot(10.5, 4.5, 2.1);
            pen.dot(11.5, 11.0, 1.3);
        }
        // A block with three fading bands: the book's rows.
        LayerToggle::Heatmap => {
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
        // A narrow band hugging an axis tick: the strip beside the price axis.
        LayerToggle::LiveStrip => {
            pen.line((1.5, 2.0), (1.5, 13.0));
            pen.line((11.5, 1.5), (11.5, 13.5));
            for (y, from) in [(4.0, 7.0), (6.5, 5.0), (9.0, 8.5), (11.5, 6.0)] {
                pen.line((11.0, y), (from, y));
            }
        }
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

/// The bars as words with a faint caret, as the control lays them out.
fn bars_text(ui: &egui::Ui, header: &PaneHeader) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let mut text = egui::text::LayoutJob::default();
    text.append(
        &header.bars_text,
        0.0,
        egui::TextFormat::simple(font.clone(), theme::TEXT_MUTED),
    );
    text.append(
        icons::CARET_DOWN,
        3.0,
        egui::TextFormat::simple(font, theme::TEXT_FAINT.gamma_multiply(0.6)),
    );
    text
}

/// How wide the bars control will be, before it is laid out.
fn bars_width(ui: &egui::Ui, header: &PaneHeader) -> f32 {
    let galley = ui.fonts(|fonts| fonts.layout_job(bars_text(ui, header)));
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
    let text = bars_text(ui, header);
    let control = ui
        .add(egui::Button::new(text).frame(false))
        .on_hover_text("Change this chart's bars");
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
