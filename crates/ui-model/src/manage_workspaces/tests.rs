//! Manage (designer A) and the create dialogs (designer A, palette B) as
//! marks, on the mock's eight workspaces: tab 1 on Flow, tabs 2 and 3 on
//! Scalp, the last custom colour #3A7BD5.
use super::*;
use quantick_session::colour_popover::ColourPopover;
use quantick_session::manage_workspaces::UndoRow;
use quantick_session::workspace_menu::WorkspaceMenuState;
use quantick_stores::workspaces::parse_book;

const NAMES: [(&str, u8); 8] = [
    ("Default", 4),
    ("Flow", 3),
    ("Macro", 14),
    ("Night", 15),
    ("Opening", 0),
    ("Scalp", 2),
    ("Swing", 5),
    ("Tape", 1),
];

fn book() -> WorkspaceBook {
    let mut text = String::from("version = 1\nlast_custom = \"#3A7BD5\"\n");
    for (name, _) in NAMES {
        text.push_str(&format!("[recipes.{name}]\nlayout = \"flow\"\n"));
    }
    text.push_str("[colours]\n");
    for (name, index) in NAMES {
        text.push_str(&format!("{name} = {index}\n"));
    }
    let mut book = parse_book(&text).unwrap();
    for (tab, name) in [(1, "Flow"), (2, "Scalp"), (3, "Scalp")] {
        book.open_tab(tab);
        book.set(tab, name).unwrap();
    }
    book
}

/// Seven points per character: what a test needs of a width.
fn measure(words: &str, size: f32) -> f32 {
    words.chars().count() as f32 * size * 0.55
}

fn symbols(tabs: &[u64]) -> Vec<String> {
    tabs.iter()
        .map(|tab| ["WINZ26", "WIN$N", "WDO$N"][*tab as usize - 1].to_owned())
        .collect()
}

fn view(dialog: &ManageDialog, dark: bool) -> Sheet {
    manage(&book(), dialog, ("Flow", 1_000, dark), &symbols, &measure)
}

fn piece<'a>(pieces: &'a [Piece], id: &str) -> &'a Piece {
    pieces
        .iter()
        .find(|piece| piece.row.as_ref().is_some_and(|row| row.scene_id() == id))
        .unwrap_or_else(|| panic!("no {id}"))
}

fn ids(pieces: &[Piece]) -> Vec<String> {
    pieces
        .iter()
        .filter_map(|piece| piece.row.as_ref().map(CellRow::scene_id))
        .collect()
}

/// One 36-point row per workspace in the list's order, the CURRENT pill on
/// the tab's own, and eight rows with Default's note fitting the list.
#[test]
fn the_rows_follow_the_list_with_the_current_pill() {
    let manage = view(&ManageDialog::new(1), true);
    let rows: Vec<&str> = manage.names.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(rows, NAMES.map(|(name, _)| name));
    assert_eq!(
        manage.list_height, LIST_PT,
        "eight rows show without scrolling"
    );
    assert_eq!(
        manage.size,
        [520.0, TITLE_PT + CAPTION_PT + LIST_PT + FOOT_PT]
    );
    let flow = piece(&manage.list, "manage_workspaces.Flow");
    assert!(flow.row.as_ref().unwrap().on);
    let pill = |name: &str| {
        let marks = &piece(&manage.list, &format!("manage_workspaces.{name}.name")).marks;
        marks
            .iter()
            .any(|mark| matches!(mark, Mark::Text(said) if said.text == "CURRENT"))
    };
    assert!(pill("Flow") && !pill("Macro"));
    let count = piece(&manage.list, "manage_workspaces.Scalp.tabs");
    let count_row = count.row.as_ref().unwrap();
    assert_eq!(count_row.label, "2 tabs");
    assert_eq!(
        count_row.tip.as_deref(),
        Some("WIN$N, WDO$N"),
        "the hover names the tabs"
    );
    assert_eq!(
        piece(&manage.list, "manage_workspaces.Night.tabs")
            .row
            .as_ref()
            .unwrap()
            .label,
        "No tabs"
    );
}

