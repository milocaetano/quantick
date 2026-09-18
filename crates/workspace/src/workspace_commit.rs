//! Workspace persistence model. Inputs describe user commands and admitted documents;
//! the caller owns file reads, diagnostics, writes, and immediate result delivery.
use crate::workspace_document::{NamedArrangement, Workspace};

pub(crate) const MAX_RECENT: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameSave {
    Wait,
    Full,
    Inspector,
}

#[derive(Clone, Copy, Debug)]
pub struct FrameFacts {
    pub size: Option<[f32; 2]>,
    pub closing: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum WritePurpose {
    Full,
    Bookmarks,
    ReplayPreference,
    Favorites,
    Inspector,
}

/// A standing edit never captures the current arrangement.
pub enum DocumentEdit {
    Bookmarks,
    ReplayFolder(Option<String>),
    ReplayDayBefore(bool),
    Favorites(Vec<String>),
    ClearStartup { favorites: Vec<String> },
}

/// Private authoritative session state; no collection or flag has a mutable view.
///
/// ```compile_fail
/// use quantick_workspace::workspace_commit::WorkspaceCommitSession;
/// let mut session = WorkspaceCommitSession::default();
/// session.bookmarks().clear();
/// ```
pub struct WorkspaceCommitSession {
    save_on_exit: bool,
    favorites_are_staged: bool,
    bookmarks: Vec<NamedArrangement>,
    saved: bool,
    recent: Vec<String>,
    window_size: Option<[f32; 2]>,
    inspector_dirty: bool,
}

impl Default for WorkspaceCommitSession {
    fn default() -> Self {
        Self {
            save_on_exit: true,
            favorites_are_staged: false,
            bookmarks: vec![],
            saved: false,
            recent: vec![],
            window_size: None,
            inspector_dirty: false,
        }
    }
}

impl WorkspaceCommitSession {
    pub fn save_on_exit(&self) -> bool {
        self.save_on_exit
    }
    pub fn set_save_on_exit(&mut self, enabled: bool) {
        self.save_on_exit = enabled;
    }
    pub fn favorites_are_staged(&self) -> bool {
        self.favorites_are_staged
    }
    pub fn stage_favorites(&mut self) {
        self.favorites_are_staged = true;
    }
    pub fn bookmarks(&self) -> &[NamedArrangement] {
        &self.bookmarks
    }
    pub fn saved(&self) -> bool {
        self.saved
    }
    /// Startup supplies actual existence, including a file with no saved tabs.
    pub fn set_saved(&mut self, saved: bool) {
        self.saved = saved;
    }
    pub fn recent(&self) -> &[String] {
        &self.recent
    }
    pub fn window_size(&self) -> Option<[f32; 2]> {
        self.window_size
    }
    pub fn inspector_pending(&self) -> bool {
        self.inspector_dirty
    }
    pub fn inspector_moved(&mut self) {
        self.inspector_dirty = true;
    }

    /// Adoption keeps the incoming list's exact order and duplicates until a visit.
    pub fn adopt(
        &mut self,
        save_on_exit: bool,
        bookmarks: Vec<NamedArrangement>,
        recent: Vec<String>,
    ) {
        self.save_on_exit = save_on_exit;
        self.bookmarks = bookmarks;
        self.recent = recent;
    }
    pub fn adopt_recent(&mut self, recent: Vec<String>) {
        self.recent = recent;
    }
    pub fn visit(&mut self, entry: String) {
        self.recent.retain(|held| *held != entry);
        self.recent.insert(0, entry);
        self.recent.truncate(MAX_RECENT);
    }
    /// Names are validated by the caller before capturing the live arrangement.
    /// The first exact-name match is replaced; deletion removes every match.
    pub fn keep_bookmark(&mut self, entry: NamedArrangement) -> bool {
        if let Some(held) = self
            .bookmarks
            .iter_mut()
            .find(|held| held.name == entry.name)
        {
            *held = entry;
            true
        } else {
            self.bookmarks.push(entry);
            false
        }
    }
    pub fn delete_bookmark(&mut self, name: &str) -> bool {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|held| held.name != name);
        self.bookmarks.len() != before
    }
    pub fn note_write(&mut self, purpose: WritePurpose, written: bool) {
        if matches!(
            purpose,
            WritePurpose::Full | WritePurpose::Bookmarks | WritePurpose::ReplayPreference
        ) {
            self.saved |= written;
        }
    }
    pub fn reset_keeps(&self, has_favorites: bool, has_stored_replay_pick: bool) -> bool {
        !self.bookmarks.is_empty()
            || has_favorites
            || !self.recent.is_empty()
            || has_stored_replay_pick
    }
    pub fn finish_reset(&mut self, kept: bool, forgotten: bool) {
        self.saved = if forgotten { kept } else { true };
    }
    /// Per frame: scalar work only. Even disabled autosave consumes inspector dirt.
    pub fn frame(&mut self, facts: FrameFacts) -> FrameSave {
        if let Some(size) = facts.size
            && size[0] > 0.0
            && size[1] > 0.0
        {
            self.window_size = Some(size);
        }
        if facts.closing && self.save_on_exit {
            self.inspector_dirty = false;
            FrameSave::Full
        } else if std::mem::take(&mut self.inspector_dirty) && self.save_on_exit {
            FrameSave::Inspector
        } else {
            FrameSave::Wait
        }
    }
    /// `None` is a refused read-swap-write, never an invitation to rebuild defaults.
    /// The shell's distinct startup/inspector reader is intentionally not used here.
    pub fn prepare_edit(
        &self,
        admitted: Option<Workspace>,
        edit: DocumentEdit,
    ) -> Option<Workspace> {
        let mut file = admitted?;
        file.save_on_exit = self.save_on_exit;
        match edit {
            DocumentEdit::Bookmarks => file.saved = self.bookmarks.clone(),
            DocumentEdit::ReplayFolder(folder) => file.replay_folder = folder,
            DocumentEdit::ReplayDayBefore(enabled) => file.replay_day_before = Some(enabled),
            DocumentEdit::Favorites(tools) => file.favorite_tools = tools,
            DocumentEdit::ClearStartup { favorites } => {
                file.tabs.clear();
                file.chrome = None;
                file.window = None;
                file.active_tab = 0;
                file.saved = self.bookmarks.clone();
                file.favorite_tools = favorites;
            }
        }
        Some(file)
    }
    /// Inspector writes use the shell's default-opening reader, not standing admission.
    pub fn prepare_inspector(
        &self,
        mut file: Workspace,
        position: Option<[f32; 2]>,
    ) -> Option<Workspace> {
        let chrome = file.chrome.as_mut()?;
        if chrome.inspector_position == position {
            return None;
        }
        chrome.inspector_position = position;
        Some(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revisiting_a_file_moves_it_up_instead_of_duplicating_it() {
        let mut session = WorkspaceCommitSession::default();
        session.visit("D:/a.qws.toml".into());
        session.visit("D:/b.qws.toml".into());
        session.visit("D:/a.qws.toml".into());
        assert_eq!(session.recent(), vec!["D:/a.qws.toml", "D:/b.qws.toml"]);
    }

    /// A menu, not a history.
    #[test]
    fn the_recent_list_stays_a_menu() {
        let mut session = WorkspaceCommitSession::default();
        for index in 0..MAX_RECENT + 5 {
            session.visit(format!("D:/w{index}.qws.toml"));
        }
        assert_eq!(session.recent().len(), MAX_RECENT);
        assert_eq!(
            session.recent()[0],
            format!("D:/w{}.qws.toml", MAX_RECENT + 4)
        );
    }
}
