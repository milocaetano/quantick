//! Marks that follow their pane's ground, and marks whose look is the trader's.

use quantick_appearance::{
    Appearance, ChartSurface, DARK, Ground, Inheritance, LIGHT, PaneCanvas, PremultipliedRgba,
    ResolvedTheme, Token,
};

use crate::presets::{PRESETS_FILE, PresetStore};
use crate::{
    ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingId, DrawingStyle, DrawingTool, Drawings,
    PresetHost, SavedDrawingExt, TextPayload, ToolCatalog, new_drawing_from_defaults,
};
use quantick_workspace::layout_document::SavedDrawing;

fn tool(id: &str) -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .expect("registered test tool")
}

fn store(label: &str) -> PresetStore {
    let path = crate::scratch::thread_dir(label).join(PRESETS_FILE);
    let _ = std::fs::remove_file(&path);
    PresetStore::load_from(&path)
}

fn theme(appearance: &mut Appearance, store: &mut PresetStore, source: &str) {
    appearance.apply(source).unwrap();
    store.sync_theme(appearance);
}

/// Place a finished mark of `tool` opened from `host`'s defaults on `canvas`,
/// the way a click does.
fn open(
    drawings: &mut Drawings<DrawingTool>,
    host: &dyn PresetHost,
    canvas: PaneCanvas,
    tool: DrawingTool,
) {
    for slot in 0..tool.required_points() {
        #[allow(clippy::cast_precision_loss)]
        let point = ChartPoint::at(1.0 + 4.0 * slot as f32, 100.0 + slot as f64);
        drawings.place_with(tool, &DrawingBand::Price, point, |tool| {
            new_drawing_from_defaults(host, canvas, tool)
        });
    }
    assert!(drawings.draft().is_none(), "{} finished", tool.id());
}

fn style_on(host: &dyn PresetHost, canvas: PaneCanvas, tool: DrawingTool) -> DrawingStyle {
    new_drawing_from_defaults(host, canvas, tool).style
}

/// The factory ink of `tool` on `canvas`: what a factory-look mark wears
/// there, whatever the bank holds.
fn factory(host: &dyn PresetHost, canvas: PaneCanvas, tool: DrawingTool) -> DrawingStyle {
    let palette = crate::defaults::palette_on(host.theme(), canvas).0;
    crate::defaults::factory_opening(palette, tool).style
}

fn red() -> DrawingStyle {
    DrawingStyle {
        color: PremultipliedRgba::from_rgb(220, 30, 30),
        width_px: 3.0,
        fill_alpha: 0,
    }
}

/// The trader's report: a light-to-dark switch left every drawing, the
/// fixed-range profile too, in light-tuned ink. Marks opened from defaults
/// follow the ground both ways; the switch is no edit and no undo step.
#[test]
fn a_theme_switch_repaints_the_marks_that_follow_their_ground_both_ways() {
    let mut store = store("look-theme-switch");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let paper: PaneCanvas = ChartSurface::Paper.into();
    let (line, profile) = (tool("trend-line"), tool(crate::frvp::TOOL_ID));
    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, paper);
    open(&mut drawings, &store, paper, line);
    open(&mut drawings, &store, paper, profile);
    assert_eq!(drawings.items().len(), 2);
    let (revision, depth) = (drawings.revision(), drawings.undo_depth());
    let light = [
        style_on(&store, paper, line),
        style_on(&store, paper, profile),
    ];

    theme(&mut appearance, &mut store, DARK);
    assert!(drawings.follow_ground(&store, paper));
    let dark = [
        style_on(&store, paper, line),
        style_on(&store, paper, profile),
    ];
    for (index, mark) in drawings.items().iter().enumerate() {
        assert_eq!(mark.look, Inheritance::Theme);
        assert_eq!(mark.style, dark[index], "{}", mark.tool.id());
        assert_ne!(mark.style, light[index], "{}", mark.tool.id());
    }
    assert_eq!(drawings.revision(), revision, "a repaint is not an edit");
    assert_eq!(drawings.undo_depth(), depth, "nor an undo step");
    assert!(!drawings.follow_ground(&store, paper), "once per move");

    // Undo takes back the profile, never the dark ink of the line before it.
    assert!(drawings.undo());
    assert_eq!(drawings.items()[0].style, dark[0]);
    assert!(drawings.redo());

    theme(&mut appearance, &mut store, LIGHT);
    assert!(drawings.follow_ground(&store, paper));
    let back: Vec<_> = drawings.items().iter().map(|mark| mark.style).collect();
    assert_eq!(back, light);
}

