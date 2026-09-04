// The `drawings/mod.rs` unit tests, moved out of the file so a session
// opening a drawing tool no longer reads 2,134 lines of tests it did not
// ask for.
//
// They stay a child module of `crate::drawings` rather than moving to an
// integration test: a child sees its ancestor's private items, so the move
// widens no visibility in production code, and the `use super::*` below is
// the line the module already had inline.

use super::*;

fn tool(id: &str) -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .expect("registered test tool")
}

/// A second implementation of the preset port, remembering everything in
/// memory. The store on disk is the first; this one proves the port is a
/// port — nothing above it knows a file is involved — and lets the
/// defaults flow be tested without touching a real preset file.
#[derive(Debug, Default)]
struct MemoryPresetHost {
    presets: std::collections::BTreeMap<(String, String), toml::Value>,
    default_preset: std::collections::BTreeMap<String, String>,
    styles: std::collections::BTreeMap<String, DrawingStyle>,
    configs: std::collections::BTreeMap<String, toml::Value>,
}

impl PresetHost for MemoryPresetHost {
    fn custom_preset_names(&self, tool_id: &str) -> Vec<String> {
        self.presets
            .keys()
            .filter(|(id, _)| id == tool_id)
            .map(|(_, name)| name.clone())
            .collect()
    }
    fn load_custom_preset(&self, tool_id: &str, name: &str) -> Option<toml::Value> {
        self.presets
            .get(&(tool_id.to_owned(), name.to_owned()))
            .cloned()
    }
    fn save_custom_preset(
        &mut self,
        tool_id: &str,
        name: &str,
        value: toml::Value,
        overwrite: bool,
    ) -> bool {
        let key = (tool_id.to_owned(), name.to_owned());
        if self.presets.contains_key(&key) && !overwrite {
            return false;
        }
        self.presets.insert(key, value);
        true
    }
    fn delete_custom_preset(&mut self, tool_id: &str, name: &str) {
        self.presets.remove(&(tool_id.to_owned(), name.to_owned()));
    }
    fn default_preset(&self, tool_id: &str) -> Option<String> {
        self.default_preset.get(tool_id).cloned()
    }
    fn set_default_preset(&mut self, tool_id: &str, name: Option<String>) {
        match name {
            Some(name) => self.default_preset.insert(tool_id.to_owned(), name),
            None => self.default_preset.remove(tool_id),
        };
    }
    fn default_style(&self, tool_id: &str) -> Option<DrawingStyle> {
        self.styles.get(tool_id).copied()
    }
    fn set_default_style(&mut self, tool_id: &str, style: Option<DrawingStyle>) {
        match style {
            Some(style) => self.styles.insert(tool_id.to_owned(), style),
            None => self.styles.remove(tool_id),
        };
    }
    fn default_config(&self, tool_id: &str) -> Option<toml::Value> {
        self.configs.get(tool_id).cloned()
    }
    fn set_default_config(&mut self, tool_id: &str, value: Option<toml::Value>) {
        match value {
            Some(value) => self.configs.insert(tool_id.to_owned(), value),
            None => self.configs.remove(tool_id),
        };
    }
    fn has_default_config(&self, tool_id: &str) -> bool {
        self.configs.contains_key(tool_id)
    }
}

/// The complaint this closes, in one flow: configure a Fib once, say
/// "like this from now on", and the next one opens that way — colours,
/// levels and all — while the ones already drawn stay exactly as they
/// are. Then reset, and the tool opens the way it did out of the box.
#[test]
fn a_saved_default_shapes_the_next_object_and_never_the_ones_already_drawn() {
    use crate::drawings::fib::{FibPayload, LabelMode};

    let mut host = MemoryPresetHost::default();
    let fib = tool("fib-retracement");
    let factory_levels = {
        let payload = fib.default_payload();
        payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len()
    };

    // One object, configured by hand the way a trader would: fewer
    // levels, one of them in its own colour, ratios read as ratios.
    let mut drawings = Drawings::default();
    assert!(!drawings.place(fib, ChartPoint::at(1.0, 100.0)));
    assert!(drawings.place(fib, ChartPoint::at(9.0, 200.0)));
    let mine = DrawingStyle {
        color: egui::Color32::from_rgb(0x20, 0xC0, 0x80),
        width_px: 2.5,
        fill_alpha: 30,
    };
    {
        let drawing = drawings.selected_mut().expect("placement selects");
        drawing.style = mine;
        let payload = drawing
            .payload
            .as_any_mut()
            .downcast_mut::<FibPayload>()
            .expect("fib payload");
        payload.levels.truncate(3);
        payload.levels[1].color = Some([0xFF, 0x00, 0x88]);
        payload.label_mode = LabelMode::RatioPrice;
    }
    save_tool_default(&mut host, &drawings.items()[0].clone());

    // The next one opens configured, with no name invented for it.
    let fresh = new_drawing_from_defaults(&host, fib);
    assert_eq!(fresh.style, mine, "the look travels with the configuration");
    let fresh_fib = fresh
        .payload
        .as_any()
        .downcast_ref::<FibPayload>()
        .expect("fib payload");
    assert_eq!(fresh_fib.levels.len(), 3);
    assert_eq!(fresh_fib.levels[1].color, Some([0xFF, 0x00, 0x88]));
    assert_eq!(fresh_fib.label_mode, LabelMode::RatioPrice);

    // A tool that was never taught anything still opens factory-fresh.
    let untouched = new_drawing_from_defaults(&host, tool("fib-extension"));
    assert_eq!(untouched.style, tool("fib-extension").default_style());

    // Reset puts the tool back the way it shipped...
    reset_tool_default(&mut host, fib);
    assert!(!has_saved_default(&host, fib));
    let after_reset = new_drawing_from_defaults(&host, fib);
    assert_eq!(
        after_reset
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len(),
        factory_levels
    );
    assert_eq!(after_reset.style, fib.default_style());

    // ...and the object on the chart never moved through any of it.
    let on_chart = drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<FibPayload>()
        .expect("fib payload");
    assert_eq!(on_chart.levels.len(), 3);
    assert_eq!(drawings.items()[0].style, mine);
}

/// Precedence, stated once: a preset the trader *named* and chose as the
/// default beats one they saved by pressing a button, because naming it
/// was the more deliberate act.
#[test]
fn a_named_default_preset_outranks_the_button_saved_configuration() {
    use crate::drawings::fib::{FibKind, FibPayload};

    let mut host = MemoryPresetHost::default();
    let fib = tool("fib-retracement");
    let mut by_button = FibPayload::new(FibKind::Retracement);
    by_button.levels.truncate(2);
    host.set_default_config(fib.id(), by_button.export_preset());

    let mut named = FibPayload::new(FibKind::Retracement);
    named.levels.truncate(5);
    host.save_custom_preset(
        fib.id(),
        "mine",
        named.export_preset().expect("export"),
        false,
    );
    host.set_default_preset(fib.id(), Some("mine".to_owned()));

    let fresh = new_drawing_from_defaults(&host, fib);
    assert_eq!(
        fresh
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len(),
        5,
        "the named default preset wins"
    );
}

/// The rectangle a popup must keep clear of is what the tool *paints*,
/// not where its anchors sit. A fixed-range profile carries two anchors at
/// one price and covers the axis; placing against the anchors walks around
/// a thin sliver and lands in the middle of the figure.
#[test]
fn a_tool_that_paints_past_its_anchors_says_so() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
    let anchors = egui::Rect::from_min_max(egui::pos2(300.0, 290.0), egui::pos2(500.0, 310.0));

    let profile = tool("fixed-range-profile");
    let painted = profile.painted_bounds(anchors, chart);
    assert_eq!(
        painted.x_range(),
        anchors.x_range(),
        "its time span is its own"
    );
    assert_eq!(
        (painted.top(), painted.bottom()),
        (chart.top(), chart.bottom()),
        "and it covers every price on screen"
    );

    let vertical = tool("vertical-line");
    assert_eq!(
        (
            vertical.painted_bounds(anchors, chart).top(),
            vertical.painted_bounds(anchors, chart).bottom()
        ),
        (chart.top(), chart.bottom()),
    );

    let horizontal = tool("horizontal-line");
    let painted = horizontal.painted_bounds(anchors, chart);
    assert_eq!(
        (painted.left(), painted.right()),
        (chart.left(), chart.right()),
        "edge to edge"
    );

    // Everything else ends where its anchors do, unchanged.
    let trend = tool("trend-line");
    assert_eq!(trend.painted_bounds(anchors, chart), anchors);
    let rect = tool("rectangle");
    assert_eq!(rect.painted_bounds(anchors, chart), anchors);
}

