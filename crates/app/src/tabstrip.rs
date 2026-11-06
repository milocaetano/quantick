//! The market tab strip and the source picker behind its `+`
//! (`docs/ux/ui-design-model.md` §11).
//!
//! One chip per open market in the native title strip. The picker shares the
//! toolbar's configured source catalog.

use eframe::egui;
use quantick_appearance::tab_strip::{MarkShape, feed_shape, feed_token, tab_fill, tab_ink};
use quantick_session::tab_chip::{ChipMeasure, CloseSlot, close_slot, fit_chip, shortened_name};
use quantick_workspace::arrangement::strip::{
    StripSurface, TabFeed, chip_parts, home_label_fits, market_slots, visible_tabs,
};

use crate::theme::{self, Palette};

const STRIP_CONTROLS_WIDTH_PX: f32 = 66.0;
const CHIP_PREFERRED_WIDTH_PX: f32 = 180.0;
const CHIP_MAX_WIDTH_PX: f32 = 220.0;
const CHIP_MIN_WIDTH_PX: f32 = 44.0;
/// A window so narrow the strip is in its compact form: the × and a sliver.
const CHIP_COMPACT_MIN_WIDTH_PX: f32 = 38.0;
pub(crate) const CLOSE_HIT_WIDTH_PX: f32 = 24.0;
/// What is kept right of the close target, so it never touches the next tab.
const CHIP_RIGHT_PADDING_PX: f32 = 2.0;
pub(crate) const CHIP_HEIGHT: f32 = 30.0;
const CHIP_ROUNDING_PX: f32 = 6.0;

const SYMBOL_TEXT_PX: f32 = 13.0;
const SUFFIX_TEXT_PX: f32 = 12.0;
const CLOSE_GLYPH_PX: f32 = 14.0;
const ADD_GLYPH_PX: f32 = 17.0;
const ADD_WIDTH_PX: f32 = 30.0;
const CHIP_PADDING_LEFT_PX: f32 = 10.0;
const CHIP_DOT_RADIUS_PX: f32 = 3.0;
const CHIP_DOT_GAP_PX: f32 = 7.0;
/// The gap between the workspace name and the edited mark.
const NAME_MARK_GAP_PT: f32 = 3.0;
/// What marks a tab holding edits.
const EDITED_MARK: &str = "\u{2022}";
/// How far a second strike of the symbol sits from the first. The bundled face
/// has one weight, and this is what reads as 600 at 13 pt.
const SEMIBOLD_STRIKE_PX: f32 = 0.35;

/// The plate-less Q, the wordmark's first letter: its 22 px box, the glyph inside.
const HOME_Q: egui::Vec2 = egui::Vec2::splat(22.0);
const HOME_TEXT_PX: f32 = 15.0;
// Window edge to the Home tab, tab edge to the Q, Q to text, text to edge.
pub(crate) const STRIP_EDGE_PX: f32 = 10.0;
pub(crate) const HOME_PADDING_LEFT_PX: f32 = 9.0;
pub(crate) const HOME_LOGO_GAP_PX: f32 = 2.0;
pub(crate) const HOME_PADDING_RIGHT_PX: f32 = 14.0;
/// The Q alone, centred in its own height with the labelled form's margin.
pub(crate) const HOME_MIN_WIDTH_PX: f32 = HOME_PADDING_LEFT_PX * 2.0 + HOME_Q.y;

/// What one tab looks like on the strip this frame.
pub struct TabChip<'a> {
    pub id: u64,
    /// `SYMBOL · venue`, composed by the tab when its market last changed.
    pub label: &'a str,
    /// The feed's mark. Amber stays reserved for an edited tab.
    pub feed: TabFeed,
    /// The workspace the tab uses.
    pub workspace: String,
    /// Whether the tab holds edits it has not saved to that workspace.
    pub modified: bool,
}

/// What the strip asked the window to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Activate(usize),
    Close(usize),
    /// The `+`: open the source picker.
    New,
    /// The first tab: show Home over the canvas.
    Home,
}

