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
    action_rect: Option<egui::Rect>,
    demo_requested: Option<bool>,
}

impl QuickRange {
    pub fn request_demo(&mut self, ready: bool) {
        self.demo_requested = Some(ready);
    }

    pub fn stage_demo(
        &mut self,
        owner: Owner,
        opening: impl FnOnce() -> Option<([ChartPoint; 2], NewDrawing)>,
    ) {
        let Some(ready) = self.demo_requested else {
            return;
        };
        let Some((anchors, look)) = opening() else {
            return;
        };
        self.demo_requested = None;
        self.state = State::Selected(Selection {
            owner,
            anchors,
            look,
            ready,
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
        self.action_rect = None;
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
        self.action_rect = None;
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
    pub fn request(&self) -> Option<PlaceRequest> {
        let State::Selected(selection) = &self.state else {
            return None;
        };
        selection.ready.then_some(PlaceRequest {
            owner: selection.owner,
            anchors: selection.anchors,
        })
    }

    #[must_use]
    pub fn control(&self, active_tab: u64) -> Option<Control> {
        let request = self.request()?;
        (request.owner.tab == active_tab).then_some(Control {
            rect: self.action_rect?,
            enabled: request
                .anchors
                .iter()
                .all(|anchor| anchor.time_ms.is_some()),
        })
    }
}

/// Draw the one-action bar after the canvas. The bar borrows the exact frame,
/// placement rule and icon geometry of the persistent drawing context bar.
pub(super) fn draw(
    quick: &mut QuickRange,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let Some(request) = quick.request() else {
        return DrawingChromeAsk::default();
    };
    if request.owner.tab != env.tab {
        quick.dismiss();
        return DrawingChromeAsk::default();
    }
    let State::Selected(selection) = &quick.state else {
        return DrawingChromeAsk::default();
    };
    let (Some(chart), Some(bounds), Some(right_limit)) =
        (selection.chart, selection.bounds, selection.right_limit)
    else {
        return DrawingChromeAsk::default();
    };
    let Some(tool) = drawings::DrawingTool::by_id(crate::frvp::TOOL_ID) else {
        return DrawingChromeAsk::default();
    };
    let size = drawings::context_bar::single_action_bar_size();
    let position = drawings::context_bar::place(chart, right_limit, bounds, size);
    let rect = egui::Rect::from_min_size(position, size);
    let enabled = request
        .anchors
        .iter()
        .all(|anchor| anchor.time_ms.is_some());
    let mut clicked = false;
    let mut action_rect = None;
    egui::Area::new(egui::Id::new(BAR_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .interactable(true)
        .show(ctx, |ui| {
            drawings::context_bar::floating_frame().show(ui, |ui| {
                let response = IconButton::new(tool.icon(), TOOLRAIL_ICON)
                    .vector_icon(tool.icon_strokes(), tool.icon_dots(), tool.icon_letter())
                    .enabled(enabled)
                    .hover_text(tool.name())
                    .disabled_explanation(
                        "This range includes chart space with no market time; drag over bars to place a profile",
                    )
                    .show(ui);
                action_rect = Some(response.rect);
                clicked = response.clicked();
            });
        });
    quick.action_rect = action_rect;

    let dismiss = !clicked
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
        place_quick_range_profile: (clicked && enabled).then_some(request),
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
        quick.request_demo(true);
        quick.stage_demo(owner, || Some(([point(2.0), point(6.0)], opening())));
        assert!(quick.request().is_some());
        quick.dismiss();
        quick.stage_demo(owner, || Some(([point(3.0), point(7.0)], opening())));
        assert!(quick.request().is_none(), "the launch hook is spent once");

        quick.request_demo(false);
        quick.stage_demo(owner, || Some(([point(3.0), point(7.0)], opening())));
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
}
