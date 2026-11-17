use super::{ExecBridge, Mt5Connector};
use quantick_sources::config::MetaTraderBrokerSettings;
use quantick_trading::BrokerConnector;

/// A bridge whose process ran and ended: the test binary listing itself.
fn ended_bridge() -> ExecBridge {
    let child = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the test binary starts");
    let mut bridge = ExecBridge { child };
    bridge.child.wait().expect("it ends");
    bridge
}

#[test]
fn a_bridge_that_exited_is_not_running() {
    assert!(!ended_bridge().running());
}

#[test]
fn revive_relaunches_only_an_ended_bridge_behind_a_listener() {
    let settings = MetaTraderBrokerSettings {
        exec_bridge_command: Vec::new(),
        ..MetaTraderBrokerSettings::default()
    };
    let mut never_listened = Mt5Connector::new(settings.clone());
    never_listened.bridge = Some(ended_bridge());
    never_listened.revive("WINZ26");
    assert!(
        never_listened.bridge.is_some(),
        "no venue was built, so nothing is relaunched"
    );

    let mut connector = Mt5Connector::new(settings);
    connector.listening = true;
    connector.bridge = Some(ended_bridge());
    connector.revive("WINZ26");
    // The ended one is let go and a launch attempted; an empty command
    // launches nothing, which is all a test without Python can see.
    assert!(connector.bridge.is_none());
}

#[test]
fn an_empty_command_launches_nothing() {
    assert!(ExecBridge::launch(&[], 1, "WINZ26").is_none());
}

#[test]
fn a_missing_script_launches_nothing() {
    let command = [
        "python".to_owned(),
        "no/such/quantick_bridge_exec.py".to_owned(),
    ];
    assert!(ExecBridge::launch(&command, 1, "WINZ26").is_none());
}
