//! Reconcile one explicitly requested launch market with a remembered cockpit.
use super::{SavedTab, Workspace};
use crate::config::{AppConfig, DeclaredLayout};

/// The first document is adopted by the already-streaming startup tab. Move a
/// matching saved tab there intact, or add the new market with feed defaults.
/// Other tabs and installation settings survive; implicit startup is unchanged.
#[must_use]
pub fn reconcile_startup(
    mut workspace: Workspace,
    config: &AppConfig,
    explicit: bool,
) -> Workspace {
    if !explicit {
        return workspace;
    }
    let selected = workspace
        .tabs
        .iter()
        .position(|tab| tab.feed == config.default_feed && tab.symbol == config.default_symbol);
    let tab = selected.map_or_else(
        || {
            let feed = config
                .feed(&config.default_feed)
                .expect("validated startup feed");
            SavedTab {
                feed: config.default_feed.clone(),
                symbol: config.default_symbol.clone(),
                layout: feed.default_layout.unwrap_or(DeclaredLayout::Flow),
                split_fraction: None,
                context_collapsed: false,
                flow_collapsed: false,
                context_heights: Vec::new(),
                context_panes_collapsed: Vec::new(),
                focus: None,
                focus_slot: 0,
                flow_bars: feed
                    .default_bars
                    .clone()
                    .unwrap_or_else(|| "tick:50".to_owned()),
                time_bars: None,
                context_bars: Vec::new(),
                flow_layout: None,
                context_layouts: Vec::new(),
                flow_legend_collapsed: false,
                time_legend_collapsed: false,
            }
        },
        |index| workspace.tabs.remove(index),
    );
    workspace.tabs.insert(0, tab);
    workspace.active_tab = 0;
    workspace
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AppConfig {
        let mut config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
        config.default_feed = "binance-futures".to_owned();
        config.default_symbol = "BTCUSDT.p".to_owned();
        config
    }

    fn tab(feed: &str, symbol: &str) -> SavedTab {
        toml::from_str(&format!("feed = '{feed}'\nsymbol = '{symbol}'\nlayout = 'time+flow'\nflow_bars = 'tick:123'\nsplit_fraction = 0.31\ncontext_bars = ['time:5m', 'time:1h']\ncontext_layouts = [7, 9]\nflow_layout = 11\n")).unwrap()
    }

    #[test]
    fn explicit_matching_market_preserves_custom_panes_and_looks() {
        let spot = tab("binance", "BTCUSDT");
        let futures = tab("binance-futures", "BTCUSDT.p");
        let workspace = Workspace::new(
            false,
            Some([900.0, 600.0]),
            0,
            vec![spot.clone(), futures.clone()],
            None,
        );
        let restored = reconcile_startup(workspace, &config(), true);
        assert_eq!(restored.tabs, vec![futures, spot]);
        assert_eq!(restored.active_tab, 0);
        assert!(!restored.save_on_exit);
        assert_eq!(restored.window, Some([900.0, 600.0]));
    }

    #[test]
    fn explicit_new_market_keeps_other_tabs_and_implicit_startup_is_identical() {
        let spot = tab("binance", "BTCUSDT");
        let workspace = Workspace::new(true, None, 0, vec![spot.clone()], None);
        assert_eq!(
            reconcile_startup(workspace.clone(), &config(), false),
            workspace
        );
        let restored = reconcile_startup(workspace, &config(), true);
        assert_eq!(
            restored.first_market(),
            Some(("binance-futures", "BTCUSDT.p"))
        );
        assert_eq!(restored.tabs[1], spot);
        assert_eq!(restored.active_tab, 0);
    }

    /// Review #25: the factory spec has one owner, the one boot falls back
    /// to, so a feed with no `default_bars` opens on it here too.
    #[test]
    fn a_feed_without_default_bars_opens_on_the_factory_spec_boot_uses() {
        let mut config = config();
        for feed in &mut config.feeds {
            feed.default_bars = None;
        }
        let workspace = Workspace::new(true, None, 0, Vec::new(), None);
        let restored = reconcile_startup(workspace, &config, true);
        assert_eq!(
            restored.tabs[0].flow_bars,
            crate::config::factory_flow_spec().to_config_string()
        );
        assert_eq!(
            config.opening_spec_for(&config.default_feed),
            crate::config::factory_flow_spec()
        );
    }
}
