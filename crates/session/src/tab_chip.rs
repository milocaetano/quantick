//! The tab chip's pure decisions: where each part of a market tab's face sits
//! and what its × slot shows. The strip paints; nothing here knows a pixel it
//! did not get.
//!
//! A tab reads `(W) WINZ26 [MT5] 172455 +0.41% ×`: the asset's icon, the
//! symbol, in its workspace's colour on the active tab only, the venue's
//! short code in an outlined tag, only when the feed is not healthy and live
//! its mark and "Replay" on a recording, then the last price and the day's
//! change. The tab hugs that content: no fixed width, no stretch gap. As the
//! strip fills, the closed tabs squeeze ([`Squeeze`]).

/// Where the face, its icon first, starts at every stage: past the corner mark
/// (`quantick_appearance::tab_strip::WORKSPACE_CORNER_PT`).
pub const TEXT_LEFT_PT: f32 = 17.0;
/// Between the symbol, the tag and the feed's mark.
pub const PART_GAP_PT: f32 = 7.0;
/// Between the mark and "Replay".
pub const WORD_GAP_PT: f32 = 3.0;
/// Air after the last part, before the close target.
pub const PADDING_RIGHT_PT: f32 = 2.0;
/// The venue tag: the code's padding either side.
pub const TAG_PADDING_PT: f32 = 4.0;
/// The widest the venue's code runs inside its tag: a code longer than four
/// capitals is clipped there, never widening the tab.
pub const TAG_CODE_MAX_PT: f32 = 30.0;
/// The feed's mark: a dot, a diamond, a triangle or a ring this wide a radius.
pub const MARK_RADIUS_PT: f32 = 3.0;
/// The text sizes of the symbol, the venue's code and "Replay".
pub const SYMBOL_TEXT_PT: f32 = 13.0;
pub const TAG_TEXT_PT: f32 = 9.5;
pub const WORD_TEXT_PT: f32 = 12.0;
/// Between the venue tag (or the feed's state) and the price.
pub const VALUE_GAP_PT: f32 = 9.0;
/// Between the price and the day's change.
pub const CHANGE_GAP_PT: f32 = 6.0;
/// On a squeezed tab's lower line: between the icon and the tag, and the tag
/// and the change (the mock's 3 px).
pub const SMALL_GAP_PT: f32 = 3.0;
pub const LOW_GAP_PT: f32 = 3.0;
/// How far a stacked line's middle sits above or below the tab's middle.
pub const STACK_DY_PT: f32 = 6.5;
/// How far the icon sits above, and the tag below, the middle at the last
/// squeeze.
pub const ICON_OVER_DY_PT: f32 = 6.0;
/// The icon's glyph, as a share of its disc.
pub const ICON_GLYPH_SHARE: f32 = 0.62;
/// How much taller the venue tag stands than its code's text size.
pub const TAG_RISE_PT: f32 = 3.5;

/// The sizes a stage writes a tab's parts in, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageSizes {
    /// The icon disc's diameter.
    pub icon: f32,
    pub symbol: f32,
    pub tag: f32,
    /// The code's air either side inside its tag.
    pub tag_padding: f32,
    pub price: f32,
    pub change: f32,
}

/// How far the closed tabs are squeezed: the open tab always reads on one
/// line, the closed ones give up first the line, then the price, then the
/// symbol (the trader's pick, option C, `docs/label/tabs-final`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Squeeze {
    /// Icon, symbol, venue, price and change on one line.
    OneLine,
    /// Symbol over icon and venue; price over change.
    Stacked,
    /// Symbol over icon, venue and change; no price.
    NoPrice,
    /// The icon over the venue alone.
    IconOverVenue,
}

