//! The palette, the colour type and the rules that keep two workspaces apart.
use super::*;

const NAMES: [&str; 16] = [
    "Cobalt",
    "Orchid",
    "Sky",
    "Violet",
    "Slate",
    "Fuchsia",
    "Petrol",
    "Lavender",
    "Ultramarine",
    "Mauve",
    "Denim",
    "Grape",
    "Ink",
    "Plum",
    "Ice",
    "Blossom",
];

fn settled(order: &[&str]) -> Colours {
    let mut colours = Colours::new();
    settle(&mut colours, order);
    colours
}

fn preset(index: u8) -> WorkspaceColour {
    WorkspaceColour::preset(index).unwrap()
}

/// The order is stored in files, so each index keeps its name and values.
#[test]
fn the_palette_pins_sixteen_names_to_their_indices_and_values() {
    assert_eq!(PRESETS.len(), 16);
    for (index, name) in NAMES.iter().enumerate() {
        assert_eq!(PRESETS[index].name, *name);
        let colour = preset(index as u8);
        assert_eq!(colour.name(), *name);
        assert_eq!(WorkspaceColour::parse(&name.to_lowercase()), Some(colour));
        assert_eq!(WorkspaceColour::parse(name), Some(colour), "any case");
    }
    assert_eq!(WorkspaceColour::preset(16), None);
    let pins = [
        (0, "#2F5BD8", "#7393F5"),
        (2, "#1F86BF", "#5DBDEB"),
        (4, "#56606E", "#A3ADBA"),
        (8, "#2C2F9E", "#8085F2"),
        (12, "#252B36", "#E3E7EE"),
        (15, "#CF6FB6", "#F4B3DD"),
    ];
    for (index, light, dark) in pins {
        assert_eq!(hex(preset(index).rgb(false)), light);
        assert_eq!(hex(preset(index).rgb(true)), dark);
    }
}

#[test]
fn every_preset_value_is_distinct_within_each_theme() {
    for dark in [false, true] {
        let mut seen: Vec<Rgb> = Vec::new();
        for index in 0..16 {
            let rgb = preset(index).rgb(dark);
            assert!(!seen.contains(&rgb), "{} repeats", NAMES[index as usize]);
            seen.push(rgb);
        }
    }
}

/// No green, red, gold or orange: those already mean buy, sell, the POC and
/// unsaved edits. A tinted value's hue stays between cyan and magenta; a
/// near-grey one (Slate, Ink) has no hue to speak of.
#[test]
fn no_preset_is_green_red_gold_or_orange() {
    for index in 0..16 {
        for dark in [false, true] {
            let [r, g, b] = preset(index).rgb(dark).map(f32::from);
            let (max, min) = (r.max(g).max(b), r.min(g).min(b));
            if (max - min) / max.max(1.0) < 0.2 {
                continue;
            }
            let hue = if max == r {
                60.0 * ((g - b) / (max - min))
            } else if max == g {
                60.0 * ((b - r) / (max - min) + 2.0)
            } else {
                60.0 * ((r - g) / (max - min) + 4.0)
            };
            let hue = hue.rem_euclid(360.0);
            let name = NAMES[index as usize];
            assert!((180.0..=330.0).contains(&hue), "{name} {dark}: {hue}");
        }
    }
}

/// A preset paints its Light value in Light and its Dark value in Dark; a
/// custom colour one value in both.
#[test]
fn the_resolver_gives_a_preset_per_theme_and_a_custom_one_value() {
    let sky = preset(2);
    assert_eq!(sky.rgb(false), [0x1F, 0x86, 0xBF]);
    assert_eq!(sky.rgb(true), [0x5D, 0xBD, 0xEB]);
    let custom = WorkspaceColour::Custom([0x3A, 0x7B, 0xD5]);
    assert_eq!(custom.rgb(false), custom.rgb(true));
    assert_eq!(custom.name(), "Custom");
    assert_eq!(sky.describe(true), "Sky #5DBDEB");
    assert_eq!(custom.describe(false), "Custom #3A7BD5");
    assert_eq!(sky.key(), "sky");
    assert_eq!(custom.key(), "#3A7BD5");
}

