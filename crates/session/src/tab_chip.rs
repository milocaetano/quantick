//! The tab chip's pure decisions: where each part of a market tab's face sits
//! and what its × slot shows. The strip paints; nothing here knows a pixel it
//! did not get.
//!
//! A tab reads `WINZ26 [MT5] ×`: the symbol, in its workspace's colour on the
//! active tab only, the venue's short code in an outlined tag and,
//! only when the feed is not healthy and live, its mark and "Replay" on a
//! recording. The tab hugs that content: no fixed width, no stretch gap.

/// Where the symbol starts: the tab's left air.
pub const TEXT_LEFT_PT: f32 = 10.0;
/// Between the symbol, the tag and the feed's mark.
pub const PART_GAP_PT: f32 = 7.0;
/// Between the mark and "Replay".
pub const WORD_GAP_PT: f32 = 3.0;
/// Air after the last part, before the close target.
pub const PADDING_RIGHT_PT: f32 = 2.0;
/// The venue tag: the code's padding either side, and the tag's height.
pub const TAG_PADDING_PT: f32 = 4.0;
pub const TAG_HEIGHT_PT: f32 = 13.0;
/// The widest the venue's code runs inside its tag: a code longer than four
/// capitals is clipped there, never widening the tab.
pub const TAG_CODE_MAX_PT: f32 = 30.0;
/// The feed's mark: a dot, a diamond, a triangle or a ring this wide a radius.
pub const MARK_RADIUS_PT: f32 = 3.0;
/// The text sizes of the symbol, the venue's code and "Replay".
pub const SYMBOL_TEXT_PT: f32 = 13.0;
pub const TAG_TEXT_PT: f32 = 9.5;
pub const WORD_TEXT_PT: f32 = 12.0;

/// What a tab's face is measured from: a change to any of it measures again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceSource<'a> {
    pub symbol: &'a str,
    pub code: &'a str,
    /// The feed wears a mark, and "Replay" on a recording.
    pub marked: bool,
    pub word: Option<&'static str>,
    /// Physical pixels per point: text measures apart at another scale.
    pub scale: f32,
}

/// Each tab's face, laid out once and kept while what it was measured from
/// holds, so the strip lays out no text a frame for a tab only to learn its
/// width, and none at all for a tab the overflow hides.
#[derive(Debug, Default)]
pub struct FaceCache {
    faces: std::collections::BTreeMap<u64, (Measured, FaceLayout)>,
}

#[derive(Debug)]
struct Measured {
    symbol: String,
    code: String,
    marked: bool,
    word: Option<&'static str>,
    scale: u32,
}

impl Measured {
    fn of(source: &FaceSource<'_>) -> Self {
        Self {
            symbol: source.symbol.to_owned(),
            code: source.code.to_owned(),
            marked: source.marked,
            word: source.word,
            scale: source.scale.to_bits(),
        }
    }

    fn holds(&self, source: &FaceSource<'_>) -> bool {
        (self.symbol == source.symbol && self.code == source.code)
            && (self.marked, self.word) == (source.marked, source.word)
            && self.scale == source.scale.to_bits()
    }
}

impl FaceCache {
    /// Tab `id`'s face: the kept one while `source` holds, else laid out from
    /// `width`, the width of a text at a size in points, and kept.
    pub fn layout(
        &mut self,
        id: u64,
        source: &FaceSource<'_>,
        width: impl Fn(&str, f32) -> f32,
    ) -> FaceLayout {
        if let Some((measured, layout)) = self.faces.get(&id)
            && measured.holds(source)
        {
            return *layout;
        }
        let layout = lay_face(FaceParts {
            symbol: width(source.symbol, SYMBOL_TEXT_PT),
            code: width(source.code, TAG_TEXT_PT),
            marked: source.marked,
            word: source.word.map(|word| width(word, WORD_TEXT_PT)),
        });
        self.faces.insert(id, (Measured::of(source), layout));
        layout
    }

    /// Forget every tab but `ids`, the ones on the strip.
    pub fn keep(&mut self, ids: impl IntoIterator<Item = u64>) {
        let ids: Vec<u64> = ids.into_iter().collect();
        if self.faces.len() > ids.len() {
            self.faces.retain(|id, _| ids.contains(id));
        }
    }
}

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
    let tag_width = parts.code.min(TAG_CODE_MAX_PT) + 2.0 * TAG_PADDING_PT;
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
    fn a_healthy_tab_hugs_symbol_and_tag() {
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
    fn a_long_venue_code_is_capped_in_its_tag() {
        let long = lay_face(FaceParts {
            code: 400.0,
            ..HEALTHY
        });
        assert_eq!(long.tag_width, TAG_CODE_MAX_PT + 2.0 * TAG_PADDING_PT);
    }

    /// A tab is measured once; a new code, mark or scale measures it again,
    /// and a closed tab is forgotten.
    #[test]
    fn a_face_is_measured_again_only_when_its_source_changes() {
        let measured = std::cell::Cell::new(0);
        let width = |text: &str, pt: f32| {
            measured.set(measured.get() + 1);
            text.len() as f32 * pt
        };
        let mut cache = FaceCache::default();
        let source = FaceSource {
            symbol: "WINZ26",
            code: "MT5",
            marked: false,
            word: None,
            scale: 1.0,
        };
        let first = cache.layout(1, &source, width);
        assert_eq!(measured.get(), 2, "the symbol and the code");
        assert_eq!(cache.layout(1, &source, width), first);
        assert_eq!(measured.get(), 2, "kept");
        let replay = FaceSource {
            marked: true,
            word: Some("Replay"),
            ..source
        };
        assert!(cache.layout(1, &replay, width).width > first.width);
        assert_eq!(measured.get(), 5);
        cache.layout(
            1,
            &FaceSource {
                scale: 2.0,
                ..replay
            },
            width,
        );
        assert_eq!(measured.get(), 8);
        cache.layout(2, &source, width);
        cache.keep([2]);
        cache.layout(1, &source, width);
        assert_eq!(measured.get(), 12, "tab 1 was forgotten");
    }

    #[test]
    fn the_close_slot_shows_the_cross_on_the_active_or_hovered_chip_only() {
        assert_eq!(close_slot(false, false), CloseSlot::Empty);
        assert_eq!(close_slot(false, true), CloseSlot::Close, "hover");
        assert_eq!(close_slot(true, false), CloseSlot::Close, "active");
    }
}
