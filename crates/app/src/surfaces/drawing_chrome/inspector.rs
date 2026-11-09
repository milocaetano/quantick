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
    DrawingChromeAsk, DrawingChromeSurface, DrawingEnv, INSPECTOR_AREA_ID,
    INSPECTOR_AUTO_PIN_CHART_WIDTH_PX, INSPECTOR_DEFAULT_WIDTH_PX, INSPECTOR_FALLBACK_HEIGHT_PX,
    INSPECTOR_LEVELS_WIDTH_PX, INSPECTOR_MAX_WIDTH_PX, INSPECTOR_MIN_WIDTH_PX,
    INSPECTOR_OBJECT_GAP_PX, INSPECTOR_TITLE_GRIP_GLYPH_PX, INSPECTOR_TITLE_HEIGHT_PX,
    INSPECTOR_TITLE_PAD_X_PX, INSPECTOR_TITLE_TEXT_PX, INSPECTOR_TITLE_TEXT_X_PX, InspectorActions,
    InspectorTab, RecordingPresetHost, clamp_into_chart,
};
use crate::drawings::Drawing;
use crate::theme;
use crate::widgets::{IconButton, TOOLBAR_ICON};
use egui_phosphor::regular as icons;
use quantick_chart_interaction::panel_placement::{
    Area, NEIGHBOURHOOD_PX, PanelPlacement, PanelSpacing, place_beside,
};

mod sections;

use sections::body;

/// Initial position of the selected-drawing inspector, before [`placement`]
/// has a size and a bbox to place it from.
const DEFAULT_POSITION: egui::Pos2 = super::DRAWING_INSPECTOR_DEFAULT_POSITION;

/// The inspector's own state. What it shares with the context bar lives in
/// [`super::Shared`]; this is what only it reads.
#[derive(Default)]
pub(crate) struct Inspector {
    /// Which tab is open, remembered across selections so a trader working
    /// through coordinates does not land back on Style every time.
    pub tab: InspectorTab,
    /// Whether the user moved the floating window this session. A manual
    /// position wins over automatic placement from then on.
    pub(super) moved: bool,
    /// The parked point, in screen coordinates. What the file remembers.
    pub(super) pos: Option<egui::Pos2>,
    /// The parked point changed and the workspace file has not caught up.
    pub(super) position_dirty: bool,
    /// How wide the window may be where it was placed — `Some` only when it
    /// had to be narrowed into a gutter beside an object too big to walk
    /// around.
    pub(super) max_width: Option<f32>,
    /// The size it was last drawn at, which beats every guess about it.
    pub(super) size: Option<egui::Vec2>,
    /// The panel as drawn last frame reached into its drawing's
    /// neighbourhood — drawn wider than the placement assumed, or grown by a
    /// taller tab — so automatic placement runs again with the drawn size.
    pub(super) crowding: bool,
    /// Whether the pin button has been pressed either way. The auto-pin width
    /// rule stops firing once the user has expressed a preference.
    pub(super) pin_touched: bool,
    /// The unpin happened this frame: skip one before placing, so the
    /// placement measures settled geometry rather than the pinned era's.
    pub(super) settle_frame: bool,
    #[cfg(test)]
    pub pin_rect: Option<egui::Rect>,
}

impl Inspector {
    /// Park the window where a hand put it. The flag and the point move
    /// together: a position without the flag would be overwritten by the next
    /// automatic placement, and the narrowing a placement computed does not
    /// survive a move the trader made.
    ///
    /// Deliberately *not* a file write. The drag calls this every frame the
    /// hand moves; the write is owed when the hand comes off, which is where
    /// [`Self::position_dirty`] is raised.
    pub fn place_by_hand(&mut self, position: egui::Pos2) {
        self.pos = Some(position);
        self.moved = true;
        self.max_width = None;
    }

    /// The popup position a workspace should record: the one a hand placed,
    /// never one the app computed.
    ///
    /// Automatic placement is a *rule* — beside the object, on the side with
    /// room. Writing down the pixel that rule produced for yesterday's drawing
    /// would freeze a stale answer into the file and stop the rule from ever
    /// running again, which is the same bug as never forgetting a drag.
    pub fn remembered_position(&self) -> Option<[f32; 2]> {
        self.pos
            .filter(|_| self.moved)
            .map(|position| [position.x, position.y])
    }