/// The vector-icon port's contract: every declared polyline has at
/// least two points and stays inside the unit square, so the paint
/// helper never has to clamp. The tools that replaced a lying glyph
/// must actually declare strokes — an accidental `&[]` would silently
/// bring the lozenge back.
#[test]
fn icon_strokes_have_two_points_each_and_stay_in_the_unit_square() {
    for tool in DRAWING_TOOLS {
        for polyline in tool.icon_strokes() {
            assert!(
                polyline.len() >= 2,
                "{}: an icon stroke needs at least two points",
                tool.id()
            );
            for (x, y) in *polyline {
                assert!(
                    (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y),
                    "{}: icon point ({x}, {y}) leaves the unit square",
                    tool.id()
                );
            }
        }
    }
    for id in ["parallel-channel", "fib-retracement", "fib-extension"] {
        assert!(
            !tool(id).icon_strokes().is_empty(),
            "{id} replaced its glyph and must keep vector strokes"
        );
    }
}

/// The letter half of the same port: it lives in the same unit square,
/// is one character rather than a word, and only exists on a tool that
/// draws its own strokes — over a font glyph it would land wherever the
/// typeface happened to leave a gap.
///
/// The two Fib tools are named because the letters are the whole reason
/// the port exists: one leg, one set of rungs, two tools. The letters
/// must differ from each other and share a column and a size, or they
/// stop reading as a choice between two rows and start reading as two
/// unrelated marks.
#[test]
fn icon_letters_stay_in_the_unit_square_and_name_the_two_fib_tools() {
    for tool in DRAWING_TOOLS {
        let Some(letter) = tool.icon_letter() else {
            continue;
        };
        assert!(
            !tool.icon_strokes().is_empty(),
            "{}: a letter needs strokes to sit in",
            tool.id()
        );
        let (x, y) = letter.at;
        assert!(
            (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
            "{}: icon letter at ({x}, {y}) leaves the unit square",
            tool.id()
        );
        assert!(
            letter.height > 0.0 && letter.height <= 1.0,
            "{}: an icon letter is a fraction of the glyph box",
            tool.id()
        );
        assert_eq!(
            letter.text.chars().count(),
            1,
            "{}: the icon has room for a letter, not a word",
            tool.id()
        );
    }
    // A family slot borrows a member's picture, and the same rule holds
    // there: `IconButton` only reaches the letter on the strokes branch,
    // so a family that declared a letter over a font glyph would carry a
    // letter nothing ever paints.
    for family in DRAWING_TOOLS.into_iter().filter_map(DrawingTool::family) {
        if let Some(letter) = family.icon_letter {
            assert!(
                !family.icon_strokes.is_empty(),
                "{}: a family letter needs strokes to sit in",
                family.id
            );
            assert_eq!(
                letter.text.chars().count(),
                1,
                "{}: the slot has room for a letter, not a word",
                family.id
            );
        }
    }
    let retracement = tool("fib-retracement")
        .icon_letter()
        .expect("the retracement icon carries its letter");
    let extension = tool("fib-extension")
        .icon_letter()
        .expect("the extension icon carries its letter");
    assert_ne!(
        retracement.text, extension.text,
        "the two Fib icons must not answer with the same letter"
    );
    assert_eq!(
        (retracement.at.0, retracement.height),
        (extension.at.0, extension.height),
        "the two Fib letters share one column and one size, or they stop reading as a pair"
    );
    assert_ne!(
        retracement.at.1, extension.at.1,
        "each letter takes the corner its own levels leave empty, so the two sit apart"
    );
}

/// The two halves of the note port answer together: `holds_text` is the
/// cheap question the placement path asks with no object in hand, and
/// `inline_text` is the borrow the editor reads. A tool that claimed one
/// and implemented the other would take the caret and then have nowhere
/// to put the words — or hold words nothing ever opens.
#[test]
fn a_tool_that_claims_words_can_actually_hand_them_over() {
    for tool in DRAWING_TOOLS {
        let payload = tool.default_payload();
        assert_eq!(
            tool.holds_text(),
            tool.inline_text(payload.as_ref()).is_some(),
            "{}: holds_text and inline_text disagree",
            tool.id()
        );
        if tool.holds_text() {
            let mut payload = tool.default_payload();
            tool.set_inline_text(payload.as_mut(), "typed".to_owned());
            assert_eq!(
                tool.inline_text(payload.as_ref()),
                Some("typed"),
                "{}: what is written must read back",
                tool.id()
            );
        }
    }
}

/// The dots half of the same port: they live in the same unit square,
/// and a tool only has them alongside strokes — a font glyph draws its
/// own anchors or none at all, and dots floating over one would land
/// wherever the typeface happened to put its ink.
///
/// The two Fib tools are named because their anchor counts are the whole
/// difference between their icons: drop the dots and the flyout offers
/// two rows of the same picture.
#[test]
fn icon_dots_stay_in_the_unit_square_and_only_accompany_strokes() {
    for tool in DRAWING_TOOLS {
        let dots = tool.icon_dots();
        assert!(
            dots.is_empty() || !tool.icon_strokes().is_empty(),
            "{}: anchor dots need strokes to sit on",
            tool.id()
        );
        for (x, y) in dots {
            assert!(
                (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y),
                "{}: icon dot ({x}, {y}) leaves the unit square",
                tool.id()
            );
        }
    }
    for (id, anchors) in [("fib-retracement", 2), ("fib-extension", 3)] {
        assert_eq!(
            tool(id).icon_dots().len(),
            anchors,
            "{id}: the icon marks one dot per anchor of the gesture"
        );
        assert_eq!(
            tool(id).icon_dots().len(),
            tool(id).required_points(),
            "{id}: the icon and the tool must agree on how many anchors it takes"
        );
    }
}

#[test]
fn every_registered_tool_has_metadata_and_a_valid_point_count() {
    let mut ids = Vec::with_capacity(DRAWING_TOOLS.len());
    for tool in DRAWING_TOOLS {
        assert!(!tool.id().is_empty());
        assert!(!ids.contains(&tool.id()), "duplicate tool id {}", tool.id());
        ids.push(tool.id());
        assert!(!tool.icon().is_empty());
        assert!(!tool.settings_title().is_empty());
        assert!(!tool.hover_text().is_empty());
        // Zero anchors is legal for exactly one shape of tool: a
        // freehand one, whose count is whatever the gesture gave. Any
        // other tool answering zero would never complete a draft.
        assert_eq!(
            tool.required_points() == 0,
            tool.freehand(),
            "{} must declare an anchor count unless it is freehand",
            tool.id()
        );
        assert!(
            !tool.freehand() || tool.placement_hint(0).is_some(),
            "{} is placed by a gesture nobody has seen before; it has to say so",
            tool.id()
        );
    }
}

/// A tool of three or more anchors says what every step wants, in words.
///
/// Two anchors need no words: the drag that starts the object also
/// finishes it, so the trader is never left holding one. The third anchor
/// is where they get stranded — the gesture ends, the object sits there,
/// and the `n/N` badge is on the far side of the screen from the cursor.
/// A new tool that stays silent mid-placement fails here rather than in
/// the running app.
#[test]
fn a_tool_that_outlasts_its_drag_says_what_each_step_wants() {
    for tool in DRAWING_TOOLS
        .into_iter()
        .filter(|tool| tool.required_points() >= 3)
    {
        for placed in 1..tool.required_points() {
            assert!(
                tool.placement_hint(placed).is_some(),
                "{} says nothing with {placed} anchors down",
                tool.id()
            );
        }
    }
}

/// The shaping port is opt-in: it changed nothing for a tool that did not
/// ask for it. Only the two whose third anchor gives a shape its
/// thickness — and which are therefore the two that can collapse into a
/// line — move the pointer at all.
#[test]
fn the_shaping_port_defaults_to_the_pointer_the_trader_is_holding() {
    let placed = [egui::pos2(10.0, 10.0), egui::pos2(110.0, 60.0)];
    // Exactly on the line between the two anchors: the collapsed case,
    // where a tool that shapes has to move and one that does not must not.
    let cursor = egui::pos2(60.0, 35.0);
    let mut shaping: Vec<&'static str> = DRAWING_TOOLS
        .into_iter()
        .filter(|tool| tool.pending_anchor(&placed, cursor, Constrain::Free) != cursor)
        .map(DrawingTool::id)
        .collect();
    shaping.sort_unstable();
    assert_eq!(shaping, vec!["parallel-channel", "triangle"]);
}

/// Which tools Shift means something to, named exactly. Everything else
/// answers with the pointer, so a trader holding the modifier out of
/// habit on some other tool gets no surprise — and a tool that *should*
/// answer to it fails here until it is listed.
#[test]
fn shift_reaches_exactly_the_tools_with_an_angle_to_hold() {
    // One anchor down: the far end of a line, which is the step the
    // modifier is about for every tool that has one.
    let placed = [egui::pos2(10.0, 10.0)];
    let cursor = egui::pos2(110.0, 60.0);
    let mut levelled: Vec<&'static str> = DRAWING_TOOLS
        .into_iter()
        .filter(|tool| tool.pending_anchor(&placed, cursor, Constrain::Level) != cursor)
        .map(DrawingTool::id)
        .collect();
    levelled.sort_unstable();
    assert_eq!(levelled, vec!["parallel-channel", "trend-line"]);
}

/// The host skips the shaping port for a freehand draft, and the reason
/// is runtime: a pencil stroke holds hundreds of anchors, and projecting
/// every one of them up to three times a frame just to consult the port
/// would cost far more than the gesture is worth — see
/// `ChartPane::shaped_placement`.
///
/// That skip is only sound while no freehand tool actually wants its
/// anchors shaped; otherwise the host would be ignoring a tool that
/// asked, in silence. This is the test that holds the two facts together,
/// so a pencil that one day wants shaping fails here instead of being
/// quietly disobeyed.
#[test]
fn no_freehand_tool_asks_to_shape_its_anchors() {
    let placed = [egui::pos2(10.0, 10.0), egui::pos2(110.0, 60.0)];
    let cursor = egui::pos2(60.0, 35.0);
    for tool in DRAWING_TOOLS.into_iter().filter(|tool| tool.freehand()) {
        assert_eq!(
            tool.pending_anchor(&placed, cursor, Constrain::Free),
            cursor,
            "{} is freehand, and the host does not consult the port for one",
            tool.id()
        );
    }
}

/// The shared half of the port: push off the line, keep the position
/// along it.
#[test]
fn off_line_by_moves_across_the_line_and_never_along_it() {
    let start = egui::pos2(0.0, 0.0);
    let end = egui::pos2(100.0, 0.0);
    let pushed = off_line_by(start, end, egui::pos2(40.0, 1.0), 10.0);
    assert!(
        (pushed.x - 40.0).abs() < 1e-3,
        "kept its place along the line"
    );
    assert!((pushed.y - 10.0).abs() < 1e-3, "stands the floor off it");
    let clear = egui::pos2(40.0, -25.0);
    assert_eq!(
        off_line_by(start, end, clear, 10.0),
        clear,
        "a point already clear is untouched"
    );
}

/// A "line" whose ends are one point has no direction of its own. It gets
/// the vertical, which is the axis a chart measures in — the alternative
/// is a NaN anchor, which is an object that can never be drawn or deleted.
#[test]
fn a_line_of_no_length_still_pushes_off_itself() {
    let point = egui::pos2(50.0, 50.0);
    let pushed = off_line_by(point, point, point, 10.0);
    assert!(pushed.is_finite());
    assert!((pushed.y - 60.0).abs() < 1e-3, "pushed along the vertical");
}

/// A price-only tool started over an indicator band still lands on the
/// candles' price axis: profile rows are prices, and a profile hanging
/// off a CVD axis would read its rows as CVD values.
#[test]
fn a_price_only_tool_lands_on_the_price_band_wherever_it_started() {
    let mut drawings = Drawings::default();
    let frvp = tool("fixed-range-profile");
    let band = DrawingBand::Indicator(PaneKey {
        kind: "native.cvd".into(),
        ordinal: 0,
    });
    drawings.place_on(frvp, &band, ChartPoint::at(1.0, 5.0));
    drawings.place_on(frvp, &band, ChartPoint::at(4.0, 9.0));
    assert_eq!(drawings.items()[0].band, DrawingBand::Price);
}

#[test]
fn horizontal_line_completes_with_one_point() {
    let mut drawings = Drawings::default();
    assert!(drawings.place(tool("horizontal-line"), ChartPoint::at(3.0, 100.0)));
}

#[test]
fn channel_needs_three_points() {
    let mut drawings = Drawings::default();
    let channel = tool("parallel-channel");
    for bar in [1.0, 2.0] {
        assert!(!drawings.place(channel, ChartPoint::at(bar, 1.0)));
    }
    assert!(drawings.place(channel, ChartPoint::at(3.0, 1.0)));
}

#[test]
fn moving_a_selected_drawing_preserves_its_shape() {
    let mut drawings = Drawings::default();
    let rectangle = tool("rectangle");
    drawings.place(rectangle, ChartPoint::at(1.0, 100.0));
    drawings.place(rectangle, ChartPoint::at(3.0, 110.0));
    drawings.translate_selected(2.0, -5.0);
    assert_eq!(
        drawings.items()[0].points,
        [ChartPoint::at(3.0, 95.0), ChartPoint::at(5.0, 105.0)]
    );
}

#[test]
fn moving_one_anchor_does_not_move_the_other_points() {
    let mut drawings = Drawings::default();
    let rectangle = tool("rectangle");
    drawings.place(rectangle, ChartPoint::at(1.0, 100.0));
    drawings.place(rectangle, ChartPoint::at(3.0, 110.0));
    let opposite = drawings.items()[0].points[1];
    let replacement = ChartPoint::at(0.5, 95.0);

    assert!(drawings.move_anchor(0, 0, replacement));

    assert_eq!(drawings.items()[0].points[0], replacement);
    assert_eq!(drawings.items()[0].points[1], opposite);
    assert!(!drawings.move_anchor(5, 0, replacement));
    assert!(!drawings.move_anchor(0, 5, replacement));
}

#[test]
fn deleting_the_selected_drawing_clears_both_object_and_selection() {
    let mut drawings = Drawings::default();
    let line = tool("horizontal-line");
    assert!(drawings.place(line, ChartPoint::at(1.0, 100.0)));
    assert_eq!(drawings.selected(), Some(0));

    assert_eq!(drawings.delete_selected(false), DeleteOutcome::Deleted);

    assert!(drawings.items().is_empty());
    assert_eq!(drawings.selected(), None);
}

/// The id is identity where the index is only position: reorders and
/// deletes move objects around the `Vec`, and the id keeps naming the
/// same object through all of it.
#[test]
fn ids_survive_reorder_and_delete_where_indices_do_not() {
    let mut drawings = Drawings::default();
    let line = tool("horizontal-line");
    for price in [100.0, 110.0, 120.0] {
        drawings.place(line, ChartPoint::at(1.0, price));
    }
    let second = drawings.items()[1].id;
    assert_eq!(drawings.index_of(second), Some(1));

    drawings.bring_to_front(0);
    drawings.select(Some(drawings.index_of(drawings.items()[0].id).unwrap()));
    assert_eq!(
        drawings.index_of(second),
        Some(0),
        "the reorder moved it; the id still finds it"
    );

    drawings.select(drawings.index_of(second));
    drawings.delete_selected(false);
    assert_eq!(
        drawings.index_of(second),
        None,
        "deleted is gone, not renumbered"
    );
}

#[test]
fn an_undone_delete_restores_the_object_under_its_old_id() {
    let mut drawings = Drawings::default();
    drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
    let id = drawings.items()[0].id;
    drawings.delete_selected(false);
    assert_eq!(drawings.index_of(id), None);
    drawings.undo();
    assert_eq!(
        drawings.index_of(id),
        Some(0),
        "an undo restores identity, not a lookalike"
    );

    // And a fresh object after the undo still gets an id never seen
    // before — the counter does not rewind with the history.
    drawings.place(tool("horizontal-line"), ChartPoint::at(2.0, 105.0));
    assert!(drawings.items()[1].id > id);
}

#[test]
fn a_duplicate_is_a_new_object_with_no_name() {
    let mut drawings = Drawings::default();
    drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
    drawings.rename_at(0, "congestão 108k");
    // These assert on the store, not on what rode across.
    let _ = drawings.duplicate_selected(2.0);
    let [original, copy] = drawings.items() else {
        panic!("one original and one copy");
    };
    assert_ne!(original.id, copy.id);
    assert_eq!(original.name.as_deref(), Some("congestão 108k"));
    assert_eq!(copy.name, None);
}

#[test]
fn rename_is_one_undo_step_and_whitespace_clears_it() {
    let mut drawings = Drawings::default();
    let line = tool("horizontal-line");
    drawings.place(line, ChartPoint::at(1.0, 100.0));
    assert_eq!(
        drawings.items()[0].display_label(0),
        format!("{} 1", line.name()),
        "no name falls back to the derived label"
    );

    drawings.rename_at(0, "  zona de venda  ");
    assert_eq!(drawings.items()[0].name.as_deref(), Some("zona de venda"));
    assert_eq!(drawings.items()[0].display_label(0), "zona de venda");

    drawings.rename_at(0, "   ");
    assert_eq!(drawings.items()[0].name, None, "whitespace clears the name");

    drawings.undo();
    assert_eq!(drawings.items()[0].name.as_deref(), Some("zona de venda"));
    drawings.undo();
    assert_eq!(drawings.items()[0].name, None);
}

#[test]
fn a_locked_drawing_ignores_geometry_edits_until_unlocked() {
    let mut drawings = Drawings::default();
    drawings.place(tool("horizontal-line"), ChartPoint::at(2.0, 100.0));
    drawings.set_selected_locked(true);

    drawings.translate_selected(3.0, 5.0);
    assert!(!drawings.move_anchor(0, 0, ChartPoint::at(9.0, 50.0)));
    assert_eq!(
        drawings.items()[0].points[0],
        ChartPoint::at(2.0, 100.0),
        "locked geometry must not move"
    );

    drawings.set_selected_locked(false);
    drawings.translate_selected(3.0, 5.0);
    assert_eq!(drawings.items()[0].points[0], ChartPoint::at(5.0, 105.0));
}

#[test]
fn deleting_a_locked_drawing_requires_explicit_force() {
    let mut drawings = Drawings::default();
    drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
    drawings.set_selected_locked(true);

    assert_eq!(
        drawings.delete_selected(false),
        DeleteOutcome::NeedsConfirmation
    );
    assert_eq!(drawings.items().len(), 1, "unforced delete must not land");

    assert_eq!(drawings.delete_selected(true), DeleteOutcome::Deleted);
    assert!(drawings.items().is_empty());

    assert!(drawings.undo(), "a forced delete is still undoable");
    assert_eq!(drawings.items().len(), 1);
    assert!(drawings.items()[0].locked, "undo restores the lock too");
}

/// Placing a drawing releases hide-all (audit M8): the finished object
/// must land on screen, never silently invisible behind the rail's eye —
/// and undo restores the hidden state along with the store.
#[test]
fn placing_a_drawing_releases_hide_all() {
    let mut drawings = Drawings::default();
    drawings.set_all_hidden(true);
    drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
    assert!(!drawings.all_hidden(), "the new mark is visible");
    assert!(drawings.undo(), "placement is one undoable step");
    assert!(drawings.items().is_empty());
    assert!(
        drawings.all_hidden(),
        "undoing the placement restores hide-all too"
    );
}

/// Delete-all is one command, one undo entry, locked objects included —
/// the caller's count-bearing confirmation is the gate, not the lock.
#[test]
fn delete_all_takes_everything_in_one_undoable_step() {
    let mut drawings = Drawings::default();
    assert_eq!(drawings.delete_all(), 0, "an empty store deletes nothing");
    drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
    drawings.set_selected_locked(true);
    drawings.place(tool("horizontal-line"), ChartPoint::at(2.0, 105.0));
    let undo_before = drawings.undo_depth();

    assert_eq!(drawings.delete_all(), 2, "locked and unlocked both go");
    assert!(drawings.items().is_empty());
    assert_eq!(
        drawings.undo_depth(),
        undo_before + 1,
        "one command, one entry"
    );
    assert!(drawings.undo(), "and it comes back whole");
    assert_eq!(drawings.items().len(), 2);
    assert!(
        drawings.items().iter().any(|drawing| drawing.locked),
        "the lock survives the round trip"
    );
}

#[test]
fn creating_dragging_and_deleting_are_one_undo_entry_each() {
    let mut drawings = Drawings::default();
    let rectangle = tool("rectangle");
    // Creation: two anchors, one entry.
    drawings.place(rectangle, ChartPoint::at(1.0, 100.0));
    drawings.place(rectangle, ChartPoint::at(3.0, 110.0));
    assert_eq!(drawings.undo_depth(), 1);

    // One drag over many frames: one entry.
    drawings.begin_gesture();
    for _ in 0..5 {
        drawings.translate_selected(0.5, 1.0);
    }
    drawings.commit_gesture();
    assert_eq!(drawings.undo_depth(), 2);

    // A gesture that changes nothing records nothing.
    drawings.begin_gesture();
    drawings.commit_gesture();
    assert_eq!(drawings.undo_depth(), 2);

    drawings.delete_selected(false);
    assert_eq!(drawings.undo_depth(), 3);

    assert!(drawings.undo(), "undo the delete");
    assert_eq!(drawings.items().len(), 1);
    assert!(drawings.undo(), "undo the drag");
    assert_eq!(drawings.items()[0].points[0], ChartPoint::at(1.0, 100.0));
    assert!(drawings.undo(), "undo the creation");
    assert!(drawings.items().is_empty());
    assert!(!drawings.undo(), "history is exhausted");
}

#[test]
fn redo_replays_an_undone_edit_until_a_new_command_clears_it() {
    let mut drawings = Drawings::default();
    drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
    drawings.undo();
    assert!(drawings.items().is_empty());
    assert!(drawings.redo());
    assert_eq!(drawings.items().len(), 1);

    drawings.undo();
    drawings.place(tool("horizontal-line"), ChartPoint::at(4.0, 90.0));
    assert!(!drawings.redo(), "a new command clears the redo stack");
}

#[test]
fn hide_all_is_a_layer_over_each_drawings_own_eye() {
    let mut drawings = Drawings::default();
    for price in [100.0, 105.0] {
        drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, price));
    }
    drawings.select(Some(0));
    drawings.set_selected_hidden(true);
    assert!(!drawings.is_visible(0));
    assert!(drawings.is_visible(1));

    drawings.set_all_hidden(true);
    assert!(!drawings.is_visible(0));
    assert!(!drawings.is_visible(1));

    drawings.set_all_hidden(false);
    assert!(
        !drawings.is_visible(0),
        "show-all must preserve the individual eye"
    );
    assert!(drawings.is_visible(1));

    drawings.select(Some(0));
    drawings.set_selected_hidden(false);
    assert!(drawings.is_visible(0));
}