#[test]
fn a_colour_word_is_a_preset_name_or_a_hex_code() {
    let custom = WorkspaceColour::Custom([0x3A, 0x7B, 0xD5]);
    assert_eq!(WorkspaceColour::parse("#3A7BD5"), Some(custom));
    assert_eq!(WorkspaceColour::parse("#3a7bd5"), Some(custom));
    for refused in ["", "green", "#3A7BD", "#3A7BD55", "3A7BD5", "#GGGGGG", "Custom"] {
        assert_eq!(WorkspaceColour::parse(refused), None, "{refused}");
    }
}

/// #978's eight words, each to its nearest preset one to one.
#[test]
fn the_eight_old_words_map_through_the_fixed_table() {
    let table = [
        ("steel", "Slate"),
        ("violet", "Grape"),
        ("cyan", "Sky"),
        ("magenta", "Orchid"),
        ("azure", "Cobalt"),
        ("indigo", "Violet"),
        ("chalk", "Ink"),
        ("cobalt", "Ultramarine"),
    ];
    for (word, name) in table {
        assert_eq!(WorkspaceColour::legacy(word).map(WorkspaceColour::name), Some(name));
    }
    assert_eq!(WorkspaceColour::legacy("green"), None);
}

/// Identical is the same resolved value in either theme.
#[test]
fn identical_is_judged_across_both_themes() {
    let cobalt = preset(0);
    let dark_cobalt = WorkspaceColour::Custom([0x73, 0x93, 0xF5]);
    let light_cobalt = WorkspaceColour::Custom([0x2F, 0x5B, 0xD8]);
    assert!(dark_cobalt.identical(cobalt) && cobalt.identical(dark_cobalt));
    assert!(light_cobalt.identical(cobalt));
    assert!(!dark_cobalt.identical(light_cobalt), "apart in both themes");
    assert!(cobalt.identical(cobalt));
    assert!(!cobalt.identical(preset(8)));
    let near = WorkspaceColour::Custom([0x73, 0x93, 0xF4]);
    assert!(!near.identical(cobalt), "close is not identical");
}

#[test]
fn next_free_starts_default_on_slate_and_then_follows_index_order() {
    let names = ["Default", "A", "B", "C", "D", "E"];
    let colours = settled(&names);
    let want = [4, 0, 1, 2, 3, 5];
    for (name, index) in names.iter().zip(want) {
        assert_eq!(colours[*name], preset(index), "{name}");
    }
}

/// The 17th, 18th and 19th workspaces get distinct generated customs, the
/// same on every run.
#[test]
fn past_sixteen_workspaces_get_generated_customs_deterministically() {
    let names: Vec<String> = std::iter::once("Default".to_owned())
        .chain((1..19).map(|n| format!("W{n:02}")))
        .collect();
    let order: Vec<&str> = names.iter().map(String::as_str).collect();
    let colours = settled(&order);
    assert_eq!(colours, settled(&order), "deterministic");
    let extra: Vec<WorkspaceColour> = order[16..].iter().map(|name| colours[*name]).collect();
    assert_eq!(extra.len(), 3);
    for colour in &extra {
        assert!(matches!(colour, WorkspaceColour::Custom(_)), "{colour:?}");
    }
    let all: Vec<WorkspaceColour> = colours.values().copied().collect();
    for (at, a) in all.iter().enumerate() {
        for b in &all[at + 1..] {
            assert!(!a.identical(*b), "{a:?} {b:?}");
        }
    }
    // The first candidate is the OKLab midpoint of Cobalt's two values.
    assert_eq!(extra[0], generated(&(0..16).map(preset).collect::<Vec<_>>()));
}

