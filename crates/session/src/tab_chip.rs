//! The tab chip's pure decisions: where each part of a market tab's face sits
//! and what its × slot shows. The strip paints; nothing here knows a pixel it
//! did not get.
//!
//! A tab reads `▌WINZ26 [MT5] ×`: the bar in its workspace's colour glued to
//! the left edge, the symbol, the venue's short code in an outlined tag and,
//! only when the feed is not healthy and live, its mark and "Replay" on a
//! recording. The tab hugs that content: no fixed width, no stretch gap.

/// The bar in the workspace's colour along the tab's left edge.
pub const BAR_PT: f32 = 9.0;
/// Where the symbol starts: past the bar and its 7 pt of air.
pub const TEXT_LEFT_PT: f32 = BAR_PT + 7.0;
/// Between the symbol, the tag and the feed's mark.
pub const PART_GAP_PT: f32 = 7.0;
/// Between the mark and "Replay".
pub const WORD_GAP_PT: f32 = 3.0;
/// Air after the last part, before the close target.
pub const PADDING_RIGHT_PT: f32 = 2.0;
/// The venue tag: the code's padding either side, and the tag's height.
pub const TAG_PADDING_PT: f32 = 4.0;
pub const TAG_HEIGHT_PT: f32 = 13.0;
/// The feed's mark: a dot, a diamond, a triangle or a ring this wide a radius.
pub const MARK_RADIUS_PT: f32 = 3.0;

/// The parts of one tab's face as the painter measured them, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceParts {
    pub symbol: f32,
    pub code: f32,
    /// The feed wears a mark: it is not healthy and live.
    pub marked: bool,
    /// "Replay", on a recording.
    pub word: Option<f32>,
}

/// Where each part starts from the tab's left edge, and the width the tab
/// hugs, its close target aside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceLayout {
    pub symbol: f32,
    /// The tag's left edge and width; the code sits [`TAG_PADDING_PT`] in.
    pub tag: f32,
    pub tag_width: f32,
    /// The mark's centre.
    pub mark: Option<f32>,
    pub word: Option<f32>,
    pub width: f32,
}

/// Lay one tab's face out left to right.
#[must_use]
pub fn lay_face(parts: FaceParts) -> FaceLayout {
    let tag = TEXT_LEFT_PT + parts.symbol + PART_GAP_PT;
    let tag_width = parts.code + 2.0 * TAG_PADDING_PT;
    let mut end = tag + tag_width;
    let mark = parts.marked.then_some(end + PART_GAP_PT + MARK_RADIUS_PT);
    if let Some(mark) = mark {
        end = mark + MARK_RADIUS_PT;
    }
    let word = parts.word.map(|width| {
        let at = end
            + if mark.is_some() {
                WORD_GAP_PT
            } else {
                PART_GAP_PT
            };
        end = at + width;
        at
    });
    FaceLayout {
        symbol: TEXT_LEFT_PT,
        tag,
        tag_width,
        mark,
        word,
        width: end + PADDING_RIGHT_PT,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const HEALTHY: FaceParts = FaceParts {
        symbol: 50.0,
        code: 20.0,
        marked: false,
        word: None,
    };

    #[test]
    fn a_healthy_tab_hugs_bar_symbol_and_tag() {
        let face = lay_face(HEALTHY);
        assert_eq!(face.symbol, TEXT_LEFT_PT);
        assert_eq!(face.tag, TEXT_LEFT_PT + 50.0 + PART_GAP_PT);
        assert_eq!(face.tag_width, 20.0 + 2.0 * TAG_PADDING_PT);
        assert_eq!((face.mark, face.word), (None, None));
        assert_eq!(face.width, face.tag + face.tag_width + PADDING_RIGHT_PT);
    }

    #[test]
    fn a_marked_feed_and_its_word_follow_the_tag() {
        let lost = lay_face(FaceParts {
            marked: true,
            ..HEALTHY
        });
        let tag_end = lost.tag + lost.tag_width;
        assert_eq!(lost.mark, Some(tag_end + PART_GAP_PT + MARK_RADIUS_PT));
        let lost_end = tag_end + PART_GAP_PT + 2.0 * MARK_RADIUS_PT;
        assert_eq!(lost.width, lost_end + PADDING_RIGHT_PT);
        let replay = lay_face(FaceParts {
            marked: true,
            word: Some(30.0),
            ..HEALTHY
        });
        let word = replay.word.unwrap();
        assert_eq!(word, replay.mark.unwrap() + MARK_RADIUS_PT + WORD_GAP_PT);
        assert_eq!(replay.width, word + 30.0 + PADDING_RIGHT_PT);
        assert!(replay.width > lost.width && lost.width > lay_face(HEALTHY).width);
    }

    #[test]
    fn the_close_slot_shows_the_cross_on_the_active_or_hovered_chip_only() {
        assert_eq!(close_slot(false, false), CloseSlot::Empty);
        assert_eq!(close_slot(false, true), CloseSlot::Close, "hover");
        assert_eq!(close_slot(true, false), CloseSlot::Close, "active");
    }
}
