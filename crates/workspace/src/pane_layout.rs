//! Pane-local layout binding and persistence bookkeeping; session owns membership.
use crate::layout_document::{DrawingKey, LayoutId};
use crate::session::LayoutView;

#[derive(Default)]
pub struct PaneLayout {
    pub view: LayoutView,
    pub opening: Option<Option<LayoutId>>,
    pub label: String,
    pub drawings_key: Option<DrawingKey>,
    pub saved_revision: u64,
}
impl PaneLayout {
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
        let mut pane = PaneLayout::default();
        pane.request_opening(Some(LayoutId(9)));
        assert_eq!(pane.layout_id(), Some(LayoutId(9)));
        pane.request_opening(None);
        assert_eq!(pane.opening, Some(None));
        assert_eq!(pane.layout_id(), None);
        assert!(!pane.seeded());
        assert!(!pane.drawings_dirty(3));
    }
}
