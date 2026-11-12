//! One registration owns module declarations, storage, effects and frame slot.
//! Migrate a legacy row to `models { [] [] field: Name => model_module = "model.rs",
//! view_module = "view.rs", effect_conversion, Slot, Host; }`. Model files live beside lib.rs; view files
//! live under app/src/surfaces. Implement View and convert typed effects into
//! the existing SurfaceResponse; its host dispatch remains authoritative.
//! No projection is rebuilt by the adapter: the view borrows cached State.
#[macro_export]
macro_rules! surface_catalog {
    ($consumer:ident) => {
        $consumer! {
            legacy {
                indicator_picker: IndicatorPickerSurface => Surface::draw, indicator_picker::IndicatorPickerSurface::capture_keyboard;
                script_editor: ScriptEditorSurface => script_editor::draw, script_editor::ScriptEditorSurface::capture_keyboard;
                indicator_preview: IndicatorPreviewSurface => Surface::draw, ignore_keyboard;
                market_palette: MarketPaletteSurface => Surface::draw, ignore_keyboard;
                strategy_popup: StrategyPopupSurface => Surface::draw, ignore_keyboard;
                toast: ToastSurface => Surface::draw, ignore_keyboard;
            }
            modules { drawing_chrome; home; remove_chart; }
            models {
                [] []
                chart_settings: ChartSettings => chart_settings = "chart_settings.rs", chart_settings = "chart_settings/mod.rs", chart_settings::effects, Overlay, Host;
                [] []
                bar_switch: BarSwitch => bar_switch = "bar_switch.rs", bar_switch = "bar_switch.rs", bar_switch::effects, Overlay, Local;
                [] []
                agent_popup: AgentPopup => agent_popup = "agent_popup.rs", agent_popup = "agent_popup.rs", agent_popup::effects, Overlay, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_title_first: ChromeTitleFirst => chrome_title_first = "chrome/probe_title_first.rs", chrome_title_first = "chrome/tests/title_first.rs", chrome_title_first::effects, TitleBar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_title_second: ChromeTitleSecond => chrome_title_second = "chrome/probe_title_second.rs", chrome_title_second = "chrome/tests/title_second.rs", chrome_title_second::effects, TitleBar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_toolbar_first: ChromeToolbarFirst => chrome_toolbar_first = "chrome/probe_toolbar_first.rs", chrome_toolbar_first = "chrome/tests/toolbar_first.rs", chrome_toolbar_first::effects, Toolbar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_toolbar_second: ChromeToolbarSecond => chrome_toolbar_second = "chrome/probe_toolbar_second.rs", chrome_toolbar_second = "chrome/tests/toolbar_second.rs", chrome_toolbar_second::effects, Toolbar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_menu_first: ChromeMenuFirst => chrome_menu_first = "chrome/probe_menu_first.rs", chrome_menu_first = "chrome/tests/menu_first.rs", chrome_menu_first::effects, MenuBar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_menu_second: ChromeMenuSecond => chrome_menu_second = "chrome/probe_menu_second.rs", chrome_menu_second = "chrome/tests/menu_second.rs", chrome_menu_second::effects, MenuBar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_tabs_first: ChromeTabsFirst => chrome_tabs_first = "chrome/probe_tabs_first.rs", chrome_tabs_first = "chrome/tests/tabs_first.rs", chrome_tabs_first::effects, TabStrip, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_tabs_second: ChromeTabsSecond => chrome_tabs_second = "chrome/probe_tabs_second.rs", chrome_tabs_second = "chrome/tests/tabs_second.rs", chrome_tabs_second::effects, TabStrip, Host;

                [] []
                chrome_menu_bar: ChromeMenuBar => chrome_menu_bar = "chrome/menu_bar.rs", chrome_menu_bar = "chrome/menu_bar.rs", chrome_menu_bar::effects, MenuBar, Host;
                [] []
                chrome_title_bar: ChromeTitleBar => chrome_title_bar = "chrome/title_bar.rs", chrome_title_bar = "chrome/title_bar.rs", chrome_title_bar::effects, TitleBar, Host;
                [] []
                chrome_tab_strip: ChromeTabStrip => chrome_tab_strip = "chrome/tab_strip.rs", chrome_tab_strip = "chrome/tab_strip.rs", chrome_tab_strip::effects, TabStrip, Host;
                [] []
                chrome_toolbar: ChromeToolbar => chrome_toolbar = "chrome/toolbar.rs", chrome_toolbar = "chrome/toolbar.rs", chrome_toolbar::effects, Toolbar, Host;
                [] []
                pane_header: PaneHeader => pane_header = "pane_header.rs", pane_header = "../pane_header.rs", pane_header::effects, PaneHeader, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                first: First => first = "first.rs", first_view = "tests/fixtures/first.rs", first_view::effects, Overlay, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                second: Second => second = "second.rs", second_view = "tests/fixtures/second.rs", second_view::effects, Toolbar, Local;
            }
        }
    };
}
