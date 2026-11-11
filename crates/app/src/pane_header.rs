//! Paints a pane's chrome: the bars control and indicators button every pane
//! carries, the flow pane's band (the mark, a rule and the Trail and Tape keys on
//! the left; the status and the gear on the right), its two side headers, and the
//! capsule docked in a header. What they say and what a press asks for are
//! `quantick_chart`'s call; this file holds the shapes and the pointer.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_chart::connection_state::Dot;
use quantick_chart::flow_band::{
    DOCKED_CAPSULE_HEIGHT_PX, DOCKED_SLOT_HEIGHT_PX, FRAME_PX, SIDE_GAP_PX, SIDE_PAD_RIGHT_PX,
    SideWidths, WINDOW_SELECTOR_HINT, docked_capsule_corner, docked_slot_origin, plate_word,
    side_fit, window_label, window_options,
};
use quantick_chart::pane_header::{
    BULB_END_PX, BULB_KEY_PX, BubbleSample, CAPSULE_HEIGHT_PX, Capsule, CapsuleItem, CapsuleSide,
    FlowHeaderState, GEAR_END_PX, GEAR_KEY_PX, GLYPH_SLOT_PX, GROUP_GAP_PX, GlyphInk,
    HeaderControl, HeaderIntent, HeaderWidths, INDICATORS_GLYPH, INDICATORS_LABEL, KEY_DOT_PX,
    LayerGlyph, LayerToggle, MARK_RULE_HEIGHT_PX, MARK_RULE_PX, PaneHeader, SAMPLE_RESET_SCALE,
    SEGMENT_GAP_PX, SEGMENT_GLYPH_PX, SEGMENT_HEIGHT_PX, SEGMENT_WORD_GAP_PX, SLOT_GAP_PX,
    SegmentInk, SideSegment, dragged_scale, flow_controls_px, flow_fit, key_dot_inset, mark_px,
    sample_diameter_px, sample_value_text, section_layer, segment_px, side_word, side_words_px,
};
use quantick_chart::side_label::SIDE_LABEL_PADDING_PX;
use quantick_engine::bar_selection::{BarInputAvailability, BarSelection};
use quantick_orderflow::LaneSide;
use quantick_workspace::orderflow_panel::{
    LIGHTS_DESCRIPTION, LIGHTS_NAME, PanelControl, PanelSection,
};
use smallvec::SmallVec;

use crate::chart_layers::shortcuts;
use crate::loading;
use crate::theme;

/// Padding inside the strip: clears the pane's edge and keeps the controls off
/// the rule below.
pub const CONTENT_PADDING: egui::Vec2 = egui::vec2(10.0, 2.0);

/// The mark: 11.5 pt monospace with .12 em of letter spacing.
const MARK_SIZE: f32 = 11.5;
const MARK_LETTER_SPACING: f32 = 1.38;
/// A side's plate: 10 pt monospace with .1 em of letter spacing, its padding and
/// corner.
const PLATE_SIZE: f32 = 10.0;
const PLATE_LETTER_SPACING: f32 = 1.0;
const PLATE_PADDING: egui::Vec2 = egui::vec2(6.0, 2.0);
const PLATE_ROUNDING: f32 = 3.0;
/// The window selector's room either side of its words: the TAPE plate then
/// stands as far from them as the TRAIL plate from its bars.
const WINDOW_PADDING_X: f32 = 3.0;
/// The bars text and its faint caret.
const BARS_SIZE: f32 = 13.0;
const CARET_SIZE: f32 = 10.0;
/// The spin inside the bars control during a recut.
const REBUILD_SPINNER_PX: f32 = 10.0;
/// The indicators button: 12 pt words, an icon and its gap, its padding.
const INDICATORS_SIZE: f32 = 12.0;
const INDICATORS_ICON_GAP_PX: f32 = 5.0;
const INDICATORS_PADDING_X: f32 = 7.0;
const INDICATORS_HEIGHT: f32 = 22.0;

