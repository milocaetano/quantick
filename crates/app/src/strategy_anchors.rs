//! Armed strategy instances anchored to drawings: the kernel's anchor
//! collection, keyed by the window's own drawing id.

use crate::drawings::DrawingId;

pub use quantick_strategy::anchors::{AlarmMark, badge_text};

/// One armed strategy riding one drawing.
pub type AnchoredInstance = quantick_strategy::anchors::AnchoredInstance<DrawingId>;

/// The pane's armed instances.
pub type StrategyAnchors = quantick_strategy::anchors::StrategyAnchors<DrawingId>;