/// Default says why under its row, its pencil and trash are greyed with
/// the lock reason, and its chip is read-only.
#[test]
fn default_s_row_explains_the_lock_and_disables_every_edit() {
    let manage = view(&ManageDialog::new(1), true);
    let band = piece(&manage.list, "manage_workspaces.Default");
    let rule = band.marks.iter().find_map(|mark| match mark {
        Mark::Box(plate) if plate.at.h == 1.0 => Some(plate.at.y),
        _ => None,
    });
    assert_eq!(rule, Some(ROW_PT + 30.0 - 1.0), "the rule under the note");
    let note = piece(&manage.list, "manage_workspaces.Default.note");
    assert_eq!(note.row.as_ref().unwrap().label, text::MANAGE_DEFAULT_NOTE);
    for part in ["rename", "delete"] {
        let icon = piece(&manage.list, &format!("manage_workspaces.Default.{part}"));
        let row = icon.row.as_ref().unwrap();
        assert!(!icon.press && row.unavailable.is_some());
        assert_eq!(
            row.tip(false),
            Some("Default is a locked built-in template; use Save as")
        );
    }
    assert!(!piece(&manage.list, "manage_workspaces.Default.chip").press);
    assert!(!piece(&manage.list, "manage_workspaces.Default.name").double);
    assert_eq!(manage.names[0], ("Default".to_owned(), true));
}

/// Rename and delete are separate 24 x 24 presses 8 points apart, with
/// their hovers; a double click on the name renames.
#[test]
fn rename_and_delete_are_separate_targets_eight_points_apart() {
    let manage = view(&ManageDialog::new(1), false);
    let pencil = piece(&manage.list, "manage_workspaces.Scalp.rename");
    let trash = piece(&manage.list, "manage_workspaces.Scalp.delete");
    assert_eq!(
        (pencil.at.w, pencil.at.h, trash.at.w, trash.at.h),
        (24.0, 24.0, 24.0, 24.0)
    );
    assert!(trash.at.x - (pencil.at.x + pencil.at.w) >= 8.0);
    assert_eq!(
        pencil.row.as_ref().unwrap().tip(false),
        Some("Rename \u{b7} F2")
    );
    assert_eq!(
        trash.row.as_ref().unwrap().tip(false),
        Some("Delete \u{b7} Del")
    );
    let rename = Some(WorkspaceCommand::Manage(ManageIntent::BeginRename(
        "Scalp".to_owned(),
    )));
    assert_eq!(pencil.row.as_ref().unwrap().command, rename);
    let name = piece(&manage.list, "manage_workspaces.Scalp.name");
    assert!(name.double && name.row.as_ref().unwrap().command == rename);
    let chip = piece(&manage.list, "manage_workspaces.Scalp.chip");
    assert_eq!((chip.at.w, chip.at.h), (24.0, 24.0));
}

/// The rename field takes the name's place at the same height with the
/// hint; a refusal stands under the row; the dialog keeps its size.
#[test]
fn an_inline_rename_keeps_the_row_height_and_the_dialog_size() {
    let rest = view(&ManageDialog::new(1), true);
    let mut dialog = ManageDialog::new(1);
    dialog.local(&ManageIntent::BeginRename("Scalp".to_owned()));
    let renaming = view(&dialog, true);
    assert_eq!(renaming.list_height, rest.list_height);
    assert_eq!(renaming.size, rest.size, "the dialog never resizes");
    assert!(renaming.field.is_some());
    let row = piece(&renaming.list, "manage_workspaces.Scalp.rename");
    assert!(!row.press, "the pencil rests while the field is open");
    let why = row.row.as_ref().unwrap().tip(false);
    assert_eq!(
        why,
        Some("Finish renaming first \u{b7} Enter or Esc"),
        "its own reason"
    );
    dialog.refuse_rename("a workspace is already called Tape".to_owned());
    let refused = view(&dialog, true);
    assert!(refused.list_height > rest.list_height, "the list scrolls");
    assert_eq!(refused.size, rest.size);
    let said = piece(&refused.list, "manage_workspaces.Scalp.refusal");
    assert_eq!(
        said.row.as_ref().unwrap().label,
        "a workspace is already called Tape"
    );
}