/// A control's hover patch's corner radius.
pub(crate) const HOVER_ROUNDING: f32 = 4.0;
/// Side of a layer glyph's drawing box, the unit its shapes are written in.
const GLYPH_BOX: f32 = 16.0;
/// The slash through a glyph switched off by hand, corner to corner.
const SLASH_PX: f32 = 1.8;
/// The outlined sample of bubbles switched off by hand.
const SAMPLE_OFF_DIAMETER_PX: f32 = 12.0;
const SAMPLE_OFF_STROKE_PX: f32 = 1.2;
/// A key's word, and the corners of the gear's well while its panel is open.
const SEGMENT_WORD_SIZE: f32 = 11.5;
const SEGMENT_ROUNDING: f32 = 3.0;
/// A lit dot's soft glow, outermost ring first: how far past the dot, how strong;
/// at rest, then under the pointer.
const DOT_GLOW: [(f32, f32); 2] = [(3.0, 0.08), (1.5, 0.2)];
pub(crate) const DOT_GLOW_HOVERED: [(f32, f32); 2] = [(3.0, 0.2), (1.5, 0.45)];
/// A key held down dims its dot and word this far: no body to sink.
pub(crate) const KEY_PRESSED_OPACITY: f32 = 0.6;
/// The rule after the mark and the status words, in the band's ink this strong.
const MARK_RULE_OPACITY: f32 = 0.35;
const STATUS_OPACITY: f32 = 0.75;
/// How wide a key's tooltip wraps its description.
const TIP_WIDTH_PX: f32 = 300.0;
/// The seam plates' corner.
const SIDE_LABEL_ROUNDING: f32 = 3.0;
/// How high the sample's value floats over the chart below it.
const VALUE_GAP_PX: f32 = 4.0;

/// What the header painted this frame, in reading order: the scene reads it,
/// so it lists a control only while a hand can reach it.
pub type PaintedControls = SmallVec<[(HeaderControl, egui::Rect); LayerToggle::ALL.len() + 7]>;

/// The rule closing the strip against the chart below it.
pub fn paint_rule(ui: &egui::Ui, strip: egui::Rect) {
    let palette = theme::get(ui.ctx());
    ui.painter().line_segment(
        [
            egui::pos2(strip.left(), strip.bottom()),
            egui::pos2(strip.right(), strip.bottom()),
        ],
        egui::Stroke::new(1.0_f32, palette.border()),
    );
}

/// The `ƒx` icon and the words, as the button writes them.
fn indicators_text(
    palette: &crate::theme::Palette,
    words: Option<egui::Color32>,
) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    let icon = egui::TextFormat {
        italics: true,
        ..egui::TextFormat::simple(palette.mono_font(INDICATORS_SIZE), palette.text_muted())
    };
    text.append(INDICATORS_GLYPH, 0.0, icon);
    if let Some(words) = words {
        let font = palette.body_font(INDICATORS_SIZE);
        let words = egui::TextFormat::simple(font, words);
        text.append(INDICATORS_LABEL, INDICATORS_ICON_GAP_PX, words);
    }
    text
}

/// How wide the indicators button is, before it is laid out, with its word or
/// without it.
pub fn indicators_width(ui: &egui::Ui, words: bool) -> f32 {
    let palette = theme::get(ui.ctx());
    let job = indicators_text(&palette, words.then_some(palette.pane_action()));
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
    let palette = theme::get(ui.ctx());
    let size = egui::vec2(
        indicators_width(ui, words),
        palette.layout(INDICATORS_HEIGHT),
    );
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
        palette.text_primary()
    } else {
        palette.pane_action()
    };
    if hovered {
        let rounding = egui::Rounding::same(HOVER_ROUNDING);
        painter.rect_filled(rect, rounding, palette.bare_hover());
    }
    let job = indicators_text(&palette, words.then_some(colour));
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    let at = rect.center() - galley.size() / 2.0;
    painter.galley(at, galley, colour);
    let clicked = response.clicked();
    if !words {
        response.on_hover_text(INDICATORS_LABEL);
    }
    clicked
}

/// What a flow pane's band draws besides its mark: `None` without a tape.
pub struct FlowRow {
    pub state: Option<FlowHeaderState>,
    /// The connection chip, while some stream is not well.
    pub chip: Option<[Dot; 3]>,
    /// The job slot's words and whether a job (not a remark) wrote them.
    pub slot: Option<(String, bool)>,
    /// The tab's order-flow panel is open: the gear wears the pressed look.
    pub settings_open: bool,
    /// The flow pane's lights; the bulb shows where the theme offers them.
    pub lights_on: bool,
}

