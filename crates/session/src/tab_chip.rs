//! The tab chip's two pure decisions: the workspace button's gesture, and what
//! fits on a chip. The strip paints; nothing here knows a pixel it did not get.
//!
//! An active chip reads `SYMBOL • Scalp ⌄ ×`. An inactive one shows the
//! workspace in grey where the venue was. The arrow goes first when narrow, the
//! name then shortens to [`MIN_NAME_LETTERS`] and is dropped below that. The
//! symbol never shortens, the mark never leaves, and the gap before × is never
//! given up.

/// How far the pointer may travel from the press before the gesture cancels.
pub const BUTTON_CANCEL_TRAVEL_PT: f32 = 4.0;
/// The space kept before the close button, whatever else goes.
pub const CLOSE_GAP_PT: f32 = 8.0;
/// A workspace name cut shorter than this reads as noise, so it is dropped.
pub const MIN_NAME_LETTERS: usize = 4;

/// What a release of the workspace button means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabButtonRelease {
    /// Open the menu.
    Open,
    /// The pointer travelled: nothing happens.
    Cancelled,
    /// No press was held.
    Idle,
}

/// The press on the workspace button. It opens on release, cancels once the
/// pointer has travelled, and owns the pointer while held, so the strip beneath
/// it never starts a tab drag.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TabButtonGesture {
    origin: Option<[f32; 2]>,
    cancelled: bool,
}

impl TabButtonGesture {
    /// The pointer went down on the button.
    pub fn press(&mut self, at: [f32; 2]) {
        self.origin = Some(at);
        self.cancelled = false;
    }

    /// The pointer moved. Travel is latched: coming back does not undo a cancel.
    pub fn moved(&mut self, to: [f32; 2]) {
        if let Some(origin) = self.origin {
            let (dx, dy) = (to[0] - origin[0], to[1] - origin[1]);
            let reach = BUTTON_CANCEL_TRAVEL_PT;
            self.cancelled |= dx * dx + dy * dy >= reach * reach;
        }
    }

    /// The pointer came up.
    pub fn release(&mut self) -> TabButtonRelease {
        let held = self.origin.take().is_some();
        match (held, std::mem::take(&mut self.cancelled)) {
            (false, _) => TabButtonRelease::Idle,
            (true, true) => TabButtonRelease::Cancelled,
            (true, false) => TabButtonRelease::Open,
        }
    }

    /// Whether the strip must leave the pointer alone: held, cancelled or not,
    /// a press that began on the button is never a tab drag.
    #[must_use]
    pub fn holds_pointer(&self) -> bool {
        self.origin.is_some()
    }
}

/// What the painter measured for one chip, in points.
#[derive(Clone, Copy)]
pub struct ChipMeasure<'a> {
    /// The width the strip can give this chip.
    pub available: f32,
    /// The symbol, which never shortens.
    pub symbol: f32,
    /// The amber mark while the tab holds edits, zero otherwise.
    pub mark: f32,
    /// The arrow on the active chip, zero on an inactive one.
    pub arrow: f32,
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
    /// Whether the arrow is drawn.
    pub arrow: bool,
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

