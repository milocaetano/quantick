//! The temporary range raised by a secondary-button drag.
//!
//! The pane resolves pointer pixels into chart coordinates and paints the
//! ruler through the drawing-tool registry. This module owns everything that
//! outlives that input pass: the pending press, the selected range, the
//! contextual action bar and the request the host sends through the control
//! action registry.

use eframe::egui;

use crate::drawings::{self, ChartPoint, DrawingPayload, NewDrawing};
use crate::pane::PaneSide;
use crate::widgets::{IconButton, TOOLRAIL_ICON};

use super::{DrawingChromeAsk, DrawingEnv};

pub(crate) const BAR_ID: &str = "quick_range_context_bar";
pub(crate) const ACTION_CONTROL_ID: &str = "quick_range.fixed_range_profile";
pub(crate) const RETRACEMENT_CONTROL_ID: &str = "quick_range.fib_retracement";
pub(crate) const PROJECTION_CONTROL_ID: &str = "quick_range.fib_projection";

/// One durable drawing offered for the settled temporary range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Profile,
    Retracement,
    Projection,
}

impl Action {
    pub const ALL: [Self; 3] = [Self::Profile, Self::Retracement, Self::Projection];

    const fn index(self) -> usize {
        match self {
            Self::Profile => 0,
            Self::Retracement => 1,
            Self::Projection => 2,
        }
    }

    pub const fn control_id(self) -> &'static str {
        match self {
            Self::Profile => ACTION_CONTROL_ID,
            Self::Retracement => RETRACEMENT_CONTROL_ID,
            Self::Projection => PROJECTION_CONTROL_ID,
        }
    }

    pub const fn tool_id(self) -> &'static str {
        match self {
            Self::Profile => crate::frvp::TOOL_ID,
            Self::Retracement => "fib-retracement",
            Self::Projection => "fib-extension",
        }
    }

    pub const fn capability_id(self) -> &'static str {
        match self {
            Self::Profile => crate::control::PROFILE_CAPABILITY_ID,
            Self::Retracement => crate::control::FIB_RETRACEMENT_CAPABILITY_ID,
            Self::Projection => crate::control::FIB_PROJECTION_CAPABILITY_ID,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Profile => "Fixed-range volume profile",
            Self::Retracement => "Fib retracement",
            Self::Projection => "Fib projection",
        }
    }
}

/// The chart that owns a temporary range. One window has one right mouse
/// button, so the surface holds one owner rather than one range per pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub tab: u64,
    pub side: PaneSide,
}

/// The request emitted by the action bar. It carries settled coordinates by
/// value because the surface disappears as soon as the host accepts it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlaceRequest {
    pub action: Action,
    pub owner: Owner,
    pub anchors: [ChartPoint; 2],
}

/// What the pane needs to paint the ruler through the registered drawing
/// implementation.
pub(crate) struct Paint<'a> {
    pub anchors: [ChartPoint; 2],
    pub style: drawings::DrawingStyle,
    pub payload: &'a dyn DrawingPayload,
}

/// Structured facts about the action control currently on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Control {
    pub action: Action,
    pub rect: egui::Rect,
    pub enabled: bool,
}

struct Selection {
    owner: Owner,
    anchors: [ChartPoint; 2],
    look: NewDrawing,
    ready: bool,
    chart: Option<egui::Rect>,
    bounds: Option<egui::Rect>,
    right_limit: Option<f32>,
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Pressed {
        owner: Owner,
        position: egui::Pos2,
        anchor: ChartPoint,
    },
    Selected(Selection),
}

/// State and chrome for the one temporary range in the window.
#[derive(Default)]
pub(crate) struct QuickRange {
    state: State,
    action_rects: [Option<egui::Rect>; 3],
    demo_requested: Option<DemoRequest>,
}

#[derive(Clone, Copy)]
struct DemoRequest {
    ready: bool,
    future: bool,
}

