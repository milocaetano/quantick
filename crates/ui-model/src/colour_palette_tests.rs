//! The palette popover (designer B) and the header swatch, read off the
//! marks the window paints, on the mock's eight workspaces.

use quantick_session::colour_popover::{ColourIntent, ColourPopover, KeyAct};
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_stores::workspaces::{WorkspaceBook, parse_book};

use super::*;
use crate::workspace_menu::{Mark, area};

fn measure(words: &str, _size: f32) -> f32 {
    words.chars().count() as f32 * 7.0
}

fn mock(last_custom: bool) -> WorkspaceBook {
    let names = [
        ("Default", 4),
        ("Flow", 3),
        ("Macro", 14),
        ("Night", 15),
        ("Opening", 0),
        ("Scalp", 2),
        ("Swing", 5),
        ("Tape", 1),
    ];
    let mut text = String::from("version = 1\n");
    if last_custom {
        text.push_str("last_custom = \"#3A7BD5\"\n");
    }
    for (name, _) in names {
        text.push_str(&format!("[recipes.{name}]\nlayout = \"flow\"\n"));
    }
    text.push_str("[colours]\n");
    for (name, index) in names {
        text.push_str(&format!("{name} = {index}\n"));
    }
    parse_book(&text).unwrap()
}

/// The menu's header pieces on tab 1, which is on `Default`.
fn header(book: &WorkspaceBook, dark: bool) -> Vec<Piece> {
    use quantick_control_handlers::workspace_book::cell_menu::cell_menu;
    use quantick_workspace::canvas::{PaneKind, PaneTree};
    let live = PaneTree::pane(0, PaneKind::Flow);
    let menu = cell_menu(book, 1, "Ctrl+S", &live, dark, |tab| format!("T{tab}"));
    crate::workspace_menu::head(&menu, 318.0, dark, &measure)
}

fn drawn(book: &WorkspaceBook, state: &ColourPopover, dark: bool) -> Popover {
    popover(book, "Flow", &mut state.clone(), dark, &measure).unwrap()
}

fn keyed<'a>(marks: &'a Popover, key: &str) -> &'a Piece {
    let found = marks.pieces.iter().find(|piece| {
        piece
            .row
            .as_ref()
            .is_some_and(|row| row.key == format!("colour.{key}"))
    });
    found.unwrap_or_else(|| panic!("{key}"))
}

fn words(piece: &Piece) -> Vec<&str> {
    let texts = piece.marks.iter().filter_map(|mark| match mark {
        Mark::Text(words) => Some(words.text.as_str()),
        _ => None,
    });
    texts.collect()
}

/// B: 272 wide, four by four 56 x 26 swatches 8 apart with the name under
/// each, the current one ringed and checked, a held one carrying its
/// owner's initials, and the last row the seventeenth swatch then Custom….
#[test]
fn palette_b_lays_four_by_four_named_swatches() {
    for dark in [true, false] {
        let marks = drawn(&mock(true), &ColourPopover::default(), dark);
        assert_eq!(marks.size, [272.0, 272.0]);
        let cobalt = keyed(&marks, "cobalt");
        assert_eq!(cobalt.at, area(12.0, 12.0, 56.0, 26.0));
        assert!(cobalt.press);
        assert_eq!(keyed(&marks, "orchid").at.x, 76.0);
        assert_eq!(keyed(&marks, "slate").at, area(12.0, 63.0, 56.0, 26.0));
        assert_eq!(keyed(&marks, "blossom").at, area(204.0, 165.0, 56.0, 26.0));
        assert_eq!(words(keyed(&marks, "petrol")), ["Petrol"]);
        assert_eq!(words(keyed(&marks, "sky")), ["Sc", "Sky"]);
        let violet = keyed(&marks, "violet");
        assert_eq!(words(violet), ["\u{e182}", "Violet"], "checked");
        let rings = violet.marks.iter().filter(
            |mark| matches!(mark, Mark::Box(plate) if plate.fill.is_none() && plate.edge.is_some()),
        );
        assert_eq!(rings.count(), 2, "a 2 pt ring");
        let row = violet.row.as_ref().unwrap();
        assert!(row.on);
        assert_eq!(row.tip(false), Some("Violet \u{b7} current"));
        assert_eq!(
            row.command,
            Some(WorkspaceCommand::Colour(ColourIntent::Pick(
                "violet".to_owned()
            )))
        );
        let last = keyed(&marks, "last_custom");
        assert_eq!(last.at, area(12.0, 216.0, 56.0, 26.0));
        assert_eq!(words(last), ["#3A7BD5"]);
        let custom = keyed(&marks, "custom");
        assert_eq!((custom.at.x, custom.at.y, custom.at.h), (76.0, 216.0, 26.0));
        assert_eq!(words(custom), ["Custom\u{2026}"]);
    }
    let fresh = drawn(&mock(false), &ColourPopover::default(), true);
    assert!(fresh.pieces.iter().all(|piece| {
        piece
            .row
            .as_ref()
            .is_none_or(|row| row.key != "colour.last_custom")
    }));
    assert_eq!(
        keyed(&fresh, "custom").at.x,
        12.0,
        "Custom… takes the first place"
    );
}

