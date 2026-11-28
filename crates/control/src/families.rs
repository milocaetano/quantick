//! Compile-time bindings, resolved by consumers in their own crates.
//!
//! A family docks by adding its files and one row. Support modules also have
//! rows so module declarations have no second list. `wire: none` declares no
//! `wire::<family>` module; `wire: [..]` declares it and names the facets it
//! publishes, `catalog` then `readbacks`. Empty bindings opt out;
//! runtime-argument actions such as strategy_describe are registered by the host.

/// Pass the complete table and optional arguments to a local consumer macro.
/// Bindings are tokens, never dependencies from control into a higher crate.
#[macro_export]
macro_rules! control_families {
    ($($consumer:ident)::+ $(, $argument:expr)* $(,)?) => {
        $($consumer)::+! {
            [$($argument),*]
            addressed_pane { wire: none, handlers: [pub addressed_pane], actions: [] }
            alerts { wire: [catalog, readbacks], handlers: [pub alerts], actions: [register] }
            analysis { wire: [readbacks], handlers: [pub analysis], actions: [] }
            annotate { wire: none, handlers: [pub annotate], actions: [register] }
            appearance { wire: [catalog, readbacks], handlers: [pub appearance], actions: [register_actions] }
            attention { wire: [readbacks], handlers: [pub attention], actions: [register] }
            axis_pane { wire: none, handlers: [pub axis_pane], actions: [] }
            bubble_pane { wire: none, handlers: [pub bubble_pane], actions: [] }
            bubble_preset { wire: [catalog, readbacks], handlers: [pub bubble_preset], actions: [register_tick, register] }
            bubble_save { wire: [readbacks], handlers: [pub bubble_save], actions: [register] }
            bubble_scale { wire: [catalog, readbacks], handlers: [pub bubble_scale], actions: [register] }
            calls { wire: [], handlers: [], actions: [] }
            canvas { wire: [catalog, readbacks], handlers: [pub canvas], actions: [register] }
            catalog { wire: [], handlers: [], actions: [] }
            chart { wire: [], handlers: [pub chart], actions: [] }
            chart_context { wire: [catalog], handlers: [pub chart_context], actions: [register] }
            chart_layer { wire: none, handlers: [pub chart_layer], actions: [] }
            chart_settings { wire: [catalog, readbacks], handlers: [pub chart_settings], actions: [register] }
            connection { wire: none, handlers: [pub connection], actions: [] }
            contract_roll { wire: [catalog], handlers: [pub contract_roll], actions: [register] }
            deal_recorder { wire: none, handlers: [pub deal_recorder], actions: [] }
            deal_recording { wire: [], handlers: [pub deal_recording], actions: [register] }
            desk { wire: [catalog, readbacks], handlers: [pub desk], actions: [register] }
            dock { wire: none, handlers: [pub dock], actions: [] }
            drawing_autosave { wire: [catalog, readbacks], handlers: [pub drawing_autosave], actions: [register] }
            drawing_magnet { wire: [readbacks], handlers: [pub drawing_magnet], actions: [register] }
            drawing_position { wire: [readbacks], handlers: [pub drawing_position], actions: [register] }
            drawing_scope { wire: [readbacks], handlers: [pub drawing_scope], actions: [] }
            drawing_text { wire: [readbacks], handlers: [pub drawing_text], actions: [register] }
            evidence { wire: [], handlers: [], actions: [] }
            families { wire: none, handlers: [families], actions: [] }
            feed { wire: [readbacks], handlers: [], actions: [] }
            feed_source { wire: [catalog], handlers: [pub feed_source], actions: [register] }
            feed_status { wire: none, handlers: [pub feed_status], actions: [] }
            health { wire: [], handlers: [pub health], actions: [] }
            history { wire: [catalog, readbacks], handlers: [pub history], actions: [register] }
            home_tab { wire: [catalog, readbacks], handlers: [pub home_tab], actions: [register] }
            indicator_guide { wire: [readbacks], handlers: [pub indicator_guide], actions: [register] }
            indicator_host { wire: none, handlers: [pub indicator_host], actions: [] }
            indicator_picker { wire: [catalog, readbacks], handlers: [pub indicator_picker], actions: [register] }
            interaction { wire: [], handlers: [], actions: [] }
            journal_sessions { wire: [catalog, readbacks], handlers: [pub journal_sessions], actions: [register] }
            launch_calls { wire: [], handlers: [], actions: [] }
            launch_values { wire: [], handlers: [], actions: [] }
            layers { wire: [readbacks], handlers: [pub layers], actions: [register] }
            layout { wire: [readbacks], handlers: [pub layout], actions: [register] }
            layout_v2 { wire: [], handlers: [], actions: [] }
            market_palette { wire: [catalog, readbacks], handlers: [pub market_palette], actions: [register] }
            market_switch { wire: none, handlers: [pub market_switch], actions: [] }
            market_tab { wire: none, handlers: [pub market_tab], actions: [] }
            market_windows { wire: [catalog, readbacks], handlers: [pub market_windows], actions: [register] }
            native_indicator { wire: [catalog, readbacks], handlers: [pub native_indicator], actions: [register] }
            native_window { wire: [catalog, readbacks], handlers: [pub native_window], actions: [register] }
            notify { wire: [readbacks], handlers: [pub notify], actions: [register] }
            observer { wire: none, handlers: [pub observer], actions: [] }
            opening_scale { wire: [catalog, readbacks], handlers: [pub opening_scale], actions: [register] }
            orderflow { wire: [], handlers: [pub orderflow], actions: [] }
            orderflow_settings { wire: [readbacks], handlers: [pub orderflow_settings], actions: [register] }
            pane_marks { wire: none, handlers: [pub pane_marks], actions: [] }
            pnl_display { wire: [readbacks], handlers: [pub pnl_display], actions: [register] }
            price_axis { wire: [catalog, readbacks], handlers: [pub price_axis], actions: [register] }
            price_tag_progress { wire: [catalog, readbacks], handlers: [pub price_tag_progress], actions: [register] }
            readback { wire: [], handlers: [], actions: [] }
            recovery { wire: [], handlers: [pub recovery], actions: [register] }
            retry_matrix { wire: [], handlers: [pub retry_matrix], actions: [] }
            scene { wire: [catalog], handlers: [], actions: [] }
            schema_documents { wire: none, handlers: [pub schema_documents], actions: [] }
            script { wire: [readbacks], handlers: [pub script], actions: [register] }
            script_alarms { wire: [catalog], handlers: [pub script_alarms], actions: [register] }
            script_editor { wire: [catalog, readbacks], handlers: [pub script_editor], actions: [register] }
            script_strategy { wire: [handler_catalog_readbacks: script_strategy], handlers: [pub script_strategy], actions: [register] }
            semantic { wire: none, handlers: [pub semantic], actions: [] }
            session { wire: [catalog], handlers: [pub session], actions: [] }
            session_reports { wire: [catalog, readbacks], handlers: [pub session_reports], actions: [register] }
            settings { wire: none, handlers: [pub settings], actions: [] }
            settings_sections { wire: [], handlers: [], actions: [] }
            strategy_describe { wire: [catalog, readbacks], handlers: [], actions: [] }
            strategy_desk { wire: [handler_catalog_readbacks: strategy_desk], handlers: [pub strategy_desk], actions: [register] }
            tabs { wire: none, handlers: [tabs], actions: [] }
            tape_fold { wire: [readbacks], handlers: [pub tape_fold], actions: [register] }
            tape_market_fold { wire: [readbacks], handlers: [pub tape_market_fold], actions: [] }
            tape_view { wire: [catalog, readbacks], handlers: [], actions: [] }
            tape_view_set { wire: [readbacks], handlers: [pub tape_view_set], actions: [register] }
            tick_bubbles { wire: [readbacks], handlers: [], actions: [] }
            ticket_money { wire: none, handlers: [pub ticket_money], actions: [] }
            time_axis { wire: [catalog, readbacks], handlers: [pub time_axis], actions: [register] }
            trace_replay { wire: [], handlers: [], actions: [] }
            trade { wire: [handler_readbacks: trade], handlers: [pub trade], actions: [register] }
            trade_account { wire: [handler_readbacks: trade_account], handlers: [pub trade_account], actions: [] }
            ui_behaviours { wire: [], handlers: [], actions: [] }
            widget { wire: [catalog, readbacks], handlers: [], actions: [] }
            widgets { wire: none, handlers: [pub widgets], actions: [register] }
            window_menu { wire: [], handlers: [], actions: [] }
            workspace { wire: [], handlers: [], actions: [] }
            workspace_book { wire: [catalog, readbacks], handlers: [pub workspace_book], actions: [register] }
            workspace_colour { wire: [catalog, readbacks], handlers: [], actions: [] }
            workspace_manage { wire: [catalog, readbacks], handlers: [], actions: [] }
        }
    };
}
