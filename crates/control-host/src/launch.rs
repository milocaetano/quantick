//! The control plane's opt-in launch scenarios: a mark, an annotation, a
//! notification or an evidence capture a scripted run asks for at boot.
//!
//! The application reads the `QUANTICK_CONTROL_*` hooks once, at launch, and
//! hands the raw values here; what remains is pure state — which scenario is
//! still pending, how a request string becomes a capability call, and how
//! long an evidence capture waits for a screenshot — so it is tested here
//! without building a window.

use std::collections::BTreeSet;

use serde_json::{Value, json};

/// The same bounded wait as the original harness: wait attempts 1..=120, then
/// capture honest missing-image coverage on attempt 121.
pub const CONTROL_EVIDENCE_HOOK_FRAMES: u32 = 120;

/// The launch scenarios still pending, one of each kind.
#[derive(Debug, Default)]
pub struct LaunchScenarios {
    pending_control_access_enable: bool,
    pending_control_annotation: Option<String>,
    pending_control_notification: Option<String>,
    pending_control_evidence: Option<String>,
    pending_control_mark: Option<String>,
    /// Lifetime-wide screenshot wait count, never reset by request rearming.
    evidence_frames: u32,
}

/// What the launch hooks asked for, as read: `None` where a hook was unset.
#[derive(Debug, Default)]
pub struct LaunchRequests {
    pub enable_access: bool,
    pub annotation: Option<String>,
    pub notification: Option<String>,
    pub evidence: Option<String>,
    pub mark: Option<String>,
}

/// One evidence capture being prepared: the scopes it reads and whether it
/// still wants the screenshot the grant allows.
#[derive(Debug)]
pub struct EvidenceCapture {
    request: String,
    pub scopes: BTreeSet<String>,
    pub wants_screenshot: bool,
    pub screenshot_not_granted: bool,
}

/// What an evidence capture does this frame.
#[derive(Debug)]
pub enum EvidenceStep {
    /// The screenshot has not arrived yet and the wait budget is not spent.
    Waiting,
    /// Capture now, labelling a screenshot that never came as timed out.
    Capture {
        scopes: BTreeSet<String>,
        screenshot: bool,
        image_timed_out: bool,
    },
}

/// A notification request resolved to the capability that carries it, or
/// refused because it names no channel.
#[derive(Debug)]
pub enum NotificationStep {
    Ready {
        capability: &'static str,
        input: Value,
    },
    Refused {
        channel: String,
    },
}

/// Test support, published on purpose: what is still pending, read by the
/// application's launch tests to assert a hook was consumed exactly once.
#[derive(Debug, PartialEq, Eq)]
pub struct PendingScenarios<'a> {
    pub enable: bool,
    pub mark: Option<&'a str>,
    pub annotation: Option<&'a str>,
    pub notification: Option<&'a str>,
    pub evidence: Option<&'a str>,
}

impl LaunchScenarios {
    /// The scenarios the launch hooks asked for.
    #[must_use]
    pub fn new(requests: LaunchRequests) -> Self {
        Self {
            pending_control_access_enable: requests.enable_access,
            pending_control_annotation: requests.annotation,
            pending_control_notification: requests.notification,
            pending_control_evidence: requests.evidence,
            pending_control_mark: requests.mark,
            evidence_frames: 0,
        }
    }

    pub fn take_enable(&mut self) -> bool {
        std::mem::take(&mut self.pending_control_access_enable)
    }

    pub fn take_mark(&mut self) -> Option<String> {
        self.pending_control_mark.take()
    }

    #[must_use]
    pub fn has_annotation(&self) -> bool {
        self.pending_control_annotation.is_some()
    }

    #[must_use]
    pub fn has_evidence(&self) -> bool {
        self.pending_control_evidence.is_some()
    }

    /// The annotation's input once the chart has an anchor to hang it on;
    /// until then the text stays pending.
    pub fn annotation(&mut self, anchor: Option<Value>) -> Option<Value> {
        let anchor = anchor?;
        let text = self.pending_control_annotation.take()?;
        Some(json!({ "anchors": [anchor], "text": text }))
    }

