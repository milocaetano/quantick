//! Deterministic synthetic preview geometry, shared independently of painting.
use quantick_orderbook::BookSide;
pub const WALLS: [(f32, f32, f32, f32, f32, BookSide); 10] = [
    (0.16, 0.034, 0.00, 0.48, 0.34, BookSide::Ask),
    (0.16, 0.034, 0.48, 0.76, 0.73, BookSide::Ask),
    (0.27, 0.042, 0.00, 0.58, 0.92, BookSide::Ask),
    (0.27, 0.042, 0.58, 0.98, 0.40, BookSide::Ask),
    (0.39, 0.030, 0.08, 0.88, 0.50, BookSide::Ask),
    (0.61, 0.032, 0.00, 0.44, 0.36, BookSide::Bid),
    (0.61, 0.032, 0.44, 1.00, 0.66, BookSide::Bid),
    (0.72, 0.045, 0.00, 0.43, 0.88, BookSide::Bid),
    (0.72, 0.045, 0.43, 0.82, 0.24, BookSide::Bid),
    (0.84, 0.032, 0.10, 1.00, 0.54, BookSide::Bid),
];
pub const PATH: [(f32, f32); 8] = [
    (0.00, 0.59),
    (0.15, 0.57),
    (0.29, 0.63),
    (0.43, 0.68),
    (0.56, 0.53),
    (0.66, 0.29),
    (0.79, 0.36),
    (1.00, 0.25),
];