/// The flow pane's band; answers what a press on it asked for.
pub fn draw_marked(
    ui: &mut egui::Ui,
    strip: egui::Rect,
    header: &PaneHeader,
    salt: u64,
    row: FlowRow,
    painted: &mut PaintedControls,
) -> Option<HeaderIntent> {
    let palette = theme::get(ui.ctx());
    painted.clear();
    ui.painter()
        .rect_filled(strip, egui::Rounding::ZERO, palette.flow_band());
    let rule = egui::Stroke::new(1.0_f32, palette.flow_band_rule());
    ui.painter()
        .hline(strip.x_range(), strip.bottom() - 0.5, rule);
    let inner = strip.shrink2(CONTENT_PADDING);
    let inner = inner.with_min_x((inner.left() + FRAME_PX).min(inner.right()));
    let bulb = palette.resolved.has_lit_flow().then_some(row.lights_on);
    let bare_controls_px = row.state.map_or(0.0, |_| flow_controls_px(bulb.is_some()));
    let bulb_px = bulb.map_or(0.0, |_| BULB_END_PX);
    let word_widths = segment_word_widths(ui, row.state.as_ref());
    let side_words = row.state.map_or(0.0, |_| side_words_px(word_widths));
    let mark = header
        .mark
        .map(|mark| ui.fonts(|fonts| fonts.layout_job(mark_text(&palette, mark))));
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
    let mark_text_px = mark.as_ref().map_or(0.0, |mark| mark.size().x);
    let fit = flow_fit(&HeaderWidths {
        strip: inner.width(),
        mark: mark_px(mark_text_px),
        controls: bare_controls_px,
        side_words,
        slot: slot_px,
        chip_with_words,
        chip_dots,
        market_held: row.state.is_some_and(|state| state.market_held),
    });
    let chip_px = match (fit.chip, fit.chip_words) {
        (true, true) => chip_with_words + GROUP_GAP_PX,
        (true, false) => chip_dots + GROUP_GAP_PX,
        _ => 0.0,
    };
    let mut left = inner.left();
    if let Some(mark) = mark.filter(|_| fit.mark) {
        let at = egui::pos2(left, inner.center().y - mark.size().y / 2.0);
        ui.painter().galley(at, mark, palette.flow_ink());
        let x = left + mark_px(mark_text_px) - MARK_RULE_PX / 2.0;
        let ink = palette.flow_ink().gamma_multiply(MARK_RULE_OPACITY);
        let half = MARK_RULE_HEIGHT_PX / 2.0;
        let tall = inner.center().y - half..=inner.center().y + half;
        ui.painter().vline(x, tall, (MARK_RULE_PX, ink));
        left += mark_px(mark_text_px) + GROUP_GAP_PX;
    }
    let chip_right = inner.right() - row.state.map_or(0.0, |_| GEAR_END_PX + bulb_px);
    if let Some(dots) = row.chip.as_ref().filter(|_| fit.chip) {
        let ink = palette.flow_ink();
        loading::draw_chip(ui, chip_right, strip, dots, fit.chip_words, ink);
    }
    if let Some(slot) = slot.as_ref().filter(|_| fit.slot) {
        let ink = palette.flow_ink().gamma_multiply(STATUS_OPACITY);
        loading::draw_slot(ui, chip_right - chip_px, strip, slot, Some(ink));
    }
    let mut intent = None;
    if let Some(state) = row.state {
        let band = clipped_child(ui, inner, strip, ("pane_header_band", salt));
        let words = fit.side_words.then_some(word_widths);
        let room = inner.with_min_x(left);
        side_control(&band, room, salt, &state, words, painted, &mut intent);
        let gear = key_rect(
            inner.right() - GEAR_KEY_PX,
            GEAR_KEY_PX,
            inner,
            palette.layout(SEGMENT_HEIGHT_PX),
        );
        settings_gear(&band, gear, salt, row.settings_open, painted, &mut intent);
        if let Some(on) = bulb {
            let height = palette.layout(SEGMENT_HEIGHT_PX);
            let rect = key_rect(gear.left() - BULB_END_PX, BULB_KEY_PX, inner, height);
            lights_bulb(&band, rect, (salt, on), painted, &mut intent);
        }
    }
    intent
}

/// The bare bulb left of the gear, lit while the flow pane's lights are on.
fn lights_bulb(
    ui: &egui::Ui,
    rect: egui::Rect,
    (salt, on): (u64, bool),
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let id = ui.id().with(("pane_lights_bulb", salt));
    let glyph = [icons::LIGHTBULB, icons::LIGHTBULB_FILAMENT][usize::from(on)];
    let lit = on.then(|| theme::get(ui.ctx()).flow_bulb_lit());
    let control = HeaderControl::Settings(PanelControl::Lights);
    let response = bare_key(ui, (rect, id), (glyph, false, lit), control, painted);
    if response.clicked() {
        *intent = Some(HeaderIntent::SetLights(!on));
    }
    response.on_hover_ui(|ui| {
        ui.set_max_width(TIP_WIDTH_PX);
        ui.strong(LIGHTS_NAME);
        ui.label(LIGHTS_DESCRIPTION);
    });
}

/// The pane's bars control and what it reads; the row's first header carries it.
pub struct BarsPart<'a> {
    pub header: &'a PaneHeader,
    pub selection: &'a mut BarSelection,
    pub inputs: BarInputAvailability,
    /// The bars are being rebuilt: the bars control says so, and nothing else.
    pub rebuilding: bool,
}