/// Saving a default, by any door (the inspector's save, the pencil
/// remembering a stroke, a note's repeat), never reaches a mark on the
/// chart: a reload, a paste and a theme switch move only the factory-look
/// marks, and only to their pane's factory ink. A mark opened from a saved
/// default is the trader's.
#[test]
fn a_saved_default_never_reaches_a_mark_on_the_chart() {
    use crate::{adopt_pane, put_away_pane};
    use quantick_workspace::market_settings::{DrawingKey, MarketSettings};

    let mut store = store("look-save-never");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let paper: PaneCanvas = ChartSurface::Paper.into();
    let (line, note, pencil) = (tool("trend-line"), tool("text"), tool("brush"));
    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, paper);
    open(&mut drawings, &store, paper, line);
    open(&mut drawings, &store, paper, note);
    let light = [factory(&store, paper, line), factory(&store, paper, note)];
    let blue = DrawingStyle {
        color: PremultipliedRgba::from_rgb(30, 60, 220),
        ..red()
    };

    let mut saved = drawings.items()[0].clone();
    saved.style = blue;
    crate::save_tool_default(&mut store, paper, &saved);
    let mut stroke = drawings.items()[0].clone();
    stroke.tool = pencil;
    let mut restroked = stroke.clone();
    restroked.style = red();
    assert!(crate::remember_last_style(
        &mut store, paper, &stroke, &restroked
    ));
    let mut repeated = drawings.items()[1].clone();
    repeated.style = red();
    crate::prepare_repeat(&mut store, paper, &repeated);
    store.set_default_style(line.id(), Ground::Dark, Some(red()));
    assert!(!drawings.follow_ground(&store, paper), "a save is no move");
    for (mark, ink) in drawings.items().iter().zip(light) {
        assert_eq!((mark.style, mark.look), (ink, Inheritance::Theme));
    }
    for tool in [line, note, pencil] {
        let opening = new_drawing_from_defaults(&store, paper, tool);
        assert_eq!(opening.look, Inheritance::Explicit, "{}", tool.id());
    }
    open(&mut drawings, &store, paper, line);
    assert_eq!(drawings.items()[2].style, blue);
    let copy = drawings.items()[0].clone();
    let _ = drawings.paste(&copy, 2.0);
    assert_eq!(
        drawings.items()[3].style,
        light[0],
        "a paste keeps factory ink"
    );

    // A reload brings every mark back as it was.
    let key = DrawingKey::of_pane("binance", "BTCUSDT".to_owned(), None);
    let (mut settings, mut base) = (MarketSettings::default(), Vec::new());
    let _ = put_away_pane(&mut drawings, &mut base, &mut settings, &key, "tick:100");
    let mut reloaded = Drawings::on_canvas(paper);
    reloaded.follow_ground(&store, paper);
    adopt_pane(&mut reloaded, &mut base, &settings, &key, "tick:100");
    let styles = |drawings: &Drawings<DrawingTool>| -> Vec<_> {
        drawings.items().iter().map(|mark| mark.style).collect()
    };
    assert_eq!(styles(&reloaded), [light[0], light[1], blue, light[0]]);

    // A switch takes the factory marks to the dark factory ink, not the bank.
    theme(&mut appearance, &mut store, DARK);
    assert!(reloaded.follow_ground(&store, paper));
    let dark = [factory(&store, paper, line), factory(&store, paper, note)];
    assert_ne!(dark[0], red());
    assert_eq!(styles(&reloaded), [dark[0], dark[1], blue, dark[0]]);
    theme(&mut appearance, &mut store, LIGHT);
    assert!(reloaded.follow_ground(&store, paper));
    assert_eq!(styles(&reloaded), [light[0], light[1], blue, light[0]]);
}