impl QuickRange {
    pub fn request_demo(&mut self, ready: bool, future: bool) {
        self.demo_requested = Some(DemoRequest { ready, future });
    }

    pub fn stage_demo(
        &mut self,
        owner: Owner,
        opening: impl FnOnce(bool) -> Option<([ChartPoint; 2], NewDrawing)>,
    ) {
        let Some(request) = self.demo_requested else {
            return;
        };
        let Some((anchors, look)) = opening(request.future) else {
            return;
        };
        self.demo_requested = None;
        self.state = State::Selected(Selection {
            owner,
            anchors,
            look,
            ready: request.ready,
            chart: None,
            bounds: None,
            right_limit: None,
        });
    }

    pub fn press(&mut self, owner: Owner, position: egui::Pos2, anchor: ChartPoint) {
        self.state = State::Pressed {
            owner,
            position,
            anchor,
        };
        self.action_rects = [None; 3];
    }

    /// Advance the held gesture. The caller supplies the ruler's opening look
    /// only when the press first crosses the drag threshold, so a plain right
    /// click allocates nothing and remains the chart menu's gesture.
    pub fn drag(
        &mut self,
        owner: Owner,
        position: egui::Pos2,
        anchor: ChartPoint,
        threshold_px: f32,
        opening: impl FnOnce() -> NewDrawing,
    ) {
        match &mut self.state {
            State::Pressed {
                owner: pressed_owner,
                position: pressed_at,
                anchor: start,
            } if *pressed_owner == owner && pressed_at.distance(position) > threshold_px => {
                self.state = State::Selected(Selection {
                    owner,
                    anchors: [*start, anchor],
                    look: opening(),
                    ready: false,
                    chart: None,
                    bounds: None,
                    right_limit: None,
                });
            }
            State::Selected(selection) if selection.owner == owner && !selection.ready => {
                selection.anchors[1] = anchor;
            }
            _ => {}
        }
    }

    /// A release below the threshold was a click; a selected range becomes
    /// the stable choice the action bar speaks for.
    pub fn release(&mut self, owner: Owner) {
        match &mut self.state {
            State::Pressed {
                owner: pressed_owner,
                ..
            } if *pressed_owner == owner => self.dismiss(),
            State::Selected(selection) if selection.owner == owner => selection.ready = true,
            _ => {}
        }
    }

    pub fn dismiss(&mut self) {
        self.state = State::Idle;
        self.action_rects = [None; 3];
    }

    pub fn dismiss_if_present(&mut self) -> bool {
        if matches!(self.state, State::Idle) {
            return false;
        }
        self.dismiss();
        true
    }

    #[must_use]
    /// `Some(false)` is a range still being dragged, `Some(true)` one waiting
    /// for its action, and `None` means ordinary drawing chrome may speak.
    pub fn reconcile_tab(&mut self, tab: u64) -> Option<bool> {
        let belongs_here = match &self.state {
            State::Idle | State::Pressed { .. } => return None,
            State::Selected(selection) => selection.owner.tab == tab,
        };
        if !belongs_here {
            self.dismiss();
            return None;
        }
        match &self.state {
            State::Selected(selection) => Some(selection.ready),
            State::Idle | State::Pressed { .. } => None,
        }
    }

