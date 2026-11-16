//! The layout picker's grid, words, states, colour and gestures, decided
//! without a window.
use super::*;
use quantick_appearance::contrast::contrast_ratio;
use quantick_workspace::canvas::preset;
use quantick_workspace::workspace_colour::{PRESETS, WorkspaceColour};

/// Ubuntu Light's and Ubuntu Bold's advance widths for ASCII 32 to 126, in
/// thousandths of an em: the faces egui's proportional text and the app's
/// bold family draw, read from the bundled TTFs.
const LIGHT_ADVANCE: [u16; 95] = [
    228, 274, 392, 653, 564, 831, 648, 240, 310, 310, 472, 564, 246, 282, 246, 359, 564, 564, 564,
    564, 564, 564, 564, 564, 564, 564, 246, 246, 564, 564, 564, 380, 940, 641, 630, 609, 702, 555,
    520, 658, 692, 248, 487, 599, 502, 865, 709, 774, 592, 774, 618, 517, 548, 680, 624, 921, 608,
    569, 560, 310, 359, 310, 564, 490, 376, 509, 583, 460, 583, 548, 376, 573, 569, 237, 237, 495,
    255, 863, 569, 582, 583, 583, 374, 428, 387, 569, 480, 775, 499, 474, 455, 319, 262, 319, 564,
];
const BOLD_ADVANCE: [u16; 95] = [
    240, 286, 465, 699, 568, 918, 705, 247, 356, 356, 502, 568, 246, 340, 246, 437, 568, 568, 568,
    568, 568, 568, 568, 568, 568, 568, 246, 246, 568, 568, 568, 455, 974, 721, 672, 648, 737, 606,
    574, 702, 734, 316, 529, 684, 563, 897, 756, 790, 644, 790, 667, 582, 614, 707, 722, 948, 675,
    661, 610, 371, 437, 371, 568, 500, 286, 553, 604, 500, 604, 584, 422, 594, 589, 289, 289, 579,
    316, 862, 589, 607, 604, 604, 422, 485, 444, 589, 550, 784, 554, 547, 500, 371, 322, 371, 568,
];

/// `text`'s width at `size` points in `face`; a character outside ASCII
/// counts as the face's widest.
fn width(text: &str, size: f32, face: &[u16; 95]) -> f32 {
    let widest = face.iter().copied().max().unwrap_or(1000);
    let em: u32 = text
        .chars()
        .map(|c| match u32::from(c) {
            code @ 32..=126 => u32::from(face[(code - 32) as usize]),
            _ => u32::from(widest),
        })
        .sum();
    em as f32 / 1000.0 * size
}

fn themes() -> [&'static ResolvedTheme; 2] {
    [ResolvedTheme::bundled(true), ResolvedTheme::bundled(false)]
}

const VIOLET: WorkspaceColour = WorkspaceColour::Preset(3);

fn view<'a>(state: &'a State, marks: LayoutMarks) -> View<'a> {
    View {
        workspace: "Flow",
        colour: Some(VIOLET),
        marks,
        state,
    }
}

fn marks(current: &str, saved: &str, unsaved: bool) -> LayoutMarks {
    LayoutMarks {
        current: preset(current),
        saved: preset(saved),
        layout_unsaved: unsaved,
    }
}

fn cell<'a>(picker: &'a Picker, id: &str) -> &'a Cell {
    picker
        .cells
        .iter()
        .find(|cell| cell.preset.id == id)
        .unwrap_or_else(|| panic!("{id} has a cell"))
}

