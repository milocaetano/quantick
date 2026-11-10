//! Fitting a ledger line into the width it is given.
//!
//! Every ledger line has a left-anchored part and a right-anchored part, and
//! a narrow panel makes the two meet. The right part carries the result (a
//! trade's P&L, a day's net), so it is placed first and the left part gets
//! what is left: optional pieces drop, then the left text is elided with a
//! visible `…`. The result is preserved whenever it fits; otherwise it is visibly elided.
//! The two parts never overlap.
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
/// fits, the shortest right is elided only if it alone exceeds the width,
/// and the shortest left is elided
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
    let right = elide_to_width(rights.last().copied().unwrap_or_default(), width, &measure);
    let left = lefts.last().copied().unwrap_or_default();
    FittedLine {
        left: elide_to_width(
            left,
            width - gap_before(&right, gap) - measure(&right),
            &measure,
        ),
        right,
    }
}

fn gap_before(right: &str, gap: f32) -> f32 {
    if right.is_empty() { 0.0 } else { gap }
}

/// One ledger line, fitted: its left words piece by piece, its figure, and
/// whether the figure stands on a line of its own under the words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FittedPieces {
    pub left: Vec<String>,
    pub right: String,
    pub stacked: bool,
}

/// Fit a line that never wraps into `width`: `right` flush right, whole
/// wherever it fits the width alone, and `left`'s pieces, written in order
/// with at least `gap` before it, keeping what the rest of the width holds:
/// cut at their end with `…` ([`elide_tail`]), the pieces after the cut
/// dropped. With `stack`, a line whose words would be cut beside the figure
/// puts the figure under them instead, each fitted to the whole width.
/// `measure(i, text)` is how wide `text` paints as piece `i`; piece
/// `left.len()` is the figure.
pub fn fit_pieces(
    width: f32,
    gap: f32,
    left: &[&str],
    right: &str,
    stack: bool,
    measure: impl Fn(usize, &str) -> f32,
) -> FittedPieces {
    let figure = |text: &str| measure(left.len(), text);
    let right = elide_to_width(right, width, figure);
    let beside = width - gap_before(&right, gap) - figure(&right);
    let whole = left.iter().enumerate().map(|(i, piece)| measure(i, piece));
    let stacked = stack && !right.is_empty() && whole.sum::<f32>() > beside;
    let room = if stacked { width } else { beside };
    FittedPieces {
        left: elide_pieces(left, room, &measure),
        right,
        stacked,
    }
}

/// The longest form of `pieces` read as one text - whole, else cut at its
/// end with `…` - whose pieces `measure` puts within `width` together.
/// Bisected like [`elide_to_width`].
pub fn elide_pieces(
    pieces: &[&str],
    width: f32,
    measure: impl Fn(usize, &str) -> f32,
) -> Vec<String> {
    let total: usize = pieces.iter().map(|piece| piece.chars().count()).sum();
    let wide = |cut: &[String]| -> f32 {
        cut.iter()
            .enumerate()
            .map(|(i, piece)| measure(i, piece))
            .sum()
    };
    let whole = cut_pieces(pieces, total);
    if wide(&whole) <= width {
        return whole;
    }
    // Invariant: budget `lo` fits (zero is every piece empty), `hi` does not.
    let (mut lo, mut hi) = (0, total);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if wide(&cut_pieces(pieces, mid)) <= width {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cut_pieces(pieces, lo)
}

/// `pieces` cut to `budget` characters in all, the cut marked with `…` on
/// the piece it falls in (spaces and a dangling ` · ` before it dropped)
/// and every piece after it empty. Below two characters nothing is left.
fn cut_pieces(pieces: &[&str], budget: usize) -> Vec<String> {
    let total: usize = pieces.iter().map(|piece| piece.chars().count()).sum();
    if budget >= total {
        return pieces.iter().map(|piece| (*piece).to_owned()).collect();
    }
    let mut keep = budget.saturating_sub(1);
    let mut cut = budget < 2;
    pieces
        .iter()
        .map(|piece| {
            if cut {
                return String::new();
            }
            let count = piece.chars().count();
            if count < keep {
                keep -= count;
                return (*piece).to_owned();
            }
            cut = true;
            let kept: String = piece.chars().take(keep).collect();
            let kept = kept.trim_end_matches(|c: char| c.is_whitespace() || c == '·');
            format!("{kept}…")
        })
        .collect()
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