#[test]
fn duplicate_lands_offset_unlocked_and_selected_as_one_entry() {
    let mut drawings = Drawings::default();
    drawings.place(tool("horizontal-line"), ChartPoint::at(4.0, 100.0));
    drawings.set_selected_locked(true);
    let depth = drawings.undo_depth();

    // These assert on the store, not on what rode across.
    let _ = drawings.duplicate_selected(2.0);

    assert_eq!(drawings.items().len(), 2);
    assert_eq!(drawings.selected(), Some(1), "the copy becomes selected");
    assert_eq!(drawings.items()[1].points[0].bar, 6.0, "the copy is offset");
    assert!(
        !drawings.items()[1].locked,
        "a copy starts unlocked even when the source was locked"
    );
    assert_eq!(drawings.undo_depth(), depth + 1);
    drawings.undo();
    assert_eq!(drawings.items().len(), 1);
}

#[test]
fn backspace_steps_back_one_draft_anchor_at_a_time() {
    let mut drawings = Drawings::default();
    let channel = tool("parallel-channel");
    for bar in [1.0, 2.0] {
        drawings.place(channel, ChartPoint::at(bar, 1.0));
    }
    assert_eq!(drawings.draft_len(), 2);

    drawings.remove_last_draft_anchor();
    assert_eq!(drawings.draft_len(), 1);
    drawings.remove_last_draft_anchor();
    assert!(
        drawings.draft().is_none(),
        "dropping the only anchor cancels the draft"
    );
}