/// What one side header draws; an absent part is `None`.
pub struct SideHeader<'a> {
    pub which: CapsuleSide,
    pub salt: u64,
    pub pad_left: f32,
    pub plate: bool,
    /// What the plate's tooltip says, worked out only while it is hovered.
    pub clock: &'a dyn Fn() -> String,
    pub bars: Option<BarsPart<'a>>,
    pub window: Option<&'a FlowHeaderState>,
    pub capsule: Option<Capsule>,
}

/// One side header, left to right: plate, the pane's bars and indicators (the
/// trail's) or the window selector (the tape's), and the capsule docked at the far
/// end, each while it fits. Answers whether the indicators button was clicked.
pub fn draw_side_header(
    ui: &mut egui::Ui,
    side: egui::Rect,
    header: SideHeader<'_>,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) -> bool {
    let palette = theme::get(ui.ctx());
    let salt = header.salt;
    let plate = header.plate.then(|| plate_galley(ui, header.which));
    let plate_w = plate.as_ref().map_or(0.0, |plate| plate_px(plate));
    let words = header.window.map(window_label);
    let window_w = words.as_deref().map_or(0.0, |words| {
        let caret = ui.fonts(|fonts| fonts.layout_job(caret_text(&palette, words)));
        caret.size().x + 2.0 * WINDOW_PADDING_X
    });
    let has_bars = header.bars.is_some();
    let indicators = |words| f32::from(u8::from(has_bars)) * indicators_width(ui, words);
    let fit = side_fit(&SideWidths {
        side: side.width(),
        pad_left: header.pad_left,
        plate: plate_w,
        bars: header
            .bars
            .as_ref()
            .map_or(0.0, |bars| bars_width(ui, bars.header)),
        indicators_with_words: indicators(true),
        indicators_icon: indicators(false),
        window: window_w,
        capsule: header.capsule.map_or(0.0, |capsule| capsule.width_px()),
    });
    let mut left = side.left() + header.pad_left;
    if let Some(plate) = plate.filter(|_| fit.plate) {
        let response = side_plate(ui, left, side, salt, plate, header.which);
        response.on_hover_ui(|ui| _ = ui.label((header.clock)()));
        left += plate_w + SIDE_GAP_PX;
    }
    let capsule = header.capsule.filter(|_| fit.capsule);
    let window = header.window.zip(words).filter(|_| fit.window);
    let end =
        side.right() - SIDE_PAD_RIGHT_PX - capsule.map_or(0.0, |c| c.width_px() + SIDE_GAP_PX);
    let mut clicked = false;
    if let Some(bars) = header.bars {
        let button_px = indicators(fit.indicators_words);
        let limit = (end - button_px).max(left);
        let room = egui::Rect::from_x_y_ranges(left..=limit, side.y_range());
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("pane_header", salt))
                .max_rect(room)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        content.set_clip_rect(room.intersect(ui.clip_rect()));
        content.spacing_mut().item_spacing.x = 0.0;
        bars_control(
            &mut content,
            bars.header,
            salt,
            bars.selection,
            bars.inputs,
            bars.rebuilding,
        );
        let button_left = (content.cursor().left() + SIDE_GAP_PX).min(limit);
        clicked = indicators_button(ui, button_left, side, salt, fit.indicators_words);
    }
    if let Some((state, words)) = window.as_ref() {
        let size = egui::vec2(window_w, palette.layout(INDICATORS_HEIGHT));
        let center = egui::pos2(left + size.x / 2.0, side.center().y);
        let rect = egui::Rect::from_center_size(center, size);
        window_selector(ui, rect, salt, state, words, painted, intent);
    }
    if let Some(capsule) = capsule {
        draw_docked_capsule(ui, &capsule, side, (header.which, salt), painted, intent);
    }
    clicked
}

/// A side's plate word, as it paints.
fn plate_galley(ui: &egui::Ui, side: CapsuleSide) -> std::sync::Arc<egui::Galley> {
    let palette = theme::get(ui.ctx());
    let mut text = egui::text::LayoutJob::default();
    let format = egui::TextFormat {
        font_id: palette.mono_font(PLATE_SIZE),
        color: palette.pane_mark(),
        extra_letter_spacing: PLATE_LETTER_SPACING * palette.mono_font(PLATE_SIZE).size
            / PLATE_SIZE,
        ..Default::default()
    };
    text.append(plate_word(side), 0.0, format);
    ui.fonts(|fonts| fonts.layout_job(text))
}

fn plate_px(galley: &egui::Galley) -> f32 {
    galley.size().x + 2.0 * PLATE_PADDING.x
}

