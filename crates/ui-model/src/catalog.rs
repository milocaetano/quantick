//! One registration owns module declarations, storage, effects and frame slot.
//! Migrate a legacy row to `models { [] [] field: model_module = "model.rs",
//! view_module = "view.rs", effect_conversion, Slot; }`. Model files live beside lib.rs; view files
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
                agent_popup: AgentPopupSurface => Surface::draw, ignore_keyboard;
                bar_switch: BarSwitchSurface => Surface::draw, ignore_keyboard;
                chart_settings: ChartSettingsSurface => Surface::draw, ignore_keyboard;
                indicator_preview: IndicatorPreviewSurface => Surface::draw, ignore_keyboard;
                market_palette: MarketPaletteSurface => Surface::draw, ignore_keyboard;
                strategy_popup: StrategyPopupSurface => Surface::draw, ignore_keyboard;
                toast: ToastSurface => Surface::draw, ignore_keyboard;
            }
            modules { drawing_chrome; home; remove_chart; }
            models {
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                first: first = "first.rs", first_view = "fixtures/first.rs", first_view::effects, Overlay;
                [cfg(any(test, feature = "test-support"))] [cfg(test)]
                second: second = "second.rs", second_view = "fixtures/second.rs", second_view::effects, Toolbar;
            }
        }
    };
}
