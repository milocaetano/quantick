//! The chart settings window, as a [`Surface`]: one window, one open flag,
//! a tab per settings family.
//!
//! The candle appearance and the footprint knobs used to be two windows with
//! two flags, so a trader could stack both over the chart and the toolbar,
//! the menus and the capture hooks each had to know which one they meant.
//! They are tabs now. A double click on the candles, the toolbar's
//! appearance button and the View menu open the Candles tab; the footprint
//! toolbar button, the layer menu's "configure footprint…" and the footprint
//! capture hook open the same window on the Footprint tab.
//!
//! # A tab docks as a file and a line
//!
//! Each tab is a [`SettingsTab`] in its own file, holding the state only it
//! reads; [`ChartSettingsSurface::tabs`] lists them in the order the window
//! shows them. The window, the tab strip, the scroll area and the appearance
//! log's debounce are drawn here once for all of them.
//!
//! # Why the surface edits a copy
//!
//! The chart style and the footprint setup are host state every renderer
//! reads, so a tab edits a copy and hands it back through
//! [`SurfaceResponse`], exactly as the two windows did.
//!
//! # The debounce, and why it is the window's
//!
//! Dragging an opacity slider changes the style on every frame of the drag.
//! The log waits [`STYLE_LOG_DEBOUNCE`] after the last change — or fires the
//! moment the window closes, because a trader who shut it is done.

pub(crate) mod candles;
pub(crate) mod footprint;

use std::time::{Duration, Instant};

use eframe::egui;

pub(crate) use quantick_control_schema::chart_settings::ChartSettingsTab;

use super::{StyleLogRequest, Surface, SurfaceEnv, SurfaceResponse};
use crate::style::CandlePreset;
pub(crate) use candles::CandlesTab;
pub(crate) use footprint::FootprintTab;

/// How long the appearance log waits for a gesture to settle. Longer than a
/// frame at any refresh rate, shorter than the pause between two deliberate
/// clicks.
const STYLE_LOG_DEBOUNCE: Duration = Duration::from_millis(350);

/// The window's default width: the widest tab's knobs without a scrollbar.
const WINDOW_WIDTH: f32 = 390.0;
/// The tallest a tab's body grows before it scrolls.
const BODY_MAX_HEIGHT: f32 = 650.0;

/// What one tab's frame asked for.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct TabResponse {
    /// The host-facing asks: a new style, a footprint change.
    pub surface: SurfaceResponse,
    /// A named candle preset was clicked, which the log prefers over reading
    /// the name back from the values.
    pub applied_preset: Option<CandlePreset>,
}

/// One tab of the chart settings window.
pub(crate) trait SettingsTab {
    /// Which tab this is — its caption and its wire name.
    fn tab(&self) -> ChartSettingsTab;

