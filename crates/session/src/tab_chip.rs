//! The tab chip's pure decisions: what fits on a chip and what its × slot
//! shows. The strip paints; nothing here knows a pixel it did not get.
//!
//! An active chip reads `SYMBOL Scalp • ×`, the amber mark after the name while
//! the tab holds edits. The name is paint, not a control: the workspace is
//! switched from the status bar's cell. An inactive chip shows the workspace in
//! grey where the venue was, and wears the mark in the × slot until the pointer
//! brings the × back. When narrow, the name shortens to [`MIN_NAME_LETTERS`]
//! and is dropped below that. The symbol never shortens, the mark never
//! leaves, and the gap before × is never given up.

/// The space kept before the close button, whatever else goes.
pub const CLOSE_GAP_PT: f32 = 8.0;
/// A workspace name cut shorter than this reads as noise, so it is dropped.
pub const MIN_NAME_LETTERS: usize = 4;

/// What the painter measured for one chip, in points.
#[derive(Clone, Copy)]
pub struct ChipMeasure<'a> {
    /// The width the strip can give this chip.
    pub available: f32,
    /// The symbol, which never shortens.
    pub symbol: f32,
    /// The amber mark and its gap while the tab holds edits, zero otherwise.
    pub mark: f32,
    /// Whether this is the active chip. An inactive one wears the mark in the
    /// × slot, so the mark costs its body nothing.
    pub active: bool,
    /// The close button.
    pub close: f32,
    /// How many letters the workspace name has.
    pub name_letters: usize,
    /// The width of the name's first `n` letters, with the ellipsis when cut.
    pub name_width: &'a dyn Fn(usize) -> f32,
}

/// What the chip shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipFit {
    /// How many letters of the name to draw; zero drops it.
    pub name_letters: usize,
    /// Whether the mark is drawn after the name.
    pub mark: bool,
    /// The chip's width. Past `available` only when symbol, mark, gap and ×
    /// alone do not fit, which is the strip's to scroll.
    pub width: f32,
}

/// The first `letters` of a name, with the ellipsis when that cuts it.
#[must_use]
pub fn shortened_name(name: &str, letters: usize) -> String {
    if letters >= name.chars().count() {
        return name.to_owned();
    }
    let mut cut: String = name.chars().take(letters).collect();
    cut.push('\u{2026}');
    cut
}

/// What the slot right of a chip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseSlot {
    /// The close button.
    Close,
    /// The amber mark of an inactive tab holding edits.
    Mark,
    /// Nothing, the target still reserved.
    Empty,
}

/// The × on the active chip or under the pointer; elsewhere the mark of a tab
/// holding edits, or nothing.
#[must_use]
pub fn close_slot(active: bool, hovered: bool, modified: bool) -> CloseSlot {
    if active || hovered {
        CloseSlot::Close
    } else if modified {
        CloseSlot::Mark
    } else {
        CloseSlot::Empty
    }
}

/// Fit one chip.
#[must_use]
pub fn fit_chip(measure: &ChipMeasure<'_>) -> ChipFit {
    let mark = if measure.active { measure.mark } else { 0.0 };
    let fixed = measure.symbol + mark + CLOSE_GAP_PT + measure.close;
    let name = |letters: usize| {
        if letters == 0 {
            0.0
        } else {
            (measure.name_width)(letters)
        }
    };
    let whole = name(measure.name_letters);
    if fixed + whole <= measure.available {
        return ChipFit {
            name_letters: measure.name_letters,
            mark: mark > 0.0,
            width: fixed + whole,
        };
    }
    // A name already under the minimum has no shorter form.
    let shortest = MIN_NAME_LETTERS.min(measure.name_letters);
    let letters = (shortest..=measure.name_letters)
        .rev()
        .find(|letters| fixed + name(*letters) <= measure.available)
        .unwrap_or(0);
    ChipFit {
        name_letters: letters,
        mark: mark > 0.0,
        width: fixed + name(letters),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: f32 = 6.0;
    /// Symbol 40, mark 0, close 10, and the gap: all there is before the name.
    const FIXED: f32 = 40.0 + CLOSE_GAP_PT + 10.0;

    fn fit(available: f32, letters: usize) -> ChipFit {
        let width = |n: usize| n as f32 * LETTER;
        fit_chip(&ChipMeasure {
            available,
            symbol: 40.0,
            mark: 0.0,
            active: true,
            close: 10.0,
            name_letters: letters,
            name_width: &width,
        })
    }

    #[test]
    fn a_cut_name_says_so_and_a_whole_one_does_not() {
        assert_eq!(shortened_name("Scalping", 4), "Scal\u{2026}");
        assert_eq!(shortened_name("Scalp", 5), "Scalp");
    }

    #[test]
    fn the_whole_chip_fits_when_there_is_room() {
        let needed = FIXED + 5.0 * LETTER;
        let fitted = fit(needed, 5);
        assert_eq!(fitted.name_letters, 5);
        assert_eq!(fitted.width, needed);
    }

    #[test]
    fn the_name_shortens_to_four_letters_and_is_then_dropped() {
        let at_four = FIXED + 4.0 * LETTER;
        assert_eq!(fit(at_four, 8).name_letters, 4);
        let dropped = fit(at_four - 0.5, 8);
        assert_eq!(dropped.name_letters, 0);
        assert_eq!(dropped.width, FIXED);
    }

    #[test]
    fn a_name_under_four_letters_is_shown_whole_or_dropped() {
        assert_eq!(fit(FIXED + 3.0 * LETTER, 3).name_letters, 3);
        assert_eq!(fit(FIXED + 3.0 * LETTER - 0.5, 3).name_letters, 0);
    }

    #[test]
    fn the_symbol_mark_and_gap_are_never_given_up() {
        let width = |n: usize| n as f32 * LETTER;
        let fitted = fit_chip(&ChipMeasure {
            available: 1.0,
            symbol: 40.0,
            mark: 6.0,
            active: true,
            close: 10.0,
            name_letters: 8,
            name_width: &width,
        });
        assert_eq!((fitted.name_letters, fitted.mark), (0, true));
        assert_eq!(fitted.width, 40.0 + 6.0 + CLOSE_GAP_PT + 10.0);
    }

    #[test]
    fn the_active_mark_follows_the_name_and_an_inactive_one_costs_the_body_nothing() {
        let width = |n: usize| n as f32 * LETTER;
        let measure = |active: bool, available: f32| {
            fit_chip(&ChipMeasure {
                available,
                symbol: 40.0,
                mark: 6.0,
                active,
                close: 10.0,
                name_letters: 5,
                name_width: &width,
            })
        };
        let whole = FIXED + 5.0 * LETTER;
        let active = measure(true, whole + 6.0);
        assert_eq!((active.name_letters, active.mark), (5, true));
        assert_eq!(active.width, whole + 6.0);
        assert_eq!(measure(true, whole).name_letters, 4, "the mark takes room");
        let inactive = measure(false, whole);
        assert_eq!((inactive.name_letters, inactive.mark), (5, false));
        assert_eq!(inactive.width, whole);
    }

    #[test]
    fn the_close_slot_shows_the_mark_only_on_a_resting_inactive_edited_tab() {
        assert_eq!(close_slot(false, false, true), CloseSlot::Mark);
        assert_eq!(close_slot(false, true, true), CloseSlot::Close, "hover");
        assert_eq!(close_slot(true, false, true), CloseSlot::Close, "active");
        assert_eq!(close_slot(false, false, false), CloseSlot::Empty);
    }
}
