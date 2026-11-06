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
