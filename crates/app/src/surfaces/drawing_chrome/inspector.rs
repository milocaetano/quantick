//! The selected object's inspector, in both its hosts.
//!
//! Floating, it is non-modal by contract: it never captures the whole canvas
//! — but it is opaque to the pointer, so a press on it never falls through to
//! the chart. Pinned, it is a dock panel at the chart's side, declared with
//! the chrome so the canvas pays its width.
//!
//! Both hosts share the title bar, the body and one action applier, which is
//! what keeps "what does the pin do" from having two answers.

use eframe::egui;

use super::{
    Chrome as DrawingChromeSurface, DrawingChromeAsk, DrawingEnv, INSPECTOR_AREA_ID,
    INSPECTOR_DEFAULT_WIDTH_PX, INSPECTOR_FALLBACK_HEIGHT_PX, INSPECTOR_LEVELS_WIDTH_PX,
    INSPECTOR_MAX_WIDTH_PX, INSPECTOR_MIN_WIDTH_PX, INSPECTOR_OBJECT_GAP_PX,
    INSPECTOR_TITLE_GRIP_GLYPH_PX, INSPECTOR_TITLE_HEIGHT_PX, INSPECTOR_TITLE_PAD_X_PX,
    INSPECTOR_TITLE_TEXT_PX, INSPECTOR_TITLE_TEXT_X_PX, InspectorActions, RecordingPresetHost,
    clamp_into_chart,
};
use crate::drawings::Drawing;
use crate::theme;
use crate::widgets::{IconButton, TOOLBAR_ICON};
use egui_phosphor::regular as icons;
use quantick_chart_interaction::panel_placement::{Area, NEIGHBOURHOOD_PX, PanelPlacement};

mod sections;

use sections::body;

/// Initial position of the selected-drawing inspector, before [`placement`]
/// has a size and a bbox to place it from.
const DEFAULT_POSITION: egui::Pos2 = super::DRAWING_INSPECTOR_DEFAULT_POSITION;

use quantick_ui_model::drawing_chrome::{Intent, inspector as model};

#[derive(Default)]
pub(crate) struct View {
    pub(super) max_width: Option<f32>,
    pub(super) size: Option<egui::Vec2>,
    pub(super) crowding: bool,
    #[cfg(test)]
    pub pin_rect: Option<egui::Rect>,
}

impl View {
    /// How big the floating inspector is, best answer available: the size it
    /// was last drawn at, then egui's area memory, then the assumed default.
    fn size(&self, ctx: &egui::Context) -> egui::Vec2 {
        self.size
            .or_else(|| {
                ctx.memory(|memory| memory.area_rect(egui::Id::new(INSPECTOR_AREA_ID)))
                    .map(|rect| rect.size())
            })
            .unwrap_or(egui::vec2(
                INSPECTOR_DEFAULT_WIDTH_PX,
                INSPECTOR_FALLBACK_HEIGHT_PX,
            ))
    }
}

/// The inspector placement rule (`docs/ux/drawing-tools-2026-08.md` §D3),
/// owned by [`quantick_chart_interaction::panel_placement::place_beside`]: beside the object, clear of its neighbourhood.
/// A dragged panel never comes here: a hand's position wins from then on
/// (see [`model::State::place_by_hand`]).
pub(crate) fn placement(chart: egui::Rect, bbox: egui::Rect, size: egui::Vec2) -> PanelPlacement {
    let area = |rect: egui::Rect| Area::new(rect.left(), rect.top(), rect.right(), rect.bottom());
    model::placement(area(chart), area(bbox), [size.x, size.y])
}