#[test]
fn rectangle_interior_hit_tests_only_while_the_fill_is_visible() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
    let rectangle = tool("rectangle");
    let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
    let payload = rectangle.default_payload();
    let points = [egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)];
    let anchors = anchors_for(&points, &scale);
    let center = egui::pos2(150.0, 150.0);
    let border = egui::pos2(100.0, 150.0);

    let filled = DrawContext {
        payload: payload.as_ref(),
        anchors: &anchors,
        scale: &scale,
        px_per_bar: 20.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: false,
        halo: false,
        content_editing: false,
    };
    assert!(rectangle.hit_test(chart, &points, center, 5.0, &filled));

    let outline_only = DrawContext {
        style: DrawingStyle {
            fill_alpha: 0,
            ..DrawingStyle::default()
        },
        ..filled
    };
    assert!(
        !rectangle.hit_test(chart, &points, center, 5.0, &outline_only),
        "with no visible fill the interior belongs to the chart"
    );
    assert!(
        rectangle.hit_test(chart, &points, border, 5.0, &outline_only),
        "the border stays selectable without a fill"
    );
}

#[test]
fn lock_all_is_one_reversible_undo_entry_and_never_deletes() {
    let mut drawings = Drawings::default();
    for price in [100.0, 105.0] {
        drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, price));
    }
    let depth_before = drawings.undo_depth();

    drawings.set_all_locked(true);
    assert!(drawings.all_locked());
    assert_eq!(drawings.items().len(), 2);
    assert_eq!(drawings.undo_depth(), depth_before + 1);

    assert!(drawings.undo());
    assert!(!drawings.all_locked(), "undo releases the bulk lock");
    assert_eq!(drawings.items().len(), 2);
}

