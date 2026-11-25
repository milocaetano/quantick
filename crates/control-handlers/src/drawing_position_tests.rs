//! The position calls against a real drawing store and preset bank.
use super::*;
use quantick_drawings::{
    ChartPoint, DrawingBand, DrawingIntent, DrawingStyle, NewDrawing, Placement, update,
};
use serde_json::json;
use std::collections::BTreeMap;

/// A bank that remembers saved defaults in memory, per tool; the store on
/// disk answers the same port.
#[derive(Default)]
struct MemoryBank {
    styles: BTreeMap<String, DrawingStyle>,
    configs: BTreeMap<String, toml::Value>,
}

impl PresetHost for MemoryBank {
    fn custom_preset_names(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
    fn load_custom_preset(&self, _: &str, _: &str) -> Option<toml::Value> {
        None
    }
    fn save_custom_preset(&mut self, _: &str, _: &str, _: toml::Value, _: bool) -> bool {
        false
    }
    fn delete_custom_preset(&mut self, _: &str, _: &str) {}
    fn default_preset(&self, _: &str) -> Option<String> {
        None
    }
    fn set_default_preset(&mut self, _: &str, _: Option<String>) {}
    fn default_style(
        &self,
        tool: &str,
        _: quantick_drawings::position_edit::Ground,
    ) -> Option<DrawingStyle> {
        self.styles.get(tool).copied()
    }
    fn set_default_style(
        &mut self,
        tool: &str,
        _: quantick_drawings::position_edit::Ground,
        style: Option<DrawingStyle>,
    ) {
        match style {
            Some(style) => self.styles.insert(tool.into(), style),
            None => self.styles.remove(tool),
        };
    }
    fn default_config(
        &self,
        tool: &str,
        _: quantick_drawings::position_edit::Ground,
    ) -> Option<toml::Value> {
        self.configs.get(tool).cloned()
    }
    fn set_default_config(
        &mut self,
        tool: &str,
        _: quantick_drawings::position_edit::Ground,
        value: Option<toml::Value>,
    ) {
        match value {
            Some(value) => self.configs.insert(tool.into(), value),
            None => self.configs.remove(tool),
        };
    }
    fn has_default_config(&self, tool: &str, _: quantick_drawings::position_edit::Ground) -> bool {
        self.configs.contains_key(tool)
    }
}

/// An anchor at `bar` on a one-minute chart, so a width in bars has a time.
fn at(bar: f32, price: f64) -> ChartPoint {
    ChartPoint::at_time(bar, price, Some((bar * 60_000.0) as i64))
}

/// A store holding a long entered at 100 with its target measured at 120,
/// at risk 1 : reward 2 — stop 90 — and a horizontal line.
fn marks() -> (Drawings<DrawingTool>, u64) {
    let mut marks = Drawings::default();
    let payload = PositionPayload {
        ratio: 2.0,
        ..PositionPayload::default()
    };
    update(
        &mut marks,
        DrawingIntent::Place(Placement {
            tool: DrawingTool::by_id("long-position").unwrap(),
            points: vec![at(10.0, 100.0), at(30.0, 120.0)],
            band: DrawingBand::Price,
            opening: NewDrawing::explicit(Default::default(), Box::new(payload)),
            author: None,
            name: None,
            text: None,
        }),
    );
    let id = marks.items()[0].id.0;
    (marks, id)
}

fn input(id: u64, edits: Value) -> PositionSetInput {
    let mut value = json!({"tab_id": "1", "pane_id": "2", "drawing_id": id.to_string()});
    value
        .as_object_mut()
        .unwrap()
        .extend(edits.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

#[test]
fn an_inspection_reads_the_look_legs_and_width_and_changes_nothing() {
    let (mut marks, id) = marks();
    let before = marks.items()[0].clone();
    let read = set(&mut marks, &input(id, json!({}))).unwrap();
    assert!(!read.changed);
    assert_eq!(read.side, PositionSideDto::Long);
    assert_eq!(
        (
            read.entry.as_str(),
            read.stop.as_str(),
            read.target.as_str()
        ),
        ("100", "90", "120")
    );
    assert_eq!(read.look.target_color, "#2EA064");
    assert_eq!(read.look.stop_color, "#D63031");
    assert_eq!(read.look.zone_opacity_pct, 10);
    assert_eq!(read.look.ratio.as_str(), "2");
    assert_eq!(read.look.ratio_text, "R/R 1:2");
    assert_eq!(read.width_bars.as_str(), "20");
    assert_eq!(marks.items()[0], before);
}

/// Every field at once, one undo step: colours, opacity, a preset that
/// moves only the target, and a width that moves no price.
#[test]
fn a_set_lands_every_edit_as_one_undo_step_and_reads_it_back() {
    let (mut marks, id) = marks();
    let edits = json!({
        "target_color": "#112233", "stop_color": "#445566", "zone_opacity_pct": 40,
        "ratio": "3", "width_bars": "32"
    });
    let read = set(&mut marks, &input(id, edits)).unwrap();
    assert!(read.changed);
    assert_eq!(
        (
            read.entry.as_str(),
            read.stop.as_str(),
            read.target.as_str()
        ),
        ("100", "90", "130"),
        "entry and stop hold; the target moves"
    );
    assert_eq!(
        (
            read.look.target_color.as_str(),
            read.look.stop_color.as_str()
        ),
        ("#112233", "#445566")
    );
    assert_eq!(read.look.zone_opacity_pct, 40);
    assert_eq!(read.look.ratio_text, "R/R 1:3");
    assert_eq!(read.width_bars.as_str(), "32");
    assert!(marks.undo());
    let back = set(&mut marks, &input(id, json!({}))).unwrap();
    assert_eq!(back.target.as_str(), "120", "one undo restores it all");
    assert_eq!(back.look.target_color, "#2EA064");
}

#[test]
fn a_custom_plan_reads_its_ratio_risk_first() {
    let (mut marks, id) = marks();
    // The stop moves from 90 to 92 with the target held at 120: 1 : 2.5.
    let moved = [at(10.0, 100.0), at(30.0, 120.0)];
    marks.items_mut()[0].payload = Box::new(PositionPayload {
        ratio: 2.5,
        projected_offset: Some(-8.0),
        ..PositionPayload::default()
    });
    marks.items_mut()[0].points = moved.to_vec();
    let read = set(&mut marks, &input(id, json!({}))).unwrap();
    assert!(read.look.custom);
    assert_eq!(read.look.ratio_text, "Custom · R/R 1:2.5");
}

/// A refusal changes nothing, whichever edit in the call it was.
#[test]
fn malformed_or_out_of_range_input_changes_nothing() {
    let (mut marks, id) = marks();
    let before = marks.items()[0].clone();
    for edits in [
        json!({"target_color": "green", "ratio": "3"}),
        json!({"stop_color": "#12345"}),
        json!({"zone_opacity_pct": 101}),
        json!({"target_color": "#112233", "ratio": "25"}),
        json!({"width_bars": "3"}),
    ] {
        assert!(
            set(&mut marks, &input(id, edits.clone())).is_err(),
            "{edits}"
        );
        assert_eq!(marks.items()[0], before, "{edits}");
    }
    assert!(set(&mut marks, &input(id + 99, json!({}))).is_err());
}

#[test]
fn a_locked_position_takes_colours_and_refuses_prices_and_width() {
    let (mut marks, id) = marks();
    marks.items_mut()[0].locked = true;
    assert!(set(&mut marks, &input(id, json!({"ratio": "1"}))).is_err());
    assert!(set(&mut marks, &input(id, json!({"width_bars": "30"}))).is_err());
    let read = set(&mut marks, &input(id, json!({"stop_color": "#000000"}))).unwrap();
    assert_eq!(read.look.stop_color, "#000000");
}

#[test]
fn another_tool_is_refused() {
    let (mut marks, id) = marks();
    marks.items_mut()[0].tool = DrawingTool::by_id("horizontal-line").unwrap();
    assert!(set(&mut marks, &input(id, json!({}))).is_err());
}

/// Save stores the drawing's look and its ratio for its own side; reset
/// gives the factory look back; inspect reads without writing.
#[test]
fn the_default_call_saves_resets_and_reads_per_tool() {
    let mut bank = MemoryBank::default();
    let (marks, _) = marks();
    let canvas = marks.canvas();
    let drawing = marks.items()[0].clone();
    let fresh = default(&mut bank, canvas, &drawing, DefaultAction::Inspect).unwrap();
    assert!(!fresh.saved);
    assert_eq!(
        fresh.opening.ratio_text, "R/R 1:1",
        "factory risk 1 : reward 1"
    );
    let saved = default(&mut bank, canvas, &drawing, DefaultAction::Save).unwrap();
    assert!(saved.saved);
    assert_eq!(saved.tool_id, "long-position");
    assert_eq!(saved.opening.ratio_text, "R/R 1:2");
    let mut short = drawing.clone();
    short.tool = DrawingTool::by_id("short-position").unwrap();
    let other = default(&mut bank, canvas, &short, DefaultAction::Inspect).unwrap();
    assert!(!other.saved, "short keeps its own default");
    let reset = default(&mut bank, canvas, &drawing, DefaultAction::Reset).unwrap();
    assert!(!reset.saved);
    assert_eq!(reset.opening.target_color, "#2EA064");
    assert_eq!(reset.opening.ratio_text, "R/R 1:1");
}

/// A flat plan states no legs to read back, so even a colour edit is
/// refused before it lands, with nothing changed.
#[test]
fn an_edit_on_a_flat_plan_is_refused_before_it_lands() {
    let (mut marks, id) = marks();
    marks.items_mut()[0].points = vec![at(10.0, 100.0), at(30.0, 100.0)];
    let before = marks.items()[0].clone();
    assert!(set(&mut marks, &input(id, json!({"target_color": "#00FF00"}))).is_err());
    assert_eq!(marks.items()[0], before);
}
