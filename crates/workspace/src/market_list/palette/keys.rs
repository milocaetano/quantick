//! What a key in the palette asks the window to do: Enter opens, focuses,
//! adds, switches or rolls; Delete asks to remove; Up and Down move what has
//! focus; Tab moves the focus.

use super::super::actions::{
    OpenChoice, PrimaryAction, SwitchChoice, primary_action, switch_action,
};
use super::super::copy::BAR_KINDS;
use super::super::rows::{MarketListing, MarketStatus};
use super::super::world::VenueKind;
use super::{MarketPalette, PaletteFocus};

/// What a key in the palette asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteIntent {
    /// Open, or focus the tab that already shows exactly this.
    Open(OpenChoice),
    /// Bring this tab forward; a replay row's Enter, so it never opens a
    /// live tab.
    Focus {
        tab_id: u64,
        symbol: String,
    },
    /// Add the symbol to the venue's list, then open it. May be refused.
    Add(OpenChoice),
    Remove {
        feed_id: String,
        symbol: String,
    },
    /// Switch mode: put the tab on another market.
    Switch(SwitchChoice),
    /// Switch mode: roll the tab onto the contract its feed names next.
    Roll {
        tab_id: u64,
        from_symbol: String,
    },
}

impl MarketPalette {
    /// Enter (or Ctrl+Enter with `background`) on the selected row: what
    /// its [`primary_action`] says. A replay row with no tab has nothing to
    /// open from here.
    pub fn submit(&mut self, listing: &MarketListing, background: bool) -> Option<PaletteIntent> {
        if self.switching.is_some() {
            // Ctrl+Enter is inert: a switch has no background.
            return (!background).then(|| self.submit_switch(listing)).flatten();
        }
        self.refusal = None;
        let row = self.selected_row(listing)?;
        let bar = self.bar(listing);
        let action = primary_action(row, &bar, &self.workspace)?;
        let choice = OpenChoice {
            feed_id: row.feed_id.clone(),
            symbol: row.symbol.clone(),
            bar,
            context_bars: row.context_bars.clone(),
            inherit_bar: self.bar.is_none() && row.bar.is_none(),
            workspace: self.workspace.clone(),
            background,
        };
        match action {
            PrimaryAction::Open | PrimaryAction::OpenSecond => Some(PaletteIntent::Open(choice)),
            PrimaryAction::AddAndOpen { .. } => Some(PaletteIntent::Add(choice)),
            PrimaryAction::Focus { tab_id, .. } => Some(PaletteIntent::Focus {
                tab_id,
                symbol: choice.symbol,
            }),
            _ => None,
        }
    }

    /// Enter in switch mode: switch the tab to the selected market with the
    /// footer's changes, or roll it.
    fn submit_switch(&mut self, listing: &MarketListing) -> Option<PaletteIntent> {
        self.refusal = None;
        let target = self.switching.as_ref()?;
        let row = self.selected_row(listing)?;
        let action = switch_action(row)?;
        if let PrimaryAction::Roll { .. } = action {
            return Some(PaletteIntent::Roll {
                tab_id: target.tab_id,
                from_symbol: target.symbol.clone(),
            });
        }
        Some(PaletteIntent::Switch(SwitchChoice {
            tab_id: target.tab_id,
            feed_id: row.feed_id.clone(),
            symbol: row.symbol.clone(),
            bar: self.bar_changed().then(|| self.bar(listing)),
            workspace: self.workspace_changed().then(|| self.workspace.clone()),
            add: matches!(action, PrimaryAction::AddAndSwitch { .. }),
        }))
    }

    /// Delete on the selected row: take that market out of its venue's list.
    /// Asked for any listed market, so one the config declares (or a tab
    /// shows) is refused where the trader can read why, not ignored.
    #[must_use]
    pub fn remove_selected(&self, listing: &MarketListing) -> Option<PaletteIntent> {
        let row = self.selected_row(listing)?;
        let listed = row.kind == VenueKind::Live
            && !matches!(row.status, MarketStatus::Add | MarketStatus::Roll { .. });
        listed.then(|| PaletteIntent::Remove {
            feed_id: row.feed_id.clone(),
            symbol: row.symbol.clone(),
        })
    }

    /// Answer a key. Up and Down move what has focus: the selection, the bar
    /// kind (opening on `default_size`'s size for it) or the workspace.
    pub fn press(
        &mut self,
        key: PaletteKey,
        listing: &MarketListing,
        workspaces: &[String],
        default_size: impl Fn(&str) -> String,
    ) -> Option<PaletteIntent> {
        let step = match key {
            PaletteKey::Tab => {
                self.cycle_focus();
                return None;
            }
            PaletteKey::Enter { background } => return self.submit(listing, background),
            PaletteKey::Delete => return self.remove_selected(listing),
            PaletteKey::Up => -1,
            PaletteKey::Down => 1,
        };
        match self.focus {
            PaletteFocus::Field => self.move_selection(step, listing),
            PaletteFocus::Bars => {
                let (kind, _) = self.bar_parts(listing);
                let next = stepped(&BAR_KINDS, &kind, step);
                self.set_bar_kind(next, &default_size(next));
            }
            PaletteFocus::Workspace => {
                let names: Vec<&str> = workspaces.iter().map(String::as_str).collect();
                let next = stepped(&names, &self.workspace, step).to_owned();
                self.set_workspace(&next);
            }
        }
        None
    }
}

/// Whether Delete in the field takes the selected row out rather than
/// editing the query: only with the cursor at the end of the text and
/// nothing selected, where Delete has nothing to delete. `cursor` is the
/// field's (primary, secondary) character offsets; `None`, no cursor yet,
/// is the end, where the field puts it.
#[must_use]
pub fn delete_removes_row(query: &str, cursor: Option<(usize, usize)>) -> bool {
    let end = query.chars().count();
    cursor.is_none_or(|(primary, secondary)| primary == end && secondary == end)
}

/// A key the palette answers; Escape is the window's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKey {
    Up,
    Down,
    Tab,
    Enter { background: bool },
    Delete,
}

fn stepped<'a>(options: &[&'a str], current: &str, step: isize) -> &'a str {
    let Some(last) = options.len().checked_sub(1) else {
        return "";
    };
    let at = options.iter().position(|option| *option == current);
    let next = at.map_or(0, |at| (at as isize + step).rem_euclid(last as isize + 1));
    options[next as usize]
}
