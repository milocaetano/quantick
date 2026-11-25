//! Compile-time bindings, resolved by consumers in their own crates.
//!
//! A family docks by adding its files and one row. Support modules also have
//! rows so module declarations have no second list. Empty bindings opt out;
//! runtime-argument actions such as strategy_describe are registered by the host.

/// Pass the complete table and optional arguments to a local consumer macro.
/// Bindings are tokens, never dependencies from control into a higher crate.
#[macro_export]
macro_rules! control_families {
    ($($consumer:ident)::+ $(, $argument:expr)* $(,)?) => {
        $($consumer)::+! {
            [$($argument),*]
            addressed_pane { schemas: [], catalog: [], readbacks: [], handlers: [pub addressed_pane], actions: [] }
            alerts { schemas: [alerts], catalog: [alerts], readbacks: [alerts], handlers: [pub alerts], actions: [register] }
            analysis { schemas: [analysis], catalog: [], readbacks: [analysis], handlers: [pub analysis], actions: [] }
            annotate { schemas: [], catalog: [], readbacks: [], handlers: [pub annotate], actions: [register] }
            appearance { schemas: [appearance], catalog: [appearance], readbacks: [appearance], handlers: [pub appearance], actions: [register_actions] }
            attention { schemas: [attention], catalog: [], readbacks: [attention], handlers: [pub attention], actions: [register] }
            axis_pane { schemas: [], catalog: [], readbacks: [], handlers: [pub axis_pane], actions: [] }
            bubble_pane { schemas: [], catalog: [], readbacks: [], handlers: [pub bubble_pane], actions: [] }
            bubble_preset { schemas: [bubble_preset], catalog: [bubble_preset], readbacks: [bubble_preset], handlers: [pub bubble_preset], actions: [register_tick, register] }
            bubble_save { schemas: [bubble_save], catalog: [], readbacks: [bubble_save], handlers: [pub bubble_save], actions: [register] }
            bubble_scale { schemas: [bubble_scale], catalog: [bubble_scale], readbacks: [bubble_scale], handlers: [pub bubble_scale], actions: [register] }
            calls { schemas: [calls], catalog: [], readbacks: [], handlers: [], actions: [] }
            canvas { schemas: [canvas], catalog: [canvas], readbacks: [canvas], handlers: [pub canvas], actions: [register] }
            catalog { schemas: [catalog], catalog: [], readbacks: [], handlers: [], actions: [] }
            chart { schemas: [chart], catalog: [], readbacks: [], handlers: [pub chart], actions: [] }
            chart_context { schemas: [chart_context], catalog: [chart_context], readbacks: [], handlers: [pub chart_context], actions: [register] }
            chart_layer { schemas: [], catalog: [], readbacks: [], handlers: [pub chart_layer], actions: [] }
            chart_settings { schemas: [chart_settings], catalog: [chart_settings], readbacks: [chart_settings], handlers: [pub chart_settings], actions: [register] }
            connection { schemas: [], catalog: [], readbacks: [], handlers: [pub connection], actions: [] }
            contract_roll { schemas: [contract_roll], catalog: [contract_roll], readbacks: [], handlers: [pub contract_roll], actions: [register] }
            deal_recorder { schemas: [], catalog: [], readbacks: [], handlers: [pub deal_recorder], actions: [] }
            deal_recording { schemas: [deal_recording], catalog: [], readbacks: [], handlers: [pub deal_recording], actions: [register] }
            desk { schemas: [desk], catalog: [desk], readbacks: [desk], handlers: [pub desk], actions: [register] }
            dock { schemas: [], catalog: [], readbacks: [], handlers: [pub dock], actions: [] }
            drawing_autosave { schemas: [drawing_autosave], catalog: [drawing_autosave], readbacks: [drawing_autosave], handlers: [pub drawing_autosave], actions: [register] }
            drawing_magnet { schemas: [drawing_magnet], catalog: [], readbacks: [drawing_magnet], handlers: [pub drawing_magnet], actions: [register] }
            drawing_scope { schemas: [drawing_scope], catalog: [], readbacks: [drawing_scope], handlers: [pub drawing_scope], actions: [] }
            drawing_text { schemas: [drawing_text], catalog: [], readbacks: [drawing_text], handlers: [pub drawing_text], actions: [register] }
            evidence { schemas: [evidence], catalog: [], readbacks: [], handlers: [], actions: [] }
            families { schemas: [], catalog: [], readbacks: [], handlers: [families], actions: [] }
            feed { schemas: [feed], catalog: [], readbacks: [feed], handlers: [], actions: [] }
            feed_source { schemas: [feed_source], catalog: [feed_source], readbacks: [], handlers: [pub feed_source], actions: [register] }
            feed_status { schemas: [], catalog: [], readbacks: [], handlers: [pub feed_status], actions: [] }
            health { schemas: [health], catalog: [], readbacks: [], handlers: [pub health], actions: [] }
            history { schemas: [history], catalog: [history], readbacks: [history], handlers: [pub history], actions: [register] }
            home_tab { schemas: [home_tab], catalog: [home_tab], readbacks: [home_tab], handlers: [pub home_tab], actions: [register] }
            indicator_guide { schemas: [indicator_guide], catalog: [], readbacks: [indicator_guide], handlers: [pub indicator_guide], actions: [register] }
            indicator_host { schemas: [], catalog: [], readbacks: [], handlers: [pub indicator_host], actions: [] }
            indicator_picker { schemas: [indicator_picker], catalog: [indicator_picker], readbacks: [indicator_picker], handlers: [pub indicator_picker], actions: [register] }
            interaction { schemas: [interaction], catalog: [], readbacks: [], handlers: [], actions: [] }
            journal_sessions { schemas: [journal_sessions], catalog: [journal_sessions], readbacks: [journal_sessions], handlers: [pub journal_sessions], actions: [register] }
            launch_calls { schemas: [launch_calls], catalog: [], readbacks: [], handlers: [], actions: [] }
            launch_values { schemas: [launch_values], catalog: [], readbacks: [], handlers: [], actions: [] }
            layers { schemas: [layers], catalog: [], readbacks: [layers], handlers: [pub layers], actions: [register] }
            layout { schemas: [layout], catalog: [], readbacks: [layout], handlers: [pub layout], actions: [register] }
            layout_v2 { schemas: [layout_v2], catalog: [], readbacks: [], handlers: [], actions: [] }
            market_palette { schemas: [market_palette], catalog: [market_palette], readbacks: [market_palette], handlers: [pub market_palette], actions: [register] }
            market_switch { schemas: [], catalog: [], readbacks: [], handlers: [pub market_switch], actions: [] }
            market_tab { schemas: [], catalog: [], readbacks: [], handlers: [pub market_tab], actions: [] }
            market_windows { schemas: [market_windows], catalog: [market_windows], readbacks: [market_windows], handlers: [pub market_windows], actions: [register] }
            native_indicator { schemas: [native_indicator], catalog: [native_indicator], readbacks: [native_indicator], handlers: [pub native_indicator], actions: [register] }
            native_window { schemas: [native_window], catalog: [native_window], readbacks: [native_window], handlers: [pub native_window], actions: [register] }
            notify { schemas: [notify], catalog: [], readbacks: [notify], handlers: [pub notify], actions: [register] }
            observer { schemas: [], catalog: [], readbacks: [], handlers: [pub observer], actions: [] }
            opening_scale { schemas: [opening_scale], catalog: [opening_scale], readbacks: [opening_scale], handlers: [pub opening_scale], actions: [register] }
            orderflow { schemas: [orderflow], catalog: [], readbacks: [], handlers: [pub orderflow], actions: [] }
            orderflow_settings { schemas: [orderflow_settings], catalog: [], readbacks: [orderflow_settings], handlers: [pub orderflow_settings], actions: [register] }
            pane_marks { schemas: [], catalog: [], readbacks: [], handlers: [pub pane_marks], actions: [] }
            pnl_display { schemas: [pnl_display], catalog: [], readbacks: [pnl_display], handlers: [pub pnl_display], actions: [register] }
            price_axis { schemas: [price_axis], catalog: [price_axis], readbacks: [price_axis], handlers: [pub price_axis], actions: [register] }
            price_tag_progress { schemas: [price_tag_progress], catalog: [price_tag_progress], readbacks: [price_tag_progress], handlers: [pub price_tag_progress], actions: [register] }
            readback { schemas: [readback], catalog: [], readbacks: [], handlers: [], actions: [] }
            recovery { schemas: [recovery], catalog: [], readbacks: [], handlers: [pub recovery], actions: [register] }
            retry_matrix { schemas: [retry_matrix], catalog: [], readbacks: [], handlers: [pub retry_matrix], actions: [] }
            scene { schemas: [scene], catalog: [scene], readbacks: [], handlers: [], actions: [] }
            schema_documents { schemas: [], catalog: [], readbacks: [], handlers: [pub schema_documents], actions: [] }
            script { schemas: [script], catalog: [], readbacks: [script], handlers: [pub script], actions: [register] }
            script_alarms { schemas: [script_alarms], catalog: [script_alarms], readbacks: [], handlers: [pub script_alarms], actions: [register] }
            script_editor { schemas: [script_editor], catalog: [script_editor], readbacks: [script_editor], handlers: [pub script_editor], actions: [register] }
            script_strategy { schemas: [script_strategy], catalog: [script_strategy], readbacks: [script_strategy], handlers: [pub script_strategy], actions: [register] }
            semantic { schemas: [], catalog: [], readbacks: [], handlers: [pub semantic], actions: [] }
            session { schemas: [session], catalog: [session], readbacks: [], handlers: [pub session], actions: [] }
            session_reports { schemas: [session_reports], catalog: [session_reports], readbacks: [session_reports], handlers: [pub session_reports], actions: [register] }
            settings { schemas: [], catalog: [], readbacks: [], handlers: [pub settings], actions: [] }
            settings_sections { schemas: [settings_sections], catalog: [], readbacks: [], handlers: [], actions: [] }
            strategy_describe { schemas: [strategy_describe], catalog: [strategy_describe], readbacks: [strategy_describe], handlers: [], actions: [] }
            strategy_desk { schemas: [strategy_desk], catalog: [strategy_desk], readbacks: [strategy_desk], handlers: [pub strategy_desk], actions: [register] }
            tabs { schemas: [], catalog: [], readbacks: [], handlers: [tabs], actions: [] }
            tape_fold { schemas: [tape_fold], catalog: [], readbacks: [tape_fold], handlers: [pub tape_fold], actions: [register] }
            tape_market_fold { schemas: [tape_market_fold], catalog: [], readbacks: [tape_market_fold], handlers: [pub tape_market_fold], actions: [] }
            tape_view { schemas: [tape_view], catalog: [tape_view], readbacks: [tape_view], handlers: [], actions: [] }
            tape_view_set { schemas: [tape_view_set], catalog: [], readbacks: [tape_view_set], handlers: [pub tape_view_set], actions: [register] }
            tick_bubbles { schemas: [tick_bubbles], catalog: [], readbacks: [tick_bubbles], handlers: [], actions: [] }
            ticket_money { schemas: [], catalog: [], readbacks: [], handlers: [pub ticket_money], actions: [] }
            time_axis { schemas: [time_axis], catalog: [time_axis], readbacks: [time_axis], handlers: [pub time_axis], actions: [register] }
            trace_replay { schemas: [trace_replay], catalog: [], readbacks: [], handlers: [], actions: [] }
            trade { schemas: [trade], catalog: [], readbacks: [trade], handlers: [pub trade], actions: [register] }
            trade_account { schemas: [trade_account], catalog: [], readbacks: [trade_account], handlers: [pub trade_account], actions: [] }
            ui_behaviours { schemas: [ui_behaviours], catalog: [], readbacks: [], handlers: [], actions: [] }
            widget { schemas: [widget], catalog: [widget], readbacks: [widget], handlers: [], actions: [] }
            widgets { schemas: [], catalog: [], readbacks: [], handlers: [pub widgets], actions: [register] }
            window_menu { schemas: [window_menu], catalog: [], readbacks: [], handlers: [], actions: [] }
            workspace { schemas: [workspace], catalog: [], readbacks: [], handlers: [], actions: [] }
            workspace_book { schemas: [workspace_book], catalog: [workspace_book], readbacks: [workspace_book], handlers: [pub workspace_book], actions: [register] }
            workspace_colour { schemas: [workspace_colour], catalog: [workspace_colour], readbacks: [workspace_colour], handlers: [], actions: [] }
            workspace_manage { schemas: [workspace_manage], catalog: [workspace_manage], readbacks: [workspace_manage], handlers: [], actions: [] }
        }
    };
}
