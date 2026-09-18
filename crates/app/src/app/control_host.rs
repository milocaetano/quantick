//! What the control plane is allowed to see and do to the window.
//!
//! The accessors an agent reads the cockpit through, the two dispatch
//! points that turn a control-plane request into a mutation
//! (`control_action`, `run_agent_action`), the popup/toast/sound lanes it
//! answers on, and the focused-pane helpers every one of them resolves a
//! target with. They are together because they share one rule: none of
//! them may assume a surface was drawn, because the caller is a script.

use std::time::Instant;

use eframe::egui;

use crate::config::AppConfig;
use crate::dock::Dock;
use crate::drawings;
use crate::feed_notice;
use crate::pane::ChartPane;
use crate::style::ChartStyle;
use crate::tab::Tab;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;

use quantick_feed::history_reach;

use super::{ControlFrameMetrics, QuantickApp};

/// The ordinary local gateway and its opt-in launch scenarios.
pub(super) struct ControlState {
    pub(super) control_access: Option<crate::control::ControlAccess>,
    #[cfg(any(feature = "control-harness", test))]
    pub(super) scenarios: ControlScenarios,
}

/// The temporary range's visible action button, if the current frame has laid
/// it out. A free read port keeps this extension out of the protected
/// `QuantickApp` implementation root.
#[cfg(test)]
pub(crate) fn control_quick_range(
    app: &QuantickApp,
) -> Option<crate::surfaces::drawing_chrome::QuickRangeControl> {
    app.drawings
        .chrome
        .quick_range
        .control(app.tabs.id_at(app.tabs.active_index()))
}

/// All drawing actions in the temporary range's visible action bar.
pub(crate) fn control_quick_range_actions(
    app: &QuantickApp,
) -> Option<[crate::surfaces::drawing_chrome::QuickRangeControl; 3]> {
    app.drawings
        .chrome
        .quick_range
        .controls(app.tabs.id_at(app.tabs.active_index()))
}

/// Control-only launch inputs, captured before owner construction.
#[cfg(any(feature = "control-harness", test))]
#[derive(Default)]
pub(crate) struct ControlLaunch {
    panel: bool,
    scopes: Option<String>,
    scenarios: ControlScenarios,
}

#[cfg(any(feature = "control-harness", test))]
impl ControlLaunch {
    pub(crate) fn capture(mut lookup: impl FnMut(&str) -> Option<std::ffi::OsString>) -> Self {
        let mut value = |name| lookup(name).and_then(|raw| raw.into_string().ok());
        let panel = value("QUANTICK_CONTROL_PANEL").is_some_and(|raw| raw == "1");
        let scopes = value("QUANTICK_CONTROL_SCOPES");
        let access = value("QUANTICK_CONTROL_ACCESS").is_some_and(|raw| raw == "1");
        let mark = value("QUANTICK_CONTROL_MARK")
            .filter(|raw| !raw.trim().is_empty())
            .map(|raw| if raw == "1" { String::new() } else { raw });
        let evidence = value("QUANTICK_CONTROL_EVIDENCE").filter(|raw| !raw.trim().is_empty());
        let annotation = value("QUANTICK_CONTROL_ANNOTATE").filter(|raw| !raw.trim().is_empty());
        let notification = value("QUANTICK_CONTROL_NOTIFY").filter(|raw| !raw.trim().is_empty());
        Self {
            panel,
            scopes,
            scenarios: ControlScenarios {
                pending_control_access_enable: access,
                pending_control_annotation: annotation,
                pending_control_notification: notification,
                pending_control_evidence: evidence,
                pending_control_mark: mark,
                evidence_frames: 0,
            },
        }
    }
}

