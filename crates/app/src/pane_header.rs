//! Paints the header line every pane carries (the rule, the mark, the bars and
//! the indicators button on the left; the Trail and Tape switches, the strip and
//! the gear on a flow pane's right), the capsule floating over each chart side and the
//! label beside it. What they say and what a press asks for are
//! `quantick_chart`'s call; this file holds the shapes and the pointer.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_chart::connection_state::Dot;
use quantick_chart::pane_header::{
    BubbleSample, CAPSULE_HEIGHT_PX, CAPSULE_INSET_TOP_PX, CAPSULE_SLOT_HEIGHT_PX, Capsule,
    CapsuleAttention, CapsuleItem, CapsuleSide, FLOW_CONTROLS_PX, FlowHeaderState, GLYPH_SLOT_PX,
    GROUP_GAP_PX, GlyphInk, HAIRLINE_GAP_PX, HeaderControl, HeaderIntent, HeaderWidths,
    INDICATORS_GLYPH, INDICATORS_LABEL, LayerGlyph, LayerToggle, PaneHeader, SAMPLE_RESET_SCALE,
    SEGMENT_FRAME_HEIGHT_PX, SEGMENT_FRAME_PAD_PX, SEGMENT_GAP_PX, SEGMENT_GLYPH_PX,
    SEGMENT_HEIGHT_PX, SEGMENT_PAD_WORDS_PX, SEGMENT_WORD_GAP_PX, SLOT_GAP_PX, SegmentInk,
    SideSegment, dragged_scale, flow_fit, sample_diameter_px, sample_value_text, section_layer,
    segment_px, side_word, side_words_px,
};
use quantick_chart::side_label::{
    LabelFit, LabelInk, SIDE_LABEL_GAP_PX, SIDE_LABEL_INSET_LEFT_PX, SIDE_LABEL_PADDING_PX,
    SideLabel,
};
use quantick_engine::bar_selection::{BarInputAvailability, BarSelection};
use quantick_orderflow::LaneSide;
use quantick_workspace::orderflow_panel::{PanelControl, PanelSection};
use smallvec::SmallVec;

use crate::chart_layers::shortcuts;
use crate::loading;
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
/// The spin inside the bars control during a recut.
const REBUILD_SPINNER_PX: f32 = 10.0;
/// The indicators button: 12 pt words, an icon and its gap, its padding.
const INDICATORS_SIZE: f32 = 12.0;
const INDICATORS_ICON_GAP_PX: f32 = 5.0;
const INDICATORS_PADDING_X: f32 = 7.0;
const INDICATORS_HEIGHT: f32 = 22.0;

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
/// The trail glyph's drawing box.
const TRAIL_BOX: f32 = 20.0;
/// A segment's word, and the corners of the control's frame and of a lit segment.
const SEGMENT_WORD_SIZE: f32 = 12.0;
const SEGMENT_FRAME_ROUNDING: f32 = 5.0;
const SEGMENT_ROUNDING: f32 = 3.0;
/// The dot a closed market's segment wears where its glyph stands.
const MARKET_DOT_RADIUS: f32 = 3.0;
/// How long the capsule takes to change tier.
const CAPSULE_FADE_S: f32 = 0.14;
/// The side label's plate corner and its mono text.
const SIDE_LABEL_ROUNDING: f32 = 3.0;
const SIDE_LABEL_SIZE: f32 = 11.0;
/// How high the sample's value floats over the chart below it.
const VALUE_GAP_PX: f32 = 4.0;

/// What the header painted this frame, in reading order: the scene reads it,
/// so it lists a control only while a hand can reach it.
pub type PaintedControls = SmallVec<[(HeaderControl, egui::Rect); LayerToggle::ALL.len() + 7]>;

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
fn indicators_text(words: Option<egui::Color32>) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    let icon = egui::TextFormat {
        italics: true,
        ..egui::TextFormat::simple(egui::FontId::monospace(INDICATORS_SIZE), theme::TEXT_MUTED)
    };
    text.append(INDICATORS_GLYPH, 0.0, icon);
    if let Some(words) = words {
        let font = egui::FontId::proportional(INDICATORS_SIZE);
        let words = egui::TextFormat::simple(font, words);
        text.append(INDICATORS_LABEL, INDICATORS_ICON_GAP_PX, words);
    }
    text
}