/// Where a freshly opened floating inspector should sit, and how wide it may
/// be there (see [`placement`]). Measured in the candles' area, which an object's span
/// is computed from, so the popup leaves the axes, lane and rail clear.
fn target_placement(
    inspector: &View,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> Option<PanelPlacement> {
    let chart = crate::bands::drawing_area(env.chart_area?, env.candles_right);
    let bbox = env.selected_bbox?;
    Some(placement(chart, bbox, inspector.size(ctx)))
}

/// The pinned inspector: a dock panel at the chart's side.
pub(crate) fn draw_pinned(
    chrome: &mut DrawingChromeSurface,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let mut ask = DrawingChromeAsk::default();
    if !chrome.shared.pinned {
        return ask;
    }
    let Some(selected) = prologue(chrome, env, &mut ask) else {
        return ask;
    };
    let index = selected.index;
    chrome.emit(Intent::InspectorClaim(index));
    let mut edited = selected.drawing.clone();
    let mut recorder = RecordingPresetHost::new(env.presets);
    let mut actions = InspectorActions::default();
    let id = egui::Id::new((ctx.viewport_id(), "drawing_inspector_panel"));
    egui::SidePanel::right(id)
        .resizable(true)
        .default_width(INSPECTOR_DEFAULT_WIDTH_PX)
        .width_range(INSPECTOR_MIN_WIDTH_PX..=INSPECTOR_MAX_WIDTH_PX)
        .show(ctx, |ui| {
            actions = title_bar(chrome, ui, &edited, env, false);
            egui::ScrollArea::vertical().show(ui, |ui| {
                actions.merge(body(chrome, ui, &mut edited, env, &mut recorder));
            });
        });
    settle(chrome, ctx, &mut ask, actions, edited, recorder, index, env);
    ask
}

/// The floating inspector.
///
/// Opens beside the selection; once the user drags the title bar, the manual
/// position wins for the rest of the session (selection changes never snap it
/// back; the only automatic move is the re-clamp when the pane shrinks).
pub(crate) fn draw_floating(
    chrome: &mut DrawingChromeSurface,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let mut ask = DrawingChromeAsk::default();
    if chrome.shared.pinned {
        // The pinned panel already drew (and cleaned up) this frame.
        return ask;
    }
    let Some(selected) = prologue(chrome, env, &mut ask) else {
        return ask;
    };
    let index = selected.index;
    chrome.emit(Intent::InspectorPrepare {
        index,
        width: env.chart_area.map(|chart| chart.width()),
    });
    let Some(selection_changed) = chrome.state().inspector.prepared else {
        return ask;
    };
    // Automatic placement only while the window is untouched.
    if (selection_changed || std::mem::take(&mut chrome.view.inspector.crowding))
        && !chrome.state().inspector.moved
        && let Some(placed) = target_placement(&chrome.view.inspector, ctx, env)
    {
        chrome.emit(Intent::InspectorPosition(Some([placed.left, placed.top])));
        chrome.view.inspector.max_width = placed.max_width;
    }
    // Repair for drawing, never overwrite: a position that does not fit the
    // chart pane is clamped into it *for this frame*, and the point the
    // trader parked survives in `pos` untouched.
    //
    // The clamp used to be written back, which was harmless while the
    // position died with the process and is not now that the workspace keeps
    // it. Every reason the popup does not fit is temporary — a taller panel
    // for the tool just selected, the split canvas opened, the window pulled
    // narrow, a smaller second monitor — and writing the repair back would
    // ratchet the parked point away a little at a time, with no way back to
    // where the hand put it. The file records what the trader did; this line
    // decides only where it is drawn today.
    let inspector_size = chrome.view.inspector.size(ctx);
    let draw_at = chrome.state().inspector.pos.map(|[x, y]| {
        let position = egui::pos2(x, y);
        env.chart_area.map_or(position, |chart| {
            clamp_into_chart(position, inspector_size, chart)
        })
    });
    let mut edited = selected.drawing.clone();
    let mut recorder = RecordingPresetHost::new(env.presets);
    // The level editor earns the wider default the spec reserves for it.
    let default_width = if edited.tool.extra_tab().is_some() {
        INSPECTOR_LEVELS_WIDTH_PX
    } else {
        INSPECTOR_DEFAULT_WIDTH_PX
    };
    // Bounded by the window and scrolled inside it. A tool's panel can be
    // taller than the screen — the Fib level editor is — and an unbounded
    // window simply gets cut at the edge with no way to reach the rest. Rows
    // a trader cannot reach read as rows that do not exist, and the control
    // that was out of reach here was the Fib's own "extend", the one that
    // decides whether its targets project forward at all.
    let max_height = (ctx.screen_rect().height()
        - env.chart_area.map_or(0.0, |chart| chart.top())
        - 2.0 * INSPECTOR_OBJECT_GAP_PX)
        .max(INSPECTOR_FALLBACK_HEIGHT_PX);
    let mut window = egui::Window::new(edited.tool.settings_title())
        .id(egui::Id::new(INSPECTOR_AREA_ID))
        .title_bar(false)
        .default_pos(DEFAULT_POSITION)
        .default_width(default_width)
        .min_width(INSPECTOR_MIN_WIDTH_PX)
        .max_width(
            chrome
                .view
                .inspector
                .max_width
                .map_or(INSPECTOR_MAX_WIDTH_PX, |width| {
                    width.clamp(INSPECTOR_MIN_WIDTH_PX, INSPECTOR_MAX_WIDTH_PX)
                }),
        )
        .max_height(max_height)
        .movable(false)
        .interactable(true)
        .resizable(true);
    if let Some(position) = draw_at {
        window = window.current_pos(position);
    }
    let response = window.show(ctx, |ui| {
        let mut actions = title_bar(chrome, ui, &edited, env, true);
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                actions.merge(body(chrome, ui, &mut edited, env, &mut recorder));
            });
        actions
    });
    let drawn = response.as_ref().map(|response| response.response.rect);
    chrome.view.inspector.size = drawn.map(|rect| rect.size());
    // Placed again next frame if it reaches into the drawing's neighbourhood.
    // egui's first frame of a window is an invisible sizing pass, so a panel
    // drawn wider than assumed is corrected before it is seen. Not while a
    // hand holds the pointer (a resize), nor for a gutter-narrowed panel.
    chrome.view.inspector.crowding = !chrome.state().inspector.moved
        && chrome.view.inspector.max_width.is_none()
        && !ctx.input(|input| input.pointer.any_down())
        && drawn.zip(env.selected_bbox).is_some_and(|(rect, bbox)| {
            rect.intersect(bbox.expand(NEIGHBOURHOOD_PX - 1.0))
                .is_positive()
        });
    let actions = response
        .and_then(|response| response.inner)
        .unwrap_or_default();
    settle(chrome, ctx, &mut ask, actions, edited, recorder, index, env);
    ask
}

