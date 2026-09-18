//! A second, headless consumer executes public decisions against fallible storage.
//! Admission and IO are shell responsibilities, represented explicitly here.
use quantick_workspace::layout_commit::{LayoutCommitPolicy, LayoutSave};
use quantick_workspace::workspace_commit::{
    DocumentEdit, FrameFacts, FrameSave, WorkspaceCommitSession, WritePurpose,
};
use quantick_workspace::workspace_document::{NamedArrangement, Workspace};
use std::time::{Duration, Instant};

#[derive(Clone)]
enum Stored {
    Unknown,
    Missing,
    Admitted(Box<Workspace>),
}

struct Storage {
    held: Stored,
    fail: bool,
    trace: Vec<&'static str>,
    attempted: Vec<Workspace>,
}

impl Storage {
    fn new(held: Stored, fail: bool) -> Self {
        Self {
            held,
            fail,
            trace: vec![],
            attempted: vec![],
        }
    }
    fn read_for_edit(&mut self) -> Option<Workspace> {
        self.trace.push("read/edit");
        match &self.held {
            Stored::Unknown => None,
            Stored::Missing => Some(Workspace::default()),
            Stored::Admitted(file) => Some(*file.clone()),
        }
    }
    fn read_or_default(&mut self) -> Workspace {
        self.trace.push("read/default");
        match &self.held {
            Stored::Admitted(file) => *file.clone(),
            _ => Workspace::default(),
        }
    }
    fn write(&mut self, file: Workspace) -> Result<(), &'static str> {
        self.trace.push("write");
        self.attempted.push(file.clone());
        if self.fail {
            self.trace.push("write/error");
            return Err("injected write failure");
        }
        self.held = Stored::Admitted(Box::new(file));
        self.trace.push("write/ok");
        Ok(())
    }
    fn remove(&mut self) -> Result<(), &'static str> {
        self.trace.push("remove");
        if self.fail {
            self.trace.push("remove/error");
            return Err("injected remove failure");
        }
        self.held = Stored::Missing;
        self.trace.push("remove/ok");
        Ok(())
    }
}

#[derive(Default)]
struct Consumer {
    session: WorkspaceCommitSession,
}

impl Consumer {
    fn standing(&mut self, store: &mut Storage, edit: DocumentEdit, purpose: WritePurpose) -> bool {
        let Some(file) = self.session.prepare_edit(store.read_for_edit(), edit) else {
            return false;
        };
        let written = store.write(file).is_ok();
        self.session.note_write(purpose, written);
        written
    }
    fn full(&mut self, store: &mut Storage, captured: Workspace) -> bool {
        let written = store.write(captured).is_ok();
        self.session.note_write(WritePurpose::Full, written);
        written
    }
    fn frame(&mut self, store: &mut Storage, closing: bool) {
        match self.session.frame(FrameFacts {
            size: None,
            closing,
        }) {
            FrameSave::Wait => {}
            FrameSave::Full => {
                self.full(store, Workspace::default());
            }
            FrameSave::Inspector => {
                if let Some(file) = self
                    .session
                    .prepare_inspector(store.read_or_default(), Some([40.0, 60.0]))
                {
                    let _ = store.write(file);
                }
            }
        }
    }
    fn reset(&mut self, store: &mut Storage) -> bool {
        let kept = self.session.reset_keeps(false, false);
        let forgotten = if kept {
            let file = self.session.prepare_edit(
                store.read_for_edit(),
                DocumentEdit::ClearStartup { favorites: vec![] },
            );
            file.is_some_and(|file| store.write(file).is_ok())
        } else {
            store.remove().is_ok()
        };
        self.session.finish_reset(kept, forgotten);
        forgotten
    }
}

fn bookmark() -> NamedArrangement {
    NamedArrangement {
        name: "research".into(),
        window: Some([640.0, 480.0]),
        active_tab: 0,
        tabs: vec![],
        chrome: None,
    }
}

#[test]
fn unknown_standing_and_inspector_reads_never_attempt_a_write() {
    let mut host = Consumer::default();
    let mut store = Storage::new(Stored::Unknown, false);
    host.session.keep_bookmark(bookmark());
    assert!(!host.standing(&mut store, DocumentEdit::Bookmarks, WritePurpose::Bookmarks));
    host.session.inspector_moved();
    host.frame(&mut store, false);
    host.frame(&mut store, false);
    assert_eq!(store.trace, ["read/edit", "read/default"]);
    assert!(store.attempted.is_empty());
    assert!(matches!(store.held, Stored::Unknown));
    assert_eq!(host.session.bookmarks(), [bookmark()]);
    assert!(!host.session.inspector_pending());
    assert!(!host.session.saved());
}

#[test]
fn admitted_bookmark_write_failure_keeps_memory_and_the_original_document() {
    let mut host = Consumer::default();
    let mut original = Workspace::default();
    original.window = Some([800.0, 600.0]);
    original.replay_folder = Some("recordings".into());
    let mut store = Storage::new(Stored::Admitted(Box::new(original.clone())), true);
    host.session.set_save_on_exit(false);
    host.session.keep_bookmark(bookmark());
    assert!(!host.standing(&mut store, DocumentEdit::Bookmarks, WritePurpose::Bookmarks));
    let mut expected = original.clone();
    expected.saved = vec![bookmark()];
    expected.save_on_exit = false;
    assert_eq!(store.trace, ["read/edit", "write", "write/error"]);
    assert_eq!(store.attempted, [expected]);
    let Stored::Admitted(held) = store.held else {
        panic!("original must remain")
    };
    assert_eq!(*held, original);
    assert_eq!(host.session.bookmarks(), [bookmark()]);
    assert!(!host.session.saved());
}

