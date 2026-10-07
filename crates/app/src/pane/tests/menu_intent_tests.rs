//! The menus' one apply site: an intent in, the pane's state out. No egui
//! frame runs here — the menus' clicks are covered where they are drawn; this
//! pins what each answer does once the pane applies it.

use super::*;
use crate::pane::menus::PaneMenuIntent;

fn pane_with_rectangle() -> ChartPane {
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool is registered");
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    pane.drawings.place(rectangle, ChartPoint::at(1.0, 100.0));
    pane.drawings.place(rectangle, ChartPoint::at(5.0, 110.0));
    pane.drawings.select(None);
    assert_eq!(pane.drawings.items().len(), 1);
    pane
}

fn apply(pane: &mut ChartPane, intents: Vec<PaneMenuIntent>) {
    with_chrome(Tool::Pointer, |chrome| {
        pane.apply_menu_intents(intents, chrome)
    });
}

#[test]
fn the_drawing_section_intents_write_the_drawing() {
    let mut pane = pane_with_rectangle();
    apply(
        &mut pane,
        vec![
            PaneMenuIntent::SelectDrawing(0),
            PaneMenuIntent::RenameDrawing {
                index: 0,
                name: "range high".to_owned(),
            },
            PaneMenuIntent::SetDrawingHidden {
                index: 0,
                hidden: true,
            },
            PaneMenuIntent::SetDrawingLocked {
                index: 0,
                locked: true,
            },
        ],
    );
    let drawing = &pane.drawings.items()[0];
    assert_eq!(pane.drawings.selected(), Some(0));
    assert_eq!(drawing.name.as_deref(), Some("range high"));
    assert!(drawing.hidden);
    assert!(drawing.locked);
}

#[test]
fn delete_removes_an_unlocked_drawing_and_spares_a_locked_one() {
    let mut pane = pane_with_rectangle();
    apply(
        &mut pane,
        vec![
            PaneMenuIntent::SetDrawingLocked {
                index: 0,
                locked: true,
            },
            PaneMenuIntent::DeleteDrawing(0),
        ],
    );
    assert_eq!(
        pane.drawings.items().len(),
        1,
        "a locked object never deletes by accident"
    );

    apply(
        &mut pane,
        vec![
            PaneMenuIntent::SetDrawingLocked {
                index: 0,
                locked: false,
            },
            PaneMenuIntent::DeleteDrawing(0),
        ],
    );
    assert!(pane.drawings.items().is_empty());
}

#[test]
fn a_closing_menu_commits_the_rename_in_flight_and_only_a_changed_one() {
    let mut pane = pane_with_rectangle();
    let id = pane.drawings.items()[0].id;

    pane.context_menu.drawing = Some(id);
    pane.context_menu.rename = String::new();
    assert!(
        pane.context_menu.close(&pane.drawings).is_none(),
        "an untouched name asks for nothing"
    );
    assert!(
        pane.context_menu.drawing.is_none(),
        "closing lets go of the drawing"
    );

    pane.context_menu.drawing = Some(id);
    pane.context_menu.rename = "vwap anchor".to_owned();
    let commit = pane.context_menu.close(&pane.drawings);
    apply(&mut pane, commit.into_iter().collect());
    assert_eq!(
        pane.drawings.items()[0].name.as_deref(),
        Some("vwap anchor")
    );
}

#[test]
fn a_layer_intent_is_the_layer_setter() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let style = crate::style::ChartStyle::default();
    let layer = ChartLayer::PointerPrice;
    let before = pane.layer_visible(layer, &style);
    apply(
        &mut pane,
        vec![PaneMenuIntent::SetLayerVisible {
            layer,
            visible: !before,
        }],
    );
    assert_eq!(pane.layer_visible(layer, &style), !before);
}

#[test]
fn chrome_intents_raise_the_hosts_flags() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    with_chrome(Tool::Pointer, |chrome| {
        assert!(!chrome.layers.open_footprint_settings);
        pane.apply_menu_intent(PaneMenuIntent::OpenFootprintSettings, chrome);
        assert!(chrome.layers.open_footprint_settings);
    });
}