#[cfg(any(feature = "control-harness", test))]
impl ControlState {
    pub(super) fn apply_launch(&mut self, launch: ControlLaunch) {
        if launch.panel
            && let Some(access) = self.control_access.as_mut()
        {
            access.open_panel();
        }
        if let Some(scopes) = launch.scopes
            && let Some(access) = self.control_access.as_mut()
            && let Err(error) = access.configure_scopes(&scopes)
        {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_SCOPE_HOOK_REFUSED",
                error = %error,
                "QUANTICK_CONTROL_SCOPES named something this build does not register"
            );
        }
        self.scenarios = launch.scenarios;
    }
}

/// Only launch scenarios live here. The ordinary gateway remains in ControlState.
#[cfg(any(feature = "control-harness", test))]
#[derive(Default)]
pub(super) struct ControlScenarios {
    pending_control_access_enable: bool,
    pending_control_annotation: Option<String>,
    pending_control_notification: Option<String>,
    pending_control_evidence: Option<String>,
    pending_control_mark: Option<String>,
    /// Lifetime-wide screenshot wait count, never reset by request rearming.
    evidence_frames: u32,
}

#[cfg(any(feature = "control-harness", test))]
pub(super) struct EvidenceCapture {
    request: String,
    scopes: std::collections::BTreeSet<String>,
    pub(super) wants_screenshot: bool,
    pub(super) screenshot_not_granted: bool,
}

#[cfg(any(feature = "control-harness", test))]
pub(super) enum EvidenceStep {
    Waiting,
    Capture {
        scopes: std::collections::BTreeSet<String>,
        screenshot: bool,
        image_timed_out: bool,
    },
}

#[cfg(any(feature = "control-harness", test))]
pub(super) enum NotificationStep {
    Ready {
        capability: &'static str,
        input: serde_json::Value,
    },
    Refused {
        channel: String,
    },
}

/// The same bounded wait as the original Harness: wait attempts1..=120,
/// then capture honest missing-image coverage on attempt121.
#[cfg(any(feature = "control-harness", test))]
pub(crate) const CONTROL_EVIDENCE_HOOK_FRAMES: u32 = 120;

#[cfg(any(feature = "control-harness", test))]
impl ControlScenarios {
    pub(super) fn take_enable(&mut self) -> bool {
        std::mem::take(&mut self.pending_control_access_enable)
    }

    pub(super) fn take_mark(&mut self) -> Option<String> {
        self.pending_control_mark.take()
    }

    pub(super) fn has_annotation(&self) -> bool {
        self.pending_control_annotation.is_some()
    }

    pub(super) fn has_evidence(&self) -> bool {
        self.pending_control_evidence.is_some()
    }

    pub(super) fn annotation(
        &mut self,
        anchor: Option<serde_json::Value>,
    ) -> Option<serde_json::Value> {
        let anchor = anchor?;
        let text = self.pending_control_annotation.take()?;
        Some(serde_json::json!({ "anchors": [anchor], "text": text }))
    }

    pub(super) fn notification(&mut self) -> Option<NotificationStep> {
        let request = self.pending_control_notification.take()?;
        let (channel, message) = request
            .split_once(':')
            .unwrap_or(("toast", request.as_str()));
        let capability = match channel.trim() {
            "popup" => "notify.popup",
            "sound" => "notify.sound",
            "toast" => "notify.toast",
            other => {
                return Some(NotificationStep::Refused {
                    channel: other.to_owned(),
                });
            }
        };
        Some(NotificationStep::Ready {
            capability,
            input: serde_json::json!({ "message": message, "title": "From your assistant" }),
        })
    }