/// The sixteen cells sit on one 4 x 4 grid of equal cells, WITH FLOW in the
/// left two columns and TIMEFRAMES in the right two, at least 12 pt apart,
/// a divider no taller than the grid between them, all inside the popover.
#[test]
fn the_sixteen_cells_sit_on_one_grid_of_equal_cells() {
    let state = State::default();
    for theme in themes() {
        let picker = picker(&view(&state, marks("flow", "flow", false)), theme);
        assert_eq!(picker.cells.len(), LAYOUT_PRESETS.len());
        assert_eq!(picker.cells.len(), COLUMNS * ROWS);
        let mut xs: Vec<f32> = picker.cells.iter().map(|cell| cell.at.x).collect();
        let mut ys: Vec<f32> = picker.cells.iter().map(|cell| cell.at.y).collect();
        xs.sort_by(f32::total_cmp);
        xs.dedup();
        ys.sort_by(f32::total_cmp);
        ys.dedup();
        assert_eq!((xs.len(), ys.len()), (COLUMNS, ROWS), "{xs:?} {ys:?}");
        for cell in &picker.cells {
            assert_eq!([cell.at.w, cell.at.h], CELL, "{}", cell.preset.id);
            assert!(cell.at.x >= 0.0 && cell.at.x + cell.at.w <= picker.size[0]);
            assert!(cell.at.y >= 0.0 && cell.at.y + cell.at.h <= picker.size[1]);
            let column = usize::from(cell.preset.group == PresetGroup::Timeframes);
            let (row, col) = place(cell.preset).unwrap();
            assert_eq!(col / 2, column, "{} sits in its group", cell.preset.id);
            assert_eq!([cell.at.x, cell.at.y], [xs[col], ys[row]]);
        }
        assert!(xs[2] - (xs[1] + CELL[0]) >= 12.0, "groups apart: {xs:?}");
        let grid = (ys[0], ys[3] + CELL[1]);
        let rules: Vec<&Rect> = picker
            .marks
            .iter()
            .filter_map(|mark| match mark {
                Mark::Fill(Plate { at, .. }) if at.w == 1.0 && at.x > xs[1] && at.x < xs[2] => {
                    Some(at)
                }
                _ => None,
            })
            .collect();
        assert_eq!(rules.len(), 1, "one divider");
        assert!(rules[0].y >= grid.0 && rules[0].y + rules[0].h <= grid.1);
    }
}

/// Every display name stands on one line inside its cell, regular and bold,
/// measured with the faces the app draws them in.
#[test]
fn every_label_fits_one_line_of_its_cell_in_both_weights() {
    let room = CELL[0] - 2.0 * RING_PT;
    for preset in LAYOUT_PRESETS {
        assert!(!preset.label.contains('\n'), "{}", preset.id);
        for (face, name) in [(&LIGHT_ADVANCE, "light"), (&BOLD_ADVANCE, "bold")] {
            let wide = width(preset.label, LABEL_PT, face);
            assert!(wide <= room, "{} {name}: {wide} > {room}", preset.label);
        }
    }
    // The header's status slot holds its longest words.
    let longest = LAYOUT_PRESETS
        .iter()
        .map(|p| p.label)
        .max_by_key(|l| l.len());
    let saved = format!("Saved: {}", longest.unwrap());
    assert!(width(&saved, SMALL_PT, &LIGHT_ADVANCE) <= STATUS_W - STATUS_ICON_W);
    let undo = format!("was {} \u{b7} Ctrl+Z", longest.unwrap());
    assert!(width(&undo, SMALL_PT, &LIGHT_ADVANCE) <= STATUS_W);
}

/// The size is the content's in every state: resting, unsaved, hovered,
/// pressed, focused and after a pick.
#[test]
fn the_size_is_the_contents_in_every_state() {
    let flow = preset("flow");
    let states = [
        State::default(),
        State {
            hover: flow.map(Target::Cell),
            pressed: flow.map(Target::Cell),
            focus: preset("charts-8"),
            ..State::default()
        },
        State {
            changed: Some(Changed {
                to: preset("flow-over-2time").unwrap(),
                was: preset("stack3+flow"),
                at_ms: 0,
            }),
            ..State::default()
        },
    ];
    for theme in themes() {
        for state in &states {
            for unsaved in [false, true] {
                let picker = picker(&view(state, marks("time", "flow", unsaved)), theme);
                assert_eq!(picker.size, SIZE);
            }
        }
    }
}

