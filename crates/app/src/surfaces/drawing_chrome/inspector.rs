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
    BAR_DRAG_SPEED, DrawingChromeAsk, DrawingChromeSurface, DrawingEnv, INSPECTOR_AREA_ID,
    INSPECTOR_AUTO_PIN_CHART_WIDTH_PX, INSPECTOR_DEFAULT_WIDTH_PX, INSPECTOR_FALLBACK_HEIGHT_PX,
    INSPECTOR_LEVELS_WIDTH_PX, INSPECTOR_MAX_WIDTH_PX, INSPECTOR_MIN_WIDTH_PX,
    INSPECTOR_OBJECT_GAP_PX, INSPECTOR_TITLE_GRIP_GLYPH_PX, INSPECTOR_TITLE_HEIGHT_PX,
    INSPECTOR_TITLE_PAD_X_PX, INSPECTOR_TITLE_TEXT_PX, INSPECTOR_TITLE_TEXT_X_PX, InspectorActions,
    InspectorTab, PRICE_DRAG_STEPS, RecordingPresetHost, SavedDefault, clamp_into_chart,
};
use crate::drawings::{
    self, DRAWING_TOOLS, Drawing, DrawingAuthor, MAX_DRAWING_FILL_ALPHA, MAX_DRAWING_WIDTH_PX,
    MIN_DRAWING_WIDTH_PX, PresetHost as _,
};
use crate::theme;
use crate::widgets::{IconButton, TOOLBAR_ICON};
use egui_phosphor::regular as icons;

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

/// Where the inspector goes, and how wide it may be there.
///
/// The width is `Some` only when the panel had to be narrowed to fit a gutter
/// beside a drawing too big to place around — see [`placement`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InspectorPlacement {
    pub position: egui::Pos2,
    pub max_width: Option<f32>,
}

