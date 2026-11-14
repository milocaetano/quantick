//! Tabs on one workspace share one indicator set per strip layout. An edit on
//! one tab is on the panes of every other tab there on that strip layout; a
//! tab landing on a held workspace takes what its tabs show; tabs brought back
//! apart heal to the fullest set without an edit; a save from any of them
//! writes what they all show. Tabs on two workspaces never share.
use super::*;
use crate::book_upkeep::{
    TabRecipeHost, Upkeep, maintain, show_recipe, tab_holdings, take_shown_sets,
};
use crate::indicator_document::SavedKind;
use crate::workspace_book::{SaveOutcome, WorkspaceBook};
use crate::workspace_recipe::{ContextPaneRecipe, WorkspaceRecipe};
use crate::workspace_sync::TabRecipeSync;
use std::time::{Duration, Instant};

fn entry(kind: SavedKind) -> SavedIndicator {
    SavedIndicator {
        kind,
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    }
}

fn native(id: &str) -> SavedIndicator {
    entry(SavedKind::native(id))
}

fn script(name: &str) -> SavedIndicator {
    entry(SavedKind::Script {
        name: name.to_owned(),
    })
}

/// The flow pane of tab `tab` is `tab + 1`, its context pane `tab + 2`.
fn panes(tab: u64) -> [u64; 2] {
    [tab + 1, tab + 2]
}

/// What a tab whose panes show `flow` and `trend` reads as.
fn showing(flow: Vec<SavedIndicator>, trend: Vec<SavedIndicator>) -> WorkspaceRecipe {
    WorkspaceRecipe {
        split_fraction: Some(0.5),
        flow_indicators: flow,
        context_panes: vec![ContextPaneRecipe {
            indicators: trend,
            ..ContextPaneRecipe::default()
        }],
        ..WorkspaceRecipe::default()
    }
}

/// The window, cut to what the book and the layouts decide: open tabs, each a
/// flow pane on the strip layout `Force` and a context pane on `Trend`, over
/// one book, one sync and one session. A pane shows its copy's set, and an
/// edit on a pane is a write into its copy, as the window mirrors it.
struct Desk {
    book: WorkspaceBook,
    sync: TabRecipeSync,
    session: LayoutSession,
    /// The open tabs in strip order.
    tabs: Vec<u64>,
    now: Instant,
}

impl Desk {
    /// `tabs` open in strip order on `Default`, autosave off.
    fn open(tabs: &[u64]) -> Self {
        let mut session = LayoutSession::new(LayoutBook::starter(Vec::new()));
        let force = session.book().active_id();
        let trend = session.create(Some("Trend")).unwrap();
        let mut book = WorkspaceBook::default();
        book.set_autosave(false);
        for &tab in tabs {
            for (pane, layout) in panes(tab).into_iter().zip([force, trend]) {
                session.register_in(tab, pane, Some(layout));
                session.seed(pane, None, None);
            }
            book.open_tab(tab);
        }
        Self::over(book, session, tabs)
    }

    fn over(book: WorkspaceBook, session: LayoutSession, tabs: &[u64]) -> Self {
        let mut desk = Self {
            book,
            sync: TabRecipeSync::default(),
            session,
            tabs: tabs.to_vec(),
            now: Instant::now(),
        };
        desk.frames(2);
        desk
    }

    /// Tabs a desk brings back on workspace `name`, whose recipe is `stored`,
    /// in strip order, each with its own copies of `Force` and `Trend` holding
    /// the sets given: as a stored file from before the tabs shared them.
    fn restored(
        name: &str,
        stored: &WorkspaceRecipe,
        tabs: &[(u64, Vec<SavedIndicator>)],
        autosave: bool,
    ) -> Self {
        let mut layouts = LayoutBook::starter(Vec::new());
        let force = layouts.active_id();
        let trend = layouts.create(Some("Trend")).unwrap();
        let mut session_panes = Vec::new();
        for (tab, flow) in tabs {
            let copies = [force, trend].map(|layout| layouts.make_private_copy(layout).unwrap());
            layouts
                .get_mut(copies[0])
                .unwrap()
                .indicators
                .clone_from(flow);
            session_panes.extend(panes(*tab).into_iter().zip(copies));
        }
        let mut session = LayoutSession::new(layouts);
        for (pane, copy) in session_panes {
            session.register(pane, Some(copy));
            session.seed(pane, None, None);
        }
        let mut book = WorkspaceBook::default();
        book.open_tab(99);
        book.observe(99, stored).unwrap();
        book.save_as(99, name).unwrap();
        book.close_tab(99);
        book.set_autosave(autosave);
        let mut sync = TabRecipeSync::default();
        for (tab, _) in tabs {
            sync.arrive(*tab, Some(name.to_owned()), None);
        }
        let ids: Vec<u64> = tabs.iter().map(|(tab, _)| *tab).collect();
        let mut desk = Self {
            book,
            sync,
            session,
            tabs: ids,
            now: Instant::now(),
        };
        desk.frames(2);
        desk
    }

