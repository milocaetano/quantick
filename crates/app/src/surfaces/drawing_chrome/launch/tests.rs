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
        !chrome.manager.open && !chrome.shared.open && !chrome.state().scenario.note_requested()
    );
    chrome.apply_launch();
    assert!(chrome.manager.open && chrome.shared.open && chrome.state().scenario.note_requested());
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
        !chrome.manager.open && !chrome.shared.open && !chrome.state().scenario.note_requested()
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
