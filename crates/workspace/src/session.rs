//! Layout-session decisions and read-only pane membership.
use crate::indicator_document::{SavedIndicator, SavedKind};
use crate::layout_document::{LayoutBook, LayoutError, LayoutId};
use std::collections::{BTreeMap, BTreeSet};
use std::{cell::Cell, rc::Rc};

#[derive(Debug, Clone, Copy, Default)]
struct Membership {
    layout: Option<LayoutId>,
    seeded: bool,
    generation: u64,
}

/// An O(1), read-only projection of the session's single membership value.
/// Like the chart consumer this session is single-threaded; it owns no runtime objects.
#[derive(Debug, Clone, Default)]
pub struct LayoutView(Option<Rc<Cell<Membership>>>);
impl LayoutView {
    #[must_use]
    pub fn layout(&self) -> Option<LayoutId> {
        self.0.as_ref().and_then(|state| state.get().layout)
    }
    #[must_use]
    pub fn seeded(&self) -> bool {
        self.0.as_ref().is_some_and(|state| state.get().seeded)
    }
}

/// Facts supplied by the chart, never a callback into its root.
#[derive(Debug, Clone, Copy, Default)]
pub struct PaneFacts {
    pub gesture_in_flight: bool,
}
impl PaneFacts {
    fn refusal(self) -> Option<LayoutError> {
        self.gesture_in_flight
            .then_some(LayoutError::GestureInFlight)
    }
}

/// Ordered host operations; membership and default changes occur at distinct boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutEffect {
    DetachIndicators,
    CommitMembership,
    MaterializeIndicators,
    RefreshLabel,
    CommitDefault,
    MarkDirty,
}
const SWAP_EFFECTS: &[LayoutEffect] = &[
    LayoutEffect::DetachIndicators,
    LayoutEffect::CommitMembership,
    LayoutEffect::MaterializeIndicators,
    LayoutEffect::RefreshLabel,
    LayoutEffect::CommitDefault,
    LayoutEffect::MarkDirty,
];
/// A validated selection. Outgoing indicators leave before this transition commits.
/// The shell can read but cannot retarget a validated selection.
/// ```compile_fail
/// use quantick_workspace::session::Selection;
/// fn retarget(selection: &mut Selection) { selection.pane = 99; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pane: u64,
    from: LayoutId,
    to: LayoutId,
    generation: u64,
}
/// A selection whose membership effect has run, but whose default has not.
#[derive(Debug, PartialEq, Eq)]
pub struct CommittedSelection {
    selection: Selection,
    generation: u64,
    /// What the pane holds now: `to`, or its tab's copy of it.
    landed: LayoutId,
}
impl Selection {
    #[must_use]
    pub fn pane(self) -> u64 {
        self.pane
    }
    #[must_use]
    pub fn from(self) -> LayoutId {
        self.from
    }
    #[must_use]
    pub fn to(self) -> LayoutId {
        self.to
    }

    #[must_use]
    pub fn changed(self) -> bool {
        self.from != self.to
    }
    #[must_use]
    pub fn effects(self) -> &'static [LayoutEffect] {
        if self.changed() {
            SWAP_EFFECTS
        } else {
            &[LayoutEffect::CommitMembership]
        }
    }
}

/// What [`LayoutSession::own_copies`] moved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnedCopies {
    /// Whether any pane moved, so the book owes the file a change.
    pub changed: bool,
    /// Panes that joined their tab's copy holding another set than they show:
    /// the host puts the copy's set on them.
    pub reshow: Vec<u64>,
}

/// The headless owner of the document and all registered pane memberships.
#[derive(Debug)]
pub struct LayoutSession {
    book: LayoutBook,
    panes: BTreeMap<u64, Rc<Cell<Membership>>>,
    next_generation: u64,
    /// The tab each pane stands in: set at [`Self::register_in`], rebuilt by
    /// [`Self::own_copies`].
    tab_of: BTreeMap<u64, u64>,
    /// Who each private copy belongs to. Not saved: a desk read back hands
    /// each copy to the holder of the tab whose pane holds it.
    owner: BTreeMap<LayoutId, Holder>,
    /// `(pane, strip layout)` to the copy of it the pane last left, kept so a
    /// switch back finds its workspace's set.
    kept: BTreeMap<(u64, LayoutId), LayoutId>,
    /// The workspace each tab is on, by its number in `workspace_ids`, as
    /// [`Self::hold_workspaces`] was last told. A tab missing is on none yet.
    workspace_of: BTreeMap<u64, u64>,
    /// Every workspace name told, numbered in the order first told: a number
    /// is never reused, so a renamed workspace is a new holder.
    workspace_ids: BTreeMap<String, u64>,
    /// The tabs whose panes stand on what their workspace showed them: what
    /// they show is what a tab landing there takes.
    settled_tabs: BTreeSet<u64>,
    /// The holder each tab left since the last [`Self::own_copies`].
    left: BTreeMap<u64, Holder>,
}