    /// Draw the tab's body into the window this frame.
    fn draw(&mut self, ui: &mut egui::Ui, env: &SurfaceEnv<'_>) -> TabResponse;

    /// The window opened, or is open, over `env`: start whatever a sitting
    /// remembers. Called every open frame, so it must be idempotent.
    fn opened(&mut self, _env: &SurfaceEnv<'_>) {}

    /// The window closed: drop whatever a sitting remembered.
    fn closed(&mut self) {}
}

/// The window, its tabs and the appearance log's debounce.
#[derive(Default)]
pub(crate) struct ChartSettingsSurface {
    open: bool,
    tab: ChartSettingsTab,
    /// The candle appearance and its presets.
    pub candles: CandlesTab,
    /// The focused chart's footprint knobs and their presets.
    pub footprint: FootprintTab,
    /// A change has happened that has not been logged yet.
    log_pending: bool,
    /// When the last change landed, for the debounce.
    last_change: Option<Instant>,
    /// The named preset that change came from, if a click named one.
    pending_preset: Option<CandlePreset>,
}

impl ChartSettingsSurface {
    /// Every tab, in the order the window shows them. A new tab is a file,
    /// a field and an entry here.
    fn tabs(&mut self) -> [&mut dyn SettingsTab; 2] {
        [&mut self.candles, &mut self.footprint]
    }

    /// Whether the window is on screen.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the window is on screen showing `tab` — what lights the
    /// toolbar button that opens that tab.
    pub fn is_open_on(&self, tab: ChartSettingsTab) -> bool {
        self.open && self.tab == tab
    }

    /// The tab shown, or shown next time the window opens.
    pub fn tab(&self) -> ChartSettingsTab {
        self.tab
    }

    /// Put the window on screen at `tab` — every entry point comes through
    /// here, so a click and a named call cannot disagree about the window.
    pub fn open(&mut self, tab: ChartSettingsTab) {
        self.open = true;
        self.tab = tab;
    }

    /// Close the window, ending the sitting. A pending log line is flushed
    /// by the next draw.
    pub fn close(&mut self) {
        self.open = false;
        for tab in self.tabs() {
            tab.closed();
        }
    }

    /// The toolbar's flip: close when already showing `tab`, else show it.
    pub fn toggle(&mut self, tab: ChartSettingsTab) {
        if self.is_open_on(tab) {
            self.close();
        } else {
            self.open(tab);
        }
    }

    /// Read every tab's preset file again, after an imported cockpit
    /// replaced them.
    pub fn reload_presets(&mut self) {
        self.candles.reload_presets();
        self.footprint.reload_presets();
    }

    /// Whether a change that has not been recorded yet should be recorded
    /// now: once the gesture has settled, or the moment the window closes.
    fn should_log(&self, now: Instant) -> bool {
        let settled = self
            .last_change
            .is_some_and(|changed| now.saturating_duration_since(changed) >= STYLE_LOG_DEBOUNCE);
        self.log_pending && (settled || !self.open)
    }

    /// Hand over the pending log line, if one is owed, and clear it.
    fn take_pending_log(&mut self) -> Option<StyleLogRequest> {
        if !self.log_pending {
            return None;
        }
        self.log_pending = false;
        Some(StyleLogRequest {
            applied_preset: self.pending_preset.take(),
        })
    }

    /// Note a style change for the debounced log.
    fn note_style_change(&mut self, now: Instant, preset: Option<CandlePreset>) {
        self.log_pending = true;
        self.last_change = Some(now);
        // A later knob drag makes the look nobody's preset again.
        self.pending_preset = preset;
    }
}

impl Surface for ChartSettingsSurface {
    fn id(&self) -> &'static str {
        "chart-settings"
    }

    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        let on = |name: &str| crate::hooks::captured::var(name).is_some_and(|value| value == "1");
        if on("QUANTICK_STYLE_PANEL") {
            self.open(ChartSettingsTab::Candles);
        }
        if on("QUANTICK_FOOTPRINT_PANEL") {
            self.open(ChartSettingsTab::Footprint);
        }
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        if !self.open {
            // A closed window still flushes a change the debounce was
            // holding: the toolbar and the menus close it from outside this
            // draw, and the record of the last edit must not be stranded.
            return SurfaceResponse {
                log_style_change: self.take_pending_log(),
                ..SurfaceResponse::default()
            };
        }
        let id = egui::Id::new(self.id());
        let mut window_open = true;
        let mut selected = self.tab;
        let mut answer = TabResponse::default();
        let tabs = self.tabs();
        for tab in tabs {
            tab.opened(env);
        }
        let mut tabs = self.tabs();
        egui::Window::new("chart settings")
            .id(id)
            .open(&mut window_open)
            .default_width(WINDOW_WIDTH)
            .min_width(WINDOW_WIDTH - 40.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for tab in tabs.iter() {
                        let which = tab.tab();
                        ui.selectable_value(&mut selected, which, which.title());
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(id.with("body"))
                    .max_height(BODY_MAX_HEIGHT)
                    .show(ui, |ui| {
                        if let Some(tab) = tabs.iter_mut().find(|tab| tab.tab() == selected) {
                            answer = tab.draw(ui, env);
                        }
                    });
            });
        self.tab = selected;
        if !window_open {
            self.close();
        }
        let TabResponse {
            surface: mut response,
            applied_preset,
        } = answer;
        if response.style.is_some() {
            self.note_style_change(env.now, applied_preset);
        }
        // Read after the window, so one closed with its own X flushes on the
        // frame it left.
        if self.should_log(env.now) {
            response.log_style_change = self.take_pending_log();
        }
        response
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_STYLE_PANEL", "QUANTICK_FOOTPRINT_PANEL"];

#[cfg(test)]
mod tests;
