//! Every workspace's colour: one slot of a fixed palette of eight, stored with
//! the workspace and never derived from where it sits in a list.
//!
//! Uniqueness is the store's invariant, kept by [`settle`] after every change
//! to the set of workspaces: no two workspaces hold one colour, and a
//! workspace without one takes the first free colour in palette order, in the
//! book's own order (`Default` first, then by name). So a new workspace takes
//! the next free colour, a deleted one frees its colour without recolouring
//! any other, a ninth holds none until one frees, and a stored book that
//! predates colours gets them deterministically on load. The values each slot
//! paints in are the theme's (`quantick_appearance`), never this crate's.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A palette slot, in the order a new workspace takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceColour {
    /// `Default`'s.
    Steel,
    Violet,
    Cyan,
    Magenta,
    Azure,
    Indigo,
    Chalk,
    Cobalt,
}

impl WorkspaceColour {
    /// The palette, in assignment order.
    pub const ALL: [Self; 8] = [
        Self::Steel,
        Self::Violet,
        Self::Cyan,
        Self::Magenta,
        Self::Azure,
        Self::Indigo,
        Self::Chalk,
        Self::Cobalt,
    ];

    /// The slot's place in [`Self::ALL`], which a theme's token list follows.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The stable word a file, a call and a snapshot use.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Steel => "steel",
            Self::Violet => "violet",
            Self::Cyan => "cyan",
            Self::Magenta => "magenta",
            Self::Azure => "azure",
            Self::Indigo => "indigo",
            Self::Chalk => "chalk",
            Self::Cobalt => "cobalt",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|colour| colour.key() == key)
    }
}

/// Who holds which colour, by workspace name.
pub type Colours = BTreeMap<String, WorkspaceColour>;

/// Restore the invariant over `order`, the workspaces in the book's order: a
/// name no longer there frees its colour, a colour held twice stays with its
/// first holder, and every workspace without one takes the first free colour.
pub fn settle(colours: &mut Colours, order: &[&str]) {
    colours.retain(|name, _| order.contains(&name.as_str()));
    let mut held = Vec::new();
    for name in order {
        match colours.get(*name) {
            Some(colour) if !held.contains(colour) => held.push(*colour),
            _ => {
                colours.remove(*name);
            }
        }
    }
    for name in order {
        if colours.contains_key(*name) {
            continue;
        }
        let Some(free) = WorkspaceColour::ALL.into_iter().find(|c| !held.contains(c)) else {
            return;
        };
        held.push(free);
        colours.insert((*name).to_owned(), free);
    }
}

/// What picking a colour did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColourOutcome {
    pub workspace: String,
    pub colour: WorkspaceColour,
    /// `false` when the workspace already held it.
    pub changed: bool,
    /// The workspace that held the colour.
    pub swapped_with: Option<String>,
    /// What [`Self::swapped_with`] holds now: the picker's old colour, or
    /// `None` when the picker had none, so the holder goes without.
    pub holder_colour: Option<WorkspaceColour>,
}

/// Give `name` the colour `colour`; a workspace holding it swaps with `name`.
/// The caller has checked that `name` is a workspace.
pub fn pick(colours: &mut Colours, name: &str, colour: WorkspaceColour) -> ColourOutcome {
    let previous = colours.get(name).copied();
    let holder = colours
        .iter()
        .find(|(held, c)| **c == colour && held.as_str() != name)
        .map(|(held, _)| held.clone());
    if let Some(holder) = &holder {
        match previous {
            Some(previous) => colours.insert(holder.clone(), previous),
            None => colours.remove(holder),
        };
    }
    colours.insert(name.to_owned(), colour);
    ColourOutcome {
        workspace: name.to_owned(),
        colour,
        changed: previous != Some(colour),
        holder_colour: holder.as_ref().and(previous),
        swapped_with: holder,
    }
}

