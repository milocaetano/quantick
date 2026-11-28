//! Headless models behind the widget panes: pure functions of the book, the
//! tape, the derivatives port and a clock the caller passes in.
//! `chart::widgets` turns each into text, colour roles and geometry; the app
//! only paints that.

pub mod big_trades;
pub mod depth;
pub mod liquidations;
pub mod ports;

#[cfg(test)]
mod big_trades_tests;
#[cfg(test)]
mod depth_tests;
#[cfg(test)]
mod liquidations_tests;
