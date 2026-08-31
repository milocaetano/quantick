//! The "Open market" dialog, as a [`Surface`].
//!
//! The dialog is `tabstrip.rs`'s [`SourcePicker`]; what the trunk held was
//! the `Option` that decides whether it is on screen, and the translation
//! from what it chose into three things only the application can do — open a
//! tab, write a symbol into the catalog, take one out again.
//!
//! # Who closes the dialog, and when
//!
//! Cancel is the surface's own business and closes here. Choosing a market
//! from the catalog is settled — the surface closes and asks for the tab.
//! **Adding** a symbol is not: the catalog can refuse one, and the rule it is
//! refused by is the whole config's, not this dialog's. So the surface asks,
//! stays open, and the host answers: it closes the dialog on success or hands
//! back the reason through [`SourcePickerSurface::refuse`], which is the
//! answer the trader reads under the field they typed in. One keystroke from
//! a symbol that does fit, with the refusal still on screen — closing on a
//! refusal would read as a crash.

use eframe::egui;

use super::{MarketRequest, Surface, SurfaceEnv, SurfaceResponse};
use crate::config::AppConfig;
use crate::tabstrip::{PickerOutcome, SourcePicker};

/// The `+` dialog and whether it is on screen.
#[derive(Default)]
pub(crate) struct SourcePickerSurface {
    picker: Option<SourcePicker>,
}

impl SourcePickerSurface {
    /// Whether the dialog is on screen.
    ///
    /// Read by the host before every frame, because the list of markets
    /// already open is built for the dialog alone: a `Vec` of owned strings
    /// per tab, which a closed dialog must not pay for at the refresh rate.
    pub fn is_open(&self) -> bool {
        self.picker.is_some()
    }

    /// Put the dialog on screen, starting on `config`'s first market. The
    /// `+` button, Ctrl+T and the capture hook all come through here.
    pub fn open(&mut self, config: &AppConfig) {
        self.picker = Some(SourcePicker::new(config));
    }

    /// Take the dialog off screen — the host's half of an add that worked.
    pub fn close(&mut self) {
        self.picker = None;
    }

    /// Carry the catalog's refusal back to the field the symbol was typed in.
    ///
    /// Does nothing if the dialog has already gone: the host answers a frame
    /// after the ask, and a trader can close the dialog in between.
    pub fn refuse(&mut self, reason: String) {
        if let Some(picker) = self.picker.as_mut() {
            picker.refuse(reason);
        }
    }

    /// The open dialog, for the tests that drive it the way a trader does.
    #[cfg(test)]
    pub fn picker(&self) -> Option<&SourcePicker> {
        self.picker.as_ref()
    }

    /// See [`Self::picker`].
    #[cfg(test)]
    pub fn picker_mut(&mut self) -> Option<&mut SourcePicker> {
        self.picker.as_mut()
    }
}

impl Surface for SourcePickerSurface {
    fn id(&self) -> &'static str {
        "source-picker"
    }

    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) {
        if std::env::var("QUANTICK_SOURCE_PICKER").is_ok_and(|value| value == "1") {
            self.open(env.config);
        }
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let Some(picker) = self.picker.as_mut() else {
            return SurfaceResponse::default();
        };
        let outcome = picker.draw(ctx, env.config, env.added_symbols, env.open_markets);
        let market = match outcome {
            PickerOutcome::Open => None,
            PickerOutcome::Cancel => {
                self.picker = None;
                None
            }
            PickerOutcome::Chosen(feed_id, symbol) => {
                self.picker = None;
                Some(MarketRequest::Open { feed_id, symbol })
            }
            // Deliberately still open: the catalog may refuse, and the
            // refusal belongs under the field it was typed in.
            PickerOutcome::Added { feed_id, symbol } => {
                Some(MarketRequest::Add { feed_id, symbol })
            }
            PickerOutcome::Removed { feed_id, symbol } => {
                Some(MarketRequest::Remove { feed_id, symbol })
            }
        };
        SurfaceResponse {
            market,
            ..SurfaceResponse::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::config::{FeedConfig, MetaTraderSettings, ProviderKind};

    /// One feed with two markets — enough for the dialog to have something
    /// real to open on, which is all these tests ask of it.
    fn config() -> AppConfig {
        AppConfig {
            default_feed: "binance".to_string(),
            default_symbol: "BTCUSDT".to_string(),
            feeds: vec![FeedConfig {
                id: "binance".to_string(),
                name: "Binance".to_string(),
                provider: ProviderKind::Binance,
                symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
                bubble_preset: None,
                symbol_bubble_presets: Default::default(),
                default_layout: None,
                default_bars: None,
            }],
            metatrader: MetaTraderSettings::default(),
            paper: Default::default(),
            history: Default::default(),
        }
    }

    fn env(config: &AppConfig) -> SurfaceEnv<'_> {
        SurfaceEnv {
            config,
            ..SurfaceEnv::quiet(Instant::now())
        }
    }

    /// A closed dialog asks for nothing, which is also what tells the host it
    /// need not build the open-markets list this frame.
    #[test]
    fn a_closed_dialog_asks_for_nothing() {
        let ctx = egui::Context::default();
        let config = config();
        let mut surface = SourcePickerSurface::default();
        assert!(!surface.is_open());
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(&config));
        });
        assert_eq!(response, SurfaceResponse::default());
    }

    /// Opening starts the dialog on a real market from the catalog rather
    /// than on an empty pick nobody can press Open on.
    #[test]
    fn opening_starts_on_a_market_the_catalog_has() {
        let config = config();
        let mut surface = SourcePickerSurface::default();
        surface.open(&config);
        let picker = surface.picker().expect("the dialog is open");
        assert!(
            config.feed(&picker.feed_id).is_some(),
            "the starting feed is one the catalog offers"
        );
    }

    /// A refusal reaches the field the symbol was typed in, and the dialog
    /// stays open around it.
    #[test]
    fn a_refusal_lands_under_the_field() {
        let config = config();
        let mut surface = SourcePickerSurface::default();
        surface.open(&config);
        surface.refuse("US500 is already offered by another feed".to_owned());
        assert!(surface.is_open(), "a refusal does not close the dialog");
        assert!(
            surface
                .picker()
                .and_then(SourcePicker::refusal)
                .is_some_and(|reason| reason.contains("US500"))
        );
    }

    /// The host may answer a frame after the ask, and the trader can close
    /// the dialog in between. A refusal with nowhere to land is dropped, not
    /// a panic.
    #[test]
    fn a_refusal_for_a_dialog_that_left_is_dropped() {
        let mut surface = SourcePickerSurface::default();
        surface.refuse("too late".to_owned());
        assert!(!surface.is_open());
    }
}
