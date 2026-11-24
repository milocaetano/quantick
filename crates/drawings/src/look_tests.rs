//! Marks that follow their pane's ground, and marks whose look is the trader's.

use quantick_appearance::{
    Appearance, ChartSurface, DARK, Ground, Inheritance, LIGHT, PaneCanvas, PremultipliedRgba,
    ResolvedTheme,
};

use crate::presets::{PRESETS_FILE, PresetStore};
use crate::{
    ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingStyle, DrawingTool, Drawings, PresetHost,
    TextPayload, ToolCatalog, new_drawing_from_defaults,
};

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

/// A default the trader saved on the dark ground is what a following mark on
/// a dark pane wears, and saving it repaints the marks still following.
#[test]
fn a_saved_dark_default_paints_the_following_marks_on_a_dark_pane() {
    let mut store = store("look-dark-bank");
    let mut appearance = Appearance::default();
    theme(&mut appearance, &mut store, LIGHT);
    let island: PaneCanvas = ChartSurface::Flow.into();
    let line = tool("trend-line");
    let mut drawings = Drawings::default();
    drawings.follow_ground(&store, island);
    open(&mut drawings, &store, island, line);
    store.set_default_style(line.id(), Ground::Dark, Some(red()));
    assert!(drawings.follow_ground(&store, island));
    assert_eq!(drawings.items()[0].style, red());
    // The light ground's bank stays out of it.
    store.set_default_style(line.id(), Ground::Light, Some(DrawingStyle::default()));
    drawings.follow_ground(&store, island);
    assert_eq!(drawings.items()[0].style, red());
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
    let light = drawings.items()[0].style;
    // The trader darkens the chart: the profile follows the canvas.
    assert!(drawings.follow_ground(&store, made_dark));
    assert_eq!(drawings.canvas(), made_dark);
    assert_ne!(drawings.items()[0].style, light);
    assert_eq!(
        drawings.items()[0].style,
        style_on(&store, made_dark, profile)
    );
}

/// Marks arriving from elsewhere were resolved on another canvas: the next
/// call resolves them again, though nothing it is keyed on moved.
#[test]
fn adopted_and_pasted_marks_resolve_on_this_pane_s_canvas() {
    let mut store = store("look-adopt");
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
    assert!(drawings.follow_ground(&store, island));
    assert_eq!(drawings.items()[0].style, style_on(&store, island, line));
    drawings.adopt([mark]);
    assert!(drawings.follow_ground(&store, island));
    assert_eq!(drawings.items()[1].style, style_on(&store, island, line));
}
