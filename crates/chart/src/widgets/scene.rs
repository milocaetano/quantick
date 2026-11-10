//! A widget pane's paint list: what a view-model lays itself out as, in
//! pixels from the pane body's top-left corner. The app maps each item to
//! one shape and decides nothing; the read capability returns the same
//! list's text, so the screen and the read cannot disagree.

use quantick_appearance::Token;

use super::WidgetCell;

/// Height of one text row.
pub const ROW_PX: f32 = 17.0;

/// Space kept clear inside the pane body's left and right edges.
pub const INSET_PX: f32 = 6.0;

/// Opacity of a highlight band behind a row.
pub const BAND_OPACITY: f32 = 0.18;

/// Where a text item's anchor sits on the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// How large a text item is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSize {
    /// Column heads, footers and notes.
    Small,
    /// Table cells.
    Body,
    /// The headline figure of a card.
    Large,
}

/// One thing to paint.
#[derive(Debug, Clone, PartialEq)]
pub enum ScenePrim {
    /// A filled rectangle: `[x, y, width, height]` at `opacity`.
    Rect {
        rect: [f32; 4],
        token: Token,
        opacity: f32,
    },
    /// One run of text, vertically centred on `at`'s `y`.
    Text {
        at: [f32; 2],
        text: String,
        token: Token,
        align: Align,
        size: TextSize,
    },
    /// A polyline through `points`.
    Line { points: Vec<[f32; 2]>, token: Token },
    /// A filled circle.
    Circle {
        centre: [f32; 2],
        radius: f32,
        token: Token,
    },
}

/// A pane body as painted.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetScene {
    /// The header's chips, and whether each is the one in force.
    pub chips: Vec<(String, bool)>,
    /// Back to front.
    pub prims: Vec<ScenePrim>,
    /// Data rows laid out (ladder rows, prints, liquidations, points).
    pub rows: usize,
    /// What the pane says instead of data, when it has none.
    pub empty: Option<String>,
}

impl WidgetScene {
    /// A scene saying `text` in the middle of a `width` by `height` body.
    #[must_use]
    pub fn empty(text: &str, chips: Vec<(String, bool)>, width: f32, height: f32) -> Self {
        let mut scene = Self {
            chips,
            empty: Some(text.to_owned()),
            ..Self::default()
        };
        scene.text([width / 2.0, height / 2.0], text, Token::TextMuted, Align::Centre);
        scene
    }

    /// Body text at `at`.
    pub fn text(&mut self, at: [f32; 2], text: &str, token: Token, align: Align) {
        self.sized(at, text, token, align, TextSize::Body);
    }

    /// Text at `at` in `size`; nothing for an empty string.
    pub fn sized(&mut self, at: [f32; 2], text: &str, token: Token, align: Align, size: TextSize) {
        if text.is_empty() {
            return;
        }
        self.prims.push(ScenePrim::Text {
            at,
            text: text.to_owned(),
            token,
            align,
            size,
        });
    }

    /// One cell at `at`.
    pub fn cell(&mut self, at: [f32; 2], cell: &WidgetCell, align: Align) {
        self.text(at, &cell.text, cell.token, align);
    }

    /// A filled rectangle; nothing when it has no area.
    pub fn rect(&mut self, rect: [f32; 4], token: Token, opacity: f32) {
        if rect[2] > 0.0 && rect[3] > 0.0 {
            self.prims.push(ScenePrim::Rect {
                rect,
                token,
                opacity,
            });
        }
    }

    /// A polyline moved down by `dy`; nothing with fewer than two points.
    pub fn line(&mut self, points: &[[f32; 2]], dy: f32, token: Token) {
        if points.len() > 1 {
            let points = points.iter().map(|[x, y]| [*x, y + dy]).collect();
            self.prims.push(ScenePrim::Line { points, token });
        }
    }

    /// Column heads in faint small caps on the row centred at `y`.
    pub fn heads(&mut self, y: f32, heads: &[(&str, f32, Align)]) {
        for (head, x, align) in heads {
            self.sized([*x, y], head, Token::TextFaint, *align, TextSize::Small);
        }
    }

    /// Small faint text on the last row of a `height` body.
    pub fn footer(&mut self, height: f32, text: &str) {
        let at = [INSET_PX, height - ROW_PX / 2.0];
        self.sized(at, text, Token::TextFaint, Align::Left, TextSize::Small);
    }

    /// Every text item, in paint order.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.prims.iter().filter_map(|prim| match prim {
            ScenePrim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }
}

/// The centre `y` of row `index` below a `top` edge.
#[must_use]
pub fn row_y(top: f32, index: usize) -> f32 {
    top + ROW_PX * (index as f32 + 0.5)
}

/// How many whole rows fit between `top` and `bottom`.
#[must_use]
pub fn rows_between(top: f32, bottom: f32) -> usize {
    ((bottom - top) / ROW_PX).floor().max(0.0) as usize
}
