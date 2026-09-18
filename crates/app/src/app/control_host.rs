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

use super::arrangement_host::ArrangementHost;
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

/// Read-only application roots available to the on-demand control
/// projections, borrowed once. The gateway never receives `QuantickApp`; it
/// receives the owned DTOs built from this narrow view. Every accessor takes
/// the view by value and hands back the root's own borrow, so a projection
/// keeps what it read for as long as it holds the window.
#[derive(Clone, Copy)]
pub(crate) struct ControlReads<'a> {
    pub(super) tabs: &'a ArrangementHost,
    pub(super) config: &'a AppConfig,
    pub(super) footprint_config: &'a crate::footprint_config::FootprintConfig,
    pub(super) style: &'a ChartStyle,
    pub(super) toolrail: &'a ToolRail,
    pub(super) chrome: &'a super::chrome::ChromeState,
    pub(super) dock: &'a Dock,
    pub(super) tz: TzOffset,
    pub(super) presets: &'a drawings::presets::PresetStore,
    pub(super) workspace: &'a crate::workspace_store::WorkspaceStore,
    pub(super) health: &'a super::health::HealthCounters,
    pub(super) history: &'a super::tabs::HistorySettings,
    pub(super) replay_view: &'a crate::replay_view::ReplayView,
}

impl<'a> ControlReads<'a> {
    /// One tab by position, for a control capability that resolved an id.
    pub(crate) fn tab_at(self, index: usize) -> Option<&'a Tab> {
        self.tabs.get(index)
    }

    /// The read side of [`ControlActions::active_paper_mut`], resolved the
    /// same way so a call and its read-back can never name different tabs.
    pub(crate) fn active_paper(self) -> Option<&'a crate::paper_trading::PaperTrading> {
        self.tabs
            .get(self.tabs.active_index())
            .map(|tab| &tab.paper)
    }

    pub(crate) fn tabs(self) -> &'a ArrangementHost {
        self.tabs
    }

    pub(crate) fn active_tab_index(self) -> usize {
        self.tabs.active_index()
    }

    /// What a freshly placed object of `tool` opens with, through the same
    /// door the click path uses — saved defaults, named preset and all.
    pub(crate) fn new_drawing(self, tool: drawings::DrawingTool) -> drawings::NewDrawing {
        drawings::new_drawing_from_defaults(self.presets, tool)
    }

    pub(crate) fn config(self) -> &'a AppConfig {
        self.config
    }

    /// The window's footprint setup — the one a pane falls back to when it
    /// carries no override of its own.
    pub(crate) fn footprint_config(self) -> &'a crate::footprint_config::FootprintConfig {
        self.footprint_config
    }

    /// The window's shared chart style, which owns the layers no pane does.
    pub(crate) fn style(self) -> &'a ChartStyle {
        self.style
    }

    /// The drawing tool rail: which tool is armed, and whether it is on
    /// screen at all.
    pub(crate) fn tool_rail(self) -> &'a ToolRail {
        self.toolrail
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
    pub(crate) fn feed_offline_accent(
        self,
        stall: Option<&quantick_feed::stall::Stall>,
    ) -> Option<egui::Color32> {
        feed_notice::report(&self.tabs[self.tabs.active_index()].notice, stall)
            .filter(feed_notice::Report::is_offline)
            .map(|report| report.accent())
    }

    /// Where the feed's offline chip was painted, or `None` when it was not.
    ///
    /// The projection reads what was drawn rather than re-deciding it, so the
    /// scene and the screen cannot disagree across the edge of a stall budget.
    pub(crate) fn feed_chip_rect(self) -> Option<egui::Rect> {
        self.chrome.feed_chip_rect
    }

    /// Whether the recovery popup that chip opens is showing, on the chart
    /// the trader is looking at.
    pub(crate) fn feed_popup_open(self) -> bool {
        self.chrome.feed_popup_tab == Some(self.tabs.active_id())
    }

    /// The right-hand dock: whether it is shown, and which tab is open.
    pub(crate) fn dock(self) -> &'a Dock {
        self.dock
    }

    pub(crate) fn timezone(self) -> TzOffset {
        self.tz
    }

    pub(crate) fn workspace_flags(self) -> (bool, bool, bool) {
        (
            self.workspace.session().save_on_exit(),
            self.health.show_perf,
            self.history.progressive_history,
        )
    }

    /// What the `by time` reach's span is now, for an operator reading back
    /// what it set.
    pub(crate) fn history_reach_span_minutes(self) -> u32 {
        self.history.history_reach_span_minutes
    }

    /// How far the window's *load older* press reaches, and whether a chart
    /// cut by trades carries the venue's candles.
    ///
    /// Both are choices an operator without a mouse has to be able to read
    /// back after setting them — the reach especially, since it decides
    /// whether one press is one request or a run of them.
    pub(crate) fn history_settings(self) -> (history_reach::HistoryReach, bool) {
        (self.history.history_reach, self.history.venue_lead_in)
    }

    /// Whether a recording opens with the session day before it joined in
    /// front, and a download fetches that day's tape too.
    ///
    /// A choice an operator without a mouse has to be able to read back after
    /// setting it: it decides what a replay they are about to open will hold.
    pub(crate) fn replay_day_before(self) -> bool {
        self.replay_view.day_before()
    }

    pub(crate) fn frame_metrics(self) -> ControlFrameMetrics {
        ControlFrameMetrics {
            wall_average_ms: self.health.frames.avg_ms(),
            wall_worst_ms: self.health.frames.worst_ms(),
            frames_per_second: self.health.frames.fps(),
            cpu_average_ms: self.health.cpu_frames.avg_ms(),
            cpu_worst_ms: self.health.cpu_frames.worst_ms(),
        }
    }
}

