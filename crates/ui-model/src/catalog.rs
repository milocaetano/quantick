//! One registration owns storage, effects and frame slots. Model and view
//! modules use conventional Rust declarations at their crate boundaries.
//! Register a surface with `models { [] [] field: Name => model_module = "model.rs",
//! view_module = "view.rs", effect_conversion, Slot, Host; }`. Implement View
//! and convert typed effects into the existing SurfaceResponse; its host
//! dispatch remains authoritative.
//! No projection is rebuilt by the adapter: the view borrows cached State.
#[macro_export]
macro_rules! surface_catalog {
    ($consumer:ident) => {
        $consumer! {
            modules {}
            models {
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_menu_dialogs_first: ChromeMenuDialogsFirst => chrome_menu_dialogs_first = "chrome/probe_menu_dialogs_first.rs", chrome_menu_dialogs_first_tests = "chrome_menu_dialogs_first_tests.rs", chrome_menu_dialogs_first_tests::effects, MenuDialogs, Local;
                [] []
                access_panel: AccessPanel => access_panel = "access_panel.rs", access_panel = "access_panel.rs", access_panel::effects, MenuDialogs, Local;
                [] []
                settings_window: SettingsWindow => settings_window = "settings_window.rs", settings_window = "settings_window.rs", settings_window::effects, Settings, Local;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_menu_dialogs_second: ChromeMenuDialogsSecond => chrome_menu_dialogs_second = "chrome/probe_menu_dialogs_second.rs", chrome_menu_dialogs_second_tests = "chrome_menu_dialogs_second_tests.rs", chrome_menu_dialogs_second_tests::effects, MenuDialogs, Local;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_editor_first: ChromeEditorFirst => chrome_editor_first = "chrome/probe_editor_first.rs", chrome_editor_first_tests = "chrome_editor_first_tests.rs", chrome_editor_first_tests::effects, EditorDialogs, Local;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_editor_second: ChromeEditorSecond => chrome_editor_second = "chrome/probe_editor_second.rs", chrome_editor_second_tests = "chrome_editor_second_tests.rs", chrome_editor_second_tests::effects, EditorDialogs, Local;
                [] []
                indicator_settings: IndicatorSettings => indicator_settings = "indicator_settings.rs", indicator_settings = "indicator_settings.rs", indicator_settings::effects, EditorDialogs, Local;
                [] []
                alerts: Alerts => alerts = "alerts.rs", alerts = "alerts/mod.rs", alerts::effects, Overlay, Local;
                [] []
                layout_dialogs: LayoutDialogs => layout_dialogs = "chrome/layout_dialogs.rs", layout_dialogs = "chrome/layout_dialogs.rs", layout_dialogs::effects, Overlay, Local;
                [] []
                feed_notice: FeedNotice => feed_notice = "feed_notice.rs", feed_notice = "feed_notice.rs", feed_notice::effects, Overlay, Local;
                [] []
                strategy_describe: StrategyDescribe => strategy_describe = "strategy_describe.rs", strategy_describe = "strategy_describe/mod.rs", strategy_describe::effects, Overlay, Host;
                [] []
                strategy_popup: StrategyPopup => strategy_popup = "strategy_popup.rs", strategy_popup = "strategy_popup.rs", strategy_popup::effects, LeadingOverlay, Local;
                [] []
                toast: Toast => toast = "toast.rs", toast = "toast.rs", toast::effects, Overlay, Local;
                [] []
                script_editor: ScriptEditor => script_editor = "script_editor.rs", script_editor = "script_editor.rs", script_editor::effects, Overlay, Local;
                [] []
                remove_chart: RemoveChart => remove_chart = "remove_chart.rs", remove_chart = "remove_chart.rs", remove_chart::effects, Overlay, Local;
                [] []
                market_palette: MarketPalette => market_palette = "market_palette.rs", market_palette = "market_palette.rs", market_palette::effects, Overlay, Local;
                [] []
                indicator_preview: IndicatorPreview => indicator_preview = "indicator_preview.rs", indicator_preview = "indicator_preview.rs", indicator_preview::effects, Overlay, Local;
                [] []
                indicator_picker: IndicatorPicker => indicator_picker = "indicator_picker.rs", indicator_picker = "indicator_picker.rs", indicator_picker::effects, Overlay, Host;
                [] []
                home: Home => home = "home.rs", home = "home.rs", home::effects, HomeCanvas, Local;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_early_first: ChromeEarlyFirst => chrome_early_first = "chrome/probe_early_first.rs", chrome_early_first_tests = "chrome_early_first_tests.rs", chrome_early_first_tests::effects, EarlyEdgePanels, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_early_second: ChromeEarlySecond => chrome_early_second = "chrome/probe_early_second.rs", chrome_early_second_tests = "chrome_early_second_tests.rs", chrome_early_second_tests::effects, EarlyEdgePanels, Host;
                [] []
                replay_browser: ReplayBrowser => replay_browser = "replay_browser.rs", replay_browser = "replay_browser.rs", replay_browser::effects, EarlyEdgePanels, Local;
                [] []
                toolrail: ToolRail => tool_rail = "tool_rail.rs", toolrail = "../toolrail/mod.rs", toolrail::effects, EdgePanels, Local;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_edge_first: ChromeEdgeFirst => chrome_edge_first = "chrome/probe_edge_first.rs", chrome_edge_first_tests = "chrome_edge_first_tests.rs", chrome_edge_first_tests::effects, EdgePanels, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_edge_second: ChromeEdgeSecond => chrome_edge_second = "chrome/probe_edge_second.rs", chrome_edge_second_tests = "chrome_edge_second_tests.rs", chrome_edge_second_tests::effects, EdgePanels, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                borrowed_first: BorrowedFirst => borrowed_first = "chrome/borrowed_first.rs", borrowed_first_tests = "borrowed_first_tests.rs", borrowed_first_tests::effects, LateEdgePanels, Local;
                [] []
                dock: Dock => dock = "dock.rs", dock = "dock.rs", dock::effects, LateEdgePanels | FrameTailPaperPanels, Local;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                paper_tail_probe: PaperTailProbe => paper_tail_probe = "chrome/paper_tail_probe.rs", paper_tail_probe_tests = "paper_tail_probe_tests.rs", paper_tail_probe_tests::effects, FrameTailPaperPanels, Local;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                borrowed_second: BorrowedSecond => borrowed_second = "chrome/borrowed_second.rs", borrowed_second_tests = "borrowed_second_tests.rs", borrowed_second_tests::effects, LateEdgePanels | PinnedInspector, Local;
                [] []
                drawing_chrome: DrawingChrome => drawing_chrome = "drawing_chrome.rs", drawing_chrome = "drawing_chrome/mod.rs", drawing_chrome::effects, PinnedInspector | DrawingChrome, Local;
                [] []
                chart_settings: ChartSettings => chart_settings = "chart_settings.rs", chart_settings = "chart_settings/mod.rs", chart_settings::effects, Overlay, Host;
                [] []
                bar_switch: BarSwitch => bar_switch = "bar_switch.rs", bar_switch = "bar_switch.rs", bar_switch::effects, Overlay, Local;
                [] []
                agent_popup: AgentPopup => agent_popup = "agent_popup.rs", agent_popup = "agent_popup.rs", agent_popup::effects, Overlay, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_title_first: ChromeTitleFirst => chrome_title_first = "chrome/probe_title_first.rs", chrome_title_first_tests = "chrome_title_first_tests.rs", chrome_title_first_tests::effects, TitleBar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_title_second: ChromeTitleSecond => chrome_title_second = "chrome/probe_title_second.rs", chrome_title_second_tests = "chrome_title_second_tests.rs", chrome_title_second_tests::effects, TitleBar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_toolbar_first: ChromeToolbarFirst => chrome_toolbar_first = "chrome/probe_toolbar_first.rs", chrome_toolbar_first_tests = "chrome_toolbar_first_tests.rs", chrome_toolbar_first_tests::effects, Toolbar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_toolbar_second: ChromeToolbarSecond => chrome_toolbar_second = "chrome/probe_toolbar_second.rs", chrome_toolbar_second_tests = "chrome_toolbar_second_tests.rs", chrome_toolbar_second_tests::effects, Toolbar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_menu_first: ChromeMenuFirst => chrome_menu_first = "chrome/probe_menu_first.rs", chrome_menu_first_tests = "chrome_menu_first_tests.rs", chrome_menu_first_tests::effects, MenuBar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_menu_second: ChromeMenuSecond => chrome_menu_second = "chrome/probe_menu_second.rs", chrome_menu_second_tests = "chrome_menu_second_tests.rs", chrome_menu_second_tests::effects, MenuBar, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_tabs_first: ChromeTabsFirst => chrome_tabs_first = "chrome/probe_tabs_first.rs", chrome_tabs_first_tests = "chrome_tabs_first_tests.rs", chrome_tabs_first_tests::effects, TabStrip, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                chrome_tabs_second: ChromeTabsSecond => chrome_tabs_second = "chrome/probe_tabs_second.rs", chrome_tabs_second_tests = "chrome_tabs_second_tests.rs", chrome_tabs_second_tests::effects, TabStrip, Host;

                [] []
                chrome_menu_bar: ChromeMenuBar => chrome_menu_bar = "chrome/menu_bar.rs", chrome_menu_bar = "chrome/menu_bar.rs", chrome_menu_bar::effects, MenuBar, Host;
                [] []
                chrome_title_bar: ChromeTitleBar => chrome_title_bar = "chrome/title_bar.rs", chrome_title_bar = "chrome/title_bar.rs", chrome_title_bar::effects, TitleBar, Host;
                [] []
                chrome_tab_strip: ChromeTabStrip => chrome_tab_strip = "chrome/tab_strip.rs", chrome_tab_strip = "chrome/tab_strip.rs", chrome_tab_strip::effects, TabStrip, Host;
                [] []
                canvas_popovers: CanvasPopovers => canvas_popovers = "chrome/canvas_popovers.rs", canvas_popovers = "chrome/canvas_popovers.rs", canvas_popovers::effects, Toolbar, Local;
                [] []
                chrome_toolbar: ChromeToolbar => chrome_toolbar = "chrome/toolbar.rs", chrome_toolbar = "chrome/toolbar.rs", chrome_toolbar::effects, Toolbar, Host;
                [] []
                pane_header: PaneHeader => pane_header = "pane_header.rs", pane_header = "../pane_header.rs", pane_header::effects, PaneHeader, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                first: First => first = "first.rs", first_view_tests = "first_view_tests.rs", first_view_tests::effects, Overlay | PinnedInspector | DrawingChrome, Host;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                second: Second => second = "second.rs", second_view_tests = "second_view_tests.rs", second_view_tests::effects, Toolbar, Local;
            }
        }
    };
}
