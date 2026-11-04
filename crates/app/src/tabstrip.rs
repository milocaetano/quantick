//! The market tab strip and the source picker behind its `+`
//! (`docs/ux/ui-design-model.md` §11).
//!
//! One chip per open market in the native title strip. The picker shares the
//! toolbar's configured source catalog.

use eframe::egui;
use quantick_session::tab_chip::{ChipMeasure, CloseSlot, close_slot, fit_chip, shortened_name};
use quantick_workspace::arrangement::strip::{
    StripSurface, TabFeedDot, chip_parts, market_slots, visible_tabs,
};

use crate::theme;

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

/// The logo, filling a 30 pt tab inside its 36 pt slot.
const HOME_LOGO_PX: f32 = 28.0;
const HOME_TEXT_PX: f32 = 12.0;
/// The window edge to the first tab, then the tab edge to the logo.
pub(crate) const STRIP_EDGE_PX: f32 = 10.0;
pub(crate) const HOME_PADDING_LEFT_PX: f32 = 10.0;
pub(crate) const HOME_LOGO_GAP_PX: f32 = 10.0;
pub(crate) const HOME_PADDING_RIGHT_PX: f32 = 14.0;
/// The logo alone, centred, with the labelled form's margin on both sides.
pub(crate) const HOME_MIN_WIDTH_PX: f32 = HOME_PADDING_LEFT_PX * 2.0 + HOME_LOGO_PX;