/// Every candidate taken: the next round lifts the midpoints' lightness.
#[test]
fn a_taken_candidate_round_moves_to_a_lighter_round() {
    let mut held: Vec<WorkspaceColour> = (0..16).map(preset).collect();
    for _ in 0..16 {
        let next = generated(&held);
        held.push(next);
    }
    let round_two = generated(&held);
    assert!(held.iter().all(|colour| !colour.identical(round_two)));
}

#[test]
fn settle_keeps_held_colours_and_drops_duplicates_and_strangers() {
    let mut colours = Colours::from([
        ("B".to_owned(), preset(4)),
        ("A".to_owned(), WorkspaceColour::Custom([0x56, 0x60, 0x6E])),
        ("Gone".to_owned(), preset(2)),
    ]);
    settle(&mut colours, &["Default", "A", "B"]);
    // `A` comes first in the book's order and its custom is Slate's Light
    // value, so `B` loses Slate. Default and B then take the first free.
    assert_eq!(colours["A"], WorkspaceColour::Custom([0x56, 0x60, 0x6E]));
    assert_eq!(colours["Default"], preset(0));
    assert_eq!(colours["B"], preset(1));
    assert!(!colours.contains_key("Gone"));
}

#[test]
fn a_taken_colour_is_refused_without_swap_and_swapped_with_it() {
    let mut colours = settled(&["Default", "Scalp", "Swing"]);
    let taken = pick(&mut colours, "Swing", preset(0), false).unwrap_err();
    assert_eq!(taken.owner, "Scalp");
    assert_eq!(taken.owner_would_get, preset(1), "Swing's own, Orchid");
    assert_eq!(colours["Swing"], preset(1), "nothing changed");
    let swap = pick(&mut colours, "Swing", preset(0), true).unwrap();
    assert_eq!(swap.swapped_with.as_deref(), Some("Scalp"));
    assert_eq!(swap.holder_colour, Some(preset(1)));
    assert!(swap.changed);
    assert_eq!((colours["Swing"], colours["Scalp"]), (preset(0), preset(1)));
    let free = pick(&mut colours, "Swing", preset(9), false).unwrap();
    assert_eq!((free.changed, free.swapped_with), (true, None));
    assert_eq!(free.holder_colour, None);
    let again = pick(&mut colours, "Swing", preset(9), false).unwrap();
    assert!(!again.changed);
    // A custom equal to a held preset's Dark value is that preset.
    let dark_slate = WorkspaceColour::Custom([0xA3, 0xAD, 0xBA]);
    let refused = pick(&mut colours, "Swing", dark_slate, false).unwrap_err();
    assert_eq!(refused.owner, "Default");
}

#[test]
fn initials_grow_only_where_two_owners_share_a_letter() {
    let among = ["Default", "Scalp", "Swing", "Flow"];
    assert_eq!(initials("Default", &among), "D");
    assert_eq!(initials("Scalp", &among), "Sc");
    assert_eq!(initials("Swing", &among), "Sw");
    assert_eq!(initials("Flow", &["Flow"]), "F");
    assert_eq!(initials("Sw", &["Sw", "Swing"]), "Sw");
    assert_eq!(initials("Swing", &["Sw", "Swing"]), "Swi");
}

/// Owners alike past four characters still read apart, each in four.
#[test]
fn initials_tell_apart_names_alike_past_four_characters() {
    for among in [["Scalp", "Scalper"], ["Swing", "Swing2"]] {
        let [first, second] = among.map(|name| initials(name, &among));
        assert_ne!(first, second, "{among:?}");
        assert!(first.chars().count() <= INITIALS_MAX, "{first}");
        assert!(second.chars().count() <= INITIALS_MAX, "{second}");
    }
    assert_eq!(initials("Scalp", &["Scalp", "Scalper"]), "Sca1");
    assert_eq!(initials("Swing2", &["Swing", "Swing2"]), "Swi2");
    assert_eq!(initials("Default", &["Default", "Scalp", "Scalper"]), "D");
}
