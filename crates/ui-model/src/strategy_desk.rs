//! The Strategies dock tab and the strategy window before anything draws
//! them: every word, ink and button state the app paints, and the command
//! each button sends. What the desk lists and what a command changes is
//! `quantick_session::strategy_desk`'s; the path a command takes is
//! `quantick_control_handlers::strategy_desk::send`, which the
//! `strategy.desk.*` calls share.

use quantick_control::error::ControlError;
use quantick_control_handlers::notify::AttentionHost;
use quantick_control_handlers::strategy_desk::{self as desk, StrategyDeskHost};
use quantick_control_schema::strategy_desk::CommandResult;
use quantick_drawings::DrawingId;
use quantick_engine::Side;
use quantick_session::script_strategies::ScriptPlacement;
use quantick_session::strategy_desk::{
    Asset, DeskAction, DeskGroup, DeskLink, DeskRow, DeskState, InstanceRef, StrategyDeskCommand,
    StrategyDetail,
};

pub type Command = StrategyDeskCommand<DrawingId>;
/// One command for the tab it names.
pub type Sent = (u64, Command);

/// The trader may start a drawn instance's paper orders again; an agent
/// through `strategy.desk.*` may not.
const TRADER_MAY_START_ORDERS: bool = true;

/// What the tab says with no instance armed.
pub const EMPTY: &str =
    "No strategy is armed. Arm one from a drawing's menu or the script library.";
/// What the Code page says for a drawn instance.
pub const NO_CODE: &str = "A strategy on a drawing runs a preset; it has no script.";

/// The theme token a word wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Muted,
    Buy,
    Sell,
    Violet,
    Accent,
}

/// The toggle a header or a row offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Pause,
    Resume,
}

/// One asset's header: its label, how many instances, and pause or resume
/// for all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub label: String,
    pub count: String,
    pub toggle: Toggle,
    pub hover: String,
    pub send: Sent,
}

/// One instance's two lines and its buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub row: DeskRow<DrawingId>,
    pub asset: String,
    /// The badge line, on hover.
    pub status: String,
    pub state: (&'static str, Ink),
    pub link: String,
    pub action: (&'static str, Ink),
    /// Pause or resume; an ended instance arms again from its window.
    pub toggle: Option<(Toggle, Sent)>,
    pub remove: Sent,
}

/// One asset as the tab paints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub header: Header,
    pub lines: Vec<Line>,
}

/// Every open tab's instances as the Strategies tab paints them.
pub fn rail<H: StrategyDeskHost + ?Sized>(host: &mut H) -> Vec<Group> {
    desk::groups(host).iter().map(group).collect()
}

fn group(group: &DeskGroup<DrawingId>) -> Group {
    let label = asset_label(&group.asset);
    let (toggle, command) = if group.paused {
        (Toggle::Resume, Command::ResumeAsset(group.asset.clone()))
    } else {
        (Toggle::Pause, Command::PauseAsset(group.asset.clone()))
    };
    let tab_id = group.rows.first().map_or(0, |row| row.tab_id);
    let header = Header {
        hover: format!("{} every strategy on {label}", verb(toggle)),
        count: group.rows.len().to_string(),
        toggle,
        send: (tab_id, command),
        label: label.clone(),
    };
    let lines = group.rows.iter().map(|row| line(row, &label)).collect();
    Group { header, lines }
}

fn line(row: &DeskRow<DrawingId>, asset: &str) -> Line {
    let state = match row.state {
        DeskState::Watching => ("WATCHING", Ink::Buy),
        DeskState::Paused => ("PAUSED", Ink::Muted),
        DeskState::Ended { .. } => ("ENDED", Ink::Sell),
    };
    let toggle = match row.state {
        DeskState::Watching => Some((Toggle::Pause, Command::Pause(row.id))),
        DeskState::Paused => Some((Toggle::Resume, Command::Resume(row.id))),
        DeskState::Ended { .. } => None,
    };
    Line {
        row: row.clone(),
        asset: asset.to_owned(),
        status: row.status.clone(),
        state,
        link: link(&row.link),
        action: match row.action {
            DeskAction::Alarm => ("alarm", Ink::Violet),
            DeskAction::Order => ("order", Ink::Accent),
        },
        toggle: toggle.map(|(toggle, command)| (toggle, (row.tab_id, command))),
        remove: (row.tab_id, Command::Remove(row.id)),
    }
}

/// What a toggle says it does.
#[must_use]
pub const fn verb(toggle: Toggle) -> &'static str {
    match toggle {
        Toggle::Pause => "pause",
        Toggle::Resume => "resume",
    }
}

#[must_use]
pub fn asset_label(asset: &Asset) -> String {
    format!("{} · {}", asset.symbol, asset.bar_type)
}

/// What an instance rides: its drawing's name, or the chart.
#[must_use]
pub fn link(link: &DeskLink<DrawingId>) -> String {
    match link {
        DeskLink::Drawing {
            name: Some(name), ..
        } => name.clone(),
        DeskLink::Drawing { drawing, .. } => format!("drawing {}", drawing.0),
        DeskLink::Chart => "chart".to_owned(),
    }
}

/// Send `sent` as the trader sends it, and post what the trader must read:
/// the session's refusal or the library file a save wrote.
pub fn send<H: StrategyDeskHost + AttentionHost + ?Sized>(host: &mut H, (tab_id, command): Sent) {
    let answer = desk::send(host, tab_id, command, TRADER_MAY_START_ORDERS);
    if let Some(toast) = toast(answer) {
        host.attention().show_toast(toast);
    }
}

fn toast(answer: Result<CommandResult, ControlError>) -> Option<String> {
    match answer {
        Ok(result) => result
            .saved_as
            .map(|file| format!("saved to the library as {file}")),
        Err(refused) => Some(refused.message),
    }
}