/// Who owns a private copy: the workspace its tabs are on, so every tab on
/// one workspace shows one set per strip layout, or, while a tab is on none
/// yet, the tab alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Holder {
    Tab(u64),
    Workspace(u64),
}

impl LayoutSession {
    #[must_use]
    pub fn new(book: LayoutBook) -> Self {
        Self {
            book,
            panes: BTreeMap::new(),
            next_generation: 0,
            tab_of: BTreeMap::new(),
            owner: BTreeMap::new(),
            kept: BTreeMap::new(),
            workspace_of: BTreeMap::new(),
            workspace_ids: BTreeMap::new(),
            settled_tabs: BTreeSet::new(),
            left: BTreeMap::new(),
        }
    }
    /// The holder of the copies `tab`'s panes share.
    fn holder(&self, tab: u64) -> Holder {
        self.workspace_of
            .get(&tab)
            .map_or(Holder::Tab(tab), |id| Holder::Workspace(*id))
    }
    /// Whether a pane of a tab on `holder` stands.
    fn standing(&self, holder: Holder) -> bool {
        self.tab_of.values().any(|tab| self.holder(*tab) == holder)
    }
    #[must_use]
    pub fn book(&self) -> &LayoutBook {
        &self.book
    }
    #[must_use]
    pub fn resolve_layout(&self, named: Option<LayoutId>) -> LayoutId {
        named
            .filter(|id| self.book.get(*id).is_some())
            .unwrap_or(self.book.active_id())
    }
    #[must_use]
    pub fn shows(&self, view: &LayoutView, layout: LayoutId) -> bool {
        view.layout() == Some(layout)
    }
    /// Whether `view` shows the strip layout `origin`, itself or a copy of it.
    #[must_use]
    pub fn shows_origin(&self, view: &LayoutView, origin: LayoutId) -> bool {
        view.layout()
            .is_some_and(|id| self.book.origin_of(id) == origin)
    }
    /// How many indicators `pane` shows, or would show, on strip layout `origin`.
    #[must_use]
    pub fn shown_count(&self, pane: u64, origin: LayoutId) -> usize {
        let held = self.panes.get(&pane).and_then(|state| state.get().layout);
        let shown = held
            .filter(|held| self.book.is_private(*held) && self.book.origin_of(*held) == origin)
            .unwrap_or_else(|| self.landing(pane, origin));
        self.book
            .get(shown)
            .map_or(0, |layout| layout.indicators.len())
    }
    pub fn matching<'a, T: 'a>(
        &'a self,
        layout: LayoutId,
        order: impl Iterator<Item = (T, &'a LayoutView)> + 'a,
    ) -> impl Iterator<Item = T> + 'a {
        order.filter_map(move |(target, view)| self.shows(view, layout).then_some(target))
    }
    /// Document payload access never exposes membership, names or identities.
    pub fn indicators_mut(
        &mut self,
        id: LayoutId,
    ) -> Option<&mut Vec<crate::indicator_document::SavedIndicator>> {
        self.book.get_mut(id).map(|layout| &mut layout.indicators)
    }
    /// [`Self::register`] a pane of `tab`, so a selection knows its tab at once.
    pub fn register_in(&mut self, tab: u64, pane: u64, named: Option<LayoutId>) -> LayoutView {
        self.tab_of.insert(pane, tab);
        self.register(pane, named)
    }
    /// Register once; repeated registration returns the same read-only value.
    pub fn register(&mut self, pane: u64, named: Option<LayoutId>) -> LayoutView {
        let next_generation = &mut self.next_generation;
        LayoutView(Some(
            self.panes
                .entry(pane)
                .or_insert_with(|| {
                    *next_generation = next_generation
                        .checked_add(1)
                        .expect("layout generation exhausted");
                    Rc::new(Cell::new(Membership {
                        layout: named,
                        seeded: false,
                        generation: *next_generation,
                    }))
                })
                .clone(),
        ))
    }
    /// Explicit removal tombstones every retained view. A later registration is distinct.
    pub fn remove_pane(&mut self, pane: u64) {
        if let Some(state) = self.panes.remove(&pane) {
            let mut value = state.get();
            let held = value.layout;
            value.layout = None;
            value.seeded = false;
            state.set(value);
            if let Some(held) = held {
                self.drop_if_unheld(held);
            }
        }
        let left: Vec<LayoutId> = self
            .kept
            .iter()
            .filter_map(|((owner, _), copy)| (*owner == pane).then_some(*copy))
            .collect();
        self.kept.retain(|(owner, _), _| *owner != pane);
        for copy in left {
            self.drop_if_unheld(copy);
        }
        // The last pane on its holder: the holder's copies go with it.
        if let Some(tab) = self.tab_of.remove(&pane)
            && !self.standing(self.holder(tab))
        {
            let copies: Vec<LayoutId> = self.copies_of(self.holder(tab)).collect();
            for copy in copies {
                self.drop_if_unheld(copy);
            }
        }
    }
    /// The private copies `holder` owns.
    fn copies_of(&self, holder: Holder) -> impl Iterator<Item = LayoutId> + '_ {
        self.owner
            .iter()
            .filter_map(move |(copy, owner)| (*owner == holder).then_some(*copy))
    }
    /// The copy of the strip layout `origin` that `holder` owns, if any.
    fn tab_copy(&self, holder: Holder, origin: LayoutId) -> Option<LayoutId> {
        self.copies_of(holder)
            .find(|copy| self.book.origin_of(*copy) == origin)
    }
    /// The layout `pane` lands on for the strip layout `to`: the copy of it the
    /// pane holds or last left, else its holder's, else `to` itself.
    fn landing(&self, pane: u64, to: LayoutId) -> LayoutId {
        let Some(tab) = self.tab_of.get(&pane).copied() else {
            return to;
        };
        let holder = self.holder(tab);
        let held = self.panes.get(&pane).and_then(|state| state.get().layout);
        let own = |copy: &LayoutId| self.owner.get(copy) == Some(&holder);
        held.filter(|held| own(held) && self.book.origin_of(*held) == to)
            .or_else(|| self.kept.get(&(pane, to)).copied().filter(own))
            .or_else(|| self.tab_copy(holder, to))
            .unwrap_or(to)
    }
    /// A private copy no pane holds leaves the book, unless it is the only
    /// copy of its strip layout that a standing holder owns, or the one a pane
    /// there left: those keep their set while its tabs show another layout.
    fn drop_if_unheld(&mut self, layout: LayoutId) {
        let held = self
            .panes
            .values()
            .any(|state| state.get().layout == Some(layout));
        if held || !self.book.is_private(layout) {
            return;
        }
        if let Some(holder) = self.owner.get(&layout).copied() {
            let standing = self.standing(holder);
            let origin = self.book.origin_of(layout);
            let twin = self
                .copies_of(holder)
                .any(|copy| copy != layout && self.book.origin_of(copy) == origin);
            let left = self
                .kept
                .iter()
                .any(|((pane, _), copy)| *copy == layout && self.panes.contains_key(pane));
            if standing && (!twin || left) {
                return;
            }
        }
        self.book.remove_private(layout);
        self.owner.remove(&layout);
        self.kept.retain(|_, copy| *copy != layout);
    }
    /// Drop every private copy that no pane holds and `named` does not name: a
    /// copy a desk read from disk no longer refers to. Answers whether any went.
    pub fn prune_private(&mut self, named: impl IntoIterator<Item = LayoutId>) -> bool {
        let mut keep: std::collections::BTreeSet<LayoutId> = named.into_iter().collect();
        keep.extend(self.panes.values().filter_map(|state| state.get().layout));
        let pruned = self.book.retain_private(|id| keep.contains(&id));
        let book = &self.book;
        self.owner.retain(|copy, _| book.is_private(*copy));
        self.kept.retain(|_, copy| book.is_private(*copy));
        pruned
    }
    /// The layout a seeded pane holds.
    fn held(&self, pane: u64) -> Option<LayoutId> {
        let value = self.panes.get(&pane)?.get();
        value.layout.filter(|_| value.seeded)
    }
    /// Put a pane on `copy`; the copy it leaves may go, see [`Self::drop_if_unheld`].
    fn repoint(&mut self, pane: u64, copy: LayoutId) {
        let Some(state) = self.panes.get(&pane) else {
            return;
        };
        let mut value = state.get();
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("layout generation exhausted");
        let left = value.layout.replace(copy);
        value.generation = self.next_generation;
        state.set(value);
        if let Some(left) = left.filter(|left| *left != copy) {
            self.drop_if_unheld(left);
        }
    }
    /// A new copy of `layout` for `holder`.
    fn copy_for(&mut self, holder: Holder, layout: LayoutId) -> Option<LayoutId> {
        let copy = self.book.make_private_copy(layout)?;
        self.owner.insert(copy, holder);
        Some(copy)
    }
    /// Put every seeded pane, given as `(tab, pane)` for every pane there is,
    /// in strip order and pane order, on its holder's copy of its strip
    /// layout: every pane of the tabs on one workspace on one strip layout
    /// shares that copy, so an edit mirrors across them, and no copy is ever
    /// another workspace's. A pane put back on a strip layout its holder holds
    /// a copy of rejoins that copy; twin copies merge, see
    /// [`Self::merge_twins`]. A tab that moved to another workspace takes its
    /// copy's set along where the new one has no copy yet, and the set its
    /// tabs show where it has. Slots and entries keep their positions.
    pub fn own_copies(&mut self, panes: impl IntoIterator<Item = (u64, u64)>) -> OwnedCopies {
        self.tab_of.clear();
        let mut seeded = Vec::new();
        for (tab, pane) in panes {
            self.tab_of.insert(pane, tab);
            if let Some(layout) = self.held(pane) {
                seeded.push((tab, pane, layout));
            }
        }
        let book = &self.book;
        self.owner.retain(|copy, _| book.is_private(*copy));
        for &(tab, _, layout) in &seeded {
            if self.book.is_private(layout) {
                let holder = self.holder(tab);
                self.owner.entry(layout).or_insert(holder);
            }
        }
        let brought = self.bring_copies(&seeded);
        let mut owned = self.merge_twins(&seeded);
        owned.changed |= brought;
        for (tab, pane, _) in seeded {
            let Some(layout) = self.held(pane) else {
                continue;
            };
            let (holder, from) = (self.holder(tab), self.owner.get(&layout).copied());
            if from == Some(holder) {
                continue;
            }
            let found = self.tab_copy(holder, self.book.origin_of(layout));
            // The tab left the workspace owning the copy: its set goes along.
            let moved = from.is_some() && self.left.get(&tab).copied() == from;
            // Another workspace's copy holds its own edits: cut from the strip.
            let source = if moved {
                layout
            } else {
                self.book.strip_layout(layout)
            };
            let Some(copy) = found.or_else(|| self.copy_for(holder, source)) else {
                continue;
            };
            let set = |id: LayoutId| self.book.get(id).map(|layout| &layout.indicators);
            if set(copy) != set(layout) {
                owned.reshow.push(pane);
            }
            self.repoint(pane, copy);
            owned.changed = true;
        }
        self.left.clear();
        owned
    }
    /// A tab that left a workspace no tab stands on any more takes its copies
    /// along, where its new one held no copy of that strip layout before this
    /// pass: a rename, or the first tab onto a workspace. Tabs that arrive in
    /// one pass bring twins, which [`Self::merge_twins`] joins by fullness; a
    /// tab arriving where a copy stands moves onto it instead, in
    /// [`Self::own_copies`]. Answers whether any copy changed hands.
    fn bring_copies(&mut self, seeded: &[(u64, u64, LayoutId)]) -> bool {
        let mut brought: Vec<LayoutId> = Vec::new();
        for &(tab, _, layout) in seeded {
            let holder = self.holder(tab);
            let Some(from) = self
                .owner
                .get(&layout)
                .copied()
                .filter(|from| *from != holder)
            else {
                continue;
            };
            let origin = self.book.origin_of(layout);
            let standing_copy = (self.copies_of(holder))
                .any(|copy| self.book.origin_of(copy) == origin && !brought.contains(&copy));
            if self.left.get(&tab) == Some(&from) && !self.standing(from) && !standing_copy {
                self.owner.insert(layout, holder);
                brought.push(layout);
            }
        }
        !brought.is_empty()
    }
    /// Twin copies of one strip layout that one holder owns, as a stored file
    /// can bring back, merge into one: one set per strip layout per workspace.
    /// The copy holding the most kinds of indicator survives, see
    /// [`fullness`]; a tie goes to the one the first pane in order holds, the
    /// first tab in strip order first, held copies before ones a pane left.
    /// Panes on a twin move to the survivor and are told to show its set; a
    /// pane that left a twin finds the survivor on its way back.
    /// The twins leave, each logged with what the survivor does not show.
    fn merge_twins(&mut self, seeded: &[(u64, u64, LayoutId)]) -> OwnedCopies {
        let own = |tab: u64, copy: LayoutId| {
            (self.owner.get(&copy) == Some(&self.holder(tab))).then_some(copy)
        };
        let held = seeded
            .iter()
            .filter_map(|&(tab, _, layout)| Some((tab, own(tab, layout)?)));
        let left = seeded.iter().flat_map(|&(tab, pane, _)| {
            (self.kept.iter())
                .filter(move |((by, _), _)| *by == pane)
                .filter_map(move |(_, copy)| Some((tab, own(tab, *copy)?)))
        });
        let size = |id| {
            self.book
                .get(id)
                .map_or(0, |layout| fullness(&layout.indicators))
        };
        let mut seen: Vec<((Holder, LayoutId), LayoutId)> = Vec::new();
        // The first tab seen on each, which the log names, and the winner.
        let mut best: BTreeMap<(Holder, LayoutId), (u64, LayoutId)> = BTreeMap::new();
        for (tab, copy) in held.chain(left) {
            let key = (self.holder(tab), self.book.origin_of(copy));
            if seen.contains(&(key, copy)) {
                continue;
            }
            seen.push((key, copy));
            let (_, winner) = best.entry(key).or_insert((tab, copy));
            if size(copy) > size(*winner) {
                *winner = copy;
            }
        }
        let merged: BTreeMap<LayoutId, (u64, LayoutId)> = seen
            .into_iter()
            .filter_map(|(key, copy)| {
                let (tab, survivor) = best[&key];
                (survivor != copy).then_some((copy, (tab, survivor)))
            })
            .collect();
        let mut owned = OwnedCopies::default();
        if merged.is_empty() {
            return owned;
        }
        for (twin, (tab, survivor)) in &merged {
            self.log_discarded(*tab, *twin, *survivor);
        }
        for copy in self.kept.values_mut() {
            if let Some((_, survivor)) = merged.get(copy) {
                *copy = *survivor;
            }
        }
        for &(tab, pane, layout) in seeded {
            let Some((_, survivor)) = merged.get(&layout).copied() else {
                continue;
            };
            if self.owner.get(&layout) != Some(&self.holder(tab)) {
                continue;
            }
            let set = |id| self.book.get(id).map(|layout| &layout.indicators);
            if set(layout) != set(survivor) {
                owned.reshow.push(pane);
            }
            self.repoint(pane, survivor);
        }
        for twin in merged.into_keys() {
            self.drop_if_unheld(twin);
        }
        owned.changed = true;
        owned
    }
    /// Say what a twin merged into `survivor` held that the survivor does not
    /// show: nothing a merge drops goes unsaid.
    fn log_discarded(&self, tab: u64, twin: LayoutId, survivor: LayoutId) {
        let set = |id| {
            self.book
                .get(id)
                .map_or(&[][..], |layout| &layout.indicators)
        };
        let (lacked, other_settings) = unshown(set(survivor), [set(twin)]);
        tracing::warn!(
            target: "quantick::workspace",
            schema_version = 1_u8,
            event_code = "LAYOUT_TWIN_MERGED",
            tab,
            layout = self.book.origin_of(twin).0,
            survivor = survivor.0,
            discarded = twin.0,
            lacked = ?lacked,
            other_settings = ?other_settings,
            action = "survivor_set_shown",
            "one workspace held two copies of a layout; the fuller one stays"
        );
    }
    /// Before a recipe lands on one tab's panes, given as `(pane, set)` in
    /// pane order: the set each pane shows. The panes on one copy show one
    /// set: the fullest they are given, see [`fullness`], a tie going to the
    /// first of them. A recipe never parts a copy; where its sets for one
    /// copy differ, what is not shown is logged. Call after
    /// [`Self::own_copies`], once per landing.
    #[must_use]
    pub fn shared_sets<'a>(
        &self,
        wanted: impl IntoIterator<Item = (u64, &'a [SavedIndicator])>,
    ) -> Vec<(u64, &'a [SavedIndicator])> {
        let wanted: Vec<_> = wanted
            .into_iter()
            .map(|(pane, set)| (pane, self.held(pane), set))
            .collect();
        let mut best: Vec<(LayoutId, &'a [_])> = Vec::new();
        for &(_, copy, set) in &wanted {
            let Some(copy) = copy else {
                continue;
            };
            match best.iter_mut().find(|(held, _)| *held == copy) {
                Some((_, winner)) if fullness(set) > fullness(winner) => *winner = set,
                Some(_) => {}
                None => best.push((copy, set)),
            }
        }
        let tab = wanted
            .first()
            .and_then(|(pane, _, _)| self.tab_of.get(pane));
        for &(copy, shown) in &best {
            let given = wanted
                .iter()
                .filter(|(_, held, _)| *held == Some(copy))
                .map(|&(_, _, set)| set);
            self.log_joined(tab.copied(), copy, shown, given, "fullest_set_shown");
        }
        let shown = |copy: Option<LayoutId>| {
            let copy = copy?;
            best.iter()
                .find(|(held, _)| *held == copy)
                .map(|(_, set)| *set)
        };
        wanted
            .iter()
            .map(|&(pane, copy, set)| (pane, shown(copy).unwrap_or(set)))
            .collect()
    }
    /// Tell which workspace each open tab is on, as `(tab, workspace,
    /// settled)`: a tab missing is on none yet. `settled`: its panes stand on
    /// what its workspace showed it, so a tab landing there takes their sets.
    /// The copies follow at the next [`Self::own_copies`]. An unchanged
    /// telling, every quiet frame's, compares and allocates nothing.
    pub fn hold_workspaces<'a, I>(&mut self, tabs: I)
    where
        I: IntoIterator<Item = (u64, &'a str, bool)>,
        I::IntoIter: Clone,
    {
        let tabs = tabs.into_iter();
        let (mut count, mut settled) = (0, 0);
        let same = tabs.clone().all(|(tab, name, standing)| {
            count += 1;
            settled += usize::from(standing);
            let id = self.workspace_ids.get(name);
            id.is_some()
                && self.workspace_of.get(&tab) == id
                && self.settled_tabs.contains(&tab) == standing
        });
        if same && count == self.workspace_of.len() && settled == self.settled_tabs.len() {
            return;
        }
        let (mut workspace_of, mut settled_tabs) = (BTreeMap::new(), BTreeSet::new());
        for (tab, name, standing) in tabs {
            let id = match self.workspace_ids.get(name) {
                Some(id) => *id,
                None => {
                    let id = u64::try_from(self.workspace_ids.len()).unwrap_or(u64::MAX);
                    self.workspace_ids.insert(name.to_owned(), id);
                    id
                }
            };
            workspace_of.insert(tab, id);
            if standing {
                settled_tabs.insert(tab);
            }
        }
        for (tab, was) in &self.workspace_of {
            if workspace_of.get(tab) != Some(was) {
                self.left.entry(*tab).or_insert(Holder::Workspace(*was));
            }
        }
        for tab in workspace_of.keys() {
            if !self.workspace_of.contains_key(tab) {
                self.left.entry(*tab).or_insert(Holder::Tab(*tab));
            }
        }
        self.workspace_of = workspace_of;
        self.settled_tabs = settled_tabs;
    }
    /// Each of `panes` with the set its copy holds, for the window to show:
    /// what [`OwnedCopies::reshow`] names.
    #[must_use]
    pub fn sets_of(&self, panes: impl IntoIterator<Item = u64>) -> Vec<(u64, Vec<SavedIndicator>)> {
        let set = |pane| {
            self.book
                .get(self.layout(pane))
                .map(|layout| layout.indicators.clone())
        };
        (panes.into_iter())
            .map(|pane| (pane, set(pane).unwrap_or_default()))
            .collect()
    }
    /// What a recipe landing on one tab shows, given as `(pane, set)` in pane
    /// order, the landing tab's panes alone: each pane and the set it is to
    /// show. A copy a settled tab's pane shows keeps its set: a tab landing
    /// on a workspace another tab holds takes what that tab shows, and where
    /// the recipe names another set, what is not shown is logged. Every
    /// other copy is [`Self::shared_sets`]'s, and any pane of another tab on
    /// it whose set that changes is listed too, so no pane ever shows a set
    /// its copy no longer holds. Call after [`Self::own_copies`], once per
    /// landing; sets are cloned only here.
    #[must_use]
    pub fn landing_sets<'a>(
        &self,
        wanted: impl IntoIterator<Item = (u64, &'a [SavedIndicator])>,
    ) -> Vec<(u64, Vec<SavedIndicator>)> {
        let wanted: Vec<(u64, &[SavedIndicator])> = wanted.into_iter().collect();
        let tab = wanted
            .first()
            .and_then(|(pane, _)| self.tab_of.get(pane))
            .copied();
        let set_of = |copy| {
            self.book
                .get(copy)
                .map_or(&[][..], |layout| &layout.indicators)
        };
        let elsewhere = |copy: LayoutId| {
            self.tab_of.iter().any(|(pane, other)| {
                Some(*other) != tab
                    && self.settled_tabs.contains(other)
                    && self.held(*pane) == Some(copy)
            })
        };
        let (kept, free): (Vec<_>, Vec<_>) =
            (wanted.iter().copied()).partition(|(pane, _)| self.held(*pane).is_some_and(elsewhere));
        let mut logged = Vec::new();
        for &(pane, _) in &kept {
            let Some(copy) = self.held(pane).filter(|copy| !logged.contains(copy)) else {
                continue;
            };
            logged.push(copy);
            let given = kept.iter().filter(|(by, _)| self.held(*by) == Some(copy));
            let given = given.map(|(_, set)| *set);
            self.log_joined(tab, copy, set_of(copy), given, "shown_set_kept");
        }
        let joined = self.shared_sets(free);
        let joined_on = |copy| {
            (joined.iter())
                .find(|(pane, _)| self.held(*pane) == Some(copy))
                .map(|(_, set)| *set)
        };
        let mut shown: Vec<(u64, Vec<SavedIndicator>)> = (wanted.iter())
            .map(|&(pane, set)| {
                let joined = joined
                    .iter()
                    .find(|(by, _)| *by == pane)
                    .map(|(_, set)| *set);
                let held = self.held(pane).map(set_of);
                (pane, joined.or(held).unwrap_or(set).to_vec())
            })
            .collect();
        for (pane, other) in &self.tab_of {
            let Some(copy) = self.held(*pane).filter(|_| Some(*other) != tab) else {
                continue;
            };
            if let Some(set) = joined_on(copy).filter(|set| *set != set_of(copy)) {
                shown.push((*pane, set.to_vec()));
            }
        }
        shown
    }
    /// Say what a landing recipe gave the panes on `copy` that `shown` leaves
    /// out, when it gave them more than one set.
    fn log_joined<'a>(
        &self,
        tab: Option<u64>,
        copy: LayoutId,
        shown: &[SavedIndicator],
        given: impl Iterator<Item = &'a [SavedIndicator]>,
        action: &'static str,
    ) {
        let (lacked, other_settings) = unshown(shown, given);
        if lacked.is_empty() && other_settings.is_empty() {
            return;
        }
        tracing::warn!(
            target: "quantick::workspace",
            schema_version = 1_u8,
            event_code = "LAYOUT_RECIPE_SETS_JOINED",
            tab = ?tab,
            layout = self.book.origin_of(copy).0,
            copy = copy.0,
            lacked = ?lacked,
            other_settings = ?other_settings,
            action,
            "a recipe gave the panes of one layout other sets than they show"
        );
    }
    pub fn replace_book(&mut self, book: LayoutBook) {
        let panes = std::mem::take(&mut self.panes);
        for state in panes.into_values() {
            let mut value = state.get();
            value.layout = None;
            value.seeded = false;
            state.set(value);
        }
        self.tab_of.clear();
        self.owner.clear();
        self.kept.clear();
        self.left.clear();
        self.book = book;
    }
    #[must_use]
    pub fn layout(&self, pane: u64) -> LayoutId {
        self.panes
            .get(&pane)
            .and_then(|state| state.get().layout)
            .filter(|id| self.book.get(*id).is_some())
            .unwrap_or(self.book.active_id())
    }
    /// Seed precedence is named, own tab's seeded flow, focused seeded pane, default.
    pub fn seed(&mut self, pane: u64, flow: Option<u64>, focused: Option<u64>) -> Option<LayoutId> {
        let state = self.panes.get(&pane)?;
        if state.get().seeded {
            return None;
        }
        let valid = |id| self.book.get(id).is_some();
        let seeded_layout = |id: u64| {
            self.panes
                .get(&id)
                .map(|state| state.get())
                .filter(|state| state.seeded)
                .and_then(|state| state.layout)
                .filter(|id| valid(*id))
        };
        let layout = state
            .get()
            .layout
            .filter(|id| valid(*id))
            .or_else(|| flow.and_then(seeded_layout))
            .or_else(|| focused.and_then(seeded_layout))
            .unwrap_or(self.book.active_id());
        let mut value = state.get();
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("layout generation exhausted");
        value.generation = self.next_generation;
        value.layout = Some(layout);
        value.seeded = true;
        state.set(value);
        Some(layout)
    }
    pub fn select(
        &self,
        pane: u64,
        to: LayoutId,
        facts: PaneFacts,
    ) -> Result<Selection, LayoutError> {
        let state = self.panes.get(&pane).ok_or(LayoutError::Unknown)?.get();
        self.book.get(to).ok_or(LayoutError::Unknown)?;
        let holder = self.tab_of.get(&pane).map(|tab| self.holder(*tab));
        if self.book.is_private(to)
            && state.layout != Some(to)
            && self.owner.get(&to) != holder.as_ref()
        {
            // A copy is named by no strip row; another workspace's would carry
            // this pane's edits to that workspace.
            return Err(LayoutError::Unknown);
        }
        // A strip layout the pane's tab holds a copy of lands on that copy.
        let to = self.landing(pane, to);
        let from = self.layout(pane);
        if from != to
            && let Some(error) = facts.refusal()
        {
            return Err(error);
        }
        Ok(Selection {
            pane,
            from,
            to,
            generation: state.generation,
        })
    }
    /// Commit at the named membership effect, after outgoing slots leave.
    pub fn commit_selection(
        &mut self,
        selection: Selection,
    ) -> Result<CommittedSelection, LayoutError> {
        self.book.get(selection.to).ok_or(LayoutError::Unknown)?;
        let state = self
            .panes
            .get(&selection.pane)
            .ok_or(LayoutError::Unknown)?;
        let mut value = state.get();
        if value.generation != selection.generation || self.layout(selection.pane) != selection.from
        {
            return Err(LayoutError::Unknown);
        }
        let holder = self
            .tab_of
            .get(&selection.pane)
            .map(|tab| self.holder(*tab));
        // A pane of a known tab never holds a strip layout another tab reads.
        let landed = match holder {
            Some(holder) if !self.book.is_private(selection.to) => {
                self.copy_for(holder, selection.to).unwrap_or(selection.to)
            }
            _ => selection.to,
        };
        let Some(state) = self.panes.get(&selection.pane) else {
            return Err(LayoutError::Unknown);
        };
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("layout generation exhausted");
        value.generation = self.next_generation;
        value.layout = Some(landed);
        if selection.changed() {
            value.seeded = true;
        }
        state.set(value);
        let generation = value.generation;
        if selection.changed() {
            let from = selection.from;
            if holder.is_some() && holder == self.owner.get(&from).copied() {
                self.kept
                    .insert((selection.pane, self.book.origin_of(from)), from);
            }
            self.drop_if_unheld(from);
        }
        Ok(CommittedSelection {
            selection,
            generation,
            landed,
        })
    }
    /// Last-pick default changes only after incoming indicators are restored.
    pub fn finish_selection(&mut self, committed: CommittedSelection) -> Result<(), LayoutError> {
        let selection = committed.selection;
        let state = self
            .panes
            .get(&selection.pane)
            .ok_or(LayoutError::Unknown)?
            .get();
        if state.generation != committed.generation || state.layout != Some(committed.landed) {
            return Err(LayoutError::Unknown);
        }
        self.book.get(selection.to).ok_or(LayoutError::Unknown)?;
        if selection.changed() {
            self.book.switch(self.book.origin_of(selection.to))?;
        }
        Ok(())
    }
    pub fn create(&mut self, name: Option<&str>) -> Result<LayoutId, LayoutError> {
        self.book.create(name)
    }
    pub fn create_for(
        &mut self,
        pane: u64,
        name: Option<&str>,
        facts: PaneFacts,
    ) -> Result<LayoutId, LayoutError> {
        if !self.panes.contains_key(&pane) {
            return Err(LayoutError::Unknown);
        }
        if let Some(error) = facts.refusal() {
            return Err(error);
        }
        self.book.create(name)
    }
    pub fn rename(&mut self, id: LayoutId, name: &str) -> Result<bool, LayoutError> {
        self.book.rename(id, name)
    }
    /// The supplied order is physical pane order; the session alone selects membership targets.
    pub fn targets<'a>(
        &'a self,
        id: LayoutId,
        order: impl Iterator<Item = u64> + 'a,
    ) -> impl Iterator<Item = u64> + 'a {
        order.filter(move |pane| {
            self.panes
                .get(pane)
                .is_some_and(|state| state.get().layout == Some(id))
        })
    }
    pub fn plan_delete(
        &self,
        id: LayoutId,
        ordered_facts: impl Iterator<Item = (u64, PaneFacts)>,
    ) -> Result<Vec<Selection>, LayoutError> {
        let index = self.book.index_of(id).ok_or(LayoutError::Unknown)?;
        if self.book.layouts().len() == 1 {
            return Err(LayoutError::Last);
        }
        let to = self
            .book
            .at(index.saturating_sub(1))
            .filter(|layout| layout.id != id)
            .or_else(|| self.book.at(index + 1))
            .ok_or(LayoutError::Last)?
            .id;
        let mut selections = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let on = |state: &Rc<Cell<Membership>>| {
            state
                .get()
                .layout
                .is_some_and(|layout| self.book.origin_of(layout) == id)
        };
        for (pane, facts) in ordered_facts {
            if self.panes.get(&pane).is_some_and(on) {
                if !seen.insert(pane) {
                    return Err(LayoutError::Unknown);
                }
                if let Some(error) = facts.refusal() {
                    return Err(error);
                }
                selections.push(self.select(pane, to, facts)?);
            }
        }
        if seen.len() != self.panes.values().filter(|state| on(state)).count() {
            return Err(LayoutError::Unknown);
        }
        Ok(selections)
    }
    /// Delete strip layout `id` once no pane shows it, and the copies of it
    /// the tabs own with it.
    pub fn finish_delete(&mut self, id: LayoutId) -> Result<(), LayoutError> {
        let book = &self.book;
        if self.panes.values().any(|state| {
            state
                .get()
                .layout
                .is_some_and(|layout| book.origin_of(layout) == id)
        }) {
            return Err(LayoutError::Unknown);
        }
        self.book.delete(id)?;
        let copies: Vec<LayoutId> = self
            .owner
            .keys()
            .copied()
            .filter(|copy| self.book.origin_of(*copy) == id)
            .collect();
        for copy in copies {
            self.book.remove_private(copy);
            self.owner.remove(&copy);
            self.kept.retain(|_, kept| *kept != copy);
        }
        Ok(())
    }
}