/// The plate at `left`, centred on the side; hover only, so it takes no press.
fn side_plate(
    ui: &egui::Ui,
    left: f32,
    side: egui::Rect,
    salt: u64,
    galley: std::sync::Arc<egui::Galley>,
    which: CapsuleSide,
) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let size = galley.size() + 2.0 * PLATE_PADDING;
    let center = egui::pos2(left + size.x / 2.0, side.center().y);
    let rect = egui::Rect::from_center_size(center, size);
    let rounding = egui::Rounding::same(PLATE_ROUNDING);
    let stroke = egui::Stroke::new(1.0_f32, palette.border());
    ui.painter().rect(rect, rounding, palette.inset(), stroke);
    ui.painter()
        .galley(rect.min + PLATE_PADDING, galley, palette.pane_mark());
    let id = ui.id().with(("flow_side_plate", salt, which as u8));
    ui.interact(rect, id, egui::Sense::hover())
}

/// The window as words with a caret; a click opens the list the tape's menu offers.
fn window_selector(
    ui: &egui::Ui,
    rect: egui::Rect,
    salt: u64,
    state: &FlowHeaderState,
    words: &str,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let palette = theme::get(ui.ctx());
    let id = ui.id().with(("flow_tape_window", salt));
    let response = ui.interact(rect, id, egui::Sense::click());
    let popup = egui::Id::new(("flow_tape_window_list", salt));
    if response.hovered() {
        let rounding = egui::Rounding::same(HOVER_ROUNDING);
        ui.painter()
            .rect_filled(rect, rounding, palette.bare_hover());
    }
    let galley = ui.fonts(|fonts| fonts.layout_job(caret_text(&palette, words)));
    let at = rect.center() - galley.size() / 2.0;
    ui.painter().galley(at, galley, palette.text_primary());
    record(ui, HeaderControl::TapeWindow, rect, painted);
    let response = response.on_hover_text(WINDOW_SELECTOR_HINT);
    if response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup));
    }
    // egui holds one popup open at a time: note it was drawn while it is this one.
    if ui.memory(|memory| memory.is_popup_open(popup)) {
        let pass = ui.ctx().cumulative_pass_nr();
        ui.ctx()
            .data_mut(|data| data.insert_temp(seen_id(), (popup, pass)));
    }
    egui::popup_below_widget(
        ui,
        popup,
        &response,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(rect.width().max(120.0));
            for option in window_options(state) {
                if ui.selectable_label(option.selected, option.label).clicked() {
                    *intent = option.pick.map(HeaderIntent::SetLaneWindow).or(*intent);
                    ui.memory_mut(egui::Memory::close_popup);
                }
            }
        },
    );
}

/// Where a window selector whose list is open notes that list and the pass it drew in.
fn seen_id() -> egui::Id {
    egui::Id::new("flow_tape_window_seen")
}

/// Close the window list when its selector was not drawn this pass, whatever hid
/// it: a closed side, a collapsed or hidden flow pane, another tab. egui would hold
/// it open, and every "a popup is open" check with it. Call after the selector's
/// chance to draw.
pub fn close_unseen_window_list(ctx: &egui::Context) {
    let seen = ctx.data(|data| data.get_temp::<(egui::Id, u64)>(seen_id()));
    let pass = ctx.cumulative_pass_nr();
    if let Some((popup, _)) = seen.filter(|(_, at)| *at != pass) {
        ctx.memory_mut(|memory| memory.is_popup_open(popup).then(|| memory.close_popup()));
    }
}

/// A capsule at a header's far end, `header` spanning the header: fixed in the
/// row, so it neither fades nor lifts. The chart's divider keeps its own grab band
/// clear of it. Its icons stand bare on the header, with no case behind them.
/// Answers where it stands; `None` in a header too short to hold it.
pub fn draw_docked_capsule(
    ui: &mut egui::Ui,
    capsule: &Capsule,
    header: egui::Rect,
    (which, salt): (CapsuleSide, u64),
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) -> Option<egui::Rect> {
    let width = capsule.width_px();
    let corner = docked_capsule_corner(header.right(), header.top(), header.bottom(), width)?;
    let rect = egui::Rect::from_min_size(
        egui::pos2(corner.0, corner.1),
        egui::vec2(width, DOCKED_CAPSULE_HEIGHT_PX),
    );
    let ui = clipped_child(ui, rect, rect, ("pane_docked_capsule", salt, which as u8));
    let origin = docked_slot_origin(corner);
    capsule_slots(&ui, capsule, origin, salt, painted, intent);
    Some(rect)
}

