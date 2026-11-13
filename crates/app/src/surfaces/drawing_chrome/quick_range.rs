//! View adapter for the headless quick-range owner.
//! Only toolkit geometry, ruler appearance and feature-gated demo setup live here.

use super::{DrawingChromeAsk, DrawingEnv};
use crate::drawings::{self, ChartPoint, DrawingBand, NewDrawing};
use crate::widgets::{IconButton, TOOLRAIL_ICON};
use core::PRICE_ONLY_REASON;
use core::conversion_plan::{ConversionReadback, PendingConversion, PlacementOutcome};
use eframe::egui;
pub(crate) use quantick_chart_interaction::quick_range::Action;
use quantick_chart_interaction::quick_range::{self as core, Phase};
use quantick_control::annotation as ids;

// Gated inside the file (`#![cfg]`), so the file itself says it is harness.
mod launch;
#[cfg(any(feature = "quick-range-harness", test))]
pub(crate) use HOOKS as QUICK_RANGE_HOOKS;

pub(crate) const BAR_ID: &str = "quick_range_context_bar";
pub(crate) const ACTION_COUNT: usize = Action::ALL.len();
const STALE_REASON_WIDTH_PX: f32 = 180.0;

/// The model owns operation identity and its names; the adapter binds each
/// action to its drawing tool and its registry capability.
pub(crate) trait ActionUi {
    fn tool_id(self) -> &'static str;
    fn capability_id(self) -> &'static str;
    fn capability_version(self) -> u32;
}
impl ActionUi for Action {
    fn tool_id(self) -> &'static str {
        match self {
            Self::Profile => crate::frvp::TOOL_ID,
            Self::Retracement => "fib-retracement",
            Self::Projection => "fib-extension",
            Self::Rectangle => ids::ZONE_TOOL_ID,
            Self::TrendLine => ids::TREND_LINE_TOOL_ID,
            Self::Horizontal => ids::HORIZONTAL_LINE_TOOL_ID,
            Self::Channel => ids::PARALLEL_CHANNEL_TOOL_ID,
        }
    }

    fn capability_id(self) -> &'static str {
        match self {
            Self::Profile => ids::PROFILE_CAPABILITY_ID,
            Self::Retracement => ids::FIB_RETRACEMENT_CAPABILITY_ID,
            Self::Projection => ids::FIB_PROJECTION_CAPABILITY_ID,
            Self::Rectangle => ids::RECTANGLE_CAPABILITY_ID,
            Self::TrendLine => ids::TREND_LINE_CAPABILITY_ID,
            Self::Horizontal => ids::HORIZONTAL_LEVELS_CAPABILITY_ID,
            Self::Channel => ids::PARALLEL_CHANNEL_CAPABILITY_ID,
        }
    }

    fn capability_version(self) -> u32 {
        match self {
            Self::Profile => ids::PROFILE_CAPABILITY_VERSION,
            Self::Retracement | Self::Projection => ids::FIB_CAPABILITY_VERSION,
            Self::Rectangle => ids::RECTANGLE_CAPABILITY_VERSION,
            Self::TrendLine => ids::TREND_LINE_CAPABILITY_VERSION,
            Self::Horizontal => ids::HORIZONTAL_LEVELS_CAPABILITY_VERSION,
            Self::Channel => ids::PARALLEL_CHANNEL_CAPABILITY_VERSION,
        }
    }
}

pub(crate) use quantick_ui_model::drawing_chrome::quick_range::{Owner, PendingRangePlacement};
use quantick_ui_model::drawing_chrome::{
    self as model,
    quick_range::{Intent, State},
};

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
pub(crate) struct View {
    geometry: Option<Geometry>,
    action_rects: [Option<egui::Rect>; ACTION_COUNT],
    #[cfg(any(feature = "quick-range-harness", test))]
    rejected_launch: Option<String>,
}

pub(crate) struct QuickRange<'a> {
    session: super::Session<'a>,
    pub view: &'a mut View,
}

impl<'a> QuickRange<'a> {
    pub(super) fn new(session: super::Session<'a>, view: &'a mut View) -> Self {
        Self { session, view }
    }
    fn model(&self) -> &State {
        &self.session.state().quick_range
    }
    fn emit(&mut self, intent: Intent) -> DrawingChromeAsk {
        self.session.emit(model::Intent::QuickRange(intent))
    }
}