/// One swatch of the picker for a workspace: the colour, who holds it and the
/// mark the swatch carries for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swatch {
    pub colour: WorkspaceColour,
    /// The workspace picking: its own colour.
    pub current: bool,
    /// Another workspace holds it: picking it swaps the two.
    pub owner: Option<String>,
    /// Picking it leaves [`Self::owner`] without a colour: the workspace
    /// picking holds none to give back.
    pub owner_goes_without: bool,
    /// The owner's initials, as short as tells it from the other owners.
    pub initials: String,
}

/// The eight swatches for `name`, in palette order.
#[must_use]
pub fn swatches(colours: &Colours, name: &str) -> Vec<Swatch> {
    let owners: Vec<&str> = colours.keys().map(String::as_str).collect();
    let colourless = !colours.contains_key(name);
    WorkspaceColour::ALL
        .into_iter()
        .map(|colour| {
            let holder = colours.iter().find(|(_, c)| **c == colour).map(|(n, _)| n);
            let owner = holder.filter(|held| held.as_str() != name).cloned();
            Swatch {
                colour,
                current: holder.is_some_and(|held| held == name),
                initials: owner
                    .as_deref()
                    .map(|owner| initials(owner, &owners))
                    .unwrap_or_default(),
                owner_goes_without: colourless && owner.is_some(),
                owner,
            }
        })
        .collect()
}

/// The most characters a swatch's initials take.
pub const INITIALS_MAX: usize = 4;

