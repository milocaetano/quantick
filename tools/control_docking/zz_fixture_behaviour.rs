use super::{Mapping, Source, UiBehaviour};
pub(super) const BINDING: (usize, &[UiBehaviour]) = (
    15,
    &[UiBehaviour {
        id: "fixture.magnet",
        title: "Set the fixture magnet",
        reach: "Disposable fixture",
        keys: &[(Source::Authored, "synthetic fixture")],
        mapping: Mapping::Capabilities(&["fixture.magnet.set"]),
    }],
);