/// A page of the strategy window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Summary,
    Code,
    History,
}

impl Page {
    pub const ALL: [Self; 3] = [Self::Summary, Self::Code, Self::History];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Code => "Code",
            Self::History => "History",
        }
    }

    /// The page a word names; any other word opens the summary.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        let word = word.trim();
        Self::ALL
            .into_iter()
            .find(|page| page.title().eq_ignore_ascii_case(word))
            .unwrap_or(Self::Summary)
    }
}

/// The strategy window over one instance: which page shows, the name a
/// save will use, and the instance's detail as last read.
#[derive(Debug, Clone)]
pub struct StrategyWindow {
    tab_id: u64,
    id: InstanceRef<DrawingId>,
    asset: String,
    pub page: Page,
    pub save_name: String,
    detail: Option<Result<StrategyDetail<DrawingId>, String>>,
}

impl StrategyWindow {
    /// The window over `row` of `asset`, on `page`.
    #[must_use]
    pub fn open(row: &DeskRow<DrawingId>, asset: String, page: Page) -> Self {
        Self {
            tab_id: row.tab_id,
            id: row.id,
            asset,
            page,
            save_name: format!("{} copy", row.name.trim_end_matches(".pine")),
            detail: None,
        }
    }

    /// The window over the first instance any tab lists, on `page`.
    pub fn first<H: StrategyDeskHost + ?Sized>(host: &mut H, page: Page) -> Option<Self> {
        let groups = desk::groups(host);
        let group = groups.iter().find(|group| !group.rows.is_empty())?;
        Some(Self::open(&group.rows[0], asset_label(&group.asset), page))
    }

    /// Read the instance in full when the last read is gone.
    pub fn read<H: StrategyDeskHost + ?Sized>(&mut self, host: &mut H) {
        if self.detail.is_none() {
            let detail = desk::instance_detail(host, self.tab_id, self.id);
            self.detail = Some(detail.map_err(|error| error.message));
        }
    }

    /// Drop the last read: a command may have changed the instance.
    pub fn forget(&mut self) {
        self.detail = None;
    }

    /// The instance as last read, or why it could not be.
    #[must_use]
    pub fn detail(&self) -> Option<Result<&StrategyDetail<DrawingId>, &str>> {
        (self.detail.as_ref()).map(|detail| detail.as_ref().map_err(String::as_str))
    }

    /// Whether the last read found the instance.
    #[must_use]
    pub fn readable(&self) -> bool {
        matches!(self.detail(), Some(Ok(_)))
    }

    /// The instance's description, or why it could not be read.
    #[must_use]
    pub fn description(&self) -> &str {
        match self.detail() {
            Some(Ok(detail)) => &detail.description,
            Some(Err(refusal)) => refusal,
            None => "",
        }
    }

    /// The script's source; a drawn instance has none.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match self.detail() {
            Some(Ok(detail)) => detail.code.as_deref(),
            _ => None,
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        match self.detail() {
            Some(Ok(detail)) => &detail.row.name,
            _ => "Strategy",
        }
    }

    /// The Summary page's rows: where, what a firing does, each declared
    /// input, the asset and the fires today.
    #[must_use]
    pub fn summary(&self) -> Vec<(String, String)> {
        let Some(Ok(detail)) = self.detail() else {
            return Vec::new();
        };
        let place = match detail.placement {
            ScriptPlacement::Region(_) => link(&detail.row.link),
            ScriptPlacement::Range { a, b } => format!("range {a}–{b}"),
            ScriptPlacement::Line { level, rising } => {
                format!("cross {level} {}", if rising { "↑" } else { "↓" })
            }
            ScriptPlacement::Anywhere => "whole chart".to_owned(),
        };
        let action = match detail.row.action {
            DeskAction::Alarm => "alarm",
            DeskAction::Order => "paper order",
        };
        let side = match detail.side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        };
        let mut rows = vec![
            ("Where".to_owned(), place),
            ("Action".to_owned(), format!("{action} · {side}")),
        ];
        rows.extend(detail.inputs.iter().cloned());
        rows.push(("Asset and bar".to_owned(), self.asset.clone()));
        rows.push(("Fires today".to_owned(), detail.fires_today.to_string()));
        rows
    }

    /// The History page: its caption, and each fire as a fraction of the way
    /// from the first loaded bar to the last fire.
    #[must_use]
    pub fn history(&self) -> (String, Vec<f32>) {
        let Some(Ok(detail)) = self.detail() else {
            return (String::new(), Vec::new());
        };
        let fired = &detail.history;
        let caption = format!("{} fires on the loaded bars", fired.len());
        let last = fired.last().copied().unwrap_or(0).max(1) as f32;
        (
            caption,
            fired.iter().map(|slot| *slot as f32 / last).collect(),
        )
    }

    /// Only an ended instance arms again.
    #[must_use]
    pub fn arms(&self) -> bool {
        matches!(self.detail(), Some(Ok(detail)) if matches!(detail.row.state, DeskState::Ended { .. }))
    }

    /// Only a script instance has source to save.
    #[must_use]
    pub fn saves(&self) -> bool {
        matches!(self.detail(), Some(Ok(detail)) if detail.code.is_some())
    }

    #[must_use]
    pub fn arm(&self) -> Sent {
        (self.tab_id, Command::Arm(self.id))
    }

    #[must_use]
    pub fn duplicate(&self) -> Sent {
        (self.tab_id, Command::Duplicate(self.id))
    }

    #[must_use]
    pub fn save_as(&self) -> Sent {
        let name = self.save_name.clone();
        let instance = self.id;
        (self.tab_id, Command::SaveAs { instance, name })
    }
}

#[cfg(test)]
#[path = "strategy_desk_tests.rs"]
mod tests;
