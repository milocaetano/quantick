//! Armed strategy instances anchored to drawings: the kernel's anchor
//! collection, keyed by the window's own drawing id.

use crate::drawings::DrawingId;

#[cfg(test)]
pub use quantick_strategy::anchors::AlarmMark;
pub use quantick_strategy::anchors::badge_text;

/// One armed strategy riding one drawing.
pub type AnchoredInstance = quantick_strategy::anchors::AnchoredInstance<DrawingId>;

/// The pane's armed instances.
pub type StrategyAnchors = quantick_strategy::anchors::StrategyAnchors<DrawingId>;

/// One pane's armed instances and the work they park for the tab.
pub type PaneStrategies = quantick_session::strategies::PaneStrategies<DrawingId>;

/// Every pane's strategies, keyed by pane id; the tab owns one.
pub type TabStrategies = quantick_session::strategies::TabStrategies<DrawingId>;

/// The tab's strategies and alarm queue, walked through each live print.
pub type TabMarket = quantick_session::market::TabMarket<DrawingId>;