/// Draw the strip and report the one action it produced.
///
/// `closable` is false for the last tab: closing it would leave the window
/// with no market and nothing to draw, so the affordance is disabled and says
/// why rather than vanishing.
#[derive(Default)]
pub(crate) struct StripDrawn {
    pub(crate) bounds: Option<egui::Rect>,
    pub(crate) drag_started: Option<u64>,
    pub(crate) reveal: Option<usize>,
    pub(crate) close: Vec<egui::Rect>,
    pub(crate) allow_close_last: bool,
    pub(crate) home: Option<egui::Rect>,
    pub(crate) tabs: Vec<(usize, egui::Rect)>,
    pub(crate) tab_ids: Vec<(u64, egui::Rect)>,
    pub(crate) add: Option<egui::Rect>,
    pub(crate) overflow: Option<egui::Rect>,
}

pub(crate) fn draw(
    ui: &mut egui::Ui,
    chips: &[TabChip<'_>],
    active: usize,
    surface: &StripSurface,
    drawn: &mut StripDrawn,
) -> Option<TabAction> {
    let palette = theme::get(ui.ctx());
    drawn.bounds = Some(ui.max_rect().intersect(ui.clip_rect()));
    drawn.drag_started = None;
    drawn.close.clear();
    drawn.tabs.clear();
    drawn.tab_ids.clear();
    drawn.add = None;
    drawn.overflow = None;
    let compact = ui.available_width() < 240.0;
    ui.spacing_mut().item_spacing.x = if compact { 2.0 } else { 4.0 };
    ui.spacing_mut().button_padding = egui::vec2(if compact { 2.0 } else { 8.0 }, 4.0);
    ui.spacing_mut().interact_size.y = CHIP_HEIGHT;
    let controls_width = if compact {
        56.0
    } else {
        STRIP_CONTROLS_WIDTH_PX
    };
    let close_reserve = CLOSE_HIT_WIDTH_PX + CHIP_RIGHT_PADDING_PX;
    let label = home_label(ui);
    let after_q = label.iter().map(|galley| galley.size().x).sum::<f32>() + HOME_LOGO_GAP_PX;
    let labelled = HOME_PADDING_LEFT_PX + HOME_Q.x + after_q + HOME_PADDING_RIGHT_PX;
    let room = ui.available_width() - controls_width - ui.spacing().item_spacing.x;
    let slots = market_slots(room, labelled, CHIP_PREFERRED_WIDTH_PX);
    let visible = visible_tabs(chips.len(), drawn.reveal.unwrap_or(active), slots);
    let fits = home_label_fits(chips.len(), slots, room, labelled, CHIP_PREFERRED_WIDTH_PX);
    let shown = fits.then_some((label, labelled));
    // A compact strip has no room for the margins: the Q alone in a square tab.
    let bare = [HOME_MIN_WIDTH_PX, CHIP_HEIGHT][usize::from(compact)];
    let mut action = draw_home_tab(ui, &palette, surface.is_home(), shown, bare, drawn)
        .then_some(TabAction::Home);
    let closable = chips.len() > 1 || drawn.allow_close_last;
    // The separator after the last unlit tab, dropped if the next one lights.
    let mut separator = None;
    let active_chip = surface.active_chip(active);
    let chip_width = ((ui.available_width() - controls_width) / visible.len().max(1) as f32).clamp(
        if compact {
            CHIP_COMPACT_MIN_WIDTH_PX
        } else {
            CHIP_MIN_WIDTH_PX
        },
        CHIP_MAX_WIDTH_PX,
    );
    for (index, chip) in chips
        .iter()
        .enumerate()
        .filter(|(index, _)| visible.contains(index))
    {
        ui.push_id(("market_tab", chip.id), |ui| {
            let selected = active_chip == Some(index);
            let surface = ui.painter().add(egui::Shape::Noop);
            ui.spacing_mut().item_spacing.x = 0.0;
            let (body, response) = ui.allocate_exact_size(
                egui::vec2((chip_width - close_reserve).max(0.0), CHIP_HEIGHT),
                egui::Sense::click_and_drag(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    selected,
                    chip.label,
                )
            });
            let tag = (chip.feed == TabFeed::Replay).then_some(" (replay)");
            let response = response.on_hover_text(format!("{}{}", chip.label, tag.unwrap_or("")));
            if response.clicked() && !selected {
                action = Some(TabAction::Activate(index));
            }
            if response.drag_started() {
                drawn.drag_started = Some(chip.id);
            }
            let bounds = response.rect.intersect(ui.clip_rect());
            if bounds.is_positive() {
                drawn.tabs.push((index, bounds));
            }
            // Reserve a stable close target so hover never shifts adjacent controls.
            {
                let close_target = egui::Rect::from_min_size(
                    ui.next_widget_position(),
                    egui::vec2(CLOSE_HIT_WIDTH_PX, CHIP_HEIGHT),
                );
                let close_hot = ui.rect_contains_pointer(close_target);
                let slot = close_slot(selected, response.hovered() || close_hot, chip.modified);
                let hot = close_hot && closable && slot == CloseSlot::Close;
                if hot {
                    let (center, radius) = (close_target.center(), CLOSE_HIT_WIDTH_PX / 2.0 - 2.0);
                    ui.painter()
                        .circle_filled(center, radius, palette.bare_hover());
                }
                let glyph = egui::RichText::new("\u{00d7}").size(CLOSE_GLYPH_PX);
                let close = ui
                    .add_enabled_ui(closable, |ui| {
                        ui.add_visible(
                            slot == CloseSlot::Close,
                            egui::Button::new(glyph.color(*palette.get(tab_ink(hot))))
                                .frame(false)
                                .min_size(egui::vec2(CLOSE_HIT_WIDTH_PX, CHIP_HEIGHT)),
                        )
                    })
                    .inner;
                let close = if closable {
                    close.on_hover_text("Close tab (Ctrl+W)")
                } else {
                    close.on_disabled_hover_text(
                        "The last tab stays open — a window with no market has nothing to show",
                    )
                };
                if slot == CloseSlot::Mark {
                    let center = close.rect.center();
                    ui.painter()
                        .circle_filled(center, CHIP_DOT_RADIUS_PX, palette.amber());
                }
                drawn.close.push(close.rect);
                let sheet = response.rect.union(close.rect);
                let chip_bounds = sheet.intersect(ui.clip_rect());
                if chip_bounds.is_positive() {
                    drawn.tab_ids.push((chip.id, chip_bounds));
                }
                let hovered = response.hovered() || close.hovered();
                let pointer = (hovered, response.has_focus() || close.has_focus());
                let lit = paint_tab_surface(ui, &palette, surface, sheet, selected, pointer, true);
                // The face reads lit whenever its sheet is: body, × or focus.
                paint_chip_face(ui, &palette, chip, body, selected, lit);
                if lit && let Some(before) = separator.take() {
                    ui.painter().set(before, egui::Shape::Noop);
                }
                separator = (!lit).then_some(surface);
                if close.clicked() {
                    action = Some(TabAction::Close(index));
                }
                ui.add_space(CHIP_RIGHT_PADDING_PX);
            }
        });
    }
    if visible.len() < chips.len() {
        let overflow = ui.push_id("market_overflow", |ui| {
            ui.menu_button(egui::RichText::new("...").size(17.0), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (index, chip) in chips.iter().enumerate() {
                            let mut text = egui::RichText::new(chip.label);
                            if chip.feed == TabFeed::Replay {
                                text = text.color(palette.replay_ink());
                            }
                            if ui
                                .push_id(chip.id, |ui| {
                                    ui.selectable_label(active_chip == Some(index), text)
                                })
                                .inner
                                .clicked()
                            {
                                action = Some(TabAction::Activate(index));
                                ui.close_menu();
                            }
                        }
                    });
            })
        });
        let bounds = overflow.inner.response.rect.intersect(ui.clip_rect());
        drawn.overflow = bounds.is_positive().then_some(bounds);
    }
    let add = ui
        .push_id("market_new", |ui| {
            ui.add_sized(
                [if compact { 24.0 } else { ADD_WIDTH_PX }, CHIP_HEIGHT],
                egui::Button::new(egui::RichText::new("+").size(ADD_GLYPH_PX)).frame(false),
            )
            .on_hover_text("Open another market (Ctrl+T)")
        })
        .inner;
    let bounds = add.rect.intersect(ui.clip_rect());
    drawn.add = bounds.is_positive().then_some(bounds);
    if add.clicked() {
        action = Some(TabAction::New);
    }
    action
}