/// The keys' cells are the swatches in order, then Custom…; Close, Swap,
/// Cancel and Apply are the popover's own presses.
#[test]
fn a_key_press_reaches_the_cell_it_names() {
    let marks = drawn(&mock(true), &ColourPopover::default(), true);
    assert_eq!(marks.cells.len(), 18);
    let pick = |word: &str| {
        Some(WorkspaceCommand::Colour(ColourIntent::Pick(
            word.to_owned(),
        )))
    };
    assert_eq!(marks.command(KeyAct::Press(2)), pick("sky"));
    assert_eq!(marks.command(KeyAct::Press(16)), pick("last_custom"));
    let custom = WorkspaceCommand::Colour(ColourIntent::Custom { dark: true });
    assert_eq!(marks.command(KeyAct::Press(17)), Some(custom));
    let colour = |intent| Some(WorkspaceCommand::Colour(intent));
    assert_eq!(marks.command(KeyAct::Close), colour(ColourIntent::Close));
    assert_eq!(
        marks.command(KeyAct::Swap),
        colour(ColourIntent::Answer(true))
    );
    assert_eq!(
        marks.command(KeyAct::Cancel),
        colour(ColourIntent::Answer(false))
    );
    assert_eq!(marks.command(KeyAct::None), None);
    assert_eq!(marks.current, Some(3), "Violet");
}

/// The question stands in the grid's place, in two lines, with the two
/// workspaces' colours after a swap and Swap in the accent's fill.
#[test]
fn the_question_replaces_the_grid() {
    let mut state = ColourPopover::default();
    state.ask("sky");
    for dark in [true, false] {
        let marks = drawn(&mock(true), &state, dark);
        assert_eq!(marks.size, [272.0, 112.0]);
        let question = keyed(&marks, "question");
        assert_eq!(
            words(question),
            [
                "Sky is used by Scalp. Swap?",
                "Scalp becomes Violet.",
                "Flow",
                "Scalp"
            ]
        );
        let swap = keyed(&marks, "swap");
        let Mark::Box(fill) = &swap.marks[0] else {
            panic!("a box")
        };
        let filled = quantick_appearance::workspace_menu::FilledButton::primary();
        assert_eq!((fill.fill, swap_ink(swap)), (Some(filled.fill), filled.ink));
        assert!(
            marks
                .pieces
                .iter()
                .all(|piece| { piece.row.as_ref().is_none_or(|row| row.key != "colour.sky") })
        );
    }
}

fn swap_ink(piece: &Piece) -> quantick_appearance::Token {
    let ink = piece.marks.iter().find_map(|mark| match mark {
        Mark::Text(words) => Some(words.ink),
        _ => None,
    });
    ink.unwrap()
}

/// Custom opens under the grid: the square, the hue strip, the field, Apply
/// and Cancel, then one line per warning; a malformed field disables Apply
/// and says how to write one.
#[test]
fn custom_opens_under_the_grid_with_its_warnings() {
    let mut state = ColourPopover::default();
    state.open_custom("#62B8EA".to_owned(), [0.0; 3]);
    let marks = drawn(&mock(true), &state, true);
    for key in ["square", "hue", "hex", "apply", "cancel"] {
        assert!(keyed(&marks, key).row.is_some(), "{key}");
    }
    assert_eq!(keyed(&marks, "square").at, area(12.0, 276.0, 248.0, 104.0));
    assert!(keyed(&marks, "square").drag && keyed(&marks, "hue").drag);
    assert!(keyed(&marks, "apply").press);
    let warning = keyed(&marks, "warning.0");
    assert_eq!(
        words(warning),
        ["\u{e4e0}", "Hard to tell from Scalp's colour (Sky)."]
    );
    assert_eq!(marks.size[1], 444.0 + 18.0);
    assert!(
        marks.field.is_some(),
        "the window lays its text field there"
    );
    state.hex = "#3A7BD".to_owned();
    let marks = drawn(&mock(true), &state, true);
    let apply = keyed(&marks, "apply");
    assert!(!apply.press, "Apply is disabled");
    assert_eq!(
        words(keyed(&marks, "hint")),
        ["Use #RRGGBB, for example #3A7BD5"]
    );
    assert_eq!(marks.command(KeyAct::Apply), None, "Enter applies nothing");
}