/// How wide the indicators button is, before it is laid out, with its word or
/// without it.
pub fn indicators_width(ui: &egui::Ui, words: bool) -> f32 {
    let job = indicators_text(words.then_some(theme::PANE_ACTION));
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    galley.size().x + 2.0 * INDICATORS_PADDING_X
}

/// The indicators button with its left edge at `left`, centred on the strip;
/// answers whether it was clicked.
pub fn indicators_button(
    ui: &egui::Ui,
    left: f32,
    strip: egui::Rect,
    salt: u64,
    words: bool,
) -> bool {
    let size = egui::vec2(indicators_width(ui, words), INDICATORS_HEIGHT);
    let center = egui::pos2(left + size.x / 2.0, strip.center().y);
    let rect = egui::Rect::from_center_size(center, size);
    let id = ui.id().with(("pane_indicators", salt));
    let response = ui.interact(rect, id, egui::Sense::click());
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(indicators_id(salt), rect));
    let painter = ui.painter().with_clip_rect(strip.intersect(ui.clip_rect()));
    let hovered = response.hovered();
    let colour = if hovered {
        theme::TEXT_PRIMARY
    } else {
        theme::PANE_ACTION
    };
    if hovered {
        let rounding = egui::Rounding::same(HOVER_ROUNDING);
        painter.rect_filled(rect, rounding, theme::BARE_HOVER);
    }
    let job = indicators_text(words.then_some(colour));
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    let at = rect.center() - galley.size() / 2.0;
    painter.galley(at, galley, colour);
    let clicked = response.clicked();
    if !words {
        response.on_hover_text(INDICATORS_LABEL);
    }
    clicked
}

/// What a flow pane's row draws besides its bars: `None` without a tape.
pub struct FlowRow {
    /// The strip glyph, unless the data blocks it.
    pub strip: Option<LayerGlyph>,
    pub state: Option<FlowHeaderState>,
    /// The connection chip, while some stream is not well.
    pub chip: Option<[Dot; 3]>,
    /// The job slot's words and whether a job (not a remark) wrote them.
    pub slot: Option<(String, bool)>,
    /// The bars are being rebuilt: the bars control says so, and nothing else.
    pub rebuilding: bool,
    /// The tab's order-flow panel is open: the gear wears the pressed look.
    pub settings_open: bool,
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
    let indicators_px = indicators_width(ui, true);
    let bare_controls_px = row.state.map_or(0.0, |_| FLOW_CONTROLS_PX + GROUP_GAP_PX);
    let word_widths = segment_word_widths(ui, row.state.as_ref());
    let side_words = row.state.map_or(0.0, |_| side_words_px(word_widths));
    let mark = header.mark.map(mark_text);
    let mark_px = mark.as_ref().map_or(0.0, |job| {
        ui.fonts(|fonts| fonts.layout_job(job.clone())).size().x
    });
    let slot = row.slot.as_ref().map(|(text, job)| loading::Slot {
        job: *job,
        text: text.as_str(),
    });
    let (chip_with_words, chip_dots) = row
        .chip
        .as_ref()
        .map_or((0.0, 0.0), |dots| loading::chip_widths(ui, dots));
    let slot_px = slot
        .as_ref()
        .map_or(0.0, |slot| loading::slot_width(ui, slot));
    let fit = flow_fit(&HeaderWidths {
        strip: inner.width(),
        mark: mark_px,
        bars: bars_width(ui, header),
        indicators: indicators_px,
        controls: bare_controls_px,
        side_words,
        slot: slot_px,
        chip_with_words,
        chip_dots,
        market_held: row.state.is_some_and(|state| state.market_held),
    });
    let controls_px = bare_controls_px + if fit.side_words { side_words } else { 0.0 };
    let indicators_px = indicators_width(ui, fit.indicators_words);
    let chip_px = match (fit.chip, fit.chip_words) {
        (true, true) => chip_with_words + GROUP_GAP_PX,
        (true, false) => chip_dots + GROUP_GAP_PX,
        _ => 0.0,
    };
    let right_px = controls_px + chip_px;
    let slot_room = if fit.slot {
        slot_px + GROUP_GAP_PX
    } else {
        0.0
    };
    let left = inner.with_max_x(inner.right() - right_px - slot_room);
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("pane_header", salt))
            .max_rect(left)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let limit = (left.right() - indicators_px).max(strip.left());
    content.set_clip_rect(strip.with_max_x(limit).intersect(ui.clip_rect()));
    content.spacing_mut().item_spacing.x = 0.0;
    if let Some(mark) = mark.filter(|_| fit.mark) {
        content.add(egui::Label::new(mark).selectable(false));
        content.add_space(GROUP_GAP_PX);
    }
    bars_control(
        &mut content,
        header,
        salt,
        selection,
        inputs,
        row.rebuilding,
    );
    let button_left = (content.cursor().left() + GROUP_GAP_PX).min(limit);
    let indicators = indicators_button(ui, button_left, strip, salt, fit.indicators_words);
    let chip_right = inner.right() - controls_px;
    if let Some(dots) = row.chip.as_ref().filter(|_| fit.chip) {
        loading::draw_chip(ui, chip_right, strip, dots, fit.chip_words);
    }
    if let Some(slot) = slot.as_ref().filter(|_| fit.slot) {
        loading::draw_slot(ui, chip_right - chip_px, strip, slot);
    }
    let mut intent = None;
    if let Some(state) = row.state {
        let room = inner.with_min_x(inner.right() - (controls_px - GROUP_GAP_PX));
        let right = clipped_child(ui, room, strip, ("pane_header_right", salt));
        let height = inner.y_range();
        let words = fit.side_words.then_some(word_widths);
        side_control(&right, room, salt, &state, words, painted, &mut intent);
        let gear =
            egui::Rect::from_x_y_ranges(inner.right() - GLYPH_SLOT_PX..=inner.right(), height);
        if let Some(glyph) = row.strip {
            let glyph_left = gear.left() - GROUP_GAP_PX - GLYPH_SLOT_PX;
            let hairline = glyph_left - HAIRLINE_GAP_PX;
            let around = inner.center().y - 8.0..=inner.center().y + 8.0;
            right
                .painter()
                .vline(hairline, around, egui::Stroke::new(1.0_f32, theme::BORDER));
            let rect = egui::Rect::from_x_y_ranges(glyph_left..=glyph_left + GLYPH_SLOT_PX, height);
            layer_glyph(&right, rect, salt, glyph, painted, &mut intent);
        }
        settings_gear(&right, gear, salt, row.settings_open, painted, &mut intent);
    }
    (intent, indicators)
}