    /// `channel:message`, or a bare message for the toast.
    pub fn notification(&mut self) -> Option<NotificationStep> {
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
            input: json!({ "message": message, "title": "From your assistant" }),
        })
    }

    /// Take the pending evidence request and expand it against the grants in
    /// force now: `readable_scopes` is asked only when the request says `all`
    /// or names nothing, and a screenshot the grant withholds is dropped and
    /// reported rather than waited for.
    pub fn prepare_evidence(
        &mut self,
        readable_scopes: impl Fn() -> Vec<String>,
        grants_screenshot: bool,
    ) -> Option<EvidenceCapture> {
        let request = self.pending_control_evidence.take()?;
        let mut wants_screenshot = false;
        let mut scopes = BTreeSet::new();
        for token in request
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            match token {
                "screenshot" => wants_screenshot = true,
                "all" | "1" => scopes.extend(readable_scopes()),
                scope => {
                    scopes.insert(scope.to_owned());
                }
            }
        }
        if scopes.is_empty() {
            scopes.extend(readable_scopes());
        }
        let screenshot_not_granted = wants_screenshot && !grants_screenshot;
        wants_screenshot &= !screenshot_not_granted;
        Some(EvidenceCapture {
            request,
            scopes,
            wants_screenshot,
            screenshot_not_granted,
        })
    }

    /// Capture, or wait one more frame for the screenshot while the budget
    /// lasts; the request is re-armed when it waits.
    pub fn finish_evidence(
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

    /// Spend one frame of the lifetime wait budget; `false` once it is gone.
    pub fn evidence_frame_waited(&mut self) -> bool {
        self.evidence_frames = self.evidence_frames.saturating_add(1);
        self.evidence_frames <= CONTROL_EVIDENCE_HOOK_FRAMES
    }

    /// Test support: how many frames the evidence capture has waited.
    #[must_use]
    pub fn evidence_frames(&self) -> u32 {
        self.evidence_frames
    }

    /// Test support: what is still pending.
    #[must_use]
    pub fn pending(&self) -> PendingScenarios<'_> {
        PendingScenarios {
            enable: self.pending_control_access_enable,
            mark: self.pending_control_mark.as_deref(),
            annotation: self.pending_control_annotation.as_deref(),
            notification: self.pending_control_notification.as_deref(),
            evidence: self.pending_control_evidence.as_deref(),
        }
    }

    /// Test support: queue an annotation as the hook would have.
    pub fn queue_annotation(&mut self, text: String) {
        self.pending_control_annotation = Some(text);
    }

    /// Test support: queue a notification as the hook would have.
    pub fn queue_notification(&mut self, request: String) {
        self.pending_control_notification = Some(request);
    }

    /// Test support: queue a mark as the hook would have.
    pub fn queue_mark(&mut self, note: String) {
        self.pending_control_mark = Some(note);
    }

    /// Test support: queue an evidence capture as the hook would have.
    pub fn queue_evidence(&mut self, request: String) {
        self.pending_control_evidence = Some(request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scopes(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn enable_and_mark_are_consumed_once() {
        let mut launch = LaunchScenarios::new(LaunchRequests {
            enable_access: true,
            mark: Some(" 1 ".to_owned()),
            ..LaunchRequests::default()
        });
        assert!(launch.take_enable());
        assert!(!launch.take_enable());
        assert_eq!(launch.take_mark().as_deref(), Some(" 1 "));
        assert_eq!(launch.take_mark(), None);
    }

    #[test]
    fn a_bare_notification_goes_to_the_toast() {
        let mut launch = LaunchScenarios::default();
        launch.queue_notification("hello".to_owned());
        let Some(NotificationStep::Ready { capability, input }) = launch.notification() else {
            panic!("a bare message must be a toast");
        };
        assert_eq!(capability, "notify.toast");
        assert_eq!(input["message"], "hello");
    }

    #[test]
    fn an_explicit_scope_list_never_asks_for_the_readable_scopes() {
        let mut launch = LaunchScenarios::default();
        launch.queue_evidence("observe.chart".to_owned());
        let capture = launch
            .prepare_evidence(
                || panic!("an explicit scope list needs no expansion"),
                false,
            )
            .unwrap();
        assert_eq!(capture.scopes, BTreeSet::from(["observe.chart".to_owned()]));
        assert!(matches!(
            launch.finish_evidence(capture, false),
            EvidenceStep::Capture {
                screenshot: false,
                image_timed_out: false,
                ..
            }
        ));
    }

    #[test]
    fn an_empty_request_reads_every_readable_scope() {
        let mut launch = LaunchScenarios::default();
        launch.queue_evidence(" , ".to_owned());
        let capture = launch
            .prepare_evidence(|| scopes(&["observe.chart", "observe.feed"]), true)
            .unwrap();
        assert_eq!(capture.scopes.len(), 2);
    }

    #[test]
    fn the_evidence_hook_waits_its_budget_and_then_stops() {
        let mut scenarios = LaunchScenarios::default();
        for frame in 1..=CONTROL_EVIDENCE_HOOK_FRAMES {
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
