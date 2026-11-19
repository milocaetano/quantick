//! New workspace choices, rendered by the existing name-dialog sheet.
use super::*;
use quantick_engine::bar_registry::{TIME_PRESETS, fmt_time_interval};
use quantick_workspace::canvas::presets::timeframes_with;

pub(super) fn controls(
    pieces: &mut Vec<Piece>,
    y: &mut f32,
    dialog: &NameDialog,
    dark: bool,
    measure: Measure<'_>,
) {
    if dialog.mode != NameDialogMode::New {
        return;
    }
    let ink = chrome(dark);
    let width = NAME_WIDTH_PT - PAD * 2.0;
    pieces.push(Piece::plain(
        area(PAD, *y, width, LINE_PT),
        vec![words(
            PAD,
            *y + LINE_PT / 2.0,
            "Timeframe",
            BODY_PT,
            ink.ink,
            width,
        )],
    ));
    *y += LINE_PT;
    let mut choices: Vec<(String, u64)> = TIME_PRESETS
        .iter()
        .map(|(label, ms)| ((*label).to_owned(), *ms as u64))
        .collect();
    if !choices
        .iter()
        .any(|(_, ms)| *ms == dialog.creation.timeframe_ms)
    {
        choices.push((
            fmt_time_interval(dialog.creation.timeframe_ms as i64),
            dialog.creation.timeframe_ms,
        ));
    }
    for (index, (label, ms)) in choices.iter().enumerate() {
        let at = area(
            PAD + (index % 4) as f32 * width / 4.0,
            *y + (index / 4) as f32 * (HIT + 4.0),
            width / 4.0 - 4.0,
            HIT,
        );
        pieces.push(choice(
            at,
            &format!("timeframe.{ms}"),
            label,
            WorkspaceCommand::CreationTimeframe(*ms),
            *ms == dialog.creation.timeframe_ms,
            &ink,
            measure,
        ));
    }
    *y += choices.len().div_ceil(4) as f32 * (HIT + 4.0) + 4.0;
    pieces.push(Piece::plain(
        area(PAD, *y, width, LINE_PT),
        vec![words(
            PAD,
            *y + LINE_PT / 2.0,
            "Charts",
            BODY_PT,
            ink.ink,
            width,
        )],
    ));
    *y += LINE_PT;
    for count in 1..=8 {
        let at = area(
            PAD + (count - 1) as f32 * width / 8.0,
            *y,
            width / 8.0 - 4.0,
            HIT,
        );
        pieces.push(choice(
            at,
            &format!("charts.{count}"),
            &count.to_string(),
            WorkspaceCommand::CreationCharts(count),
            count == dialog.creation.charts,
            &ink,
            measure,
        ));
    }
    *y += HIT + 8.0;
    if let Some(preset) = timeframes_with(dialog.creation.charts) {
        pieces.push(Piece::plain(
            area(PAD, *y, width, 36.0),
            vec![
                Mark::Tree(area(PAD, *y, 64.0, 32.0), preset.tree.clone()),
                words(
                    PAD + 76.0,
                    *y + 16.0,
                    "Empty layout · name assigned automatically",
                    SMALL_PT,
                    ink.support,
                    width - 76.0,
                ),
            ],
        ));
    }
    *y += 40.0;
}

fn choice(
    at: Area,
    key: &str,
    label: &str,
    command: WorkspaceCommand,
    selected: bool,
    ink: &MenuChrome,
    measure: Measure<'_>,
) -> Piece {
    let row = CellRow {
        scope: NAME_SCOPE,
        on: selected,
        command: Some(command),
        capability: Some(calls::WORKSPACE_NEW_CAPABILITY_ID),
        ..CellRow::new(RowKind::Press, key, label)
    };
    let mut piece = press(
        at,
        row,
        true,
        vec![words(
            at.x + (at.w - measure(label, BODY_PT)) / 2.0,
            at.y + at.h / 2.0,
            label,
            BODY_PT,
            ink.ink,
            at.w,
        )],
        ink,
    );
    piece.marks.insert(
        0,
        plate(
            at,
            Some(ink.control),
            Some(if selected { ink.accent } else { ink.edge }),
            u8::MAX,
        ),
    );
    piece
}

pub(super) fn colour(
    pieces: &mut Vec<Piece>,
    y: &mut f32,
    colour: Option<WorkspaceColour>,
    dark: bool,
    measure: Measure<'_>,
) {
    let ink = chrome(dark);
    let width = NAME_WIDTH_PT - PAD * 2.0;
    let at = area(PAD, *y, 36.0, 24.0);
    let marks = match colour {
        Some(colour) => vec![
            Mark::Colour {
                at,
                radius: 4.0,
                rgb: colour.rgb(dark),
            },
            colour_palette::selection_check(at, colour.rgb(dark), dark, measure),
            words(
                PAD + 46.0,
                *y + 12.0,
                &format!("Selected · {}", said(colour)),
                BODY_PT,
                ink.ink,
                width - 46.0,
            ),
        ],
        None => vec![words(
            PAD,
            *y + 12.0,
            "Enter a valid #RRGGBB color",
            BODY_PT,
            Token::Warn,
            width,
        )],
    };
    let row = CellRow {
        scope: NAME_SCOPE,
        on: colour.is_some(),
        colour: colour.map(|colour| RowColour::of(colour, dark)),
        ..CellRow::new(RowKind::Current, "selected_colour", "Selected color")
    };
    pieces.push(Piece {
        row: Some(row),
        ..Piece::plain(area(PAD, *y, width, 28.0), marks)
    });
    *y += 32.0;
}
