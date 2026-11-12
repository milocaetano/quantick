//! Paints the settings panels' block structure as
//! [`quantick_appearance::panel_chrome`] decides it: the ground, the
//! full-bleed plates, a section's block under its band, and the subgroup
//! captions. The order-flow settings panel and the TRADES · SIM dock both
//! call these, so the two cannot drift apart.

use eframe::egui;
use quantick_appearance::panel_chrome::{
    self as chrome, BAND_PAD_X_PX, BAND_PAD_Y_PX, BAND_STATE_SIZE, BLOCK_EDGE_PX, BLOCK_PAD_PX,
    BLOCK_RADIUS_PX, CAPTION_GAP_PX, CAPTION_RULE_PX, CAPTION_SIZE, CAPTION_TRACKING_PX,
    FOLD_ICON_PX, PANEL_MARGIN_X_PX, PANEL_MARGIN_Y_PX, PLATE_PAD_X_PX, PLATE_PAD_Y_PX, Plate,
};

use crate::theme;

/// What a band leaves between its title and its state words.
const STATE_GAP_PX: f32 = 8.0;

/// A panel's own frame: the ground behind its blocks, inside the margin a
/// full-bleed plate reaches back across.
pub(crate) fn ground(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::none()
        .fill(*theme::get(ctx).get(chrome::GROUND.fill))
        .inner_margin(egui::Margin::symmetric(
            PANEL_MARGIN_X_PX,
            PANEL_MARGIN_Y_PX,
        ))
}

/// What `add` draws on a plate of `surface` reaching `bleed` past the ui's
/// edges, every word on it in the plate's ink.
fn plate<R>(
    ui: &mut egui::Ui,
    surface: Plate,
    [x, top, bottom]: [f32; 3],
    rounding: egui::Rounding,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let palette = theme::get(ui.ctx());
    let bleed = egui::Margin {
        left: -x,
        right: -x,
        top: -top,
        bottom: -bottom,
    };
    egui::Frame::none()
        .fill(*palette.get(surface.fill))
        .rounding(rounding)
        .inner_margin(egui::Margin::symmetric(PLATE_PAD_X_PX, PLATE_PAD_Y_PX))
        .outer_margin(bleed)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.visuals_mut().override_text_color = Some(*palette.get(surface.ink));
            add(ui)
        })
        .inner
}

/// The panel's title plate, across its top margin; its buttons wear the
/// plate's ink, framed by it, and the band's fill under the pointer.
pub(crate) fn title_plate<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let reach = [PANEL_MARGIN_X_PX, PANEL_MARGIN_Y_PX, 0.0];
    plate(ui, chrome::TITLE_PLATE, reach, egui::Rounding::ZERO, |ui| {
        let palette = theme::get(ui.ctx());
        let ink = *palette.get(chrome::TITLE_PLATE.ink);
        let widgets = &mut ui.visuals_mut().widgets;
        for look in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            look.fg_stroke.color = ink;
            look.bg_stroke = egui::Stroke::new(BLOCK_EDGE_PX, ink);
        }
        widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        widgets.hovered.weak_bg_fill = *palette.get(chrome::BAND.fill);
        add(ui)
    })
}

/// The panel's filter bar, across its sides, with the block edge's rule
/// under it.
pub(crate) fn filter_bar<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let reach = [PANEL_MARGIN_X_PX, 0.0, 0.0];
    let inner = plate(ui, chrome::FILTER_BAR, reach, egui::Rounding::ZERO, add);
    let rect = ui.min_rect().expand2(egui::vec2(PANEL_MARGIN_X_PX, 0.0));
    rule(
        ui,
        rect.x_range(),
        ui.min_rect().bottom(),
        chrome::BLOCK_EDGE,
    );
    inner
}

/// The panel's footer plate, across its sides.
pub(crate) fn panel_footer<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let reach = [PANEL_MARGIN_X_PX, 0.0, 0.0];
    plate(ui, chrome::FOOTER, reach, egui::Rounding::ZERO, add)
}

/// A block's footer plate: the last thing in a [`band`]'s body, reaching
/// the block's edges and its rounded foot.
pub(crate) fn block_footer<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let foot = BLOCK_RADIUS_PX - BLOCK_EDGE_PX;
    let rounding = egui::Rounding {
        nw: 0.0,
        ne: 0.0,
        sw: foot,
        se: foot,
    };
    let reach = [BLOCK_PAD_PX, 0.0, BLOCK_PAD_PX];
    plate(ui, chrome::FOOTER, reach, rounding, add)
}

/// A white block inside its edge, holding what `add` draws.
pub(crate) fn block<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let palette = theme::get(ui.ctx());
    egui::Frame::none()
        .fill(*palette.get(chrome::BLOCK.fill))
        .stroke(egui::Stroke::new(
            BLOCK_EDGE_PX,
            *palette.get(chrome::BLOCK_EDGE),
        ))
        .rounding(BLOCK_RADIUS_PX)
        .inner_margin(BLOCK_EDGE_PX)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A section's header band across the top of its [`block`]: the fold
