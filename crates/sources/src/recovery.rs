//! The two acts that get a trader out of a stalled feed.
//!
//! Vocabulary, not behaviour: the feed host decides which one a stall calls
//! for, and the control plane names the capability each one is. It sits here,
//! below both, so the one mapping from an act to its capability can be an
//! exhaustive `match` in a headless crate.

/// Which control gets the trader out of this particular stall.
///
/// The pair exists because the two acts have genuinely different costs, and the
/// application picks between them rather than asking the trader to diagnose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Respawn the transport and keep the timeline: the bars, drawings,
    /// indicators, armed strategies and any open paper position all survive.
    /// Nothing on screen moves. For a connection that never landed or dropped.
    Reconnect,
    /// Throw the timeline away and rebuild it from zero. Refetches history,
    /// flattens the paper position and disarms every strategy — so it is what
    /// the trader is offered only when the cheap act cannot help. For a
    /// transport that claims to be connected while nothing comes down it.
    Reload,
}

impl Recovery {
    /// Stable machine-readable name, shared by observers and recovery controls.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Reconnect => "reconnect",
            Self::Reload => "reload",
        }
    }

    /// The word on the button.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Reconnect => "Reconnect",
            Self::Reload => "Reload",
        }
    }

    /// The other one.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::Reconnect => Self::Reload,
            Self::Reload => Self::Reconnect,
        }
    }
}