    /// Put `tabs` on a new workspace `name`, saved from the first of them; the
    /// others are switched onto it as a row does.
    fn on(&mut self, name: &str, tabs: &[u64]) {
        let first = tabs[0];
        let live = self.live_recipe(first).unwrap();
        self.book.observe(first, &live).unwrap();
        self.book.save_as(first, name).unwrap();
        for &tab in &tabs[1..] {
            self.book.set(tab, name).unwrap();
            let recipe = self.book.tab_copy(tab).cloned().unwrap();
            show_recipe(self, tab, &recipe);
        }
        self.frames(2);
    }

    /// The window's frame start: the copies follow the workspaces.
    fn share(&mut self) -> OwnedCopies {
        let holdings = tab_holdings((&self.book, &self.sync));
        self.session.hold_workspaces(holdings);
        let panes: Vec<(u64, u64)> = (self.tabs.iter())
            .flat_map(|tab| panes(*tab).map(|pane| (*tab, pane)))
            .collect();
        self.session.own_copies(panes)
    }

    /// One frame, a third of a second after the last.
    fn frame(&mut self) -> Upkeep {
        self.share();
        self.now += Duration::from_millis(350);
        maintain(self, self.now)
    }

    /// `count` frames, writing through what autosave asks; answers the tabs
    /// written, in order.
    fn frames(&mut self, count: usize) -> Vec<u64> {
        let mut written = Vec::new();
        for _ in 0..count {
            for tab in self.frame().write_through {
                self.save(tab);
                written.push(tab);
            }
        }
        written
    }

    /// Save to, as the control call does it.
    fn save(&mut self, tab: u64) -> SaveOutcome {
        take_shown_sets(&mut self.book, &self.sync, tab);
        let saved = self.book.save_to(tab).unwrap();
        for other in &saved.tabs_changed {
            let recipe = self.book.tab_copy(*other).cloned().unwrap();
            show_recipe(self, *other, &recipe);
        }
        saved
    }

    /// The set pane `pane` shows.
    fn set(&self, pane: u64) -> Vec<SavedIndicator> {
        let copy = self.session.book().get(self.session.layout(pane));
        copy.map(|layout| layout.indicators.clone())
            .unwrap_or_default()
    }

    /// An indicator added on `pane`.
    fn add(&mut self, pane: u64, indicator: SavedIndicator) {
        let copy = self.session.layout(pane);
        self.session.indicators_mut(copy).unwrap().push(indicator);
    }

    fn modified(&self, tab: u64) -> bool {
        self.book.tab_state(tab).is_some_and(|state| state.modified)
    }
}

impl TabRecipeHost for Desk {
    fn recipe_book(&self) -> (&WorkspaceBook, &TabRecipeSync) {
        (&self.book, &self.sync)
    }
    fn recipe_book_mut(&mut self) -> (&mut WorkspaceBook, &mut TabRecipeSync) {
        (&mut self.book, &mut self.sync)
    }
    fn recipe_tabs(&self) -> Vec<u64> {
        self.tabs.clone()
    }
    fn active_recipe_tab(&self) -> u64 {
        self.tabs[0]
    }
    fn tab_settled(&self, _tab: u64) -> bool {
        true
    }
    fn live_recipe(&self, tab: u64) -> Option<WorkspaceRecipe> {
        let [flow, trend] = panes(tab).map(|pane| self.set(pane));
        self.tabs.contains(&tab).then(|| showing(flow, trend))
    }
    fn recipe_tab_symbol(&self, _tab: u64) -> Option<String> {
        Some("WINZ26".to_owned())
    }
    fn recipe_tab_layout(&self, _tab: u64) -> Option<String> {
        None
    }
    fn new_tab_recipe(&self, _tab: u64) -> WorkspaceRecipe {
        WorkspaceRecipe::default()
    }
    fn recipe_signal(&self, tab: u64) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::hash::DefaultHasher::new();
        format!("{:?}", self.live_recipe(tab)).hash(&mut hash);
        hash.finish()
    }
    fn pin_tab_footprint(&mut self, _tab: u64) {}
    fn apply_recipe_canvas(&mut self, _tab: u64, _recipe: &WorkspaceRecipe) {}
    fn apply_recipe_switches(&mut self, _tab: u64, _recipe: &WorkspaceRecipe) {}
    /// What the window does: the copies follow first, then each pane the
    /// session names shows the set it names.
    fn apply_recipe_indicators(&mut self, tab: u64, recipe: &WorkspaceRecipe) {
        self.share();
        let wanted = (panes(tab).into_iter().enumerate())
            .map(|(role, pane)| (pane, recipe.for_pane(role).0));
        let shown = self.session.landing_sets(wanted);
        for (pane, set) in shown {
            let copy = self.session.layout(pane);
            *self.session.indicators_mut(copy).unwrap() = set;
        }
    }
}