/// Current, hover, pressed and focus each paint differently: the current by
/// its ring in the workspace colour, its ✓ and its bold name; hover by the
/// grey plate; pressed by a deeper plate and a 1 pt sink; focus by a dashed
/// ring outside the cell in an ink that is not the workspace colour.
#[test]
fn current_hover_pressed_and_focus_differ_visibly() {
    for theme in themes() {
        let state = State {
            hover: preset("charts-2").map(Target::Cell),
            pressed: preset("charts-2").map(Target::Cell),
            focus: preset("charts-3"),
            ..State::default()
        };
        let pressed_picker = picker(&view(&state, marks("time+flow", "flow", true)), theme);
        let hover_state = State {
            hover: preset("charts-2").map(Target::Cell),
            focus: preset("charts-3"),
            ..State::default()
        };
        let hover_picker = picker(&view(&hover_state, marks("time+flow", "flow", true)), theme);
        let look = look::chrome(theme.dark);
        let ink = |token| theme.color(token).to_array();
        let fills = |cell: &Cell| -> Vec<[u8; 4]> {
            cell.marks
                .iter()
                .filter_map(|mark| match mark {
                    Mark::Fill(Plate { at, rgba, .. }) if *at == cell.at => Some(*rgba),
                    _ => None,
                })
                .collect()
        };
        let current = cell(&pressed_picker, "time+flow");
        let inks = colour_inks(VIOLET.rgb(theme.dark), theme.color(look.ground), theme.dark);
        assert!(current.current);
        assert!(current.marks.iter().any(|mark| matches!(mark,
            Mark::Edge(Plate { rgba, .. }, width) if *width == RING_PT && *rgba == inks.ring.to_array())));
        assert!(current.marks.iter().any(|mark| matches!(mark,
            Mark::Words(Words { text, .. }) if text == CHECK)));
        assert!(current.marks.iter().any(|mark| matches!(mark,
            Mark::Words(Words { text, bold: true, .. }) if text == "1 TF + Flow")));
        let hovered = cell(&hover_picker, "charts-2");
        assert_eq!(fills(hovered), vec![ink(look.hover)]);
        let pressed = cell(&pressed_picker, "charts-2");
        assert_eq!(fills(pressed), vec![look::pressed(theme).to_array()]);
        let label_y = |cell: &Cell| {
            cell.marks.iter().find_map(|mark| match mark {
                Mark::Words(Words { y, text, .. }) if text == cell.preset.label => Some(*y),
                _ => None,
            })
        };
        assert_eq!(
            label_y(pressed).zip(label_y(hovered)).map(|(p, h)| p - h),
            Some(SINK_PT),
            "a press sinks the cell"
        );
        let focused = cell(&hover_picker, "charts-3");
        let dashes: Vec<&Rect> = focused
            .marks
            .iter()
            .filter_map(|mark| match mark {
                Mark::Fill(Plate { at, rgba, .. }) if *rgba == ink(look.focus) => Some(at),
                _ => None,
            })
            .collect();
        assert!(dashes.len() > 8, "a dashed ring");
        let out = FOCUS_GAP_PT;
        for dash in dashes {
            let inside = dash.x >= focused.at.x - out + 0.5
                && dash.x + dash.w <= focused.at.x + focused.at.w + out - 0.5
                && dash.y >= focused.at.y - out + 0.5
                && dash.y + dash.h <= focused.at.y + focused.at.h + out - 0.5;
            assert!(!inside, "every dash stands outside the cell: {dash:?}");
        }
        assert_ne!(ink(look.focus), inks.ring.to_array());
        let resting = cell(&hover_picker, "charts-4");
        assert!(fills(resting).is_empty() && !resting.current);
    }
}

/// Every preset and a custom colour, in both themes: the ring, the flow
/// edge and the ✓ hold 3:1 on the popover, the flow fill 1.5:1 on an empty
/// chart cell, else the edge widens; timeframe edges are the ring at 60%.
#[test]
fn every_preset_and_a_custom_colour_pass_the_clamp_in_both_themes() {
    let customs = [
        [255, 255, 255],
        [0, 0, 0],
        [0x17, 0x1B, 0x26],
        [0x80, 0x80, 0x80],
    ];
    let colours = (0..PRESETS.len() as u8)
        .filter_map(WorkspaceColour::preset)
        .chain(customs.map(WorkspaceColour::Custom));
    for colour in colours {
        for theme in themes() {
            let name = format!("{} {}", theme.name, colour.describe(theme.dark));
            let ground = theme.color(look::chrome(theme.dark).ground);
            let inks = colour_inks(colour.rgb(theme.dark), ground, theme.dark);
            let ring = contrast_ratio(inks.ring, ground);
            assert!(ring >= look::RING_CONTRAST, "{name} ring {ring}");
            let check = contrast_ratio(inks.check, inks.ring);
            assert!(check >= look::RING_CONTRAST, "{name} check {check}");
            match inks.fill {
                Some(fill) => {
                    let ratio = contrast_ratio(fill, ground);
                    assert!(ratio >= look::FILL_CONTRAST, "{name} fill {ratio}");
                    assert_eq!(inks.flow_edge, look::FLOW_EDGE_PT, "{name}");
                }
                None => assert_eq!(inks.flow_edge, look::FLOW_EDGE_BARE_PT, "{name}"),
            }
            let edge = quantick_appearance::contrast::over(inks.time_edge, ground);
            let ratio = contrast_ratio(edge, ground);
            assert!(
                ratio >= look::RING_CONTRAST,
                "{name} timeframe edge {ratio}"
            );
            assert!(
                inks.time_edge.to_array()[3] >= look::TIME_EDGE_ALPHA,
                "{name}"
            );
        }
    }
}