/// An armed row asks at the same height, naming the tabs, with Delete (the
/// danger fill, carrying the time) and Keep.
#[test]
fn an_armed_delete_asks_in_the_row_at_the_same_height() {
    let mut dialog = ManageDialog::new(1);
    dialog.local(&ManageIntent::Arm("Scalp".to_owned()));
    let armed = view(&dialog, true);
    assert_eq!(armed.list_height, LIST_PT);
    let row = piece(&armed.list, "manage_workspaces.Scalp")
        .row
        .as_ref()
        .unwrap();
    assert_eq!(
        row.summary,
        "Delete Scalp? WIN$N and WDO$N go back to Default."
    );
    let confirm = piece(&armed.list, "manage_workspaces.Scalp.confirm");
    let delete = WorkspaceCommand::Manage(ManageIntent::Delete { now_ms: 1_000 });
    assert_eq!(confirm.row.as_ref().unwrap().command, Some(delete));
    assert!(matches!(&confirm.marks[0], Mark::Box(plate) if plate.fill == Some(Token::ErrorText)));
    let keep = piece(&armed.list, "manage_workspaces.Scalp.keep");
    assert_eq!(keep.row.as_ref().unwrap().label, "Keep");
    assert!(!ids(&armed.list).contains(&"manage_workspaces.Scalp.delete".to_owned()));
}

/// After a delete the Undo row stands where the workspace was, with
/// Ctrl+Z, Undo and the line that empties; at ten seconds it is gone.
#[test]
fn the_undo_row_stands_in_the_deleted_row_s_place() {
    let mut book = book();
    book.delete_at("Scalp", 0).unwrap();
    let mut dialog = ManageDialog::new(1);
    dialog.deleted(UndoRow {
        workspace: "Scalp".to_owned(),
        words: "Scalp deleted \u{b7} WIN$N and WDO$N back on Default".to_owned(),
        until_ms: 10_000,
    });
    let at = |now: i64| manage(&book, &dialog, ("Flow", now, true), &symbols, &measure);
    let half = at(5_000);
    let order = ids(&half.list);
    let place = |id: &str| order.iter().position(|seen| seen == id).unwrap();
    let undo = place("manage_workspaces.Scalp");
    assert!(place("manage_workspaces.Opening") < undo && undo < place("manage_workspaces.Swing"));
    let button = piece(&half.list, "manage_workspaces.Scalp.undo");
    let asked = WorkspaceCommand::Manage(ManageIntent::Undo { now_ms: 5_000 });
    assert_eq!(button.row.as_ref().unwrap().command, Some(asked));
    let row = piece(&half.list, "manage_workspaces.Scalp");
    let bar = row.marks.iter().rev().find_map(|mark| match mark {
        Mark::Box(plate) if plate.at.h == 2.0 => Some(plate.at.w),
        _ => None,
    });
    assert_eq!(bar, Some(260.0), "half the ten seconds left");
    assert!(!ids(&at(10_000).list).contains(&"manage_workspaces.Scalp.undo".to_owned()));
}

/// A chip opens that row's palette under it, indented to the name column,
/// its swatches listed under the row; the dialog keeps its size.
#[test]
fn a_chip_opens_the_same_palette_under_its_row() {
    let mut dialog = ManageDialog::new(1);
    dialog.local(&ManageIntent::Chip("Scalp".to_owned()));
    let open = view(&dialog, true);
    let (at, pop) = open.palette.as_ref().unwrap();
    assert_eq!(at[0], PAD - 2.0, "hung from the chip");
    let rest = view(&ManageDialog::new(1), true);
    assert_eq!(
        open.list_height, rest.list_height,
        "every row stays where it was"
    );
    let sky = piece(&pop.pieces, "manage_workspaces.Scalp.colour.sky");
    assert!(sky.row.as_ref().unwrap().on, "Scalp's own colour");
    let violet = piece(&pop.pieces, "manage_workspaces.Scalp.colour.violet");
    assert!(
        violet.press,
        "a held colour asks to swap, never locked here"
    );
    assert_eq!(open.size, view(&ManageDialog::new(1), true).size);
    assert!(open.closes_out, "a click outside it closes it");
    let foot = ids(&open.foot);
    assert!(foot.contains(&"manage_workspaces.new".to_owned()));
    assert!(foot.contains(&"manage_workspaces.close".to_owned()));
}

/// The chosen row is lit and named for the scroll.
#[test]
fn the_chosen_row_is_lit() {
    let mut dialog = ManageDialog::new(1);
    dialog.focus = Some(5);
    let focused = view(&dialog, true);
    assert_eq!(
        focused.focus.map(|at| at.y),
        Some(5.0 * ROW_PT + 30.0),
        "below Default's note"
    );
}

