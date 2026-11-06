//! The market tab strip's colour decisions, so every painter of a tab reads
//! the same answer (`docs/ux/layout-appearance-2026-11-06`).
use crate::Token;

/// The mark beside a tab's symbol. Which one a tab wears is the caller's
/// rule (`quantick_workspace`'s tab feed state); this is only how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedMark {
    Live,
    /// Connected while the venue reports a failure.
    Attention,
    Replay,
    /// A live session was lost.
    Lost,
    Stopped,
}

/// The mark's shape: the state survives without its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkShape {
    Dot,
    Diamond,
    Triangle,
    Ring,
}

/// The mark's shape: attention's filled diamond is neither live's dot nor
/// lost's hollow ring, though it shares the ring's ink.
#[must_use]
pub const fn feed_shape(mark: FeedMark) -> MarkShape {
    match mark {
        FeedMark::Live => MarkShape::Dot,
        FeedMark::Attention => MarkShape::Diamond,
        FeedMark::Replay => MarkShape::Triangle,
        FeedMark::Lost | FeedMark::Stopped => MarkShape::Ring,
    }
}

/// Amber is not here: on a tab it means unsaved edits.
#[must_use]
pub const fn feed_token(mark: FeedMark) -> Token {
    match mark {
        FeedMark::Live => Token::BuyInk,
        FeedMark::Replay => Token::ReplayInk,
        FeedMark::Attention | FeedMark::Lost => Token::Warn,
        FeedMark::Stopped => Token::TextFaint,
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

/// A market tab's text and glyphs: its symbol and the ×.
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
    fn each_feed_state_has_its_own_shape_or_colour() {
        let all = [
            FeedMark::Live,
            FeedMark::Attention,
            FeedMark::Replay,
            FeedMark::Lost,
            FeedMark::Stopped,
        ];
        // Attention is told from live and from lost by shape alone.
        for other in [FeedMark::Live, FeedMark::Lost] {
            assert_ne!(feed_shape(FeedMark::Attention), feed_shape(other));
        }
        for (index, a) in all.iter().enumerate() {
            for b in &all[index + 1..] {
                assert!(
                    feed_shape(*a) != feed_shape(*b) || feed_token(*a) != feed_token(*b),
                    "{a:?} {b:?}"
                );
                assert_ne!(feed_token(*a), Token::Amber, "amber means unsaved");
            }
        }
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
            let marks = [
                FeedMark::Live,
                FeedMark::Attention,
                FeedMark::Lost,
                FeedMark::Stopped,
            ];
            let graphics = marks.map(|mark| (feed_token(mark), 3.0));
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