/// An edit of the look is the trader's, through any door that records it;
/// writing a note's words is not one.
#[test]
fn an_edited_look_is_never_repainted_and_words_are_not_a_look() {
    let mut store = store("look-explicit");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let paper: PaneCanvas = ChartSurface::Paper.into();
    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, paper);
    open(&mut drawings, &store, paper, tool("trend-line"));
    open(&mut drawings, &store, paper, tool("text"));

    let before = drawings.items()[0].clone();
    drawings.items_mut()[0].style = red();
    drawings.record_edit_of(0, before);
    assert_eq!(drawings.items()[0].look, Inheritance::Explicit);

    let before = drawings.items()[1].clone();
    let note = drawings.items_mut()[1]
        .payload
        .as_any_mut()
        .downcast_mut::<TextPayload>()
        .unwrap();
    note.text = "POC".to_owned();
    drawings.record_edit_of(1, before);
    assert_eq!(drawings.items()[1].look, Inheritance::Theme);

    theme(&mut appearance, &mut store, DARK);
    drawings.follow_ground(&store, paper);
    assert_eq!(drawings.items()[0].style, red());
    let note = &drawings.items()[1];
    assert_eq!(note.style, style_on(&store, paper, tool("text")));
    let words = note.payload.as_any().downcast_ref::<TextPayload>().unwrap();
    assert_eq!(words.text, "POC", "a repaint keeps the words");

    // Undo hands the edited line back to its ground, at the ground's ink.
    assert!(drawings.undo());
    assert!(drawings.undo());
    let line = &drawings.items()[0];
    assert_eq!(line.look, Inheritance::Theme);
    assert_eq!(line.style, style_on(&store, paper, tool("trend-line")));
}

/// The inspector installs its copy while the gesture is open; that copy is
/// already the trader's, so no repaint can take the edit back mid-gesture.
#[test]
fn an_inspector_copy_owns_the_look_before_its_step_is_recorded() {
    let mut store = store("look-inspector");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let paper: PaneCanvas = ChartSurface::Paper.into();
    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, paper);
    open(&mut drawings, &store, paper, tool("trend-line"));
    let mut edited = drawings.items()[0].clone();
    edited.style = red();
    drawings.replace_edited(0, edited.clone());
    theme(&mut appearance, &mut store, DARK);
    drawings.follow_ground(&store, paper);
    // The next frame installs the same copy again: still the trader's.
    drawings.replace_edited(0, edited);
    assert_eq!(drawings.items()[0].style, red());
    assert_eq!(drawings.items()[0].look, Inheritance::Explicit);
}

/// One rule for every pane: a time chart the trader gave a dark background on
/// a light theme reads the dark bank, and the dark ink where it has none.
#[test]
fn a_time_chart_made_dark_reads_the_dark_bank_and_the_dark_ink() {
    let mut store = store("look-made-dark");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let made_dark = PaneCanvas {
        surface: ChartSurface::Paper,
        background: Some(PremultipliedRgba::from_rgb(16, 18, 24)),
    };
    let (line, profile) = (tool("trend-line"), tool(crate::frvp::TOOL_ID));
    store.set_default_style(line.id(), Ground::Dark, Some(red()));
    assert_eq!(crate::defaults_ground(&store, made_dark), Ground::Dark);
    assert_eq!(style_on(&store, made_dark, line), red());
    assert_eq!(
        style_on(&store, made_dark, profile),
        profile.themed_style(Some(ResolvedTheme::bundled(true)))
    );

    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, ChartSurface::Paper.into());
    open(&mut drawings, &store, ChartSurface::Paper.into(), profile);
    open(&mut drawings, &store, ChartSurface::Paper.into(), line);
    let light = [drawings.items()[0].style, drawings.items()[1].style];
    // The trader darkens the chart: the factory marks wear the dark factory
    // ink, never the dark bank.
    assert!(drawings.follow_ground(&store, made_dark));
    assert_eq!(drawings.canvas(), made_dark);
    let dark = [
        factory(&store, made_dark, profile),
        factory(&store, made_dark, line),
    ];
    for (index, mark) in drawings.items().iter().enumerate() {
        assert_ne!(mark.style, light[index], "{}", mark.tool.id());
        assert_eq!(mark.style, dark[index], "{}", mark.tool.id());
    }
    assert_ne!(dark[1], red());
    // The tokens a profile paints its casing and POC with are the dark
    // ground's too, not the light paper's.
    let palette = drawings.palette().expect("followed a ground");
    let bundled_dark = ResolvedTheme::bundled(true);
    for token in [Token::Casing, Token::Poc] {
        assert_eq!(palette.color(token), bundled_dark.color(token), "{token:?}");
    }
    // A line opened there reads the dark bank, and is the trader's.
    open(&mut drawings, &store, made_dark, line);
    assert_eq!(drawings.items()[2].style, red());
    assert_eq!(drawings.items()[2].look, Inheritance::Explicit);
}

