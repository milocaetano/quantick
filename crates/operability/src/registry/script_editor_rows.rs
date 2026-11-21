use super::{Mapping, Source, UiBehaviour};

pub(super) const BINDING: (usize, &[UiBehaviour]) = (7, ROWS);
const ROWS: &[UiBehaviour] = &[UiBehaviour {
    id: "indicator.source.edit",
    title: "Read, copy and edit Pine source",
    reach: "Indicators > Scripts > View source; Pine source window",
    keys: &[(
        Source::Authored,
        "View source / Create editable copy / Save / source text editor",
    )],
    mapping: Mapping::Capabilities(&["indicator.library.edit"]),
}];
