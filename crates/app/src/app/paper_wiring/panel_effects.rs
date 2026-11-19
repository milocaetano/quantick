//! Apply paper panel effects around the dock and tail painting.
use crate::replay_view::ReplayAction;
use crate::surfaces::replay_browser::ReplayBrowserExt as _;
/// The same recipient surrounds both Dock and tail painting, borrowing only
/// the account collection and browser that its prefix can affect.
pub(in crate::app) fn begin_paper_panel_effects(
    tabs: &mut crate::app::arrangement_host::ArrangementHost,
    browser: &mut crate::surfaces::replay_browser::ReplayBrowser,
    response: crate::surfaces::SurfaceResponse,
) -> Result<crate::surfaces::PaperPanelEffects, Box<crate::surfaces::SurfaceResponse>> {
    let mut paper = response.into_paper_panel()?;
    #[cfg(test)]
    paper_panel_steps::record("journal-fanout");
    if let Some(edit) = paper.journal_edit.take() {
        let _ = crate::app::paper_wiring::apply_journal_edit_across(tabs, &edit);
    }
    #[cfg(test)]
    paper_panel_steps::record("replay-browser");
    if paper.open_replay_browser {
        browser.open_browser();
    }
    Ok(paper)
}

pub(in crate::app) fn finish_paper_panel_effects(
    tabs: &mut crate::app::arrangement_host::ArrangementHost,
    config: &crate::config::AppConfig,
    workspace: &mut crate::workspace_store::WorkspaceStore,
    response: crate::surfaces::PaperPanelEffects,
) {
    #[cfg(test)]
    paper_panel_steps::record("close-replay");
    if response.close_replay {
        // A click that lost its slot has the trader's next click behind
        // it; only the one-shot hook cares about the answer.
        let active = tabs.active_index();
        let _ = crate::app::replay_and_history::apply_replay_action(
            tabs.runtime_mut(active),
            config,
            ReplayAction::Close,
        );
    }
    #[cfg(test)]
    paper_panel_steps::record("navigation");
    if let Some((opened, closed)) = response.navigate_to_trade {
        let active = tabs.active_index();
        crate::app::frame::panels::center_flow_pane_on_trade(
            tabs.runtime_mut(active),
            opened,
            closed,
        );
    }
    let mut settings = crate::app::paper_wiring::PaperSettingsAdapter { tabs, workspace };
    #[cfg(test)]
    paper_panel_steps::record("trades-picker-open");
    if response.pick_trades_dir {
        settings.open_trades_dir_picker();
    }
    #[cfg(test)]
    paper_panel_steps::record("order-settings");
    if response.order_strategies_changed {
        settings.persist(crate::app::paper_wiring::PaperSettingsChange::OrderStrategies);
    }
    #[cfg(test)]
    paper_panel_steps::record("command-settings");
    if response.cmd_trading_changed {
        settings.persist(crate::app::paper_wiring::PaperSettingsChange::CmdTrading);
    }
    #[cfg(test)]
    paper_panel_steps::record("risk-settings");
    if response.risk_settings_changed {
        settings.persist(crate::app::paper_wiring::PaperSettingsChange::RiskSettings);
    }
}

/// Observe actual recipient checkpoints without replacing any host operation.
/// A checkpoint records reaching a conditional section, not taking its branch.
#[cfg(test)]
pub(in crate::app) mod paper_panel_steps {
    use std::cell::RefCell;

    thread_local! {
        static STEPS: RefCell<Option<Vec<&'static str>>> = const { RefCell::new(None) };
    }

    pub(in crate::app) fn record(step: &'static str) {
        STEPS.with_borrow_mut(|steps| {
            if let Some(steps) = steps {
                steps.push(step);
            }
        });
    }

    pub(crate) fn observe<R>(run: impl FnOnce() -> R) -> (R, Vec<&'static str>) {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                STEPS.with_borrow_mut(|steps| *steps = None);
            }
        }
        STEPS.with_borrow_mut(|steps| {
            assert!(steps.is_none(), "paper recipient observation is not nested");
            *steps = Some(Vec::new());
        });
        let _reset = Reset;
        let result = run();
        let steps = STEPS.with_borrow_mut(|steps| steps.take().unwrap());
        (result, steps)
    }
}
