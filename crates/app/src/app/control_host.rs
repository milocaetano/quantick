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

impl QuantickApp {
    /// The active tab beside the config it reads.
    ///
    /// Split here, once, because almost every tab operation needs both and
    /// `self.tabs[i].f(&self.config)` is a borrow error at every call site.
    pub(super) fn active_with_config(&mut self) -> (&mut Tab, &AppConfig) {
        (&mut self.tabs[self.active_tab], &self.config)
    }

    /// The tab on screen.
    pub(super) fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// See [`Self::active_tab`].
    pub(super) fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
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
        self.tabs.get_mut(self.active_tab).map(|tab| &mut tab.paper)
    }

    /// The read side of [`Self::control_active_paper_mut`], resolved the same
    /// way so a call and its read-back can never name different tabs.
    pub(crate) fn control_active_paper(&self) -> Option<&crate::paper_trading::PaperTrading> {
        self.tabs.get(self.active_tab).map(|tab| &tab.paper)
    }

    pub(crate) fn control_tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub(crate) fn control_active_tab_index(&self) -> usize {
        self.active_tab
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
        self.alerts
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
        self.tabs[tab_index].pane_mut(side)
    }

    /// What a freshly placed object of `tool` opens with, through the same
    /// door the click path uses — saved defaults, named preset and all.
    pub(crate) fn control_new_drawing(&self, tool: drawings::DrawingTool) -> drawings::NewDrawing {
        drawings::new_drawing_from_defaults(&self.drawing_presets, tool)
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
        self.feed_chip_rect
    }

    /// Whether the recovery popup that chip opens is showing, on the chart
    /// the trader is looking at.
    pub(crate) fn control_feed_popup_open(&self) -> bool {
        self.feed_popup_tab == Some(self.active_tab().id)
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
            self.show_perf,
            self.progressive_history,
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
        self.history_reach = reach;
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
        let ceiling = (history_reach::MAX_CAMPAIGN_SPAN_MS / 60_000) as u32;
        self.history_reach_span_minutes = minutes.clamp(1, ceiling);
    }

    /// What that span is now, for an operator reading back what it set.
    pub(crate) fn control_history_reach_span_minutes(&self) -> u32 {
        self.history_reach_span_minutes
    }

    /// How far the window's *load older* press reaches, and whether a chart
    /// cut by trades carries the venue's candles.
    ///
    /// Both are choices an operator without a mouse has to be able to read
    /// back after setting them — the reach especially, since it decides
    /// whether one press is one request or a run of them.
    pub(crate) fn control_history_settings(&self) -> (history_reach::HistoryReach, bool) {
        (self.history_reach, self.venue_lead_in)
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
        let Some(mut access) = self.control_access.take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        let outcome =
            access.invoke_local_action(self, capability_id, capability_version, input, origin);
        self.control_access = Some(access);
        outcome
    }

    /// How many objects an operator other than the trader placed, across
    /// every pane one can reach — an assistant may annotate any open tab, so
    /// counting the active pane alone would offer to take back a subset and
    /// call it all of them.
    pub(super) fn authored_object_count(tabs: &[Tab]) -> usize {
        tabs.iter()
            .map(|tab| {
                tab.panes()
                    .map(|(pane, _side)| pane.drawings.authored_count())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Take back every object an operator placed, wherever it is. One undo
    /// entry per pane, and the resting orders of any armed strategy go with
    /// the objects they were anchored to.
    pub(super) fn remove_every_authored_object(&mut self) -> usize {
        let mut removed = 0;
        for tab in &mut self.tabs {
            // Every pane the tab holds, not the two it used to. "Remove
            // objects placed for you" promises to take them *all* back, and a
            // sweep that skipped the second stacked chart would leave an
            // assistant's marks behind while reporting the job done.
            for pane in tab.panes_mut() {
                let taken = pane.drawings.remove_authored();
                if taken > 0 {
                    pane.sweep_strategy_orphans();
                    removed += taken;
                }
            }
        }
        removed
    }

    /// The annotate tier's launch hooks: one agent-authored label, one
    /// notification. Both go through the registered action with an agent
    /// actor — the same path the gateway takes for a remote client — so what
    /// a screenshot shows is what a real assistant would have produced.
    pub(super) fn apply_control_annotate_hooks(&mut self) {
        if let Some(text) = self.pending_control_annotation.take() {
            let anchor = {
                let pane = self.active_tab().drawing_pane();
                let slot = pane.slots().saturating_sub(1);
                match (pane.slot_open_time(slot), pane.closed_bar(slot)) {
                    (Some(time), Some(bar)) => Some(serde_json::json!({
                        "time_unix_ms": time,
                        "price": rust_decimal::prelude::ToPrimitive::to_f64(&bar.close)
                            .unwrap_or(1.0)
                            .to_string(),
                    })),
                    // No bars yet: put the hook back and take it next frame,
                    // rather than annotating a chart that has nothing on it.
                    _ => {
                        self.pending_control_annotation = Some(text.clone());
                        None
                    }
                }
            };
            if let Some(anchor) = anchor {
                self.pending_control_annotation = None;
                self.run_hook_action(
                    "annotate.label.create",
                    serde_json::json!({ "anchors": [anchor], "text": text }),
                );
            }
        }
        if let Some(request) = self.pending_control_notification.take() {
            let (channel, message) = request
                .split_once(':')
                .unwrap_or(("toast", request.as_str()));
            let capability = match channel.trim() {
                "popup" => Some("notify.popup"),
                "sound" => Some("notify.sound"),
                "toast" => Some("notify.toast"),
                other => {
                    tracing::warn!(
                        target: "quantick::control",
                        event_code = "CONTROL_NOTIFY_HOOK_REFUSED",
                        channel = other,
                        "QUANTICK_CONTROL_NOTIFY names no notification channel"
                    );
                    None
                }
            };
            if let Some(capability) = capability {
                self.run_hook_action(
                    capability,
                    serde_json::json!({ "message": message, "title": "From your assistant" }),
                );
            }
        }
    }

    /// Invoke one registered action as an *agent* would, from inside this
    /// window. The hooks use it so a screenshot shows a real assistant's
    /// object, attribution and all, without a client on the socket.
    pub(super) fn run_agent_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.control_access.take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        // No identity, no actor to sign with: the same structured refusal an
        // action gets, rather than a panic on the first frame.
        let Some(actor) = access.hook_agent_actor() else {
            self.control_access = Some(access);
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
        self.control_access = Some(access);
        outcome
    }

    /// A launch hook's action, with its failure reported where a scripted run
    /// will see it: the hook is fire-and-forget, so nothing else would.
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
            wall_average_ms: self.frames.avg_ms(),
            wall_worst_ms: self.frames.worst_ms(),
            frames_per_second: self.frames.fps(),
            cpu_average_ms: self.cpu_frames.avg_ms(),
            cpu_worst_ms: self.cpu_frames.worst_ms(),
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
    pub(super) fn drawing_pane(&self) -> &ChartPane {
        self.active_tab().drawing_pane()
    }

    /// See [`Self::drawing_pane`].
    pub(super) fn drawing_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().drawing_pane_mut()
    }
}
