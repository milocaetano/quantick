//! The Fib tools' window half: the Levels tab both mount in the inspector.
//! The tools themselves are headless, in `quantick_drawings`.

mod levels_tab;

pub(super) use levels_tab::{draw_levels_tab, remember_drawing_color};

#[cfg(test)]
mod tests;