/// triangle and `title` in the band's ink, `state` on its right in the
/// state ink, cut to the room left. The whole band answers the pointer (a
/// pointing hand, the hover fill); the body shows, padded, while `open`.
pub(crate) fn band<R>(
    ui: &mut egui::Ui,
    id: egui::Id,
    (title, state): (&str, &str),
    open: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> (egui::Response, Option<R>) {
    let palette = theme::get(ui.ctx());
    let mut fold =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open);
    fold.set_open(open);
    let openness = fold.openness(ui.ctx());
    let ink = *palette.get(chrome::BAND.ink);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let words = ui.painter().layout_no_wrap(title.to_owned(), font, ink);
    let size = egui::vec2(ui.available_width(), words.size().y + 2.0 * BAND_PAD_Y_PX);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let kind = egui::WidgetType::CollapsingHeader;
    response.widget_info(|| egui::WidgetInfo::labeled(kind, ui.is_enabled(), title));
    if ui.is_rect_visible(rect) {
        let top = BLOCK_RADIUS_PX - BLOCK_EDGE_PX;
        let foot = if openness > 0.0 { 0.0 } else { top };
        let rounding = egui::Rounding {
            nw: top,
            ne: top,
            sw: foot,
            se: foot,
        };
        let fill = response.hovered().then_some(chrome::BAND_HOVER);
        let fill = *palette.get(fill.unwrap_or(chrome::BAND.fill));
        ui.painter().rect_filled(rect, rounding, fill);
        let icon = egui::Rect::from_center_size(
            egui::pos2(
                rect.left() + BAND_PAD_X_PX + FOLD_ICON_PX / 2.0,
                rect.center().y,
            ),
            egui::Vec2::splat(FOLD_ICON_PX),
        );
        fold_icon(ui, &response.clone().with_new_rect(icon), openness, ink);
        let at = egui::pos2(icon.right() + ui.spacing().icon_spacing, rect.center().y);
        let title_right = at.x + words.size().x;
        ui.painter()
            .galley(at - egui::vec2(0.0, words.size().y / 2.0), words, ink);
        let room = rect.right() - BAND_PAD_X_PX - title_right - STATE_GAP_PX;
        if room > 0.0 {
            let state_ink = *palette.get(chrome::BAND_STATE.ink);
            let mut job = egui::text::LayoutJob::simple_singleline(
                state.to_owned(),
                palette.mono_font(BAND_STATE_SIZE),
                state_ink,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(room);
            let galley = ui.fonts(|fonts| fonts.layout_job(job));
            let at = egui::pos2(
                rect.right() - BAND_PAD_X_PX - galley.size().x,
                rect.center().y,
            );
            ui.painter().galley(
                at - egui::vec2(0.0, galley.size().y / 2.0),
                galley,
                state_ink,
            );
        }
    }
    let shown = fold.show_body_unindented(ui, |ui| {
        egui::Frame::none()
            .inner_margin(BLOCK_PAD_PX)
            .show(ui, body)
            .inner
    });
    (response, shown.map(|shown| shown.inner))
}

/// A subgroup's caption: its name in capitals over a rule across the body.
pub(crate) fn caption(ui: &mut egui::Ui, name: &str) {
    ui.add_space(CAPTION_GAP_PX);
    let words = caption_words(ui, name);
    let shown = ui.add(egui::Label::new(words).selectable(false));
    rule(
        ui,
        ui.max_rect().x_range(),
        shown.rect.bottom(),
        chrome::CAPTION_RULE,
    );
}

/// A subgroup that still folds, its header worn as a [`caption`]: the fold
/// triangle in the caption's ink, the rule under the header line.
pub(crate) fn folding_caption<R>(
    ui: &mut egui::Ui,
    (name, salt): (&str, &str),
    default_open: bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    ui.add_space(CAPTION_GAP_PX);
    let ink = *theme::get(ui.ctx()).get(chrome::CAPTION.ink);
    let shown = egui::CollapsingHeader::new(caption_words(ui, name))
        .id_salt(salt)
        .default_open(default_open)
        .icon(move |ui, openness, response| fold_icon(ui, response, openness, ink))
        .show(ui, body);
    let header = shown.header_response.rect;
    rule(
        ui,
        ui.max_rect().x_range(),
        header.bottom(),
        chrome::CAPTION_RULE,
    );
    shown.body_returned
}

fn caption_words(ui: &egui::Ui, name: &str) -> egui::RichText {
    egui::RichText::new(chrome::caption_words(name))
        .size(CAPTION_SIZE)
        .extra_letter_spacing(CAPTION_TRACKING_PX)
        .color(*theme::get(ui.ctx()).get(chrome::CAPTION.ink))
}

fn rule(ui: &egui::Ui, across: egui::Rangef, y: f32, token: quantick_appearance::Token) {
    let stroke = egui::Stroke::new(CAPTION_RULE_PX, *theme::get(ui.ctx()).get(token));
    ui.painter().hline(across, y + CAPTION_RULE_PX, stroke);
}

/// egui's fold triangle in `ink`, `FOLD_ICON_PX` across at the centre of
/// `at` (egui draws in three quarters of its rect): right shut, down open.
fn fold_icon(ui: &mut egui::Ui, at: &egui::Response, openness: f32, ink: egui::Color32) {
    let side = egui::Vec2::splat(FOLD_ICON_PX / 0.75);
    let rect = egui::Rect::from_center_size(at.rect.center(), side);
    let saved = ui.visuals().widgets.clone();
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.fg_stroke.color = ink;
    widgets.hovered.fg_stroke.color = ink;
    widgets.active.fg_stroke.color = ink;
    egui::collapsing_header::paint_default_icon(ui, openness, &at.clone().with_new_rect(rect));
    ui.visuals_mut().widgets = saved;
}
