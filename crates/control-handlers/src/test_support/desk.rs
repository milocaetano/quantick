#![cfg(test)]

use super::{FakeTab, FakeWindow};
use crate::deal_recorder::{DealRecorder, DealRecordingPort};
use crate::settings::{SettingsAct, SettingsOwner, SettingsReadings, SettleError};

/// The fake window's desk: its tabs' recorders and the record-by-default row.
struct FakeDesk<'a> {
    tabs: &'a mut Vec<FakeTab>,
    record_by_default: &'a mut Option<bool>,
}

impl crate::desk::DeskHost for FakeWindow {
    fn desk(&mut self) -> Option<Box<dyn crate::desk::DeskOwner + '_>> {
        let (tabs, record_by_default) = (&mut self.tabs, &mut self.record_by_default);
        Some(Box::new(FakeDesk {
            tabs,
            record_by_default,
        }))
    }
}

impl DealRecordingPort for FakeDesk<'_> {
    fn deal_recorder(&mut self, index: usize) -> Option<&mut dyn DealRecorder> {
        Some(self.tabs.get_mut(index)?)
    }
}

impl SettingsOwner for FakeDesk<'_> {
    fn readings(&self) -> SettingsReadings {
        SettingsReadings {
            record_deals: self.record_by_default.unwrap_or_default(),
            ..SettingsReadings::default()
        }
    }
    fn settle(&mut self, act: SettingsAct) -> Result<(), SettleError> {
        if let SettingsAct::RecordDeals(on) = act {
            *self.record_by_default = Some(on);
        }
        Ok(())
    }
}

impl crate::desk::DeskOwner for FakeDesk<'_> {
    fn act(&mut self, _act: crate::desk::DeskAct) -> bool {
        false
    }
}
