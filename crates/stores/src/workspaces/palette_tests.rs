//! The palette popover's model, its swap question and the Custom warnings,
//! on the mock's eight workspaces.
use super::*;
use crate::workspaces::{WorkspaceColour, parse_book};
use quantick_appearance::contrast::{contrast_ratio, oklab_distance};
use quantick_appearance::{PremultipliedRgba, ResolvedTheme, Token};

const BLUE: [u8; 3] = [0x3A, 0x7B, 0xD5];

fn preset(index: u8) -> WorkspaceColour {
    WorkspaceColour::preset(index).unwrap()
}

/// The mock's book: Flow is Violet, Default Slate, Macro Ice, Night
/// Blossom, Opening Cobalt, Scalp Sky, Swing Fuchsia, Tape Orchid; the last
/// custom colour, when asked, #3A7BD5.
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

/// The current colour is marked, a held one carries its owner's initials,
/// a free one neither; the seventeenth swatch shows once a custom colour was
/// applied; the next free colour is Petrol.
#[test]
fn the_swatches_mark_the_current_colour_and_the_held_ones() {
    let model = palette(&mock(true), "Flow").unwrap();
    assert_eq!(model.presets.len(), 16);
    assert_eq!(
        (model.workspace.as_str(), model.current),
        ("Flow", preset(3))
    );
    let violet = &model.presets[3];
    assert!(violet.current && violet.holder.is_none() && violet.initials.is_none());
    assert_eq!(violet.tip(), "Violet \u{b7} current");
    let sky = &model.presets[2];
    assert_eq!((sky.key.as_str(), sky.name.as_str()), ("sky", "Sky"));
    assert_eq!(sky.holder.as_deref(), Some("Scalp"));
    assert_eq!(sky.initials.as_deref(), Some("Sc"));
    assert_eq!(sky.tip(), "Sky \u{b7} used by Scalp");
    let initials: Vec<Option<&str>> = (model.presets.iter())
        .map(|swatch| swatch.initials.as_deref())
        .collect();
    let held = [
        Some("O"),
        Some("T"),
        Some("Sc"),
        None,
        Some("D"),
        Some("Sw"),
    ];
    assert_eq!(initials[..6], held);
    assert_eq!(initials[14..], [Some("M"), Some("N")]);
    let petrol = &model.presets[6];
    assert!(!petrol.current && petrol.holder.is_none());
    assert_eq!(petrol.tip(), "Petrol");
    assert_eq!(model.next_free, preset(6), "Petrol");
    let last = model
        .last_custom
        .as_ref()
        .expect("a custom colour was applied");
    assert_eq!(
        (last.key.as_str(), last.name.as_str()),
        (LAST_CUSTOM_KEY, "#3A7BD5")
    );
    assert_eq!(last.colour, WorkspaceColour::Custom(BLUE));
    assert_eq!(last.tip(), "Last custom #3A7BD5");
    assert_eq!(model.swatches().count(), 17);
    let fresh = palette(&mock(false), "Flow").unwrap();
    assert!(fresh.last_custom.is_none(), "none before one is applied");
    assert_eq!(fresh.swatches().count(), 16);
    assert!(palette(&mock(false), "Nope").is_none());
}

/// A free colour applies; a held one asks, in the plan's words for a preset
/// and for a custom colour, and the mock's for a typed colour equal to a
/// preset another workspace holds.
#[test]
fn a_held_colour_asks_to_swap_in_its_own_words() {
    let book = mock(true);
    let model = palette(&book, "Flow").unwrap();
    assert_eq!(model.pick(preset(6), false), Pick::Apply(preset(6)));
    assert_eq!(model.pick(preset(3), false), Pick::Apply(preset(3)));
    let Pick::Ask(question) = model.pick(preset(2), false) else {
        panic!("Sky is held")
    };
    assert_eq!(
        question.words,
        "Sky is used by Scalp. Swap? Scalp becomes Violet."
    );
    assert_eq!(
        (question.owner.as_str(), question.owner_gets),
        ("Scalp", preset(3))
    );
    let typed = WorkspaceColour::Custom([0x5D, 0xBD, 0xEB]);
    let Pick::Ask(question) = model.pick(typed, true) else {
        panic!("Sky's Dark value is held")
    };
    assert_eq!(
        question.words,
        "#5DBDEB is Sky, used by Scalp. Swap? Scalp becomes Violet."
    );
    assert_eq!(question.colour, preset(2), "the swap takes Sky itself");
    let mut book = book;
    book.set_colour("Scalp", WorkspaceColour::Custom(BLUE), false)
        .unwrap();
    let model = palette(&book, "Flow").unwrap();
    for typed in [false, true] {
        let Pick::Ask(question) = model.pick(WorkspaceColour::Custom(BLUE), typed) else {
            panic!("Scalp holds the custom colour")
        };
        let words = "Custom #3A7BD5 is used by Scalp. Swap? Scalp becomes Violet.";
        assert_eq!(question.words, words, "typed {typed}");
    }
    assert_eq!(
        model.last_custom.as_ref().unwrap().initials.as_deref(),
        Some("Sc"),
        "the seventeenth swatch carries its owner's initials too"
    );
}

/// The thresholds: close below 0.03 ΔE_OK, near a trade colour below 0.06,
/// low contrast below 1.5:1, each strictly.
#[test]
fn each_warning_starts_just_under_its_threshold() {
    assert!(is_close(0.0299) && !is_close(0.0301));
    assert!(is_near_trade(0.0599) && !is_near_trade(0.0601));
    assert!(is_low_contrast(1.499) && !is_low_contrast(1.501));
}

