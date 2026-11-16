//! Where designer B's parts stand, read off the marks the window paints.

use quantick_control_handlers::workspace_book::cell_menu::{QUIET_SAVE, cell_menu};
use quantick_stores::workspaces::{WorkspaceBook, WorkspaceRecipe};
use quantick_workspace::canvas::{PaneKind, PaneTree};

use super::*;

/// Words measure seven points a character.
fn measure(words: &str, _size: f32) -> f32 {
    words.chars().count() as f32 * 7.0
}

/// Tab 1 on `Scalp`, saved, autosave as asked; with `edited` the tab holds
/// an edit.
fn menu(autosave: bool, edited: bool) -> CellMenu {
    let mut book = WorkspaceBook::default();
    book.open_tab(1);
    book.set_autosave(false);
    let saved = WorkspaceRecipe {
        footprint_style: Some("heatmap".to_owned()),
        ..WorkspaceRecipe::default()
    };
    book.observe(1, &saved).unwrap();
    book.save_as(1, "Scalp").unwrap();
    if edited {
        let edit = WorkspaceRecipe {
            footprint_style: Some("bars".to_owned()),
            ..saved
        };
        book.observe(1, &edit).unwrap();
    }
    book.set_autosave(autosave);
    let live = PaneTree::pane(0, PaneKind::Flow);
    cell_menu(&book, 1, "Ctrl+S", &live, true, |tab| format!("T{tab}"))
}

fn keyed<'a>(pieces: &'a [Piece], key: &str) -> &'a Piece {
    let mut found = pieces.iter();
    found
        .find(|piece| piece.row.as_ref().is_some_and(|row| row.key == key))
        .unwrap_or_else(|| panic!("{key}"))
}

fn words(piece: &Piece) -> Vec<&str> {
    let texts = piece.marks.iter().filter_map(|mark| match mark {
        Mark::Text(words) => Some(words.text.as_str()),
        _ => None,
    });
    texts.collect()
}

/// The header block is 81 tall with the caption's 24 under it; Save takes
/// what Save as and Rename leave of the row, 4 apart, 19 in and 10 short of
/// the right edge.
#[test]
fn the_header_lays_its_line_and_buttons_as_the_mock_does() {
    let pieces = head(&menu(false, true), 318.0, true, &measure);
    let block = &pieces[0];
    assert_eq!((block.at.h, block.row.is_none()), (81.0, true));
    let current = keyed(&pieces, "current");
    assert_eq!((current.at.x, current.at.y, current.at.h), (8.0, 8.0, 34.0));
    assert!(!current.press, "the header line is no button");
    assert!(words(current).contains(&"Unsaved"), "{:?}", words(current));
    let [save, save_as, rename] = ["save", "save_as", "rename"].map(|key| keyed(&pieces, key).at);
    assert_eq!((save.x, save.y, save.h), (19.0, 48.0, 24.0));
    assert_eq!(save_as.x, save.x + save.w + 4.0);
    assert_eq!(rename.x, save_as.x + save_as.w + 4.0);
    assert_eq!(rename.x + rename.w, 318.0 - 10.0);
    let primary = keyed(&pieces, "save").marks[0].clone();
    let Mark::Box(primary) = primary else {
        panic!("a box")
    };
    assert_eq!(primary.fill, Some(Token::Accent), "Save stands out");
    assert_eq!(primary.alpha, chrome(true).tint, "in the accent's tint");
    assert_eq!(primary.edge, Some(Token::Accent));
    let caption = pieces.last().unwrap();
    assert_eq!((caption.at.y, caption.at.h), (81.0, 24.0));
    assert_eq!(words(caption), ["WORKSPACES", "2"]);
}

/// Autosave on: Save is a quiet line no press reaches, and no Unsaved.
#[test]
fn autosave_on_paints_save_as_a_quiet_line() {
    let pieces = head(&menu(true, true), 318.0, false, &measure);
    let save = keyed(&pieces, "save");
    assert!(!save.press && save.hover.is_none());
    let caption = pieces.last().unwrap();
    let Mark::Text(spaced) = &caption.marks[0] else {
        panic!("words")
    };
    assert!(
        (spaced.tracking - 0.8).abs() < 1e-5,
        "the caption's letters spaced"
    );
    assert!(words(save).contains(&QUIET_SAVE));
    assert!(!words(keyed(&pieces, "current")).contains(&"Unsaved"));
}

