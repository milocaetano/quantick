//! The names a drawing gets when the trader gives it none: `zone-1`,
//! `line-2`, `fib-retracement-1`. A name is written into the saved document,
//! so every surface — the object list, the chart-context read, a strategy
//! naming the mark it rides — reads the same word, after a restart too.
//!
//! The number is per prefix and per market, one past the highest the market
//! already uses, so a deleted name is not handed to the next mark while the
//! higher one stands. A name the trader typed is never touched, even when it
//! happens to read like one of these.

use std::collections::BTreeMap;

/// The prefix of a rectangle's name: the trader's word for the region it marks.
pub const ZONE_PREFIX: &str = "zone";

/// The prefix of every straight-line tool's name.
pub const LINE_PREFIX: &str = "line";

/// The tool ids named [`ZONE_PREFIX`].
pub const ZONE_TOOL_IDS: [&str; 1] = ["rectangle"];

/// The tool ids named [`LINE_PREFIX`]: the drawing catalog's lines family,
/// which `quantick-drawings` checks this list against.
pub const LINE_TOOL_IDS: [&str; 7] = [
    "arrow",
    "extended-line",
    "horizontal-line",
    "horizontal-ray",
    "ray",
    "trend-line",
    "vertical-line",
];

/// The highest number each prefix has used: the floor the next name of that
/// prefix starts above.
pub type NameFloors = BTreeMap<String, u64>;

/// The prefix a mark of `tool_id` is named with; any tool outside the zone
/// and line groups is named by its own id.
#[must_use]
pub fn prefix_for(tool_id: &str) -> &str {
    if ZONE_TOOL_IDS.contains(&tool_id) {
        ZONE_PREFIX
    } else if LINE_TOOL_IDS.contains(&tool_id) {
        LINE_PREFIX
    } else {
        tool_id
    }
}

/// The prefix and number of `name` when it reads `<prefix>-<n>` with
/// `n >= 1`, whatever the prefix: a generated name of any tool, or a typed
/// name shaped like one, which the next generated name must not repeat.
#[must_use]
pub fn split_generated(name: &str) -> Option<(&str, u64)> {
    let (prefix, digits) = name.rsplit_once('-')?;
    if prefix.is_empty()
        || digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || digits.starts_with('0')
    {
        return None;
    }
    Some((prefix, digits.parse().ok()?))
}

/// Whether `name` is a generated name of a mark of `tool_id`.
#[must_use]
pub fn is_generated_for(name: &str, tool_id: &str) -> bool {
    split_generated(name).is_some_and(|(prefix, _)| prefix == prefix_for(tool_id))
}

/// Raise `floors` to every name in `names` shaped like a generated one.
pub fn raise_floors<'a>(floors: &mut NameFloors, names: impl IntoIterator<Item = &'a str>) {
    for (prefix, n) in names.into_iter().filter_map(split_generated) {
        let floor = floors.entry(prefix.to_owned()).or_insert(0);
        *floor = (*floor).max(n);
    }
}

/// The next name for a mark of `tool_id`, raising its prefix's floor so the
/// name is not handed out twice.
pub fn next_name(floors: &mut NameFloors, tool_id: &str) -> String {
    let prefix = prefix_for(tool_id);
    let floor = floors.entry(prefix.to_owned()).or_insert(0);
    *floor += 1;
    format!("{prefix}-{floor}")
}

#[cfg(test)]
#[path = "drawing_names_tests.rs"]
mod tests;
