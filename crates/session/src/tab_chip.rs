//! The tab chip's pure decisions: what fits on a chip and what its × slot
//! shows. The strip paints; nothing here knows a pixel it did not get.
//!
//! A chip reads `• WINZ26 MetaTrader 5 ×`: the venue dot, the symbol and the
//! broker in muted ink. It names no workspace and wears no edited mark — the
//! status bar's cell is the one place the workspace and its mark show. When
//! narrow, the broker shortens to [`MIN_NAME_LETTERS`] and is dropped below
//! that. The symbol never shortens, and the gap before × is never given up.

/// The space kept before the close button, whatever else goes.
pub const CLOSE_GAP_PT: f32 = 8.0;
/// A broker name cut shorter than this reads as noise, so it is dropped.
pub const MIN_NAME_LETTERS: usize = 4;

/// What the painter measured for one chip, in points.
#[derive(Clone, Copy)]
pub struct ChipMeasure<'a> {
    /// The width the strip can give this chip.
    pub available: f32,
    /// The symbol, which never shortens.
    pub symbol: f32,
    /// The close button.
    pub close: f32,
    /// How many letters the broker name has.
    pub name_letters: usize,
    /// The width of the name's first `n` letters, with the ellipsis when cut.
    pub name_width: &'a dyn Fn(usize) -> f32,
}

/// What the chip shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipFit {
    /// How many letters of the broker to draw; zero drops it.
    pub name_letters: usize,
    /// The chip's width. Past `available` only when symbol, gap and × alone
    /// do not fit, which is the strip's to scroll.
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

/// The broker a chip names: the venue's name up to its ` — ` qualifier, so
/// `MetaTrader 5 — B3` reads `MetaTrader 5` and `Binance` stays whole.
#[must_use]
pub fn broker_name(venue: &str) -> &str {
    venue
        .split_once(" \u{2014} ")
        .map_or(venue, |(broker, _)| broker)
}

/// What the slot right of a chip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseSlot {
    /// The close button.
    Close,
    /// Nothing, the target still reserved.
    Empty,
}

/// The × on the active chip or under the pointer; elsewhere nothing. No chip
/// wears an edited mark: that is the status bar cell's.
#[must_use]
pub fn close_slot(active: bool, hovered: bool) -> CloseSlot {
    if active || hovered {
        CloseSlot::Close
    } else {
        CloseSlot::Empty
    }
}

/// Fit one chip.
#[must_use]
pub fn fit_chip(measure: &ChipMeasure<'_>) -> ChipFit {
    let fixed = measure.symbol + CLOSE_GAP_PT + measure.close;
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
        width: fixed + name(letters),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: f32 = 6.0;
    /// Symbol 40, close 10, and the gap: all there is before the broker.
    const FIXED: f32 = 40.0 + CLOSE_GAP_PT + 10.0;

    fn fit(available: f32, letters: usize) -> ChipFit {
        let width = |n: usize| n as f32 * LETTER;
        fit_chip(&ChipMeasure {
            available,
            symbol: 40.0,
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
    fn the_symbol_and_gap_are_never_given_up() {
        let fitted = fit(1.0, 8);
        assert_eq!(fitted.name_letters, 0);
        assert_eq!(fitted.width, FIXED);
    }

    #[test]
    fn the_broker_is_the_venue_name_before_its_qualifier() {
        assert_eq!(broker_name("MetaTrader 5 \u{2014} B3"), "MetaTrader 5");
        assert_eq!(broker_name("Binance"), "Binance");
        assert_eq!(
            broker_name("Binance USD-M perpetual"),
            "Binance USD-M perpetual"
        );
    }

    #[test]
    fn the_close_slot_shows_the_cross_on_the_active_or_hovered_chip_only() {
        assert_eq!(close_slot(false, false), CloseSlot::Empty);
        assert_eq!(close_slot(false, true), CloseSlot::Close, "hover");
        assert_eq!(close_slot(true, false), CloseSlot::Close, "active");
    }
}
