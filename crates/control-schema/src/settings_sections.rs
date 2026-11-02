//! The Settings window as data: its sections and the rows each one holds.
//!
//! The brief is `docs/ux/window-menu-and-workspace-tabs.md`. The app paints this
//! table and carries out what a row asks; nothing here knows a pixel.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::window_menu::{PROGRESSIVE_HISTORY_HOVER, VENUE_LEAD_IN_HOVER};

/// One section of the Settings window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSection {
    #[default]
    Display,
    Data,
    Recording,
    AgentAccess,
}

/// One row of a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRow {
    Appearance,
    Timezone,
    PerfReadings,
    ProgressiveHistory,
    VenueLeadIn,
    RecordDeals,
    AgentSwitch,
    ManageClients,
}

impl SettingsSection {
    /// Every section, in the order the window lists them.
    pub const ALL: [Self; 4] = [
        Self::Display,
        Self::Data,
        Self::Recording,
        Self::AgentAccess,
    ];

    /// What the window's list calls it.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::Data => "Data",
            Self::Recording => "Recording",
            Self::AgentAccess => "Agent access",
        }
    }

    /// Its control in the scene, selected while the window shows it.
    #[must_use]
    pub const fn control_id(self) -> &'static str {
        match self {
            Self::Display => "settings.section.display",
            Self::Data => "settings.section.data",
            Self::Recording => "settings.section.recording",
            Self::AgentAccess => "settings.section.agent_access",
        }
    }

    /// The rows it holds, top to bottom.
    #[must_use]
    pub const fn rows(self) -> &'static [SettingRow] {
        match self {
            Self::Display => &[
                SettingRow::Appearance,
                SettingRow::Timezone,
                SettingRow::PerfReadings,
            ],
            Self::Data => &[SettingRow::ProgressiveHistory, SettingRow::VenueLeadIn],
            Self::Recording => &[SettingRow::RecordDeals],
            Self::AgentAccess => &[SettingRow::AgentSwitch, SettingRow::ManageClients],
        }
    }
}

impl SettingRow {
    /// The row's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance…",
            Self::Timezone => "Timezone",
            Self::PerfReadings => "Perf readings",
            Self::ProgressiveHistory => "Progressive venue history",
            Self::VenueLeadIn => "Venue candles on charts cut by trades",
            Self::RecordDeals => "Record deals by default",
            Self::AgentSwitch => "Local agent access",
            Self::ManageClients => "Manage clients…",
        }
    }

    /// The line under the name.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Appearance => "Colours, fonts and the bubble look",
            Self::Timezone => "Axis and status-bar clock",
            Self::PerfReadings => "fps, frame time and trade count on the status bar",
            Self::ProgressiveHistory => PROGRESSIVE_HISTORY_HOVER,
            Self::VenueLeadIn => VENUE_LEAD_IN_HOVER,
            Self::RecordDeals => {
                "Start writing the venue's deal counter when a MetaTrader B3 symbol connects"
            }
            Self::AgentSwitch => {
                "Lets a local tool reach this window over the control plane, with the scopes \
                 chosen under Manage clients…"
            }
            Self::ManageClients => "Who is connected, what each may do, and how to revoke it",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(section: SettingsSection) -> Vec<&'static str> {
        section.rows().iter().map(|row| row.label()).collect()
    }

    #[test]
    fn each_section_holds_the_rows_of_the_brief() {
        assert_eq!(
            labels(SettingsSection::Display),
            ["Appearance…", "Timezone", "Perf readings"]
        );
        assert_eq!(
            labels(SettingsSection::Data),
            [
                "Progressive venue history",
                "Venue candles on charts cut by trades"
            ]
        );
        assert_eq!(
            labels(SettingsSection::Recording),
            ["Record deals by default"]
        );
        assert_eq!(
            labels(SettingsSection::AgentAccess),
            ["Local agent access", "Manage clients…"]
        );
    }

    #[test]
    fn the_window_opens_on_display_and_lists_the_sections_in_order() {
        assert_eq!(SettingsSection::default(), SettingsSection::Display);
        let titles = SettingsSection::ALL.map(SettingsSection::title);
        assert_eq!(titles, ["Display", "Data", "Recording", "Agent access"]);
    }

    #[test]
    fn a_section_token_on_the_wire_names_its_scene_control() {
        for section in SettingsSection::ALL {
            let token = serde_json::to_value(section).unwrap();
            let token = token.as_str().unwrap();
            assert_eq!(section.control_id(), format!("settings.section.{token}"));
        }
    }
}
