use super::*;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_chart::style::ChartStyle;
use quantick_control_host::actions::ActionRegistry;
use quantick_control_schema::chart_settings::{ChartSettingsInput, ChartSettingsResult};
use quantick_stores::{
    candle_presets::PresetError,
    market_settings::{DrawingKey, MarketSettings, MarketSettingsStore},
};

struct Host {
    market: MarketSettingsStore,
    style: ChartStyle,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            market: MarketSettingsStore::new(
                MarketSettings::default(),
                "unused.toml".into(),
                false,
            ),
            style: ChartStyle::default(),
        }
    }
}
impl ChartStylePort for Host {
    fn chart_style(&self) -> &ChartStyle {
        &self.style
    }
    fn apply_chart_settings(
        &mut self,
        _: ChartSettingsInput,
    ) -> Result<ChartSettingsResult, PresetError> {
        unreachable!()
    }
    fn switch_chart_layer(&mut self, _: u64, _: u64, _: &str, _: bool) {
        unreachable!()
    }
    fn set_price_tag_progress(&mut self, _: u64, _: u64, _: bool) -> Result<bool, ControlError> {
        unreachable!()
    }
    fn drawing_autosave(
        &mut self,
        tab: u64,
        pane: u64,
        on: Option<bool>,
    ) -> Result<(bool, bool), ControlError> {
        if tab != 1 || pane > 1 {
            return Err(ControlError::invalid_request("unknown pane"));
        }
        let key = DrawingKey::of_pane("fixture", "asset".into(), None);
        let changed = on.is_some_and(|on| {
            self.market
                .set_drawing_autosave(&key, on, std::time::Instant::now())
        });
        Ok((self.market.drawing_autosave(&key), changed))
    }
}

#[test]
fn drawing_autosave_call_inspects_sets_and_retries() {
    let mut registry = ActionRegistry::<Host, FakeAccess>::new();
    register(&mut registry).unwrap();
    let action = registry
        .lookup(wire::DRAWING_AUTOSAVE_CAPABILITY_ID, 1)
        .unwrap();
    assert!(action.traced(true).is_err());
    let mut host = Host::default();
    let mut access = FakeAccess::with_budget(0);
    let actor = FakeWindow::assistant();
    let read = json!({"tab_id": "1", "pane_id": "0"});
    action.input.validate(&read).unwrap();
    let first = (action.handler)(&mut host, &mut access, &actor, &read).unwrap();
    action.output.validate(&first).unwrap();
    assert_eq!(first["on"], true);
    assert_eq!(first["changed"], false);
    assert!(access.events.is_empty());
    let off = json!({"tab_id": "1", "pane_id": "0", "on": false});
    let changed = (action.handler)(&mut host, &mut access, &actor, &off).unwrap();
    assert_eq!(changed["changed"], true);
    assert_eq!(
        (action.handler)(&mut host, &mut access, &actor, &off).unwrap()["changed"],
        false
    );
    assert_eq!(
        (action.handler)(&mut host, &mut access, &actor, &read).unwrap()["on"],
        false
    );
    assert_eq!(access.events.len(), 1);
    assert_eq!(access.events[0].payload["drawing_autosave"]["on"], false);
}

#[test]
fn malformed_or_missing_pane_is_refused_before_mutation() {
    let mut host = Host::default();
    let mut access = FakeAccess::with_budget(0);
    let actor = FakeWindow::assistant();
    for input in [
        json!({"tab_id": "9", "pane_id": "0", "on": false}),
        json!({"tab_id": "1", "pane_id": "9", "on": false}),
        json!({"tab_id": "1", "pane_id": "0", "on": "false"}),
        json!({"tab_id": "1", "pane_id": "0", "on": false, "extra": true}),
        json!({"tab_id": "1", "on": false}),
    ] {
        assert!(
            set(&mut host, &mut access, &actor, &input).is_err(),
            "{input}"
        );
    }
    assert!(access.events.is_empty());
    assert!(!host.market.owes_write());
}

#[test]
fn unavailable_persistence_is_retryable_and_identified() {
    let error = unavailable("the disk is full".into());
    assert!(error.retryable);
    assert_eq!(
        error.context.details.unwrap()["reason"],
        "drawing_autosave_not_written"
    );
}