fn create(mode: &str) -> NameDialog {
    let mut menu = WorkspaceMenuState::default();
    match mode {
        "save_as" => menu.open_save_as(1),
        _ => menu.open_new(1),
    }
    let mut dialog = menu.dialog.unwrap();
    dialog.name = "Flow 2".to_owned();
    dialog.colour = Some("petrol".to_owned());
    dialog
}

/// Save as: its title, the field, "Colour · Petrol", the palette with
/// Petrol chosen and every held colour locked (hatched, initialled, no
/// press, sending the trader to Manage), Save the Enter default; under 420
/// points tall.
#[test]
fn the_create_dialog_holds_the_palette_on_the_next_free_colour() {
    let shown = name_dialog(&book(), &create("save_as"), true, &measure);
    let title = piece(&shown.list, "name_dialog").row.as_ref().unwrap();
    assert_eq!(title.label, "Save as new workspace");
    let caption = piece(&shown.list, "name_dialog.colour")
        .row
        .as_ref()
        .unwrap();
    assert_eq!(caption.label, "Colour \u{b7} Petrol");
    let (_, pop) = shown.palette.as_ref().unwrap();
    let petrol = piece(&pop.pieces, "name_dialog.colour.petrol");
    assert!(petrol.row.as_ref().unwrap().on && petrol.press);
    let sky = piece(&pop.pieces, "name_dialog.colour.sky");
    assert!(!sky.press);
    assert!(
        sky.marks
            .iter()
            .any(|mark| matches!(mark, Mark::Hatch { .. }))
    );
    assert_eq!(
        sky.row.as_ref().unwrap().tip(false),
        Some("Used by Scalp \u{b7} change it in Manage workspaces")
    );
    let violet = piece(&pop.pieces, "name_dialog.colour.violet");
    assert!(
        violet
            .marks
            .iter()
            .any(|mark| matches!(mark, Mark::Text(said) if said.text == "F"))
    );
    assert!(shown.saves);
    let save = piece(&shown.list, "name_dialog.save");
    assert!(save.press);
    assert_eq!(
        save.row.as_ref().unwrap().command,
        Some(WorkspaceCommand::SubmitDialog),
        "Enter saves"
    );
    assert!(shown.size[1] <= 420.0, "{}", shown.size[1]);
    let new = name_dialog(&book(), &create("new"), false, &measure);
    let title = piece(&new.list, "name_dialog").row.as_ref().unwrap();
    assert_eq!(title.label, "New workspace");
    assert_eq!(new.size[0], shown.size[0]);
    assert!(new.size[1] > shown.size[1], "room for the creation choices");
}

/// A locked swatch's initials are pure black or white and read at least
/// 4.5:1 on the veiled colour they stand on, in both themes.
#[test]
fn locked_initials_read_on_their_veiled_swatch_in_both_themes() {
    use quantick_appearance::{PremultipliedRgba, contrast::contrast_ratio};
    for dark in [true, false] {
        let shown = name_dialog(&book(), &create("save_as"), dark, &measure);
        let (_, pop) = shown.palette.as_ref().unwrap();
        for key in [
            "cobalt", "orchid", "sky", "violet", "slate", "fuchsia", "ice", "blossom",
        ] {
            let swatch = piece(&pop.pieces, &format!("name_dialog.colour.{key}"));
            let colour = swatch.row.as_ref().unwrap().colour.as_ref().unwrap().rgb;
            let under = crate::colour_palette::veiled(colour, dark);
            let words = swatch.marks.iter().find_map(|mark| match mark {
                Mark::Text(said) if said.rgb.is_some() => said.rgb,
                _ => None,
            });
            let [r, g, b] = words.expect("initials in black or white");
            assert!(r == g && g == b && (r == 0 || r == 255), "{key}");
            let on = |[r, g, b]: [u8; 3]| PremultipliedRgba::from_rgb(r, g, b);
            let ratio = contrast_ratio(on([r, g, b]), on(under));
            assert!(ratio >= 4.5, "{key} dark {dark}: {ratio}");
        }
    }
}