#[test]
fn direct_full_overwrite_and_saved_or_law_use_actual_sink_results() {
    let mut host = Consumer::default();
    let mut store = Storage::new(Stored::Unknown, true);
    assert!(!host.full(&mut store, Workspace::default()));
    assert!(!host.session.saved());
    store.fail = false;
    assert!(host.full(&mut store, Workspace::default()));
    assert!(host.session.saved());
    store.fail = true;
    assert!(!host.full(&mut store, Workspace::default()));
    assert!(host.session.saved());
    assert_eq!(
        store.trace,
        [
            "write",
            "write/error",
            "write",
            "write/ok",
            "write",
            "write/error"
        ]
    );
}

#[test]
fn reset_directly_removes_unknown_storage_and_reports_real_remove_failure() {
    for fail in [false, true] {
        let mut host = Consumer::default();
        host.session.set_saved(true);
        let mut store = Storage::new(Stored::Unknown, fail);
        assert_eq!(host.reset(&mut store), !fail);
        assert_eq!(
            store.trace,
            if fail {
                vec!["remove", "remove/error"]
            } else {
                vec!["remove", "remove/ok"]
            }
        );
        assert_eq!(host.session.saved(), fail);
        assert!(store.attempted.is_empty());
        assert_eq!(matches!(store.held, Stored::Missing), !fail);
    }
}

#[test]
fn reset_with_kept_choices_writes_only_admitted_documents_and_keeps_saved_state() {
    for fail in [false, true] {
        let mut host = Consumer::default();
        host.session.keep_bookmark(bookmark());
        let mut original = Workspace::default();
        original.window = Some([800.0, 600.0]);
        original.active_tab = 3;
        original.replay_folder = Some("recordings".into());
        let mut store = Storage::new(Stored::Admitted(Box::new(original.clone())), fail);
        assert_eq!(host.reset(&mut store), !fail);
        let mut expected = Workspace::default();
        expected.saved = vec![bookmark()];
        expected.replay_folder = Some("recordings".into());
        assert_eq!(store.attempted, [expected.clone()]);
        assert_eq!(
            store.trace,
            if fail {
                vec!["read/edit", "write", "write/error"]
            } else {
                vec!["read/edit", "write", "write/ok"]
            }
        );
        let Stored::Admitted(held) = store.held else {
            panic!("file retained")
        };
        assert_eq!(*held, if fail { original } else { expected });
        assert!(host.session.saved());
        assert_eq!(host.session.bookmarks(), [bookmark()]);
    }
    let mut host = Consumer::default();
    host.session.keep_bookmark(bookmark());
    let mut store = Storage::new(Stored::Unknown, false);
    assert!(!host.reset(&mut store));
    assert_eq!(store.trace, ["read/edit"]);
    assert!(host.session.saved());
}

#[test]
fn failed_inspector_write_is_consumed_without_an_implicit_retry() {
    let mut host = Consumer::default();
    let original: Workspace = toml::from_str(
        "version = 1\n[chrome]\ntimezone_minutes = 0\ndock_visible = true\nrail_visible = true\nrail_dock = \"left\"\nperf_readings = false\n",
    ).unwrap();
    let mut store = Storage::new(Stored::Admitted(Box::new(original.clone())), true);
    host.session.inspector_moved();
    host.frame(&mut store, false);
    assert!(!host.session.inspector_pending());
    host.frame(&mut store, false);
    assert_eq!(store.trace, ["read/default", "write", "write/error"]);
    let mut expected = original.clone();
    expected.chrome.as_mut().unwrap().inspector_position = Some([40.0, 60.0]);
    assert_eq!(store.attempted, [expected]);
    let Stored::Admitted(held) = store.held else {
        panic!("original must remain")
    };
    assert_eq!(*held, original);
    assert!(!host.session.saved());
}

#[test]
fn failed_layout_write_and_blocked_change_are_both_consumed_before_storage() {
    let mut policy = LayoutCommitPolicy::new(false);
    let mut store = Storage::new(Stored::Missing, true);
    let now = Instant::now();
    policy.mark_changed(now);
    assert_eq!(
        policy.take_save(now + Duration::from_millis(999)),
        LayoutSave::Wait
    );
    assert!(policy.is_dirty());
    assert_eq!(
        policy.take_save(now + Duration::from_millis(1000)),
        LayoutSave::Write
    );
    assert!(!policy.is_dirty());
    assert_eq!(
        store.write(Workspace::default()),
        Err("injected write failure")
    );
    assert_eq!(policy.take_flush(), LayoutSave::Wait);
    policy.set_blocked(true);
    policy.mark_changed(now);
    assert_eq!(policy.take_flush(), LayoutSave::Blocked);
    assert!(!policy.is_dirty());
    assert_eq!(policy.take_flush(), LayoutSave::Wait);
    assert_eq!(store.trace, ["write", "write/error"]);
}