/// A pasted mark was dressed for another canvas: it wears this pane's ink at
/// once, before the pane paints it.
#[test]
fn a_pasted_mark_wears_this_pane_s_ink_at_once() {
    let mut store = store("look-paste");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let (paper, island) = (ChartSurface::Paper.into(), ChartSurface::Flow.into());
    let line = tool("trend-line");
    let mut source = Drawings::default();
    open(&mut source, &store, paper, line);
    let mark = source.items()[0].clone();

    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, island);
    let _ = drawings.paste(&mark, 2.0);
    assert_eq!(drawings.items()[0].style, factory(&store, island, line));
    assert_ne!(drawings.items()[0].style, mark.style);
    assert!(
        !drawings.follow_ground(&store, island),
        "nothing left to do"
    );
}

/// One workspace/layout, a light time chart and a dark flow chart sharing a mark with
/// the factory look: each shows its own ground's ink once the other writes,
/// and resolving it is no edit, so neither chart writes its ink back.
#[test]
fn a_shared_mark_wears_each_chart_s_ground_and_never_flips_the_set() {
    use crate::{adopt_pane, persist_pane, refresh_shared};
    use quantick_workspace::market_settings::{DrawingKey, MarketSettings};

    let mut store = store("look-shared");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let (paper, island) = (ChartSurface::Paper.into(), ChartSurface::Flow.into());
    let line = tool("horizontal-line");
    let owner = quantick_workspace::market_settings::DrawingScope {
        workspace: 7,
        layout: quantick_workspace::layout_document::LayoutId(1),
    };
    let market = |role: Option<&str>| {
        DrawingKey::of_pane("binance", "BTCUSDT".to_owned(), role.map(str::to_owned))
            .in_scope(owner)
    };
    let (time_key, flow_key) = (market(Some("time:5m")), market(None));
    let mut settings = MarketSettings::default();
    let (mut time, mut time_base) = (Drawings::on_canvas(paper), Vec::new());
    let (mut flow, mut flow_base) = (Drawings::on_canvas(island), Vec::new());
    time.set_layout(Some(owner.layout));
    flow.set_layout(Some(owner.layout));
    adopt_pane(&mut time, &mut time_base, &settings, &time_key, "time:5m");
    adopt_pane(&mut flow, &mut flow_base, &settings, &flow_key, "tick:100");
    time.follow_ground(&store, paper);
    flow.follow_ground(&store, island);

    let point = ChartPoint::at_time(1.0, 100.0, Some(1_700_000_000_000));
    time.place_with(line, &DrawingBand::Price, point, |tool| {
        new_drawing_from_defaults(&store, paper, tool)
    });
    time.selected_mut().expect("just placed").scope = crate::DrawingScope::AllCharts;
    let write = persist_pane(
        &mut time,
        &mut time_base,
        &mut settings,
        &time_key,
        "time:5m",
    );
    assert!(write.all_timeframes);
    let (light, dark) = (
        style_on(&store, paper, line),
        style_on(&store, island, line),
    );
    assert_ne!(light, dark);

    // A default saved on either ground never reaches the shared mark.
    store.set_default_style(line.id(), Ground::Light, Some(red()));
    store.set_default_style(line.id(), Ground::Dark, Some(red()));
    let revision = flow.revision();
    refresh_shared(&mut flow, &mut flow_base, &settings, &flow_key, "tick:100");
    assert_eq!(flow.items()[0].style, dark, "the arrival wears it at once");
    assert_eq!(flow.revision(), revision + 1, "one refresh, no edit");
    assert!(!flow.follow_ground(&store, island));
    assert!(!time.follow_ground(&store, paper));
    assert_eq!(time.items()[0].style, light);

    // The flow chart writes an edit of its own: the shared set keeps its ink.
    assert!(flow.place(line, ChartPoint::at(2.0, 90.0)));
    let write = persist_pane(
        &mut flow,
        &mut flow_base,
        &mut settings,
        &flow_key,
        "tick:100",
    );
    assert!(
        write.own && !write.all_timeframes && !write.merged,
        "{write:?}"
    );
    let stored = |settings: &MarketSettings| settings.shared_drawings(&time_key);
    assert_eq!(stored(&settings).len(), 1);
    assert!(stored(&settings)[0].follows_ground);
    let ink = |entry: &SavedDrawing| entry.to_drawing(DrawingId(1)).unwrap().style;
    assert_eq!(ink(&stored(&settings)[0]), light);

    // A second write from it, the shared mark moved there: the file keeps the
    // writer-neutral look, and nothing merges.
    let before = flow.items()[0].clone();
    flow.items_mut()[0].points[0].price += 5.0;
    flow.record_edit_of(0, before);
    assert_eq!(
        flow.items()[0].look,
        Inheritance::Theme,
        "a move is no look"
    );
    let write = persist_pane(
        &mut flow,
        &mut flow_base,
        &mut settings,
        &flow_key,
        "tick:100",
    );
    assert!(write.all_timeframes && !write.merged, "{write:?}");
    assert_eq!(ink(&stored(&settings)[0]), light);
    assert!(stored(&settings)[0].follows_ground);
    refresh_shared(&mut time, &mut time_base, &settings, &time_key, "time:5m");
    assert!(!time.follow_ground(&store, paper));
    let price = |drawings: &Drawings<DrawingTool>| drawings.items()[0].points[0].price;
    assert_eq!(price(&time), price(&flow));
    assert_eq!(time.items()[0].style, light);
    assert_eq!(flow.items()[0].style, dark);
}

