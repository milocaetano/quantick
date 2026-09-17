//! View adapter for the headless quick-range owner.
//! Only toolkit geometry, ruler appearance and feature-gated demo setup live here.

use super::{DrawingChromeAsk, DrawingEnv};
use crate::drawings::{self, ChartPoint, DrawingPayload, NewDrawing};
use crate::pane::PaneSide;
use crate::widgets::{IconButton, TOOLRAIL_ICON};
use eframe::egui;
pub(crate) use quantick_chart_interaction::quick_range::Action;
use quantick_chart_interaction::quick_range::{self as core, Command, Effect, Event, Phase};

#[cfg(feature = "quick-range-harness")]
mod launch;
#[cfg(feature = "quick-range-harness")]
pub(crate) use launch::QuickRangeLaunch;

pub(crate) const BAR_ID: &str = "quick_range_context_bar";
pub(crate) const ACTION_CONTROL_ID: &str = "quick_range.fixed_range_profile";
pub(crate) const RETRACEMENT_CONTROL_ID: &str = "quick_range.fib_retracement";
pub(crate) const PROJECTION_CONTROL_ID: &str = "quick_range.fib_projection";
const STALE_REASON_WIDTH_PX: f32 = 180.0;

/// The model owns operation identity; the adapter supplies registry/UI names.
pub(crate) trait ActionUi {
    fn index(self) -> usize;
    fn control_id(self) -> &'static str;
    fn tool_id(self) -> &'static str;
    fn capability_id(self) -> &'static str;
    fn label(self) -> &'static str;
}
impl ActionUi for Action {
    fn index(self) -> usize {
        match self {
            Self::Profile => 0,
            Self::Retracement => 1,
            Self::Projection => 2,
        }
    }

    fn control_id(self) -> &'static str {
        match self {
            Self::Profile => ACTION_CONTROL_ID,
            Self::Retracement => RETRACEMENT_CONTROL_ID,
            Self::Projection => PROJECTION_CONTROL_ID,
        }
    }

    fn tool_id(self) -> &'static str {
        match self {
            Self::Profile => crate::frvp::TOOL_ID,
            Self::Retracement => "fib-retracement",
            Self::Projection => "fib-extension",
        }
    }

    fn capability_id(self) -> &'static str {
        match self {
            Self::Profile => crate::control::PROFILE_CAPABILITY_ID,
            Self::Retracement => crate::control::FIB_RETRACEMENT_CAPABILITY_ID,
            Self::Projection => crate::control::FIB_PROJECTION_CAPABILITY_ID,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Profile => "Fixed-range volume profile",
            Self::Retracement => "Fib retracement",
            Self::Projection => "Fib projection",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub tab: u64,
    pub side: PaneSide,
    pub pane: u64,
    pub revision: u64,
    pub layout: Option<u64>,
}

