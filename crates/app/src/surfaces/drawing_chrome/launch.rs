//! Opt-in drawing-chrome launch scenarios and the note placement policy.
//! Normal editing stays on the ordinary owner; only scripted requests live here.

use std::ffi::OsString;

use eframe::egui;

use super::{DrawingChromeSurface, InspectorTab};
use crate::drawings::ChartPoint;

#[derive(Default)]
pub(crate) struct DrawingChromeLaunch {
    demos: super::demo::DrawingDemoLaunch,
    manager: bool,
    inspector: bool,
    note: bool,
    tab: Option<InspectorTab>,
    rejected_tab: Option<String>,
    inspector_position: Option<egui::Pos2>,
    bar_position: Option<egui::Pos2>,
}

impl DrawingChromeLaunch {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut read = |name| lookup(name).and_then(|value| value.into_string().ok());
        let manager = read("QUANTICK_DRAWINGS_MANAGER").is_some_and(|value| value == "1");
        let inspector = read("QUANTICK_DRAWING_INSPECTOR").is_some_and(|value| value == "1");
        let note = read("QUANTICK_TEXT_NOTE").is_some_and(|value| value == "1");
        let (tab, rejected_tab) = match read("QUANTICK_DRAWING_INSPECTOR_TAB") {
            None => (None, None),
            Some(value) => match value.trim() {
                "style" => (Some(InspectorTab::Style), None),
                "extra" => (Some(InspectorTab::Extra), None),
                "coordinates" => (Some(InspectorTab::Coordinates), None),
                other => (None, Some(other.to_owned())),
            },
        };
        let inspector_position =
            read("QUANTICK_DRAWING_INSPECTOR_POS").and_then(|value| parse_point(&value));
        let bar_position = read("QUANTICK_CONTEXT_BAR_POS").and_then(|value| parse_point(&value));
        Self {
            demos: super::demo::DrawingDemoLaunch::capture(lookup),
            manager,
            inspector,
            note,
            tab,
            rejected_tab,
            inspector_position,
            bar_position,
        }
    }

    fn apply(self, chrome: &mut DrawingChromeSurface) {
        chrome.scenario.demos.install(self.demos);
        if self.manager {
            chrome.manager.open = true;
        }
        if self.inspector {
            chrome.shared.open = true;
        }
        if self.note {
            chrome.scenario.request_note(true);
        }
        if let Some(tab) = self.tab {
            chrome.inspector.tab = tab;
        }
        if let Some(tab) = self.rejected_tab {
            tracing::warn!(tab, "unknown drawing inspector tab");
        }
        if let Some(position) = self.inspector_position {
            chrome.inspector.place_by_hand(position);
        }
        if let Some(position) = self.bar_position {
            chrome.bar.bar.set_manual(position);
        }
    }
}

#[derive(Default)]
pub(crate) struct DrawingScenario {
    demos: super::demo::DrawingDemoState,
    pending_launch: Option<DrawingChromeLaunch>,
    pending_note: bool,
}

impl DrawingScenario {
    pub fn request_note(&mut self, pending: bool) {
        self.pending_note = pending;
    }
    pub fn note_requested(&self) -> bool {
        self.pending_note
    }
    /// Placement success consumes the request even if no editor was opened.
    pub fn acknowledge_placement(&mut self, placed: bool) {
        if placed {
            self.pending_note = false;
        }
    }
}

impl DrawingChromeSurface {
    pub fn demos(&self) -> &super::demo::DrawingDemoState {
        &self.scenario.demos
    }
    pub fn demos_mut(&mut self) -> &mut super::demo::DrawingDemoState {
        &mut self.scenario.demos
    }
    pub fn queue_launch(&mut self, input: DrawingChromeLaunch) {
        self.scenario.pending_launch = Some(input);
    }

    pub fn apply_launch(&mut self) {
        if let Some(input) = self.scenario.pending_launch.take() {
            input.apply(self);
        }
    }
}

/// The note scenario's existing visible-window span, in chart slots.
const NOTE_VISIBLE_SLOTS: usize = 90;

pub(crate) struct NotePlacement {
    pub slot: usize,
    price: f64,
}

impl NotePlacement {
    /// The existing first text-capable tool, never a fallback substitute.
    pub fn tool() -> Option<crate::drawings::DrawingTool> {
        crate::drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
    }

