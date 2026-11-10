//! The published documents whose wire types this crate owns. The window's
//! catalogue adds the few it still owns itself and commits them all under
//! `schemas/control`.

/// Every document this crate publishes, as `(file name, schema)`.
#[must_use]
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    let mut documents = Vec::new();
    documents.extend(crate::workspace_book::documents());
    documents.extend(crate::native_window::documents());
    documents.extend(crate::home_tab::documents());
    documents.extend(crate::indicator_picker::documents());
    documents.extend(crate::scene::documents());
    documents.extend(crate::price_axis::documents());
    documents.extend(crate::price_tag_progress::documents());
    documents.extend(crate::chart_settings::documents());
    documents.extend(crate::journal_sessions::documents());
    documents.extend(crate::time_axis::documents());
    documents.extend(crate::opening_scale::documents());
    documents.extend(crate::bubble_scale::documents());
    documents.extend(crate::history::documents());
    documents.extend(crate::feed_source::documents());
    documents.extend(crate::tape_view::documents());
    documents.extend(crate::session::documents());
    documents.extend(crate::desk::documents());
    documents.extend(crate::contract_roll::documents());
    documents.extend(crate::market_windows::documents());
    documents.extend(crate::market_palette::documents());
    documents.extend(crate::bubble_preset::documents());
    documents.extend(crate::appearance::documents());
    documents.extend(crate::script_editor::documents());
    documents.extend(crate::canvas::documents());
    documents
}