/// The feed's mark, the symbol in its heavier strike, "Replay" on a recording
/// and the workspace in muted ink, `Scalp •` with the amber mark while edited.
/// The name is paint, never a control: the status bar's cell switches it.
fn paint_chip_face(
    ui: &egui::Ui,
    palette: &Palette,
    chip: &TabChip<'_>,
    body: egui::Rect,
    selected: bool,
    lit: bool,
) {
    let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
    let center_y = body.center().y;
    let dot_x = body.left() + CHIP_PADDING_LEFT_PX + CHIP_DOT_RADIUS_PX;
    let feed_ink = *palette.get(feed_token(chip.feed));
    paint_feed_mark(
        &painter,
        egui::pos2(dot_x, center_y),
        feed_shape(chip.feed),
        feed_ink,
    );
    let (symbol, _venue_on_hover) = chip_parts(chip.label);
    let ink = *palette.get(tab_ink(lit));
    let text_x = dot_x + CHIP_DOT_RADIUS_PX + CHIP_DOT_GAP_PX;
    let laid = |text: String, px: f32, color: egui::Color32| {
        ui.fonts(|fonts| fonts.layout_no_wrap(text, egui::FontId::proportional(px), color))
    };
    let galley = laid(symbol.to_owned(), SYMBOL_TEXT_PX, ink);
    let glyphs = galley.size().x;
    let at = egui::pos2(text_x, center_y - galley.size().y / 2.0);
    painter.galley(at, galley.clone(), ink);
    painter.galley(at + egui::vec2(SEMIBOLD_STRIKE_PX, 0.0), galley, ink);

    let mark = chip
        .modified
        .then(|| laid(EDITED_MARK.to_owned(), SUFFIX_TEXT_PX, palette.amber()));
    let gapped = |galley: &Option<std::sync::Arc<egui::Galley>>| {
        galley
            .as_ref()
            .map_or(0.0, |galley| galley.size().x + NAME_MARK_GAP_PT)
    };
    // The word stays with the symbol, so the name gives way before it does.
    let word = (chip.feed.word()).map(|word| laid(word.to_owned(), SUFFIX_TEXT_PX, feed_ink));
    let symbol_width = glyphs + gapped(&word);
    let name_ink = palette.text_muted();
    let name_width = |letters: usize| {
        let cut = shortened_name(&chip.workspace, letters);
        laid(cut, SUFFIX_TEXT_PX, name_ink).size().x
    };
    let fit = fit_chip(&ChipMeasure {
        available: body.width(),
        symbol: text_x - body.left() + symbol_width + CHIP_DOT_GAP_PX,
        mark: gapped(&mark),
        active: selected,
        close: 0.0,
        name_letters: chip.workspace.chars().count(),
        name_width: &name_width,
    });
    let mut x = text_x + glyphs + CHIP_DOT_GAP_PX;
    let name = (fit.name_letters > 0).then(|| {
        laid(
            shortened_name(&chip.workspace, fit.name_letters),
            SUFFIX_TEXT_PX,
            name_ink,
        )
    });
    let trail = [word, name, mark.filter(|_| fit.mark)];
    for galley in trail.into_iter().flatten() {
        let at = egui::pos2(x, center_y - galley.size().y / 2.0);
        x += galley.size().x + NAME_MARK_GAP_PT;
        painter.galley(at, galley, palette.text_muted());
    }
}