/// How full a set is: the kinds of indicator it shows. Three CVDs are one
/// kind, so they never outweigh a bar study and a CVD.
fn fullness(set: &[SavedIndicator]) -> usize {
    let mut kinds: Vec<&SavedKind> = Vec::new();
    for entry in set {
        if !kinds.contains(&&entry.kind) {
            kinds.push(&entry.kind);
        }
    }
    kinds.len()
}

/// What `others` hold that `shown` does not show, by stable name, once each:
/// the kinds it lacks, and the kinds it has whose inputs, style or visibility
/// differ from every one of its entries of that kind.
fn unshown<'a>(
    shown: &[SavedIndicator],
    others: impl IntoIterator<Item = &'a [SavedIndicator]>,
) -> (Vec<&'a str>, Vec<&'a str>) {
    let (mut lacked, mut other_settings) = (Vec::new(), Vec::new());
    for entry in others.into_iter().flatten() {
        if shown.contains(entry) {
            continue;
        }
        let name = entry.kind.stable_name();
        let list = if shown.iter().any(|kept| kept.kind == entry.kind) {
            &mut other_settings
        } else {
            &mut lacked
        };
        if !list.contains(&name) {
            list.push(name);
        }
    }
    (lacked, other_settings)
}

#[cfg(test)]
mod shared_tests;
#[cfg(test)]
mod tests;
