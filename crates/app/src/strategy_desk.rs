//! The Strategies dock tab and the strategy window, painted from
//! `quantick_ui_model::strategy_desk`; a click sends one command through it.

mod rail;
pub(crate) mod window;

use eframe::egui;
use quantick_control_handlers::notify::AttentionHost;
use quantick_control_handlers::strategy_desk::StrategyDeskHost;
use quantick_ui_model::strategy_desk as model;

use crate::app::control_host::ChromePort;
use crate::dock::DockTab;
pub(crate) use rail::Rail;

/// Send what the tab asked last frame, draw the window and send what it
/// asks; the tab's groups while it is open.
pub(crate) fn settle<H>(host: &mut H, ctx: &egui::Context) -> Option<Rail>
where
    H: StrategyDeskHost + AttentionHost + ChromePort + ?Sized,
{
    let stale = rail::take(ctx)
        .map(|sent| model::send(host, sent))
        .is_some();
    if let Some(sent) = window::show(host, ctx, stale) {
        model::send(host, sent);
    }
    let open = host.chrome_reads().dock().tab() == Some(DockTab::Strategies);
    open.then(|| Rail(model::rail(host)))
}
