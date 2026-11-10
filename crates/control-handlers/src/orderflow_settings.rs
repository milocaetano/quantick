//! `orderflow.settings.*` over the panel's narrow port.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::orderflow_settings::{
    AssetFolds, OrderFlowPanel, PanelSection, SettingsCloseInput, SettingsLightsInput,
    SettingsOpenInput, SettingsResult, SettingsSectionInput, close_descriptor, lights_descriptor,
    open_descriptor, section_descriptor,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

/// What a call on one tab's panel works on.
pub struct PanelSeat<'a> {
    pub panel: &'a mut OrderFlowPanel,
    pub folds: &'a mut AssetFolds,
    pub asset: &'a str,
    /// The tab is showing its flow pane, which the panel is attached to.
    pub flow_shown: bool,
    /// The theme authors a lit flow pane, so the bulb is offered.
    pub lights_offered: bool,
}

/// The tab a call names.
pub trait OrderflowSettingsPort {
    fn panel_seat(&mut self, tab: u64) -> Result<PanelSeat<'_>, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: OrderflowSettingsPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(open_descriptor(), open)?;
    registry.register(close_descriptor(), close)?;
    registry.register(section_descriptor(), set_section)?;
    registry.register(lights_descriptor(), set_lights)
}

fn open<H: OrderflowSettingsPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: SettingsOpenInput = parse(input)?;
    let seat = host.panel_seat(input.tab_id.get())?;
    if !seat.flow_shown {
        return Err(ControlError::invalid_request(
            "the tab is not showing its flow pane, so it has no order-flow panel to open",
        ));
    }
    let section = input.section.map(PanelSection::from);
    // Expanded here and not at the next frame, so the answer is already true.
    let expanded = section.is_some_and(|section| seat.folds.set(seat.asset, section, true));
    let opened = seat.panel.open_on(section);
    answer(input.tab_id, &seat, opened || expanded)
}

fn close<H: OrderflowSettingsPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: SettingsCloseInput = parse(input)?;
    let seat = host.panel_seat(input.tab_id.get())?;
    let closed = seat.panel.close();
    answer(input.tab_id, &seat, closed)
}

fn set_section<H: OrderflowSettingsPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: SettingsSectionInput = parse(input)?;
    let seat = host.panel_seat(input.tab_id.get())?;
    if !input.open {
        seat.panel.drop_request(input.section.into());
    }
    let changed = seat.folds.set(seat.asset, input.section.into(), input.open);
    answer(input.tab_id, &seat, changed)
}

fn set_lights<H: OrderflowSettingsPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: SettingsLightsInput = parse(input)?;
    let seat = host.panel_seat(input.tab_id.get())?;
    if !seat.lights_offered {
        return Err(ControlError::invalid_request(
            "the theme authors no lit order-flow surface, so the flow pane has no lights to switch",
        ));
    }
    let changed = seat.panel.set_lights(input.on);
    answer(input.tab_id, &seat, changed)
}

fn answer(
    tab: quantick_control::wire::WireU64,
    seat: &PanelSeat<'_>,
    changed: bool,
) -> Result<Value, ControlError> {
    let result = SettingsResult::read(tab, seat.asset, seat.panel, seat.folds, changed);
    to_value(&result)
}

fn parse<T: DeserializeOwned>(input: &Value) -> Result<T, ControlError> {
    serde_json::from_value(input.clone()).map_err(invalid)
}