/// The least strip room per tab each stage needs, in points: a strip with
/// less per tab squeezes one stage further. Set from the faces the mock's
/// symbols take beside the open tab and the labelled Home, closed tabs
/// reserving no room for a close target ([`CloseTarget`]): at a 1,918-point
/// window a stage is named only for counts whose faces fit: 6 on one line,
/// 10 stacked, 14 without a price, 26 icon over venue (`quantick-ui-model`'s
/// `the_mocks_stages_fit_a_1918_point_window`). [`Squeeze::fitting`]
/// covers longer faces.
pub const ONE_LINE_ROOM_PT: f32 = 250.0;
pub const STACKED_ROOM_PT: f32 = 150.0;
pub const NO_PRICE_ROOM_PT: f32 = 110.0;

impl Squeeze {
    /// The stage `tabs` market tabs take on a strip `strip_width` points wide.
    #[must_use]
    pub fn of_strip(tabs: usize, strip_width: f32) -> Self {
        let room = strip_width / tabs.max(1) as f32;
        if room >= ONE_LINE_ROOM_PT {
            Self::OneLine
        } else if room >= STACKED_ROOM_PT {
            Self::Stacked
        } else if room >= NO_PRICE_ROOM_PT {
            Self::NoPrice
        } else {
            Self::IconOverVenue
        }
    }

    /// The first stage from this one on at which the tabs' faces fit `room`:
    /// `needed(stage)` is the width they take at a stage, as measured. The
    /// count's guess can be short for long symbols or prices; the last
    /// squeeze stands when none fits, and the overflow takes the rest.
    #[must_use]
    pub fn fitting(self, room: f32, mut needed: impl FnMut(Self) -> f32) -> Self {
        let mut stage = self;
        while needed(stage) > room
            && let Some(next) = stage.next()
        {
            stage = next;
        }
        stage
    }

    const fn next(self) -> Option<Self> {
        match self {
            Self::OneLine => Some(Self::Stacked),
            Self::Stacked => Some(Self::NoPrice),
            Self::NoPrice => Some(Self::IconOverVenue),
            Self::IconOverVenue => None,
        }
    }

    /// The stage one tab wears: the open one never squeezes.
    #[must_use]
    pub const fn for_tab(self, open: bool) -> Self {
        if open { Self::OneLine } else { self }
    }

    /// The sizes this stage writes in: one line at full size, the stacked
    /// stages smaller so two lines fit the tab.
    #[must_use]
    pub const fn sizes(self) -> StageSizes {
        match self {
            Self::OneLine => StageSizes {
                icon: 16.0,
                symbol: SYMBOL_TEXT_PT,
                tag: TAG_TEXT_PT,
                tag_padding: TAG_PADDING_PT,
                price: 12.0,
                change: 11.0,
            },
            Self::Stacked | Self::NoPrice => StageSizes {
                icon: 10.0,
                symbol: 11.5,
                tag: 8.0,
                tag_padding: 3.0,
                price: 11.0,
                change: 10.0,
            },
            Self::IconOverVenue => StageSizes {
                icon: 13.0,
                symbol: 11.5,
                tag: 7.5,
                tag_padding: 2.0,
                price: 11.0,
                change: 10.0,
            },
        }
    }

    const fn shows_price(self) -> bool {
        matches!(self, Self::OneLine | Self::Stacked)
    }

    const fn shows_symbol(self) -> bool {
        !matches!(self, Self::IconOverVenue)
    }

    /// The stage's name on the control plane's scene.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OneLine => "one_line",
            Self::Stacked => "stacked",
            Self::NoPrice => "no_price",
            Self::IconOverVenue => "icon_over_venue",
        }
    }
}

/// What a tab's face is measured from: a change to any of it measures again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceSource<'a> {
    pub symbol: &'a str,
    pub code: &'a str,
    /// The feed wears a mark, and "Replay" on a recording.
    pub marked: bool,
    pub word: Option<&'static str>,
    /// The last price and the day's change, as written.
    pub price: Option<&'a str>,
    pub change: Option<&'a str>,
    /// The stage this tab wears ([`Squeeze::for_tab`]).
    pub stage: Squeeze,
    /// Physical pixels per point: text measures apart at another scale.
    pub scale: f32,
}