/// A drag on the square or the strip asks for the picker's new place, in
/// thousandths, clamped to the box.
#[test]
fn a_drag_names_its_place_in_thousandths() {
    assert_eq!(
        dragged("colour.square", [0.25, 1.5]),
        Some(WorkspaceCommand::Colour(ColourIntent::Square {
            x: 250,
            y: 1000
        }))
    );
    assert_eq!(
        dragged("colour.hue", [-0.5, 0.0]),
        Some(WorkspaceCommand::Colour(ColourIntent::Hue(0)))
    );
    assert_eq!(dragged("colour.sky", [0.5, 0.5]), None);
}

/// The header's swatch: 16 x 16 with a 4 pt radius, centred on the 34 pt
/// line, pressed over 24 x 24, ringed in the accent on hover; the miniature
/// and the name move right by the swatch's extra width, 13 pt.
#[test]
fn the_header_swatch_is_a_sixteen_point_press() {
    let book = mock(true);
    let marks = header(&book, true);
    let swatch = marks
        .iter()
        .find(|piece| piece.row.as_ref().is_some_and(|row| row.key == "colour"))
        .expect("the swatch");
    assert_eq!(swatch.at, area(4.0, 13.0, 24.0, 24.0));
    assert!(swatch.press);
    assert!(matches!(
        swatch.marks[0],
        Mark::Colour { at, radius, .. } if at == area(8.0, 17.0, 16.0, 16.0) && radius == 4.0
    ));
    let ring = swatch.hover.expect("a hover ring");
    assert_eq!(
        (ring.fill, ring.edge),
        (None, Some(quantick_appearance::Token::Accent))
    );
    let current = marks
        .iter()
        .find(|piece| piece.row.as_ref().is_some_and(|row| row.key == "current"))
        .unwrap();
    let mini = current.marks.iter().find_map(|mark| match mark {
        Mark::Tree(at, _) => Some(*at),
        _ => None,
    });
    assert_eq!(
        mini.unwrap().x,
        8.0 + 16.0 + 8.0,
        "13 pt right of the old 19"
    );
    assert!(
        !current
            .marks
            .iter()
            .any(|mark| matches!(mark, Mark::Colour { .. })),
        "the swatch paints the colour"
    );
}

fn handle(marks: &Popover) -> Area {
    let square = keyed(marks, "square");
    let found = square.marks.iter().find_map(|mark| match mark {
        Mark::Box(plate) if plate.fill.is_none() && plate.at.w == 10.0 => Some(plate.at),
        _ => None,
    });
    found.expect("the marker")
}

fn plates(piece: &Piece) -> Vec<Plate> {
    let found = piece.marks.iter().filter_map(|mark| match mark {
        Mark::Box(plate) => Some(*plate),
        _ => None,
    });
    found.collect()
}

/// A question whose colour freed under it is left in the state too, so the
/// keys answer the grid that is drawn.
#[test]
fn a_freed_question_is_left_where_it_is_drawn() {
    let mut book = mock(true);
    let mut state = ColourPopover::default();
    state.ask("sky");
    book.set_colour("Scalp", WorkspaceColour::preset(6).unwrap(), false)
        .unwrap();
    let marks = popover(&book, "Flow", &mut state, true, &measure).unwrap();
    assert_eq!(state.asked, None);
    assert!(marks.cells.len() == 18, "the grid");
}

/// A malformed field keeps the picker on the last valid colour; one staged
/// malformed from the start shows the current colour.
#[test]
fn a_malformed_field_keeps_the_last_valid_picker() {
    let book = mock(true);
    let mut state = ColourPopover::default();
    state.open_custom("#62B8EA".to_owned(), [0.0, 0.0, 0.0]);
    let valid = handle(&popover(&book, "Flow", &mut state, true, &measure).unwrap());
    state.hex = "#62B8E".to_owned();
    let broken = popover(&book, "Flow", &mut state, true, &measure).unwrap();
    assert_eq!(handle(&broken), valid);
    let mut staged = ColourPopover {
        custom: true,
        hex: "#3A7BD".to_owned(),
        ..ColourPopover::default()
    };
    let current = handle(&popover(&book, "Flow", &mut staged, true, &measure).unwrap());
    let mut violet = ColourPopover::default();
    violet.open_custom("#A887F2".to_owned(), [0.0; 3]);
    let expected = handle(&popover(&book, "Flow", &mut violet, true, &measure).unwrap());
    assert_eq!(current, expected, "Violet as Dark paints it");
}

