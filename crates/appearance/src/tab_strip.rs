//! The market tab strip's colour decisions, so every painter of a tab reads
//! the same answer (`docs/ux/layout-appearance-2026-11-06`).
use crate::Token;

/// What a market tab's feed is doing, as the mark beside its symbol says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabFeed {
    Live,
    /// Playing a recording; the tab also says so in words.
    Replay,
    /// A live session was lost or needs attention.
    Disconnected,
    /// No live session yet.
    Stopped,
}

/// The mark's shape: the state survives without its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkShape {
    Dot,
    Triangle,
    Ring,
}

impl TabFeed {
    /// A recording wins; a session needing attention reads as disconnected.
    #[must_use]
    pub const fn of(replaying: bool, connected: bool, attention: bool) -> Self {
        match (replaying, attention, connected) {
            (true, _, _) => Self::Replay,
            (false, true, _) => Self::Disconnected,
            (false, false, true) => Self::Live,
            (false, false, false) => Self::Stopped,
        }
    }

    #[must_use]
    pub const fn shape(self) -> MarkShape {
        match self {
            Self::Live => MarkShape::Dot,
            Self::Replay => MarkShape::Triangle,
            Self::Disconnected | Self::Stopped => MarkShape::Ring,
        }
    }

    /// Amber is not here: on a tab it means unsaved edits.
    #[must_use]
    pub const fn token(self) -> Token {
        match self {
            Self::Live => Token::BuyInk,
            Self::Replay => Token::ReplayInk,
            Self::Disconnected => Token::Warn,
            Self::Stopped => Token::TextFaint,
        }
    }

    /// The word printed after the symbol: a recording is never passed off as live.
    #[must_use]
    pub const fn word(self) -> Option<&'static str> {
        match self {
            Self::Replay => Some("Replay"),
            _ => None,
        }
    }
}

/// The fill behind a tab: the active one is the toolbar's own sheet, a
/// hovered or focused one lifts, the rest stay bare on the strip.
#[must_use]
pub const fn tab_fill(selected: bool, lit: bool) -> Option<Token> {
    match (selected, lit) {
        (true, _) => Some(Token::Chrome),
        (false, true) => Some(Token::BareHover),
        (false, false) => None,
    }
}

/// A tab's text and glyphs: the Home wordmark's "uan", a symbol, the ×.
#[must_use]
pub const fn tab_ink(lit: bool) -> Token {
    if lit {
        Token::TextPrimary
    } else {
        Token::TextMuted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LIGHT, ResolvedTheme, contrast::contrast_ratio};

    #[test]
    fn each_feed_state_has_its_own_shape_or_colour_and_replay_its_word() {
        let all = [
            TabFeed::Live,
            TabFeed::Replay,
            TabFeed::Disconnected,
            TabFeed::Stopped,
        ];
        for (index, a) in all.iter().enumerate() {
            for b in &all[index + 1..] {
                assert!(
                    a.shape() != b.shape() || a.token() != b.token(),
                    "{a:?} {b:?}"
                );
                assert_ne!(a.token(), Token::Amber, "amber means unsaved");
            }
        }
        assert_eq!(TabFeed::Replay.word(), Some("Replay"));
        assert!(all.iter().filter(|feed| feed.word().is_some()).count() == 1);
    }

    #[test]
    fn a_recording_wins_and_attention_reads_as_disconnected() {
        assert_eq!(TabFeed::of(true, false, true), TabFeed::Replay);
        assert_eq!(TabFeed::of(false, true, false), TabFeed::Live);
        assert_eq!(TabFeed::of(false, true, true), TabFeed::Disconnected);
        assert_eq!(TabFeed::of(false, false, true), TabFeed::Disconnected);
        assert_eq!(TabFeed::of(false, false, false), TabFeed::Stopped);
    }

    #[test]
    fn the_active_tab_wears_chrome_and_a_lit_one_the_hover() {
        assert_eq!(tab_fill(true, false), Some(Token::Chrome));
        assert_eq!(tab_fill(false, true), Some(Token::BareHover));
        assert_eq!(tab_fill(false, false), None);
        assert_eq!(tab_ink(true), Token::TextPrimary);
        assert_eq!(tab_ink(false), Token::TextMuted);
    }

    /// The three layout tokens differ between the themes and stay readable
    /// on the strip and on the active tab in both.
    #[test]
    fn dark_and_light_resolve_distinct_readable_strip_tokens() {
        let dark = ResolvedTheme::default();
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        for (token, want) in [
            (Token::TabStrip, [[0x10, 0x14, 0x1D], [0xD9, 0xD9, 0xD9]]),
            (Token::Brand, [[0x3A, 0xF0, 0xB2], [0x00, 0x7A, 0x55]]),
            (Token::ReplayInk, [[0xC5, 0x8C, 0xFF], [0x7F, 0x36, 0xB3]]),
        ] {
            for (theme, rgb) in [(&dark, want[0]), (&light, want[1])] {
                assert_eq!(theme.color(token).to_array()[..3], rgb, "{}", token.key());
            }
            assert_ne!(dark.color(token), light.color(token), "{}", token.key());
        }
        for theme in [&dark, &light] {
            let strip = theme.color(Token::TabStrip);
            assert_ne!(strip, theme.color(Token::Chrome), "the strip reads apart");
            // Text at 4.5:1; the wordmark and the feed marks are graphics, 3:1.
            let text = [(Token::TextMuted, 4.5), (Token::ReplayInk, 4.5)];
            let marks = [TabFeed::Live, TabFeed::Disconnected, TabFeed::Stopped];
            let graphics = marks.map(|feed| (feed.token(), 3.0));
            for (ink, floor) in text
                .into_iter()
                .chain([(Token::Brand, 3.0)])
                .chain(graphics)
            {
                for under in [strip, theme.color(Token::Chrome)] {
                    let ratio = contrast_ratio(theme.color(ink), under);
                    assert!(ratio >= floor, "{} {}: {ratio}", theme.name, ink.key());
                }
            }
        }
    }
}