/// Custom identical to a held colour disables Save and Apply and says
/// whose colour it is; a free one saves.
#[test]
fn custom_identical_to_a_held_colour_disables_save_with_its_reason() {
    let mut dialog = create("save_as");
    dialog.palette = ColourPopover {
        custom: true,
        hex: "#5DBDEB".to_owned(),
        ..ColourPopover::default()
    };
    let shown = name_dialog(&book(), &dialog, true, &measure);
    assert!(!shown.saves);
    assert!(!piece(&shown.list, "name_dialog.save").press);
    let (_, pop) = shown.palette.as_ref().unwrap();
    assert!(!piece(&pop.pieces, "name_dialog.colour.apply").press);
    let reason = piece(&pop.pieces, "name_dialog.colour.hint")
        .row
        .as_ref()
        .unwrap();
    assert_eq!(reason.label, "Used by Scalp. Pick another colour.");
    let caption = piece(&shown.list, "name_dialog.colour")
        .row
        .as_ref()
        .unwrap();
    assert_eq!(caption.label, "Colour \u{b7} Petrol", "until Apply");
    dialog.palette.hex = "#3A7BD6".to_owned();
    assert!(name_dialog(&book(), &dialog, true, &measure).saves);
    let rest = name_rest_height(&book(), &dialog);
    assert!(
        rest < name_dialog(&book(), &dialog, true, &measure).size[1],
        "Custom grows it"
    );
    let capped = capped(name_dialog(&book(), &dialog, true, &measure), 300.0);
    assert_eq!(
        (capped.size[1], capped.list_view),
        (300.0, 300.0),
        "the rest scrolls"
    );
    assert!(capped.list_height > 300.0);
}

/// Rename keeps its dialog without a palette.
#[test]
fn rename_asks_only_the_name() {
    let mut menu = WorkspaceMenuState::default();
    menu.open_rename(1, "Scalp");
    let shown = name_dialog(&book(), menu.dialog.as_ref().unwrap(), true, &measure);
    assert!(shown.palette.is_none());
    assert_eq!(
        piece(&shown.list, "name_dialog")
            .row
            .as_ref()
            .unwrap()
            .label,
        "Rename workspace"
    );
    assert!(shown.saves);
}

/// A frame's keys win over its presses; Enter submits only while Save would
/// save, and a typed change clears the refusal.
#[test]
fn a_frame_s_keys_and_presses_answer_one_command() {
    let mut dialog = create("save_as");
    dialog.refusal = Some("taken".to_owned());
    let none = name_answer(&mut dialog, "Flow 2", None, ([true, true], false, false));
    assert_eq!(none, None, "Save would not save");
    let saved = name_answer(&mut dialog, "Flow", None, ([true, true], true, false));
    assert_eq!(saved, Some(WorkspaceCommand::SubmitDialog));
    assert_eq!(dialog.refusal, None, "the name changed");
    let after_click = name_answer(&mut dialog, "Flow 2", None, ([false, true], true, false));
    assert_eq!(
        after_click,
        Some(WorkspaceCommand::SubmitDialog),
        "Enter off the field saves"
    );
    dialog.palette.custom = true;
    let apply = name_answer(&mut dialog, "Flow 2", None, ([true, true], true, false));
    assert_eq!(
        apply,
        Some(WorkspaceCommand::NameColour(ColourIntent::Apply))
    );
    dialog.palette.custom = false;
    let esc = name_answer(&mut dialog, "Flow 2", None, ([false, false], true, true));
    assert_eq!(esc, Some(WorkspaceCommand::CloseDialog));
    let keep = Some(ManageIntent::Keep);
    let pick = Some(WorkspaceCommand::Colour(ColourIntent::Pick(
        "sky".to_owned(),
    )));
    assert_eq!(
        manage_answer(keep.clone(), pick.clone(), true),
        Some(WorkspaceCommand::Manage(ManageIntent::Keep))
    );
    assert_eq!(
        manage_answer(None, pick, true),
        Some(WorkspaceCommand::Manage(ManageIntent::Colour(
            ColourIntent::Pick("sky".to_owned())
        )))
    );
    assert_eq!(
        manage_answer(None, None, true),
        Some(WorkspaceCommand::Manage(ManageIntent::CommitRename))
    );
}