    // The caller obtains access before this takes the pending input.
    pub(super) fn prepare_evidence(
        &mut self,
        access: &crate::control::ControlAccess,
    ) -> Option<EvidenceCapture> {
        let request = self.pending_control_evidence.take()?;
        let mut wants_screenshot = false;
        let mut scopes = std::collections::BTreeSet::new();
        for token in request
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            match token {
                "screenshot" => wants_screenshot = true,
                "all" | "1" => scopes.extend(
                    access
                        .readable_scopes()
                        .into_iter()
                        .map(|scope| scope.to_string()),
                ),
                scope => {
                    scopes.insert(scope.to_owned());
                }
            }
        }
        if scopes.is_empty() {
            scopes.extend(
                access
                    .readable_scopes()
                    .into_iter()
                    .map(|scope| scope.to_string()),
            );
        }
        let screenshot_not_granted = wants_screenshot && !access.grants_screenshot();
        wants_screenshot &= !screenshot_not_granted;
        Some(EvidenceCapture {
            request,
            scopes,
            wants_screenshot,
            screenshot_not_granted,
        })
    }

    pub(super) fn finish_evidence(
        &mut self,
        capture: EvidenceCapture,
        has_screenshot: bool,
    ) -> EvidenceStep {
        let missing = capture.wants_screenshot && !has_screenshot;
        if missing && self.evidence_frame_waited() {
            self.pending_control_evidence = Some(capture.request);
            return EvidenceStep::Waiting;
        }
        EvidenceStep::Capture {
            scopes: capture.scopes,
            screenshot: capture.wants_screenshot,
            image_timed_out: missing,
        }
    }

    pub(super) fn evidence_frame_waited(&mut self) -> bool {
        self.evidence_frames = self.evidence_frames.saturating_add(1);
        self.evidence_frames <= CONTROL_EVIDENCE_HOOK_FRAMES
    }
}

#[cfg(test)]
pub(super) struct PendingControl<'a> {
    pub(super) enable: bool,
    pub(super) mark: Option<&'a str>,
    pub(super) annotation: Option<&'a str>,
    pub(super) notification: Option<&'a str>,
    pub(super) evidence: Option<&'a str>,
}

#[cfg(test)]
impl ControlScenarios {
    pub(super) fn pending(&self) -> PendingControl<'_> {
        PendingControl {
            enable: self.pending_control_access_enable,
            mark: self.pending_control_mark.as_deref(),
            annotation: self.pending_control_annotation.as_deref(),
            notification: self.pending_control_notification.as_deref(),
            evidence: self.pending_control_evidence.as_deref(),
        }
    }

    pub(super) fn queue_annotation(&mut self, text: String) {
        self.pending_control_annotation = Some(text);
    }

    pub(super) fn queue_notification(&mut self, request: String) {
        self.pending_control_notification = Some(request);
    }

    pub(super) fn queue_mark(&mut self, note: String) {
        self.pending_control_mark = Some(note);
    }

    pub(super) fn queue_evidence(&mut self, request: String) {
        self.pending_control_evidence = Some(request);
    }
}

crate::hooks::declare_hooks![
    "QUANTICK_CONTROL_ACCESS",
    "QUANTICK_CONTROL_ANNOTATE",
    "QUANTICK_CONTROL_EVIDENCE",
    "QUANTICK_CONTROL_MARK",
    "QUANTICK_CONTROL_NOTIFY",
    "QUANTICK_CONTROL_PANEL",
    "QUANTICK_CONTROL_SCOPES"
];

impl QuantickApp {
    /// The active tab beside the config it reads.
    ///
    /// Split here, once, because almost every tab operation needs both and
    /// `self.tabs[i].f(&self.config)` is a borrow error at every call site.
    pub(super) fn active_with_config(&mut self) -> (&mut Tab, &AppConfig) {
        (
            self.tabs.runtime_mut(self.tabs.active_index()),
            &self.config,
        )
    }

    /// The tab on screen.
    pub(super) fn active_tab(&self) -> &Tab {
        &self.tabs[self.tabs.active_index()]
    }

    /// See [`Self::active_tab`].
    pub(super) fn active_tab_mut(&mut self) -> &mut Tab {
        self.tabs.runtime_mut(self.tabs.active_index())
    }

    /// Read-only application roots available to the on-demand control
    /// projections. The gateway never receives `QuantickApp`; it receives the
    /// owned DTOs built from these narrow views.
    /// One tab by position, for a control capability that resolved an id.
    pub(crate) fn control_tab_at(&self, index: usize) -> Option<&Tab> {
        self.tabs.get(index)
    }