    /// Preserve the existing arithmetic, including its permissive range test.
    /// The host projects the selected slot's timestamp after this decision.
    pub fn plan(
        chart_available: bool,
        slots: usize,
        close: Option<f64>,
        range: Option<(f64, f64)>,
    ) -> Option<Self> {
        if !chart_available || slots == 0 {
            return None;
        }
        let price = range
            .filter(|(lo, hi)| hi > lo)
            .map_or(close.unwrap_or(1.0), |(lo, hi)| (lo + hi) / 2.0);
        let visible = NOTE_VISIBLE_SLOTS.min(slots);
        let slot = (slots - visible / 2).min(slots.saturating_sub(1));
        Some(Self { slot, price })
    }

    pub fn point(&self, time: Option<i64>) -> ChartPoint {
        ChartPoint::at_time(self.slot as f32 + 0.5, self.price, time)
    }
}

/// Screen points: malformed or nonfinite input leaves automatic placement.
fn parse_point(raw: &str) -> Option<egui::Pos2> {
    let (x, y) = raw.split_once(',')?;
    let x: f32 = x.trim().parse().ok()?;
    let y: f32 = y.trim().parse().ok()?;
    (x.is_finite() && y.is_finite()).then_some(egui::pos2(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_values_are_captured_once_and_applied_at_the_owner_phase() {
        let mut reads = Vec::new();
        let input = DrawingChromeLaunch::capture(|name| {
            reads.push(name.to_owned());
            Some(
                match name {
                    "QUANTICK_DRAWING_INSPECTOR_TAB" => " coordinates ",
                    "QUANTICK_DRAWING_INSPECTOR_POS" => " -4, 200.5 ",
                    "QUANTICK_CONTEXT_BAR_POS" => "0,-8",
                    _ => "1",
                }
                .into(),
            )
        });
        assert_eq!(
            reads,
            [
                "QUANTICK_DRAWINGS_MANAGER",
                "QUANTICK_DRAWING_INSPECTOR",
                "QUANTICK_TEXT_NOTE",
                "QUANTICK_DRAWING_INSPECTOR_TAB",
                "QUANTICK_DRAWING_INSPECTOR_POS",
                "QUANTICK_CONTEXT_BAR_POS",
                "QUANTICK_DRAWINGS_DEMO",
                "QUANTICK_DRAWINGS_DEMO_SHARED",
                "QUANTICK_DRAWINGS_DEMO_SELECT",
                "QUANTICK_DRAWINGS_DEMO_RECUT",
                "QUANTICK_FRVP_DEMO",
                "QUANTICK_FRVP_DEMO_SELECT",
                "QUANTICK_AVWAP_DEMO",
                "QUANTICK_DRAWING_DRAFT",
                "QUANTICK_DRAWING_CONSTRAIN"
            ]
        );
        let mut chrome = DrawingChromeSurface::default();
        chrome.queue_launch(input);
        assert!(!chrome.manager.open && !chrome.shared.open && !chrome.scenario.note_requested());
        chrome.apply_launch();
        assert!(chrome.manager.open && chrome.shared.open && chrome.scenario.note_requested());
        assert!(matches!(chrome.inspector.tab, InspectorTab::Coordinates));
        assert_eq!(chrome.inspector.remembered_position(), Some([-4.0, 200.5]));
        assert_eq!(
            chrome.bar.bar.manual_position(),
            Some(egui::pos2(0.0, -8.0))
        );
        chrome.manager.open = false;
        chrome.shared.open = false;
        chrome.note_text_note_placed();
        chrome.apply_launch();
        assert!(!chrome.manager.open && !chrome.shared.open && !chrome.scenario.note_requested());
    }

    #[test]
    fn grammar_and_deferred_diagnostics_preserve_existing_values() {
        for raw in ["", "0", "true", " 1 "] {
            let input = DrawingChromeLaunch::capture(|_| Some(raw.into()));
            assert!(!input.manager && !input.inspector && !input.note);
            assert!(input.tab.is_none());
            assert_eq!(input.rejected_tab.as_deref(), Some(raw.trim()));
        }
        for (raw, expected) in [
            ("style", InspectorTab::Style),
            (" extra ", InspectorTab::Extra),
            ("coordinates", InspectorTab::Coordinates),
        ] {
            let input = DrawingChromeLaunch::capture(|name| {
                (name == "QUANTICK_DRAWING_INSPECTOR_TAB").then(|| raw.into())
            });
            assert_eq!(input.tab, Some(expected));
            assert!(input.rejected_tab.is_none());
        }
        for raw in ["STYLE", " unknown ", "   "] {
            let input = DrawingChromeLaunch::capture(|name| {
                (name == "QUANTICK_DRAWING_INSPECTOR_TAB").then(|| raw.into())
            });
            assert!(input.tab.is_none());
            assert_eq!(input.rejected_tab.as_deref(), Some(raw.trim()));
        }
        let absent = DrawingChromeLaunch::capture(|_| None);
        assert!(!absent.manager && !absent.inspector && !absent.note);
        assert!(absent.tab.is_none() && absent.rejected_tab.is_none());
    }

    #[test]
    fn rejected_tab_warning_waits_for_apply_and_is_emitted_once() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};
        #[derive(Clone)]
        struct Log(Arc<Mutex<Vec<u8>>>);
        impl Write for Log {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let writer = Log(bytes.clone());
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let input = DrawingChromeLaunch::capture(|name| {
                (name == "QUANTICK_DRAWING_INSPECTOR_TAB").then(|| " unknown ".into())
            });
            let mut chrome = DrawingChromeSurface::default();
            chrome.queue_launch(input);
            assert!(
                bytes.lock().unwrap().is_empty(),
                "capture precedes tracing in production"
            );
            chrome.apply_launch();
            chrome.apply_launch();
        });
        let log = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert_eq!(log.matches("unknown drawing inspector tab").count(), 1);
        assert!(log.contains("unknown"));
    }

    #[cfg(windows)]
    #[test]
    fn non_unicode_inputs_are_absent_without_lossy_conversion() {
        use std::os::windows::ffi::OsStringExt;
        let input = DrawingChromeLaunch::capture(|_| Some(OsString::from_wide(&[0xD800])));
        assert!(!input.manager && !input.inspector && !input.note);
        assert!(input.tab.is_none() && input.rejected_tab.is_none());
        assert!(input.inspector_position.is_none() && input.bar_position.is_none());
    }

    /// Retained original position-parser assertions, with finite edge cases.
    #[test]
    fn the_popup_position_hook_refuses_what_is_not_a_point() {
        assert_eq!(parse_point("420,200"), Some(egui::pos2(420.0, 200.0)));
        assert_eq!(parse_point(" 420 , 200.5 "), Some(egui::pos2(420.0, 200.5)));
        assert_eq!(parse_point("-1,0"), Some(egui::pos2(-1.0, 0.0)));
        for raw in [
            "", "420", "420,", ",200", "left,top", "420x200", "1,2,3", "NaN,0", "0,inf",
        ] {
            assert_eq!(parse_point(raw), None, "{raw:?} is not a point");
        }
    }

    #[test]
    fn note_geometry_preserves_original_slot_range_and_fallback_law() {
        assert!(NotePlacement::plan(false, 100, Some(10.0), None).is_none());
        assert!(NotePlacement::plan(true, 0, Some(10.0), None).is_none());
        for (slots, slot) in [(1, 0), (2, 1), (89, 45), (90, 45), (100, 55)] {
            let plan = NotePlacement::plan(true, slots, None, None).unwrap();
            assert_eq!(plan.slot, slot);
            assert_eq!(plan.price, 1.0);
            assert_eq!(
                plan.point(Some(123)),
                ChartPoint::at_time(slot as f32 + 0.5, 1.0, Some(123))
            );
        }
        assert_eq!(
            NotePlacement::plan(true, 2, Some(7.0), Some((5.0, 3.0)))
                .unwrap()
                .price,
            7.0
        );
        assert_eq!(
            NotePlacement::plan(true, 2, Some(7.0), Some((2.0, 4.0)))
                .unwrap()
                .price,
            3.0
        );
        assert!(
            NotePlacement::plan(true, 2, None, Some((0.0, f64::INFINITY)))
                .unwrap()
                .price
                .is_infinite()
        );
        assert!(
            NotePlacement::plan(true, 2, None, Some((f64::NEG_INFINITY, f64::INFINITY)))
                .unwrap()
                .price
                .is_nan()
        );
    }

    #[test]
    fn placement_acknowledgement_depends_only_on_placed_not_editor_state() {
        let mut scenario = DrawingScenario::default();
        scenario.request_note(true);
        scenario.acknowledge_placement(false);
        assert!(scenario.note_requested());
        scenario.acknowledge_placement(true);
        assert!(!scenario.note_requested());
        scenario.acknowledge_placement(false);
        assert!(!scenario.note_requested());
    }
}