/// The inspector placement rule (`docs/ux/drawing-tools-2026-08.md` §D3).
///
/// The old rule scored least overlap with the object's *bounding box*, and a
/// small object has a small box: "beside it with a 12 px gap" scored zero and
/// won, dropping the panel straight onto the price action the trader drew the
/// line to read. The read is the neighbourhood of the object, not its box.
///
/// So the corners come first, and the winner is the **farthest** clear one:
///
/// 1. the two **top** corners first, inset by the gap. Top before bottom is
///    structural, not taste: a panel is positioned by its top-left and grows
///    downwards, so a top corner always has the whole pane to grow into. A
///    bottom-anchored panel that turns out taller than the placement assumed
///    runs off the window and loses its last rows — and rows a trader cannot
///    reach read as rows that do not exist;
/// 2. of the two, the one that clears `bbox` and whose centre is farthest
///    from the object wins, so the panel walks away from the drawing across
///    the chart. An exact tie (a centred object) takes the left one, so the
///    panel appears in the same place every time;
/// 3. only if neither top corner is free, the bottom two on the same rule;
/// 4. and if every corner is fouled — a large object covering the chart —
///    the beside-the-object candidates, least overlap first.
pub(crate) fn placement(
    chart: egui::Rect,
    bbox: egui::Rect,
    size: egui::Vec2,
) -> InspectorPlacement {
    let gap = INSPECTOR_OBJECT_GAP_PX;
    let top_corners = [
        egui::pos2(chart.left() + gap, chart.top() + gap),
        egui::pos2(chart.right() - gap - size.x, chart.top() + gap),
    ];
    let bottom_corners = [
        egui::pos2(chart.left() + gap, chart.bottom() - gap - size.y),
        egui::pos2(chart.right() - gap - size.x, chart.bottom() - gap - size.y),
    ];
    let farthest_clear = |candidates: [egui::Pos2; 2]| {
        let mut best: Option<(egui::Pos2, f32)> = None;
        for candidate in candidates {
            let position = clamp_into_chart(candidate, size, chart);
            let rect = egui::Rect::from_min_size(position, size);
            if rect.intersect(bbox).is_positive() {
                continue;
            }
            let distance = rect.center().distance(bbox.center());
            if best.is_none_or(|(_, best)| distance > best) {
                best = Some((position, distance));
            }
        }
        best.map(|(position, _)| position)
    };
    if let Some(position) = farthest_clear(top_corners).or_else(|| farthest_clear(bottom_corners)) {
        return InspectorPlacement {
            position,
            max_width: None,
        };
    }
    // No corner is clear, which is what a big object — a volume profile, a
    // range across the whole chart — does to all four of them. The panel then
    // goes into whichever *gutter* the object leaves beside it, narrowed to
    // fit, because a settings panel sitting on the drawing it configures is
    // the one outcome this whole function exists to avoid.
    //
    // Horizontal gutters only. Narrowing a panel reflows it and its scroll
    // area keeps every row reachable; shortening one hides rows, and rows a
    // trader cannot reach read as rows that do not exist.
    let left_gutter = bbox.left() - gap - chart.left();
    let right_gutter = chart.right() - bbox.right() - gap;
    let (gutter, gutter_left) = if right_gutter >= left_gutter {
        (right_gutter, bbox.right() + gap)
    } else {
        (left_gutter, chart.left() + gap)
    };
    if gutter >= INSPECTOR_MIN_WIDTH_PX {
        let width = gutter.min(size.x);
        let position = clamp_into_chart(
            egui::pos2(gutter_left, chart.top() + gap),
            egui::vec2(width, size.y),
            chart,
        );
        return InspectorPlacement {
            position,
            max_width: Some(width),
        };
    }

    // The object leaves no strip wide enough to be a panel in. Nothing can be
    // placed clear of it, and saying so by crowding it least is more honest
    // than pretending: this is the one case the rule cannot keep.
    let corners = [top_corners, bottom_corners].concat();
    let fallbacks = [
        egui::pos2(bbox.right() + gap, bbox.top()),
        egui::pos2(bbox.left() - gap - size.x, bbox.top()),
        egui::pos2(bbox.left(), bbox.bottom() + gap),
        egui::pos2(bbox.left(), bbox.top() - gap - size.y),
    ];
    let mut best: Option<(egui::Pos2, f32, f32)> = None;
    for candidate in fallbacks.into_iter().chain(corners.iter().copied()) {
        let position = clamp_into_chart(candidate, size, chart);
        let rect = egui::Rect::from_min_size(position, size);
        let overlap = rect.intersect(bbox);
        let overlap_area = if overlap.is_positive() {
            overlap.area()
        } else {
            0.0
        };
        let distance = rect.center().distance(bbox.center());
        let wins = match &best {
            None => true,
            Some((_, best_area, best_distance)) => {
                overlap_area < *best_area
                    || (overlap_area == *best_area && distance > *best_distance)
            }
        };
        if wins {
            best = Some((position, overlap_area, distance));
        }
    }
    InspectorPlacement {
        position: best.map_or_else(|| chart.left_top(), |(position, _, _)| position),
        max_width: None,
    }
}

