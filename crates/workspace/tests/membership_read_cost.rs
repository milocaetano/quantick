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

/// Twelve layouts, the book's cap: the first eleven hold `others` panes each
/// and the last, the one a delete removes, holds `members`. Returns the
/// session, every pane with its retained view in order, and the last layout.
fn full_book(
    others: u64,
    members: u64,
) -> (
    LayoutSession,
    Vec<(u64, quantick_workspace::session::LayoutView)>,
    LayoutId,
) {
    use quantick_workspace::layout_document::MAX_LAYOUTS;
    let mut session = LayoutSession::new(LayoutBook::default());
    let mut layouts = vec![LayoutId(1)];
    while layouts.len() < MAX_LAYOUTS {
        layouts.push(session.create(None).unwrap());
    }
    let last = *layouts.last().unwrap();
    let mut views = Vec::new();
    for (index, layout) in layouts.iter().enumerate() {
        let count = if *layout == last { members } else { others };
        for slot in 0..count {
            let pane = index as u64 * 1_000 + slot;
            views.push((pane, session.register(pane, Some(*layout))));
        }
    }
    (session, views, last)
}

/// Allocations `work` makes on this thread.
fn allocations(work: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|track| track.set(true));
    work();
    TRACK.with(|track| track.set(false));
    ALLOCATIONS.with(Cell::get)
}

/// Planning a delete of the last layout, over every registered pane: the
/// number of selections and the allocations the plan made.
fn delete_cost(others: u64, members: u64) -> (usize, usize) {
    use quantick_workspace::session::PaneFacts;
    let (session, views, last) = full_book(others, members);
    let facts: Vec<(u64, PaneFacts)> = views
        .iter()
        .map(|(pane, _)| (*pane, PaneFacts::default()))
        .collect();
    let mut planned = 0;
    let cost = allocations(|| {
        planned = session.plan_delete(last, facts.into_iter()).unwrap().len();
    });
    (planned, cost)
}

/// #526 A4 at the book's cap: mirror reads walk every pane of twelve layouts
/// without allocating, and planning a delete allocates for the deleted
/// layout's own members only, the same count whether the other eleven
/// layouts hold eight panes each or eighty.
#[test]
fn twelve_layout_reads_allocate_nothing_and_delete_scales_with_its_members() {
    let (session, views, _) = full_book(8, 8);
    let order: Vec<u64> = views.iter().map(|(pane, _)| *pane).collect();
    let reads = allocations(|| {
        for _ in 0..100 {
            for layout in 1..=12 {
                let id = LayoutId(layout);
                std::hint::black_box(session.targets(id, order.iter().copied()).count());
                std::hint::black_box(
                    session
                        .matching(id, views.iter().map(|(pane, view)| (*pane, view)))
                        .count(),
                );
            }
        }
    });
    assert_eq!(reads, 0, "targets and matching over 12 layouts x 8 panes");

    let (planned, narrow) = delete_cost(8, 8);
    assert_eq!(planned, 8, "one selection per member of the deleted layout");
    let (planned, wide) = delete_cost(80, 8);
    assert_eq!(planned, 8);
    assert_eq!(
        narrow, wide,
        "ten times the other panes, the same allocating work"
    );
}
