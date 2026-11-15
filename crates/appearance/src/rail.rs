//! A folded pane's rail as the theme paints it: its band, its edges and its
//! grip (the chevron and the ticks), each in its own token at rest and while
//! the pointer is on the rail's hit area. Which mark wears which is the
//! workspace's (`canvas::RailInk`, whose order these follow).
use crate::Token;

/// The rail's inks: band, edge, grip, lit while `hot`.
#[must_use]
pub const fn rail_inks(hot: bool) -> [Token; 3] {
    if hot {
        [
            Token::RailBandHover,
            Token::RailEdgeHover,
            Token::RailGripHover,
        ]
    } else {
        [Token::RailBand, Token::RailEdge, Token::RailGrip]
    }
}