/// Each tab's face at each stage it was asked for, laid out once and kept
/// while what it was measured from holds, so the strip lays out no text a
/// frame for a tab only to learn its width, and none at all for a tab the
/// overflow hides.
///
/// The price and the change are kept by their shape — their length, sign
/// and separators, their digits as one class — and measured with every
/// digit at the widest, so an ordinary tick neither measures again nor
/// moves the tab's neighbours and × target.
#[derive(Debug, Default)]
pub struct FaceCache {
    faces: std::collections::BTreeMap<(u64, Squeeze), (Measured, FaceLayout)>,
}

#[derive(Debug)]
struct Measured {
    symbol: String,
    code: String,
    marked: bool,
    word: Option<&'static str>,
    price: Option<String>,
    change: Option<String>,
    scale: u32,
}

impl Measured {
    fn of(source: &FaceSource<'_>) -> Self {
        Self {
            symbol: source.symbol.to_owned(),
            code: source.code.to_owned(),
            marked: source.marked,
            word: source.word,
            price: source.price.map(str::to_owned),
            change: source.change.map(str::to_owned),
            scale: source.scale.to_bits(),
        }
    }

    fn holds(&self, source: &FaceSource<'_>) -> bool {
        (self.symbol == source.symbol && self.code == source.code)
            && (self.marked, self.word) == (source.marked, source.word)
            && same_shape(self.price.as_deref(), source.price)
            && same_shape(self.change.as_deref(), source.change)
            && self.scale == source.scale.to_bits()
    }
}

/// Whether two values have one shape: both missing, or the same text but
/// for which digit stands where.
fn same_shape(kept: Option<&str>, now: Option<&str>) -> bool {
    match (kept, now) {
        (Some(kept), Some(now)) => {
            kept.len() == now.len()
                && (kept.chars().zip(now.chars()))
                    .all(|(a, b)| a == b || (a.is_ascii_digit() && b.is_ascii_digit()))
        }
        (kept, now) => kept.is_none() && now.is_none(),
    }
}

/// `text`'s width at `pt` with every digit written as the widest digit.
fn shape_width(text: &str, pt: f32, width: &impl Fn(&str, f32) -> f32) -> f32 {
    let digit = |d: char| width(d.encode_utf8(&mut [0; 4]), pt);
    let widest = ('0'..='9').max_by(|a, b| digit(*a).total_cmp(&digit(*b)));
    let widest = widest.unwrap_or('0');
    let shaped: String = (text.chars())
        .map(|c| if c.is_ascii_digit() { widest } else { c })
        .collect();
    width(&shaped, pt)
}

impl FaceCache {
    /// Tab `id`'s face at `source`'s stage: the kept one while `source`
    /// holds, else laid out from `width`, the width of a text at a size in
    /// points, and kept.
    pub fn layout(
        &mut self,
        id: u64,
        source: &FaceSource<'_>,
        width: impl Fn(&str, f32) -> f32,
    ) -> FaceLayout {
        if let Some((measured, layout)) = self.faces.get(&(id, source.stage))
            && measured.holds(source)
        {
            return *layout;
        }
        let (stage, sizes) = (source.stage, source.stage.sizes());
        fn shown(text: Option<&str>, on: bool) -> Option<&str> {
            text.filter(|_| on)
        }
        let layout = lay_face(FaceParts {
            symbol: shown(Some(source.symbol), stage.shows_symbol())
                .map_or(0.0, |symbol| width(symbol, sizes.symbol)),
            code: width(source.code, sizes.tag),
            marked: source.marked,
            word: shown(source.word, stage == Squeeze::OneLine)
                .map(|word| width(word, WORD_TEXT_PT)),
            price: shown(source.price, stage.shows_price())
                .map(|price| shape_width(price, sizes.price, &width)),
            change: shown(source.change, stage.shows_symbol())
                .map(|change| shape_width(change, sizes.change, &width)),
            stage,
        });
        let kept = (Measured::of(source), layout);
        self.faces.insert((id, stage), kept);
        layout
    }