    /// The mutable twin, for the cockpit tier.
    ///
    /// Narrow on purpose: the layout capabilities need to *change* a tab, and
    /// handing them the whole application would let a later one reach past the
    /// canvas into the feed or the simulator.
    pub(crate) fn control_tab_at_mut(&mut self, index: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(index)
    }

    /// One tab beside the configuration it reads, by position.
    ///
    /// [`Self::active_with_config`] for a tab that is not necessarily the
    /// active one — a capability names the tab it acts on, and respawning a
    /// feed needs the feed table the same way a click in the corner does.
    pub(crate) fn control_tab_with_config(
        &mut self,
        index: usize,
    ) -> Option<(&mut Tab, &AppConfig)> {
        let Self { tabs, config, .. } = self;
        tabs.get_mut(index).map(|tab| (tab, &*config))
    }

    /// The trading host of the tab on screen — where the `trade.*` actions
    /// land. The active tab and not an addressed one: an order belongs to
    /// the symbol the trader is looking at, and a call that could quietly
    /// trade a chart nobody has open is a call nobody should be able to
    /// make.
    pub(crate) fn control_active_paper_mut(
        &mut self,
    ) -> Option<&mut crate::paper_trading::PaperTrading> {
        // Fallible, because the rest of the control code does not trust the
        // invariant either: `annotate::resolve_target` guards an empty tab
        // list and clamps the index, and two more sites clamp it. A
        // `trade.*` call must answer "this window has no chart open" rather
        // than panic the whole trading application, and it must resolve the
        // *same* tab its own read-back resolves.
        self.tabs
            .get_mut(self.tabs.active_index())
            .map(|tab| &mut tab.paper)
    }

    /// The read side of [`Self::control_active_paper_mut`], resolved the same
    /// way so a call and its read-back can never name different tabs.
    pub(crate) fn control_active_paper(&self) -> Option<&crate::paper_trading::PaperTrading> {
        self.tabs
            .get(self.tabs.active_index())
            .map(|tab| &tab.paper)
    }

    pub(crate) fn control_tabs(&self) -> &crate::app::arrangement_host::ArrangementHost {
        &self.tabs
    }

    pub(crate) fn control_active_tab_index(&self) -> usize {
        self.tabs.active_index()
    }

    /// Open the assistant's popup. One at a time: a second message replaces
    /// the first rather than stacking windows over a chart someone is
    /// trading, and the trader dismisses it.
    pub(crate) fn show_agent_popup(&mut self, popup: crate::control::AgentPopup) {
        self.surfaces.agent_popup.show(popup);
    }

    /// Post one line to the window's own acknowledgement lane — the same
    /// channel a delete or a workspace save uses, with no Undo: there is
    /// nothing to take back from having been told something.
    pub(crate) fn show_agent_toast(&mut self, message: String) {
        self.surfaces.toast.note(message, Instant::now());
    }

    /// Ask for the platform's attention sound, through the same sink the
    /// alarms use, and report honestly when it could not be made rather
    /// than letting a client believe it was heard.
    pub(crate) fn sound_agent_alert(&mut self) -> Option<String> {
        self.audio
            .alerts
            .play(&[crate::audio::Cue::default()])
            .err()
            .map(ToOwned::to_owned)
    }

    /// One pane, by tab position and side — the mutable half of
    /// [`Self::control_tabs`], for the actions that place objects.
    pub(crate) fn control_pane_mut(
        &mut self,
        tab_index: usize,
        side: crate::pane::PaneSide,
    ) -> &mut ChartPane {
        self.tabs.runtime_mut(tab_index).pane_mut(side)
    }

    /// What a freshly placed object of `tool` opens with, through the same
    /// door the click path uses — saved defaults, named preset and all.
    pub(crate) fn control_new_drawing(&self, tool: drawings::DrawingTool) -> drawings::NewDrawing {
        drawings::new_drawing_from_defaults(&self.drawings.presets, tool)
    }