/// The bare gear, sunk into a well while the panel is open. Left click opens or
/// closes; right click is the two-item menu.
fn settings_gear(
    ui: &egui::Ui,
    rect: egui::Rect,
    salt: u64,
    open: bool,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let id = ui.id().with(("pane_settings_gear", salt));
    let control = HeaderControl::Settings(PanelControl::Gear);
    let response = bare_key(ui, (rect, id), (icons::GEAR, open, None), control, painted);
    if response.clicked() {
        *intent = Some(HeaderIntent::ToggleSettings);
    }
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

/// A bare key on the flow band, recorded as `control`: `glyph` in the gear's
/// ink, the key-up ink while hovered or pressed, the pressed plate while
/// `open` or held down; a `lit` key wears that colour over a glow instead.
fn bare_key(
    ui: &egui::Ui,
    (rect, id): (egui::Rect, egui::Id),
    (glyph, open, lit): (&str, bool, Option<egui::Color32>),
    control: HeaderControl,
    painted: &mut PaintedControls,
) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let response = ui.interact(rect, id, egui::Sense::click());
    let pressed = open || response.is_pointer_button_down_on();
    let ink = lit.unwrap_or(if pressed || response.hovered() {
        palette.flow_key_up_ink()
    } else {
        palette.flow_gear_ink()
    });
    if pressed {
        let rounding = egui::Rounding::same(SEGMENT_ROUNDING);
        ui.painter()
            .rect_filled(rect, rounding, palette.flow_key_down());
        let shade = (1.0, palette.flow_key_down_shade());
        let lip = rect.x_range().shrink(SEGMENT_ROUNDING);
        ui.painter().hline(lip, rect.top() + 1.5, shade);
    }
    for (past, strength) in DOT_GLOW.iter().filter(|_| lit.is_some()) {
        let radius = SEGMENT_GLYPH_PX / 2.0 + past;
        ui.painter()
            .circle_filled(rect.center(), radius, ink.gamma_multiply(*strength));
    }
    let font = egui::FontId::new(SEGMENT_GLYPH_PX, palette.body_font(SEGMENT_GLYPH_PX).family);
    let at = egui::Align2::CENTER_CENTER;
    ui.painter().text(rect.center(), at, glyph, font, ink);
    record(ui, control, rect, painted);
    response
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

/// Words on the casing plate that keeps them readable over heat, its corner at `min`.
fn plated_text(
    painter: &egui::Painter,
    min: egui::Pos2,
    galley: std::sync::Arc<egui::Galley>,
    height: f32,
) {
    let palette = theme::get(painter.ctx());
    let width = galley.size().x + 2.0 * SIDE_LABEL_PADDING_PX;
    let plate =
        egui::Rect::from_min_size(min, egui::vec2(width, height.max(galley.size().y + 8.0)));
    let rounding = egui::Rounding::same(SIDE_LABEL_ROUNDING);
    painter.rect_filled(plate, rounding, palette.casing());
    let at = plate.left_center() + egui::vec2(SIDE_LABEL_PADDING_PX, -galley.size().y / 2.0);
    painter.galley(at, galley, palette.text_muted());
}

/// The tape's seam: a dashed line at the last print, and its plated lines after it.
pub fn draw_tape_seam(ui: &egui::Ui, side: egui::Rect, x: f32, lines: [Option<&str>; 2]) {
    let palette = theme::get(ui.ctx());
    let painter = ui.painter().with_clip_rect(side);
    let ends = [egui::pos2(x, side.top()), egui::pos2(x, side.bottom())];
    let stroke = egui::Stroke::new(1.0_f32, palette.amber());
    painter.add(egui::Shape::dashed_line(&ends, stroke, 4.0, 5.0));
    let font = palette.body_font(SEGMENT_WORD_SIZE);
    let ink = palette.text_muted();
    let lines = lines.map(|line| line.map(|l| painter.layout_no_wrap(l.into(), font.clone(), ink)));
    let px = [0, 1].map(|at| lines[at].as_ref().map(|line| line.size().x));
    let at = quantick_chart::market_fold::seam_plates(x, side.right(), side.y_range().into(), px);
    for (line, plate) in lines.into_iter().zip(at) {
        if let (Some(line), Some((left, top))) = (line, plate) {
            plated_text(&painter, egui::pos2(left, top), line, CAPSULE_HEIGHT_PX);
        }
    }
}

/// A capsule's slots, left to right from `origin`.
fn capsule_slots(
    ui: &egui::Ui,
    capsule: &Capsule,
    origin: (f32, f32),
    salt: u64,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let palette = theme::get(ui.ctx());
    let (mut x, y) = origin;
    for item in capsule.items() {
        let slot = egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(item.slot_px(), DOCKED_SLOT_HEIGHT_PX),
        );
        match item {
            CapsuleItem::Layer(glyph) => layer_glyph(ui, slot, salt, glyph, painted, intent),
            CapsuleItem::Sample(which, scale) => {
                sample(ui, slot, salt, which, scale, painted, intent);
            }
            CapsuleItem::SampleOff(_) => {
                ui.painter().circle_stroke(
                    slot.center(),
                    SAMPLE_OFF_DIAMETER_PX / 2.0,
                    egui::Stroke::new(SAMPLE_OFF_STROKE_PX, palette.off()),
                );
            }
        }
        x = slot.right() + SLOT_GAP_PX;
    }
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
    let palette = theme::get(ui.ctx());
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
        palette.text_primary(),
    );
    record(ui, HeaderControl::Sample(which), rect, painted);
    response.on_hover_text(which.name());
}