/// Shared prologue of both hosts: the selection, or the cleanup for none.
///
/// A gesture that outlives its object's selection is **committed**, never
/// dropped: the trader made those edits, and an entry thrown away here would
/// take a slider drag out of the undo history because the selection moved on
/// before the pointer came up.
fn prologue<'a>(
    chrome: &mut DrawingChromeSurface,
    env: &'a DrawingEnv<'a>,
    ask: &mut DrawingChromeAsk,
) -> Option<&'a super::SelectedDrawing<'a>> {
    ask.merge(chrome.emit(Intent::InspectorPrologue(env.selected_index())));
    env.selected.as_ref()
}

/// Fold one host's frame into the ask: the edited copy when something changed,
/// the preset writes, and everything [`super::apply_actions`] decides.
#[expect(
    clippy::too_many_arguments,
    reason = "the two hosts hand over exactly what they drew; bundling it into a \
              struct used once at each of two call sites would hide the arguments \
              rather than reduce them"
)]
fn settle(
    chrome: &mut DrawingChromeSurface,
    ctx: &egui::Context,
    ask: &mut DrawingChromeAsk,
    actions: InspectorActions,
    edited: Drawing,
    recorder: RecordingPresetHost<'_>,
    index: usize,
    env: &DrawingEnv<'_>,
) {
    // Handed back because it *differs*, not because a flag said so.
    //
    // `Tool::draw_extra_tab` returns "did anything change", and its contract
    // calls that an advisory hint the caller folds into the undo coalescing.
    // While the tab wrote through a `&mut` into the live store, a tab that
    // mutated and returned `false` still persisted the mutation. Editing a
    // copy would quietly promote that hint to a commit flag, and the
    // sixteenth tool — written from the unchanged doc comment — would lose
    // every edit it made with no compile error and no failing test. The
    // comparison costs one `PartialEq` on a copy this frame already made, and
    // only while a panel is open.
    if actions.edited
        || env
            .selected
            .as_ref()
            .is_none_or(|held| *held.drawing != edited)
    {
        ask.edited = Some(Box::new(edited));
    }
    ask.presets.extend(recorder.into_writes());
    ask.merge(super::apply_actions(chrome, ctx, actions, index, env));
}

