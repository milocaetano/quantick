//! Drawing pointer fixtures explicitly request the whole canvas for candles.
use super::*;

pub(super) fn app_with_history(count: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    app_with_history_and_launch(count, AppLaunch::default())
}

pub(super) fn app_with_history_and_launch(
    count: u64,
    launch: AppLaunch,
) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, commands) = super::app_with_history_and_launch(count, launch);
    app.active_tab_mut().flow_pane.set_layer_visible(
        ChartLayer::TapeChart,
        false,
        &mut Default::default(),
    );
    (app, commands)
}
