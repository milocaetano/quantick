use crate::indicator_document::{SavedIndicator, SavedInput, SavedKind, SavedPlotStyle};
use crate::indicator_mirror::*;
use crate::layout_document::LayoutId;

/// A pane address as the app spells one: tab id and side index.
type Pane = (u64, u8);

const SHARED: LayoutId = LayoutId(1);
const OTHER: LayoutId = LayoutId(2);

fn saved(kind: SavedKind) -> SavedIndicator {
    SavedIndicator {
        kind,
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    }
}

fn pane(pane: Pane, layout: LayoutId, slots: &[u64]) -> PaneSlots<Pane, u64> {
    PaneSlots {
        pane,
        layout: Some(layout),
        slots: slots.to_vec(),
    }
}

/// Two tabs on the shared layout, a third on another, and an unseeded pane.
fn workspace() -> Vec<PaneSlots<Pane, u64>> {
    vec![
        pane((1, 0), SHARED, &[10, 11]),
        pane((2, 0), SHARED, &[20, 21]),
        pane((3, 0), OTHER, &[30, 31]),
        PaneSlots {
            pane: (4, 0),
            layout: None,
            slots: Vec::new(),
        },
    ]
}

#[test]
fn input_edit_mirrors_to_panes_on_the_same_layout() {
    let mut entries = vec![saved(SavedKind::native("native.ema")); 2];
    let origin = (1, 0);
    let index = layout_index(&workspace()[0].slots, &11).expect("a layout slot");
    let inputs = vec![SavedInput::Int(21)];
    let actions = mirror(
        Some(&mut entries),
        (SHARED, index),
        &origin,
        LayoutEdit::Inputs(inputs.clone()),
        workspace(),
    );
    assert_eq!(
        actions,
        vec![MirrorAction::Edit {
            pane: (2, 0),
            slot: 21
        }],
        "only the other pane on the same layout, at the same entry index"
    );
    assert_eq!(entries[1].inputs, inputs);
    assert!(entries[0].inputs.is_empty(), "the other entry is untouched");
}

#[test]
fn add_inserts_the_entry_and_reaches_every_other_pane_of_the_layout() {
    let mut entries = vec![saved(SavedKind::native("native.cvd"))];
    let kind = SavedKind::native("native.rsi");
    let actions = mirror(
        Some(&mut entries),
        (SHARED, 0),
        &(2, 0),
        LayoutEdit::Add(kind.clone()),
        workspace(),
    );
    assert_eq!(
        actions,
        vec![MirrorAction::Add {
            pane: (1, 0),
            kind: kind.clone()
        }]
    );
    assert_eq!(entries[0], saved(kind), "inserted at the index, defaults");
    let actions = mirror(
        Some(&mut entries),
        (SHARED, 9),
        &(1, 0),
        LayoutEdit::Add(SavedKind::native("native.vwap")),
        Vec::<PaneSlots<Pane, u64>>::new(),
    );
    assert!(actions.is_empty());
    assert_eq!(
        entries[2].kind,
        SavedKind::native("native.vwap"),
        "appended"
    );
}

#[test]
fn an_index_a_pane_or_the_layout_lacks_writes_and_reaches_nothing_there() {
    let mut entries = vec![saved(SavedKind::native("native.ema"))];
    let actions = mirror(
        Some(&mut entries),
        (SHARED, 1),
        &(1, 0),
        LayoutEdit::Remove,
        vec![pane((2, 0), SHARED, &[20])],
    );
    assert!(actions.is_empty(), "the other pane has no slot at index 1");
    assert_eq!(entries.len(), 1, "no entry at index 1 to remove");
    let actions = mirror(
        None::<&mut Vec<SavedIndicator>>,
        (SHARED, 0),
        &(1, 0),
        LayoutEdit::Hidden(true),
        workspace(),
    );
    assert_eq!(
        actions,
        vec![MirrorAction::Edit {
            pane: (2, 0),
            slot: 20
        }],
        "a layout with no entries still reaches its panes"
    );
}

#[test]
fn each_edit_writes_its_own_field() {
    let mut entries = vec![saved(SavedKind::native("native.ema")); 2];
    let style = SavedPlotStyle {
        visible: Some(false),
        color: None,
        width: Some(2.5),
    };
    for edit in [
        LayoutEdit::Hidden(true),
        LayoutEdit::Styles(vec![style]),
        LayoutEdit::MouseVerticalLine(true),
    ] {
        let _ = mirror(Some(&mut entries), (SHARED, 0), &(1, 0), edit, workspace());
    }
    assert!(entries[0].hidden && entries[0].mouse_vertical_line);
    assert_eq!(entries[0].plot_styles, vec![style]);
    let _ = mirror(
        Some(&mut entries),
        (SHARED, 0),
        &(1, 0),
        LayoutEdit::Remove,
        workspace(),
    );
    assert_eq!(entries, vec![saved(SavedKind::native("native.ema"))]);
}

#[test]
fn saved_set_materialises_as_before() {
    let style = SavedPlotStyle {
        visible: None,
        color: Some([1, 2, 3, 255]),
        width: Some(1.5),
    };
    let set = vec![
        SavedIndicator {
            kind: SavedKind::native("native.ema"),
            hidden: true,
            mouse_vertical_line: false,
            inputs: vec![SavedInput::Int(9), SavedInput::Source("close".into())],
            plot_styles: vec![style],
        },
        SavedIndicator {
            kind: SavedKind::Script {
                name: "zigzag.pine".into(),
            },
            hidden: false,
            mouse_vertical_line: true,
            inputs: vec![SavedInput::Int(5), SavedInput::Source("nowhere".into())],
            plot_styles: Vec::new(),
        },
        saved(SavedKind::native("native.cvd")),
    ];
    // The app's reader refuses a source name it does not know.
    let read = |input: &SavedInput| match input {
        SavedInput::Source(name) if name != "close" => None,
        other => Some(other.clone()),
    };
    let plan = materialise(&set, read);
    assert_eq!(
        plan,
        vec![
            Materialise {
                kind: &set[0].kind,
                inputs: SavedInputs::Bind(set[0].inputs.clone()),
                hidden: true,
                mouse_vertical_line: false,
                plot_styles: Some(&[style][..]),
            },
            Materialise {
                kind: &set[1].kind,
                inputs: SavedInputs::Dropped {
                    saved: 2,
                    readable: 1
                },
                hidden: false,
                mouse_vertical_line: true,
                plot_styles: None,
            },
            Materialise {
                kind: &set[2].kind,
                inputs: SavedInputs::Defaults,
                hidden: false,
                mouse_vertical_line: false,
                plot_styles: None,
            },
        ],
        "every entry added in order, an unreadable input set falling back whole"
    );
}
