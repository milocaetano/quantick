//! The list's fixed words: the field's placeholder, the group headings, a
//! recording's venue and chip, the command prefix, the limits, and the
//! toasts a switch and a roll leave.

pub const PLACEHOLDER: &str = "Search markets, or > for commands";
pub const RECENT_GROUP: &str = "Recent";
pub const START_HERE_GROUP: &str = "Start here";
pub const MARKETS_GROUP: &str = "Markets";
pub const COMMANDS_GROUP: &str = "Commands";
/// The switch-mode group holding the contract roll's row.
pub const ROLL_GROUP: &str = "Contract roll";
/// What a recording's row says where a venue's name would be.
pub const REPLAY_VENUE: &str = "Replay";
pub const REPLAY_CHIP: char = 'R';
/// A query starting with this lists commands, never markets.
pub const COMMAND_PREFIX: char = '>';
/// How many closed markets Recent remembers.
pub const RECENT_CLOSED_LIMIT: usize = 8;
/// The kinds the Bars selector offers, in its order.
pub const BAR_KINDS: [&str; 5] = ["tick", "volume", "dollar", "imbalance", "time"];

/// What a roll row says on hover when nothing refuses it.
pub const ROLL_KEEPS: &str = "Keeps this tab's workspace and drawings";

/// The no-match heading.
#[must_use]
pub fn no_match_heading(query: &str) -> String {
    format!("No market matches \u{201c}{query}\u{201d}")
}

/// The toast after a switch.
#[must_use]
pub fn switch_toast(symbol: &str) -> String {
    format!("Switched this tab to {symbol} \u{b7} drawings stay with each market")
}

/// The toast after a roll.
#[must_use]
pub fn roll_toast(target: &str) -> String {
    format!("Rolled to {target} \u{b7} workspace and drawings kept")
}