/// What one tab looks like on the strip this frame.
pub struct TabChip<'a> {
    pub id: u64,
    /// `SYMBOL · venue`, composed by the tab when its market last changed.
    pub label: &'a str,
    /// Whether this tab is playing a recording rather than streaming.
    pub replaying: bool,
    /// The dot's state. Amber stays reserved for an edited tab.
    pub dot: TabFeedDot,
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
    let labelled = (HOME_PADDING_LEFT_PX
        + HOME_LOGO_PX
        + HOME_LOGO_GAP_PX
        + label.iter().map(|galley| galley.size().x).sum::<f32>()
        + HOME_PADDING_RIGHT_PX)
        .max(HOME_MIN_WIDTH_PX);
    let room = ui.available_width() - controls_width - ui.spacing().item_spacing.x;
    let slots = market_slots(room, labelled, CHIP_PREFERRED_WIDTH_PX);
    let visible = visible_tabs(chips.len(), drawn.reveal.unwrap_or(active), slots);
    let shown = (visible.len() == chips.len()).then_some((label, labelled));
    // A compact strip has no room for the margins: the logo fills the tab.
    let bare = if compact {
        CHIP_HEIGHT
    } else {
        HOME_MIN_WIDTH_PX
    };
    let mut action =
        draw_home_tab(ui, surface.is_home(), shown, bare, drawn).then_some(TabAction::Home);
    let closable = chips.len() > 1 || drawn.allow_close_last;
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
            let response = response.on_hover_text(if chip.replaying {
                format!("{} (replay)", chip.label)
            } else {
                chip.label.to_owned()
            });
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
            let lit = selected || response.hovered();
            paint_chip_face(ui, chip, body, selected, lit);
            // Reserve a stable close target so hover never shifts adjacent controls.
            {
                let close_target = egui::Rect::from_min_size(
                    ui.next_widget_position(),
                    egui::vec2(CLOSE_HIT_WIDTH_PX, CHIP_HEIGHT),
                );
                let hovered = response.hovered() || ui.rect_contains_pointer(close_target);
                let slot = close_slot(selected, hovered, chip.modified);
                let close = ui
                    .add_enabled_ui(closable, |ui| {
                        ui.add_visible(
                            slot == CloseSlot::Close,
                            egui::Button::new(egui::RichText::new("\u{00d7}").size(CLOSE_GLYPH_PX))
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
                        .circle_filled(center, CHIP_DOT_RADIUS_PX, theme::AMBER);
                }
                drawn.close.push(close.rect);
                let chip_bounds = response.rect.union(close.rect).intersect(ui.clip_rect());
                if chip_bounds.is_positive() {
                    drawn.tab_ids.push((chip.id, chip_bounds));
                }
                paint_tab_surface(
                    ui,
                    surface,
                    response.rect.union(close.rect),
                    selected,
                    response.hovered() || close.hovered(),
                    response.has_focus() || close.has_focus(),
                );
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
                            if chip.replaying {
                                text = text.color(theme::REPLAY);
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

/// The dot, the symbol in its heavier strike and the workspace in muted ink,
/// `Scalp •` with the amber mark while edited. The name is paint, never a
/// control: the status bar's cell switches the workspace.
fn paint_chip_face(ui: &egui::Ui, chip: &TabChip<'_>, body: egui::Rect, selected: bool, lit: bool) {
    let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
    let center_y = body.center().y;
    let dot_x = body.left() + CHIP_PADDING_LEFT_PX + CHIP_DOT_RADIUS_PX;
    painter.circle_filled(
        egui::pos2(dot_x, center_y),
        CHIP_DOT_RADIUS_PX,
        match chip.dot {
            TabFeedDot::Live => theme::BUY,
            TabFeedDot::Stopped => theme::TEXT_MUTED,
            TabFeedDot::Replay => theme::REPLAY,
        },
    );
    let (symbol, _venue_on_hover) = chip_parts(chip.label);
    let ink = if chip.replaying {
        theme::REPLAY
    } else if lit {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };
    let text_x = dot_x + CHIP_DOT_RADIUS_PX + CHIP_DOT_GAP_PX;
    let laid = |text: String, px: f32, color: egui::Color32| {
        ui.fonts(|fonts| fonts.layout_no_wrap(text, egui::FontId::proportional(px), color))
    };
    let galley = laid(symbol.to_owned(), SYMBOL_TEXT_PX, ink);
    let symbol_width = galley.size().x;
    let at = egui::pos2(text_x, center_y - galley.size().y / 2.0);
    painter.galley(at, galley.clone(), ink);
    painter.galley(at + egui::vec2(SEMIBOLD_STRIKE_PX, 0.0), galley, ink);

    let mark = chip
        .modified
        .then(|| laid(EDITED_MARK.to_owned(), SUFFIX_TEXT_PX, theme::AMBER));
    let gapped = |galley: &Option<std::sync::Arc<egui::Galley>>| {
        galley
            .as_ref()
            .map_or(0.0, |galley| galley.size().x + NAME_MARK_GAP_PT)
    };
    let name_ink = theme::TEXT_MUTED;
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
    let mut x = text_x + symbol_width + CHIP_DOT_GAP_PX;
    let name = (fit.name_letters > 0).then(|| {
        laid(
            shortened_name(&chip.workspace, fit.name_letters),
            SUFFIX_TEXT_PX,
            name_ink,
        )
    });
    for galley in [name, mark.filter(|_| fit.mark)].into_iter().flatten() {
        let at = egui::pos2(x, center_y - galley.size().y / 2.0);
        x += galley.size().x + NAME_MARK_GAP_PT;
        painter.galley(at, galley, theme::TEXT_MUTED);
    }
}

type HomeLabel = [std::sync::Arc<egui::Galley>; 2];

/// The wordmark: "Quan" lifts on hover and while active, "tick" never changes.
fn home_label(ui: &egui::Ui) -> HomeLabel {
    let font = egui::FontId::proportional(HOME_TEXT_PX);
    ["Quan", "tick"].map(|part| {
        ui.fonts(|f| f.layout_no_wrap(part.to_owned(), font.clone(), egui::Color32::PLACEHOLDER))
    })
}

/// The first tab: logo and the "Quantick" wordmark, or the logo alone and
/// centred in `bare` points when `label` is `None`. Shaped and lit like a
/// market tab, with no close target. Returns whether it was pressed.
fn draw_home_tab(
    ui: &mut egui::Ui,
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
    let hovered = response.hovered();
    paint_tab_surface(ui, surface, rect, active, hovered, response.has_focus());
    let logo_x = if label.is_some() {
        rect.left() + HOME_PADDING_LEFT_PX + HOME_LOGO_PX / 2.0
    } else {
        rect.center().x
    };
    let logo = egui::Rect::from_center_size(
        egui::pos2(logo_x, rect.center().y),
        egui::Vec2::splat(HOME_LOGO_PX),
    );
    crate::surfaces::home::paint_logo(ui, logo);
    if let Some(([quan, tick], _)) = label {
        let mut x = logo.right() + HOME_LOGO_GAP_PX;
        let ink = if active || hovered {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_MUTED
        };
        for (galley, color) in [(quan, ink), (tick, theme::BRAND)] {
            let at = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
            x += galley.size().x;
            ui.painter().galley(at, galley, color);
        }
    }
    response.clicked()
}

fn paint_tab_surface(
    ui: &egui::Ui,
    slot: egui::layers::ShapeIdx,
    bounds: egui::Rect,
    selected: bool,
    hovered: bool,
    focused: bool,
) {
    let mut rect = bounds;
    rect.max.y = ui.max_rect().bottom();
    // The active tab wears the toolbar's fill, so the two read as one sheet.
    let fill = if selected {
        theme::CHROME
    } else {
        theme::BARE_HOVER
    };
    if selected || hovered || focused {
        ui.painter().set(
            slot,
            egui::Shape::rect_filled(
                rect,
                egui::Rounding {
                    nw: CHIP_ROUNDING_PX,
                    ne: CHIP_ROUNDING_PX,
                    sw: 0.0,
                    se: 0.0,
                },
                fill,
            ),
        );
    } else {
        ui.painter().set(
            slot,
            egui::Shape::line_segment(
                [
                    egui::pos2(rect.right(), rect.top() + 7.0),
                    egui::pos2(rect.right(), rect.bottom() - 7.0),
                ],
                egui::Stroke::new(1.0_f32, theme::BORDER),
            ),
        );
    }
    if focused {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            CHIP_ROUNDING_PX,
            egui::Stroke::new(1.0_f32, theme::ACCENT),
        );
    }
}
