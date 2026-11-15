//! A folded pane's rail as the theme paints it: one band framed by an edge
//! along each long side, and a grip, each in its own token at rest and while
//! the pointer is on the rail's hit area. Where each mark goes is the
//! workspace's `canvas::rail_marks`, laid out in the same order.
use crate::Token;

/// The rail's inks back to front: the band, its two edges and the grip, lit
/// while `hot`.
#[must_use]
pub const fn rail_inks(hot: bool) -> [Token; 4] {
    if hot {
        [
            Token::RailBandHover,
            Token::RailEdgeHover,
            Token::RailEdgeHover,
            Token::RailGripHover,
        ]
    } else {
        [
            Token::RailBand,
            Token::RailEdge,
            Token::RailEdge,
            Token::RailGrip,
        ]
    }
}
