//! The palette while it is on screen: the query, the selection, the footer's
//! bar and workspace, and which footer stop has the keyboard. What a key
//! asks the window to do lives in `keys`.

use super::actions::{PrimaryAction, SwitchTarget, primary_action, switch_action};
use super::copy::COMMAND_PREFIX;
use super::rows::{MarketListing, MarketRow};
use super::search::list_markets;
use super::world::MarketWorld;
use crate::workspace_book::DEFAULT_WORKSPACE;

mod keys;
pub use keys::{PaletteIntent, PaletteKey, delete_removes_row};

/// Which footer stop has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteFocus {
    #[default]
    Field,
    Bars,
    Workspace,
}

/// The palette while it is on screen: the query, the selection and the
/// footer's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketPalette {
    query: String,
    /// Index among the selectable rows; `None` until the trader moves it,
    /// which reads as the first selectable row that is not a roll.
    selected: Option<usize>,
    focus: PaletteFocus,
    /// The stop whose widget should take the keyboard, once: set when the
    /// palette opens and on Tab, so a click elsewhere is not pulled back.
    focus_request: Option<PaletteFocus>,
    /// The trader's bar, as `kind:size`; `None` follows the selected row.
    bar: Option<String>,
    workspace: String,
    refusal: Option<String>,
    /// Switch mode: the tab Enter switches, rather than opening a tab.
    switching: Option<SwitchTarget>,
}

impl Default for MarketPalette {
    fn default() -> Self {
        Self {
            query: String::new(),
            selected: None,
            focus: PaletteFocus::Field,
            focus_request: Some(PaletteFocus::Field),
            bar: None,
            workspace: DEFAULT_WORKSPACE.to_owned(),
            refusal: None,
            switching: None,
        }
    }
}

impl MarketPalette {
    /// The palette in switch mode, its footer on the tab's bar and workspace.
    #[must_use]
    pub fn switching(target: SwitchTarget) -> Self {
        Self {
            workspace: target.workspace.clone(),
            switching: Some(target),
            ..Self::default()
        }
    }

    /// The palette as it comes up: on `query`, switching `switching`'s tab
    /// when it names one.
    #[must_use]
    pub fn opened(query: &str, switching: Option<SwitchTarget>) -> Self {
        let mut palette = switching.map_or_else(Self::default, Self::switching);
        palette.set_query(query);
        palette
    }

    #[must_use]
    pub fn switch_target(&self) -> Option<&SwitchTarget> {
        self.switching.as_ref()
    }

    /// The list for the query in this palette's mode.
    #[must_use]
    pub fn listing(&self, world: &MarketWorld<'_>) -> MarketListing {
        let listing = list_markets(world, &self.query);
        match &self.switching {
            Some(target) => target.listing(world, listing, &self.query),
            None => listing,
        }
    }