#[test]
fn undo_snapshots_shift_with_prepended_history() {
    let mut drawings = Drawings::default();
    drawings.place(tool("horizontal-line"), ChartPoint::at(2.0, 100.0));
    drawings.begin_gesture();
    drawings.translate_selected(1.0, 0.0);
    drawings.commit_gesture();

    drawings.shift_bars(3);
    assert_eq!(drawings.items()[0].points[0].bar, 6.0);

    drawings.undo();
    assert_eq!(
        drawings.items()[0].points[0].bar,
        5.0,
        "the undone position must sit on the shifted bars, not the stale ones"
    );
}

#[test]
fn selection_preserves_the_drawing_color_and_adds_white_handles() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
    let color = egui::Color32::from_rgb(0xFF, 0x9F, 0x43);
    let style = DrawingStyle {
        color,
        ..DrawingStyle::default()
    };
    let line = tool("horizontal-line");
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(chart),
        ..Default::default()
    };
    let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
    let payload = line.default_payload();
    let anchors = [ChartPoint::at(0.0, scale.price_at(120.0))];
    let output = ctx.run(input, |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("selection-test"),
        ));
        let ctxt = DrawContext {
            payload: payload.as_ref(),
            anchors: &anchors,
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style,
            selected: true,
            halo: false,
            content_editing: false,
        };
        line.paint(
            &painter,
            chart,
            style,
            &[egui::pos2(100.0, 120.0)],
            &ctxt,
            true,
        );
    });

    let mut kept_color = false;
    let mut ring_handle = false;
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::Shape::LineSegment { stroke, .. } => {
                kept_color |= stroke.color == egui::epaint::ColorMode::Solid(color);
            }
            egui::Shape::Circle(circle) => {
                ring_handle |= circle.fill == SELECTED_ANCHOR_FILL;
            }
            _ => {}
        }
    }
    assert!(
        kept_color,
        "selection must keep painting the configured colour"
    );
    assert!(ring_handle, "selection must add ring anchor handles");
}

/// Nothing that exists changes: every object still opens on the chart it
/// was drawn on, and sharing is something the trader asks for.
#[test]
fn a_new_drawing_belongs_to_the_chart_it_was_drawn_on() {
    let mut drawings = Drawings::default();
    assert!(drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0)));
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);
    assert!(!drawings.items()[0].shared());
}

/// Market time is the only coordinate two panes agree on, so an anchor
/// without one cannot be re-expressed — and none is invented for it.
#[test]
fn an_anchor_without_a_market_time_cannot_be_shared() {
    let mut untimed = Drawings::default();
    assert!(untimed.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0)));
    assert!(
        !untimed.items()[0].shareable(),
        "an anchor past the newest bar has no instant behind it"
    );

    let mut timed = Drawings::default();
    assert!(timed.place(
        tool("horizontal-line"),
        ChartPoint::at_time(1.0, 100.0, Some(1_700_000_000_000))
    ));
    assert!(timed.items()[0].shareable());
}

/// A shared object that is switched off shows nowhere, including on the
/// other pane: one eye, one answer.
#[test]
fn hiding_a_shared_drawing_hides_it_everywhere() {
    let mut drawings = Drawings::default();
    assert!(drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(1.0, 100.0, Some(1_700_000_000_000))
    ));
    drawings.selected_mut().expect("just placed").scope = DrawingScope::AllCharts;
    assert!(drawings.items()[0].shared());
    drawings.set_hidden_at(0, true);
    assert!(!drawings.items()[0].shared());
}

/// A multi-anchor object shares only when *every* anchor can be placed:
/// half a channel on the other chart would be a lie about where it is.
#[test]
fn one_untimed_anchor_blocks_sharing_the_whole_object() {
    let mut drawings = Drawings::default();
    let rectangle = tool("rectangle");
    assert!(!drawings.place(
        rectangle,
        ChartPoint::at_time(1.0, 100.0, Some(1_700_000_000_000))
    ));
    assert!(drawings.place(rectangle, ChartPoint::at(4.0, 110.0)));
    assert!(!drawings.items()[0].shareable());
}

/// A re-cut used to throw the whole store away, draft included. Nothing is
/// thrown away now, and a half-placed object is no exception: its first
/// anchor moves onto the new bars like any other, so the trader finishes
/// the shape they started rather than starting over.
#[test]
fn reanchoring_carries_an_unfinished_draft_too() {
    let mut drawings = Drawings::default();
    assert!(!drawings.place(
        tool("rectangle"),
        ChartPoint::at_time(6.0, 100.0, Some(1_700_000_006_000))
    ));

    drawings.reanchor(10, 5, halved);

    let draft = drawings.draft().expect("the draft survives the re-cut");
    assert_eq!(draft.points[0].bar, 3.0);
    assert_eq!(draft.points[0].time_ms, Some(1_700_000_006_000));
}

#[test]
fn prepending_history_shifts_completed_and_draft_bar_anchors() {
    let mut drawings = Drawings::default();
    drawings.place(tool("horizontal-line"), ChartPoint::at(2.5, 100.0));
    drawings.place(tool("rectangle"), ChartPoint::at(4.0, 101.0));

    drawings.shift_bars(3);

    assert_eq!(drawings.items()[0].points[0].bar, 5.5);
    assert_eq!(
        drawings.draft().expect("rectangle draft").points[0].bar,
        7.0
    );
}

/// A series re-cut so that every instant lands on half its old slot —
/// what doubling a timeframe does to the bars under a drawing.
fn halved(time: i64) -> Option<f32> {
    Some((time - 1_700_000_000_000) as f32 / 2_000.0)
}

#[test]
fn reanchoring_puts_an_anchor_on_the_slot_its_timestamp_now_lands_on() {
    let mut drawings = Drawings::default();
    drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(6.0, 100.0, Some(1_700_000_006_000)),
    );

    drawings.reanchor(10, 5, halved);

    assert_eq!(drawings.items()[0].points[0].bar, 3.0);
    assert_eq!(drawings.items()[0].points[0].price, 100.0);
    assert!(!drawings.items()[0].off_series);
}

#[test]
fn reanchoring_keeps_an_undated_anchor_past_the_end_of_the_new_series() {
    let mut drawings = Drawings::default();
    // Two bars past the newest of a ten-bar series, where the tape has
    // written nothing and there is no instant to look up.
    drawings.place(tool("horizontal-line"), ChartPoint::at(12.0, 100.0));

    drawings.reanchor(10, 5, halved);

    // Still two bars past the newest, now that the newest is bar 5.
    assert_eq!(drawings.items()[0].points[0].bar, 7.0);
}

#[test]
fn reanchoring_flags_an_anchor_the_new_series_cannot_reach() {
    let mut drawings = Drawings::default();
    drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(6.0, 100.0, Some(1_700_000_006_000)),
    );

    // A symbol switch: the new instrument's series does not cover the
    // instant this mark was drawn at.
    drawings.reanchor(10, 5, |_| None);

    assert!(drawings.items()[0].off_series, "the mark must say so");
    assert_eq!(
        drawings.items()[0].points[0].time_ms,
        Some(1_700_000_006_000),
        "the instant it was placed at is never rewritten"
    );
}

#[test]
fn reanchoring_travels_with_the_undo_history() {
    let mut drawings = Drawings::default();
    drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(6.0, 100.0, Some(1_700_000_006_000)),
    );
    drawings.begin_gesture();
    drawings.translate_selected(4.0, 0.0);
    drawings.commit_gesture();

    drawings.reanchor(10, 5, halved);
    drawings.undo();

    // Undo must not resurrect a coordinate from the series that is gone.
    assert_eq!(drawings.items()[0].points[0].bar, 3.0);
}