/// One display name per layout: the cell, the header after a pick and the
/// read all say the registry's label.
#[test]
fn one_display_name_per_layout_in_cell_header_and_read() {
    let theme = ResolvedTheme::bundled(false);
    for preset in LAYOUT_PRESETS {
        let state = State {
            changed: Some(Changed {
                to: preset,
                was: super::preset_at(0, 0),
                at_ms: 0,
            }),
            ..State::default()
        };
        let marks = LayoutMarks {
            current: Some(preset),
            saved: Some(preset),
            layout_unsaved: false,
        };
        let shown = view(&state, marks);
        let picker = picker(&shown, theme);
        let drawn = cell(&picker, preset.id);
        assert!(drawn.marks.iter().any(|mark| matches!(mark,
            Mark::Words(Words { text, .. }) if text == preset.label)));
        assert_eq!(
            picker.header.line,
            format!("{CHANGED} {}", preset.label),
            "header"
        );
        assert_eq!(picker.header.status, format!("{SAVED} {}", preset.label));
        let read = read(shown.workspace, shown.marks, shown.state, true);
        let row = read.cells.iter().find(|row| row.preset_id == preset.id);
        assert_eq!(row.map(|row| row.name.as_str()), Some(preset.label));
        assert_eq!(read.changed.as_deref(), Some(picker.header.line.as_str()));
    }
}

/// The header says the save state in its fixed slot: Saved with the
/// layout's name, or Not saved with Ctrl+S, never hidden by a pick.
#[test]
fn the_header_never_hides_the_unsaved_state() {
    let theme = ResolvedTheme::bundled(true);
    let rest = State::default();
    let saved = picker(&view(&rest, marks("flow", "flow", false)), theme);
    assert_eq!(saved.header.status, "Saved: Flow");
    assert_eq!(saved.header.line, HINT);
    let after = State {
        changed: Some(Changed {
            to: preset("time+time+flow").unwrap(),
            was: preset("time+flow"),
            at_ms: 0,
        }),
        ..State::default()
    };
    let unsaved = picker(&view(&after, marks("time+time+flow", "flow", true)), theme);
    assert_eq!(unsaved.header.status, NOT_SAVED);
    assert_eq!(unsaved.header.line, "Changed to 2 TF + Flow");
    let undo = unsaved
        .header
        .undo
        .as_ref()
        .map(|(_, words)| words.as_str());
    assert_eq!(undo, Some("was 1 TF + Flow \u{b7} Ctrl+Z"));
    assert!(unsaved.header.label().contains(NOT_SAVED));
    assert!(unsaved.header.label().contains("Changed to 2 TF + Flow"));
    assert_eq!(saved.header.at, unsaved.header.at, "the slot never moves");
}

fn seen(now_ms: u64, current: &str) -> Seen {
    Seen {
        now_ms,
        current: preset(current),
        ..Seen::default()
    }
}

/// A press commits on release inside the same cell; dragging off cancels.
#[test]
fn a_pick_commits_on_release_inside_and_dragging_off_cancels() {
    let target = preset("charts-2").map(Target::Cell);
    let mut state = State::default();
    let press = Seen {
        over: target,
        press: true,
        ..seen(0, "flow")
    };
    assert_eq!(state.step(press), Step::default());
    let off = Seen {
        over: preset("charts-3").map(Target::Cell),
        release: true,
        ..seen(10, "flow")
    };
    assert_eq!(state.step(off), Step::default(), "released elsewhere");
    assert_eq!(state.pressed, None);
    state.step(press);
    let release = Seen {
        over: target,
        release: true,
        ..seen(20, "flow")
    };
    let step = state.step(release);
    assert_eq!(step.pick, preset("charts-2"));
    assert!(!step.close, "it stays open to confirm");
    let changed = state.changed.expect("the pick is confirmed");
    assert_eq!(
        (changed.to.id, changed.was.map(|p| p.id)),
        ("charts-2", Some("flow"))
    );
}