    /// Forget every tab but `ids`, the ones on the strip.
    pub fn keep(&mut self, ids: impl IntoIterator<Item = u64>) {
        let ids: Vec<u64> = ids.into_iter().collect();
        self.faces.retain(|(id, _), _| ids.contains(id));
    }
}

/// The parts of one tab's face as the painter measured them, in points, at
/// its stage's sizes; a part the stage does not show is not measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceParts {
    pub symbol: f32,
    pub code: f32,
    /// The feed wears a mark: it is not healthy and live.
    pub marked: bool,
    /// "Replay", on a recording, on one line only.
    pub word: Option<f32>,
    pub price: Option<f32>,
    pub change: Option<f32>,
    pub stage: Squeeze,
}

/// Where one part sits: its left edge from the tab's (an icon's: its
/// centre), its middle's offset below the tab's middle, and its size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub x: f32,
    pub dy: f32,
    pub pt: f32,
}

impl Spot {
    /// The same spot `dx` points further right.
    #[must_use]
    pub fn right(self, dx: f32) -> Self {
        Self {
            x: self.x + dx,
            ..self
        }
    }
}

/// Where each part sits, and the width the tab hugs, its close target aside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceLayout {
    /// The icon's disc: its centre, its diameter as `pt`.
    pub icon: Spot,
    /// `None` at the last squeeze.
    pub symbol: Option<Spot>,
    /// The tag's left edge and the code's size, as `pt`.
    pub tag: Spot,
    pub tag_width: f32,
    /// The code: its stage's padding inside the tag.
    pub code: Spot,
    /// The mark's centre, on the tab's middle, and "Replay" after it.
    pub mark: Option<f32>,
    pub word: Option<Spot>,
    pub price: Option<Spot>,
    pub change: Option<Spot>,
    pub width: f32,
}

impl FaceLayout {
    /// Half the venue tag's height: its code's size and the tag's rise.
    #[must_use]
    pub fn tag_half_height(&self) -> f32 {
        (self.tag.pt + TAG_RISE_PT) / 2.0
    }
}