/// A dot, a play triangle or a hollow ring: the state survives without colour.
fn paint_feed_mark(painter: &egui::Painter, at: egui::Pos2, shape: MarkShape, ink: egui::Color32) {
    let r = CHIP_DOT_RADIUS_PX;
    let tip = [(-r, -r), (r, 0.0), (-r, r)].map(|(x, y)| at + egui::vec2(x, y));
    painter.add(match shape {
        MarkShape::Dot => egui::Shape::circle_filled(at, r, ink),
        MarkShape::Ring => egui::Shape::circle_stroke(at, r - 0.5, (1.0, ink)),
        MarkShape::Triangle => egui::Shape::convex_polygon(tip.into(), ink, egui::Stroke::NONE),
    });
}

type HomeLabel = [std::sync::Arc<egui::Galley>; 2];

/// After the Q, in Hack at −0.02 em: "uan" in primary ink, "tick" in the brand.
fn home_label(ui: &egui::Ui) -> HomeLabel {
    let font = egui::FontId::monospace(HOME_TEXT_PX);
    let mut format = egui::TextFormat::simple(font, egui::Color32::PLACEHOLDER);
    format.extra_letter_spacing = -0.02 * HOME_TEXT_PX;
    let job = |part: &str| egui::text::LayoutJob::single_section(part.into(), format.clone());
    ui.fonts(|f| ["uan", "tick"].map(|part| f.layout_job(job(part))))
}

