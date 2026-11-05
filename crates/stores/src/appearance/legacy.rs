//! Recognize the exact Light documents saved before Bookmap became independent.
//! These fixtures are unmodified Git blobs of crates/appearance/themes/light.toml:
//! e4a9249b: 2c488285102be909b026192e6c9f18839c1e1928 (native choices introduced),
//! e24a42a3: ace82b76e7df710e94ed164af4e5c23af155d685 (neutral Light),
//! 8bb894e1: d28a176148a2cc4bb479a30964633002ec544c4f (solid Light candles).

const LIGHT_SOURCES: [&str; 3] = [
    include_str!("legacy-light-e4a9249b.toml"),
    include_str!("legacy-light-e24a42a3.toml"),
    include_str!("legacy-light-8bb894e1.toml"),
];

/// Only generated choice siblings call this; authored base documents stay exact.
/// Loading publishes current defaults without writing the old file. The next
/// explicit save uses the canonical source through the ordinary write gate.
pub(super) fn current_source(source: &str) -> Option<&'static str> {
    let normalized = source.replace("\r\n", "\n");
    LIGHT_SOURCES
        .iter()
        .any(|known| normalized == *known)
        .then_some(quantick_appearance::LIGHT)
}

#[cfg(test)]
#[path = "legacy_tests.rs"]
mod tests;
