//! The market tab strip and the source picker behind its `+`
//! (`docs/ux/ui-design-model.md` §11).
//!
//! One chip per open market in the native title strip. The picker shares the
//! toolbar's configured source catalog.

use eframe::egui;
use quantick_appearance::Token;
use quantick_appearance::tab_strip::{
    FeedMark, MarkShape, TAB_CORNER_PT, feed_shape, feed_token, glow_mesh, tab_ink, tab_layers,
};
use quantick_session::tab_chip::{
    ChipMeasure, CloseSlot, broker_name, close_slot, fit_chip, shortened_name,
};
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
pub(crate) const CHIP_HEIGHT: f32 = 30.0;
const CHIP_ROUNDING_PX: f32 = TAB_CORNER_PT;

const SYMBOL_TEXT_PX: f32 = 13.0;
const SUFFIX_TEXT_PX: f32 = 12.0;
const CLOSE_GLYPH_PX: f32 = 14.0;
const ADD_GLYPH_PX: f32 = 17.0;
const ADD_WIDTH_PX: f32 = 30.0;
const CHIP_PADDING_LEFT_PX: f32 = 10.0;
const CHIP_DOT_RADIUS_PX: f32 = 3.0;
const CHIP_DOT_GAP_PX: f32 = 7.0;
/// The gap between the chip's trailing words.
const NAME_MARK_GAP_PT: f32 = 3.0;
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
    /// The feed's mark.
    pub feed: TabFeed,
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
    (drawn, root): (&mut StripDrawn, &Palette),
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
    let controls_width = [STRIP_CONTROLS_WIDTH_PX, 56.0][usize::from(compact)];
    let close_reserve = CLOSE_HIT_WIDTH_PX;
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
    let active_chip = surface.active_chip(active);
    let floor = [CHIP_MIN_WIDTH_PX, CHIP_COMPACT_MIN_WIDTH_PX][usize::from(compact)];
    let chip_width = ((ui.available_width() - controls_width) / visible.len().max(1) as f32)
        .clamp(floor, CHIP_MAX_WIDTH_PX);
    for index in visible.clone() {
        let chip = &chips[index];
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
                let slot = close_slot(selected, response.hovered() || close_hot);
                let hot = close_hot && closable && slot == CloseSlot::Close;
                if hot {
                    let (center, radius) = (close_target.center(), CLOSE_HIT_WIDTH_PX / 2.0 - 2.0);
                    ui.painter()
                        .circle_filled(center, radius, root.bare_hover());
                }
                let glyph = egui::RichText::new("\u{00d7}").size(CLOSE_GLYPH_PX);
                let close = ui
                    .add_enabled_ui(closable, |ui| {
                        ui.add_visible(
                            slot == CloseSlot::Close,
                            egui::Button::new(glyph.color(*root.get(tab_ink(hot))))
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
                drawn.close.push(close.rect);
                let sheet = response.rect.union(close.rect);
                let chip_bounds = sheet.intersect(ui.clip_rect());
                if chip_bounds.is_positive() {
                    drawn.tab_ids.push((chip.id, chip_bounds));
                }
                let hovered = response.hovered() || close.hovered();
                let pointer = (hovered, response.has_focus() || close.has_focus());
                let lit = paint_tab_surface(ui, root, surface, sheet, selected, pointer);
                // The face reads lit whenever its sheet is: body, × or focus.
                paint_chip_face(ui, root, chip, body, lit);
                if close.clicked() {
                    action = Some(TabAction::Close(index));
                }
            }
        });
    }
    if visible.len() < chips.len() {
        let overflow = ui.push_id("market_overflow", |ui| {
            let dots = egui::Button::new(egui::RichText::new("...").size(17.0)).frame(false);
            egui::menu::menu_custom_button(ui, dots, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (index, chip) in chips.iter().enumerate() {
                            let mut text = egui::RichText::new(chip.label);
                            if chip.feed == TabFeed::Replay {
                                text = text.color(root.replay_ink());
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
/// and the broker in muted ink: `• WINZ26 MetaTrader 5`. No workspace and no
/// mark: the status bar's cell shows both.
fn paint_chip_face(
    ui: &egui::Ui,
    palette: &Palette,
    chip: &TabChip<'_>,
    body: egui::Rect,
    lit: bool,
) {
    let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
    let center_y = body.center().y;
    let dot_x = body.left() + CHIP_PADDING_LEFT_PX + CHIP_DOT_RADIUS_PX;
    let mark = feed_mark(chip.feed);
    let feed_ink = *palette.get(feed_token(mark));
    paint_feed_mark(&painter, egui::pos2(dot_x, center_y), mark, feed_ink);
    let (symbol, venue) = chip_parts(chip.label);
    let broker = broker_name(venue);
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
        let cut = shortened_name(broker, letters);
        laid(cut, SUFFIX_TEXT_PX, name_ink).size().x
    };
    let fit = fit_chip(&ChipMeasure {
        available: body.width(),
        symbol: text_x - body.left() + symbol_width + CHIP_DOT_GAP_PX,
        close: 0.0,
        name_letters: broker.chars().count(),
        name_width: &name_width,
    });
    let mut x = text_x + glyphs + CHIP_DOT_GAP_PX;
    let name = (fit.name_letters > 0).then(|| {
        laid(
            shortened_name(broker, fit.name_letters),
            SUFFIX_TEXT_PX,
            name_ink,
        )
    });
    let trail = [word, name];
    for galley in trail.into_iter().flatten() {
        let at = egui::pos2(x, center_y - galley.size().y / 2.0);
        x += galley.size().x + NAME_MARK_GAP_PT;
        painter.galley(at, galley, palette.text_muted());
    }
}

/// The workspace's feed state as the theme's mark.
pub(crate) const fn feed_mark(feed: TabFeed) -> FeedMark {
    match feed {
        TabFeed::Live => FeedMark::Live,
        TabFeed::Attention => FeedMark::Attention,
        TabFeed::Replay => FeedMark::Replay,
        TabFeed::Disconnected => FeedMark::Lost,
        TabFeed::Stopped => FeedMark::Stopped,
    }
}

/// A dot, a diamond, a play triangle or a hollow ring: the state survives without colour.
fn paint_feed_mark(painter: &egui::Painter, at: egui::Pos2, mark: FeedMark, ink: egui::Color32) {
    let r = CHIP_DOT_RADIUS_PX;
    // Corners in radii; the diamond's 1.25 gives it about the dot's area.
    let fill = |unit: &[(f32, f32)]| {
        let points = unit.iter().map(|&(x, y)| at + r * egui::vec2(x, y));
        egui::Shape::convex_polygon(points.collect(), ink, egui::Stroke::NONE)
    };
    painter.add(match feed_shape(mark) {
        MarkShape::Dot => egui::Shape::circle_filled(at, r, ink),
        MarkShape::Diamond => fill(&[(0.0, -1.25), (1.25, 0.0), (0.0, 1.25), (-1.25, 0.0)]),
        MarkShape::Ring => egui::Shape::circle_stroke(at, r - 0.5, (1.0, ink)),
        MarkShape::Triangle => fill(&[(-1.0, -1.0), (1.0, 0.0), (-1.0, 1.0)]),
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
/// centred in `bare` points when `label` is `None`. No tab shape: while Home is
/// shown a soft glow sits behind the Q. Returns whether it was pressed.
fn draw_home_tab(
    ui: &mut egui::Ui,
    palette: &Palette,
    active: bool,
    label: Option<(HomeLabel, f32)>,
    bare: f32,
    drawn: &mut StripDrawn,
) -> bool {
    let width = label.as_ref().map_or(bare, |(_, width)| *width);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, CHIP_HEIGHT), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, "Home")
    });
    let response = response.on_hover_text("Home · Ctrl+1");
    let bounds = rect.intersect(ui.clip_rect());
    drawn.home = bounds.is_positive().then_some(bounds);
    if response.has_focus() {
        let ring = (2.0_f32, palette.accent());
        ui.painter()
            .rect_stroke(rect.shrink(1.0), CHIP_ROUNDING_PX, ring);
    }
    let q_x = match label {
        Some(_) => rect.left() + HOME_PADDING_LEFT_PX + HOME_Q.x / 2.0,
        None => rect.center().x,
    };
    let q = egui::Rect::from_center_size(egui::pos2(q_x, rect.center().y), HOME_Q);
    let q = ui.painter().round_rect_to_pixels(q); // One grid for the Q and its text.
    if active {
        // Under the Q: `home_glow` (premultiplied, its alpha the peak).
        let (color, glow) = (*palette.get(Token::HomeGlow), glow_mesh());
        let vertex = |&([x, y], shade): &([f32; 2], f32)| egui::epaint::Vertex {
            pos: q.center() + egui::vec2(x, y),
            uv: egui::epaint::WHITE_UV,
            color: color.gamma_multiply(shade),
        };
        let vertices = glow.vertices.iter().map(vertex).collect();
        let (indices, mesh) = (glow.indices.clone(), egui::Mesh::default());
        ui.painter().add(egui::Mesh {
            indices,
            vertices,
            ..mesh
        });
    }
    crate::surfaces::home::paint_logo(ui, q, crate::surfaces::home::Logo::Q);
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

/// A market tab's [`tab_layers`], its base past the strip's floor, where the
/// clip opens it onto the toolbar. Returns whether the tab is lit.
fn paint_tab_surface(
    ui: &egui::Ui,
    palette: &Palette,
    slot: egui::layers::ShapeIdx,
    bounds: egui::Rect,
    selected: bool,
    (hovered, focused): (bool, bool),
) -> bool {
    let rect = bounds.with_max_y(ui.max_rect().bottom());
    let sheet = rect.with_max_y(rect.bottom() + 2.0);
    let layer = |layer: quantick_appearance::tab_strip::TabLayer| {
        let radius = CHIP_ROUNDING_PX - layer.corner_inset;
        let rounding = egui::Rounding {
            nw: radius,
            ne: radius,
            ..Default::default()
        };
        let fill = *palette.get(layer.fill);
        let edge = layer
            .outline
            .map(|token| egui::Stroke::new(1.0_f32, *palette.get(token)));
        let at = sheet.with_min_y(sheet.top() + layer.inset);
        let at = layer
            .height
            .map_or(at, |height| at.with_max_y(at.top() + height));
        egui::epaint::RectShape::new(at, rounding, fill, edge.unwrap_or_default()).into()
    };
    let shapes = tab_layers(selected).into_iter().map(layer);
    ui.painter().set(slot, egui::Shape::Vec(shapes.collect()));
    if focused {
        let ring = (2.0_f32, palette.accent());
        ui.painter()
            .rect_stroke(rect.shrink(1.0), CHIP_ROUNDING_PX, ring);
    }
    selected || hovered || focused
}
