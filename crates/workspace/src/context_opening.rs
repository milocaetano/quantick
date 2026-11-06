//! The workspace's pending context pane documents, consumed as panes are built.
use crate::arrangement_document::SavedPaneStyle;
#[derive(Debug, Default)]
pub struct ContextOpening {
    pub layouts: Vec<Option<u64>>,
    pub styles: Vec<SavedPaneStyle>,
}
impl ContextOpening {
    pub fn set_styles(&mut self, styles: &[SavedPaneStyle], existing_contexts: usize) {
        self.styles = styles
            .iter()
            .filter(|entry| entry.pane_index > existing_contexts)
            .cloned()
            .collect();
    }
    pub fn take_style(&mut self, pane_index: usize) -> Option<SavedPaneStyle> {
        let index = self
            .styles
            .iter()
            .position(|entry| entry.pane_index == pane_index)?;
        Some(self.styles.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_restored_style_is_consumed_once_and_existing_panes_have_no_pending_copy() {
        let entry = |pane_index| SavedPaneStyle {
            pane_index,
            version: 1,
            style: toml::Value::Table(Default::default()),
        };
        let mut opening = ContextOpening::default();
        opening.set_styles(&[entry(0), entry(1), entry(2)], 1);
        assert_eq!(opening.styles.len(), 1);
        assert!(opening.take_style(2).is_some());
        assert!(
            opening.take_style(2).is_none(),
            "a removed chart cannot donate its old style to a replacement"
        );
    }
}