/// The sample's value, floating just below its slot so nothing shifts.
fn paint_value(ui: &egui::Ui, id: egui::Id, rect: egui::Rect, scale: f32) {
    let palette = theme::get(ui.ctx());
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(egui::Order::Tooltip, id));
    let galley = painter.layout_no_wrap(
        sample_value_text(scale),
        palette.mono_font(11.0),
        palette.text_primary(),
    );
    let at = egui::pos2(
        rect.center().x - galley.size().x / 2.0,
        rect.top() + rect.height().max(palette.layout(CAPSULE_HEIGHT_PX)) + VALUE_GAP_PX,
    );
    painter.rect_filled(
        egui::Rect::from_min_size(at, galley.size()).expand(2.0),
        egui::Rounding::same(2.0),
        palette.chrome(),
    );
    painter.galley(at, galley, palette.text_primary());
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
    let palette = theme::get(ui.ctx());
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
            palette.bare_hover(),
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
    let palette = theme::get(ui.ctx());
    [LaneSide::Trail, LaneSide::Tape].map(|side| {
        let word = state.map_or(side_word(side), |state| SideSegment::of(side, state).word());
        ui.fonts(|fonts| {
            fonts
                .layout_no_wrap(
                    word.to_owned(),
                    palette.body_font(SEGMENT_WORD_SIZE),
                    palette.text_primary(),
                )
                .size()
                .x
        })
    })
}

/// A key `width` wide from `left`, centred on `line`'s height.
fn key_rect(left: f32, width: f32, line: egui::Rect, height: f32) -> egui::Rect {
    let min = egui::pos2(left, line.center().y - height / 2.0);
    egui::Rect::from_min_size(min, egui::vec2(width, height))
}

/// The Trail and Tape keys from `room`'s left edge, a few points apart, in no
/// frame and with no body. `words` holds their widths while the line has room.
fn side_control(
    ui: &egui::Ui,
    room: egui::Rect,
    salt: u64,
    state: &FlowHeaderState,
    words: Option<[f32; 2]>,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let palette = theme::get(ui.ctx());
    let mut left = room.left();
    for (at, segment) in SideSegment::pair(state).into_iter().enumerate() {
        let rect = key_rect(
            left,
            segment_px(words.map(|words| words[at])),
            room,
            palette.layout(SEGMENT_HEIGHT_PX),
        );
        side_segment(ui, rect, salt, segment, words.is_some(), painted, intent);
        left = rect.right() + SEGMENT_GAP_PX;
    }
}

/// A key's dot and ink: lit, unlit (white ink under the pointer), or the market's
/// red, the slate surface's warning ink, which reads on the band in both themes.
fn key_inks(ui: &egui::Ui, ink: SegmentInk, hovered: bool) -> [egui::Color32; 2] {
    let palette = theme::get(ui.ctx());
    match ink {
        SegmentInk::Lit => [palette.flow_key_lit_dot(), palette.flow_key_up_ink()],
        SegmentInk::Unlit if hovered => [palette.flow_key_unlit_dot(), palette.flow_key_up_ink()],
        SegmentInk::Unlit => [palette.flow_key_unlit_dot(), palette.flow_key_unlit_ink()],
        SegmentInk::MarketClosed => [theme::bookmap(ui.ctx()).warn(); 2],
    }
}