/// Fit one chip.
#[must_use]
pub fn fit_chip(measure: &ChipMeasure<'_>) -> ChipFit {
    let fixed = measure.symbol + measure.mark + CLOSE_GAP_PT + measure.close;
    let name = |letters: usize| {
        if letters == 0 {
            0.0
        } else {
            (measure.name_width)(letters)
        }
    };
    let whole = name(measure.name_letters);
    if fixed + whole + measure.arrow <= measure.available {
        return ChipFit {
            name_letters: measure.name_letters,
            arrow: measure.arrow > 0.0,
            width: fixed + whole + measure.arrow,
        };
    }
    // The arrow has gone. A name already under the minimum has no shorter form.
    let shortest = MIN_NAME_LETTERS.min(measure.name_letters);
    let letters = (shortest..=measure.name_letters)
        .rev()
        .find(|letters| fixed + name(*letters) <= measure.available)
        .unwrap_or(0);
    ChipFit {
        name_letters: letters,
        arrow: false,
        width: fixed + name(letters),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_still_press_opens_on_release_and_never_before() {
        let mut gesture = TabButtonGesture::default();
        gesture.press([10.0, 10.0]);
        assert!(gesture.holds_pointer(), "the strip starts no tab drag");
        assert_eq!(gesture.release(), TabButtonRelease::Open);
        assert!(!gesture.holds_pointer());
        assert_eq!(gesture.release(), TabButtonRelease::Idle);
    }

    #[test]
    fn just_under_the_travel_still_opens() {
        let mut gesture = TabButtonGesture::default();
        gesture.press([0.0, 0.0]);
        gesture.moved([BUTTON_CANCEL_TRAVEL_PT - 0.01, 0.0]);
        assert_eq!(gesture.release(), TabButtonRelease::Open);
    }

    #[test]
    fn the_travel_itself_cancels_in_any_direction_and_coming_back_does_not_undo_it() {
        let mut gesture = TabButtonGesture::default();
        gesture.press([0.0, 0.0]);
        gesture.moved([0.0, BUTTON_CANCEL_TRAVEL_PT]);
        assert!(
            gesture.holds_pointer(),
            "a cancelled press is still no drag"
        );
        gesture.moved([0.0, 0.0]);
        assert_eq!(gesture.release(), TabButtonRelease::Cancelled);
        gesture.press([0.0, 0.0]);
        gesture.moved([3.0, 3.0]);
        assert_eq!(gesture.release(), TabButtonRelease::Cancelled, "diagonal");
        gesture.press([0.0, 0.0]);
        assert_eq!(gesture.release(), TabButtonRelease::Open, "a new press");
    }

    const LETTER: f32 = 6.0;
    /// Symbol 40, mark 0, close 10, and the gap: all there is before the name.
    const FIXED: f32 = 40.0 + CLOSE_GAP_PT + 10.0;

    fn fit(available: f32, arrow: f32, letters: usize) -> ChipFit {
        let width = |n: usize| n as f32 * LETTER;
        fit_chip(&ChipMeasure {
            available,
            symbol: 40.0,
            mark: 0.0,
            arrow,
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
        let needed = FIXED + 5.0 * LETTER + 12.0;
        let fitted = fit(needed, 12.0, 5);
        assert_eq!((fitted.name_letters, fitted.arrow), (5, true));
        assert_eq!(fitted.width, needed);
    }

    #[test]
    fn the_arrow_goes_first_and_the_name_stays_whole() {
        let needed = FIXED + 5.0 * LETTER + 12.0;
        let fitted = fit(needed - 0.5, 12.0, 5);
        assert_eq!((fitted.name_letters, fitted.arrow), (5, false));
    }

    #[test]
    fn the_name_shortens_to_four_letters_and_is_then_dropped() {
        let at_four = FIXED + 4.0 * LETTER;
        assert_eq!(fit(at_four, 0.0, 8).name_letters, 4);
        let dropped = fit(at_four - 0.5, 0.0, 8);
        assert_eq!(dropped.name_letters, 0);
        assert_eq!(dropped.width, FIXED);
    }

    #[test]
    fn a_name_under_four_letters_is_shown_whole_or_dropped() {
        assert_eq!(fit(FIXED + 3.0 * LETTER, 0.0, 3).name_letters, 3);
        assert_eq!(fit(FIXED + 3.0 * LETTER - 0.5, 0.0, 3).name_letters, 0);
    }

    #[test]
    fn the_symbol_mark_and_gap_are_never_given_up() {
        let width = |n: usize| n as f32 * LETTER;
        let fitted = fit_chip(&ChipMeasure {
            available: 1.0,
            symbol: 40.0,
            mark: 6.0,
            arrow: 12.0,
            close: 10.0,
            name_letters: 8,
            name_width: &width,
        });
        assert_eq!((fitted.name_letters, fitted.arrow), (0, false));
        assert_eq!(fitted.width, 40.0 + 6.0 + CLOSE_GAP_PT + 10.0);
    }
}