/// The gear, bare like the indicators button, pressed while the panel is open.
/// Left click opens or closes; right click is the two-item menu.
fn settings_gear(
    ui: &egui::Ui,
    rect: egui::Rect,
    salt: u64,
    open: bool,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let id = ui.id().with(("pane_settings_gear", salt));
    let response = ui.interact(rect, id, egui::Sense::click());
    if response.clicked() {
        *intent = Some(HeaderIntent::ToggleSettings);
    }
    let patch = egui::Rect::from_center_size(
        rect.center(),
        egui::Vec2::splat(GLYPH_SLOT_PX.min(rect.height())),
    );
    let rounding = egui::Rounding::same(HOVER_ROUNDING);
    if open {
        ui.painter().rect_filled(patch, rounding, theme::CONTROL);
    } else if response.hovered() {
        ui.painter().rect_filled(patch, rounding, theme::BARE_HOVER);
    }
    let colour = if open || response.hovered() {
        theme::TEXT_PRIMARY
    } else {
        theme::PANE_ACTION
    };
    let font = egui::FontId::proportional(SEGMENT_GLYPH_PX);
    let at = egui::Align2::CENTER_CENTER;
    ui.painter()
        .text(rect.center(), at, icons::GEAR, font, colour);
    record(
        ui,
        HeaderControl::Settings(PanelControl::Gear),
        rect,
        painted,
    );
    let key = Some(shortcuts::ORDERFLOW_SETTINGS_SHORTCUT);
    let response = response.on_hover_ui(|ui| {
        ui.label(with_key(ui.ctx(), "Order flow settings", key));
    });
    response.context_menu(|ui| {
        for section in PanelSection::ALL {
            if ui.button(section.title()).clicked() {
                *intent = Some(HeaderIntent::OpenLayerSettings(section_layer(section)));
                ui.close_menu();
            }
        }
    });
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

/// Where a capsule stands and what it may notice there.
pub struct CapsuleSpot {
    pub corner: (f32, f32),
    pub side: CapsuleSide,
    /// The whole chart side it floats over.
    pub side_rect: egui::Rect,
    pub pane_focused: bool,
}

/// The side's label on its plate, on the capsule's own line, in the room left of
/// `capsule_left`. Painted only: a press goes through it to the chart.
pub fn draw_side_label(ui: &egui::Ui, label: &SideLabel, side: egui::Rect, capsule_left: f32) {
    let min = egui::pos2(
        side.left() + SIDE_LABEL_INSET_LEFT_PX,
        side.top() + CAPSULE_INSET_TOP_PX,
    );
    if min.y + CAPSULE_HEIGHT_PX > side.bottom() {
        return;
    }
    let room = capsule_left - SIDE_LABEL_GAP_PX - min.x - 2.0 * SIDE_LABEL_PADDING_PX;
    let measure = |fit| {
        let job = side_label_text(ui.ctx(), label, fit);
        ui.fonts(|fonts| fonts.layout_job(job))
    };
    let (whole, short) = (measure(LabelFit::Whole), measure(LabelFit::NoClock));
    let galley = match LabelFit::of(whole.size().x, short.size().x, room) {
        LabelFit::Whole => whole,
        LabelFit::NoClock => short,
        LabelFit::None => return,
    };
    let plate = egui::Rect::from_min_size(
        min,
        egui::vec2(
            galley.size().x + 2.0 * SIDE_LABEL_PADDING_PX,
            CAPSULE_HEIGHT_PX,
        ),
    );
    let painter = ui.painter().with_clip_rect(side);
    painter.rect_filled(
        plate,
        egui::Rounding::same(SIDE_LABEL_ROUNDING),
        theme::CASING,
    );
    let at = plate.left_center() + egui::vec2(SIDE_LABEL_PADDING_PX, -galley.size().y / 2.0);
    painter.galley(at, galley, theme::TEXT_MUTED);
}

/// The label's runs in their inks: one job, so its width is what it paints.
fn side_label_text(ctx: &egui::Context, label: &SideLabel, fit: LabelFit) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    for (run, ink) in label.runs(fit) {
        let color = match ink {
            LabelInk::Mark => theme::PANE_MARK,
            LabelInk::Muted => theme::TEXT_MUTED,
            LabelInk::Amber => theme::AMBER,
            LabelInk::FaintPulse => theme::TEXT_FAINT.gamma_multiply(loading::pulse(ctx)),
            LabelInk::Warn => theme::WARN,
        };
        let format = egui::TextFormat {
            font_id: egui::FontId::monospace(SIDE_LABEL_SIZE),
            color,
            ..Default::default()
        };
        text.append(run, 0.0, format);
    }
    text
}