/// The first tab: the Q and "uantick" centred on one line, or the Q alone and
/// centred in `bare` points when `label` is `None`. Shaped and lit like a
/// market tab, with no close target. Returns whether it was pressed.
fn draw_home_tab(
    ui: &mut egui::Ui,
    palette: &Palette,
    active: bool,
    label: Option<(HomeLabel, f32)>,
    bare: f32,
    drawn: &mut StripDrawn,
) -> bool {
    let width = label.as_ref().map_or(bare, |(_, width)| *width);
    let surface = ui.painter().add(egui::Shape::Noop);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, CHIP_HEIGHT), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, "Home")
    });
    let response = response.on_hover_text("Home · Ctrl+1");
    let bounds = rect.intersect(ui.clip_rect());
    drawn.home = bounds.is_positive().then_some(bounds);
    let lit = (response.hovered(), response.has_focus());
    paint_tab_surface(ui, palette, surface, rect, active, lit, false);
    let q_x = match label {
        Some(_) => rect.left() + HOME_PADDING_LEFT_PX + HOME_Q.x / 2.0,
        None => rect.center().x,
    };
    let q = egui::Rect::from_center_size(egui::pos2(q_x, rect.center().y), HOME_Q);
    let q = ui.painter().round_rect_to_pixels(q); // One grid for the Q and its text.
    crate::surfaces::home::paint_logo(ui, q, true);
    if let Some(([uan, tick], _)) = label {
        let mut x = q.right() + HOME_LOGO_GAP_PX;
        // The wordmark never dims: lit or not, "uan" stays in primary ink.
        for (galley, color) in [(uan, palette.text_primary()), (tick, palette.brand())] {
            // CSS centres the 15 px line box: the font's unrounded metrics.
            let glyph = galley.rows[0].glyphs[0];
            let y = q.center().y + glyph.font_ascent - glyph.font_height / 2.0 - glyph.pos.y;
            let at = egui::pos2(x, y);
            x += galley.size().x;
            ui.painter().galley(at, galley, color);
        }
    }
    response.clicked()
}

/// A lit tab's sheet, or the separator an unlit one ends with when
/// `separated`. Returns whether the tab is lit.
fn paint_tab_surface(
    ui: &egui::Ui,
    palette: &Palette,
    slot: egui::layers::ShapeIdx,
    bounds: egui::Rect,
    selected: bool,
    (hovered, focused): (bool, bool),
    separated: bool,
) -> bool {
    let painter = ui.painter();
    let mut rect = bounds;
    rect.max.y = ui.max_rect().bottom();
    let fill = tab_fill(selected, hovered || focused);
    if let Some(fill) = fill {
        // Only the active tab is outlined, and its outline runs past the
        // strip's floor, where the clip drops the bottom edge: the tab and the
        // toolbar read as one sheet.
        let outline = egui::Stroke::new(f32::from(u8::from(selected)), palette.border());
        let rounding = egui::Rounding {
            nw: CHIP_ROUNDING_PX,
            ne: CHIP_ROUNDING_PX,
            ..Default::default()
        };
        let sheet = rect.with_max_y(rect.bottom() + 2.0);
        let shape = egui::epaint::RectShape::new(sheet, rounding, *palette.get(fill), outline);
        painter.set(slot, shape);
    } else if separated {
        let top = egui::pos2(rect.right(), rect.top() + 7.0);
        let stroke = egui::Stroke::new(1.0_f32, palette.seam_line());
        let line = [top, egui::pos2(rect.right(), rect.bottom() - 7.0)];
        painter.set(slot, egui::Shape::line_segment(line, stroke));
    }
    if focused {
        let ring = egui::Stroke::new(2.0_f32, palette.accent());
        painter.rect_stroke(rect.shrink(1.0), CHIP_ROUNDING_PX, ring);
    }
    fill.is_some()
}
