//! The declared list: every family's rows, joined into one table.
//!
//! Adding a capability adds a row to its own family module. The only line
//! that changes here is a new family's, which is why this file is short and
//! stays short.

use crate::readback::{Readback, flatten, total};
use crate::{analysis, attention, feed, indicator_guide, layers, layout, notify, script, trade};

/// Every family that declares readback rows. Order is irrelevant: the
/// document is rendered in the registry's order.
pub const FAMILIES: &[&[Readback]] = &[
    crate::alerts::READBACKS,
    analysis::READBACKS,
    crate::appearance::READBACKS,
    attention::READBACKS,
    crate::bubble_save::READBACKS,
    crate::bubble_scale::READBACKS,
    crate::canvas::READBACKS,
    crate::chart_settings::READBACKS,
    crate::indicator_picker::READBACKS,
    crate::native_indicator::READBACKS,
    crate::drawing_magnet::READBACKS,
    crate::drawing_scope::READBACKS,
    crate::drawing_text::READBACKS,
    feed::READBACKS,
    crate::history::READBACKS,
    indicator_guide::READBACKS,
    crate::journal_sessions::READBACKS,
    layers::READBACKS,
    layout::READBACKS,
    notify::READBACKS,
    crate::opening_scale::READBACKS,
    crate::tick_bubbles::READBACKS,
    crate::bubble_preset::READBACKS,
    crate::pnl_display::READBACKS,
    crate::desk::READBACKS,
    crate::workspace_book::READBACKS,
    crate::price_axis::READBACKS,
    crate::price_tag_progress::READBACKS,
    crate::home_tab::READBACKS,
    crate::native_window::READBACKS,
    crate::market_windows::READBACKS,
    crate::market_palette::READBACKS,
    crate::orderflow_settings::READBACKS,
    crate::tape_fold::READBACKS,
    crate::tape_market_fold::READBACKS,
    crate::tape_view::READBACKS,
    crate::tape_view_set::READBACKS,
    crate::time_axis::READBACKS,
    script::READBACKS,
    crate::script_editor::READBACKS,
    trade::READBACKS,
    crate::widget::READBACKS,
];

const TOTAL: usize = total(FAMILIES);
const JOINED: [Readback; TOTAL] = flatten(FAMILIES);

/// Every mutable capability's readback.
pub const READBACKS: &[Readback] = &JOINED;

/// The row for `capability`, if the matrix has one.
pub fn readback(capability: &str) -> Option<&'static Readback> {
    READBACKS.iter().find(|row| row.capability == capability)
}