/// After a pick the popover stays open about 800 ms, then closes; Esc and a
/// press outside close at once.
#[test]
fn the_confirmation_closes_after_its_time_and_esc_closes_at_once() {
    let mut state = State {
        changed: Some(Changed {
            to: preset("charts-2").unwrap(),
            was: preset("flow"),
            at_ms: 1_000,
        }),
        ..State::default()
    };
    assert!(!state.step(seen(1_000 + CONFIRM_MS - 1, "charts-2")).close);
    assert!(state.step(seen(1_000 + CONFIRM_MS, "charts-2")).close);
    assert_eq!(state, State::default(), "closing forgets the pick");
    let mut held = State {
        held: true,
        ..state.clone()
    };
    held.changed = Some(Changed {
        to: preset("charts-2").unwrap(),
        was: None,
        at_ms: 0,
    });
    assert!(!held.step(seen(60_000, "charts-2")).close, "a held capture");
    let escape = Seen {
        keys: Keys {
            escape: true,
            ..Keys::default()
        },
        ..seen(0, "flow")
    };
    assert!(State::default().step(escape).close);
    let outside = Seen {
        outside: true,
        ..seen(0, "flow")
    };
    assert!(State::default().step(outside).close);
}

/// Ctrl+Z, or a click on the header's "was" words, restores the layout the
/// pick replaced; with nothing picked it does nothing.
#[test]
fn ctrl_z_or_the_was_words_restore_the_previous_layout() {
    let picked = State {
        changed: Some(Changed {
            to: preset("charts-2").unwrap(),
            was: preset("flow"),
            at_ms: 0,
        }),
        ..State::default()
    };
    let undo = Seen {
        keys: Keys {
            undo: true,
            ..Keys::default()
        },
        ..seen(100, "charts-2")
    };
    let mut state = picked.clone();
    assert_eq!(state.step(undo).pick, preset("flow"));
    let back = state.changed.unwrap();
    assert_eq!(
        (back.to.id, back.was.map(|p| p.id)),
        ("flow", Some("charts-2"))
    );
    let mut state = picked.clone();
    let press = Seen {
        over: Some(Target::Undo),
        press: true,
        ..seen(100, "charts-2")
    };
    state.step(press);
    let release = Seen {
        over: Some(Target::Undo),
        release: true,
        ..seen(120, "charts-2")
    };
    assert_eq!(state.step(release).pick, preset("flow"));
    assert_eq!(State::default().step(undo).pick, None);
}

/// Arrows walk the 4 x 4 grid, crossing between the groups; Enter picks the
/// focused cell; picking the current layout closes without a change.
#[test]
fn arrows_cross_the_groups_and_enter_picks() {
    let key = |keys: Keys| Seen {
        keys,
        ..seen(0, "time+flow")
    };
    let right = key(Keys {
        right: true,
        ..Keys::default()
    });
    let mut state = State::default();
    state.step(right);
    assert_eq!(
        state.focus,
        preset("time+flow"),
        "the first arrow lands on the current cell"
    );
    state.step(right);
    assert_eq!(
        state.focus,
        preset("time"),
        "right of WITH FLOW is TIMEFRAMES"
    );
    let left = key(Keys {
        left: true,
        ..Keys::default()
    });
    state.step(left);
    assert_eq!(state.focus, preset("time+flow"));
    let down = key(Keys {
        down: true,
        ..Keys::default()
    });
    for _ in 0..5 {
        state.step(down);
    }
    assert_eq!(
        state.focus,
        preset("flow-over-2time"),
        "held at the last row"
    );
    let enter = key(Keys {
        enter: true,
        ..Keys::default()
    });
    assert_eq!(state.step(enter).pick, preset("flow-over-2time"));
    let mut same = State {
        focus: preset("time+flow"),
        ..State::default()
    };
    let step = same.step(enter);
    assert_eq!(
        (step.pick, step.close),
        (None, true),
        "the current one closes"
    );
}

/// The popover hangs below its button, right edges flush, and never climbs
/// over the toolbar: a short window scrolls it instead, a narrow one shifts it.
#[test]
fn it_hangs_below_its_button_and_scrolls_in_a_short_window() {
    let button = Rect::new(1200.0, 40.0, 54.0, 22.0);
    let screen = Rect::new(0.0, 0.0, 1600.0, 1000.0);
    let tall = place_under(button, screen);
    assert_eq!(tall.x + OUTER[0], button.x + button.w);
    assert_eq!(tall.y, button.y + button.h + DROP_PT);
    assert_eq!((tall.height, tall.scrolls), (OUTER[1], false));
    let short = place_under(button, Rect::new(0.0, 0.0, 1600.0, 250.0));
    assert_eq!(short.y, tall.y, "it never moves up");
    assert!(short.scrolls && short.height < OUTER[1]);
    assert!(short.y + short.height <= 250.0);
    let narrow = place_under(Rect::new(100.0, 40.0, 54.0, 22.0), screen);
    assert!(narrow.x >= 0.0 && narrow.y == tall.y);
}

