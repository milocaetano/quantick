use super::*;

/// A script whose last character is multi-byte and which does not end in a
/// newline puts the lexer's end-of-line span *inside* that character:
/// `end_logical_line` emits it at `pos - 1`, and `pos` is the length in
/// bytes. Reporting that position must answer with a diagnostic, not take
/// the application thread down with it.
#[test]
fn a_span_inside_a_character_reports_a_position() {
    let source = "x = // ré";
    let offset = source.len() - 1;
    assert!(!source.is_char_boundary(offset), "the case under test");
    let error = quantick_pine::PineError::new(
        quantick_pine::ErrorCode::PineSyntax,
        quantick_pine::Span::at(offset),
        "interior character span",
    );
    let reported = compile_error(&[error], source);
    let details = reported.context.details.unwrap();
    let diagnostic = &details["diagnostics"][0];
    assert_eq!(
        (
            diagnostic["line"].as_u64().unwrap(),
            diagnostic["column"].as_u64().unwrap()
        ),
        (1, 9),
    );
}
