//! Compile-time family rows. Bindings are identifiers interpreted by each
//! consumer, never paths into a higher crate. Empty groups explicitly declare
//! that a family has no binding for that concern.

/// The declared binding names for one family, in source order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FamilySpec {
    pub id: &'static str,
    pub schemas: &'static [&'static str],
    pub actions: &'static [&'static str],
    pub projections: &'static [&'static str],
    pub authority: &'static [&'static str],
    pub readbacks: &'static [&'static str],
    pub behaviours: &'static [&'static str],
    pub samples: &'static [&'static str],
}

/// Expand the explicitly id-sorted rows through a consumer's local macro.
///
/// Each group contains binding identifiers, not functions or types owned by
/// this crate. The consumer resolves only the groups it needs in its own
/// dependency graph. An empty group declares no binding for that concern.
#[macro_export]
macro_rules! control_families {
    ($consumer:ident) => {
        $consumer! {
            "alerts" { schemas: [alerts], actions: [alerts], projections: [], authority: [alerts], readbacks: [alerts], behaviours: [], samples: [alerts], }
            "analysis" { schemas: [analysis], actions: [], projections: [analysis], authority: [], readbacks: [analysis], behaviours: [], samples: [], }
            "annotate" { schemas: [annotate], actions: [annotate], projections: [], authority: [annotate], readbacks: [], behaviours: [], samples: [], }
            "appearance" { schemas: [appearance], actions: [appearance], projections: [appearance], authority: [], readbacks: [appearance], behaviours: [], samples: [appearance], }
            "attention" { schemas: [attention], actions: [attention], projections: [], authority: [attention], readbacks: [attention], behaviours: [], samples: [], }
            "bubble_preset" { schemas: [bubble_preset], actions: [bubble_preset], projections: [], authority: [], readbacks: [bubble_preset], behaviours: [], samples: [bubble_preset], }
            "bubble_save" { schemas: [], actions: [bubble_save], projections: [], authority: [], readbacks: [bubble_save], behaviours: [], samples: [bubble_save], }
            "bubble_scale" { schemas: [bubble_scale], actions: [bubble_scale], projections: [], authority: [], readbacks: [bubble_scale], behaviours: [], samples: [bubble_scale], }
            "canvas" { schemas: [canvas], actions: [canvas], projections: [], authority: [canvas], readbacks: [canvas], behaviours: [], samples: [canvas], }
            "chart" { schemas: [chart], actions: [], projections: [chart], authority: [chart], readbacks: [], behaviours: [], samples: [], }
            "chart_canvas" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [chart_canvas], samples: [], }
            "chart_context" { schemas: [chart_context], actions: [chart_context], projections: [], authority: [], readbacks: [], behaviours: [], samples: [], }
            "chart_history" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [chart_history], samples: [], }
            "chart_settings" { schemas: [chart_settings], actions: [chart_settings], projections: [], authority: [], readbacks: [chart_settings], behaviours: [], samples: [chart_settings], }
            "cockpit" { schemas: [], actions: [], projections: [], authority: [cockpit], readbacks: [], behaviours: [], samples: [], }
            "contract_roll" { schemas: [contract_roll], actions: [contract_roll], projections: [], authority: [], readbacks: [], behaviours: [], samples: [], }
            "control" { schemas: [], actions: [], projections: [], authority: [control], readbacks: [], behaviours: [], samples: [], }
            "control_chrome" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [control_chrome], samples: [], }
            "control_feed_recovery" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [control_feed_recovery], samples: [], }
            "control_layout_tabs" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [control_layout_tabs], samples: [], }
            "control_window_and_tabs" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [control_window_and_tabs], samples: [], }
            "deal_recording" { schemas: [], actions: [deal_recording], projections: [], authority: [], readbacks: [], behaviours: [], samples: [], }
            "desk" { schemas: [desk], actions: [desk], projections: [], authority: [desk], readbacks: [desk], behaviours: [], samples: [desk], }
            "drawing_magnet" { schemas: [drawing_magnet], actions: [drawing_magnet], projections: [], authority: [], readbacks: [drawing_magnet], behaviours: [], samples: [drawing_magnet], }
            "drawing_scope" { schemas: [drawing_scope], actions: [], projections: [], authority: [], readbacks: [drawing_scope], behaviours: [], samples: [drawing_scope], }
            "drawing_text" { schemas: [], actions: [drawing_text], projections: [], authority: [], readbacks: [drawing_text], behaviours: [], samples: [drawing_text], }
            "drawings_objects" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [drawings_objects], samples: [], }
            "drawings_tool_rail" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [drawings_tool_rail], samples: [], }
            "events" { schemas: [events], actions: [], projections: [], authority: [events], readbacks: [], behaviours: [], samples: [], }
            "evidence" { schemas: [evidence], actions: [], projections: [], authority: [evidence], readbacks: [], behaviours: [], samples: [], }
            "feed" { schemas: [feed], actions: [], projections: [feed], authority: [feed], readbacks: [feed], behaviours: [], samples: [feed], }
            "feed_source" { schemas: [feed_source], actions: [feed_source], projections: [], authority: [], readbacks: [], behaviours: [], samples: [], }
            "health" { schemas: [health], actions: [], projections: [health], authority: [health], readbacks: [], behaviours: [], samples: [], }
            "history" { schemas: [history], actions: [history], projections: [], authority: [], readbacks: [history], behaviours: [], samples: [history], }
            "home_tab" { schemas: [home_tab], actions: [home_tab], projections: [], authority: [home_tab], readbacks: [home_tab], behaviours: [], samples: [home_tab], }
            "indicator_guide" { schemas: [indicator_guide], actions: [indicator_guide], projections: [], authority: [], readbacks: [indicator_guide], behaviours: [], samples: [indicator_guide], }
            "indicator_picker" { schemas: [indicator_picker], actions: [indicator_picker], projections: [], authority: [], readbacks: [indicator_picker], behaviours: [], samples: [indicator_picker], }
            "indicators_rows" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [indicators_rows], samples: [], }
            "interaction" { schemas: [interaction], actions: [], projections: [interaction], authority: [interaction], readbacks: [], behaviours: [], samples: [], }
            "journal_sessions" { schemas: [journal_sessions], actions: [journal_sessions], projections: [], authority: [journal_sessions], readbacks: [journal_sessions], behaviours: [], samples: [journal_sessions], }
            "layers" { schemas: [layers], actions: [layers], projections: [layers], authority: [], readbacks: [layers], behaviours: [], samples: [layers], }
            "layers_rows" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [layers_rows], samples: [], }
            "layout" { schemas: [], actions: [layout], projections: [], authority: [layout], readbacks: [layout], behaviours: [], samples: [layout], }
            "layout_v2" { schemas: [layout_v2], actions: [], projections: [], authority: [], readbacks: [], behaviours: [], samples: [], }
            "market_palette" { schemas: [market_palette], actions: [market_palette], projections: [], authority: [market_palette], readbacks: [market_palette], behaviours: [], samples: [market_palette], }
            "market_windows" { schemas: [market_windows], actions: [market_windows], projections: [market_windows], authority: [], readbacks: [market_windows], behaviours: [], samples: [], }
            "native_indicator" { schemas: [native_indicator], actions: [native_indicator], projections: [], authority: [native_indicator], readbacks: [native_indicator], behaviours: [], samples: [], }
            "native_window" { schemas: [native_window], actions: [native_window], projections: [], authority: [], readbacks: [native_window], behaviours: [], samples: [], }
            "notify" { schemas: [notify], actions: [notify], projections: [], authority: [notify], readbacks: [notify], behaviours: [], samples: [], }
            "observer" { schemas: [], actions: [], projections: [], authority: [observer], readbacks: [], behaviours: [], samples: [], }
            "opening_scale" { schemas: [opening_scale], actions: [opening_scale], projections: [], authority: [], readbacks: [opening_scale], behaviours: [], samples: [opening_scale], }
            "orderflow" { schemas: [orderflow], actions: [], projections: [orderflow], authority: [orderflow], readbacks: [], behaviours: [], samples: [], }
            "orderflow_settings" { schemas: [orderflow_settings], actions: [orderflow_settings], projections: [], authority: [], readbacks: [orderflow_settings], behaviours: [], samples: [orderflow_settings], }
            "pnl_display" { schemas: [], actions: [pnl_display], projections: [], authority: [], readbacks: [pnl_display], behaviours: [], samples: [pnl_display], }
            "price_axis" { schemas: [price_axis], actions: [price_axis], projections: [], authority: [], readbacks: [price_axis], behaviours: [], samples: [price_axis], }
            "price_tag_progress" { schemas: [price_tag_progress], actions: [price_tag_progress], projections: [], authority: [], readbacks: [price_tag_progress], behaviours: [], samples: [price_tag_progress], }
            "protocol" { schemas: [protocol], actions: [], projections: [], authority: [], readbacks: [], behaviours: [], samples: [], }
            "recovery" { schemas: [], actions: [recovery], projections: [], authority: [recovery], readbacks: [], behaviours: [], samples: [], }
            "replay_rows" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [replay_rows], samples: [], }
            "scene" { schemas: [scene], actions: [], projections: [scene], authority: [], readbacks: [], behaviours: [], samples: [], }
            "script" { schemas: [script], actions: [script], projections: [], authority: [script], readbacks: [script], behaviours: [], samples: [], }
            "script_alarms" { schemas: [script_alarms], actions: [script_alarms], projections: [], authority: [], readbacks: [], behaviours: [], samples: [], }
            "script_editor" { schemas: [script_editor], actions: [script_editor], projections: [], authority: [], readbacks: [script_editor], behaviours: [], samples: [script_editor], }
            "script_editor_rows" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [script_editor_rows], samples: [], }
            "script_strategy" { schemas: [script_strategy], actions: [script_strategy], projections: [], authority: [script_strategy], readbacks: [script_strategy], behaviours: [], samples: [], }
            "session" { schemas: [session], actions: [], projections: [session], authority: [session], readbacks: [], behaviours: [], samples: [], }
            "session_reports" { schemas: [session_reports], actions: [session_reports], projections: [], authority: [], readbacks: [session_reports], behaviours: [], samples: [session_reports], }
            "snapshot" { schemas: [snapshot], actions: [], projections: [], authority: [snapshot], readbacks: [], behaviours: [], samples: [], }
            "system" { schemas: [system], actions: [], projections: [system], authority: [system], readbacks: [], behaviours: [], samples: [], }
            "tape_fold" { schemas: [], actions: [tape_fold], projections: [], authority: [], readbacks: [tape_fold], behaviours: [], samples: [tape_fold], }
            "tape_market_fold" { schemas: [], actions: [], projections: [], authority: [], readbacks: [tape_market_fold], behaviours: [], samples: [tape_market_fold], }
            "tape_view" { schemas: [tape_view], actions: [], projections: [], authority: [], readbacks: [tape_view], behaviours: [], samples: [tape_view], }
            "tape_view_set" { schemas: [], actions: [tape_view_set], projections: [], authority: [], readbacks: [tape_view_set], behaviours: [], samples: [tape_view_set], }
            "tick_bubbles" { schemas: [], actions: [], projections: [], authority: [], readbacks: [tick_bubbles], behaviours: [], samples: [tick_bubbles], }
            "time_axis" { schemas: [time_axis], actions: [time_axis], projections: [], authority: [], readbacks: [time_axis], behaviours: [], samples: [time_axis], }
            "trade" { schemas: [], actions: [trade], projections: [], authority: [trade], readbacks: [trade], behaviours: [], samples: [], }
            "trade_account" { schemas: [], actions: [], projections: [], authority: [], readbacks: [trade_account], behaviours: [], samples: [], }
            "trading_rows" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [trading_rows], samples: [], }
            "widget" { schemas: [widget], actions: [], projections: [], authority: [widget], readbacks: [widget], behaviours: [], samples: [widget], }
            "widgets" { schemas: [], actions: [widgets], projections: [], authority: [], readbacks: [], behaviours: [], samples: [], }
            "workspace" { schemas: [workspace], actions: [], projections: [workspace], authority: [workspace], readbacks: [], behaviours: [], samples: [], }
            "workspace_book" { schemas: [workspace_book], actions: [workspace_book], projections: [], authority: [workspace_book], readbacks: [workspace_book], behaviours: [], samples: [workspace_book], }
            "workspace_colour" { schemas: [workspace_colour], actions: [], projections: [], authority: [], readbacks: [workspace_colour], behaviours: [], samples: [workspace_colour], }
            "workspace_manage" { schemas: [workspace_manage], actions: [], projections: [], authority: [], readbacks: [workspace_manage], behaviours: [], samples: [workspace_manage], }
            "workspace_rows" { schemas: [], actions: [], projections: [], authority: [], readbacks: [], behaviours: [workspace_rows], samples: [], }
        }
    };
}

macro_rules! family_specs {
    ($($id:literal { $($group:ident: [$($binding:ident),*],)* })*) => {
        /// Families in explicit id order; no runtime discovery or sorting.
        pub const FAMILIES: &[FamilySpec] = &[
            $(FamilySpec {
                id: $id,
                $($group: &[$(stringify!($binding)),*],)*
            },)*
        ];
    };
}

control_families!(family_specs);

#[cfg(test)]
#[path = "families_tests.rs"]
mod tests;
