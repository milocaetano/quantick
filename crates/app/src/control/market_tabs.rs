//! The window's part in the feed and market calls: resolve the addressed tab
//! and lend the config, the layout's drawings and the History default.
use crate::app::arrangement_adapter;
use quantick_control_handlers::market_palette::{MarketPaletteOwners, MarketTabs, PaletteShowing};
use quantick_control_handlers::market_tab::{LentTab, MarketTabHost};
use quantick_stores::symbol_catalog::SymbolCatalog;
use quantick_ui_model::market_palette::Intent;
use quantick_workspace::market_list::{self as list, SwitchTarget};

use crate::app::QuantickApp;
use crate::app::control_host::{ControlPort, TabReads, TabsMut, TabsMutPort, TabsPort};

impl MarketTabHost for dyn ControlPort {
    fn market_tab_mut(&mut self, index: usize) -> Option<LentTab<'_>> {
        self.tabs_mut().lend_tab(index)
    }
    fn drawing_gesture_on(&self, feed_id: &str, from: &str, to: &str) -> bool {
        self.workspace()
            .layout_state()
            .drawing_gesture_on(feed_id, from, to)
    }
    fn carry_market_drawings(&mut self, feed_id: &str, from: &str, to: &str) -> usize {
        self.workspace_calls()
            .layout()
            .carry_market_drawings(feed_id, from, to)
    }
}

impl TabsPort for QuantickApp {
    fn tab_reads(&self) -> TabReads<'_> {
        let roots = crate::app::control_host::control_roots(self);
        roots
            .runtime
            .resolve(&roots.presentation.binding)
            .expect("bound workspace");
        TabReads::new(
            &roots.runtime.tabs,
            roots.config,
            &roots.runtime.footprint_config,
            &roots.drawings.presets,
        )
        .with_appearance(roots.runtime.appearance.resolved())
    }
}

impl TabsMutPort for QuantickApp {
    fn tabs_mut(&mut self) -> TabsMut<'_> {
        let roots = crate::app::control_host::control_roots_mut(self);
        roots
            .runtime
            .resolve(&roots.presentation.binding)
            .expect("bound workspace");
        let reach = &mut roots.runtime.history.history_reach;
        TabsMut::new(&mut roots.runtime.tabs, roots.config, reach)
    }
}

impl MarketPaletteOwners for QuantickApp {
    fn listed_markets(&self) -> list::MarketSnapshot {
        let roots = crate::app::control_host::control_roots(self);
        let lists = (roots.config, roots.added_symbols);
        let recent = &roots.surfaces.market_palette.model_state.recent;
        let book = &roots.runtime.workspace.book;
        arrangement_adapter::market_snapshot(&roots.runtime.tabs, lists, book, recent)
    }
    fn market_tabs(&mut self) -> Option<Box<dyn MarketTabs + '_>> {
        Some(Box::new(self.arrangement_adapter()))
    }
    fn market_catalog(&mut self) -> Option<SymbolCatalog<'_>> {
        let roots = crate::app::control_host::control_roots_mut(self);
        Some(SymbolCatalog {
            config: roots.config,
            added: roots.added_symbols,
            path: roots.runtime.workspace.symbols_path(),
        })
    }
    fn show_market_palette(&mut self, query: &str, target: Option<SwitchTarget>) -> PaletteShowing {
        let roots = crate::app::control_host::control_roots_mut(self);
        let palette = &mut roots.surfaces.market_palette;
        palette.update(Intent::open(query, target));
        Ok(())
    }
}
