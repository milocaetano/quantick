//! Compile the actual text model with only its lower-level geometry available.
//! A scene/tool dependency fails to resolve here, including inline crate paths
//! and root reexports that the repository's use-statement cycle scan cannot see.
pub use quantick_drawings::Px;

mod geometry {
    pub use quantick_drawings::{pos2, vec2};
}
mod rect {
    pub use quantick_drawings::PxRect;
}

#[path = "../src/text.rs"]
pub mod text;

#[test]
fn text_model_measures_and_anchors_without_a_scene_or_tool() {
    let format = text::TextFormat {
        font: text::FontSpec::proportional(12.0),
        appearance: text::TextAppearance::default(),
    };
    let size = text::TextExtent::default().measure("Note", format, &text::UNMEASURED);
    assert!(size.x > 0.0 && size.y > 0.0);
    let anchor = Px::new(100.0, 200.0);
    let bounds = text::text_bounds(anchor, size, format.appearance);
    assert_eq!(text::anchor_from_bounds(bounds, format.appearance), anchor);
}
