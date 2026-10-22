//! Fitting a ledger line into the width it is given.
//!
//! Every ledger line has a left-anchored part and a right-anchored part, and
//! a narrow panel makes the two meet. The right part carries the result (a
//! trade's P&L, a day's net), so it is placed first and the left part gets
//! what is left: optional pieces drop, then the left text is elided with a
//! visible `…`. The result is never cut and the two parts never overlap.
//!
//! Widths come from a `measure` the caller supplies - the painter's text
//! layout in the app, a character count in a test - so the decision is the
//! same one whatever draws it.

/// The separator between the facts on one ledger line.
pub const FACT_SEPARATOR: &str = " · ";

/// Cut `text` to `budget` characters, marking the cut with `…` so a
/// shortened exit reason can never be read as a complete one. Below the
/// ellipsis plus one character there is nothing honest left to say, so the
/// text is dropped entirely rather than reduced to a lone `…`.
pub fn elide_tail(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_owned();
    }
    if budget < 2 {
        return String::new();
    }
    let mut out: String = text.chars().take(budget - 1).collect();
    out.push('…');
    out
}

/// The longest form of `text` - whole, else elided by [`elide_tail`] - that
/// `measure` puts within `width`. Empty when not even two characters fit.
/// A form only narrows as its budget shrinks, so the budget is found by
/// bisection: a handful of measures per call, never one per character.
pub fn elide_to_width(text: &str, width: f32, measure: impl Fn(&str) -> f32) -> String {
    let fits = |budget| measure(&elide_tail(text, budget)) <= width;
    let (mut lo, mut hi) = (0, text.chars().count());
    if fits(hi) {
        return text.to_owned();
    }
    // Invariant: `lo` fits (budget 0 is the empty string), `hi` does not.
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if fits(mid) { lo = mid } else { hi = mid }
    }
    if measure("") > width {
        return String::new();
    }
    elide_tail(text, lo)
}

/// The first fact of a ` · `-separated readout: the headline figure (money
/// where a point value is known, else points), without the qualifiers after
/// it that a narrow panel can do without.
pub fn first_fact(text: &str) -> &str {
    text.split(FACT_SEPARATOR).next().unwrap_or(text)
}

/// A day's or a session's right-hand readout, `count · net`, in its two
/// forms: whole, then with the net cut to its [`first_fact`]. The count and
/// the headline figure are never dropped.
pub fn count_and_net(count: usize, net: &str) -> [String; 2] {
    [
        format!("{count}{FACT_SEPARATOR}{net}"),
        format!("{count}{FACT_SEPARATOR}{}", first_fact(net)),
    ]
}

/// A line's left and right text, fitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FittedLine {
    pub left: String,
    pub right: String,
}

/// Fits a line into `width` with at least `gap` between its two parts.
/// `lefts` and `rights` are each part's forms, longest first. The left part
/// gives way before the right does: every left form is tried against the
/// full right before the right falls back to a shorter form. When no pair
/// fits, the shortest right stays whole and the shortest left is elided
/// into what remains. An empty right needs no gap.
pub fn fit_line(
    width: f32,
    gap: f32,
    lefts: &[&str],
    rights: &[&str],
    measure: impl Fn(&str) -> f32,
) -> FittedLine {
    for right in rights {
        let room = width - gap_before(right, gap) - measure(right);
        if let Some(left) = lefts.iter().find(|left| measure(left) <= room) {
            return FittedLine {
                left: (*left).to_owned(),
                right: (*right).to_owned(),
            };
        }
    }
    let right = rights.last().copied().unwrap_or_default();
    let left = lefts.last().copied().unwrap_or_default();
    FittedLine {
        left: elide_to_width(
            left,
            width - gap_before(right, gap) - measure(right),
            &measure,
        ),
        right: right.to_owned(),
    }
}

fn gap_before(right: &str, gap: f32) -> f32 {
    if right.is_empty() { 0.0 } else { gap }
}

/// What a trade row's head line keeps at a given width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadLine {
    /// Whether the instrument tag is shown.
    pub tag: bool,
    /// The entry-to-exit route, whole or elided.
    pub route: String,
}

/// Fits the part of a trade row's head line after side and quantity - the
/// route, then right-anchored the source marker, instrument tag and P&L -
/// into `width`, measured from where the route starts. `fixed` is the part
/// of that width nothing may cut: the P&L, the marker and their gaps; `tag`
/// is the tag with its gap. The tag goes first, then the route is elided.
pub fn fit_head(
    width: f32,
    fixed: f32,
    tag: f32,
    route: &str,
    measure: impl Fn(&str) -> f32,
) -> HeadLine {
    let room = width - fixed;
    if measure(route) + tag <= room {
        return HeadLine {
            tag: true,
            route: route.to_owned(),
        };
    }
    HeadLine {
        tag: false,
        route: elide_to_width(route, room, measure),
    }
}

#[cfg(test)]
#[path = "fit_tests.rs"]
mod fit_tests;