    pub(crate) fn control_config(&self) -> &AppConfig {
        &self.config
    }

    /// The window's footprint setup — the one a pane falls back to when it
    /// carries no override of its own.
    pub(crate) fn control_footprint_config(&self) -> &crate::footprint_config::FootprintConfig {
        &self.footprint_config
    }

    /// The window's shared chart style, which owns the layers no pane does.
    pub(crate) fn control_style(&self) -> &ChartStyle {
        &self.style
    }

    pub(crate) fn control_set_layer(
        &mut self,
        tab: usize,
        side: crate::pane::PaneSide,
        layer: crate::chart_layers::ChartLayer,
        visible: bool,
    ) {
        self.tabs.runtime_mut(tab).pane_mut(side).set_layer_visible(
            layer,
            visible,
            &mut self.workspace.layers_mut().actions,
        );
        self.layer_wiring().apply_actions();
    }

    /// The drawing tool rail: which tool is armed, and whether it is on
    /// screen at all.
    pub(crate) fn control_tool_rail(&self) -> &ToolRail {
        &self.toolrail
    }

    /// The colour the chart's corner is wearing, or `None` while the chart
    /// is being fed.
    ///
    /// The status line's provenance dot takes this rather than deciding for
    /// itself. It used to read the connection alone, which is a socket's
    /// opinion: a terminal that froze with the socket open had the
    /// bottom-left of the window saying `live` while the bottom-right said
    /// `offline`, about the same feed, at the same moment. Two surfaces
    /// disagreeing about the one question the trader is asking is worse than
    /// either answer alone, so there is one report and both read it.
    pub(super) fn feed_offline_accent(
        &self,
        stall: Option<&quantick_feed::stall::Stall>,
    ) -> Option<egui::Color32> {
        feed_notice::report(&self.active_tab().notice, stall)
            .filter(feed_notice::Report::is_offline)
            .map(|report| report.accent())
    }

    /// Where the feed's offline chip was painted, or `None` when it was not.
    ///
    /// The projection reads what was drawn rather than re-deciding it, so the
    /// scene and the screen cannot disagree across the edge of a stall budget.
    pub(crate) fn control_feed_chip_rect(&self) -> Option<egui::Rect> {
        self.chrome.feed_chip_rect
    }

    /// Whether the recovery popup that chip opens is showing, on the chart
    /// the trader is looking at.
    pub(crate) fn control_feed_popup_open(&self) -> bool {
        self.chrome.feed_popup_tab == Some(self.tabs.active_id())
    }

    /// The right-hand dock: whether it is shown, and which tab is open.
    pub(crate) fn control_dock(&self) -> &Dock {
        &self.dock
    }

    pub(crate) fn control_timezone(&self) -> TzOffset {
        self.tz
    }

    pub(crate) fn control_workspace_flags(&self) -> (bool, bool, bool) {
        (
            self.workspace.session().save_on_exit(),
            self.health.show_perf,
            self.history.progressive_history,
        )
    }

    /// Choose how far one press of *load older* reaches.
    ///
    /// The named call behind the history menu's reach chips and the
    /// `QUANTICK_HISTORY_REACH` hook — one path, so an operator without a
    /// mouse sets what a click sets. Mirrored onto every tab by `drain_tabs`,
    /// where a run in flight also reads it: withdrawing the longer reach is
    /// how a trader calls that run off.
    pub(crate) fn set_history_reach(&mut self, reach: history_reach::HistoryReach) {
        self.history.history_reach = reach;
    }

    /// How far back one press of the `by time` reach pulls, in minutes of
    /// traded time.
    ///
    /// Clamped rather than refused: a span of zero is a press that asks for
    /// nothing, and the operator that sent it meant *some* history. The
    /// ceiling is the campaign's own span cap, past which no run can reach
    /// anyway, so accepting a larger number would be promising a reach the
    /// budgets forbid.
    pub(crate) fn set_history_reach_span_minutes(&mut self, minutes: u32) {
        self.history.set_span_minutes(minutes);
    }

