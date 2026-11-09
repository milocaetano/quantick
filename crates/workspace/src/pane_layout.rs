//! Pane-local layout binding and persistence bookkeeping; session owns membership.
use crate::layout_document::{LayoutId, SavedDrawing};
use crate::market_settings::{DrawingKey, PaneRole};
use crate::session::LayoutView;

/// `Bars` is the host's bar rule, compared and never read here.
pub struct PaneLayout<Bars = ()> {
    pub view: LayoutView,
    pub opening: Option<Option<LayoutId>>,
    pub label: String,
    pub drawings_key: Option<DrawingKey>,
    /// The feed and symbol `drawings_key` was resolved from, so a frame can
    /// tell a tab moved without resolving the asset again.
    pub drawings_source: Option<(String, String)>,
    /// The bars `drawings_key` was resolved at: a frame compares this instead
    /// of formatting a role, and a change re-keys a context chart.
    pub drawings_spec: Option<Bars>,
    pub saved_revision: u64,
    /// The market's all-timeframes marks as this pane last loaded them: what
    /// its next write is merged against.
    pub shared_base: Vec<SavedDrawing>,
    /// A cut `recut_in_place` already found it could not take in place, so a
    /// chart held at its old bars is not asked again every frame.
    pub recut_checked: Option<Bars>,
}
impl<Bars> Default for PaneLayout<Bars> {
    fn default() -> Self {
        Self {
            view: LayoutView::default(),
            opening: None,
            label: String::new(),
            drawings_key: None,
            drawings_source: None,
            drawings_spec: None,
            saved_revision: 0,
            shared_base: Vec::new(),
            recut_checked: None,
        }
    }
}
impl<Bars> PaneLayout<Bars> {
    pub fn layout_id(&self) -> Option<LayoutId> {
        self.opening.unwrap_or_else(|| self.view.layout())
    }
    pub fn request_opening(&mut self, id: Option<LayoutId>) {
        self.opening = Some(id);
    }
    pub fn seeded(&self) -> bool {
        self.view.seeded()
    }
    /// Whether the held drawings no longer belong on this pane: the tab left
    /// the feed and symbol they were resolved from, or a context chart
    /// (`context_bars`) left their bars. Compares; allocates nothing.
    pub fn needs_rekey(&self, feed: &str, symbol: &str, context_bars: Option<&Bars>) -> bool
    where
        Bars: PartialEq,
    {
        let moved = self
            .drawings_source
            .as_ref()
            .is_some_and(|held| held.0 != feed || held.1 != symbol);
        let recut = context_bars.is_some_and(|bars| self.drawings_spec.as_ref() != Some(bars));
        self.seeded() && self.drawings_key.is_some() && (moved || recut)
    }
    /// The flow chart cut to `now` keeps its key. When `alike` finds no mark
    /// pinned to either cut, only the held bars move: nothing is rebuilt, so
    /// selection and undo survive. Answers whether they moved.
    pub fn recut_in_place(
        &mut self,
        now: Bars,
        alike: impl FnOnce(&DrawingKey, &Bars, &Bars) -> bool,
    ) -> bool
    where
        Bars: PartialEq + Copy,
    {
        let (Some(key), Some(held)) = (&self.drawings_key, self.drawings_spec) else {
            return false;
        };
        if held == now || key.role != PaneRole::Flow || self.recut_checked == Some(now) {
            return false;
        }
        let moved = alike(key, &held, &now);
        if moved {
            self.drawings_spec = Some(now);
        }
        self.recut_checked = (!moved).then_some(now);
        moved
    }
    pub fn drawings_dirty(&self, revision: u64) -> bool {
        self.drawings_key.is_some() && revision != self.saved_revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_default_is_distinct_from_no_request() {
        let mut pane = PaneLayout::<()>::default();
        pane.request_opening(Some(LayoutId(9)));
        assert_eq!(pane.layout_id(), Some(LayoutId(9)));
        pane.request_opening(None);
        assert_eq!(pane.opening, Some(None));
        assert_eq!(pane.layout_id(), None);
        assert!(!pane.seeded());
        assert!(!pane.drawings_dirty(3));
    }

    #[test]
    fn only_the_flow_chart_moves_its_bars_in_place_and_only_when_alike() {
        let keyed = |role| PaneLayout::<u32> {
            drawings_key: Some(DrawingKey {
                market: crate::market_settings::MarketKey {
                    venue: "v".to_owned(),
                    asset: "a".to_owned(),
                },
                role,
            }),
            drawings_spec: Some(1),
            ..PaneLayout::default()
        };
        let mut flow = keyed(PaneRole::Flow);
        assert!(!flow.recut_in_place(2, 0, |_, _, _| false), "a pinned mark");
        assert_eq!(flow.drawings_spec, Some(1));
        assert!(flow.recut_in_place(3, 0, |_, held, now| (*held, *now) == (1, 3)));
        assert_eq!(flow.drawings_spec, Some(3));
        assert!(!flow.recut_in_place(3, 0, |_, _, _| true), "nothing moved");

        let bars = "time:1m".to_owned();
        let mut context = keyed(PaneRole::Context { bars });
        assert!(
            !context.recut_in_place(2, 0, |_, _, _| true),
            "a context chart's key names its bars"
        );
    }

    /// A chart held at its old bars (a strategy keeps it) asks the store once
    /// per cut and store revision, not every frame: the answer costs a
    /// formatted rule, and only a write to the store can change it.
    #[test]
    fn a_recut_that_cannot_move_in_place_is_asked_once() {
        let mut flow = PaneLayout::<u32> {
            drawings_key: Some(DrawingKey::of_pane("v", "a".to_owned(), None)),
            drawings_spec: Some(1),
            ..PaneLayout::default()
        };
        let mut asked = 0;
        for _ in 0..3 {
            assert!(!flow.recut_in_place(2, 7, |_, _, _| {
                asked += 1;
                false
            }));
        }
        assert_eq!(asked, 1);
        assert!(
            flow.recut_in_place(2, 8, |_, _, _| true),
            "the store moved: asked again"
        );
        assert!(flow.recut_in_place(3, 8, |_, _, _| true), "a new cut is asked");
    }

    #[test]
    fn an_unkeyed_pane_never_asks_for_a_new_key() {
        let pane = PaneLayout::<u32> {
            drawings_source: Some(("f".to_owned(), "s".to_owned())),
            drawings_spec: Some(1),
            ..PaneLayout::default()
        };
        assert!(
            !pane.needs_rekey("other", "s", Some(&2)),
            "not seeded, no key"
        );
    }
}
