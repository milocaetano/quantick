use super::{Mapping, Source, UiBehaviour};
pub const ROWS: &[UiBehaviour] = &[UiBehaviour {
    id: "strategy.describe",
    title: "Describe a strategy",
    reach: "Panels > Describe a strategy...",
    keys: &[(Source::MenuEntry, "Describe a strategy...")],
    mapping: Mapping::Capabilities(&[
        "strategy.describe.open",
        "strategy.describe.read",
        "strategy.describe.submit",
        "strategy.describe.cancel",
        "strategy.describe.save",
    ]),
}];