    /// What that span is now, for an operator reading back what it set.
    pub(crate) fn control_history_reach_span_minutes(&self) -> u32 {
        self.history.history_reach_span_minutes
    }

    /// How far the window's *load older* press reaches, and whether a chart
    /// cut by trades carries the venue's candles.
    ///
    /// Both are choices an operator without a mouse has to be able to read
    /// back after setting them — the reach especially, since it decides
    /// whether one press is one request or a run of them.
    pub(crate) fn control_history_settings(&self) -> (history_reach::HistoryReach, bool) {
        (self.history.history_reach, self.history.venue_lead_in)
    }

    /// Whether a recording opens with the session day before it joined in
    /// front, and a download fetches that day's tape too.
    ///
    /// A choice an operator without a mouse has to be able to read back after
    /// setting it: it decides what a replay they are about to open will hold.
    pub(crate) fn control_replay_day_before(&self) -> bool {
        self.replay_view.day_before()
    }

    /// Invoke one registered control action from inside the application,
    /// attributed to the human at this window (or to automation when a
    /// control trace replays it). The hotkey, the `QUANTICK_CONTROL_MARK`
    /// hook and the tests all arrive here; there is no second path.
    pub(crate) fn control_action(
        &mut self,
        capability_id: &str,
        capability_version: u32,
        origin: crate::control::ActionOrigin,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.control.control_access.take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        let outcome =
            access.invoke_local_action(self, capability_id, capability_version, input, origin);
        self.control.control_access = Some(access);
        outcome
    }

