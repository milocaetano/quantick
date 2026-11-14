//! Opt-in drawing-chrome launch scenarios and the note placement policy.
//! Normal editing stays on the ordinary owner; only scripted requests live here.
#![cfg(any(feature = "drawing-harness", test))]

use std::ffi::OsString;

use eframe::egui;

use super::{Chrome, InspectorTab};
use quantick_ui_model::drawing_chrome::scenario;
use scenario::{DrawingDraft, DrawingsDemo, FrvpDemo};

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
}
impl Chrome<'_> {
    pub fn queue_launch(&mut self, input: DrawingChromeLaunch) {
        self.view.rejected_tab = input.rejected_tab;
        self.emit(super::Intent::Scenario(scenario::Intent::QueueLaunch(
            scenario::Launch {
                demos: input.demos.into(),
                manager: input.manager,
                inspector: input.inspector,
                note: input.note,
                tab: input.tab,
                inspector_position: input.inspector_position.map(|p| [p.x, p.y]),
                bar_position: input.bar_position.map(|p| [p.x, p.y]),
            },
        )));
    }
    pub fn apply_launch(&mut self) {
        let mut effects = self.emit(super::Intent::Scenario(scenario::Intent::ApplyLaunch));
        if let Some(scenario::Effect::Launch {
            inspector_position,
            bar_position,
        }) = effects.take_scenario()
        {
            if inspector_position.is_some() {
                self.view.inspector.max_width = None;
            }
            if let Some([x, y]) = bar_position {
                self.view.bar.bar.set_manual(egui::pos2(x, y));
            }
        }
        if let Some(tab) = self.view.rejected_tab.take() {
            tracing::warn!(tab, "unknown drawing inspector tab");
        }
    }
}
/// Binds the model's note plan to the host's timestamp vocabulary.
pub(crate) struct NotePlacement;
impl NotePlacement {
    pub fn point(plan: scenario::NotePlacement, time: Option<i64>) -> crate::drawings::ChartPoint {
        crate::drawings::ChartPoint::at_time(plan.slot as f32 + 0.5, plan.price, time)
    }
    pub fn tool() -> Option<crate::drawings::DrawingTool> {
        crate::drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
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
        let mut owner = super::super::new_owner();
        let mut chrome = super::super::borrow(&mut owner);
        chrome.queue_launch(input);
        assert!(
            !chrome.manager.open
                && !chrome.shared.open
                && !chrome.state().scenario.note_requested()
        );
        chrome.apply_launch();
        assert!(
            chrome.manager.open && chrome.shared.open && chrome.state().scenario.note_requested()
        );
        assert!(matches!(
            chrome.state().inspector.tab,
            InspectorTab::Coordinates
        ));
        assert_eq!(
            chrome.state().inspector.remembered_position(),
            Some([-4.0, 200.5])
        );
        assert_eq!(
            chrome.view.bar.bar.manual_position(),
            Some(egui::pos2(0.0, -8.0))
        );
        chrome.set_manager_open(false);
        chrome.emit(super::super::Intent::InspectorOpen(false));
        chrome.note_text_note_placed();
        chrome.apply_launch();
        assert!(
            !chrome.manager.open
                && !chrome.shared.open
                && !chrome.state().scenario.note_requested()
        );
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
            let mut owner = super::super::new_owner();
            let mut chrome = super::super::borrow(&mut owner);
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
    fn note_timestamp_binding_preserves_the_selected_slot() {
        assert_eq!(
            NotePlacement::point(
                scenario::NotePlacement {
                    slot: 55,
                    price: 1.0
                },
                Some(123)
            ),
            crate::drawings::ChartPoint::at_time(55.5, 1.0, Some(123))
        );
    }
}

#[derive(Default)]
pub(crate) struct DrawingDemoLaunch {
    pub(super) gallery: Option<DrawingsDemo>,
    pub(super) recut: bool,
    pub(super) profile: Option<FrvpDemo>,
    pub(super) avwap: bool,
    pub(super) draft: Option<DrawingDraft>,
}
impl DrawingDemoLaunch {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut read = |name| lookup(name).and_then(|value| value.into_string().ok());
        let gallery = read("QUANTICK_DRAWINGS_DEMO")
            .filter(|value| matches!(value.as_str(), "1" | "bands"))
            .map(|value| DrawingsDemo {
                bands: value == "bands",
                shared: read("QUANTICK_DRAWINGS_DEMO_SHARED")
                    .filter(|value| value == "0" || value == "1")
                    .map(|value| value == "1"),
                select_tool: read("QUANTICK_DRAWINGS_DEMO_SELECT"),
            });
        let recut = read("QUANTICK_DRAWINGS_DEMO_RECUT").is_some_and(|value| value == "1");
        let profile = read("QUANTICK_FRVP_DEMO")
            .filter(|value| matches!(value.trim(), "1" | "compare" | "stress"))
            .map(|value| FrvpDemo {
                compare: value.trim() == "compare",
                stress: value.trim() == "stress",
                select: read("QUANTICK_FRVP_DEMO_SELECT").is_some_and(|value| value.trim() == "1"),
            });
        let avwap = read("QUANTICK_AVWAP_DEMO").is_some_and(|value| value.trim() == "1");
        let draft = read("QUANTICK_DRAWING_DRAFT")
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|anchors| *anchors > 0)
            .map(|anchors| DrawingDraft {
                anchors,
                constrain: read("QUANTICK_DRAWING_CONSTRAIN").is_some_and(|value| value == "1"),
            });
        Self {
            gallery,
            recut,
            profile,
            avwap,
            draft,
        }
    }
}

impl From<DrawingDemoLaunch> for scenario::DrawingDemoLaunch {
    fn from(input: DrawingDemoLaunch) -> Self {
        Self {
            gallery: input.gallery,
            recut: input.recut,
            profile: input.profile,
            avwap: input.avwap,
            draft: input.draft,
        }
    }
}
