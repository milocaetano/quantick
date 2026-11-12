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

/// A tab's licence to roll itself off a contract the venue no longer lists,
/// and the contracts it already decided.
///
/// State only: the rule that reads it is the contract roll's. Armed when the
/// tab is opened from a saved workspace and disarmed once its feed connects.
/// Each missing contract is decided once, rolled or refused, so a chain of
/// unlisted contracts is walked one step per report and a configured cycle
/// cannot loop.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AutoRoll {
    armed: bool,
    decided: std::collections::BTreeSet<String>,
    pressed: Option<String>,
}

impl AutoRoll {
    /// The tab was opened from a saved workspace.
    pub fn arm(&mut self) {
        self.armed = true;
    }

    /// The tab's feed connected: from here on a roll waits for a hand.
    pub fn disarm(&mut self) {
        self.armed = false;
    }

    /// Whether the tab still rolls by itself.
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// The trader pressed the notice's Roll to `target`; the roll runs on the
    /// window's next settle, where the drawings it carries are reachable.
    pub fn press(&mut self, target: &str) {
        self.pressed = Some(target.to_owned());
    }

    /// The pressed Roll, once.
    pub fn take_press(&mut self) -> Option<String> {
        self.pressed.take()
    }

    /// Record the decision about `symbol`; `false` when it was made already.
    pub fn decide(&mut self, symbol: &str) -> bool {
        self.decided.insert(symbol.to_owned())
    }
}