    /// Launch scenarios invoke the registered label and notification handlers
    /// through trusted local actions with an agent actor. Unlike remote calls,
    /// these local actions do not pass through configured remote-grant admission.
    #[cfg(any(feature = "control-harness", test))]
    pub(super) fn apply_control_annotate_hooks(&mut self) {
        if self.control.scenarios.has_annotation() {
            let pane = self.active_tab().drawing_pane();
            let slot = pane.slots().saturating_sub(1);
            let anchor = match (pane.slot_open_time(slot), pane.closed_bar(slot)) {
                (Some(time), Some(bar)) => Some(serde_json::json!({
                    "time_unix_ms": time,
                    "price": rust_decimal::prelude::ToPrimitive::to_f64(&bar.close).unwrap_or(1.0).to_string(),
                })),
                _ => None,
            };
            if let Some(input) = self.control.scenarios.annotation(anchor) {
                self.run_hook_action("annotate.label.create", input);
            }
        }
        match self.control.scenarios.notification() {
            Some(NotificationStep::Ready { capability, input }) => {
                self.run_hook_action(capability, input)
            }
            Some(NotificationStep::Refused { channel }) => tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_NOTIFY_HOOK_REFUSED",
                channel = %channel,
                "QUANTICK_CONTROL_NOTIFY names no notification channel"
            ),
            None => {}
        }
    }

    /// Invoke one registered action as an *agent* would, from inside this
    /// window. The hooks use it so a screenshot shows a real assistant's
    /// object, attribution and all, without a client on the socket.
    #[cfg(any(feature = "control-harness", test))]
    pub(super) fn run_agent_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.control.control_access.take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        // No identity, no actor to sign with: the same structured refusal an
        // action gets, rather than a panic on the first frame.
        let Some(actor) = access.hook_agent_actor() else {
            self.control.control_access = Some(access);
            return Err(quantick_control::error::ControlError::invalid_request(
                "this window has no control identity to act with",
            ));
        };
        let outcome = access.invoke_local_action(
            self,
            capability_id,
            1,
            input,
            crate::control::ActionOrigin::Remote(Box::new(actor)),
        );
        self.control.control_access = Some(access);
        outcome
    }

    /// A launch hook's action, with its failure reported where a scripted run
    /// will see it: the hook is fire-and-forget, so nothing else would.
    #[cfg(any(feature = "control-harness", test))]
    fn run_hook_action(&mut self, capability_id: &str, input: serde_json::Value) {
        if let Err(error) = self.run_agent_action(capability_id, input) {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_HOOK_ACTION_FAILED",
                capability = capability_id,
                error_code = %error.code,
                error = %error.message,
                "an annotate hook could not run its action"
            );
        }
    }

    /// The mark hotkey's body: `attention.mark.create` with the resolved
    /// cursor target, attributed to the human.
    pub(crate) fn take_mark(&mut self, note: Option<String>) {
        let mut input = serde_json::Map::new();
        if let Some(note) = note {
            input.insert("note".to_owned(), serde_json::Value::String(note));
        }
        // No target: the action port resolves the pointer at the moment of
        // the gesture and records the resolved input, so the trace line
        // determines the mark on its own and a rerun marks the same bar.
        match self.control_action(
            crate::control::MARK_CAPABILITY_ID,
            crate::control::MARK_CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::Value::Object(input),
        ) {
            Ok(result) => tracing::info!(
                target: "quantick::control",
                event_code = "CONTROL_MARK_TAKEN",
                sequence = %result["sequence"],
                "mark taken"
            ),
            Err(error) => tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_MARK_REFUSED",
                code = %error.code,
                "mark refused"
            ),
        }
    }

    pub(crate) fn control_frame_metrics(&self) -> ControlFrameMetrics {
        ControlFrameMetrics {
            wall_average_ms: self.health.frames.avg_ms(),
            wall_worst_ms: self.health.frames.worst_ms(),
            frames_per_second: self.health.frames.fps(),
            cpu_average_ms: self.health.cpu_frames.avg_ms(),
            cpu_worst_ms: self.health.cpu_frames.worst_ms(),
        }
    }

    /// The pane the chrome speaks for: the active tab's focused pane (§11).
    pub(super) fn focused_pane(&self) -> &ChartPane {
        self.active_tab().focused_pane()
    }

    /// See [`Self::focused_pane`].
    pub(super) fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().focused_pane_mut()
    }

    /// The pane every drawing surface speaks for: the one holding the
    /// selection, which is the focused pane unless a shared mark was taken
    /// from the chart it is mirrored on (see [`Tab::drawing_side`]).
    ///
    /// The inspector, the keyboard, the object manager and the toast all read
    /// through here, so an object selected on either of its two charts is
    /// edited and deleted from either of them.
    pub(super) fn drawing_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().drawing_pane_mut()
    }
}

#[cfg(test)]
mod control_launch_tests {
    use super::*;