/// The mutable half of [`ControlReads`], for the cockpit tier: the tabs an
/// action changes and the three lanes the assistant answers on.
///
/// Narrow on purpose: the layout capabilities need to *change* a tab, and
/// handing them the whole application would let a later one reach past the
/// canvas into the feed or the simulator. Each accessor consumes the view
/// and hands back one borrow, so an action names its target once.
pub(crate) struct ControlActions<'a> {
    pub(super) tabs: &'a mut ArrangementHost,
    pub(super) config: &'a AppConfig,
    pub(super) agent_popup: &'a mut crate::surfaces::AgentPopupSurface,
    pub(super) toast: &'a mut crate::surfaces::ToastSurface,
    pub(super) audio: &'a mut super::replay_and_history::AlertState,
}

impl<'a> ControlActions<'a> {
    /// The mutable twin of [`ControlReads::tab_at`].
    pub(crate) fn tab_at_mut(self, index: usize) -> Option<&'a mut Tab> {
        self.tabs.get_mut(index)
    }

    /// One tab beside the configuration it reads, by position.
    ///
    /// [`QuantickApp::active_with_config`] for a tab that is not necessarily
    /// the active one — a capability names the tab it acts on, and respawning
    /// a feed needs the feed table the same way a click in the corner does.
    pub(crate) fn tab_with_config(self, index: usize) -> Option<(&'a mut Tab, &'a AppConfig)> {
        let config = self.config;
        self.tabs.get_mut(index).map(|tab| (tab, config))
    }

    /// The trading host of the tab on screen — where the `trade.*` actions
    /// land. The active tab and not an addressed one: an order belongs to
    /// the symbol the trader is looking at, and a call that could quietly
    /// trade a chart nobody has open is a call nobody should be able to
    /// make.
    pub(crate) fn active_paper_mut(self) -> Option<&'a mut crate::paper_trading::PaperTrading> {
        // Fallible, because the rest of the control code does not trust the
        // invariant either: `annotate::resolve_target` guards an empty tab
        // list and clamps the index, and two more sites clamp it. A
        // `trade.*` call must answer "this window has no chart open" rather
        // than panic the whole trading application, and it must resolve the
        // *same* tab its own read-back resolves.
        let active = self.tabs.active_index();
        self.tabs.get_mut(active).map(|tab| &mut tab.paper)
    }

    /// One pane, by tab position and side — the mutable half of
    /// [`ControlReads::tabs`], for the actions that place objects.
    pub(crate) fn pane_mut(
        self,
        tab_index: usize,
        side: crate::pane::PaneSide,
    ) -> &'a mut ChartPane {
        self.tabs.runtime_mut(tab_index).pane_mut(side)
    }

    /// Open the assistant's popup. One at a time: a second message replaces
    /// the first rather than stacking windows over a chart someone is
    /// trading, and the trader dismisses it.
    pub(crate) fn show_popup(self, popup: crate::control::AgentPopup) {
        self.agent_popup.show(popup);
    }

    /// Post one line to the window's own acknowledgement lane — the same
    /// channel a delete or a workspace save uses, with no Undo: there is
    /// nothing to take back from having been told something.
    pub(crate) fn show_toast(self, message: String) {
        self.toast.note(message, Instant::now());
    }

    /// Ask for the platform's attention sound, through the same sink the
    /// alarms use, and report honestly when it could not be made rather
    /// than letting a client believe it was heard.
    pub(crate) fn sound_alert(self) -> Option<String> {
        self.audio.play(&[crate::audio::Cue::default()])
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