/// A row is 34 tall: the tab's own on the selection fill with a check, the
/// focused one ringed in the accent, every other hovered in the hover fill.
#[test]
fn a_row_wears_its_status_and_the_focus_ring() {
    let menu = menu(false, false);
    let own = menu.list.iter().find(|row| row.on).unwrap();
    let other = menu.list.iter().find(|row| !row.on).unwrap();
    let lit = row(own, 310.0, false, true);
    assert_eq!(
        (lit.at.x, lit.at.w, lit.at.h),
        (4.0, 302.0, 34.0),
        "inside the padding"
    );
    assert!(lit.press, "the tab's own row closes the menu");
    assert!(words(&lit).contains(&CHECK), "the check");
    // Every workspace row tinted toward its own colour, its own the most.
    let fill = |row: &CellRow, tint| Some(row_fill(row.colour.as_ref().unwrap().rgb, true, tint));
    let Mark::Box(back) = &lit.marks[0] else {
        panic!("a box")
    };
    assert_eq!(back.rgb, fill(own, RowTint::Current));
    let rest = row(other, 310.0, false, true);
    let Mark::Box(at_rest) = &rest.marks[0] else {
        panic!("a box")
    };
    assert_eq!(at_rest.rgb, fill(other, RowTint::Rest));
    let focused = row(other, 310.0, true, true);
    let Mark::Box(ring) = &focused.marks[1] else {
        panic!("a box")
    };
    assert_eq!(ring.edge, Some(Token::Accent));
    let hovered = rest.hover.expect("a hover fill");
    assert_eq!(
        (hovered.rgb, hovered.radius),
        (fill(other, RowTint::Hover), 4.0)
    );
    let named: Vec<bool> = (lit.marks.iter())
        .filter_map(|mark| match mark {
            Mark::Text(words) => Some(words.names),
            _ => None,
        })
        .collect();
    assert_eq!(
        named.iter().filter(|names| **names).count(),
        1,
        "only the name"
    );
    let tree = (lit.marks.iter()).find_map(|mark| match mark {
        Mark::Tree(at, _) => Some(*at),
        _ => None,
    });
    let tree = tree.expect("a miniature");
    assert_eq!((tree.w, tree.h), (26.0, 14.0));
    assert_eq!(tree.x, 4.0 + 4.0 + 3.0 + 8.0, "after the empty colour slot");
}

/// The footer: the switch row 28 tall, then New taking what Manage leaves.
#[test]
fn the_footer_lays_the_switch_then_new_and_manage() {
    let pieces = foot(&menu(false, false), 318.0, true, &measure);
    assert_eq!(pieces[0].at.h, 65.0);
    let switch = keyed(&pieces, "autosave");
    assert_eq!((switch.at.y, switch.at.h), (5.0, 28.0));
    assert!(words(switch).contains(&"Off \u{b7} Ctrl+S saves"));
    let (new, manage) = (keyed(&pieces, "new").at, keyed(&pieces, "manage").at);
    assert_eq!((new.y, new.h), (33.0, 24.0));
    assert_eq!(new.x + new.w + 4.0, manage.x);
    assert_eq!(manage.x + manage.w, 318.0 - 6.0);
}

#[test]
fn the_filter_field_spans_the_rows_in_the_control_s_fill() {
    let menu = menu(false, false);
    let row = CellRow {
        label: "desk".to_owned(),
        ..menu.current.clone()
    };
    let field = filter(row, 318.0, true);
    let line = super::row(&menu.list[0], 318.0, false, true);
    assert_eq!(
        (field.at.x, field.at.w, field.at.h),
        (line.at.x, line.at.w, 24.0)
    );
    let Mark::Box(plate) = &field.marks[0] else {
        panic!("a box")
    };
    assert_eq!(plate.fill, Some(Token::Control));
    let words = filter_text(318.0);
    assert!(words.x > field.at.x && words.x + words.w < field.at.x + field.at.w);
}

/// An enabled button is edged in the menu's edge; a disabled one is bare,
/// on the block's fainter edge, so the two never read alike.
#[test]
fn a_disabled_button_reads_disabled_in_both_themes() {
    for dark in [true, false] {
        let pieces = head(&menu(false, false), 318.0, dark, &measure);
        let plate = |key: &str| match &keyed(&pieces, key).marks[0] {
            Mark::Box(plate) => *plate,
            _ => panic!("a box"),
        };
        let ink = chrome(dark);
        let (save, save_as) = (plate("save"), plate("save_as"));
        assert_eq!(
            (save_as.fill, save_as.edge),
            (Some(ink.control), Some(ink.edge))
        );
        assert_eq!(
            (save.fill, save.edge),
            (None, Some(ink.block_edge)),
            "no edits"
        );
        assert!(!keyed(&pieces, "save").press);
    }
}

/// Only the switch and its label take a press: the state words at the
/// row's right end lie outside it.
#[test]
fn only_the_switch_and_its_label_take_the_autosave_press() {
    let pieces = foot(&menu(false, false), 318.0, true, &measure);
    let switch = keyed(&pieces, "autosave");
    assert!(switch.press);
    assert!(switch.at.w < 150.0, "{}", switch.at.w);
    let state = (switch.marks.iter()).find_map(|mark| match mark {
        Mark::Text(words) if words.text.starts_with("Off") => Some(words.x),
        _ => None,
    });
    assert!(state.expect("the state words") > switch.at.x + switch.at.w);
}

