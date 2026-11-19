use super::*;
use quantick_stores::indicator_state::SavedInput;

fn saved(kind: SavedKind, inputs: Vec<SavedInput>) -> SavedIndicator {
    SavedIndicator {
        kind,
        hidden: false,
        mouse_vertical_line: false,
        inputs,
        plot_styles: Vec::new(),
    }
}

#[test]
fn a_native_names_every_input_and_fills_the_unsaved_with_defaults() {
    let ema = saved(SavedKind::native("native.ema"), vec![SavedInput::Int(21)]);
    let read = indicators(vec![(4, ema, true)]);
    let specs = quantick_indicators::native::native("native.ema")
        .unwrap()
        .default_inputs();
    assert_eq!(read[0].kind, IndicatorKind::Native);
    assert_eq!(read[0].id, "native.ema");
    assert_eq!(read[0].slot_id, WireU64::new(4));
    assert!(read[0].loaded);
    assert_eq!(read[0].inputs.len(), specs.len());
    assert_eq!(read[0].inputs[0].name.as_deref(), Some(specs[0].name()));
    assert!(matches!(read[0].inputs[0].value, Value::Int(21)));
    let default = Value::from_input(&specs[1].default_value());
    let json = |value: &Value| serde_json::to_value(value).unwrap();
    assert_eq!(json(&read[0].inputs[1].value), json(&default));
}

#[test]
fn a_script_lists_its_values_in_order_without_names() {
    let script = SavedKind::Script {
        name: "zigzag.pine".to_owned(),
    };
    let inputs = vec![SavedInput::Int(5), SavedInput::Bool(true)];
    let read = indicators(vec![(9, saved(script, inputs), false)]);
    assert_eq!(read[0].kind, IndicatorKind::Script);
    assert_eq!(read[0].id, "zigzag.pine");
    assert!(!read[0].loaded);
    assert!(read[0].inputs.iter().all(|input| input.name.is_none()));
    assert!(matches!(read[0].inputs[0].value, Value::Int(5)));
    assert!(matches!(read[0].inputs[1].value, Value::Bool(true)));
}

#[test]
fn the_list_stops_at_its_cap() {
    let ema = || saved(SavedKind::native("native.ema"), Vec::new());
    let many = (0..MAX_INDICATORS as u64 + 3).map(|slot| (slot, ema(), true));
    assert_eq!(indicators(many.collect()).len(), MAX_INDICATORS);
}

#[test]
fn the_read_is_read_only_and_asks_for_user_text_and_paper() {
    let descriptor = descriptor();
    assert!(descriptor.read_only);
    let permissions: Vec<&str> = (descriptor.required_permissions.iter())
        .map(|id| id.as_str())
        .collect();
    assert!(permissions.contains(&"observe.user_text"));
    assert!(permissions.contains(&"observe.paper"));
}
