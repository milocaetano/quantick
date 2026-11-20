//! The Strategies dock tab and the strategy window over every armed instance,
//! as `quantick_session::strategy_desk` groups and details them. A click sends
//! one `StrategyDeskCommand` down the path the `strategy.desk.*` calls take
//! (`quantick_control_handlers::strategy_desk::send`); nothing here decides.

mod rail;
pub(crate) mod window;

use eframe::egui;
use quantick_control_handlers::notify::AttentionHost;
use quantick_control_handlers::strategy_desk::{self, StrategyDeskHost};
use quantick_drawings::DrawingId;
use quantick_session::strategy_desk::StrategyDeskCommand;

pub(crate) use rail::Rail;

/// One command for the tab it names.
type Sent = (u64, StrategyDeskCommand<DrawingId>);

/// Draw the window, send what it or the tab asked this frame, and toast the
/// session's refusal or the file a save wrote.
pub(crate) fn settle<H>(host: &mut H, ctx: &egui::Context, rail: Option<Rail>)
where
    H: StrategyDeskHost + AttentionHost + ?Sized,
{
    let from_window = window::show(host, ctx);
    let Some((tab_id, command)) = rail.and_then(|rail| rail.sent).or(from_window) else {
        return;
    };
    window::refresh(ctx);
    // The trader may start a drawn instance's paper orders again; an agent
    // through `strategy.desk.*` may not.
    let toast = match strategy_desk::send(host, tab_id, command, true) {
        Ok(result) => result
            .saved_as
            .map(|file| format!("saved to the library as {file}")),
        Err(refused) => Some(refused.message),
    };
    if let Some(toast) = toast {
        host.attention().show_toast(toast);
    }
}