    /// Adopt what a workspace remembers about the popup, including its
    /// silence: no recorded position hands the window back to automatic
    /// placement rather than leaving the previous cockpit's position behind.
    ///
    /// A non-finite pair is silence too. The file is hand-editable and TOML
    /// spells `nan`, and NaN is the one value the repair cannot walk back:
    /// every comparison against it is false, so `f32::clamp` returns it
    /// unchanged and the popup goes somewhere with no pixels — where its own
    /// title bar, and with it the double-click that would undo this, cannot
    /// be reached. It would then be written straight back at the next save.
    /// The env hook already refuses the same input
    /// ([`super::parse_point`]); this is the door the file comes through.
    pub fn restore_position(&mut self, remembered: Option<[f32; 2]>) {
        match remembered.filter(|[x, y]| x.is_finite() && y.is_finite()) {
            Some([x, y]) => self.place_by_hand(egui::pos2(x, y)),
            None => {
                self.pos = None;
                self.moved = false;
                self.max_width = None;
            }
        }
    }

    /// Whether a file write is owed, taken so it is owed once.
    pub fn take_position_dirty(&mut self) -> bool {
        std::mem::take(&mut self.position_dirty)
    }

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
/// owned by [`place_beside`]: beside the object, clear of its neighbourhood.
/// A dragged panel never comes here: a hand's position wins from then on
/// (see [`Inspector::place_by_hand`]).
pub(crate) fn placement(chart: egui::Rect, bbox: egui::Rect, size: egui::Vec2) -> PanelPlacement {
    let area = |rect: egui::Rect| Area::new(rect.left(), rect.top(), rect.right(), rect.bottom());
    let spacing = PanelSpacing {
        neighbourhood: NEIGHBOURHOOD_PX,
        gap: INSPECTOR_OBJECT_GAP_PX,
        min_width: INSPECTOR_MIN_WIDTH_PX,
    };
    place_beside(area(chart), area(bbox), size.x, size.y, spacing)
}

/// Where a freshly opened floating inspector should sit, and how wide it may
/// be there (see [`placement`]). Measured in the candles' area, which an object's span
/// is computed from, so the popup leaves the axes, lane and rail clear.
fn target_placement(
    inspector: &Inspector,
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
    chrome.shared.last_selection = Some(index);
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
    // Selecting an object no longer opens this window — the context bar does.
    // The gear on that bar is the one door, so the panel only covers the
    // chart when the trader asked for the panel.
    if !chrome.shared.open {
        return ask;
    }
    if std::mem::take(&mut chrome.inspector.settle_frame) {
        // The unpin happened this frame: the side panel still occupies this
        // frame's layout and the drawing projects against the pinned-era
        // chart. Wait one frame and place against the settled geometry.
        return ask;
    }
    let selection_changed = chrome.shared.last_selection != Some(index);
    // The auto-pin (§4.2): a fresh selection on a chart too narrow for a
    // floating window opens pinned instead — decided here because this host
    // is the one that would otherwise claim the selection. Stops firing once
    // the user touches the pin.
    //
    // A parked window stops it too, and for the same reason: the rule reads
    // "no floating position here would leave the geometry alone", and a
    // trader who chose where the floating window goes has answered that.
    // Without this clause the everyday layout never reaches the remembered
    // position at all — a split canvas puts the pane a drawing lives on under
    // the threshold, so every selection would re-dock the panel. The pin flag
    // itself is left alone: it records that the *pin button* was pressed, and
    // borrowing it here would leak a placement into the next workspace
    // opened, which carries no pin preference.
    if selection_changed
        && !chrome.inspector.pin_touched
        && !chrome.inspector.moved
        && env
            .chart_area
            .is_some_and(|chart| chart.width() < INSPECTOR_AUTO_PIN_CHART_WIDTH_PX)
    {
        chrome.shared.pinned = true;
        // The pinned panel draws from the next frame on.
        return ask;
    }
    chrome.shared.last_selection = Some(index);
    // Automatic placement only while the window is untouched.
    if (selection_changed || std::mem::take(&mut chrome.inspector.crowding))
        && !chrome.inspector.moved
        && let Some(placed) = target_placement(&chrome.inspector, ctx, env)
    {
        chrome.inspector.pos = Some(egui::pos2(placed.left, placed.top));
        chrome.inspector.max_width = placed.max_width;
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
    let inspector_size = chrome.inspector.size(ctx);
    let draw_at = chrome.inspector.pos.map(|position| {
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
    chrome.inspector.size = drawn.map(|rect| rect.size());
    // Placed again next frame if it reaches into the drawing's neighbourhood.
    // egui's first frame of a window is an invisible sizing pass, so a panel
    // drawn wider than assumed is corrected before it is seen. Not while a
    // hand holds the pointer (a resize), nor for a gutter-narrowed panel.
    chrome.inspector.crowding = !chrome.inspector.moved
        && chrome.inspector.max_width.is_none()
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
    let Some(selected) = env.selected.as_ref() else {
        chrome.shared.delete_confirm = false;
        ask.commit_edit_gesture = chrome.shared.edit_baseline.take().map(Box::new);
        chrome.shared.last_selection = None;
        return None;
    };
    if chrome.baseline_is_stale(Some(selected.index)) {
        ask.commit_edit_gesture = chrome.shared.edit_baseline.take().map(Box::new);
    }
    Some(selected)
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
                chrome.inspector.pin_rect = Some(pin.rect);
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
            // The reset path: back to automatic placement.
            chrome.inspector.moved = false;
            let placed = target_placement(&chrome.inspector, ui.ctx(), env);
            chrome.inspector.pos = placed.map(|placed| egui::pos2(placed.left, placed.top));
            chrome.inspector.max_width = placed.and_then(|placed| placed.max_width);
            // Giving the placement back is a decision too, and it has to
            // reach the file: a reset that lived only until the next launch
            // would hand the trader back the very position they discarded.
            chrome.inspector.position_dirty = true;
        } else if bar.dragged() {
            // The gesture starts from where the window actually is, in
            // preference to where it is remembered: the two differ whenever
            // the pane is too small for the parked point and the host is
            // drawing a clamped copy. Dragging the clamped window from the
            // un-clamped point would make it jump the whole difference on the
            // first pixel — and the trader is dragging what they can see.
            if bar.drag_started()
                && let Some(drawn) = ui
                    .ctx()
                    .memory(|memory| memory.area_rect(egui::Id::new(INSPECTOR_AREA_ID)))
                    .map(|rect| rect.min)
            {
                chrome.inspector.pos = Some(drawn);
            }
            // From there the delta accumulates on the *remembered* point,
            // never on the drawn one. The window is positioned from `pos`
            // before this bar is laid out, so the drawn rect is always one
            // frame behind it: adding this frame's delta to that rect throws
            // the previous frame's away, and the popup travels at half the
            // speed of the hand — which is what "ela treme" was, reported
            // from the running app.
            match chrome.inspector.pos {
                Some(position) => chrome.inspector.place_by_hand(position + bar.drag_delta()),
                // No position to move yet — the window has not been laid out.
                // The hand is still down, so the flag alone records that
                // placement is no longer the app's to decide.
                None => chrome.inspector.moved = true,
            }
        }
        // Written when the hand comes off the window, not while it is still
        // moving: one file write per gesture, and the position it records is
        // the one the trader stopped at.
        if bar.drag_stopped() {
            chrome.inspector.position_dirty = true;
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

    /// Automatic placement is not a preference, so it is not written down.
    /// Only a hand — or the hook standing in for one — makes a position the
    /// file has to remember.
    #[test]
    fn only_a_hand_placed_position_reaches_the_file() {
        let mut inspector = Inspector::default();
        assert_eq!(inspector.remembered_position(), None);
        inspector.pos = Some(egui::pos2(10.0, 20.0));
        assert_eq!(
            inspector.remembered_position(),
            None,
            "a placed position is the app's decision, not the trader's"
        );
        inspector.place_by_hand(egui::pos2(30.0, 40.0));
        assert_eq!(inspector.remembered_position(), Some([30.0, 40.0]));
        assert!(
            !inspector.take_position_dirty(),
            "parking alone owes nothing: the drag calls it on every frame the              hand moves, and the file write is owed when the hand comes off"
        );
        inspector.position_dirty = true;
        assert!(inspector.take_position_dirty(), "and then exactly once");
        assert!(!inspector.take_position_dirty());
    }

    /// A file with a nonsense position leaves automatic placement in charge
    /// rather than parking the panel at an invented pixel.
    #[test]
    fn a_nonsense_remembered_position_is_refused() {
        let mut inspector = Inspector::default();
        inspector.place_by_hand(egui::pos2(30.0, 40.0));
        inspector.restore_position(Some([f32::NAN, 0.0]));
        assert_eq!(inspector.remembered_position(), None);
        inspector.restore_position(Some([12.0, 34.0]));
        assert_eq!(inspector.remembered_position(), Some([12.0, 34.0]));
    }
}