#[test]
fn setting_times_rewrites_the_instants_behind_the_anchors() {
    let mut drawings = Drawings::default();
    let rectangle = tool("rectangle");
    drawings.place(
        rectangle,
        ChartPoint::at_time(1.0, 100.0, Some(1_700_000_001_000)),
    );
    drawings.place(
        rectangle,
        ChartPoint::at_time(3.0, 110.0, Some(1_700_000_003_000)),
    );

    drawings.translate_selected(2.0, 0.0);
    drawings.set_times(0, &[Some(1_700_000_003_000), Some(1_700_000_005_000)]);

    let times: Vec<_> = drawings.items()[0]
        .points
        .iter()
        .map(|point| point.time_ms)
        .collect();
    assert_eq!(
        times,
        [Some(1_700_000_003_000), Some(1_700_000_005_000)],
        "a moved mark carries its new instants, or its shared twin stays behind"
    );
}

/// The honesty hole a symbol switch opens, and the one `off_series`
/// cannot close: both instruments traded at the same instants, so every
/// anchor resolves onto a real bar and only the *price* is meaningless.
/// A mark left painting at full strength there reads as a level on a
/// market it was never drawn on.
#[test]
fn a_market_change_marks_every_object_as_belonging_to_the_old_one() {
    let mut drawings = Drawings::default();
    drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(6.0, 118_000.0, Some(1_700_000_006_000)),
    );

    drawings.mark_market_changed();

    assert!(drawings.items()[0].foreign_market);
    assert!(
        !drawings.items()[0].off_series,
        "the instants are still on this series - which is exactly why the \
             time-based flag cannot catch this case"
    );

    // Anything drawn after the switch is on the market now showing.
    drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(7.0, 138_000.0, Some(1_700_000_007_000)),
    );
    assert!(!drawings.items()[1].foreign_market);
}

#[test]
fn undo_cannot_restore_a_mark_that_claims_the_new_market() {
    let mut drawings = Drawings::default();
    drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(6.0, 118_000.0, Some(1_700_000_006_000)),
    );
    drawings.begin_gesture();
    drawings.translate_selected(1.0, 0.0);
    drawings.commit_gesture();

    drawings.mark_market_changed();
    drawings.undo();

    assert!(
        drawings.items()[0].foreign_market,
        "stepping back through history must not undo the market change"
    );
}

/// Chart anchors consistent with `points` under `scale` — the identity
/// projection the tool tests run with.
fn anchors_for(points: &[egui::Pos2], scale: &PriceScale) -> Vec<ChartPoint> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| ChartPoint::at(index as f32, scale.price_at(point.y)))
        .collect()
}

#[test]
fn horizontal_line_is_selectable_from_anywhere_on_its_stroke() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
    let line = tool("horizontal-line");
    let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
    let payload = line.default_payload();
    let points = [egui::pos2(100.0, 120.0)];
    let anchors = anchors_for(&points, &scale);
    let ctxt = DrawContext {
        payload: payload.as_ref(),
        anchors: &anchors,
        scale: &scale,
        px_per_bar: 20.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: false,
        halo: false,
        content_editing: false,
    };
    assert!(line.hit_test(chart, &points, egui::pos2(450.0, 123.0), 5.0, &ctxt));
}

/// The two tools that *are* a price say so, and nothing else does.
///
/// The registry is walked rather than the two named, so a tool added
/// later either declares a level deliberately or is caught here — the
/// gutter fills up one careless `axis_levels` at a time otherwise, and a
/// price axis tagged with every anchor on the chart is unreadable.
/// A canvas every tool's own `test_geometry` anchors fit inside, so a
/// tool answering "not on this chart" is answering about the rect and not
/// about a fixture that missed it.
const AXIS_TEST_RECT: egui::Rect = egui::Rect {
    min: egui::pos2(0.0, 0.0),
    max: egui::pos2(600.0, 400.0),
};

#[test]
fn only_the_tools_that_name_a_price_ask_the_axis_to_say_it() {
    const DECLARING: [&str; 2] = ["horizontal-line", "horizontal-ray"];
    for tool in DRAWING_TOOLS {
        let (points, _) = tool.test_geometry();
        let levels = tool.axis_levels(AXIS_TEST_RECT, &points);
        if DECLARING.contains(&tool.id()) {
            assert_eq!(
                levels.as_slice(),
                &[points[0].y],
                "{} is one price and the axis says which",
                tool.id()
            );
        } else {
            assert!(
                levels.is_empty(),
                "{} crosses prices without naming one, so it tags nothing",
                tool.id()
            );
        }
    }
}

/// A level moves with the object: the tag is read off the *projected*
/// points, so the same anchor at another zoom or scroll tags where the
/// line actually is rather than where it was placed.
#[test]
fn a_declared_level_follows_the_object_that_declared_it() {
    let tool = DrawingTool::by_id("horizontal-line").expect("a registered tool");
    assert_eq!(
        tool.axis_levels(AXIS_TEST_RECT, &[egui::pos2(10.0, 40.0)])
            .as_slice(),
        &[40.0]
    );
    assert_eq!(
        tool.axis_levels(AXIS_TEST_RECT, &[egui::pos2(10.0, 175.5)])
            .as_slice(),
        &[175.5],
        "the projection moved and the tag went with it"
    );
    assert!(
        tool.axis_levels(AXIS_TEST_RECT, &[]).is_empty(),
        "a draft with no anchor yet declares nothing"
    );
    assert!(
        tool.axis_levels(AXIS_TEST_RECT, &[egui::pos2(10.0, f32::NAN)])
            .is_empty(),
        "and a level that is not a position is not one the axis can write"
    );
}

/// A tool may not tag a level it is not drawing.
///
/// The horizontal ray runs from its anchor to the right edge, so an
/// anchor panned past that edge leaves no stroke at all — and a chip on
/// the gutter would then be the axis marking a level whose line is gone.
/// Its twin, the horizontal line, spans the whole width from any anchor
/// and so is never in that position.
#[test]
fn a_ray_panned_off_the_canvas_stops_claiming_the_axis() {
    let ray = DrawingTool::by_id("horizontal-ray").expect("a registered tool");
    let line = DrawingTool::by_id("horizontal-line").expect("a registered tool");
    let on = [egui::pos2(100.0, 200.0)];
    let past = [egui::pos2(AXIS_TEST_RECT.right() + 40.0, 200.0)];
    assert_eq!(ray.axis_levels(AXIS_TEST_RECT, &on).as_slice(), &[200.0]);
    assert!(
        ray.axis_levels(AXIS_TEST_RECT, &past).is_empty(),
        "no stroke on the canvas, so nothing on the gutter"
    );
    assert_eq!(
        line.axis_levels(AXIS_TEST_RECT, &past).as_slice(),
        &[200.0],
        "a horizontal line is drawn edge to edge wherever its anchor sits"
    );
}

/// The port docks: a tool the axis has never heard of gets its level
/// tagged by declaring one, with no edit to the axis, the pane or this
/// registry's other members.
#[test]
fn a_new_tool_reaches_the_price_axis_by_declaring_a_level() {
    /// A fake tool that is two prices — the shape the next declaring tool
    /// will most likely have, and one no shipped tool has yet.
    struct BandTool;
    impl DrawingToolImpl for BandTool {
        fn id(&self) -> &'static str {
            "band"
        }
        fn name(&self) -> &'static str {
            "Band"
        }
        fn settings_title(&self) -> &'static str {
            "Band settings"
        }
        fn icon(&self) -> &'static str {
            "B"
        }
        fn hover_text(&self) -> &'static str {
            "A fake tool that names two prices"
        }
        fn required_points(&self) -> usize {
            2
        }
        fn axis_levels(&self, _chart_rect: egui::Rect, points: &[egui::Pos2]) -> AxisLevels {
            points.iter().map(|point| point.y).collect()
        }
        fn paint(
            &self,
            _painter: &egui::Painter,
            _chart_rect: egui::Rect,
            _style: DrawingStyle,
            _points: &[egui::Pos2],
            _ctxt: &DrawContext<'_>,
        ) {
        }
        fn hit_test(
            &self,
            _chart_rect: egui::Rect,
            _points: &[egui::Pos2],
            _position: egui::Pos2,
            _radius_px: f32,
            _ctxt: &DrawContext<'_>,
        ) -> bool {
            false
        }
        #[cfg(test)]
        fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
            (
                vec![egui::pos2(10.0, 30.0), egui::pos2(90.0, 70.0)],
                egui::pos2(50.0, 30.0),
            )
        }
    }
    static BAND: BandTool = BandTool;
    let tool = DrawingTool(&BAND);
    let (points, _) = tool.test_geometry();
    assert_eq!(
        tool.axis_levels(AXIS_TEST_RECT, &points).as_slice(),
        &[30.0, 70.0],
        "both of its prices, in the order it declared them"
    );
}