/// The inspector's title bar, shared by both hosts: grip + title, then the
/// view controls (hide, pin, close) as icon buttons. In the floating host the
/// whole bar is the drag surface — the body never is, so a slider drag can
/// never move the window; double-click re-runs the automatic placement.
fn title_bar(
    chrome: &mut DrawingChromeSurface,
    ui: &mut egui::Ui,
    drawing: &Drawing,
    env: &DrawingEnv<'_>,
    floating: bool,
) -> InspectorActions {
    let palette = theme::get(ui.ctx());
    let mut actions = InspectorActions::default();
    let hidden = drawing.hidden;
    // The band belongs in the title, because that is where "which of these
    // two trend lines am I editing" is actually asked. Nothing is added on
    // the price band: it is where drawings have always lived, and a suffix on
    // every object would be noise.
    let title = match env.selected_band.as_deref() {
        Some(band) => format!("{} · {band}", drawing.tool.settings_title()),
        None => drawing.tool.settings_title().to_owned(),
    };
    let sense = if floating {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::hover()
    };
    let (bar_rect, bar) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), INSPECTOR_TITLE_HEIGHT_PX),
        sense,
    );
    if ui.is_rect_visible(bar_rect) {
        let painter = ui.painter();
        if floating {
            painter.text(
                egui::pos2(
                    bar_rect.left() + INSPECTOR_TITLE_PAD_X_PX,
                    bar_rect.center().y,
                ),
                egui::Align2::LEFT_CENTER,
                icons::DOTS_SIX_VERTICAL,
                palette.body_font(INSPECTOR_TITLE_GRIP_GLYPH_PX),
                palette.text_faint(),
            );
        }
        let title_x = if floating {
            INSPECTOR_TITLE_TEXT_X_PX
        } else {
            INSPECTOR_TITLE_PAD_X_PX
        };
        painter.text(
            egui::pos2(bar_rect.left() + title_x, bar_rect.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            palette.body_font(INSPECTOR_TITLE_TEXT_PX),
            palette.text_primary(),
        );
    }
    // The controls are registered after the bar, so they win its pointer.
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = IconButton::new(icons::X, TOOLBAR_ICON)
                .hover_text("Close - keeps the drawing, clears the selection")
                .show(ui);
            if close.clicked() {
                actions.close = true;
            }
            let pin_hover = if chrome.shared.pinned {
                "Unpin - float the inspector over the chart"
            } else {
                "Pin - dock the inspector at the side of the chart"
            };
            let pin = IconButton::new(icons::PUSH_PIN, TOOLBAR_ICON)
                .active(chrome.shared.pinned)
                .hover_text(pin_hover)
                .show(ui);
            #[cfg(test)]
            {
                chrome.view.inspector.pin_rect = Some(pin.rect);
            }
            if pin.clicked() {
                actions.toggle_pin = true;
            }
            let eye_icon = if hidden { icons::EYE_SLASH } else { icons::EYE };
            let eye_hover = if hidden {
                "Show this drawing again"
            } else {
                "Hide this drawing - the inspector keeps the way back"
            };
            let eye = IconButton::new(eye_icon, TOOLBAR_ICON)
                .active(hidden)
                .hover_text(eye_hover)
                .show(ui);
            if eye.clicked() {
                actions.toggle_hidden = true;
            }
        });
    });
    if floating {
        let bar = bar.on_hover_text("Drag to move · double-click to reposition automatically");
        if bar.double_clicked() {
            let placed = target_placement(&chrome.view.inspector, ui.ctx(), env);
            chrome.emit(Intent::InspectorReset(
                placed.map(|placed| [placed.left, placed.top]),
            ));
            chrome.view.inspector.max_width = placed.and_then(|placed| placed.max_width);
        } else if bar.dragged() {
            let drawn_start = bar
                .drag_started()
                .then(|| {
                    ui.ctx()
                        .memory(|memory| memory.area_rect(egui::Id::new(INSPECTOR_AREA_ID)))
                        .map(|rect| [rect.min.x, rect.min.y])
                })
                .flatten();
            let delta = bar.drag_delta();
            let had_position = drawn_start.is_some() || chrome.state().inspector.pos.is_some();
            chrome.emit(Intent::InspectorDrag {
                start: drawn_start,
                delta: [delta.x, delta.y],
            });
            if had_position {
                chrome.view.inspector.max_width = None;
            }
        }
        // Written when the hand comes off the window, not while it is still
        // moving: one file write per gesture, and the position it records is
        // the one the trader stopped at.
        if bar.drag_stopped() {
            chrome.emit(Intent::InspectorPositionDirty);
        }
    }
    ui.separator();
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed tape's rail narrows what an object spans by its width, and the
    /// area it is placed in by the same: the choice of side does not move.
    #[test]
    fn a_closed_tapes_rail_moves_no_side_for_an_object_spanning_the_candles() {
        let whole = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_000.0, 600.0));
        let size = egui::vec2(220.0, 160.0);
        let at = |candles_right: Option<f32>| {
            let chart = crate::bands::drawing_area(whole, candles_right);
            let bbox = egui::Rect::from_min_max(
                egui::pos2(chart.left(), 280.0),
                egui::pos2(chart.right(), 320.0),
            );
            (placement(chart, bbox, size), bbox)
        };
        let rail = whole.right() - quantick_orderflow::LANE_FOLD_RAIL_PX;
        let ((closed, bbox), (open, _)) = (at(Some(rail)), at(None));
        assert_eq!(closed.top, open.top, "the same side either way");
        assert!(closed.top >= bbox.bottom(), "below the band: {closed:?}");
        assert_eq!(closed.max_width, open.max_width);
    }
}