impl QuickRange<'_> {
    /// Read-only application adapter. The model handles removal, layout and revision changes.
    pub fn reconcile(&mut self, owner: Option<Owner>) {
        self.emit(Intent::Reconcile(owner));
    }
    pub fn owner(&self) -> Option<Owner> {
        self.model().owner()
    }

    pub fn note_selection(&mut self, pane: u64, selected: Option<u64>) {
        self.emit(Intent::Selection { pane, selected });
    }

    pub fn press(
        &mut self,
        owner: Owner,
        position: egui::Pos2,
        value: ChartPoint,
        eligibility: core::GestureEligibility,
        band: DrawingBand,
    ) {
        self.emit(Intent::Press {
            owner,
            position: [position.x, position.y],
            value,
            eligibility,
            band,
        });
        if self.model().owner() == Some(owner) && self.model().view().is_none() {
            self.view.geometry = None;
            self.view.action_rects = [None; ACTION_COUNT];
        }
    }

    pub fn drag(
        &mut self,
        owner: Owner,
        position: egui::Pos2,
        value: ChartPoint,
        threshold_px: f32,
        level: bool,
        opening: impl FnOnce() -> NewDrawing,
    ) {
        let was_open = self.model().view().is_some();
        self.emit(Intent::Drag {
            owner,
            position: [position.x, position.y],
            value,
            threshold_px,
            level,
        });
        if !was_open && self.model().view().is_some() {
            self.emit(Intent::Opened(opening()));
        }
    }

    /// The band the current range was pressed on.
    pub fn band(&self) -> Option<&DrawingBand> {
        self.model().band()
    }

    /// Why `action` cannot run on the current range, by the model's rule and
    /// the tool's own [`drawings::DrawingTool::holds_band`].
    fn unavailable_reason(&self, action: Action) -> Option<&'static str> {
        self.model().unavailable_reason(action.tool_id())
    }

    pub fn release(&mut self, owner: Owner) {
        self.emit(Intent::Release(owner));
    }

    pub fn dismiss(&mut self) -> bool {
        self.view.geometry = None;
        self.view.action_rects = [None; ACTION_COUNT];
        let consumed = self.model().present();
        self.emit(Intent::Dismiss);
        consumed
    }

    pub fn reconcile_tab(&mut self, tab: u64) -> Option<bool> {
        self.emit(Intent::ActiveTab(tab));
        self.model()
            .view()
            .map(|view| view.phase != Phase::Dragging)
    }

    pub fn remember_geometry(
        &mut self,
        owner: Owner,
        chart: egui::Rect,
        bounds: egui::Rect,
        right_limit: f32,
    ) {
        if let Some(view) = self.model().view()
            && view.context.owner == owner.context().owner
        {
            self.view.geometry = Some(Geometry {
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

    pub(crate) fn convert(&mut self, action: Action) -> Option<Box<PendingRangePlacement>> {
        self.emit(Intent::Convert(action)).place_quick_range
    }

    pub fn finish_conversion(
        &mut self,
        conversion: PendingConversion,
        outcome: PlacementOutcome,
    ) -> ConversionReadback {
        self.emit(Intent::FinishConversion {
            conversion,
            outcome,
        })
        .take_quick_range_readback()
        .expect("conversion completion is a host operation")
    }
}

pub(super) fn draw(
    quick: &mut QuickRange,
    ctx: &egui::Context,
    _env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let palette = crate::theme::get(ctx);
    let Some(view) = quick.model().view() else {
        return DrawingChromeAsk::default();
    };
    let Some(Geometry {
        chart,
        bounds,
        right_limit,
    }) = quick.view.geometry.take()
    else {
        quick.view.action_rects = [None; ACTION_COUNT];
        return DrawingChromeAsk::default();
    };
    let tools = Action::ALL.map(|action| drawings::DrawingTool::by_id(action.tool_id()));
    if tools.iter().any(Option::is_none) {
        return DrawingChromeAsk::default();
    }
    let mut size = drawings::context_bar::single_action_bar_size();
    size.x += TOOLRAIL_ICON.hit * (Action::ALL.len() - 1) as f32;
    if view.phase == Phase::Stale {
        size.x += STALE_REASON_WIDTH_PX;
    }
    let position = drawings::context_bar::place(chart, right_limit, bounds, size);
    let rect = egui::Rect::from_min_size(position, size);
    let mut clicked = None;
    let mut action_rects = [None; ACTION_COUNT];
    egui::Area::new(egui::Id::new(BAR_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .interactable(true)
        .show(ctx, |ui| {
            drawings::context_bar::floating_frame(&palette).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (action, tool) in Action::ALL.into_iter().zip(tools.into_iter().flatten()) {
                        let reason = quick.unavailable_reason(action);
                        let response = IconButton::new(tool.icon(), TOOLRAIL_ICON)
                            .vector_icon(tool.icon_strokes(), tool.icon_dots(), tool.icon_letter())
                            .enabled(reason.is_none())
                            .disabled_explanation(if reason == Some(PRICE_ONLY_REASON) {
                                "This tool reads price. Draw the range on the candles."
                            } else {
                                "The chart changed. Draw the range again."
                            })
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
    quick.view.action_rects = action_rects;
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

/// Immutable projection of model state and the last painted action geometry.
pub(crate) struct Read<'a> {
    pub(super) state: &'a State,
    pub(super) view: &'a View,
}
impl std::ops::Deref for Read<'_> {
    type Target = State;
    fn deref(&self) -> &State {
        self.state
    }
}
impl Read<'_> {
    pub fn controls(&self, tab: u64) -> Option<[Control; ACTION_COUNT]> {
        let view = self.state.view()?;
        if view.context.owner.tab != tab
            || view.phase == Phase::Dragging
            || self.view.action_rects.iter().any(Option::is_none)
        {
            return None;
        }
        Some(Action::ALL.map(|action| {
            let unavailable_reason = self.state.unavailable_reason(action.tool_id());
            Control {
                action,
                rect: self.view.action_rects[action.index()].expect("checked rectangles"),
                enabled: unavailable_reason.is_none(),
                unavailable_reason,
            }
        }))
    }

    #[cfg(test)]
    pub fn control(&self, tab: u64) -> Option<Control> {
        self.controls(tab)?
            .into_iter()
            .find(|control| control.action == Action::Profile)
    }
}

#[cfg(any(feature = "quick-range-harness", test))]
use quantick_ui_model::drawing_chrome::quick_range::Staging;
#[cfg(any(feature = "quick-range-harness", test))]
use std::ffi::OsString;
#[cfg(any(feature = "quick-range-harness", test))]
type Staged = ([ChartPoint; 2], NewDrawing);
#[cfg(any(feature = "quick-range-harness", test))]
#[derive(Default)]
pub(crate) struct QuickRangeLaunch {
    requested: Option<core::DemoRequest>,
    rejected: Option<String>,
}

#[cfg(any(feature = "quick-range-harness", test))]
impl QuickRangeLaunch {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let Some(value) =
            lookup("QUANTICK_QUICK_RANGE_DEMO").and_then(|value| value.into_string().ok())
        else {
            return Self::default();
        };
        let requested = core::DemoRequest::parse(value.trim());
        Self {
            rejected: requested.is_none().then(|| value.trim().to_owned()),
            requested,
        }
    }

    #[cfg(test)]
    fn deliver(self, request: impl FnOnce(core::DemoRequest)) {
        if let Some(requested) = self.requested {
            request(requested);
        }
        if let Some(value) = self.rejected {
            tracing::warn!(value, "unknown quick-range demo state");
        }
    }
}

#[cfg(any(feature = "quick-range-harness", test))]
impl QuickRange<'_> {
    /// Construction queues only the input; the drawing launch phase applies it.
    pub fn queue_launch(&mut self, input: QuickRangeLaunch) {
        self.emit(Intent::QueueLaunch(input.requested));
        self.view.rejected_launch = input.rejected;
    }

    pub fn apply_launch(&mut self) {
        self.emit(Intent::ApplyLaunch);
        if let Some(value) = self.view.rejected_launch.take() {
            tracing::warn!(value, "unknown quick-range demo state");
        }
    }

    /// `opening` reads the asked band: `Ok(None)` waits for it to have
    /// values, `Err` is a band the chart will not show, which drops the ask.
    pub fn stage_demo(
        &mut self,
        owner: Owner,
        opening: impl FnOnce(&DrawingBand, bool) -> Result<Option<Staged>, &'static str>,
    ) {
        let Some(request) = self.model().demo_requested() else {
            return;
        };
        let staging = match opening(&request.band, request.future) {
            Ok(Some((anchors, look))) => Staging::Ready {
                owner,
                anchors,
                look,
            },
            Ok(None) => Staging::Wait,
            Err(reason) => {
                tracing::warn!(reason, "quick-range demo band refused");
                Staging::Refused
            }
        };
        self.emit(Intent::StageDemo(staging));
    }
}

#[cfg(any(feature = "quick-range-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_QUICK_RANGE_DEMO"];