/// The question's chips show what each workspace will be painted in the
/// theme: a typed Sky value swaps Sky itself.
#[test]
fn the_question_chips_paint_each_workspace_s_new_colour_in_the_theme() {
    for dark in [true, false] {
        let mut state = ColourPopover::default();
        state.ask("#5DBDEB");
        let marks = drawn(&mock(true), &state, dark);
        let chips: Vec<[u8; 3]> = (keyed(&marks, "question").marks.iter())
            .filter_map(|mark| match mark {
                Mark::Colour { rgb, .. } => Some(*rgb),
                _ => None,
            })
            .collect();
        let sky = WorkspaceColour::preset(2).unwrap().rgb(dark);
        let violet = WorkspaceColour::preset(3).unwrap().rgb(dark);
        assert_eq!(chips, [sky, violet], "dark {dark}");
    }
}

/// The popover hangs below the header's words, and the question ends on a
/// row boundary of the list behind it.
#[test]
fn the_popover_clears_the_header_and_the_question_ends_on_a_row() {
    use crate::workspace_menu::{HEAD_PT, SWATCH_BOTTOM_PT};
    use quantick_session::workspace_menu::{CHIP_MENU_BORDER_PT, CHIP_MENU_ROW_PT};
    let pieces = header(&mock(true), true);
    let swatch = pieces
        .iter()
        .find(|piece| piece.row.as_ref().is_some_and(|row| row.key == "colour"))
        .unwrap();
    assert_eq!(swatch.at.y + swatch.at.h - 4.0, SWATCH_BOTTOM_PT);
    let current = pieces
        .iter()
        .find(|piece| piece.row.as_ref().is_some_and(|row| row.key == "current"))
        .unwrap();
    let summary = current.marks.iter().filter_map(|mark| match mark {
        Mark::Text(words) => Some(words.y + words.size * 0.6),
        _ => None,
    });
    let words_end = summary.fold(0.0_f32, f32::max);
    let top = SWATCH_BOTTOM_PT + DROP_PT;
    assert!(top >= words_end, "{top} under {words_end}");
    let mut state = ColourPopover::default();
    state.ask("sky");
    let marks = drawn(&mock(true), &state, true);
    let bottom = top + marks.size[1] + 2.0 * CHIP_MENU_BORDER_PT - HEAD_PT;
    assert!(bottom > 0.0 && bottom % CHIP_MENU_ROW_PT == 0.0, "{bottom}");
}

/// Custom's field wears the accent's 1 pt focus border, its preview chip
/// the popover's border, and a disabled Apply no accent at all.
#[test]
fn the_field_the_chip_and_a_disabled_apply_wear_their_edges() {
    let accent = quantick_appearance::Token::Accent;
    for dark in [true, false] {
        let ink = quantick_appearance::workspace_menu::chrome(dark);
        let mut state = ColourPopover::default();
        state.open_custom("#3A3550".to_owned(), [0.0; 3]);
        let marks = drawn(&mock(true), &state, dark);
        assert!(plates(keyed(&marks, "hex")).iter().any(|p| p.edge == Some(accent)));
        assert!(plates(keyed(&marks, "preview")).iter().any(|p| p.edge == Some(ink.edge)));
        state.hex = "#3A7BD".to_owned();
        let marks = drawn(&mock(true), &state, dark);
        let apply = keyed(&marks, "apply");
        assert!(plates(apply).iter().all(|p| p.fill != Some(accent) && p.edge != Some(accent)));
        let label = apply.marks.iter().find_map(|mark| match mark {
            Mark::Text(words) => Some(words.ink),
            _ => None,
        });
        assert_eq!(label, Some(ink.faint));
    }
}

/// A swatch's initials and check are pure black or pure white, whichever
/// reads more, and reach 4.5:1 on every preset in both themes.
#[test]
fn swatch_marks_read_on_every_preset_in_both_themes() {
    use quantick_appearance::{PremultipliedRgba, contrast::contrast_ratio};
    let rgb = |[r, g, b]: [u8; 3]| PremultipliedRgba::from_rgb(r, g, b);
    for dark in [true, false] {
        for index in 0..16 {
            let fill = WorkspaceColour::preset(index).unwrap().rgb(dark);
            let ink = mono_ink(fill);
            assert!(ink == [0; 3] || ink == [255; 3]);
            let ratio = contrast_ratio(rgb(ink), rgb(fill));
            assert!(ratio >= 4.5, "{index} dark {dark}: {ratio}");
        }
    }
    let marks = drawn(&mock(true), &ColourPopover::default(), true);
    let initials = keyed(&marks, "sky").marks.iter().find_map(|mark| match mark {
        Mark::Text(words) if words.text == "Sc" => words.rgb,
        _ => None,
    });
    assert_eq!(initials, Some(mono_ink(WorkspaceColour::preset(2).unwrap().rgb(true))));
}