impl Owner {
    fn context(self) -> core::RangeContext {
        core::RangeContext {
            owner: core::Owner {
                tab: self.tab,
                pane: self.pane,
                layout: self.layout,
            },
            revision: self.revision,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlaceRequest {
    pub operation: core::PlaceRequest,
    pub side: PaneSide,
}

pub(crate) struct Paint<'a> {
    pub anchors: [ChartPoint; 2],
    pub style: drawings::DrawingStyle,
    pub payload: &'a dyn DrawingPayload,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Control {
    pub action: Action,
    pub rect: egui::Rect,
    pub enabled: bool,
    pub unavailable_reason: Option<&'static str>,
}

#[derive(Clone, Copy)]
struct Geometry {
    chart: egui::Rect,
    bounds: egui::Rect,
    right_limit: f32,
}

#[derive(Default)]
pub(crate) struct QuickRange {
    model: core::QuickRangeModel,
    side: Option<PaneSide>,
    look: Option<NewDrawing>,
    geometry: Option<Geometry>,
    action_rects: [Option<egui::Rect>; 3],
    #[cfg(feature = "quick-range-harness")]
    demo_requested: Option<(bool, bool)>,
    #[cfg(feature = "quick-range-harness")]
    pending_launch: Option<QuickRangeLaunch>,
}

fn anchor(point: ChartPoint) -> core::Anchor {
    core::Anchor {
        bar: point.bar,
        price: point.price,
        time_ms: point.time_ms,
    }
}

fn point(anchor: core::Anchor) -> ChartPoint {
    ChartPoint::at_time(anchor.bar, anchor.price, anchor.time_ms)
}

impl QuickRange {
    /// Read-only application adapter. The model handles removal, layout and revision changes.
    pub fn reconcile_panes(&mut self, tab: &crate::tab::Tab) {
        self.reconcile_tab(tab.id);
        let Some(owner) = self.owner() else { return };
        let current = tab.sides().find_map(|side| {
            let pane = tab.pane(side);
            (pane.id == owner.pane).then_some(Owner {
                tab: tab.id,
                side,
                pane: pane.id,
                revision: pane.pagination_revision(),
                layout: pane.layout.map(|id| id.0),
            })
        });
        self.reconcile(current);
    }
    pub fn reconcile(&mut self, owner: Option<Owner>) {
        match owner {
            Some(owner) => self.reconcile_owner(owner),
            None => self.remove_owner(),
        }
    }
    pub fn owner(&self) -> Option<Owner> {
        let context = self.model.context()?;
        Some(Owner {
            tab: context.owner.tab,
            side: self.side?,
            pane: context.owner.pane,
            revision: context.revision,
            layout: context.owner.layout,
        })
    }

    pub fn reconcile_owner(&mut self, owner: Owner) {
        self.model.observe(Event::Reconcile, owner.context());
        if self
            .model
            .context()
            .is_some_and(|c| c.owner.pane == owner.pane)
        {
            self.side = Some(owner.side);
        }
    }

    pub fn remove_owner(&mut self) {
        if let Some(context) = self.model.context() {
            self.model
                .observe(Event::PaneRemoved(context.owner.pane), context);
        }
    }

    pub fn note_selection(&mut self, pane: u64, selected: Option<u64>) {
        self.model.observe(
            Event::Selection(selected.map(|drawing| core::Selection { pane, drawing })),
            self.model.context().unwrap_or_default(),
        );
    }

    pub fn press(
        &mut self,
        owner: Owner,
        position: egui::Pos2,
        value: ChartPoint,
        eligibility: core::GestureEligibility,
    ) {
        self.model.update(
            Command::Press {
                position: [position.x, position.y],
                anchor: anchor(value),
                eligibility,
            },
            owner.context(),
        );
        if self.model.context() == Some(owner.context()) && self.model.view().is_none() {
            self.side = Some(owner.side);
            self.geometry = None;
            self.action_rects = [None; 3];
        }
    }

    pub fn drag(
        &mut self,
        owner: Owner,
        position: egui::Pos2,
        value: ChartPoint,
        threshold_px: f32,
        opening: impl FnOnce() -> NewDrawing,
    ) {
        let transition = self.model.update(
            Command::Drag {
                position: [position.x, position.y],
                anchor: anchor(value),
                threshold_px,
            },
            owner.context(),
        );
        if transition.opened {
            self.look = Some(opening());
        }
    }

    pub fn release(&mut self, owner: Owner) {
        self.model.update(Command::Release, owner.context());
    }

    pub fn dismiss(&mut self) -> bool {
        let result = self
            .model
            .update(Command::Dismiss, self.model.context().unwrap_or_default());
        self.geometry = None;
        self.action_rects = [None; 3];
        result.consumed
    }

    pub fn reconcile_tab(&mut self, tab: u64) -> Option<bool> {
        self.model.observe(
            Event::ActiveTab(tab),
            self.model.context().unwrap_or_default(),
        );
        self.model.view().map(|view| view.phase != Phase::Dragging)
    }

    pub fn paint(&self, owner: Owner) -> Option<Paint<'_>> {
        let view = self.model.view()?;
        let look = self.look.as_ref()?;
        (view.paintable() && view.context.owner == owner.context().owner).then(|| Paint {
            anchors: view.anchors.map(point),
            style: look.style,
            payload: look.payload.as_ref(),
        })
    }

    pub fn remember_geometry(
        &mut self,
        owner: Owner,
        chart: egui::Rect,
        bounds: egui::Rect,
        right_limit: f32,
    ) {
        if let Some(view) = self.model.view()
            && view.context.owner == owner.context().owner
        {
            self.geometry = Some(Geometry {
                chart,
                right_limit,
                bounds: if view.paintable() {
                    bounds
                } else {
                    egui::Rect::from_center_size(chart.center(), egui::Vec2::ZERO)
                },
            });
        }
    }

    pub fn stale(&self, owner: Owner) -> bool {
        self.model.view().is_some_and(|view| {
            view.context.owner == owner.context().owner && view.phase == Phase::Stale
        })
    }

    #[cfg(test)]
    pub fn control(&self, tab: u64) -> Option<Control> {
        self.controls(tab)?
            .into_iter()
            .find(|control| control.action == Action::Profile)
    }

    pub fn controls(&self, tab: u64) -> Option<[Control; 3]> {
        let view = self.model.view()?;
        if view.context.owner.tab != tab
            || view.phase == Phase::Dragging
            || self.action_rects.iter().any(Option::is_none)
        {
            return None;
        }
        Some(Action::ALL.map(|action| Control {
            action,
            rect: self.action_rects[action.index()].expect("checked rectangles"),
            enabled: view.actionable(),
            unavailable_reason: view.unavailable_reason(),
        }))
    }

    fn convert(&mut self, action: Action) -> Option<PlaceRequest> {
        let context = self.model.context()?;
        let Some(Effect::Place(operation)) =
            self.model.update(Command::Convert(action), context).effect
        else {
            return None;
        };
        Some(PlaceRequest {
            operation,
            side: self.side?,
        })
    }

    pub fn completed(&mut self, id: u64, succeeded: bool) -> bool {
        let event = if succeeded {
            Event::Completed(id)
        } else {
            Event::Refused(id)
        };
        self.model
            .observe(event, self.model.context().unwrap_or_default())
            .effect
            == Some(Effect::ExplainRefusal)
    }
}

pub(super) fn draw(
    quick: &mut QuickRange,
    ctx: &egui::Context,
    _env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let Some(view) = quick.model.view() else {
        return DrawingChromeAsk::default();
    };
    let Some(Geometry {
        chart,
        bounds,
        right_limit,
    }) = quick.geometry.take()
    else {
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
    if view.phase == Phase::Stale {
        size.x += STALE_REASON_WIDTH_PX;
    }
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
                            .enabled(view.actionable())
                            .disabled_explanation("The chart changed. Draw the range again.")
                            .hover_text(action.label())
                            .show(ui);
                        action_rects[action.index()] = Some(response.rect);
                        if response.clicked() {
                            clicked = Some(action);
                        }
                    }
                    if view.phase == Phase::Stale {
                        ui.label("Range changed; draw again.");
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
        place_quick_range: clicked.and_then(|action| quick.convert(action)),
        dismiss_quick_range: dismiss,
        ..DrawingChromeAsk::default()
    }
}