/// The handle port with a second implementation on it — one implementer
/// never proves a trait is a port. This fake's handle is nowhere near its
/// anchor, so every host path that must go through the tool shows up:
/// what gets painted, what can be grabbed, and what a grab does.
#[test]
fn a_tool_may_own_its_handles_and_the_host_paints_hits_and_drags_them() {
    /// One anchor, one handle floating a fixed distance above it.
    struct PivotTool;
    const PIVOT_HANDLE_OFFSET_PX: f32 = 20.0;
    impl DrawingToolImpl for PivotTool {
        fn id(&self) -> &'static str {
            "pivot"
        }
        fn name(&self) -> &'static str {
            "Pivot"
        }
        fn settings_title(&self) -> &'static str {
            "Pivot settings"
        }
        fn icon(&self) -> &'static str {
            "P"
        }
        fn hover_text(&self) -> &'static str {
            "A fake tool whose handle is not its anchor"
        }
        fn required_points(&self) -> usize {
            1
        }
        fn paint(
            &self,
            _painter: &egui::Painter,
            _chart_rect: egui::Rect,
            _style: DrawingStyle,
            _points: &[egui::Pos2],
            _ctxt: &DrawContext<'_>,
        ) {
        }
        fn hit_test(
            &self,
            _chart_rect: egui::Rect,
            _points: &[egui::Pos2],
            _position: egui::Pos2,
            _radius_px: f32,
            _ctxt: &DrawContext<'_>,
        ) -> bool {
            false
        }
        fn handles(
            &self,
            _chart_rect: egui::Rect,
            points: &[egui::Pos2],
            _ctxt: &DrawContext<'_>,
        ) -> Option<Handles> {
            Some(Handles::from_slice(&[
                points[0] - egui::vec2(0.0, PIVOT_HANDLE_OFFSET_PX)
            ]))
        }
        fn drag_handle(
            &self,
            _chart_rect: egui::Rect,
            _points: &[egui::Pos2],
            _handle: usize,
            to: egui::Pos2,
            _ctxt: &DrawContext<'_>,
            _constrain: Constrain,
        ) -> Option<Handles> {
            Some(Handles::from_slice(&[
                to + egui::vec2(0.0, PIVOT_HANDLE_OFFSET_PX)
            ]))
        }
        #[cfg(test)]
        fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
            (vec![egui::pos2(50.0, 50.0)], egui::pos2(50.0, 30.0))
        }
    }
    static PIVOT: PivotTool = PivotTool;
    let tool = DrawingTool(&PIVOT);

    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 200.0));
    let scale = PriceScale::from_range(0.0, 200.0, 0.0, 200.0);
    let points = vec![egui::pos2(50.0, 50.0)];
    let anchors = anchors_for(&points, &scale);
    let payload = tool.default_payload();
    let ctxt = DrawContext {
        payload: payload.as_ref(),
        anchors: &anchors,
        scale: &scale,
        px_per_bar: 20.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: true,
        halo: false,
        content_editing: false,
    };
    let handle = egui::pos2(50.0, 30.0);
    assert_eq!(tool.handles(chart, &points, &ctxt).as_slice(), &[handle]);
    assert!(
        tool.hit_test(chart, &points, handle, 4.0, &ctxt),
        "the declared handle is what the trader grabs"
    );
    assert!(
        !tool.hit_test(chart, &points, points[0], 4.0, &ctxt),
        "the anchor is not a grab point unless the tool says so"
    );
    assert_eq!(
        tool.drag_handle(
            chart,
            &points,
            0,
            egui::pos2(80.0, 10.0),
            &ctxt,
            Constrain::Free
        )
        .expect("the tool owns the drag")
        .as_slice(),
        &[egui::pos2(80.0, 30.0)],
        "the tool decides which anchors a handle moves"
    );

    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(chart),
        ..Default::default()
    };
    let output = ctx.run(input, |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("pivot-handles"),
        ));
        tool.paint(
            &painter,
            chart,
            DrawingStyle::default(),
            &points,
            &ctxt,
            true,
        );
    });
    let rings: Vec<egui::Pos2> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Circle(circle) if circle.fill == SELECTED_ANCHOR_FILL => {
                Some(circle.center)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        rings,
        vec![handle],
        "the ring the trader sees is the point they can grab, not the anchor"
    );
}

/// The style-default port is additive: a tool that declares nothing is
/// born in the stock look, and the one that declares (the anchored VWAP,
/// a series) is born in its own — colour, weight and fill together.
#[test]
fn style_defaults_are_per_tool_and_additive() {
    let stock = DrawingStyle::default();
    let mut declaring = 0;
    for tool in DRAWING_TOOLS {
        let style = tool.default_style();
        if tool.id() == "anchored-vwap" {
            assert_eq!(style.color, crate::theme::DRAW_CYAN);
            assert!(style.width_px > stock.width_px, "a series outweighs a note");
            assert!(style.fill_alpha > stock.fill_alpha);
            declaring += 1;
        } else {
            assert_eq!(
                (style.width_px, style.fill_alpha),
                (stock.width_px, stock.fill_alpha),
                "{} must keep the stock weight and fill",
                tool.id()
            );
        }
    }
    assert_eq!(declaring, 1, "exactly one tool declares today");
}

/// The context-menu port is additive too: exactly the tools that declare
/// a label appear in the pane's right-click sweep.
#[test]
fn the_context_menu_port_is_declared_not_hardcoded() {
    let labelled: Vec<&str> = DRAWING_TOOLS
        .into_iter()
        .filter_map(|tool| tool.context_menu_label())
        .collect();
    assert_eq!(labelled, vec!["Anchor VWAP here"]);
}

/// The handle port is additive: a tool that does not declare handles is
/// grabbed by its raw anchors, exactly as before the port existed. Only
/// the channel opts out, and it opts out on purpose.
#[test]
fn a_tool_that_declares_no_handles_is_still_grabbed_by_its_anchors() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
    let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
    for drawing_tool in DRAWING_TOOLS {
        let (points, _) = drawing_tool.test_geometry();
        let payload = drawing_tool.default_payload();
        let anchors = anchors_for(&points, &scale);
        let ctxt = DrawContext {
            payload: payload.as_ref(),
            anchors: &anchors,
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: true,
            halo: false,
            content_editing: false,
        };
        let handles = drawing_tool.handles(chart, &points, &ctxt);
        if drawing_tool.id() == "parallel-channel" {
            assert_eq!(
                handles.len(),
                6,
                "the channel adds a corner and a centre per rail"
            );
            continue;
        }
        if drawing_tool.id() == "fixed-range-profile" {
            // The profile opts out too: its anchors' prices are not data
            // (the extent comes from the profile), so the grab points
            // ride the drawn object — anchor x's, visible-extent middle.
            assert_eq!(handles.len(), 2);
            assert_eq!(
                handles.iter().map(|handle| handle.x).collect::<Vec<_>>(),
                points.iter().map(|point| point.x).collect::<Vec<_>>(),
                "profile handles keep their anchors' x"
            );
            continue;
        }
        if drawing_tool.id() == "brush" {
            // The one tool that answers "none", on purpose: a ring on
            // every captured point is a cloud nobody can aim at, over a
            // shape whose individual points mean nothing.
            assert!(
                handles.is_empty(),
                "a scribble is moved whole, never point by point"
            );
            continue;
        }
        assert_eq!(
            handles.as_slice(),
            points.as_slice(),
            "{} must keep being grabbed by its anchors",
            drawing_tool.id()
        );
        assert!(
            drawing_tool
                .drag_handle(
                    chart,
                    &points,
                    0,
                    egui::pos2(1.0, 1.0),
                    &ctxt,
                    Constrain::Free
                )
                .is_none(),
            "{} leaves the drag to the host",
            drawing_tool.id()
        );
    }
}

#[test]
fn every_registered_tool_paints_and_hits_its_finished_geometry() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
    let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
    for drawing_tool in DRAWING_TOOLS {
        let (points, hit) = drawing_tool.test_geometry();
        if drawing_tool.freehand() {
            // A captured path has no declared length; two points is the
            // shortest thing that is still a stroke.
            assert!(points.len() >= 2, "{} needs a path", drawing_tool.id());
        } else {
            assert_eq!(points.len(), drawing_tool.required_points());
        }
        let payload = drawing_tool.default_payload();
        let anchors = anchors_for(&points, &scale);
        let ctxt = DrawContext {
            payload: payload.as_ref(),
            anchors: &anchors,
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: false,
            halo: false,
            content_editing: false,
        };
        assert!(
            drawing_tool.hit_test(chart, &points, hit, 5.0, &ctxt),
            "{} cannot be selected from its visible geometry",
            drawing_tool.id()
        );

        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(chart),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new(drawing_tool.id()),
            ));
            drawing_tool.paint(
                &painter,
                chart,
                DrawingStyle::default(),
                &points,
                &ctxt,
                false,
            );
        });
        assert!(
            !output.shapes.is_empty(),
            "{} rendered no geometry",
            drawing_tool.id()
        );
    }
}

