//! The staged session a capture photographs reads back as the order
//! surfaces will paint it.

use quantick_broker_mt5::fake::{
    FakeBridge, STAGED_ORDER_TICKET, STAGED_POSITION_TICKET, StagedSession,
};
use quantick_broker_mt5::{BrokerConfig, Mt5Venue, TradeMode};
use quantick_trading::{BrokerVenue, TradingVenue};
use rust_decimal::Decimal;

#[test]
fn a_staged_session_holds_a_protected_position_and_a_working_order() {
    let (bridge, handle) = FakeBridge::new();
    let mut venue = Mt5Venue::new(
        BrokerConfig {
            symbol: "WINZ26".into(),
            magic: 7_710_001,
            deviation_points: 10,
            session_tag: "staged".into(),
        },
        Box::new(bridge),
    );
    handle.stage(&StagedSession {
        symbol: "WINZ26",
        magic: 7_710_001,
        login: 51_234_567,
        company: "XP Investimentos",
        trade_mode: "real",
        balance: "12340.00",
        currency: "BRL",
        entry: Decimal::from(172_796),
    });
    let _ = Mt5Venue::poll(&mut venue);
    let facts = BrokerVenue::account(&venue).expect("the account is known");
    assert_eq!(facts.trade_mode, TradeMode::Real);
    assert_eq!(facts.short_name(), "XP 51234567");
    let position = venue.position().expect("the staged long");
    assert_eq!(position.avg_price, Decimal::from(172_796));
    assert!(position.stop_loss.is_some() && position.take_profit.is_some());
    assert_eq!(venue.position_ref(), Some(STAGED_POSITION_TICKET));
    let order = venue.working_orders()[0].id;
    assert_eq!(venue.order_ref(order), Some(STAGED_ORDER_TICKET));
    assert!(BrokerVenue::is_acknowledged(&venue, order));
}

#[test]
fn a_staged_connector_builds_the_account_asked_for_and_cuts_its_link() {
    use quantick_broker_mt5::fake::StagedConnector;
    use quantick_trading::{BrokerConnector, LinkState};
    let mut connector = StagedConnector::new(true, 7_710_001, Decimal::from(172_796));
    let mut venue = connector.connect("WINZ26").expect("a staged venue");
    let _ = venue.poll();
    let facts = venue.account().expect("the account is known");
    assert_eq!(facts.trade_mode, TradeMode::Demo);
    assert_eq!(venue.link_state(), LinkState::Up);
    connector.sever();
    let _ = venue.poll();
    assert_eq!(venue.link_state(), LinkState::Lost);
}
