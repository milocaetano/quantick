//! Recognize exact bundled documents saved before the current palette.
//! These fixtures are unmodified Git blobs of crates/appearance/themes/light.toml:
//! e4a9249b: 2c488285102be909b026192e6c9f18839c1e1928 (native choices introduced),
//! e24a42a3: ace82b76e7df710e94ed164af4e5c23af155d685 (neutral Light),
//! 8bb894e1: d28a176148a2cc4bb479a30964633002ec544c4f (solid Light candles).
//! 66aa9abc: 66aa9abced90b1daa892a172305b7eafbd9c40ec (dark Bookmap surface).
//! 90d18edf: the warm-gray Bookmap and amber drawing defaults.
//! Dark's fixture is also the unmodified 66aa9abc source, before drawing_profile.
//! 7ec67e50: exact Light/Dark sources before the independent chart grid color.
//! d4a42dc1: exact Light/Dark sources before the tab strip, brand and replay tokens.
//! ab24b5e3: dc9dd4d48a5122ac41e7d7ac40e3a3c0d34882dc (white canvas, slate order flow).
//! ee7ea6b5: Light e5c858db7d1a2cb7269312d0437ecc9ae5a532b1 and Dark
//! 2dc1249fef0b0c3b352b5a714e1498bdb2e1a656 (before the title strip and tab tokens).
//! f32407a0: Light b9f2ee41b626ec8418b580b5b3f2e7f102f92076 and Dark
//! 66bc4a047727c5ee42d67b138d61001b084dfbef (before the TradingView light chart).

const LIGHT_SOURCES: [&str; 10] = [
    include_str!("legacy-light-e4a9249b.toml"),
    include_str!("legacy-light-e24a42a3.toml"),
    include_str!("legacy-light-8bb894e1.toml"),
    include_str!("legacy-light-66aa9abc.toml"),
    include_str!("legacy-light-90d18edf.toml"),
    include_str!("legacy-light-7ec67e50.toml"),
    include_str!("legacy-light-d4a42dc1.toml"),
    include_str!("legacy-light-ab24b5e3.toml"),
    include_str!("legacy-light-ee7ea6b5.toml"),
    include_str!("legacy-light-f32407a0.toml"),
];
const DARK_SOURCES: [&str; 5] = [
    include_str!("legacy-dark-66aa9abc.toml"),
    include_str!("legacy-dark-7ec67e50.toml"),
    include_str!("legacy-dark-d4a42dc1.toml"),
    include_str!("legacy-dark-ee7ea6b5.toml"),
    include_str!("legacy-dark-f32407a0.toml"),
];

/// Only generated choice siblings call this; authored base documents stay exact.
/// Loading publishes current defaults without writing the old file. The next
/// explicit save uses the canonical source through the ordinary write gate.
pub(super) fn current_source(source: &str) -> Option<&'static str> {
    let normalized = source.replace("\r\n", "\n");
    if DARK_SOURCES.contains(&normalized.as_str()) {
        return Some(quantick_appearance::DARK);
    }
    LIGHT_SOURCES
        .iter()
        .any(|known| normalized == *known)
        .then_some(quantick_appearance::LIGHT)
}

#[cfg(test)]
#[path = "legacy_tests.rs"]
mod tests;