/// A repaint puts on the theme's ink and nothing else: config the mark
/// carries (a band run to the chart's edge, which an armed strategy reads)
/// survives the switch, so a following mark is never reset to the factory.
#[test]
fn a_repaint_changes_the_ink_and_keeps_the_config() {
    let mut store = store("look-keeps-config");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let paper: PaneCanvas = ChartSurface::Paper.into();
    let band = tool("rectangle");
    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, paper);
    open(&mut drawings, &store, paper, band);
    drawings.items_mut()[0]
        .payload
        .as_any_mut()
        .downcast_mut::<crate::RectanglePayload>()
        .expect("a rectangle carries a rectangle payload")
        .extend_right = true;

    theme(&mut appearance, &mut store, DARK);
    assert!(drawings.follow_ground(&store, paper));
    let mark = &drawings.items()[0];
    assert_eq!(mark.look, Inheritance::Theme);
    assert_eq!(mark.style, factory(&store, paper, band), "the dark ink");
    let payload = mark
        .payload
        .as_any()
        .downcast_ref::<crate::RectanglePayload>()
        .expect("still a rectangle");
    assert!(payload.extend_right, "the band still runs to the edge");
}

/// A position's ratio is its plan, not its look: picking one leaves the mark
/// following its ground, so a light pane's black stop turns light grey on a
/// dark one. A colour pick makes the look the trader's.
#[test]
fn a_ratio_pick_keeps_a_position_on_its_ground_and_a_colour_pick_owns_it() {
    use crate::position_edit::{PositionEdit, PositionLeg, edited_position};
    const FACTORY_STOP_RGB: [u8; 3] = [0, 0, 0];
    const FACTORY_STOP_RGB_ON_DARK: [u8; 3] = [0xB2, 0xB5, 0xBE];
    let mut store = store("look-position-ratio");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let paper: PaneCanvas = ChartSurface::Paper.into();
    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, paper);
    open(&mut drawings, &store, paper, tool("long-position"));
    open(&mut drawings, &store, paper, tool("long-position"));
    let stop = |drawings: &Drawings<DrawingTool>, index: usize| {
        let drawing = &drawings.items()[index];
        let payload = drawing.payload.as_any();
        let payload = payload.downcast_ref::<crate::PositionPayload>().unwrap();
        (drawing.look, payload.stop_rgb, payload.ratio)
    };
    assert_eq!(
        stop(&drawings, 0),
        (Inheritance::Theme, FACTORY_STOP_RGB, 1.0)
    );

    let ratio = edited_position(&drawings.items()[0], PositionEdit::Ratio(2.0)).unwrap();
    drawings.replace_edited(0, ratio);
    let red = edited_position(
        &drawings.items()[1],
        PositionEdit::Colour(PositionLeg::Stop, [200, 0, 0]),
    )
    .unwrap();
    drawings.replace_edited(1, red);
    theme(&mut appearance, &mut store, DARK);
    drawings.follow_ground(&store, paper);
    assert_eq!(
        stop(&drawings, 0),
        (Inheritance::Theme, FACTORY_STOP_RGB_ON_DARK, 2.0)
    );
    assert_eq!(
        stop(&drawings, 1),
        (Inheritance::Explicit, [200, 0, 0], 1.0)
    );
}