/// No preset warns: none is close to another in either theme, none near a
/// buy or sell colour; and Light's black `sell` never counts as red.
#[test]
fn no_preset_warns_and_light_s_black_sell_is_no_red() {
    for dark in [true, false] {
        for a in 0..16 {
            for b in a + 1..16 {
                let (x, y) = (preset(a).rgb(dark), preset(b).rgb(dark));
                let distance = oklab_distance(rgb(x), rgb(y));
                assert!(!is_close(distance), "{a} {b} dark {dark}: {distance}");
            }
        }
    }
    let inks = trade_inks();
    let light_sell = ResolvedTheme::bundled(false).color(Token::Sell);
    assert_eq!(light_sell, PremultipliedRgba::from_rgb(0, 0, 0));
    assert!(
        !inks.iter().any(|trade| trade.ink == light_sell),
        "{inks:?}"
    );
    assert_eq!(inks.len(), 3, "both buys and Dark's sell");
    let book = mock(false);
    for index in 0..16 {
        let warned = warnings(&book, "Flow", preset(index));
        let trade =
            |w: &ColourWarning| matches!(w, ColourWarning::NearBuy | ColourWarning::NearSell);
        assert!(!warned.iter().any(trade), "{index}: {warned:?}");
    }
}

fn rgb([r, g, b]: [u8; 3]) -> PremultipliedRgba {
    PremultipliedRgba::from_rgb(r, g, b)
}

/// The mock's four Custom values each raise their one warning, in its words.
#[test]
fn the_mock_s_custom_values_raise_their_warnings() {
    let book = mock(true);
    let said = |hex: [u8; 3]| -> Vec<String> {
        let found = warnings(&book, "Flow", WorkspaceColour::Custom(hex));
        found.iter().map(ColourWarning::words).collect()
    };
    assert_eq!(
        said([0x62, 0xB8, 0xEA]),
        ["Hard to tell from Scalp's colour (Sky)."]
    );
    assert_eq!(said([0x2B, 0xA5, 0x97]), ["Close to the buy green."]);
    assert_eq!(said([0xEA, 0x56, 0x56]), ["Close to the sell red."]);
    assert_eq!(
        said([0x3A, 0x35, 0x50]),
        ["Low contrast on the Light tab strip."]
    );
    let dark_strip = ResolvedTheme::bundled(true)
        .title_strip()
        .color(Token::TabStrip);
    let light_strip = ResolvedTheme::bundled(false)
        .title_strip()
        .color(Token::TabStrip);
    assert_eq!(dark_strip, rgb([0x10, 0x14, 0x1D]));
    assert_eq!(light_strip, rgb([0x25, 0x2F, 0x37]));
    assert!(contrast_ratio(rgb([0x3A, 0x35, 0x50]), dark_strip) >= 1.5);
    let both = said([0x10, 0x10, 0x10]);
    assert_eq!(
        both,
        [
            "Low contrast on the Dark tab strip.",
            "Low contrast on the Light tab strip."
        ]
    );
    assert!(said(BLUE).is_empty(), "the mock's custom colour is clean");
    assert_eq!(HEX_HINT, "Use #RRGGBB, for example #3A7BD5");
}

/// The create dialogs' palette: the next free colour (Petrol) chosen, every
/// held swatch locked with its owner's initials, Flow's Violet included,
/// and a locked swatch's hover sends the trader to Manage.
#[test]
fn the_create_palette_chooses_the_next_free_colour_and_locks_the_held_ones() {
    let book = mock(true);
    let model = choosing(&book, None);
    assert!(model.choosing);
    assert_eq!(model.current, preset(6), "Petrol, the next free");
    assert_eq!(model.current, model.next_free);
    let petrol = &model.presets[6];
    assert!(petrol.current && !petrol.locked);
    let violet = &model.presets[3];
    assert!(violet.locked && !violet.current);
    assert_eq!(violet.initials.as_deref(), Some("F"));
    let sky = &model.presets[2];
    assert!(sky.locked);
    assert_eq!(
        sky.tip(),
        "Used by Scalp \u{b7} change it in Manage workspaces"
    );
    assert!(!model.presets[7].locked, "Lavender is free");
    let last = model.last_custom.as_ref().unwrap();
    assert!(!last.locked, "#3A7BD5 is nobody's");
    let chosen = choosing(&book, Some(preset(7)));
    assert!(chosen.presets[7].current && !chosen.presets[6].current);
}

/// On create a held colour is never swapped: a pick or a typed identical
/// colour is refused with its owner, a free one is chosen.
#[test]
fn on_create_a_held_colour_is_refused_and_a_free_one_chosen() {
    let model = choosing(&mock(false), None);
    let refused = |pick| match pick {
        Pick::Refused(words) => words,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        refused(model.pick(preset(2), false)),
        "Used by Scalp. Pick another colour."
    );
    let typed = WorkspaceColour::Custom([0x5D, 0xBD, 0xEB]);
    assert_eq!(
        refused(model.pick(typed, true)),
        "Used by Scalp. Pick another colour."
    );
    assert_eq!(model.pick(preset(7), false), Pick::Apply(preset(7)));
    let blue = WorkspaceColour::Custom(BLUE);
    assert_eq!(model.pick(blue, true), Pick::Apply(blue));
}

/// The menu's and Manage's palette never lock: a held colour asks to swap.
#[test]
fn the_apply_palette_locks_nothing() {
    let model = palette(&mock(true), "Flow").unwrap();
    assert!(!model.choosing);
    assert!(model.swatches().all(|swatch| !swatch.locked));
}
