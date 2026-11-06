//! A centred question and its answers, apart from its pixels: the window
//! paints any [`Prompt`] the same way, so a new question is words, not a
//! painter.

/// One question the window asks before it acts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The window's identity, stable while the question stands.
    pub id: String,
    pub title: String,
    pub body: String,
    /// A smaller line under the body.
    pub note: Option<String>,
    /// The answers, left to right.
    pub buttons: Vec<String>,
    /// The button Enter presses.
    pub enter: usize,
    /// The button Esc presses.
    pub escape: usize,
}

impl Prompt {
    /// Deleting a layout `showing` charts show: Enter deletes, Esc cancels.
    #[must_use]
    pub fn layout_delete(name: &str, showing: usize) -> Self {
        let mut what =
            "Its indicator set goes with it. Drawings stay with their markets.".to_owned();
        if showing > 0 {
            what.push_str(&format!(
                " {showing} chart(s) showing it move to the layout beside it."
            ));
        }
        Self {
            id: "layout_delete_confirm".to_owned(),
            title: "Delete layout".to_owned(),
            body: format!("Delete \"{name}\"?"),
            note: Some(what),
            buttons: vec!["Delete".to_owned(), "Cancel".to_owned()],
            enter: 0,
            escape: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_delete_names_the_charts_that_move() {
        let quiet = Prompt::layout_delete("Scalp", 0);
        assert_eq!(quiet.body, "Delete \"Scalp\"?");
        assert_eq!(quiet.buttons[quiet.enter], "Delete");
        assert_eq!(quiet.buttons[quiet.escape], "Cancel");
        let shown = Prompt::layout_delete("Scalp", 2).note.unwrap();
        assert!(shown.ends_with(" 2 chart(s) showing it move to the layout beside it."));
    }
}