    fn launch(inputs: &[(&str, &str)]) -> ControlLaunch {
        ControlLaunch::capture(|name| {
            inputs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| std::ffi::OsString::from(value))
        })
    }

    #[test]
    fn captured_enable_and_mark_are_consumed_once() {
        let mut launch = launch(&[
            ("QUANTICK_CONTROL_ACCESS", "1"),
            ("QUANTICK_CONTROL_MARK", " 1 "),
        ]);
        assert!(launch.scenarios.take_enable());
        assert!(!launch.scenarios.take_enable());
        assert_eq!(launch.scenarios.take_mark().as_deref(), Some(" 1 "));
        assert_eq!(launch.scenarios.take_mark(), None);
    }

    #[test]
    fn annotation_retains_raw_text_until_an_anchor_exists() {
        let mut launch = launch(&[("QUANTICK_CONTROL_ANNOTATE", " keep this ")]);
        assert!(launch.scenarios.annotation(None).is_none());
        assert!(launch.scenarios.has_annotation());
        let anchor = serde_json::json!({"time_unix_ms": 1800, "price": "100.8"});
        assert_eq!(
            launch.scenarios.annotation(Some(anchor.clone())),
            Some(serde_json::json!({"anchors": [anchor], "text": " keep this "}))
        );
        assert!(!launch.scenarios.has_annotation());
    }

    #[test]
    fn notification_preserves_message_and_types_unknown_channel() {
        let mut launch = launch(&[("QUANTICK_CONTROL_NOTIFY", " popup :a:b ")]);
        let Some(NotificationStep::Ready { capability, input }) = launch.scenarios.notification()
        else {
            panic!("registered popup must be ready");
        };
        assert_eq!(capability, "notify.popup");
        assert_eq!(
            input,
            serde_json::json!({"message": "a:b ", "title": "From your assistant"})
        );
        assert!(launch.scenarios.notification().is_none());
        launch
            .scenarios
            .queue_notification(" unknown :message".into());
        let Some(NotificationStep::Refused { channel }) = launch.scenarios.notification() else {
            panic!("unknown channel must be typed refusal");
        };
        assert_eq!(channel, "unknown");
        assert!(launch.scenarios.notification().is_none());
    }

    #[test]
    fn evidence_reexpands_current_grants_and_keeps_its_lifetime_wait_count() {
        let mut access = crate::control::ControlAccess::new();
        access
            .configure_scopes("all-reads,observe.evidence,observe.screenshot")
            .unwrap();
        let mut launch = launch(&[("QUANTICK_CONTROL_EVIDENCE", "all,screenshot")]);
        for _ in 0..CONTROL_EVIDENCE_HOOK_FRAMES {
            let capture = launch.scenarios.prepare_evidence(&access).unwrap();
            assert!(capture.wants_screenshot);
            assert!(!capture.screenshot_not_granted);
            assert!(matches!(
                launch.scenarios.finish_evidence(capture, false),
                EvidenceStep::Waiting
            ));
        }
        access
            .configure_scopes("observe.events,observe.evidence,observe.screenshot")
            .unwrap();
        let capture = launch.scenarios.prepare_evidence(&access).unwrap();
        let expected: std::collections::BTreeSet<_> = access
            .readable_scopes()
            .into_iter()
            .map(|scope| scope.to_string())
            .collect();
        assert_eq!(capture.scopes, expected);
        assert!(matches!(
            launch.scenarios.finish_evidence(capture, false),
            EvidenceStep::Capture {
                screenshot: true,
                image_timed_out: true,
                ..
            }
        ));
        assert!(!launch.scenarios.has_evidence());
        launch.scenarios.queue_evidence("all,screenshot".into());
        let capture = launch.scenarios.prepare_evidence(&access).unwrap();
        assert!(matches!(
            launch.scenarios.finish_evidence(capture, false),
            EvidenceStep::Capture {
                image_timed_out: true,
                ..
            }
        ));
    }

    #[test]
    fn screenshot_permission_is_checked_before_waiting() {
        let access = crate::control::ControlAccess::new();
        let mut launch = launch(&[("QUANTICK_CONTROL_EVIDENCE", "all,screenshot")]);
        let capture = launch.scenarios.prepare_evidence(&access).unwrap();
        assert!(capture.screenshot_not_granted);
        assert!(!capture.wants_screenshot);
        assert!(matches!(
            launch.scenarios.finish_evidence(capture, false),
            EvidenceStep::Capture {
                screenshot: false,
                image_timed_out: false,
                ..
            }
        ));
        assert_eq!(launch.scenarios.evidence_frames, 0);
    }

    #[test]
    fn the_evidence_hook_waits_its_budget_and_then_stops() {
        let mut scenarios = super::ControlScenarios::default();
        for frame in 1..=super::CONTROL_EVIDENCE_HOOK_FRAMES {
            assert!(
                scenarios.evidence_frame_waited(),
                "frame {frame} is within the budget"
            );
        }
        assert!(
            !scenarios.evidence_frame_waited(),
            "the window never delivered a frame to rasterise"
        );
    }
}