/// The trader's rule across tabs: on three tabs of one workspace every pane on
/// one strip layout holds one copy, so an add or a removal on one tab's Trend
/// pane is on the other two at once, with no save.
#[test]
fn three_tabs_on_one_workspace_show_one_set_per_strip_layout() {
    let mut desk = Desk::open(&[10, 20, 30]);
    desk.on("Scalp", &[10, 20, 30]);
    let force = [11, 21, 31].map(|pane| desk.session.layout(pane));
    let trend = [12, 22, 32].map(|pane| desk.session.layout(pane));
    assert!(force.iter().all(|copy| *copy == force[0]), "{force:?}");
    assert!(trend.iter().all(|copy| *copy == trend[0]), "{trend:?}");
    assert_ne!(force[0], trend[0]);
    let stored = desk.book.recipe("Scalp").cloned();

    desk.add(22, native("native.cvd"));
    desk.add(22, native("native.ema"));
    let both = vec![native("native.cvd"), native("native.ema")];
    assert_eq!(
        [12, 22, 32].map(|pane| desk.set(pane)),
        [(); 3].map(|()| both.clone())
    );
    desk.session.indicators_mut(trend[0]).unwrap().remove(0);
    let ema = vec![native("native.ema")];
    assert_eq!(
        [12, 22, 32].map(|pane| desk.set(pane)),
        [(); 3].map(|()| ema.clone())
    );
    assert!(desk.set(11).is_empty(), "Force is another strip layout");
    assert!(desk.frames(3).is_empty(), "autosave is off");
    assert_eq!(desk.book.recipe("Scalp").cloned(), stored, "no save");
}

/// Tabs on another workspace never share, though their strip layouts are the
/// same ones by name; a tab saved under a new name stops sharing, keeping
/// what it showed.
#[test]
fn another_workspace_and_a_save_as_never_share() {
    let mut desk = Desk::open(&[10, 20, 30]);
    desk.on("Scalp", &[10, 20]);
    desk.on("Swing", &[30]);
    let origin = |desk: &Desk, pane| desk.session.book().origin_of(desk.session.layout(pane));
    assert_eq!(origin(&desk, 31), origin(&desk, 11), "one strip layout");
    assert_ne!(desk.session.layout(31), desk.session.layout(11));
    assert_eq!(desk.session.layout(21), desk.session.layout(11));
    desk.add(11, native("native.cvd"));
    assert!(desk.set(31).is_empty(), "Swing's Force pane");

    desk.book.save_as(20, "Scalp 2").unwrap();
    let owned = desk.share();
    assert!(
        owned.reshow.is_empty(),
        "it keeps what it showed: {owned:?}"
    );
    assert_ne!(desk.session.layout(21), desk.session.layout(11));
    assert_eq!(desk.set(21), [native("native.cvd")]);
    desk.add(11, native("native.ema"));
    assert_eq!(desk.set(21), [native("native.cvd")], "no longer Scalp's");
    assert_eq!(desk.set(11).len(), 2);
}

