//! Pane-local layout binding and persistence bookkeeping; session owns membership.
use crate::layout_document::{LayoutId, SavedDrawing};
use crate::market_settings::{DrawingKey, PaneRole, PaneWrite};
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
    /// A cut `recut_in_place` already found it could not take in place, and
    /// the store revision it asked at: a chart held at its old bars is not
    /// asked again every frame, and is once the store moves.
    pub recut_checked: Option<(Bars, u64)>,
    /// What another pane's write changed that this one has not taken yet: it
    /// was busy, or held an edit not yet written.
    pub behind: PaneWrite,
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
            behind: PaneWrite::default(),
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
    /// selection and undo survive. Answers whether they moved. A refusal is
    /// asked again only for another cut or store `revision`.
    pub fn recut_in_place(
        &mut self,
        now: Bars,
        revision: u64,
        alike: impl FnOnce(&DrawingKey, &Bars, &Bars) -> bool,
    ) -> bool
    where
        Bars: PartialEq + Copy,
    {
        let (Some(key), Some(held)) = (&self.drawings_key, self.drawings_spec) else {
            return false;
        };
        let asked = self.recut_checked == Some((now, revision));
        if held == now || key.role != PaneRole::Flow || asked {
            return false;
        }
        let moved = alike(key, &held, &now);
        if moved {
            self.drawings_spec = Some(now);
        }
        self.recut_checked = (!moved).then_some((now, revision));
        moved
    }
    /// Whether another pane's write waits on this one: see [`Self::behind`].
    pub fn is_behind(&self) -> bool {
        self.behind.changed()
    }
    pub fn drawings_dirty(&self, revision: u64) -> bool {
        self.drawings_key.is_some() && revision != self.saved_revision
    }
    /// Whether the held drawings were resolved from `feed` at one of `symbols`.
    #[must_use]
    pub fn holds_marks_of(&self, feed: &str, symbols: [&str; 2]) -> bool {
        let source = self.drawings_source.as_ref();
        source.is_some_and(|(venue, symbol)| venue == feed && symbols.contains(&symbol.as_str()))
    }
}

#[cfg(test)]
#[path = "pane_layout_tests.rs"]
mod tests;