/// A narrow window keeps the whole frame inside its right edge; a very short
/// one shows less than the least height rather than hang below the window.
#[test]
fn it_stays_inside_a_narrow_and_a_very_short_window() {
    let screen = Rect::new(0.0, 0.0, 600.0, 1000.0);
    let past = place_under(Rect::new(560.0, 40.0, 80.0, 22.0), screen);
    assert!(past.x + OUTER[0] <= screen.w, "{past:?}");
    assert!(past.x >= 0.0);
    let tiny = Rect::new(0.0, 0.0, 1600.0, 100.0);
    let short = place_under(Rect::new(1200.0, 40.0, 80.0, 22.0), tiny);
    assert!(short.y + short.height <= tiny.h, "{short:?}");
    assert!(short.height >= 0.0 && short.scrolls);
}

/// The header's status and the "was" words end on one right edge; the was
/// words underline under the pointer.
#[test]
fn the_header_status_lines_end_together_and_was_underlines_on_hover() {
    let theme = ResolvedTheme::bundled(true);
    let state = State {
        hover: Some(Target::Undo),
        changed: Some(Changed {
            to: preset("charts-2").unwrap(),
            was: preset("time+flow"),
            at_ms: 0,
        }),
        ..State::default()
    };
    let shown = picker(&view(&state, marks("charts-2", "time+flow", true)), theme);
    let right = |wanted: &str| {
        shown.marks.iter().find_map(|mark| match mark {
            Mark::Words(words) if words.text.starts_with(wanted) => Some(words.clone()),
            _ => None,
        })
    };
    let status = right(NOT_SAVED).expect("the status words");
    let was = right("was ").expect("the was words");
    assert_eq!((status.anchor, was.anchor), (Anchor::Right, Anchor::Right));
    assert_eq!(status.x, was.x);
    assert!(was.underline, "a link under the pointer");
    let rest = State {
        hover: None,
        ..state
    };
    let calm = picker(&view(&rest, marks("charts-2", "time+flow", true)), theme);
    let calm_was = calm.marks.iter().find_map(|mark| match mark {
        Mark::Words(words) if words.text.starts_with("was ") => Some(words.clone()),
        _ => None,
    });
    assert!(!calm_was.unwrap().underline);
}

/// The picker belongs to the tab it opened on: a frame from another tab
/// closes it and forgets the pick, so Ctrl+Z there restores nothing.
#[test]
fn another_tab_closes_the_picker_and_forgets_the_pick() {
    let mut state = State {
        changed: Some(Changed {
            to: preset("charts-2").unwrap(),
            was: preset("flow"),
            at_ms: 0,
        }),
        tab: Some(1),
        ..State::default()
    };
    let undo = Seen {
        tab: 2,
        keys: Keys {
            undo: true,
            ..Keys::default()
        },
        ..seen(100, "time+flow")
    };
    let step = state.step(undo);
    assert_eq!((step.pick, step.close), (None, true));
    assert_eq!(state, State::default());
}

/// The read names every cell as the grid draws it, with the save state.
#[test]
fn the_read_names_every_cell_with_its_marks() {
    let state = State::default();
    let read = read("Flow", marks("time+flow", "flow", true), &state, false);
    assert!(!read.open);
    assert_eq!(read.workspace, "Flow");
    assert_eq!(read.status, NOT_SAVED);
    assert_eq!(read.cells.len(), LAYOUT_PRESETS.len());
    let current: Vec<&str> = read
        .cells
        .iter()
        .filter(|c| c.current)
        .map(|c| c.preset_id.as_str())
        .collect();
    let saved: Vec<&str> = read
        .cells
        .iter()
        .filter(|c| c.saved)
        .map(|c| c.preset_id.as_str())
        .collect();
    assert_eq!((current, saved), (vec!["time+flow"], vec!["flow"]));
    let time = read.cells.iter().find(|c| c.preset_id == "time").unwrap();
    assert_eq!((time.row, time.column), (0, 2));
    assert_eq!(read.current_id.as_deref(), Some("time+flow"));
    assert_eq!(read.undo_id, None);
}