/// Scalp's tab holds an unsaved CVD on Force; Swing's tab shows two kinds
/// there. Put on Scalp, Swing's tab takes the CVD its tab shows: not the
/// stored recipe, not the fuller set it came with. The tab already there
/// keeps its set.
#[test]
fn a_tab_landing_on_a_held_workspace_takes_what_its_tabs_show() {
    let mut desk = Desk::open(&[10, 20]);
    desk.on("Scalp", &[10]);
    desk.on("Swing", &[20]);
    let shown = vec![native("native.cvd")];
    desk.add(11, native("native.cvd"));
    desk.add(21, script("force_bar.pine"));
    desk.add(21, script("cvd.pine"));
    desk.frames(2);
    let stored = desk.book.recipe("Scalp").cloned().unwrap();
    assert!(stored.flow_indicators.is_empty(), "Scalp's edit is held");

    desk.book.set(20, "Scalp").unwrap();
    show_recipe(&mut desk, 20, &stored);
    assert_eq!(desk.session.layout(21), desk.session.layout(11));
    assert_eq!(desk.set(21), shown, "what Scalp's tab shows");
    desk.frames(2);
    assert_eq!(desk.set(11), shown, "the tab already there keeps it");
    assert_eq!(desk.set(21), shown);
    assert!(
        desk.modified(10) && desk.modified(20),
        "one held edit, on both"
    );

    // Its save writes what both show, and both read clean: one edit, once.
    let saved = desk.save(20);
    assert!(saved.changed);
    assert_eq!(desk.book.recipe("Scalp").unwrap().flow_indicators, shown);
    assert!(!desk.modified(10) && !desk.modified(20));
    assert!(!desk.save(10).changed, "nothing left to save");
}

/// The defect found reproducing the report: the second tab's save overwrote
/// the first tab's Trend pane. An add on tab 10's Trend pane is on tab 20's
/// too, both read held, and tab 20's save writes it.
#[test]
fn a_save_from_the_second_tab_writes_what_both_tabs_show() {
    let mut desk = Desk::open(&[10, 20]);
    desk.on("Scalp", &[10, 20]);
    desk.add(12, native("native.ema"));
    desk.frames(3);
    assert!(desk.modified(10) && desk.modified(20));
    let saved = desk.save(20);
    assert!(saved.changed);
    let recipe = desk.book.recipe("Scalp").unwrap();
    assert_eq!(recipe.context_panes[0].indicators, [native("native.ema")]);
    assert_eq!(desk.set(12), [native("native.ema")], "tab 10's Trend pane");
    assert!(!desk.modified(10) && !desk.modified(20));
}

/// With autosave on, an edit two tabs show is one edit, written once.
#[test]
fn an_edit_two_tabs_show_is_written_once() {
    let mut desk = Desk::open(&[10, 20]);
    desk.on("Scalp", &[10, 20]);
    desk.book.set_autosave(true);
    desk.frames(4);
    desk.add(21, native("native.cvd"));
    let written = desk.frames(8);
    assert_eq!(written.len(), 1, "{written:?}");
    let recipe = desk.book.recipe("Scalp").unwrap();
    assert_eq!(recipe.flow_indicators, [native("native.cvd")]);
    assert!(desk.book.differing_tabs().is_empty());
}

/// Two tabs brought back on Scalp from a file where each held its own copy of
/// Force, with another set of as many kinds: both come back on the first
/// tab's in strip order, which is the stored recipe's, so the heal is no edit
/// and autosave writes nothing.
#[test]
fn tabs_restored_apart_tie_to_the_first_tab_and_the_heal_is_no_edit() {
    let first = vec![script("force_bar.pine"), native("native.cvd")];
    let second = vec![script("force_bar.pine"), script("cvd.pine")];
    let stored = showing(first.clone(), Vec::new());
    let tabs = [(20, first.clone()), (10, second)];
    let mut desk = Desk::restored("Scalp", &stored, &tabs, true);
    assert_eq!(desk.session.layout(11), desk.session.layout(21));
    assert_eq!([11, 21].map(|pane| desk.set(pane)), [first.clone(), first]);
    assert!(desk.frames(8).is_empty(), "autosave wrote nothing");
    assert_eq!(desk.book.recipe("Scalp"), Some(&stored));
    assert!(!desk.modified(10) && !desk.modified(20));
    assert!(desk.book.differing_tabs().is_empty());
}

/// The fuller set wins over strip order.
#[test]
fn tabs_restored_apart_heal_to_the_fullest_set() {
    let fuller = vec![script("force_bar.pine"), script("cvd.pine")];
    let stored = showing(fuller.clone(), Vec::new());
    let tabs = [(20, vec![script("force_bar.pine")]), (10, fuller.clone())];
    let desk = Desk::restored("Scalp", &stored, &tabs, false);
    assert_eq!(desk.session.layout(11), desk.session.layout(21));
    assert_eq!(
        [11, 21].map(|pane| desk.set(pane)),
        [fuller.clone(), fuller]
    );
    assert!(!desk.modified(10) && !desk.modified(20));
}