/// Lay one tab's face out left to right at its stage.
#[must_use]
pub fn lay_face(parts: FaceParts) -> FaceLayout {
    let sizes = parts.stage.sizes();
    let tag_width = parts.code.min(TAG_CODE_MAX_PT) + 2.0 * sizes.tag_padding;
    let at = |x: f32, dy: f32, pt: f32| Spot { x, dy, pt };
    let (left, up, down) = (TEXT_LEFT_PT, -STACK_DY_PT, STACK_DY_PT);
    let quiet = FaceParts {
        word: None,
        ..parts
    };
    match parts.stage {
        Squeeze::OneLine => {
            let symbol = left + sizes.icon + PART_GAP_PT;
            let tag = symbol + parts.symbol + PART_GAP_PT;
            let (mut end, mark, word) = feed_state(tag + tag_width, parts);
            let price = parts.price.map(|width| {
                let x = end + VALUE_GAP_PT;
                end = x + width;
                at(x, 0.0, sizes.price)
            });
            let gap = [VALUE_GAP_PT, CHANGE_GAP_PT][usize::from(price.is_some())];
            let change = parts.change.map(|width| {
                let x = end + gap;
                end = x + width;
                at(x, 0.0, sizes.change)
            });
            FaceLayout {
                icon: at(left + sizes.icon / 2.0, 0.0, sizes.icon),
                symbol: Some(at(symbol, 0.0, sizes.symbol)),
                tag: at(tag, 0.0, sizes.tag),
                tag_width,
                code: at(tag + sizes.tag_padding, 0.0, sizes.tag),
                mark,
                word,
                price,
                change,
                width: end + PADDING_RIGHT_PT,
            }
        }
        Squeeze::Stacked | Squeeze::NoPrice => {
            let tag = left + sizes.icon + SMALL_GAP_PT;
            let mut low = tag + tag_width;
            let low_change = (parts.stage == Squeeze::NoPrice)
                .then_some(parts.change)
                .flatten()
                .map(|width| {
                    let x = low + LOW_GAP_PT;
                    low = x + width;
                    at(x, down, sizes.change)
                });
            let mut end = (left + parts.symbol).max(low);
            let (price, change) = match (parts.stage, parts.price.or(parts.change)) {
                (Squeeze::Stacked, Some(_)) => {
                    let column = parts.price.unwrap_or(0.0).max(parts.change.unwrap_or(0.0));
                    end += VALUE_GAP_PT + column;
                    let right = |width: f32, dy: f32, pt: f32| at(end - width, dy, pt);
                    let price = parts.price.map(|width| right(width, up, sizes.price));
                    (
                        price,
                        parts.change.map(|width| right(width, down, sizes.change)),
                    )
                }
                _ => (None, low_change),
            };
            let (end, mark, _) = feed_state(end, quiet);
            FaceLayout {
                icon: at(left + sizes.icon / 2.0, down, sizes.icon),
                symbol: Some(at(left, up, sizes.symbol)),
                tag: at(tag, down, sizes.tag),
                tag_width,
                code: at(tag + sizes.tag_padding, down, sizes.tag),
                mark,
                word: None,
                price,
                change,
                width: end + PADDING_RIGHT_PT,
            }
        }
        Squeeze::IconOverVenue => {
            let column = sizes.icon.max(tag_width);
            let tag = left + (column - tag_width) / 2.0;
            let (end, mark, _) = feed_state(left + column, quiet);
            FaceLayout {
                icon: at(left + column / 2.0, -ICON_OVER_DY_PT, sizes.icon),
                symbol: None,
                tag: at(tag, ICON_OVER_DY_PT, sizes.tag),
                tag_width,
                code: at(tag + sizes.tag_padding, ICON_OVER_DY_PT, sizes.tag),
                mark,
                word: None,
                price: None,
                change: None,
                width: end + PADDING_RIGHT_PT,
            }
        }
    }
}

/// The feed's mark and "Replay" after `end`, and where the face ends then.
fn feed_state(mut end: f32, parts: FaceParts) -> (f32, Option<f32>, Option<Spot>) {
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
        Spot {
            x: at,
            dy: 0.0,
            pt: WORD_TEXT_PT,
        }
    });
    (end, mark, word)
}

/// The close target's width, in points.
pub const CLOSE_TARGET_PT: f32 = 24.0;

/// Where a tab's close target sits. As in the mock, only the open tab keeps
/// room for its ×: a closed tab's × covers its face's end while hovered, and
/// at the last squeeze, too narrow to keep a close apart from the tab, it has
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseTarget {
    /// Beside the face, its room reserved.
    Beside,
    /// Over the face's end, no room of its own.
    Over,
    None,
}

impl CloseTarget {
    /// The target of a tab, `open` or not, wearing `stage`.
    #[must_use]
    pub const fn of(open: bool, stage: Squeeze) -> Self {
        match (open, stage) {
            (true, _) => Self::Beside,
            (false, Squeeze::IconOverVenue) => Self::None,
            (false, _) => Self::Over,
        }
    }

    /// The room the target adds beside the face.
    #[must_use]
    pub const fn reserve(self) -> f32 {
        match self {
            Self::Beside => CLOSE_TARGET_PT,
            Self::Over | Self::None => 0.0,
        }
    }

