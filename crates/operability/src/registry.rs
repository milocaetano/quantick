//! The table: one row per supported UI behaviour.
//!
//! Read [`super`] first for why the rows are written and the drift guard is
//! mechanical. This file joins the data; each family file holds its own.
//!
//! Adding a behaviour is a row in the family it belongs to — `drawings`,
//! `layers`, `workspace`, `replay`, `indicators`, `trading`, `chart`,
//! `control` — and [`UI_BEHAVIOURS`] is [`FAMILIES`] joined at compile time,
//! in the order the matrix renders. Adding a family is one line in that list.
//!
//! The guard tells you when a row is missing — it names the registry entry
//! nothing claims — and it will not tell you what the row should say, because
//! that is the decision the table exists to record: either a capability
//! performs the behaviour, or somebody has written down why none does.

use super::{ExclusionClass, Mapping, Source, UiBehaviour};

/// Behaviours nothing refuses and nothing performs yet, by group.
///
/// Every one of these cites the same issue on purpose: the matrix is the
/// enumeration, and thirty stub issues would put that list in two places. The
/// text differs so a reader sees what each group is actually missing.
pub const PENDING_ISSUE: &str = "https://github.com/milocaetano/quantick/issues/401";

/// One capability, as a slice, because [`Mapping::Capabilities`] takes the
/// plural case as the general one.
macro_rules! capability {
    ($($id:literal),+ $(,)?) => {
        Mapping::Capabilities(&[$($id),+])
    };
}

/// An exclusion, spelled once per class so a row reads as prose.
macro_rules! excluded {
    ($class:ident, $reason:literal) => {
        Mapping::Excluded {
            class: ExclusionClass::$class,
            reason: $reason,
        }
    };
}

/// A window or panel that opens, with no capability that opens it.
const PENDING_SURFACE: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability opens or closes this surface; an operator can read what is on screen \
             and not change it. Tracked in issue 401",
};

mod chart;
mod control;
mod drawings;
mod indicators;
mod layers;
mod replay;
mod trading;
mod workspace;

/// Every family's rows, in the order the matrix renders them.
///
/// The order here is the order a reviewer reads, which is why a family that
/// owns rows in two places in that reading declares two slices rather than
/// one: where a row is declared cannot move a line of the document.
pub const FAMILIES: &[&[UiBehaviour]] = &[
    control::WINDOW_AND_TABS,
    replay::ROWS,
    control::LAYOUT_TABS,
    chart::CANVAS,
    layers::ROWS,
    chart::HISTORY,
    indicators::ROWS,
    trading::ROWS,
    control::FEED_RECOVERY,
    drawings::TOOL_RAIL,
    drawings::OBJECTS,
    workspace::ROWS,
    control::CHROME,
];

/// How many rows the declared families hold between them.
const fn total(families: &[&[UiBehaviour]]) -> usize {
    let mut counted = 0;
    let mut family = 0;
    while family < families.len() {
        counted += families[family].len();
        family += 1;
    }
    counted
}

/// The declared families as one table, at compile time.
///
/// The seed row is the first family's first, so a family declared empty and
/// listed first fails compilation rather than silently shortening the table.
const fn flatten<const N: usize>(families: &[&[UiBehaviour]]) -> [UiBehaviour; N] {
    assert!(
        N == total(families),
        "the joined table is as long as the declared families, and no longer"
    );
    let mut joined = [families[0][0]; N];
    let mut next = 0;
    let mut family = 0;
    while family < families.len() {
        let rows = families[family];
        let mut row = 0;
        while row < rows.len() {
            joined[next] = rows[row];
            next += 1;
            row += 1;
        }
        family += 1;
    }
    joined
}

const TOTAL: usize = total(FAMILIES);
const JOINED: [UiBehaviour; TOTAL] = flatten(FAMILIES);

/// Every supported UI behaviour, in identifier order within its group.
///
/// The order here is the order the matrix renders and therefore the order a
/// reviewer reads: [`FAMILIES`] declares it once. Grouped by what the trader
/// is doing rather than by which registry registered it, because "what can I
/// not do without a mouse" is the question this answers.
pub const UI_BEHAVIOURS: &[UiBehaviour] = &JOINED;

/// Registry entries that are deliberately **not** behaviours.
///
/// The `super::drift` guard demands a row for every registered entry, and
/// these are the entries where a row would be a lie: a submenu that holds
/// other entries and performs nothing itself, a hook that opens a menu so a
/// capture can photograph it, a "nothing was clicked" enum variant.
///
/// Each carries its reason, for the same reason `hooks::NOT_HOOKS` does: an
/// allowlist is how a parity guard is quietly defeated, and a reader who
/// disagrees with an entry needs something to disagree with.
pub const NOT_A_BEHAVIOUR: &[(Source, &str, &str)] = &[
    (
        Source::MenuEntry,
        "File",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "View",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "Workspace",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "Tools",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "Help",
        "a top-level menu; it holds entries and performs nothing",
    ),
    (
        Source::MenuEntry,
        "Layouts",
        "a submenu holding the layout tabs and the three edits the strip's own menu holds; \
         each of those is a row of its own",
    ),
    (
        Source::RailTool,
        "Drawing",
        "the rail's slot for the drawing registry rather than a tool of its own; every drawing tool behind it is a `tool.*` row",
    ),
    (
        Source::NoticeAction,
        "None",
        "the feed notice's \"nothing was clicked\" variant: an absence, not a behaviour",
    ),
    (
        Source::ScriptedMenu,
        "workspace",
        "a capture hook that opens the Workspace menu so a screenshot can see it. It performs \
         nothing the menu's own entries do not",
    ),
    (
        Source::ScriptedMenu,
        "history",
        "a capture hook that opens the toolbar's history caret. The behaviour behind it is \
         `history.reach.set`",
    ),
];