    /// What Enter does on `row` in this mode.
    fn action(&self, row: &MarketRow, bar: &str) -> Option<PrimaryAction> {
        match self.switching {
            Some(_) => switch_action(row),
            None => primary_action(row, bar, &self.workspace),
        }
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Replace the query; the selection goes back to the first row, and a
    /// refusal about the old query goes.
    pub fn set_query(&mut self, query: &str) {
        if self.query != query {
            query.clone_into(&mut self.query);
            (self.selected, self.refusal) = (None, None);
        }
    }

    /// The editable query, for the text field; call [`Self::settle_query`]
    /// with what it held before.
    pub fn query_mut(&mut self) -> &mut String {
        &mut self.query
    }

    /// After the field edited the query: a changed query selects the first
    /// row and drops the refusal about the old one.
    pub fn settle_query(&mut self, before: &str) {
        if self.query != before {
            (self.selected, self.refusal) = (None, None);
        }
    }

    /// The row Enter acts on: the one the trader moved to, else the first
    /// selectable row that is not a roll. A reflex Enter never rolls.
    #[must_use]
    pub fn selected_row<'l>(&self, listing: &'l MarketListing) -> Option<&'l MarketRow> {
        (self.selected)
            .and_then(|index| listing.selectable().nth(index))
            .or_else(|| listing.selectable().find(|row| !row.is_roll()))
    }

    /// The selected row's index among the selectable rows.
    fn selected_index(&self, listing: &MarketListing) -> Option<usize> {
        let row = self.selected_row(listing)?;
        (listing.selectable()).position(|candidate| std::ptr::eq(candidate, row))
    }

    /// Whether `row` is the selected one.
    #[must_use]
    pub fn is_selected(&self, listing: &MarketListing, row: &MarketRow) -> bool {
        self.selected_row(listing)
            .is_some_and(|selected| std::ptr::eq(selected, row))
    }

    /// Move over the selectable rows, wrapping.
    pub fn move_selection(&mut self, delta: isize, listing: &MarketListing) {
        let count = listing.selectable().count();
        if count == 0 {
            return;
        }
        // Nothing selected (only a roll is left): Down lands on the first
        // row, Up on the last.
        let from =
            (self.selected_index(listing)).map_or(isize::from(delta < 0) - 1, |at| at as isize);
        let index = (from + delta).rem_euclid(count as isize) as usize;
        self.choose(index, listing);
    }

    /// Select the selectable row at `index`; a refusal about another row goes.
    fn choose(&mut self, index: usize, listing: &MarketListing) {
        if self.selected_index(listing) != Some(index) {
            self.refusal = None;
        }
        self.selected = Some(index);
    }

    /// Select `row`, as a hover or click does.
    pub fn select(&mut self, listing: &MarketListing, row: &MarketRow) {
        if let Some(index) = listing
            .selectable()
            .position(|candidate| std::ptr::eq(candidate, row))
        {
            self.choose(index, listing);
        }
    }

    /// The bar Enter opens on: the trader's, else (switching) the tab's, else
    /// the selected row's, else the one a new tab inherits.
    #[must_use]
    pub fn bar(&self, listing: &MarketListing) -> String {
        (self.bar.clone())
            .or_else(|| self.switching.as_ref().map(|target| target.bar.clone()))
            .or_else(|| self.selected_row(listing).and_then(|row| row.bar.clone()))
            .or_else(|| listing.inherited_bar.clone())
            .unwrap_or_default()
    }

    /// The trader picked a bar other than the one the footer started on.
    #[must_use]
    pub fn bar_changed(&self) -> bool {
        let start = self.switching.as_ref().map(|target| &target.bar);
        self.bar.is_some() && self.bar.as_ref() != start
    }

    pub fn set_bar_kind(&mut self, kind: &str, size: &str) {
        self.bar = Some(format!("{kind}:{size}"));
    }

    /// A new size on the kind shown.
    pub fn set_bar_size(&mut self, listing: &MarketListing, size: &str) {
        let bar = self.bar(listing);
        let kind = bar.split_once(':').map_or(bar.as_str(), |(kind, _)| kind);
        self.bar = Some(format!("{kind}:{size}"));
    }

    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    #[must_use]
    pub fn workspace_changed(&self) -> bool {
        let start = self.switching.as_ref();
        self.workspace != start.map_or(DEFAULT_WORKSPACE, |target| target.workspace.as_str())
    }

    pub fn set_workspace(&mut self, name: &str) {
        name.clone_into(&mut self.workspace);
    }

    #[must_use]
    pub fn focus(&self) -> PaletteFocus {
        self.focus
    }

    /// Tab: the field, then Bars, then Workspace, then the field.
    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            PaletteFocus::Field => PaletteFocus::Bars,
            PaletteFocus::Bars => PaletteFocus::Workspace,
            PaletteFocus::Workspace => PaletteFocus::Field,
        };
        self.focus_request = Some(self.focus);
    }

    /// Keep a widget's keyboard focus and `stop` agreeing, as each footer
    /// stop's widget draws: a click on it (`clicked`) moves the stop there.
    /// Answers whether the widget should ask for the keyboard now: once when
    /// its stop changes, after a click, or while it is the stop and nothing
    /// holds the keyboard (`idle`, as after a press on a row) -- never every
    /// frame, so a click elsewhere is not pulled back.
    pub fn settle_focus(&mut self, stop: PaletteFocus, clicked: bool, idle: bool) -> bool {
        if clicked {
            self.adopt_focus(stop);
        }
        self.take_focus_request(stop) || (self.focus == stop && (idle || clicked))
    }

    /// A click gave `stop` the keyboard: follow it, and ask nothing back.
    pub fn adopt_focus(&mut self, stop: PaletteFocus) {
        if self.focus != stop {
            (self.focus, self.focus_request) = (stop, None);
        }
    }

    /// Whether `stop`'s widget should request the keyboard now; answers
    /// true once per open or Tab.
    pub fn take_focus_request(&mut self, stop: PaletteFocus) -> bool {
        let asked = self.focus_request == Some(stop);
        if asked {
            self.focus_request = None;
        }
        asked
    }

    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Show why the last ask was refused, under the field.
    pub fn refuse(&mut self, reason: String) {
        self.refusal = Some(reason);
    }

    /// The query starts with `>`: commands, not markets.
    #[must_use]
    pub fn commands_mode(&self) -> bool {
        self.query.trim_start().starts_with(COMMAND_PREFIX)
    }

    /// The Bars selector's kind and size.
    #[must_use]
    pub fn bar_parts(&self, listing: &MarketListing) -> (String, String) {
        let bar = self.bar(listing);
        let (kind, size) = bar.split_once(':').unwrap_or((&bar, ""));
        (kind.to_owned(), size.to_owned())
    }

    /// What the primary label says Enter does.
    #[must_use]
    pub fn primary_label(&self, listing: &MarketListing) -> String {
        if self.commands_mode() {
            return PrimaryAction::Run.label();
        }
        let bar = self.bar(listing);
        let chosen = self
            .selected_row(listing)
            .and_then(|row| self.action(row, &bar));
        match (chosen, &self.switching) {
            (Some(action), _) => action.label(),
            (None, Some(_)) => "Switch".to_owned(),
            (None, None) => PrimaryAction::Open.label(),
        }
    }
}
