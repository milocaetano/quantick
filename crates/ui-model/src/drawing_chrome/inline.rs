use super::{Drawing, DrawingEdit};
use quantick_chart_interaction::pane::PaneSide;

/// The editor's state: at most one open edit.
#[derive(Default)]
pub struct InlineEditor {
    edit: Option<DrawingEdit>,
}

impl InlineEditor {
    /// Which note is being typed, by index alone — what a second operator
    /// reads to know the keyboard belongs to an object.
    pub fn editing_index(&self) -> Option<usize> {
        self.edit.as_ref().map(|edit| edit.index)
    }

    /// The full target, which is what every pane needs to stand exactly one
    /// object down: an index alone names a different object on each pane.
    pub fn target(&self) -> Option<(u64, PaneSide, usize)> {
        self.edit
            .as_ref()
            .map(|edit| (edit.tab, edit.side, edit.index))
    }

    /// Open the editor on this object.
    ///
    /// Refused for an object that holds no words or is locked: a locked
    /// object's geometry and content are both protected, and an editor that
    /// opened and then dropped every keystroke would be worse than none.
    pub fn begin(&mut self, tab: u64, side: PaneSide, index: usize, drawing: &Drawing) -> bool {
        if drawing.locked || !drawing.tool.holds_text() {
            return false;
        }
        // Typing is one edit: the note as it stands is kept here and recorded
        // when the editor closes, so undo takes back the note, not the last
        // letter of it.
        self.edit = Some(DrawingEdit {
            tab,
            side,
            index,
            before: drawing.clone(),
        });
        true
    }

    /// Close the editor, handing back what the undo history is owed — on the
    /// pane the note actually lives on, which is not necessarily the one in
    /// front when it closes.
    pub fn end(&mut self) -> Option<DrawingEdit> {
        self.edit.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, Drawings};

    fn note() -> Drawing {
        let tool = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
            .expect("a tool that holds text");
        let mut store = Drawings::default();
        store.place_on(
            tool,
            &DrawingBand::Price,
            ChartPoint::at_time(1.0, 10.0, None),
        );
        store.items()[0].clone()
    }

    #[test]
    fn a_locked_note_refuses_the_caret() {
        let mut editor = InlineEditor::default();
        let mut drawing = note();
        drawing.locked = true;
        assert!(
            !editor.begin(1, PaneSide::Flow, 0, &drawing),
            "an editor that opened and then dropped every keystroke is worse than none"
        );
        assert_eq!(editor.target(), None);
    }

    /// The caret belongs to words. A rectangle has none, and an editor that
    /// opened over one would take the keyboard for nothing.
    #[test]
    fn an_object_without_words_refuses_the_caret() {
        let mut editor = InlineEditor::default();
        let mut wordless = note();
        wordless.tool = DRAWING_TOOLS
            .into_iter()
            .find(|tool| !tool.holds_text())
            .expect("a tool that holds no text");
        assert!(!editor.begin(1, PaneSide::Flow, 0, &wordless));
        assert_eq!(editor.target(), None);
    }

    /// The whole reason the target carries a tab and a pane: closing hands
    /// back the pane the note lives on, not the one in front.
    #[test]
    fn closing_hands_back_the_pane_the_note_lives_on() {
        let mut editor = InlineEditor::default();
        assert!(editor.begin(7, PaneSide::Time(0), 3, &note()));
        let closed = editor.end().expect("the editor was open");
        assert_eq!(
            (closed.tab, closed.side, closed.index),
            (7, PaneSide::Time(0), 3)
        );
        assert!(editor.end().is_none(), "closing twice owes nothing twice");
    }
}
