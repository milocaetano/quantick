//! The Settings window's rows as an owner port: what they read and what each
//! asks of the window.
//!
//! The window's rows and `settings.set` settle through the same
//! [`SettingsOwner::settle`], so a named call and a click cannot disagree.
//! Rate class: a human or an agent changing a setting. Never per trade.

use quantick_civil::TzOffset;
use quantick_control::error::ControlError;
use quantick_control_schema::desk::SettingsResult;
use quantick_stores::appearance::Builtin;

/// What a Settings row asks the window to do, for the rows a connection may
/// settle too. The Agent access rows widen a connection's own authority, so
/// they are not acts here: only the window's own draw carries them out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettingsAct {
    OpenAppearance,
    Theme(Builtin),
    Timezone(TzOffset),
    PerfReadings(bool),
    ProgressiveHistory(bool),
    VenueLeadIn(bool),
    RecordDeals(bool),
}

/// Why an act did not land.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettleError {
    /// The theme could not be applied, in the words its menu shows.
    Theme(String),
}

impl From<SettleError> for ControlError {
    fn from(error: SettleError) -> Self {
        match error {
            SettleError::Theme(message) => Self::invalid_request(message),
        }
    }
}

/// The values the rows show, read from their owners.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SettingsReadings {
    pub timezone: TzOffset,
    /// The bundled theme in use; `None` when a custom one is.
    pub theme: Option<Builtin>,
    pub show_perf: bool,
    pub progressive_history: bool,
    pub venue_lead_in: bool,
    pub record_deals: bool,
}

/// The owner of every value a Settings row shows.
pub trait SettingsOwner {
    fn readings(&self) -> SettingsReadings;
    /// Carry out one act, as the row's click does.
    ///
    /// # Errors
    /// Why the act did not land; a refused act changed nothing.
    fn settle(&mut self, act: SettingsAct) -> Result<(), SettleError>;
}

impl From<SettingsReadings> for SettingsResult {
    fn from(readings: SettingsReadings) -> Self {
        Self {
            timezone_offset_minutes: readings.timezone.minutes(),
            timezone_label: readings.timezone.label(),
            theme: readings.theme.map_or("custom", Builtin::key).to_owned(),
            perf_readings: readings.show_perf,
            progressive_history: readings.progressive_history,
            venue_lead_in: readings.venue_lead_in,
            record_deals: readings.record_deals,
        }
    }
}

/// The offset `minutes` names, when the Timezone row offers it.
///
/// # Errors
/// An offset the row does not list.
pub fn offered_zone(minutes: i32) -> Result<TzOffset, String> {
    let zone = TzOffset::new(minutes);
    if TzOffset::ALL.contains(&zone) {
        return Ok(zone);
    }
    Err(format!(
        "{minutes} is not an offset the Timezone row offers; it lists the standard zones \
         from -720 to 840 minutes east of UTC"
    ))
}