/// The tab's own row writes its summary at 4.5:1 or better on the
/// selection fill, in Dark and in Light.
#[test]
fn the_selected_row_s_summary_reads_on_its_fill() {
    use quantick_appearance::contrast::contrast_ratio;
    use quantick_appearance::{LIGHT, ResolvedTheme};
    let menu = menu(false, false);
    let own = menu.list.iter().find(|row| row.on).unwrap();
    for theme in [
        ResolvedTheme::default(),
        ResolvedTheme::parse(LIGHT).unwrap(),
    ] {
        let lit = super::row(own, 310.0, false, theme.dark);
        let Mark::Box(back) = &lit.marks[0] else {
            panic!("a box")
        };
        let summary = (lit.marks.iter()).find_map(|mark| match mark {
            Mark::Text(words) if words.text.contains("ind") => Some(words.clone()),
            _ => None,
        });
        let summary = summary.expect("a summary");
        let [r, g, b] = back.rgb.expect("its own colour's tint");
        let fill = quantick_appearance::PremultipliedRgba::from_rgb(r, g, b);
        let ratio = contrast_ratio(theme.color(summary.ink), fill);
        assert!(ratio >= 4.5, "{}: {ratio}", theme.name);
    }
}

/// The colour fills #1036's slot, 3 x 22 with a 1.5 radius, in every row,
/// and the header's 16 x 16 swatch, as the resolved value: a custom colour
/// is no token.
#[test]
fn the_colour_fills_the_slot_in_every_row_and_the_header() {
    use quantick_stores::workspaces::WorkspaceColour;
    let mut book = WorkspaceBook::default();
    book.open_tab(1);
    book.observe(1, &WorkspaceRecipe::default()).unwrap();
    book.save_as(1, "Scalp").unwrap();
    let custom = WorkspaceColour::Custom([0x3A, 0x7B, 0xD5]);
    book.set_colour("Scalp", custom, false).unwrap();
    let live = PaneTree::pane(0, PaneKind::Flow);
    let slot = |piece: &Piece| {
        let mut slots = piece.marks.iter().filter_map(|mark| match mark {
            Mark::Colour { at, radius, rgb } => Some((*at, *radius, *rgb)),
            _ => None,
        });
        let found = slots.next().expect("a colour slot");
        assert!(slots.next().is_none(), "one slot");
        found
    };
    for dark in [true, false] {
        let menu = cell_menu(&book, 1, "Ctrl+S", &live, dark, |tab| format!("T{tab}"));
        let slate = WorkspaceColour::preset(4).unwrap().rgb(dark);
        let [default, scalp] = [&menu.list[0], &menu.list[1]].map(|r| row(r, 310.0, false, dark));
        assert_eq!(slot(&default), (area(8.0, 6.0, 3.0, 22.0), 1.5, slate));
        assert_eq!(slot(&scalp).2, [0x3A, 0x7B, 0xD5]);
        let pieces = head(&menu, 318.0, dark, &measure);
        let header = slot(keyed(&pieces, "colour"));
        assert_eq!(
            header,
            (area(8.0, 17.0, 16.0, 16.0), 4.0, [0x3A, 0x7B, 0xD5]),
            "the header's slot is its swatch"
        );
    }
}

/// The selection and hover fills start one point past the colour slot, so
/// the slot stands on the panel and never sinks into the highlight; the
/// geometry is the same in both themes.
#[test]
fn the_row_highlight_starts_past_the_colour_slot() {
    let menu = menu(false, false);
    let own = menu.list.iter().find(|row| row.on).unwrap();
    let other = menu.list.iter().find(|row| !row.on).unwrap();
    for dark in [true, false] {
        let lit = row(own, 310.0, false, dark);
        let slot = (lit.marks.iter())
            .find_map(|mark| match mark {
                Mark::Colour { at, .. } => Some(*at),
                _ => None,
            })
            .expect("a slot");
        let Mark::Box(back) = &lit.marks[0] else {
            panic!("a box")
        };
        assert_eq!(back.at.x, slot.x + slot.w + 1.0, "dark {dark}");
        assert_eq!(
            back.at.x + back.at.w,
            lit.at.x + lit.at.w,
            "to the row's end"
        );
        let hovered = row(other, 310.0, false, dark).hover.unwrap();
        assert_eq!(hovered.at, back.at, "hover takes the same box");
        let focused = row(other, 310.0, true, dark);
        let Mark::Box(ring) = &focused.marks[0] else {
            panic!("a box")
        };
        assert_eq!(ring.at, back.at, "and the focus ring");
    }
}