fn to_value(value: &impl Serialize) -> Result<Value, ControlError> {
    serde_json::to_value(value).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BTC: &str = "BTCUSDT";

    struct Tab {
        panel: OrderFlowPanel,
        folds: AssetFolds,
        flow_shown: bool,
        lights_offered: bool,
    }

    impl Tab {
        fn showing_flow(flow_shown: bool) -> Self {
            Self {
                panel: OrderFlowPanel::default(),
                folds: AssetFolds::default(),
                flow_shown,
                lights_offered: true,
            }
        }
    }

    impl OrderflowSettingsPort for Tab {
        fn panel_seat(&mut self, _: u64) -> Result<PanelSeat<'_>, ControlError> {
            Ok(PanelSeat {
                panel: &mut self.panel,
                folds: &mut self.folds,
                asset: BTC,
                flow_shown: self.flow_shown,
                lights_offered: self.lights_offered,
            })
        }
    }

    fn actor() -> ActorContext {
        crate::test_support::FakeWindow::assistant()
    }

    #[test]
    fn open_with_a_section_expands_it_requests_the_scroll_and_a_repeat_says_so() {
        let mut tab = Tab::showing_flow(true);
        tab.folds.set(BTC, PanelSection::L2, false);
        let input = json!({"tab_id": "1", "section": "l2"});
        let first = open(&mut tab, &mut (), &actor(), &input).expect("opened");
        assert_eq!(first["open"], true);
        assert_eq!(first["l2_open"], true);
        assert_eq!(first["changed"], true);
        assert_eq!(first["asset"], BTC);
        let again = open(&mut tab, &mut (), &actor(), &input).expect("opened");
        assert_eq!(again["changed"], false);
        assert_eq!(
            tab.panel.take_request(BTC, &mut tab.folds),
            Some(PanelSection::L2)
        );
    }

    #[test]
    fn an_unknown_section_keeps_nothing_and_a_tab_without_its_flow_pane_refuses() {
        let mut tab = Tab::showing_flow(true);
        let bad = json!({"tab_id": "1", "section": "session"});
        assert!(open(&mut tab, &mut (), &actor(), &bad).is_err());
        assert!(!tab.panel.is_open());

        let mut hidden = Tab::showing_flow(false);
        let input = json!({"tab_id": "1"});
        let refused = open(&mut hidden, &mut (), &actor(), &input).expect_err("refused");
        assert!(refused.message.contains("flow pane"));
        assert!(!hidden.panel.is_open());
    }

    #[test]
    fn close_and_a_fold_read_back_and_retry_safely() {
        let mut tab = Tab::showing_flow(true);
        open(&mut tab, &mut (), &actor(), &json!({"tab_id": "1"})).expect("opened");
        let fold = json!({"tab_id": "1", "section": "bubbles", "open": false});
        let folded = set_section(&mut tab, &mut (), &actor(), &fold).expect("folded");
        assert_eq!(folded["bubbles_open"], false);
        assert_eq!(folded["changed"], true);
        let repeat = set_section(&mut tab, &mut (), &actor(), &fold).expect("folded");
        assert_eq!(repeat["changed"], false);
        let shut = close(&mut tab, &mut (), &actor(), &json!({"tab_id": "1"})).expect("closed");
        assert_eq!(shut["open"], false);
        assert_eq!(shut["changed"], true);
        let again = close(&mut tab, &mut (), &actor(), &json!({"tab_id": "1"})).expect("closed");
        assert_eq!(again["changed"], false);
    }

    #[test]
    fn the_lights_switch_read_back_retry_safely_and_refuse_a_theme_without_them() {
        let mut tab = Tab::showing_flow(true);
        let on = json!({"tab_id": "1", "on": true});
        let first = set_lights(&mut tab, &mut (), &actor(), &on).expect("lit");
        assert_eq!(first["lights_on"], true);
        assert_eq!(first["changed"], true);
        let again = set_lights(&mut tab, &mut (), &actor(), &on).expect("lit");
        assert_eq!(again["lights_on"], true);
        assert_eq!(again["changed"], false);
        assert!(tab.panel.lights_on());
        let bad = json!({"tab_id": "1", "on": "bright"});
        assert!(set_lights(&mut tab, &mut (), &actor(), &bad).is_err());
        assert!(tab.panel.lights_on(), "a refused call keeps nothing");
        let off = json!({"tab_id": "1", "on": false});
        let dark = set_lights(&mut tab, &mut (), &actor(), &off).expect("off");
        assert_eq!(
            (dark["lights_on"].clone(), dark["changed"].clone()),
            (json!(false), json!(true))
        );

        let mut dark_theme = Tab::showing_flow(true);
        dark_theme.lights_offered = false;
        let refused = set_lights(&mut dark_theme, &mut (), &actor(), &on).expect_err("refused");
        assert!(refused.message.contains("lit order-flow surface"));
        assert!(!dark_theme.panel.lights_on());
    }
}
