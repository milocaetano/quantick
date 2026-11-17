//! The account route in the window: the tabs as the route session reaches
//! them, the Trading panel's `Account` block, and the hooks that stage it.
//!
//! Every decision is `quantick_paper::route`'s [`BrokerSession`]; the venue
//! is built by `quantick_feed`'s connector. This paints the session's
//! [`Sheet`] and hands each press back as a [`RoutePress`].

use eframe::egui;
use quantick_paper::route::{
    AccountsPort, BrokerSession, RouteHost, RoutePress, RouteTone, RowGesture, RowState, Sheet,
    SourceFacts,
};

use crate::app::arrangement_host::ArrangementHost;
use crate::config::AppConfig;

/// The session the window holds: the configured MetaTrader connector.
pub(crate) fn session(config: &AppConfig) -> BrokerSession {
    let mt5 = &config.broker.metatrader;
    let connector = quantick_feed::mt5_bridge::exec::Mt5Connector::new(mt5.clone());
    BrokerSession::new(Box::new(connector), mt5.allow_agent_orders)
}

/// The window's tabs and config, as the route reaches them.
pub(crate) struct RouteTabs<'a>(pub(crate) &'a mut ArrangementHost, pub(crate) &'a AppConfig);

impl AccountsPort for RouteTabs<'_> {
    fn active_tab(&self) -> u64 {
        self.0.active_id()
    }
    fn account(&self, key: u64) -> Option<&quantick_paper::PaperAccount> {
        self.0.by_id(key).map(|tab| &*tab.paper.account)
    }
    fn account_mut(&mut self, key: u64) -> Option<&mut quantick_paper::PaperAccount> {
        self.0.by_id_mut(key).map(|tab| &mut *tab.paper.account)
    }
}

impl RouteHost for RouteTabs<'_> {
    fn source(&self) -> SourceFacts {
        let tab = &self.0[self.0.active_index()];
        SourceFacts {
            broker_enabled: self.1.broker.metatrader.enabled,
            source_routes: tab.capabilities(self.1).order_routing,
            replay: tab.link.replay.is_some(),
        }
    }
    fn now_ms(&self) -> i64 {
        crate::metrics::wall_clock_ms()
    }
    fn ticket_quantity(&self) -> Option<rust_decimal::Decimal> {
        let tab = &self.0[self.0.active_index()];
        tab.paper.desk.ticket.quantity_preview()
    }
    fn toast(&mut self, text: String) {
        let active = self.0.active_index();
        self.0.runtime_mut(active).paper.show_toast(text);
    }
    /// The scene the launch asked for (`QUANTICK_ACCOUNT_*`), over a fake
    /// bridge whose position opens at the chart's first print.
    #[cfg(any(feature = "scenario-harness", test))]
    fn staged(&self) -> Option<quantick_paper::route::Staged> {
        let hooks = [
            "QUANTICK_ACCOUNT_ROUTE",
            "QUANTICK_ACCOUNT_LIST",
            "QUANTICK_ACCOUNT_HOLD",
        ];
        let [route, list, hold] = hooks.map(crate::hooks::captured::var);
        let mark = self.0[self.0.active_index()].paper.mark_price();
        let magic = self.1.broker.metatrader.magic;
        let values = [route.as_deref(), list.as_deref(), hold.as_deref()];
        quantick_paper::route::Scene::stage(values, mark, |demo, entry| {
            Box::new(quantick_broker_mt5::fake::StagedConnector::new(
                demo, magic, entry,
            ))
        })
    }
}

/// The colour a tone wears, or the panel's own ink.
pub(crate) fn tone_color(tone: RouteTone, ink: egui::Color32) -> egui::Color32 {
    tone.rgb()
        .map_or(ink, |[r, g, b]| egui::Color32::from_rgb(r, g, b))
}

/// The Trading panel's first row, `Account`, and the list it opens inside
/// the panel. Answers what was pressed.
pub(crate) fn draw_route_block(ui: &mut egui::Ui, sheet: &Sheet) -> Option<RoutePress> {
    let palette = crate::theme::get(ui.ctx());
    let ink = tone_color(sheet.tone, palette.text_primary());
    let mut press = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(sheet.selector_label).color(palette.text_muted()));
        let caret = if sheet.locked { "🔒" } else { "▾" };
        let face = format!("{}{}   {caret}", sheet.notice.dot.mark(), sheet.selector);
        let selector = egui::Button::new(egui::RichText::new(face).color(ink).strong())
            .stroke(egui::Stroke::new(1.0_f32, ink.gamma_multiply(0.8)))
            .min_size(egui::vec2(ui.available_width(), 22.0));
        if ui.add(selector).clicked() {
            press = Some(RoutePress::ToggleList);
        }
    });
    if !sheet.open {
        return press;
    }
    egui::Frame::group(ui.style())
        .fill(palette.control())
        .show(ui, |ui| {
            for row in &sheet.rows {
                press = draw_row(ui, row, &palette).or(press);
            }
            if let Some(footer) = &sheet.footer {
                ui.label(egui::RichText::new(footer).color(palette.text_muted()));
            }
            if sheet.leave_prompt {
                ui.label(quantick_paper::route::LEAVE_PROMPT);
                ui.horizontal(|ui| {
                    for (label, leave) in quantick_paper::route::LEAVE_CHOICES {
                        if ui.button(label).clicked() {
                            press = Some(RoutePress::Leave(leave));
                        }
                    }
                });
            }
        });
    press
}

/// One account row: title and hint, the facts under them, and the REAL
/// row's hold sweeping across it while it is pressed.
fn draw_row(
    ui: &mut egui::Ui,
    row: &quantick_paper::route::AccountRow,
    palette: &crate::theme::Palette,
) -> Option<RoutePress> {
    let muted = palette.text_muted();
    let title_ink = if row.state == RowState::Off {
        muted
    } else {
        tone_color(row.tone, palette.text_primary())
    };
    let inner = egui::Frame::none()
        .fill(if row.state == RowState::Selected {
            palette.chrome()
        } else {
            egui::Color32::TRANSPARENT
        })
        .inner_margin(egui::Margin::symmetric(6.0, 4.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&row.title).strong().color(title_ink));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(&row.hint).small().color(muted));
                });
            });
            for sub in &row.subs {
                ui.label(egui::RichText::new(sub).small().monospace().color(muted));
            }
        });
    let rect = inner.response.rect;
    if let Some(progress) = row.progress {
        let [r, g, b] = row.tone.rgb().unwrap_or([0x80, 0x80, 0x80]);
        let sweep =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * progress, rect.height()));
        let fill = egui::Color32::from_rgba_unmultiplied(r, g, b, 41);
        ui.painter().rect_filled(sweep, 0.0, fill);
        ui.ctx().request_repaint();
    }
    let id = ui.id().with(("account_row", &row.title));
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let held = response.is_pointer_button_down_on();
    match row.gesture {
        RowGesture::Click(route) if response.clicked() => Some(RoutePress::Click(route)),
        RowGesture::Hold if held && row.progress.is_none() => Some(RoutePress::HoldStart),
        RowGesture::Hold if !held && row.progress.is_some() => Some(RoutePress::HoldRelease),
        _ => None,
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_ACCOUNT_HOLD",
    "QUANTICK_ACCOUNT_LIST",
    "QUANTICK_ACCOUNT_ROUTE"
];
