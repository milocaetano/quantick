//! This separate test binary measures only the current thread's borrowed layout reads.
use quantick_workspace::{
    layout_document::{LayoutBook, LayoutId},
    session::LayoutSession,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
struct Counter;
fn note() {
    TRACK.with(|track| {
        if track.get() {
            ALLOCATIONS.with(|n| n.set(n.get() + 1));
        }
    });
}
// SAFETY: every operation delegates the exact pointer/layout contract to System.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller supplied a valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller supplies the pointer and original layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        note();
        // SAFETY: the caller supplies a live allocation and valid new size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
#[test]
fn idle_membership_seed_and_mirror_reads_allocate_nothing() {
    let mut session = LayoutSession::new(LayoutBook::default());
    let a = session.register(1, Some(LayoutId(1)));
    let b = session.register(2, Some(LayoutId(1)));
    session.seed(1, None, None);
    session.seed(2, None, None);
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|track| track.set(true));
    for _ in 0..1000 {
        std::hint::black_box(a.layout());
        std::hint::black_box(b.seeded());
        std::hint::black_box(session.resolve_layout(a.layout()));
        std::hint::black_box(
            session
                .matching(LayoutId(1), [(1, &a), (2, &b)].into_iter())
                .count(),
        );
        std::hint::black_box(session.seed(1, None, None));
    }
    TRACK.with(|track| track.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

/// The model's idle selection reads compose with a borrowed contiguous host
/// projection. Actual app host getters use the same direct Vec indexing/iteration;
/// this test measures the public core port, not the app's rendering work.
#[test]
fn idle_arrangement_selection_and_borrowed_order_allocate_nothing() {
    use quantick_workspace::arrangement::{ArrangementLifecycle, TabFacts, TabId, Topology};
    struct Facts(Vec<(TabId, String)>);
    impl Topology for Facts {
        fn len(&self) -> usize {
            self.0.len()
        }
        fn tab(&self, index: usize) -> Option<TabFacts<'_>> {
            self.0.get(index).map(|(id, symbol)| TabFacts {
                id: *id,
                feed: "feed",
                symbol,
            })
        }
    }
    let model = ArrangementLifecycle::new(TabId(0));
    let facts = Facts(vec![(TabId(0), "TEST".to_owned())]);
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|track| track.set(true));
    for _ in 0..1000 {
        std::hint::black_box(model.active_id());
        std::hint::black_box(facts.tab(model.active_index()));
        for entry in &facts.0 {
            std::hint::black_box((&entry.0, entry.1.as_str()));
        }
    }
    TRACK.with(|track| track.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn idle_commit_policy_and_session_reads_allocate_nothing() {
    use quantick_workspace::workspace_commit::{FrameFacts, FrameSave, WorkspaceCommitSession};
    let mut session = WorkspaceCommitSession::default();
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|track| track.set(true));
    for _ in 0..1000 {
        assert_eq!(
            session.frame(FrameFacts {
                size: Some([640.0, 480.0]),
                closing: false
            }),
            FrameSave::Wait
        );
        std::hint::black_box(session.bookmarks());
        std::hint::black_box(session.recent());
        std::hint::black_box(session.saved());
    }
    TRACK.with(|track| track.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}
