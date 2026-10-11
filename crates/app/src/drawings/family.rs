//! The vocabulary the tool registry speaks that names an egui key. The rest
//! of it — rail families, icons, the two gestures every tool answers — is
//! headless, in `quantick_drawings`.

use eframe::egui;

/// A tool's arming shortcut, declared by the tool itself so the keyboard
/// map never becomes a central match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolShortcut {
    pub key: egui::Key,
    pub shift: bool,
}
