//! The `[broker.metatrader]` section as the config file reads it.

use super::*;

#[test]
fn the_broker_section_is_read_and_off_unless_it_says_otherwise() {
    let shipped = parse(
        EMBEDDED_DEFAULT,
        ConfigSource::Embedded,
        &AddedSymbols::default(),
    )
    .expect("the shipped config parses");
    assert_eq!(
        shipped.broker.metatrader,
        MetaTraderBrokerSettings::default()
    );
    assert!(
        !shipped.broker.metatrader.enabled,
        "a real broker stays off until enabled"
    );
    assert!(!shipped.broker.metatrader.allow_agent_orders);

    let text = r#"
        default_feed = "mt"
        default_symbol = "WIN$N"
        [[feeds]]
        id = "mt"
        name = "MetaTrader 5"
        provider = "metatrader"
        symbols = ["WIN$N"]
        [broker.metatrader]
        enabled = true
        exec_port = 9300
        magic = 42
        deviation_points = 5
        allow_agent_orders = true
    "#;
    let config = parse(text, ConfigSource::Embedded, &AddedSymbols::default()).unwrap();
    assert_eq!(
        config.broker.metatrader,
        MetaTraderBrokerSettings {
            enabled: true,
            exec_port: 9300,
            magic: 42,
            deviation_points: 5,
            allow_agent_orders: true,
            ..MetaTraderBrokerSettings::default()
        }
    );

    let clash = text.replace("exec_port = 9300", "exec_port = 9100");
    let refused = parse(&clash, ConfigSource::Embedded, &AddedSymbols::default());
    assert!(
        refused.is_err(),
        "the execution port may not be a feed port"
    );
    let misspelt = text.replace("enabled = true", "enable = true");
    assert!(parse(&misspelt, ConfigSource::Embedded, &AddedSymbols::default()).is_err());
}
