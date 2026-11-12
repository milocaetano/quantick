//! Presentation lifecycle and semantic input over the authoritative workspace palette.
use quantick_control_handlers::market_palette::default_bar_size;
use quantick_workspace::market_list::{
    ClosedMarket, MarketListing, MarketPalette, MarketSnapshot, PaletteFocus, PaletteIntent,
    PaletteKey, RecentMarkets, SwitchTarget,
};

#[derive(Default)]
pub struct State {
    pub open: Option<MarketPalette>,
    pub recent: RecentMarkets,
    pub listing: MarketListing,
    pub snapshot: MarketSnapshot,
    pub request_focus: bool,
    pending: Option<PaletteIntent>,
}
impl State {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
    /// Projection used only while the palette is open.
    pub fn palette(&self) -> &MarketPalette {
        self.open.as_ref().expect("open palette during paint")
    }
    pub fn bar_parts(&self) -> ((String, String), bool) {
        (
            self.palette().bar_parts(&self.listing),
            self.palette().bar_changed(),
        )
    }
    pub fn workspace(&self) -> (String, bool) {
        (
            self.palette().workspace().to_owned(),
            self.palette().workspace_changed(),
        )
    }
    pub fn offers_background(&self) -> bool {
        !self.palette().commands_mode() && self.palette().switch_target().is_none()
    }
    pub fn primary_label(&self) -> String {
        self.palette().primary_label(&self.listing)
    }
    pub fn selected(&self, row: &quantick_workspace::market_list::MarketRow) -> bool {
        self.palette().is_selected(&self.listing, row)
    }
    fn refresh(&mut self) {
        self.listing = self
            .open
            .as_ref()
            .map(|p| p.listing(&self.snapshot.world()))
            .unwrap_or_default();
    }
}

pub enum Intent {
    Open {
        query: String,
        switching: Option<SwitchTarget>,
    },
    Close,
    Refuse(String),
    Remember(ClosedMarket),
    Refresh(Box<MarketSnapshot>),
    Query(String),
    Keys(Vec<PaletteKey>),
    Row {
        index: usize,
        submit: bool,
    },
    Focus {
        stop: PaletteFocus,
        acquired: bool,
        idle: bool,
    },
    BarKind(String),
    BarSize(String),
    Workspace(String),
    Finish {
        escape: bool,
        scrim_clicked: bool,
        dropdown_open: bool,
    },
}
impl Intent {
    pub fn open(query: impl Into<String>, switching: Option<SwitchTarget>) -> Self {
        Self::Open {
            query: query.into(),
            switching,
        }
    }
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Option<PaletteIntent>;
    fn update(state: &mut State, intent: Intent) -> Self::Effects {
        match intent {
            Intent::Open { query, switching } => {
                state.open = Some(MarketPalette::opened(&query, switching));
                state.pending = None;
                state.refresh();
            }
            Intent::Close => {
                state.open = None;
                state.pending = None;
                state.listing = MarketListing::default();
            }
            Intent::Refuse(reason) => {
                if let Some(p) = state.open.as_mut() {
                    p.refuse(reason);
                }
            }
            Intent::Remember(closed) => state.recent.closed(closed),
            Intent::Refresh(snapshot) => {
                if state.snapshot != *snapshot {
                    state.snapshot = *snapshot;
                    state.refresh();
                }
            }
            Intent::Query(query) => {
                if let Some(p) = state.open.as_mut()
                    && p.query() != query
                {
                    p.set_query(&query);
                    state.refresh();
                }
            }
            Intent::Keys(keys) => {
                if let Some(p) = state.open.as_mut() {
                    for key in keys {
                        let effect = p.press(
                            key,
                            &state.listing,
                            &state.snapshot.workspaces,
                            default_bar_size,
                        );
                        state.pending = state.pending.take().or(effect);
                    }
                }
            }
            Intent::Row { index, submit } => {
                if let Some(p) = state.open.as_mut()
                    && let Some(row) = state
                        .listing
                        .rows()
                        .nth(index)
                        .filter(|row| row.selectable())
                {
                    p.select(&state.listing, row);
                    if submit {
                        let effect = p.submit(&state.listing, false);
                        state.pending = state.pending.take().or(effect);
                    }
                }
            }
            Intent::Focus {
                stop,
                acquired,
                idle,
            } => {
                state.request_focus = state
                    .open
                    .as_mut()
                    .is_some_and(|p| p.settle_focus(stop, acquired, idle));
            }
            Intent::BarKind(kind) => {
                if let Some(p) = state.open.as_mut() {
                    p.set_bar_kind(&kind, &default_bar_size(&kind));
                }
            }
            Intent::BarSize(size) => {
                if let Some(p) = state.open.as_mut() {
                    p.set_bar_size(&state.listing, &size);
                }
            }
            Intent::Workspace(name) => {
                if let Some(p) = state.open.as_mut() {
                    p.set_workspace(&name);
                }
            }
            Intent::Finish {
                escape,
                scrim_clicked,
                dropdown_open,
            } => {
                if escape || (scrim_clicked && !dropdown_open) {
                    return Self::update(state, Intent::Close);
                }
                return state.pending.take();
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "market_palette/tests.rs"]
mod tests;
