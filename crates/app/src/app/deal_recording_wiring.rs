//! How the app hands each tab its deal recorder and keeps the one standing
//! choice — record by default — in step across tabs, the workspace and the
//! config.
//!
//! A child of `app` for the same reason `layout_wiring` is: it reads the
//! app's own fields, and lives apart only so `app.rs` grows by one field,
//! one `mod` line and a handful of one-line calls.

use crate::deal_recording::{self, DealRecorder};

use super::QuantickApp;

impl QuantickApp {
    /// A recorder for a tab about to open on `feed_id`/`symbol`.
    ///
    /// The default it opens on is decided in this order: the launch hook
    /// (`QUANTICK_DEAL_RECORDING=on|off`), the workspace's saved choice, the
    /// feed's `record_deals` in the config. A feed with no counter never
    /// starts whatever the default says — the recorder itself waits for the
    /// feed to declare one.
    pub(crate) fn deal_recorder_for(
        &mut self,
        feed_id: &str,
        symbol: &str,
        day_cache: deal_recording::DayCache,
    ) -> DealRecorder {
        let default_on = self
            .harness
            .deal_recording_default()
            .or(self.record_deals)
            .unwrap_or_else(|| self.config.records_deals(feed_id));
        let mut recorder = DealRecorder::with_cache(
            symbol,
            deal_recording::resolve_dir(self.config.deals_dir()),
            default_on,
            day_cache,
        );
        recorder.set_timezone(self.tz.minutes());
        recorder
    }

    /// Give every tab a recorder for the market it streams — a restored
    /// tab, a market switch and a fresh tab all pass through here, once per
    /// frame at the cost of one string compare per tab. A recorder being
    /// replaced is stopped first, so its file is flushed.
    pub(crate) fn ensure_deal_recorders(&mut self) {
        let tz_minutes = self.tz.minutes();
        for index in 0..self.tabs.len() {
            let (feed_id, symbol) = {
                let tab = &mut self.tabs[index];
                // The display timezone names the day files; it follows the
                // trader's setting rather than the one at construction.
                tab.deal_recorder.set_timezone(tz_minutes);
                if tab.deal_recorder.is_for(&tab.active.1) {
                    continue;
                }
                tab.active.clone()
            };
            // The scan cache outlives the recorder: a month of day files
            // is parsed once per session, not once per market switch.
            let day_cache = self.tabs[index].deal_recorder.take_day_cache();
            let mut recorder = self.deal_recorder_for(&feed_id, &symbol, day_cache);
            let tab = &mut self.tabs[index];
            // What the feed already said it can count, carried over: the
            // drain that would say it again is a frame away, and a REC that
            // vanished for one frame on every market switch would flicker.
            recorder.set_available(tab.feed_capabilities.borrow().deal_counter);
            tab.deal_recorder.stop();
            tab.deal_recorder = recorder;
        }
    }

    /// The standing choice as the Tools menu shows it: the saved override,
    /// else what the active tab's feed config says.
    pub(crate) fn record_deals_default(&self) -> bool {
        self.record_deals
            .unwrap_or_else(|| self.config.records_deals(&self.active_tab().feed_id))
    }

    /// The Tools menu's entry for the standing choice.
    pub(crate) fn draw_record_deals_toggle(&mut self, ui: &mut eframe::egui::Ui) {
        let mut record_deals = self.record_deals_default();
        if ui
            .checkbox(&mut record_deals, "Record deals by default")
            .on_hover_text(
                "start writing the venue's deal counter when a MetaTrader B3 symbol \
                 connects, so trades bars cover the whole session",
            )
            .changed()
        {
            self.set_record_deals_default(record_deals);
        }
    }

    /// Set the standing choice: saved to the workspace, and told to every
    /// open recorder that has not decided yet.
    pub(crate) fn set_record_deals_default(&mut self, on: bool) {
        self.record_deals = Some(on);
        for tab in &mut self.tabs {
            tab.deal_recorder.set_default(on);
        }
    }
}