    #[must_use]
    pub fn paint(&self, owner: Owner) -> Option<Paint<'_>> {
        let State::Selected(selection) = &self.state else {
            return None;
        };
        (selection.owner == owner).then_some(Paint {
            anchors: selection.anchors,
            style: selection.look.style,
            payload: selection.look.payload.as_ref(),
        })
    }

    pub fn remember_geometry(
        &mut self,
        owner: Owner,
        chart: egui::Rect,
        bounds: egui::Rect,
        right_limit: f32,
    ) {
        if let State::Selected(selection) = &mut self.state
            && selection.owner == owner
        {
            selection.chart = Some(chart);
            selection.bounds = Some(bounds);
            selection.right_limit = Some(right_limit);
        }
    }

    #[must_use]
    fn range(&self) -> Option<(Owner, [ChartPoint; 2])> {
        let State::Selected(selection) = &self.state else {
            return None;
        };
        selection
            .ready
            .then_some((selection.owner, selection.anchors))
    }

    #[cfg(test)]
    fn request(&self) -> Option<PlaceRequest> {
        let (owner, anchors) = self.range()?;
        Some(PlaceRequest {
            action: Action::Profile,
            owner,
            anchors,
        })
    }

    #[must_use]
    #[cfg(test)]
    pub fn control(&self, active_tab: u64) -> Option<Control> {
        self.controls(active_tab)?
            .into_iter()
            .find(|control| control.action == Action::Profile)
    }

    #[must_use]
    pub fn controls(&self, active_tab: u64) -> Option<[Control; 3]> {
        let (owner, _) = self.range()?;
        if owner.tab != active_tab || self.action_rects.iter().any(Option::is_none) {
            return None;
        }
        Some(Action::ALL.map(|action| Control {
            action,
            rect: self.action_rects[action.index()].expect("all action rectangles were checked"),
            enabled: true,
        }))
    }
}