/// One key on the band: a dot and its word, with no body. A lit dot glows. A
/// refusing key takes the click and does nothing; its tooltip says why.
fn side_segment(
    ui: &egui::Ui,
    rect: egui::Rect,
    salt: u64,
    segment: SideSegment,
    words: bool,
    painted: &mut PaintedControls,
    intent: &mut Option<HeaderIntent>,
) {
    let palette = theme::get(ui.ctx());
    let id = ui.id().with(("side", salt, segment.side as u8));
    let response = ui.interact(rect, id, egui::Sense::click());
    if response.clicked()
        && let Some(asked) = segment.click()
    {
        *intent = Some(asked);
    }
    let (hovered, pressed) = (response.hovered(), response.is_pointer_button_down_on());
    let dim = if pressed { KEY_PRESSED_OPACITY } else { 1.0 };
    let [dot, ink] = key_inks(ui, segment.ink, hovered).map(|colour| colour.gamma_multiply(dim));
    let painter = ui.painter();
    let center = rect.left_center() + egui::vec2(key_dot_inset(words), 0.0);
    let radius = KEY_DOT_PX / 2.0;
    if segment.ink == SegmentInk::Lit {
        let glow = if hovered { DOT_GLOW_HOVERED } else { DOT_GLOW };
        for (past, strength) in glow {
            painter.circle_filled(center, radius + past, dot.gamma_multiply(strength));
        }
    }
    painter.circle_filled(center, radius, dot);
    if words {
        painter.text(
            center + egui::vec2(radius + SEGMENT_WORD_GAP_PX, 0.0),
            egui::Align2::LEFT_CENTER,
            segment.word(),
            palette.body_font(SEGMENT_WORD_SIZE),
            ink,
        );
    }
    record(ui, HeaderControl::Side(segment.side), rect, painted);
    response.on_hover_ui(|ui| side_tip(ui, segment));
}

/// A key's tooltip: its name with the shortcut at the right, what its side
/// shows, then why it refuses, if it does.
fn side_tip(ui: &mut egui::Ui, segment: SideSegment) {
    ui.set_max_width(TIP_WIDTH_PX);
    let key = ui
        .ctx()
        .format_shortcut(&shortcuts::side_shortcut(segment.side));
    ui.horizontal(|ui| {
        ui.strong(segment.word());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(key)
        });
    });
    ui.label(segment.description());
    if let Some(note) = segment.note() {
        ui.label(note);
    }
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
}

fn paint_layer(painter: &egui::Painter, boxed: egui::Rect, toggle: LayerToggle, ink: GlyphInk) {
    let palette = theme::get(painter.ctx());
    let lit = ink == GlyphInk::Lit;
    let colour = if lit {
        palette.text_primary()
    } else {
        palette.off()
    };
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

/// The mark as the band writes it: bright monospace, spaced out.
fn mark_text(palette: &crate::theme::Palette, mark: &str) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        mark,
        0.0,
        egui::TextFormat {
            font_id: palette.mono_font(MARK_SIZE),
            color: palette.flow_ink(),
            extra_letter_spacing: MARK_LETTER_SPACING * palette.mono_font(MARK_SIZE).size
                / MARK_SIZE,
            ..Default::default()
        },
    );
    text
}

/// A control's words in full ink with a faint caret, as it lays them out.
fn caret_text(palette: &crate::theme::Palette, words: &str) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        words,
        0.0,
        egui::TextFormat::simple(palette.body_font(BARS_SIZE), palette.text_primary()),
    );
    text.append(
        icons::CARET_DOWN,
        3.0,
        egui::TextFormat::simple(
            palette.body_font(CARET_SIZE),
            if palette.resolved.dark {
                palette.text_faint().gamma_multiply(0.6)
            } else {
                palette.text_support()
            },
        ),
    );
    text
}

/// How wide the bars control will be, before it is laid out.
fn bars_width(ui: &egui::Ui, header: &PaneHeader) -> f32 {
    let palette = theme::get(ui.ctx());
    let galley = ui.fonts(|fonts| fonts.layout_job(caret_text(&palette, &header.bars_text)));
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
    let palette = theme::get(ui.ctx());
    let galley = ui.fonts(|fonts| fonts.layout_job(caret_text(&palette, &header.bars_text)));
    let padding = ui.spacing().button_padding;
    let (rect, control) = ui.allocate_exact_size(
        galley.size() + egui::vec2(2.0 * padding.x, 2.0 * padding.y),
        egui::Sense::click(),
    );
    if control.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::same(HOVER_ROUNDING),
            palette.bare_hover(),
        );
    }
    let at = rect.min + padding;
    if rebuilding {
        // The one place a recut spins: the control the hand just changed, its
        // words dimmed under the spin.
        ui.painter().galley(at, galley, palette.text_faint());
        ui.put(
            egui::Rect::from_center_size(
                egui::pos2(rect.right() - REBUILD_SPINNER_PX / 2.0, rect.center().y),
                egui::Vec2::splat(REBUILD_SPINNER_PX),
            ),
            egui::Spinner::new()
                .size(REBUILD_SPINNER_PX)
                .color(palette.loading_meta()),
        );
    } else {
        ui.painter().galley(
            at + egui::vec2(crate::theme::BOLD_STRIKE_PX, 0.0),
            galley.clone(),
            palette.text_primary(),
        );
        ui.painter().galley(at, galley, palette.text_primary());
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
