//! What the script editor shows after an edit: inputs, or the first errors.
use quantick_pine::compile::VALIDATION_ERROR_LIMIT;
use quantick_pine::{compile, validate};

#[test]
fn a_valid_source_reports_its_declared_inputs_and_no_error() {
    let source = "//@version=5\nindicator(\"t\")\nlength = input.int(9, \"Length\")\nplot(ta.sma(close, length))\n";
    let (inputs, error) = validate(source, "t");
    assert_eq!(error, None);
    assert_eq!(inputs, compile(source, "t").expect("valid").inputs);
    assert_eq!(inputs.len(), 1);
}

#[test]
fn an_invalid_source_renders_at_most_the_limit_one_per_line() {
    let lines: String = (0..VALIDATION_ERROR_LIMIT + 3)
        .map(|n| format!("plot(missing_{n})\n"))
        .collect();
    let source = format!("//@version=5\nindicator(\"t\")\n{lines}");
    let Err(errors) = compile(&source, "broken") else {
        panic!("undefined names do not compile");
    };
    assert!(errors.len() > VALIDATION_ERROR_LIMIT, "{}", errors.len());

    let (inputs, error) = validate(&source, "broken");
    assert!(inputs.is_empty());
    let expected: Vec<String> = errors
        .iter()
        .take(VALIDATION_ERROR_LIMIT)
        .map(|error| error.render("broken", &source))
        .collect();
    assert_eq!(error, Some(expected.join("\n")));
}