/// The extension port, proven end to end: a fake tool with a property no
/// other tool has (`ray_count`) docks through the registry surface alone.
/// Its payload rides the envelope, survives undo snapshots and renders
/// its own inspector tab — with zero edits to the shared model or the
/// central inspector code.
#[test]
fn a_fake_tool_with_its_own_payload_docks_without_touching_the_model() {
    #[derive(Debug, Clone, PartialEq)]
    struct RayPayload {
        ray_count: u32,
    }
    impl DrawingPayload for RayPayload {
        fn clone_box(&self) -> Box<dyn DrawingPayload> {
            Box::new(self.clone())
        }
        fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
            other
                .as_any()
                .downcast_ref::<Self>()
                .is_some_and(|other| self == other)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }
    struct RayTool;
    impl DrawingToolImpl for RayTool {
        fn id(&self) -> &'static str {
            "test-ray"
        }
        fn name(&self) -> &'static str {
            "Ray fan"
        }
        fn settings_title(&self) -> &'static str {
            "Ray fan settings"
        }
        fn icon(&self) -> &'static str {
            "R"
        }
        fn hover_text(&self) -> &'static str {
            "Ray fan - test tool"
        }
        fn required_points(&self) -> usize {
            1
        }
        fn default_payload(&self) -> Box<dyn DrawingPayload> {
            Box::new(RayPayload { ray_count: 2 })
        }
        fn extra_tab(&self) -> Option<&'static str> {
            Some("Rays")
        }
        fn draw_extra_tab(
            &self,
            ui: &mut egui::Ui,
            drawing: &mut Drawing,
            _host: &mut dyn PresetHost,
        ) -> bool {
            let payload = drawing
                .payload
                .as_any_mut()
                .downcast_mut::<RayPayload>()
                .expect("a ray tool always carries a ray payload");
            ui.add(egui::Slider::new(&mut payload.ray_count, 1..=8).text("rays"))
                .changed()
        }
        fn paint(
            &self,
            painter: &egui::Painter,
            _chart_rect: egui::Rect,
            style: DrawingStyle,
            points: &[egui::Pos2],
            ctxt: &DrawContext<'_>,
        ) {
            let payload = ctxt
                .payload
                .as_any()
                .downcast_ref::<RayPayload>()
                .expect("ray payload");
            if let Some(origin) = points.first() {
                for ray in 0..payload.ray_count {
                    let target = *origin + egui::vec2(40.0, 10.0 * (ray as f32 + 1.0));
                    painter.line_segment([*origin, target], drawing_stroke(style));
                }
            }
        }
        fn hit_test(
            &self,
            _chart_rect: egui::Rect,
            points: &[egui::Pos2],
            position: egui::Pos2,
            radius_px: f32,
            _ctxt: &DrawContext<'_>,
        ) -> bool {
            points
                .first()
                .is_some_and(|point| point.distance(position) <= radius_px * 10.0)
        }
        #[cfg(test)]
        fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
            (vec![egui::pos2(50.0, 50.0)], egui::pos2(60.0, 60.0))
        }
    }
    static RAY: RayTool = RayTool;
    let ray_tool = DrawingTool(&RAY);

    let mut drawings = Drawings::default();
    assert!(drawings.place(ray_tool, ChartPoint::at(1.0, 100.0)));
    // The unique property lives in the payload and rides the undo
    // history exactly like the shared fields do.
    drawings.begin_gesture();
    drawings
        .selected_mut()
        .expect("placement selects")
        .payload
        .as_any_mut()
        .downcast_mut::<RayPayload>()
        .expect("ray payload")
        .ray_count = 5;
    drawings.commit_gesture();
    assert!(drawings.undo(), "the payload edit is one undo entry");
    let restored = drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<RayPayload>()
        .expect("ray payload")
        .ray_count;
    assert_eq!(restored, 2, "undo restores the tool-owned property");

    // The tool-owned inspector tab renders through the same port every
    // tool uses — no central match, no central form edit.
    assert_eq!(ray_tool.extra_tab(), Some("Rays"));
    let ctx = egui::Context::default();
    let mut host = NullPresetHost;
    let mut painted = Vec::new();
    let input = egui::RawInput::default();
    let output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let drawing = drawings.selected_mut().expect("still selected");
            ray_tool.draw_extra_tab(ui, drawing, &mut host);
        });
    });
    for clipped in &output.shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            painted.push(text.galley.text().to_owned());
        }
    }
    assert!(
        painted.iter().any(|text| text.contains("rays")),
        "the fake tool's own section rendered: {painted:?}"
    );
}

/// The under-candles pass is a *port*, so it is proved with a second
/// implementation rather than with its one real user.
///
/// Two fakes: one that overrides `paint_under` and one that does not.
/// The second is the whole reason the method has a default body — every
/// registered tool but the volume profile relies on it, and a default
/// that quietly painted something would put every line, note and
/// Fibonacci grid under the price.
#[test]
fn the_background_pass_reaches_a_tool_that_wants_it_and_no_other() {
    use std::cell::Cell;

    thread_local! {
        static UNDER: Cell<usize> = const { Cell::new(0) };
        static OVER: Cell<usize> = const { Cell::new(0) };
    }

    /// Context: draws in both passes, the way the profile does.
    #[derive(Debug)]
    struct Contextual;
    /// Annotation: never overrides `paint_under`, like every other tool.
    #[derive(Debug)]
    struct Annotation;

    macro_rules! stub {
        ($ty:ty, $id:literal) => {
            impl DrawingToolImpl for $ty {
                fn id(&self) -> &'static str {
                    $id
                }
                fn name(&self) -> &'static str {
                    $id
                }
                fn settings_title(&self) -> &'static str {
                    $id
                }
                fn icon(&self) -> &'static str {
                    "?"
                }
                fn hover_text(&self) -> &'static str {
                    $id
                }
                fn required_points(&self) -> usize {
                    2
                }
                fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
                    (Vec::new(), egui::Pos2::ZERO)
                }
                fn paint(
                    &self,
                    _painter: &egui::Painter,
                    _chart_rect: egui::Rect,
                    _style: DrawingStyle,
                    _points: &[egui::Pos2],
                    _ctxt: &DrawContext<'_>,
                ) {
                    OVER.with(|c| c.set(c.get() + 1));
                }
                fn hit_test(
                    &self,
                    _chart_rect: egui::Rect,
                    _points: &[egui::Pos2],
                    _position: egui::Pos2,
                    _radius_px: f32,
                    _ctxt: &DrawContext<'_>,
                ) -> bool {
                    false
                }
            }
        };
    }
    stub!(Contextual, "stub-contextual");
    stub!(Annotation, "stub-annotation");

    // The one override, which is what the profile does for real.
    impl Contextual {
        fn mark_under() {
            UNDER.with(|c| c.set(c.get() + 1));
        }
    }

    let ctx = egui::Context::default();
    let painter = ctx.debug_painter();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
    let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
    let payload: Box<dyn DrawingPayload> = Box::new(NoPayload);
    let ctxt = DrawContext {
        payload: payload.as_ref(),
        anchors: &[],
        scale: &scale,
        px_per_bar: 20.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: false,
        halo: false,
        content_editing: false,
    };

    // The tool that does not override it: the default body runs, paints
    // nothing, and is not silently the over-candles pass either.
    let before_over = OVER.with(Cell::get);
    DrawingToolImpl::paint_under(
        &Annotation,
        &painter,
        rect,
        DrawingStyle::default(),
        &[],
        &ctxt,
    );
    assert_eq!(
        UNDER.with(Cell::get),
        0,
        "the default background pass draws nothing"
    );
    assert_eq!(
        OVER.with(Cell::get),
        before_over,
        "and it is not quietly the over-candles pass"
    );

    // While the pass it does implement still runs.
    DrawingToolImpl::paint(
        &Annotation,
        &painter,
        rect,
        DrawingStyle::default(),
        &[],
        &ctxt,
    );
    assert_eq!(OVER.with(Cell::get), before_over + 1);

    // And a tool that wants the pass is reached by it.
    Contextual::mark_under();
    assert_eq!(UNDER.with(Cell::get), 1);
}
