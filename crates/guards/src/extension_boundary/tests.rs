use super::*;

#[test]
fn lexical_data_cannot_create_items_or_close_delimiters() {
    let source = "impl QuantickApp {\n fn text(&self) { let x = r###\" } impl ChartState { \"###; /* { /* } */ } */ let _ = ('}', x); }\n}\n";
    let scanned = scan::scan(source).unwrap();
    assert_eq!(scanned.lines["QuantickApp"].len(), 3);
    assert!(!scanned.lines.contains_key("ChartState"));
}

#[test]
fn existing_callbacks_are_not_root_aliases() {
    for source in [
        "type Handler = fn(&mut QuantickApp) -> Result<(), Error>;",
        "type Projector = Box<dyn Fn(&QuantickApp, CaptureContext) -> Box<dyn ProjectionPayload> + Send>;",
    ] {
        assert!(scan::scan(source).is_ok(), "{source}");
    }
    for source in [
        "type Alias = crate::app::QuantickApp;",
        "type Alias = &mut ChartState;",
        "use crate::{app::QuantickApp as Other};",
    ] {
        assert!(scan::scan(source).is_err(), "{source}");
    }
}

#[test]
fn parameter_impl_trait_is_not_a_root_implementation_item() {
    let source = "fn register(title: impl Into<String>, project: fn(&QuantickApp) -> T) {}";
    assert!(scan::scan(source).unwrap().lines.is_empty());
    let source = "impl\n crate::app::QuantickApp\n{\n fn register(title: impl Into<String>, project: fn(&QuantickApp) -> T) {}\n}\n";
    assert_eq!(scan::scan(source).unwrap().lines["QuantickApp"].len(), 5);
    assert!(scan::scan("impl<T> crate::app::QuantickApp { }").is_err());
}

#[test]
fn nested_const_blocks_count_explicit_root_items() {
    for (source, expected) in [
        (
            "const _: () = ({ impl QuantickApp { fn deposited(&self) {} } });",
            1,
        ),
        (
            "const _: [(); { impl QuantickApp { fn deposited(&self) {} } 0 }] = [];",
            1,
        ),
        (
            "const _: () = ({\n    impl QuantickApp {\n        fn deposited(&self) {}\n    }\n});",
            3,
        ),
        (
            "const _: [(); {\n    impl QuantickApp {\n        fn deposited(&self) {}\n    }\n    0\n}] = [];",
            3,
        ),
    ] {
        let scanned = scan::scan(source).unwrap();
        assert_eq!(scanned.lines["QuantickApp"].len(), expected, "{source}");
    }
    // An item nested in a const block can itself take an impl-Trait parameter.
    let source = "const _: () = ({ impl QuantickApp { fn register(title: impl Into<String>, project: fn(&QuantickApp) -> T) {} } });";
    assert_eq!(scan::scan(source).unwrap().lines["QuantickApp"].len(), 1);
}

#[test]
fn lexical_lifetimes_characters_and_nested_type_delimiters_are_supported() {
    let source = "pub(super) struct IndicatorSlots<'a> { pub values: &'a mut Vec<(usize, Option<&'a str>)>, }\n";
    let scan = scan::scan(source).unwrap();
    assert_eq!(
        scan.shapes[0].1,
        "pub ( super )|struct < 'a >|pub values : & 'a mut Vec < ( usize , Option < & 'a str > ) >"
    );
    for literal in [
        "\"} {\"",
        "b\"} {\"",
        "r#\"} {\"#",
        "br###\"} {\"###",
        "'}'",
        "b'{'",
        "'\\n'",
    ] {
        let source = format!("impl ChartState {{ fn value(&self) {{ let _ = {literal}; }} }}\n");
        assert_eq!(scan::scan(&source).unwrap().lines["ChartState"].len(), 1);
    }
}

#[test]
fn inline_modules_and_multiline_visibility_count_the_full_declaration() {
    let source = "mod nested {\npub\nstruct ChartState { field: usize, }\nimpl crate::state::ChartState { fn f(&self) {} }\n}\n";
    let scanned = scan::scan(source).unwrap();
    assert_eq!(scanned.lines["ChartState"].len(), 3);
    assert_eq!(scanned.shapes[0].1, "pub|struct |field : usize");
}

#[test]
fn malformed_test_classifier_exclusions_are_not_a_scope_escape() {
    let literal_marker =
        "const TEXT: &str = r#\"\n#[cfg(test)]\nmod fake {\n}\n\"#;\nimpl QuantickApp {}\n";
    assert!(
        scan::scan(literal_marker)
            .unwrap_err()
            .contains("inside lexical data")
    );
    let cut =
        "#[cfg(test)]\nmod tests {\n const DATA: &str = r#\"\n}\n\"#;\n}\nimpl QuantickApp {}\n";
    assert!(scan::scan(cut).unwrap_err().contains("cuts a lexical item"));
    let valid = "#[cfg(test)]\nmod tests {\n fn fixture() {}\n}\nimpl QuantickApp {}\n";
    assert_eq!(scan::scan(valid).unwrap().lines["QuantickApp"].len(), 1);
}
