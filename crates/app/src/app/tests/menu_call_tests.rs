//! A menu command with a named call and that call leave the window the same:
//! both reach the owner through the port the call's handler takes.
use super::*;

use super::super::menu_bar::{MenuCall, MenuCommand, apply_menu_command_on};
use crate::control::ActionOrigin;
use quantick_control_handlers::desk::DeskAct;
use quantick_session::paper::TradeCommand;
use serde_json::{Value, json};

/// What a case reads back once the click or the call has run.
type Reading = fn(&mut QuantickApp) -> String;

fn layout(app: &mut QuantickApp) -> String {
    format!("{:?}", app.active_tab().canvas.layout)
}

fn strip(app: &mut QuantickApp) -> String {
    format!("{:?}", app.runtime.layout_state().layouts().layouts().len())
}

fn context_order(app: &mut QuantickApp) -> String {
    let panes = app.active_tab().panes().map(|(pane, _)| pane.id);
    format!("{:?}", panes.collect::<Vec<_>>())
}

fn picker(app: &mut QuantickApp) -> String {
    format!("{:?}", app.runtime.workspace.picker_mut().is_open())
}

fn alerts(app: &mut QuantickApp) -> String {
    format!("{:?}", app.surfaces.alerts.is_open())
}

fn flow_panel(app: &mut QuantickApp) -> String {
    format!("{:?}", app.active_tab().tape().settings_panel.is_open())
}

fn position(app: &mut QuantickApp) -> String {
    format!("{:?}", app.active_tab().paper.is_flat())
}

fn layer(app: &mut QuantickApp) -> String {
    let pane = app.focused_pane();
    format!(
        "{:?}",
        pane.layer_visible(ChartLayer::Footprint, &app.runtime.style)
    )
}

/// One case: the menu command, its call, the call's input, the reading.
type Case = (fn() -> MenuCall, &'static str, Value, Reading);

/// Each mapped command next to its call: the click on one window, the call
/// on a twin, the same reading on both, and a reading that moved. A `null`
/// `tab_id` or `pane_id` in a case's input is filled with the active tab and
/// its focused pane.
#[test]
fn a_menu_call_and_its_named_call_leave_the_same_window() {
    let preset = &crate::canvas_layout::LAYOUT_PRESETS[2];
    let section = crate::ui_state::PanelSection::L2;
    let cases: [Case; 9] = [
        (|| MenuCall::OpenAlerts, "alerts.open", json!({}), alerts),
        (
            || MenuCall::ApplyLayoutPreset(&crate::canvas_layout::LAYOUT_PRESETS[2]),
            "layout.preset.apply",
            json!({ "preset_id": preset.id }),
            layout,
        ),
        (
            || MenuCall::Strip(crate::layout_strip::StripAction::Create),
            "layout.tab.create",
            json!({}),
            strip,
        ),
        (
            || MenuCall::MoveContextChart { from: 1, to: 0 },
            "layout.pane.move",
            json!({ "from": "1", "to": "0" }),
            context_order,
        ),
        (
            || MenuCall::Desk(DeskAct::PickExport),
            "desk.export",
            json!({}),
            picker,
        ),
        (
            || MenuCall::Desk(DeskAct::PickImport),
            "desk.import",
            json!({}),
            picker,
        ),
        (
            || MenuCall::OpenOrderFlowPanel(crate::ui_state::PanelSection::L2),
            "orderflow.settings.open",
            json!({ "tab_id": null, "section": section }),
            flow_panel,
        ),
        (
            || {
                MenuCall::Trade(TradeCommand::Market {
                    side: quantick_engine::Side::Buy,
                })
            },
            "trade.order.place",
            json!({ "side": "buy", "kind": "market", "quantity": "1" }),
            position,
        ),
        (
            || MenuCall::ToggleLayer(ChartLayer::Footprint),
            "layers.visibility.set",
            json!({ "tab_id": null, "pane_id": null, "layer_id": ChartLayer::Footprint.id(), "visible": false }),
            layer,
        ),
    ];
    let ctx = egui::Context::default();
    for (call, capability, mut input, read) in cases {
        let (mut clicked, _, _, _) = test_app();
        let (mut called, _, _, _) = test_app();
        for app in [&mut clicked, &mut called] {
            // Two context charts beside the flow pane, so a move has room.
            crate::tab::layout::pick(
                app.active_tab_mut(),
                &crate::canvas_layout::LAYOUT_PRESETS[3],
            );
            // One print, so a market entry has a mark to fill at.
            let print = quantick_engine::Trade {
                agg_id: 1,
                timestamp_ms: 1_700_000_000_000,
                price: rust_decimal::Decimal::ONE_HUNDRED,
                quantity: rust_decimal::Decimal::ONE,
                side: quantick_engine::Side::Buy,
            };
            app.active_tab_mut()
                .ingest_live_trade_at(&print, print.timestamp_ms);
            run_frame(app, &ctx);
        }
        let before = read(&mut clicked);
        apply_menu_command_on(&mut clicked, &ctx, MenuCommand::Call(call()));
        if input.get("tab_id").is_some() {
            input["tab_id"] = json!(called.runtime.tabs.active_id().to_string());
        }
        if input.get("pane_id").is_some() {
            input["pane_id"] = json!(called.focused_pane().id.to_string());
        }
        called
            .control_action(capability, 1, ActionOrigin::Human, input)
            .unwrap_or_else(|error| panic!("{capability}: {error:?}"));
        for app in [&mut clicked, &mut called] {
            run_frame(app, &ctx);
        }
        let after = read(&mut clicked);
        assert_ne!(before, after, "{capability}: the click changed nothing");
        assert_eq!(
            after,
            read(&mut called),
            "{capability}: click and call differ"
        );
    }
}