/// The shortest start of `name`, up to [`INITIALS_MAX`] characters, no other
/// of `among` shares, case aside: `Scalp` and `Swing` are `Sc` and `Sw`,
/// `Default` is `D`. Names alike that far take one character less and their
/// place among the names alike, in `among`'s order: `Scalp` and `Scalper`
/// are `Sca1` and `Sca2`.
#[must_use]
pub fn initials(name: &str, among: &[&str]) -> String {
    let start = |text: &str, letters: usize| -> String { text.chars().take(letters).collect() };
    let alike = |other: &str, letters: usize| {
        start(other, letters).to_lowercase() == start(name, letters).to_lowercase()
    };
    let unique = |letters: usize| !among.iter().any(|o| *o != name && alike(o, letters));
    if let Some(letters) = (1..=INITIALS_MAX).find(|letters| unique(*letters)) {
        return start(name, letters);
    }
    let mut twins = among.iter().filter(|other| alike(other, INITIALS_MAX));
    let place = twins
        .position(|other| *other == name)
        .map_or(1, |at| at + 1);
    format!("{}{place}", start(name, INITIALS_MAX - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled(order: &[&str]) -> Colours {
        let mut colours = Colours::new();
        settle(&mut colours, order);
        colours
    }

    #[test]
    fn the_palette_order_is_the_index_order_and_keys_round_trip() {
        for (index, colour) in WorkspaceColour::ALL.into_iter().enumerate() {
            assert_eq!(colour.index(), index);
            assert_eq!(WorkspaceColour::from_key(colour.key()), Some(colour));
        }
        assert_eq!(WorkspaceColour::from_key("green"), None);
    }

    #[test]
    fn workspaces_take_free_colours_in_order_and_a_ninth_holds_none() {
        let names = ["Default", "A", "B", "C", "D", "E", "F", "G", "H"];
        let colours = settled(&names);
        for (name, colour) in names.iter().zip(WorkspaceColour::ALL) {
            assert_eq!(colours.get(*name), Some(&colour), "{name}");
        }
        assert_eq!(colours.get("H"), None);
    }

    #[test]
    fn settle_keeps_held_colours_and_drops_duplicates_and_strangers() {
        let mut colours = Colours::from([
            ("B".to_owned(), WorkspaceColour::Steel),
            ("A".to_owned(), WorkspaceColour::Steel),
            ("Gone".to_owned(), WorkspaceColour::Cyan),
        ]);
        settle(&mut colours, &["Default", "A", "B"]);
        // `A` comes first in the book's order, so it keeps Steel; Default and
        // B take the first free colours, Violet and Cyan (freed by `Gone`).
        assert_eq!(colours.get("A"), Some(&WorkspaceColour::Steel));
        assert_eq!(colours.get("Default"), Some(&WorkspaceColour::Violet));
        assert_eq!(colours.get("B"), Some(&WorkspaceColour::Cyan));
        assert!(!colours.contains_key("Gone"));
    }

    #[test]
    fn picking_a_held_colour_swaps_and_a_free_one_frees_the_old() {
        let mut colours = settled(&["Default", "Scalp", "Swing"]);
        let swap = pick(&mut colours, "Swing", WorkspaceColour::Violet);
        assert_eq!(swap.swapped_with.as_deref(), Some("Scalp"));
        assert_eq!(swap.holder_colour, Some(WorkspaceColour::Cyan));
        assert!(swap.changed);
        assert_eq!(colours["Swing"], WorkspaceColour::Violet);
        assert_eq!(colours["Scalp"], WorkspaceColour::Cyan);
        let free = pick(&mut colours, "Swing", WorkspaceColour::Chalk);
        assert_eq!((free.changed, free.swapped_with), (true, None));
        assert_eq!(free.holder_colour, None);
        let again = pick(&mut colours, "Swing", WorkspaceColour::Chalk);
        assert!(!again.changed);
    }

    #[test]
    fn a_colourless_ninth_picking_a_held_colour_leaves_its_holder_none() {
        let names = ["Default", "A", "B", "C", "D", "E", "F", "G", "H"];
        let mut colours = settled(&names);
        let row = swatches(&colours, "H");
        assert!(row[4].owner_goes_without, "H holds nothing to give D back");
        assert!(row.iter().all(|swatch| swatch.owner.is_some()));
        let outcome = pick(&mut colours, "H", WorkspaceColour::Azure);
        assert_eq!(outcome.swapped_with.as_deref(), Some("D"));
        assert_eq!(outcome.holder_colour, None, "D goes without");
        assert_eq!(colours.get("D"), None);
        assert_eq!(colours.get("H"), Some(&WorkspaceColour::Azure));
    }

    #[test]
    fn swatches_mark_the_current_one_and_name_the_owners() {
        let colours = settled(&["Default", "Scalp", "Swing"]);
        let row = swatches(&colours, "Scalp");
        assert_eq!(row.len(), 8);
        assert!(row[1].current && row[1].owner.is_none());
        assert_eq!(row[0].owner.as_deref(), Some("Default"));
        assert_eq!(row[0].initials, "D");
        assert_eq!(row[2].initials, "Sw");
        assert!(
            !row[0].owner_goes_without,
            "Scalp gives Default violet back"
        );
        assert!(
            row[3..]
                .iter()
                .all(|swatch| swatch.owner.is_none() && !swatch.current)
        );
    }

    #[test]
    fn initials_grow_only_where_two_owners_share_a_letter() {
        let among = ["Default", "Scalp", "Swing", "Flow"];
        assert_eq!(initials("Default", &among), "D");
        assert_eq!(initials("Scalp", &among), "Sc");
        assert_eq!(initials("Swing", &among), "Sw");
        assert_eq!(initials("Flow", &["Flow"]), "F");
        assert_eq!(initials("Sw", &["Sw", "Swing"]), "Sw");
        assert_eq!(initials("Swing", &["Sw", "Swing"]), "Swi");
    }

    /// Owners alike past four characters still read apart, each in four.
    #[test]
    fn initials_tell_apart_names_alike_past_four_characters() {
        for among in [["Scalp", "Scalper"], ["Swing", "Swing2"]] {
            let [first, second] = among.map(|name| initials(name, &among));
            assert_ne!(first, second, "{among:?}");
            assert!(first.chars().count() <= INITIALS_MAX, "{first}");
            assert!(second.chars().count() <= INITIALS_MAX, "{second}");
        }
        assert_eq!(initials("Scalp", &["Scalp", "Scalper"]), "Sca1");
        assert_eq!(initials("Swing2", &["Swing", "Swing2"]), "Swi2");
        assert_eq!(initials("Default", &["Default", "Scalp", "Scalper"]), "D");
    }
}