/// A palette's press becomes the press of the place it stands in.
#[test]
fn a_palette_press_is_routed_to_its_place() {
    let pick = WorkspaceCommand::Colour(ColourIntent::Pick("sky".to_owned()));
    assert_eq!(
        row_colour(pick.clone()),
        WorkspaceCommand::Manage(ManageIntent::Colour(ColourIntent::Pick("sky".to_owned())))
    );
    assert_eq!(
        name_colour(pick),
        WorkspaceCommand::NameColour(ColourIntent::Pick("sky".to_owned()))
    );
    let mut dialog = create("new");
    assert_eq!(name_escape(&dialog), WorkspaceCommand::CloseDialog);
    dialog.palette.custom = true;
    assert_eq!(
        name_escape(&dialog),
        WorkspaceCommand::NameColour(ColourIntent::Close)
    );
}

/// Manage tints each row toward its colour as menu B does, over the list's
/// ground, and its words are chosen on that tint as menu B's are.
#[test]
fn every_row_wears_its_colour_and_its_words_read_on_it() {
    use quantick_appearance::workspace_menu::{RowTint, RowWords, chrome};
    use quantick_stores::workspaces::WorkspaceColour;
    let shown = view(&ManageDialog::new(1), true);
    let sky = WorkspaceColour::preset(2).unwrap().rgb(true);
    let at = (shown.list.iter()).position(|piece| {
        piece
            .row
            .as_ref()
            .is_some_and(|row| row.scene_id() == "manage_workspaces.Scalp.name")
    });
    let summary = &shown.list[at.expect("Scalp's name") + 1];
    let Some(Mark::Text(words)) = summary.marks.first() else {
        panic!("the summary")
    };
    let row = RowWords {
        ground: chrome(true).ground,
        colour: sky,
        current: false,
    };
    assert_eq!(words.on, Some(row), "the summary is chosen on Sky's tint");
    let band = (shown.list.iter()).find_map(|piece| match piece.marks.first() {
        Some(Mark::Box(plate)) if piece.row.as_ref().is_some_and(|row| row.label == "Scalp") => {
            Some((plate.fill, plate.toward))
        }
        _ => None,
    });
    let rest = (
        Some(chrome(true).ground),
        Some((sky, RowTint::Rest.share())),
    );
    assert_eq!(
        band,
        Some(rest),
        "Scalp's own colour over the ground, at rest"
    );
}

/// The name dialog's top stays where it opens while Custom grows it, and
/// what does not fit the window scrolls inside it.
#[test]
fn the_name_dialog_keeps_its_top_and_fits_the_window() {
    let book = book();
    let mut dialog = create("save_as");
    let screen = [0.0, 900.0];
    let (top, room) = name_place(&book, &dialog, screen);
    dialog.palette.custom = true;
    assert_eq!(
        name_place(&book, &dialog, screen),
        (top, room),
        "Custom grows it downward"
    );
    assert_eq!(room, screen[1] - DIALOG_MARGIN_PT - top);
    let (short_top, short_room) = name_place(&book, &dialog, [0.0, 300.0]);
    assert_eq!(
        short_top, DIALOG_MARGIN_PT,
        "never above the window's margin"
    );
    assert_eq!(short_room, 300.0 - 2.0 * DIALOG_MARGIN_PT);
}

#[test]
fn new_keeps_focus_on_the_selected_choice_and_submits_only_a_valid_draft() {
    let mut dialog = create("new");
    dialog.palette.custom = true;
    for hex in ["#invalid", "#5DBDEB"] {
        dialog.palette.hex = hex.to_owned();
        let shown = name_dialog(&book(), &dialog, true, &measure);
        assert!(!piece(&shown.list, "name_dialog.save").press);
        assert_eq!(
            name_answer(&mut dialog, "", None, ([false, true], shown.saves, false)),
            None
        );
    }
    dialog.palette.hex = "#123456".to_owned();
    let shown = name_dialog(&book(), &dialog, false, &measure);
    assert!(shown.saves);
    let pick = WorkspaceCommand::CreationCharts(3);
    assert_eq!(
        name_answer(
            &mut dialog,
            "",
            Some(pick.clone()),
            ([false, true], true, false)
        ),
        Some(pick)
    );
    assert_eq!(dialog.palette.hex, "#123456");
    assert_eq!(
        name_answer(&mut dialog, "", None, ([false, true], true, false)),
        Some(WorkspaceCommand::SubmitDialog)
    );
}
