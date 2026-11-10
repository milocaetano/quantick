//! Headless models behind the widget panes: pure functions of the book, the
//! tape and a clock the caller passes in. `chart::widgets` turns each into
//! text, colour roles and geometry; the app only paints that.

pub mod big_trades;
pub mod depth;

#[cfg(test)]
mod big_trades_tests;
#[cfg(test)]
mod depth_tests;
