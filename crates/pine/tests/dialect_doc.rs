//! Doc-drift guard (§3.7): every builtin the registry accepts must appear
//! in `docs/pine-dialect.md`, and every documented error code must exist.
//! Cheap, and it turns "update the docs" from a habit into a build rule.

use quantick_pine::builtins::Builtin;

const DIALECT_DOC: &str = include_str!("../../../docs/pine-dialect.md");

#[test]
fn every_registered_builtin_is_documented() {
    let missing: Vec<&str> = Builtin::all_names()
        .iter()
        .filter(|name| !DIALECT_DOC.contains(*name))
        .copied()
        .collect();
    assert!(
        missing.is_empty(),
        "builtins accepted by the registry but absent from docs/pine-dialect.md: {missing:?}"
    );
}

#[test]
fn every_error_code_is_documented() {
    use quantick_pine::ErrorCode::*;
    let codes = [
        PineLex,
        PineSyntax,
        PineIndent,
        PineNoSecurity,
        PineNoTimeframe,
        PineNoStrategy,
        PineNoCollections,
        PineNoCalendar,
        PineUnsupported,
        PineUnknownName,
        PineArity,
        PineInputNotConst,
        PineSeriesLength,
        PineStatefulInLoop,
        PineRecursion,
        PineVersion,
        PineType,
        PineLoopBudget,
    ];
    let missing: Vec<&str> = codes
        .iter()
        .map(|c| c.as_str())
        .filter(|code| !DIALECT_DOC.contains(code))
        .collect();
    assert!(
        missing.is_empty(),
        "error codes absent from docs/pine-dialect.md: {missing:?}"
    );
}
