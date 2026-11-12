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

    /// What the control costs, in the trader's terms, on its own hover.
    ///
    /// The caption below the buttons names what the destructive one ends, and for
    /// a while that was the only sentence either button had. A warning beside one
    /// control and silence beside the other does not read as "that one is safe" —
    /// it reads as "one of these is dangerous and I cannot tell which", which
    /// pushes a trader toward pressing neither at the moment they most need to
    /// press one. This is the status line's old hover text, restored to the
    /// surface that inherited its job.
    #[must_use]
    pub fn cost(self) -> &'static str {
        match self {
            Self::Reconnect => {
                "Keeps the chart as it is: bars, drawings, strategies and any open paper position."
            }
            Self::Reload => {
                "Rebuilds the chart from scratch: closes any open paper position and disarms every strategy."
            }
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
/// and what its roll surfaces read.
///
/// State only: the rule that reads it is the contract roll's. Armed for the one
/// contract a saved workspace restored the tab on, and moved along the chain
/// only by a roll it made itself; any other switch disarms it, and so does a
/// feed that connects. A contract is decided once a roll left it, so a
/// configured cycle cannot loop; a refused roll decides nothing and runs once
/// the refusal clears, saying the refusal once.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AutoRoll {
    armed: Option<String>,
    decided: std::collections::BTreeSet<String>,
    pressed: Option<String>,
    offer: Option<RollOffer>,
    refusal_said: Option<&'static str>,
    rolled: Option<(String, String)>,
}

/// The roll a tab's notice offers off a contract the venue does not list, as
/// the last settle found it: every surface that offers it reads this one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RollOffer {
    /// The contract the venue does not list.
    pub from: String,
    /// The configured next contract, or why the roll cannot run right now.
    pub to: Result<String, &'static str>,
}

impl AutoRoll {
    /// The tab was opened on `symbol` from a saved workspace: a fresh licence,
    /// whatever the runtime tab decided before.
    pub fn arm(&mut self, symbol: &str) {
        self.armed = Some(symbol.to_owned());
        self.decided.clear();
        self.refusal_said = None;
    }

    /// The tab's feed connected: from here on a roll waits for a hand.
    pub fn disarm(&mut self) {
        self.armed = None;
    }

    /// Whether the tab still rolls by itself.
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.armed.is_some()
    }

    /// Whether the tab rolls itself off `symbol`: the contract it is armed
    /// for, not decided yet.
    #[must_use]
    pub fn armed_for(&self, symbol: &str) -> bool {
        self.armed.as_deref() == Some(symbol) && !self.decided.contains(symbol)
    }

    /// The tab switched to `symbol`. Only the roll the licence made itself
    /// keeps it; a hand that picked another contract took the tab over.
    pub fn switched_to(&mut self, symbol: &str) {
        if self.armed.as_deref() != Some(symbol) {
            self.armed = None;
        }
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

    /// The tab is rolling off `from` onto `to`: `from` is decided, the attach
    /// that follows explains the roll, and an `automatic` one carries the
    /// licence to `to` while a pressed one ends it.
    pub fn rolling(&mut self, from: &str, to: &str, automatic: bool) {
        self.decided.insert(from.to_owned());
        self.armed = automatic.then(|| to.to_owned());
        self.refusal_said = None;
        self.offer = None;
        self.rolled = Some((from.to_owned(), to.to_owned()));
    }

    /// The roll the attach that follows it explains, once.
    pub fn take_rolled(&mut self) -> Option<(String, String)> {
        self.rolled.take()
    }

    /// Whether `reason` is news: a refusal is said once, until another one
    /// or a roll.
    pub fn refusal_is_news(&mut self, reason: &'static str) -> bool {
        let news = self.refusal_said != Some(reason);
        self.refusal_said = Some(reason);
        news
    }

    /// What the last settle found on offer.
    pub fn set_offer(&mut self, offer: Option<RollOffer>) {
        self.offer = offer;
    }

    /// The roll on offer, as the last settle found it.
    #[must_use]
    pub fn offer(&self) -> Option<&RollOffer> {
        self.offer.as_ref()
    }
}

/// What the trader reads after a roll off an unlisted contract.
#[must_use]
pub fn rolled_sentence(from: &str, to: &str) -> String {
    format!("{from} is no longer listed; rolled to {to}")
}

/// The label of the control that rolls to `target`.
#[must_use]
pub fn roll_label(target: &str) -> String {
    format!("Roll to {target}")
}

/// What heads a refused roll's reason under the popup's next step.
pub const ROLL_REFUSED_PREFIX: &str = "Cannot roll to the next contract: ";

/// The sentence the popup says about a roll refused for `reason`.
#[must_use]
pub fn roll_refused_sentence(reason: &str) -> String {
    format!("{ROLL_REFUSED_PREFIX}{reason}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_licence_follows_its_own_rolls_and_no_hand() {
        let mut roll = AutoRoll::default();
        roll.arm("WINV26");
        assert!(roll.armed_for("WINV26") && !roll.armed_for("WINZ26"));
        roll.switched_to("WINV26");
        assert!(
            roll.armed_for("WINV26"),
            "the restored contract's own spawn"
        );
        roll.rolling("WINV26", "WINZ26", true);
        roll.switched_to("WINZ26");
        assert!(roll.armed_for("WINZ26"), "its own roll carries it along");
        assert_eq!(
            roll.take_rolled(),
            Some(("WINV26".to_owned(), "WINZ26".to_owned()))
        );
        assert_eq!(roll.take_rolled(), None);
        roll.switched_to("WING27");
        assert!(!roll.is_armed(), "a hand switch takes the tab over");
        roll.switched_to("WINZ26");
        assert!(
            !roll.armed_for("WINZ26"),
            "and coming back by hand re-arms nothing"
        );

        roll.arm("WINV26");
        assert!(
            roll.armed_for("WINV26"),
            "arming again forgets what was decided"
        );
        roll.rolling("WINV26", "WINZ26", false);
        assert!(!roll.is_armed(), "a pressed roll is a hand");
    }

    #[test]
    fn a_refusal_is_said_once_until_it_changes() {
        let mut roll = AutoRoll::default();
        assert!(roll.refusal_is_news("paper"));
        assert!(!roll.refusal_is_news("paper"));
        assert!(roll.refusal_is_news("gesture"));
        roll.rolling("A", "B", true);
        assert!(
            roll.refusal_is_news("gesture"),
            "a roll starts the count over"
        );
    }
}
