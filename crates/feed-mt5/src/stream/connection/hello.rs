//! Pure admission rules, evaluated before any connected owner exists.
use super::diagnostic::Diagnostic;
use crate::protocol::{Hello, SCHEMA_VERSION};

pub(super) fn refusal(hello: &Hello, expected: &str) -> Option<(String, Diagnostic)> {
    if hello.schema != SCHEMA_VERSION {
        return Some((
            format!("schema mismatch (bridge {})", hello.schema),
            Diagnostic::Schema {
                schema: hello.schema,
                bridge: hello.bridge.clone(),
                version: hello.bridge_version.clone(),
            },
        ));
    }
    if hello.symbol != expected {
        return Some((
            format!("symbol mismatch ({})", hello.symbol),
            Diagnostic::Symbol(hello.symbol.clone()),
        ));
    }
    None
}
