//! The widget panes, docked as floating windows over the chart. Each pane is
//! one [`WIDGET_SURFACES`] line: its id, header title, the feed capability
//! it needs and its headless scene builder. One shared [`painter`] draws
//! every scene, the control plane opens, closes and reads them, and
//! `QUANTICK_WIDGETS=<id,...>` opens them for a capture. The "+" menu
//! adopts this list later.

mod painter;

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use quantick_appearance::Token;
use quantick_civil::TzOffset;
use quantick_control_handlers::widgets::{self as handlers, WidgetDeck, WidgetFeed, WidgetSpec};
use quantick_control_schema::widget::WidgetRead;

use crate::app::arrangement_host::ArrangementHost;
use crate::config::AppConfig;
use crate::pane::PaneSide;

#[rustfmt::skip]
pub(crate) const WIDGET_SURFACES: [WidgetSpec; 5] = [
    WidgetSpec { id: "depth", title: "DEPTH", gate: |caps| caps.book_capture, scene: handlers::depth_scene },
    WidgetSpec { id: "big_trades", title: "BIG TRADES", gate: |_| true, scene: handlers::big_trades_scene },
    WidgetSpec { id: "liquidations", title: "LIQUIDATIONS", gate: |caps| caps.liquidations, scene: handlers::liquidations_scene },
    WidgetSpec { id: "open_interest", title: "OI + FUNDING", gate: |caps| caps.open_interest, scene: handlers::open_interest_scene },
    WidgetSpec { id: "spot_perp_cvd", title: "SPOT vs PERP CVD", gate: |caps| caps.spot_perp_pair, scene: handlers::spot_perp_cvd_scene },
];

/// A pane body's size before its window first lays out, and its window's.
const BODY: (f32, f32) = (440.0, 320.0);
const GAP: f32 = 12.0;

/// The open panes and the body size each last painted at.
#[derive(Default)]
pub(crate) struct WidgetSurfaces {
    deck: WidgetDeck,
    sizes: BTreeMap<&'static str, (f32, f32)>,
    #[cfg(any(feature = "scenario-harness", test))]
    hooked: bool,
}

/// Lend `run` what the panes read from the active tab, and its market.
fn with_feed<R>(
    tabs: &mut ArrangementHost,
    (config, tz): (&AppConfig, TzOffset),
    run: impl FnOnce(WidgetFeed<'_>, String) -> R,
) -> R {
    let tab_id = tabs.active_id();
    let tab = tabs.runtime_mut(tabs.active_index());
    let capabilities = tab.capabilities(config);
    let market = format!("{} · {}", tab.symbol, tab.feed_display_name(config));
    let (setup, base_unit) = config.widget_setup(&tab.feed_id, &tab.symbol);
    let pane = tab.pane_mut(PaneSide::Flow);
    let at_ms = pane
        .orderflow
        .as_mut()
        .and_then(|view| view.health().last_event_ms);
    let book = pane.orderflow.as_ref().map(|view| view.cached_book());
    run(
        WidgetFeed {
            tab_id,
            capabilities,
            ladder: book.and_then(|book| book.1).zip(at_ms),
            earlier: None,
            fallback_tick: book.map(|book| book.2).unwrap_or_default(),
            trades: pane.state.trades(),
            setup: &setup,
            base_unit: &base_unit,
            tz,
            now_ms: crate::metrics::wall_clock_ms(),
        },
        market,
    )
}

/// Draw every open pane in its own window, laid out for the size its body
/// had last frame.
pub(crate) fn draw(
    ctx: &egui::Context,
    surfaces: &mut WidgetSurfaces,
    tabs: &mut ArrangementHost,
    reads: (&AppConfig, TzOffset),
) {
    #[cfg(any(feature = "scenario-harness", test))]
    surfaces.apply_hook();
    let open = surfaces.deck.open_ids();
    if open.is_empty() {
        return;
    }
    let palette = crate::theme::get(ctx);
    let ink = |on: bool| {
        *palette.get(if on {
            Token::TextPrimary
        } else {
            Token::TextFaint
        })
    };
    with_feed(tabs, reads, |feed, market| {
        for id in open {
            let Ok(spec) = handlers::find(&WIDGET_SURFACES, id) else {
                continue;
            };
            let slot = WIDGET_SURFACES
                .iter()
                .position(|each| each.id == id)
                .unwrap_or(0);
            let corner = egui::pos2(
                GAP + (slot % 3) as f32 * (BODY.0 + GAP),
                90.0 + (slot / 3) as f32 * (BODY.1 + 4.0 * GAP),
            );
            let size = surfaces.sizes.get(id).copied().unwrap_or(BODY);
            let scene = surfaces.deck.scene(spec, &feed, size);
            let mut shown = true;
            egui::Window::new(spec.title)
                .id(egui::Id::new(("widget", id)))
                .title_bar(false)
                .default_pos(corner)
                .default_size(BODY)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(spec.title)
                                .monospace()
                                .strong()
                                .color(ink(true)),
                        );
                        ui.label(RichText::new(&market).small().color(ink(false)));
                        for (chip, on) in &scene.chips {
                            ui.label(RichText::new(chip).monospace().small().color(ink(*on)));
                        }
                        shown = !ui.small_button("×").clicked();
                    });
                    let (rect, _) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                    painter::paint(&ui.painter_at(rect), rect.min, scene, &palette);
                    surfaces.sizes.insert(id, (rect.width(), rect.height()));
                });
            if !shown {
                surfaces.deck.set_open(id, false);
            }
        }
    });
}

/// The control plane's door: open or close `id` when asked, then the
/// scene the window paints for it, at the size it paints it.
pub(crate) fn answer(
    (surfaces, tabs): (&mut WidgetSurfaces, &mut ArrangementHost),
    reads: (&AppConfig, TzOffset),
    (id, open): (&str, Option<bool>),
) -> Result<WidgetRead, String> {
    let spec = handlers::find(&WIDGET_SURFACES, id)?;
    if let Some(open) = open {
        surfaces.deck.set_open(spec.id, open);
    }
    let size = surfaces.sizes.get(spec.id).copied().unwrap_or(BODY);
    with_feed(tabs, reads, |feed, market| {
        let state = handlers::state(spec, &surfaces.deck, &feed.capabilities);
        Ok(handlers::read(
            state,
            market,
            surfaces.deck.scene(spec, &feed, size),
        ))
    })
}

#[cfg(any(feature = "scenario-harness", test))]
impl WidgetSurfaces {
    /// Open what `QUANTICK_WIDGETS` lists, once, on the first frame.
    fn apply_hook(&mut self) {
        if !std::mem::replace(&mut self.hooked, true) {
            let ids = crate::hooks::captured::var("QUANTICK_WIDGETS").unwrap_or_default();
            self.deck.open_listed(&WIDGET_SURFACES, &ids);
        }
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_WIDGETS"];
