use super::*;

#[test]
fn the_defaults_are_off_refuse_agents_and_validate() {
    let settings = BrokerSettings::default();
    assert!(!settings.metatrader.enabled);
    assert!(!settings.metatrader.allow_agent_orders);
    assert_eq!(settings.validate(&MetaTraderSettings::default()), Ok(()));
}

#[test]
fn impossible_settings_are_refused_with_the_key() {
    let feed = MetaTraderSettings::default();
    let with = |change: fn(&mut MetaTraderBrokerSettings)| {
        let mut settings = BrokerSettings::default();
        change(&mut settings.metatrader);
        settings.validate(&feed).unwrap_err()
    };
    assert!(with(|s| s.magic = 0).contains("magic"));
    assert!(with(|s| s.exec_port = 0).contains("exec_port"));
    assert!(with(|s| s.exec_port = 9100).contains("feed port"));
    assert!(with(|s| s.deviation_points = 99_999).contains("deviation_points"));
}
