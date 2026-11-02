//! Pane-local layout binding and persistence bookkeeping; session owns membership.
use crate::layout_document::LayoutId;
use crate::market_settings::DrawingKey;
use crate::session::LayoutView;

/// `Bars` is the host's bar rule, compared and never read here.
pub struct PaneLayout<Bars = ()> {
    pub view: LayoutView,
    pub opening: Option<Option<LayoutId>>,
    pub label: String,
    pub drawings_key: Option<DrawingKey>,
    /// The feed and symbol `drawings_key` was resolved from, so a frame can
    /// tell a tab moved without resolving the asset again.
    pub drawings_source: Option<(String, String)>,
    /// The bars `drawings_key` was resolved at: a frame compares this instead
    /// of formatting a role, and a change re-keys a context chart.
    pub drawings_spec: Option<Bars>,
    pub saved_revision: u64,
}
impl<Bars> Default for PaneLayout<Bars> {
    fn default() -> Self {
        Self {
            view: LayoutView::default(),
            opening: None,
            label: String::new(),
            drawings_key: None,
            drawings_source: None,
            drawings_spec: None,
            saved_revision: 0,
        }
    }
}
impl<Bars> PaneLayout<Bars> {
    pub fn layout_id(&self) -> Option<LayoutId> {
        self.opening.unwrap_or_else(|| self.view.layout())
    }
    pub fn request_opening(&mut self, id: Option<LayoutId>) {
        self.opening = Some(id);
    }
    pub fn seeded(&self) -> bool {
        self.view.seeded()
    }
    pub fn drawings_dirty(&self, revision: u64) -> bool {
        self.drawings_key.is_some() && revision != self.saved_revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_default_is_distinct_from_no_request() {
        let mut pane = PaneLayout::<()>::default();
        pane.request_opening(Some(LayoutId(9)));
        assert_eq!(pane.layout_id(), Some(LayoutId(9)));
        pane.request_opening(None);
        assert_eq!(pane.opening, Some(None));
        assert_eq!(pane.layout_id(), None);
        assert!(!pane.seeded());
        assert!(!pane.drawings_dirty(3));
    }
}