/// Draw the one-action bar after the canvas. The bar borrows the exact frame,
/// placement rule and icon geometry of the persistent drawing context bar.
pub(super) fn draw(
    quick: &mut QuickRange,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let Some((owner, anchors)) = quick.range() else {
        return DrawingChromeAsk::default();
    };
    if owner.tab != env.tab {
        quick.dismiss();
        return DrawingChromeAsk::default();
    }
    let State::Selected(selection) = &mut quick.state else {
        return DrawingChromeAsk::default();
    };
    // Consumed once per frame: the owning pane measures the range again each
    // time it paints it, so a pane a layout stopped painting (a collapsed
    // column, a hidden flow pane) leaves no bar over whatever took its place.
    let (Some(chart), Some(bounds), Some(right_limit)) = (
        selection.chart.take(),
        selection.bounds.take(),
        selection.right_limit.take(),
    ) else {
        quick.action_rects = [None; 3];
        return DrawingChromeAsk::default();
    };
    let [Some(profile), Some(retracement), Some(projection)] =
        Action::ALL.map(|action| drawings::DrawingTool::by_id(action.tool_id()))
    else {
        return DrawingChromeAsk::default();
    };
    let tools = [profile, retracement, projection];
    let mut size = drawings::context_bar::single_action_bar_size();
    size.x += TOOLRAIL_ICON.hit * (Action::ALL.len() - 1) as f32;
    let position = drawings::context_bar::place(chart, right_limit, bounds, size);
    let rect = egui::Rect::from_min_size(position, size);
    let mut clicked = None;
    let mut action_rects = [None; 3];
    egui::Area::new(egui::Id::new(BAR_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .interactable(true)
        .show(ctx, |ui| {
            drawings::context_bar::floating_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (action, tool) in Action::ALL.into_iter().zip(tools) {
                        let response = IconButton::new(tool.icon(), TOOLRAIL_ICON)
                            .vector_icon(tool.icon_strokes(), tool.icon_dots(), tool.icon_letter())
                            .hover_text(action.label())
                            .show(ui);
                        action_rects[action.index()] = Some(response.rect);
                        if response.clicked() {
                            clicked = Some(action);
                        }
                    }
                });
            });
        });
    quick.action_rects = action_rects;

    let dismiss = clicked.is_none()
        && ctx.input(|input| {
            input.pointer.primary_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| chart.contains(at) && !rect.contains(at))
        });
    if dismiss {
        quick.dismiss();
    }
    DrawingChromeAsk {
        place_quick_range: clicked.map(|action| PlaceRequest {
            action,
            owner,
            anchors,
        }),
        dismiss_quick_range: dismiss,
        ..DrawingChromeAsk::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(bar: f32) -> ChartPoint {
        ChartPoint::at_time(bar, 100.0 + f64::from(bar), Some(i64::from(bar as i32)))
    }

    fn opening() -> NewDrawing {
        let tool = drawings::DrawingTool::by_id("measure").expect("ruler is registered");
        NewDrawing {
            style: tool.default_style(),
            payload: tool.default_payload(),
        }
    }

    #[test]
    fn a_click_never_becomes_a_temporary_range() {
        let owner = Owner {
            tab: 7,
            side: PaneSide::Flow,
        };
        let mut quick = QuickRange::default();
        quick.press(owner, egui::pos2(10.0, 10.0), point(1.0));
        quick.drag(owner, egui::pos2(12.0, 10.0), point(2.0), 4.0, opening);
        quick.release(owner);
        assert!(quick.request().is_none());
    }

    #[test]
    fn a_drag_settles_one_range_and_a_second_press_replaces_it() {
        let first = Owner {
            tab: 7,
            side: PaneSide::Flow,
        };
        let second = Owner {
            tab: 8,
            side: PaneSide::Time(0),
        };
        let mut quick = QuickRange::default();
        quick.press(first, egui::pos2(10.0, 10.0), point(1.0));
        quick.drag(first, egui::pos2(20.0, 10.0), point(4.0), 4.0, opening);
        quick.release(first);
        assert_eq!(
            quick.request().expect("ready").anchors,
            [point(1.0), point(4.0)]
        );

        quick.press(second, egui::pos2(30.0, 10.0), point(5.0));
        quick.release(second);
        assert!(quick.request().is_none());
    }

    #[test]
    fn the_demo_is_one_shot_and_uses_the_same_ready_state() {
        let owner = Owner {
            tab: 7,
            side: PaneSide::Flow,
        };
        let mut quick = QuickRange::default();
        quick.request_demo(true, false);
        quick.stage_demo(owner, |_| Some(([point(2.0), point(6.0)], opening())));
        assert!(quick.request().is_some());
        quick.dismiss();
        quick.stage_demo(owner, |_| Some(([point(3.0), point(7.0)], opening())));
        assert!(quick.request().is_none(), "the launch hook is spent once");

        quick.request_demo(false, true);
        quick.stage_demo(owner, |future| {
            assert!(future);
            Some(([point(3.0), point(7.0)], opening()))
        });
        assert!(quick.paint(owner).is_some(), "the active ruler is painted");
        assert!(
            quick.request().is_none(),
            "but raises no action before release"
        );
    }

    #[test]
    fn leaving_the_owning_tab_drops_even_an_unreleased_range() {
        let owner = Owner {
            tab: 7,
            side: PaneSide::Flow,
        };
        let mut quick = QuickRange::default();
        quick.press(owner, egui::pos2(10.0, 10.0), point(1.0));
        quick.drag(owner, egui::pos2(20.0, 10.0), point(4.0), 4.0, opening);

        assert_eq!(quick.reconcile_tab(7), Some(false));
        assert_eq!(quick.reconcile_tab(8), None);
        assert!(quick.paint(owner).is_none());
        assert!(quick.request().is_none());
    }

    #[test]
    fn a_future_space_range_keeps_its_action_enabled() {
        let owner = Owner {
            tab: 7,
            side: PaneSide::Flow,
        };
        let mut quick = QuickRange::default();
        quick.press(owner, egui::pos2(10.0, 10.0), point(4.0));
        quick.drag(
            owner,
            egui::pos2(30.0, 10.0),
            ChartPoint::at(12.0, 112.0),
            4.0,
            opening,
        );
        quick.release(owner);
        quick.action_rects = [Some(egui::Rect::from_min_size(
            egui::pos2(20.0, 20.0),
            egui::vec2(24.0, 24.0),
        )); 3];

        assert!(
            quick.control(owner.tab).expect("the action").enabled,
            "future chart space is a valid drawing coordinate"
        );
    }
}