/// Where a freshly opened floating inspector should sit, and how wide it may
/// be there: the farthest chart corner that clears the object, then a gutter
/// beside it narrowed to fit. The chart pane already excludes both axes and
/// the live lane, so the popup can never cover them or leave the view.
fn target_placement(
    inspector: &Inspector,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> Option<InspectorPlacement> {
    let chart = env.chart_area?;
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
    egui::SidePanel::right("drawing_inspector_panel")
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
    if selection_changed
        && !chrome.inspector.moved
        && let Some(placed) = target_placement(&chrome.inspector, ctx, env)
    {
        chrome.inspector.pos = Some(placed.position);
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
    chrome.inspector.size = response
        .as_ref()
        .map(|response| response.response.rect.size());
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
    if actions.edited {
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
                egui::FontId::proportional(INSPECTOR_TITLE_GRIP_GLYPH_PX),
                theme::TEXT_FAINT,
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
            egui::FontId::proportional(INSPECTOR_TITLE_TEXT_PX),
            theme::TEXT_PRIMARY,
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
            chrome.inspector.pos = placed.map(|placed| placed.position);
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

/// Everything the inspector shows for the selected object, shared by the
/// floating window and the pinned dock panel. Sections are driven by the
/// tool's capabilities — an unsupported property is absent, not disabled.
///
/// `edited` is the host's object **copied**, not borrowed: every widget below
/// writes into the copy, and the caller hands it back through the response.
/// The original stays where every renderer reads it.
fn body(
    chrome: &mut DrawingChromeSurface,
    ui: &mut egui::Ui,
    edited: &mut Drawing,
    env: &DrawingEnv<'_>,
    presets: &mut RecordingPresetHost<'_>,
) -> InspectorActions {
    let mut actions = InspectorActions::default();
    let tool = edited.tool;
    let locked = edited.locked;
    let hidden = edited.hidden;
    let author = edited.author.as_ref().map(DrawingAuthor::label);
    let shareable = edited.shareable();
    let mut shared = edited.scope == drawings::DrawingScope::AllCharts;
    let show_confirm = chrome.shared.delete_confirm && locked;

    // The always-visible textual actions (UX spec: never glyph-only, never
    // behind a scroll). Identity and the view controls live in the host's
    // title bar, not here.
    let intent = drawings::action_bar::draw(ui, locked);
    actions.toggle_lock |= intent.toggle_lock;
    actions.delete |= intent.delete;

    if let Some(author) = &author {
        // Data honesty, where the trader decides what to do with the object:
        // an assistant's mark never passes for their own.
        ui.label(
            egui::RichText::new(format!("Placed by {author} - not by you."))
                .small()
                .color(theme::TEXT_SUPPORT),
        );
    }
    if locked {
        ui.label(
            egui::RichText::new("Locked - protected from accidental moves. Style stays editable.")
                .small()
                .color(theme::TEXT_SUPPORT),
        );
    }
    if hidden {
        ui.label(
            egui::RichText::new("Hidden - Show brings it back.")
                .small()
                .color(theme::TEXT_SUPPORT),
        );
    }
    // Where the object appears. Always visible, never behind a tab.
    //
    // It used to live on the Coordinates tab, because sharing is a statement
    // about the anchors — which is the implementer's mental model, not the
    // trader's. Nobody hunting for "also show this on the other chart" opens
    // a tab called Coordinates, and that is not even the tab the panel opens
    // on. Reported as unfindable, and it was.
    ui.separator();
    let sharing = ui.add_enabled(
        shareable,
        egui::Checkbox::new(&mut shared, "Show on all charts"),
    );
    if sharing.changed() {
        edited.scope = if shared {
            drawings::DrawingScope::AllCharts
        } else {
            drawings::DrawingScope::ThisChart
        };
        actions.edited = true;
    }
    // A disabled control with no reason reads as a bug.
    let sharing_hint = if shareable {
        "The other chart of this tab draws it at the same moment in market time"
    } else {
        "This drawing has an anchor past the newest bar, so there is no market time to place          it by on another chart"
    };
    sharing.on_hover_text(sharing_hint);
    if !shareable {
        ui.label(
            egui::RichText::new(sharing_hint)
                .small()
                .color(theme::TEXT_SUPPORT),
        );
    }

    if show_confirm {
        ui.separator();
        ui.label("Delete locked drawing?");
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                actions.cancel_delete = true;
            }
            if ui.button("Delete anyway").clicked() {
                actions.force_delete = true;
            }
        });
    }
    ui.separator();

    if chrome.inspector.tab == InspectorTab::Extra && tool.extra_tab().is_none() {
        // The previous selection had an extra tab; this tool brings none.
        chrome.inspector.tab = InspectorTab::Style;
    }
    ui.horizontal(|ui| {
        ui.selectable_value(&mut chrome.inspector.tab, InspectorTab::Style, "Style");
        // A tool that brings its own tab (the Fib level editor) mounts it
        // here by name; the central code never learns what is inside.
        if let Some(extra) = tool.extra_tab() {
            ui.selectable_value(&mut chrome.inspector.tab, InspectorTab::Extra, extra);
        }
        ui.selectable_value(
            &mut chrome.inspector.tab,
            InspectorTab::Coordinates,
            "Coordinates",
        );
    });
    ui.separator();

    let price_speed = env.auto_range.map_or(1.0, |(lo, hi)| {
        ((hi - lo) / PRICE_DRAG_STEPS).abs().max(1e-9)
    });
    match chrome.inspector.tab {
        InspectorTab::Extra => {
            actions.edited |= tool.draw_extra_tab(ui, edited, presets);
        }
        InspectorTab::Style => {
            ui.label("Style");
            actions.edited |= ui
                .color_edit_button_srgba(&mut edited.style.color)
                .changed();
            // Capability-driven, like the fill slider below: a tool with no
            // stroke has no line width, and the repo's rule is that an
            // unsupported property is *absent*, not present and inert. Caught
            // by the visual pass — the text note's Style tab was offering a
            // slider that moved nothing.
            if tool.supports_stroke_width() {
                actions.edited |= ui
                    .add(
                        egui::Slider::new(
                            &mut edited.style.width_px,
                            MIN_DRAWING_WIDTH_PX..=MAX_DRAWING_WIDTH_PX,
                        )
                        .text("line width (px)"),
                    )
                    .changed();
            }
            if tool.supports_fill() {
                actions.edited |= ui
                    .add(
                        egui::Slider::new(&mut edited.style.fill_alpha, 0..=MAX_DRAWING_FILL_ALPHA)
                            .text("fill opacity"),
                    )
                    .changed();
            }

            // Stop asking for the same look every single time. Every tool has
            // a Style tab, so every tool gets this — the named-preset editor
            // only ever existed on the Fib tab, which left fifteen tools with
            // no way to remember anything.
            //
            // New objects only: a default that repainted the marks already on
            // the chart would be a bulk edit nobody asked for.
            ui.separator();
            ui.label(egui::RichText::new("Default for new drawings").small());
            ui.horizontal(|ui| {
                let style = edited.style;
                if ui
                    .button("Save as default")
                    .on_hover_text(format!(
                        "New {} objects open configured exactly like this one",
                        tool.name().to_lowercase()
                    ))
                    .clicked()
                {
                    drawings::save_tool_default(presets, edited);
                    actions.saved_default = Some(SavedDefault::OneTool);
                }
                // Style only, and that is not a shortcut: a Fib's level list
                // means nothing to a rectangle, so the one property every
                // tool shares is the only one this can carry.
                if ui
                    .button("Colour on all tools")
                    .on_hover_text("Every new drawing opens with this colour, width and fill")
                    .clicked()
                {
                    for other in DRAWING_TOOLS {
                        presets.set_default_style(other.id(), Some(style));
                    }
                    actions.saved_default = Some(SavedDefault::EveryTool);
                }
                if drawings::has_saved_default(presets, tool)
                    && ui
                        .button("Reset to factory")
                        .on_hover_text(format!(
                            concat!(
                                "New {} objects go back to how they opened ",
                                "out of the box. Clears the default preset ",
                                "choice too; saved presets are kept"
                            ),
                            tool.name().to_lowercase()
                        ))
                        .clicked()
                {
                    drawings::reset_tool_default(presets, tool);
                    actions.saved_default = Some(SavedDefault::Forgotten);
                }
            });
        }
        InspectorTab::Coordinates => {
            // Geometry through numbers: bar index and price per anchor, the
            // same canonical coordinates drags write. Locked blocks geometry
            // here exactly as it does on the canvas.
            const ANCHOR_LABELS: [&str; 4] = ["A", "B", "C", "D"];
            ui.add_enabled_ui(!locked, |ui| {
                for (point_index, point) in edited.points.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(ANCHOR_LABELS.get(point_index).copied().unwrap_or("?"));
                        actions.edited |= ui
                            .add(
                                egui::DragValue::new(&mut point.bar)
                                    .speed(BAR_DRAG_SPEED)
                                    .prefix("bar "),
                            )
                            .changed();
                        actions.edited |= ui
                            .add(egui::DragValue::new(&mut point.price).speed(price_speed))
                            .changed();
                    });
                }
            });
            if locked {
                ui.label(
                    egui::RichText::new("Unlock the drawing to edit its coordinates.").small(),
                );
            }
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The eight §4.2 candidates, clamped — restated here so a drifted
    /// implementation cannot silently shrink its own search space.
    fn placement_candidates(
        chart: egui::Rect,
        bbox: egui::Rect,
        size: egui::Vec2,
    ) -> [egui::Pos2; 8] {
        let gap = INSPECTOR_OBJECT_GAP_PX;
        [
            egui::pos2(bbox.right() + gap, bbox.top()),
            egui::pos2(bbox.left() - gap - size.x, bbox.top()),
            egui::pos2(bbox.left(), bbox.bottom() + gap),
            egui::pos2(bbox.left(), bbox.top() - gap - size.y),
            egui::pos2(chart.left() + gap, chart.top() + gap),
            egui::pos2(chart.right() - gap - size.x, chart.top() + gap),
            egui::pos2(chart.left() + gap, chart.bottom() - gap - size.y),
            egui::pos2(chart.right() - gap - size.x, chart.bottom() - gap - size.y),
        ]
        .map(|candidate| clamp_into_chart(candidate, size, chart))
    }

    #[test]
    fn placement_sends_the_panel_to_a_corner_not_beside_a_small_object() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_400.0, 800.0));
        let bbox = egui::Rect::from_center_size(chart.center(), egui::vec2(40.0, 40.0));
        let size = egui::vec2(320.0, 280.0);
        let position = placement(chart, bbox, size).position;
        let rect = egui::Rect::from_min_size(position, size);
        assert!(!rect.intersects(bbox), "a clear candidate exists and wins");
        assert!(chart.contains_rect(rect));
        assert_ne!(
            position,
            egui::pos2(bbox.right() + INSPECTOR_OBJECT_GAP_PX, bbox.top()),
            "beside-the-object is exactly the placement this rule replaced"
        );
        assert!(
            placement_candidates(chart, bbox, size)[4..].contains(&position),
            "the winner is one of the four chart corners"
        );
        assert_eq!(
            position,
            placement(chart, bbox, size).position,
            "identical inputs give identical placements"
        );
    }

    /// The farthest clear corner wins, so the panel walks away from the
    /// object instead of hugging the nearest empty spot.
    #[test]
    fn placement_picks_the_corner_farthest_from_the_object() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_400.0, 800.0));
        // Object parked in the bottom-left quadrant.
        let bbox = egui::Rect::from_center_size(egui::pos2(260.0, 640.0), egui::vec2(40.0, 40.0));
        let size = egui::vec2(320.0, 280.0);
        let gap = INSPECTOR_OBJECT_GAP_PX;
        assert_eq!(
            placement(chart, bbox, size).position,
            egui::pos2(chart.right() - gap - size.x, chart.top() + gap),
            "the opposite corner is the farthest clear one"
        );
    }

    /// With the object centred, all four corners are equidistant, so the
    /// order tie-break decides — and it must always decide the same way, or
    /// the panel appears somewhere new every time (Duda, §D3).
    #[test]
    fn placement_prefers_the_top_left_corner_on_a_tie() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_400.0, 800.0));
        let bbox = egui::Rect::from_center_size(chart.center(), egui::vec2(40.0, 40.0));
        let size = egui::vec2(320.0, 280.0);
        let gap = INSPECTOR_OBJECT_GAP_PX;
        assert_eq!(
            placement(chart, bbox, size).position,
            egui::pos2(chart.left() + gap, chart.top() + gap)
        );
    }

    /// A panel taller than the placement assumed must not be sent to a bottom
    /// corner, where it would grow off the window and lose its last rows. The
    /// top-first order is what guarantees that, so it is worth a test of its
    /// own: whatever height the panel turns out to have, the chosen spot
    /// leaves room for it below.
    #[test]
    fn placement_leaves_a_tall_panel_room_to_grow_downwards() {
        let chart = egui::Rect::from_min_size(egui::pos2(60.0, 88.0), egui::vec2(1_224.0, 744.0));
        let bbox = egui::Rect::from_center_size(egui::pos2(700.0, 300.0), egui::vec2(40.0, 40.0));
        // The height the placement believes in, and the height the panel
        // turns out to want once its level editor is open.
        let assumed = egui::vec2(360.0, 280.0);
        let actual = 620.0;
        let position = placement(chart, bbox, assumed).position;
        assert!(
            position.y + actual <= chart.bottom(),
            "a panel that grows to {actual} px must still fit below {position:?}"
        );
    }

    #[test]
    fn placement_picks_the_least_overlap_when_nothing_clears() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_000.0, 600.0));
        // The object spans ~90% of the pane: every candidate overlaps.
        let bbox = chart.shrink2(egui::vec2(50.0, 30.0));
        let size = egui::vec2(320.0, 280.0);
        let position = placement(chart, bbox, size).position;
        let chosen = egui::Rect::from_min_size(position, size)
            .intersect(bbox)
            .area();
        for candidate in placement_candidates(chart, bbox, size) {
            let overlap = egui::Rect::from_min_size(candidate, size)
                .intersect(bbox)
                .area();
            assert!(
                chosen <= overlap + 0.01,
                "the chosen spot must cover the object least: {chosen} vs {overlap}"
            );
        }
        assert!(chart.contains_rect(egui::Rect::from_min_size(position, size)));
    }

    #[test]
    fn placement_never_returns_the_blind_first_candidate_at_the_right_edge() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_000.0, 600.0));
        let bbox = egui::Rect::from_min_max(egui::pos2(900.0, 200.0), egui::pos2(1_000.0, 300.0));
        let size = egui::vec2(320.0, 280.0);
        let position = placement(chart, bbox, size).position;
        let rect = egui::Rect::from_min_size(position, size);
        assert!(
            !rect.intersects(bbox),
            "left of the object clears; the old code clamped right-of back onto it"
        );
        assert!(
            position.x < bbox.left(),
            "the panel must sit clear on the left, not clamp over the object"
        );
    }

    /// The session's complaint (`docs/ux/drawing-tools-2026-08.md` §F3): the
    /// old rule put the panel 12 px beside a small object, right on top of
    /// the price action the trader drew it to read. A corner must win.
    /// The rule a volume profile broke: a drawing big enough to foul all four
    /// corners used to get the panel dropped straight onto it.
    ///
    /// A profile is exactly that shape — tall enough to span the price axis,
    /// wide enough to reach past every corner candidate — so "least overlap"
    /// meant "on the figure", and the panel covered the thing it configures.
    /// It goes into the gutter beside the object instead, narrowed to fit.
    #[test]
    fn a_drawing_too_big_for_any_corner_sends_the_panel_to_a_gutter_beside_it() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1200.0, 700.0));
        let size = egui::vec2(320.0, 280.0);
        // A profile down the middle, wide enough that a panel parked at any
        // corner overlaps it, and narrow enough that the strip beside it can
        // still hold a narrowed one. That band is exactly what narrowing is
        // for: the corner candidate assumes the full width and fouls, the
        // gutter takes what fits.
        let bbox = egui::Rect::from_min_max(egui::pos2(320.0, 0.0), egui::pos2(880.0, 700.0));
        let gap = INSPECTOR_OBJECT_GAP_PX;
        for corner in [
            egui::pos2(chart.left() + gap, chart.top() + gap),
            egui::pos2(chart.right() - gap - size.x, chart.top() + gap),
            egui::pos2(chart.left() + gap, chart.bottom() - gap - size.y),
            egui::pos2(chart.right() - gap - size.x, chart.bottom() - gap - size.y),
        ] {
            assert!(
                egui::Rect::from_min_size(corner, size)
                    .intersect(bbox)
                    .is_positive(),
                "the fixture has to actually foul every corner: {corner:?}"
            );
        }

        let placed = placement(chart, bbox, size);
        let rect = egui::Rect::from_min_size(
            placed.position,
            egui::vec2(placed.max_width.unwrap_or(size.x), size.y),
        );
        assert!(
            !rect.intersect(bbox).is_positive(),
            "the panel must not cover the drawing it configures: {rect:?} vs {bbox:?}"
        );
        assert!(
            chart.contains_rect(rect),
            "and it stays inside the chart: {rect:?}"
        );
        assert!(
            placed
                .max_width
                .is_some_and(|w| w >= INSPECTOR_MIN_WIDTH_PX),
            "narrowed, but never below what a panel needs: {:?}",
            placed.max_width
        );
    }

    /// The wider side wins, so the panel goes where there is most room — and
    /// a corner that *is* free still beats any gutter, because the gutter is
    /// the fallback, not the rule.
    #[test]
    fn the_gutter_is_the_wider_side_and_never_preferred_over_a_free_corner() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1200.0, 700.0));
        let size = egui::vec2(320.0, 280.0);

        // Object hugging the left: the room is on its right.
        let left_heavy = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 700.0));
        let placed = placement(chart, left_heavy, size);
        assert!(
            placed.position.x >= left_heavy.right(),
            "the panel takes the wider side: {placed:?}"
        );

        // Object hugging the right: the room is on its left.
        let right_heavy =
            egui::Rect::from_min_max(egui::pos2(800.0, 0.0), egui::pos2(1200.0, 700.0));
        let placed = placement(chart, right_heavy, size);
        assert!(
            placed.position.x + placed.max_width.unwrap_or(size.x) <= right_heavy.left(),
            "and the other side when that is where the room is: {placed:?}"
        );

        // A small object leaves corners free, and a free corner is unnarrowed.
        let small = egui::Rect::from_center_size(chart.center(), egui::vec2(40.0, 40.0));
        let placed = placement(chart, small, size);
        assert_eq!(
            placed.max_width, None,
            "a corner placement never narrows the panel: {placed:?}"
        );
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