/// The tape's seam: a dashed line at the last print, and the muted line after it.
pub fn draw_tape_seam(ui: &egui::Ui, side: egui::Rect, x: f32, words: Option<&str>) {
    let painter = ui.painter().with_clip_rect(side);
    let ends = [egui::pos2(x, side.top()), egui::pos2(x, side.bottom())];
    let stroke = egui::Stroke::new(1.0_f32, theme::AMBER);
    painter.add(egui::Shape::dashed_line(&ends, stroke, 4.0, 5.0));
    let Some(words) = words else {
        return;
    };
    let font = egui::FontId::proportional(SEGMENT_WORD_SIZE);
    let galley = painter.layout_no_wrap(words.to_owned(), font, theme::TEXT_MUTED);
    let at = quantick_chart::market_fold::seam_words_x(x, side.right(), galley.size().x);
    if let Some(left) = at {
        let at = egui::pos2(left, side.center().y - galley.size().y / 2.0);
        painter.galley(at, galley, theme::TEXT_MUTED);
    }
}

/// One capsule at its spot: its backdrop takes the pointer so a press between
/// the slots never reaches the chart. Registered after the chart, so it wins the
/// press. Answers the capsule's rect.
pub fn draw_capsule(
    ui: &mut egui::Ui,
    capsule: &Capsule,
    spot: &CapsuleSpot,
    salt: u64,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) -> egui::Rect {
    let corner = spot.corner;
    let rect = egui::Rect::from_min_size(
        egui::pos2(corner.0, corner.1),
        egui::vec2(capsule.width_px(), CAPSULE_HEIGHT_PX),
    );
    let tier = capsule.tier(CapsuleAttention {
        pointer_over_capsule: ui.rect_contains_pointer(rect),
        pointer_over_side: ui.rect_contains_pointer(spot.side_rect),
        pane_focused: spot.pane_focused,
    });
    let clip = rect.expand(CAPSULE_SHADOW_ROOM_PX);
    let mut ui = clipped_child(ui, rect, clip, ("pane_capsule", salt, spot.side as u8));
    let opacity = if loading::reduced_motion() {
        tier.opacity()
    } else {
        ui.ctx()
            .animate_value_with_time(ui.id(), tier.opacity(), CAPSULE_FADE_S)
    };
    ui.set_opacity(opacity);
    let ui = ui;
    let _ = ui.interact(rect, ui.id(), egui::Sense::click_and_drag());
    let rounding = egui::Rounding::same(CAPSULE_ROUNDING);
    let shadow = egui::Shadow {
        offset: egui::vec2(0.0, 1.0),
        blur: 4.0,
        spread: 0.0,
        color: theme::CAPSULE_SHADOW,
    };
    let painter = ui.painter();
    if tier.shadowed() {
        painter.add(shadow.as_shape(rect, rounding));
    }
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
    ui.painter().circle_filled(
        rect.center(),
        sample_diameter_px(shown) / 2.0,
        theme::TEXT_PRIMARY,
    );
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

/// The words' widths, as the segments will paint them: Trail, then Tape.
fn segment_word_widths(ui: &egui::Ui, state: Option<&FlowHeaderState>) -> [f32; 2] {
    [LaneSide::Trail, LaneSide::Tape].map(|side| {
        let word = state.map_or(side_word(side), |state| SideSegment::of(side, state).word());
        ui.fonts(|fonts| {
            fonts
                .layout_no_wrap(
                    word.to_owned(),
                    egui::FontId::proportional(SEGMENT_WORD_SIZE),
                    theme::TEXT_PRIMARY,
                )
                .size()
                .x
        })
    })
}

/// The Trail and Tape switches in their frame, from `room`'s left edge. `words`
/// holds their widths while the line has room for them.
fn side_control(
    ui: &egui::Ui,
    room: egui::Rect,
    salt: u64,
    state: &FlowHeaderState,
    words: Option<[f32; 2]>,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let widths = [0, 1].map(|at| segment_px(words.map(|words| words[at])));
    let inner_px = widths[0] + SEGMENT_GAP_PX + widths[1];
    let frame = egui::Rect::from_center_size(
        egui::pos2(
            room.left() + SEGMENT_FRAME_PAD_PX + inner_px / 2.0,
            room.center().y,
        ),
        egui::vec2(
            inner_px + 2.0 * SEGMENT_FRAME_PAD_PX,
            SEGMENT_FRAME_HEIGHT_PX.min(room.height()),
        ),
    );
    ui.painter().rect(
        frame,
        egui::Rounding::same(SEGMENT_FRAME_ROUNDING),
        theme::INSET,
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    let mut left = frame.left() + SEGMENT_FRAME_PAD_PX;
    for (segment, width) in SideSegment::pair(state).into_iter().zip(widths) {
        let rect = egui::Rect::from_center_size(
            egui::pos2(left + width / 2.0, frame.center().y),
            egui::vec2(width, SEGMENT_HEIGHT_PX.min(frame.height())),
        );
        side_segment(ui, rect, salt, segment, words.is_some(), painted, intent);
        left = rect.right() + SEGMENT_GAP_PX;
    }
}

/// One switch: lit, or faint and brighter under the pointer. A refusing segment
/// takes the click and does nothing; its tooltip says why.
fn side_segment(
    ui: &egui::Ui,
    rect: egui::Rect,
    salt: u64,
    segment: SideSegment,
    words: bool,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let id = ui.id().with(("side", salt, segment.side as u8));
    let response = ui.interact(rect, id, egui::Sense::click());
    if response.clicked()
        && let Some(asked) = segment.click()
    {
        *intent = Some(asked);
    }
    let lit = segment.ink == SegmentInk::Lit;
    let closed = segment.ink == SegmentInk::MarketClosed;
    let colour = match (segment.ink, response.hovered()) {
        (SegmentInk::Lit, _) => theme::TEXT_PRIMARY,
        (SegmentInk::MarketClosed, _) => theme::WARN,
        (SegmentInk::Unlit, true) => theme::PANE_ACTION,
        (SegmentInk::Unlit, false) => theme::TEXT_FAINT,
    };
    let painter = ui.painter();
    if lit {
        painter.rect_filled(rect, egui::Rounding::same(SEGMENT_ROUNDING), theme::CONTROL);
    }
    if closed {
        painter.rect_stroke(
            rect,
            egui::Rounding::same(SEGMENT_ROUNDING),
            (1.0, theme::WARN),
        );
    }
    let glyph_center = if words {
        egui::pos2(
            rect.left() + SEGMENT_PAD_WORDS_PX + SEGMENT_GLYPH_PX / 2.0,
            rect.center().y,
        )
    } else {
        rect.center()
    };
    match segment.side {
        _ if closed => _ = painter.circle_filled(glyph_center, MARKET_DOT_RADIUS, theme::WARN),
        LaneSide::Trail => paint_trail(painter, glyph_center, colour, lit),
        LaneSide::Tape => {
            painter.text(
                glyph_center,
                egui::Align2::CENTER_CENTER,
                icons::CASSETTE_TAPE,
                egui::FontId::proportional(SEGMENT_GLYPH_PX),
                colour,
            );
        }
    }
    if words {
        painter.text(
            egui::pos2(
                glyph_center.x + SEGMENT_GLYPH_PX / 2.0 + SEGMENT_WORD_GAP_PX,
                rect.center().y,
            ),
            egui::Align2::LEFT_CENTER,
            segment.word(),
            egui::FontId::proportional(SEGMENT_WORD_SIZE),
            colour,
        );
    }
    record(ui, HeaderControl::Side(segment.side), rect, painted);
    response.on_hover_ui(|ui| {
        let key = Some(shortcuts::side_shortcut(segment.side));
        let name = with_key(ui.ctx(), segment.word(), key);
        ui.label(match segment.note() {
            Some(note) => format!("{name} · {note}"),
            None => name,
        });
    });
}

pub(crate) fn record(
    ui: &egui::Ui,
    control: HeaderControl,
    rect: egui::Rect,
    painted: &mut PaintedControls,
) {
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

    /// A candle body, outlined and filled while lit.
    fn body(&self, x: f32, y: f32, width: f32, height: f32) {
        self.outline(x, y, width, height);
        self.band(x, y, width, height, 1.0);
    }
}

fn paint_layer(painter: &egui::Painter, boxed: egui::Rect, toggle: LayerToggle, ink: GlyphInk) {
    let lit = ink == GlyphInk::Lit;
    let colour = if lit { theme::TEXT_PRIMARY } else { theme::OFF };
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

/// Three candles with their wicks: filled while the side is lit.
fn paint_trail(painter: &egui::Painter, center: egui::Pos2, colour: egui::Color32, lit: bool) {
    let unit = SEGMENT_GLYPH_PX / TRAIL_BOX;
    let pen = Pen {
        painter,
        origin: center - egui::Vec2::splat(SEGMENT_GLYPH_PX / 2.0),
        unit,
        colour,
        filled: lit,
    };
    for (x, top, body_top, body, bottom) in [
        (3.0, 4.0, 7.0, 7.0, 16.0),
        (8.5, 2.0, 4.0, 8.0, 16.0),
        (14.0, 6.0, 9.0, 5.0, 17.0),
    ] {
        let wick = x + 1.5;
        pen.line((wick, top), (wick, body_top));
        pen.line((wick, body_top + body), (wick, bottom));
        pen.body(x, body_top, 3.0, body);
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
    rebuilding: bool,
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
    if rebuilding {
        // The one place a recut spins: the control the hand just changed, its
        // words dimmed under the spin.
        ui.painter().galley(at, galley, theme::TEXT_FAINT);
        ui.put(
            egui::Rect::from_center_size(
                egui::pos2(rect.right() - REBUILD_SPINNER_PX / 2.0, rect.center().y),
                egui::Vec2::splat(REBUILD_SPINNER_PX),
            ),
            egui::Spinner::new()
                .size(REBUILD_SPINNER_PX)
                .color(loading::GREY_MID),
        );
    } else {
        ui.painter().galley(
            at + egui::vec2(SEMIBOLD_OFFSET_PX, 0.0),
            galley.clone(),
            theme::TEXT_PRIMARY,
        );
        ui.painter().galley(at, galley, theme::TEXT_PRIMARY);
    }
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
