//! Registered stages of one tab's per-frame intake: what its feed sent, into
//! the panes, the book and the notices, then the heartbeats that react to it.
//!
//! Every tab runs this plan every frame, on screen or not. The caller owns
//! the channels and the effects; this names the stages and what each one
//! must follow.

use crate::stage_registry::declare_stages;

#[cfg(test)]
mod tests;

declare_stages! {
    pub enum TabDrainStage {
        /// Trades, history and resets into every pane, through the
        /// source-drain plan.
        ReceiveSource after [],
        /// Indicator worker results onto the panes.
        ApplyIndicatorResults after [],
        /// A bounded slice of depth events into the book.
        ReceiveBook after [],
        /// The newest feed notice.
        ReceiveNotices after [],
        /// Keep the depth recorder running. After the source drain: a
        /// `Reset` there turns capture off for the rebuilt market, and the
        /// heartbeat restarts it on the same frame rather than the next.
        BookCaptureHeartbeat after [ReceiveSource],
        /// The window's history choices mirrored onto the tab.
        MirrorHistoryPolicy after [],
        /// Ask for venue candles. After the mirror: the request is phrased
        /// the way the trader last chose, including on the frame they chose.
        PollCandleHistory after [MirrorHistoryPolicy],
    }
}

/// The canonical traversal a tab's drain executes.
pub struct TabDrainPlan;

impl TabDrainPlan {
    pub fn stages() -> impl ExactSizeIterator<Item = TabDrainStage> + Clone {
        TabDrainStage::canonical()
    }
}