    /// The glyph the target shows: none where there is no target.
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Beside | Self::Over => "\u{00d7}",
            Self::None => "",
        }
    }

    /// The target's own width, at the tab's right end.
    #[must_use]
    pub const fn width(self) -> f32 {
        match self {
            Self::Beside | Self::Over => CLOSE_TARGET_PT,
            Self::None => 0.0,
        }
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
        price: None,
        change: None,
        stage: Squeeze::OneLine,
    };

    /// The mock's BTCUSDT: a price and a change measured beside it.
    const QUOTED: FaceParts = FaceParts {
        price: Some(52.0),
        change: Some(38.0),
        ..HEALTHY
    };

    #[test]
    fn a_healthy_tab_hugs_icon_symbol_and_tag() {
        let face = lay_face(HEALTHY);
        let icon = Squeeze::OneLine.sizes().icon;
        assert_eq!(face.icon.x, TEXT_LEFT_PT + icon / 2.0);
        let symbol = face.symbol.expect("one line names the symbol");
        assert_eq!(symbol.x, TEXT_LEFT_PT + icon + PART_GAP_PT);
        assert_eq!(face.tag.x, symbol.x + 50.0 + PART_GAP_PT);
        assert_eq!(face.tag_width, 20.0 + 2.0 * TAG_PADDING_PT);
        assert_eq!(
            (face.mark, face.word, face.price, face.change),
            (None, None, None, None)
        );
        assert_eq!(face.width, face.tag.x + face.tag_width + PADDING_RIGHT_PT);
    }

    #[test]
    fn one_line_reads_icon_symbol_venue_price_then_change() {
        let face = lay_face(QUOTED);
        let (price, change) = (face.price.unwrap(), face.change.unwrap());
        let tag_end = face.tag.x + face.tag_width;
        assert_eq!(price.x, tag_end + VALUE_GAP_PT);
        assert_eq!(change.x, price.x + 52.0 + CHANGE_GAP_PT);
        assert_eq!(face.width, change.x + 38.0 + PADDING_RIGHT_PT);
        assert!(
            [face.icon.dy, price.dy, change.dy]
                .iter()
                .all(|dy| *dy == 0.0)
        );
        let unpriced = lay_face(FaceParts {
            price: None,
            ..QUOTED
        });
        assert_eq!(unpriced.change.unwrap().x, tag_end + VALUE_GAP_PT);
        let lost = lay_face(FaceParts {
            marked: true,
            ..QUOTED
        });
        assert!(
            lost.price.unwrap().x > lost.mark.unwrap(),
            "state sits by the venue"
        );
    }

    #[test]
    fn stacked_puts_symbol_over_venue_and_price_over_change_right_aligned() {
        let face = lay_face(FaceParts {
            stage: Squeeze::Stacked,
            ..QUOTED
        });
        let (symbol, price, change) = (
            face.symbol.unwrap(),
            face.price.unwrap(),
            face.change.unwrap(),
        );
        assert_eq!((symbol.dy, price.dy), (-STACK_DY_PT, -STACK_DY_PT));
        assert_eq!(
            (face.icon.dy, face.tag.dy, change.dy),
            (STACK_DY_PT, STACK_DY_PT, STACK_DY_PT)
        );
        assert_eq!(price.x + 52.0, change.x + 38.0, "one right edge");
        assert_eq!(face.width, price.x + 52.0 + PADDING_RIGHT_PT);
        assert!(face.width < lay_face(QUOTED).width);
    }

    #[test]
    fn no_price_moves_the_change_beside_the_venue() {
        let face = lay_face(FaceParts {
            stage: Squeeze::NoPrice,
            ..QUOTED
        });
        assert_eq!(face.price, None);
        let change = face.change.unwrap();
        assert_eq!(change.x, face.tag.x + face.tag_width + LOW_GAP_PT);
        assert_eq!((change.dy, face.tag.dy), (STACK_DY_PT, STACK_DY_PT));
        let stacked = lay_face(FaceParts {
            stage: Squeeze::Stacked,
            ..QUOTED
        });
        assert!(face.width < stacked.width);
    }

    #[test]
    fn the_last_squeeze_keeps_the_icon_over_the_venue_alone() {
        let face = lay_face(FaceParts {
            stage: Squeeze::IconOverVenue,
            marked: true,
            word: Some(30.0),
            ..QUOTED
        });
        assert_eq!(
            (face.symbol, face.price, face.change, face.word),
            (None, None, None, None)
        );
        let column = face.tag_width.max(face.icon.pt);
        assert_eq!(face.icon.x, TEXT_LEFT_PT + column / 2.0);
        assert_eq!(face.tag.x + face.tag_width / 2.0, face.icon.x, "centred");
        assert!(face.icon.dy < 0.0 && face.tag.dy > 0.0);
        assert!(face.mark.is_some(), "a lost feed still says so");
    }

    #[test]
    fn a_marked_feed_and_its_word_follow_the_tag() {
        let lost = lay_face(FaceParts {
            marked: true,
            ..HEALTHY
        });
        let tag_end = lost.tag.x + lost.tag_width;
        assert_eq!(lost.mark, Some(tag_end + PART_GAP_PT + MARK_RADIUS_PT));
        let lost_end = tag_end + PART_GAP_PT + 2.0 * MARK_RADIUS_PT;
        assert_eq!(lost.width, lost_end + PADDING_RIGHT_PT);
        let replay = lay_face(FaceParts {
            marked: true,
            word: Some(30.0),
            ..HEALTHY
        });
        let word = replay.word.unwrap().x;
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
            price: None,
            change: None,
            stage: Squeeze::OneLine,
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
        let rescaled = FaceSource {
            scale: 2.0,
            ..replay
        };
        cache.layout(1, &rescaled, width);
        assert_eq!(measured.get(), 8);
        cache.layout(2, &source, width);
        cache.keep([2]);
        cache.layout(1, &source, width);
        assert_eq!(measured.get(), 12, "tab 1 was forgotten");
        let squeezed = FaceSource {
            stage: Squeeze::IconOverVenue,
            ..source
        };
        cache.layout(1, &squeezed, width);
        assert_eq!(
            measured.get(),
            13,
            "the last squeeze measures the code alone"
        );
        cache.layout(1, &source, width);
        assert_eq!(
            measured.get(),
            13,
            "each stage's face is kept beside the others"
        );
    }

    /// The price and the change are kept by their shape: a tick that only
    /// changes digits keeps the tab's width; a longer price or a flipped
    /// sign measures again.
    #[test]
    fn an_ordinary_tick_keeps_the_tabs_width() {
        let measured = std::cell::Cell::new(0);
        let width = |text: &str, pt: f32| {
            measured.set(measured.get() + 1);
            let narrow = text.chars().filter(|c| *c == '1').count() as f32;
            (text.chars().count() as f32 - narrow / 2.0) * pt
        };
        let mut cache = FaceCache::default();
        let quoted = |price, change| FaceSource {
            symbol: "BTCUSDT",
            code: "BIN",
            marked: false,
            word: None,
            price: Some(price),
            change: Some(change),
            stage: Squeeze::OneLine,
            scale: 1.0,
        };
        let face = cache.layout(1, &quoted("76200.00", "+1.12%"), width);
        let once = measured.get();
        assert_eq!(cache.layout(1, &quoted("76211.10", "+0.98%"), width), face);
        assert_eq!(measured.get(), once, "the same shape is not measured again");
        let mut digits = |id, price| cache.layout(id, &quoted(price, "+1.12%"), width).width;
        assert_eq!(
            digits(9, "11111.11"),
            digits(10, "00000.00"),
            "the widest digit"
        );
        assert!(cache.layout(1, &quoted("176200.00", "+1.12%"), width).width > face.width);
        cache.layout(1, &quoted("76200.00", "\u{2212}1.12%"), width);
        assert!(measured.get() > once, "a new length or sign measures again");
    }

    /// The mock's strip, 1,620 points: up to 6 tabs on one line, 7 to 10
    /// stacked, 11 to 14 without a price, then the icon over the venue.
    #[test]
    fn the_closed_tabs_squeeze_by_stage_as_the_strip_fills() {
        let strip = 1_620.0;
        assert_eq!(Squeeze::of_strip(0, strip), Squeeze::OneLine);
        assert_eq!(Squeeze::of_strip(6, strip), Squeeze::OneLine);
        assert_eq!(Squeeze::of_strip(7, strip), Squeeze::Stacked);
        assert_eq!(Squeeze::of_strip(10, strip), Squeeze::Stacked);
        assert_eq!(Squeeze::of_strip(11, strip), Squeeze::NoPrice);
        assert_eq!(Squeeze::of_strip(14, strip), Squeeze::NoPrice);
        assert_eq!(Squeeze::of_strip(15, strip), Squeeze::IconOverVenue);
        assert_eq!(Squeeze::of_strip(26, strip), Squeeze::IconOverVenue);
    }

    #[test]
    fn each_stage_starts_where_the_room_per_tab_runs_short() {
        let at = |room: f32| Squeeze::of_strip(4, 4.0 * room);
        assert_eq!(at(ONE_LINE_ROOM_PT), Squeeze::OneLine);
        assert_eq!(at(ONE_LINE_ROOM_PT - 0.5), Squeeze::Stacked);
        assert_eq!(at(STACKED_ROOM_PT), Squeeze::Stacked);
        assert_eq!(at(STACKED_ROOM_PT - 0.5), Squeeze::NoPrice);
        assert_eq!(at(NO_PRICE_ROOM_PT), Squeeze::NoPrice);
        assert_eq!(at(NO_PRICE_ROOM_PT - 0.5), Squeeze::IconOverVenue);
        assert_eq!(Squeeze::IconOverVenue.for_tab(true), Squeeze::OneLine);
        assert_eq!(Squeeze::NoPrice.for_tab(false), Squeeze::NoPrice);
    }

    #[test]
    fn a_strip_squeezes_further_when_the_measured_faces_do_not_fit() {
        let needed = |stage| match stage {
            Squeeze::OneLine => 900.0,
            Squeeze::Stacked => 700.0,
            Squeeze::NoPrice => 500.0,
            Squeeze::IconOverVenue => 300.0,
        };
        assert_eq!(Squeeze::OneLine.fitting(900.0, needed), Squeeze::OneLine);
        assert_eq!(Squeeze::OneLine.fitting(899.0, needed), Squeeze::Stacked);
        assert_eq!(Squeeze::Stacked.fitting(500.0, needed), Squeeze::NoPrice);
        assert_eq!(
            Squeeze::OneLine.fitting(100.0, needed),
            Squeeze::IconOverVenue,
            "the last squeeze stands; the overflow takes the rest"
        );
        let mut asked = Vec::new();
        let fit = Squeeze::Stacked.fitting(1_000.0, |stage| {
            asked.push(stage);
            0.0
        });
        assert_eq!(
            (fit, asked),
            (Squeeze::Stacked, vec![Squeeze::Stacked]),
            "never wider"
        );
    }

    /// Only the open tab keeps room for its ×; a closed one covers its
    /// face's end, and the last squeeze has none.
    #[test]
    fn only_the_open_tab_reserves_room_for_its_close_target() {
        let open = CloseTarget::of(true, Squeeze::OneLine);
        assert_eq!(
            (open, open.reserve()),
            (CloseTarget::Beside, CLOSE_TARGET_PT)
        );
        for stage in [Squeeze::OneLine, Squeeze::Stacked, Squeeze::NoPrice] {
            let closed = CloseTarget::of(false, stage);
            assert_eq!((closed, closed.reserve()), (CloseTarget::Over, 0.0));
            assert_eq!(closed.width(), CLOSE_TARGET_PT, "over the face's end");
        }
        let last = CloseTarget::of(false, Squeeze::IconOverVenue);
        assert_eq!(
            (last, last.reserve(), last.width()),
            (CloseTarget::None, 0.0, 0.0)
        );
    }

    #[test]
    fn the_close_slot_shows_the_cross_on_the_active_or_hovered_chip_only() {
        assert_eq!(close_slot(false, false), CloseSlot::Empty);
        assert_eq!(close_slot(false, true), CloseSlot::Close, "hover");
        assert_eq!(close_slot(true, false), CloseSlot::Close, "active");
    }
}
