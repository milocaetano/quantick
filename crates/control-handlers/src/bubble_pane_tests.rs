//! The tick sphere rule the picker and `orderflow.bubbles.tick.set` share.
use super::*;

/// Review #19/#29: one rule decides tick spheres for the picker and for
/// tick.set. Choosing needs a tick chart and the asset's config mode;
/// dropping them is always allowed, whatever put them on.
/// Review round 2 #3: the panel tunes exactly when the call could: a
/// named preset worn where the call admits it.
#[test]
fn tuning_follows_the_rule_the_call_asks() {
    let pane = |tick_chart, asset_mode| TickSpherePane {
        tick_chart,
        asset_mode,
    };
    assert_eq!(pane(true, true).tune(Some("mini index regions")), Ok(()));
    assert_eq!(
        pane(true, true).tune(None),
        Err(SphereRefusal::NoNamedPreset)
    );
    assert_eq!(
        pane(false, true).tune(Some("mini index regions")),
        Err(SphereRefusal::NotTickChart)
    );
    assert_eq!(
        pane(true, false).tune(Some("mini index regions")),
        Err(SphereRefusal::AssetModeOff)
    );
    for refusal in [SphereRefusal::NotTickChart, SphereRefusal::AssetModeOff] {
        let error = ControlError::from(refusal);
        assert!(format!("{error:?}").contains(refusal.reason()), "{error:?}");
    }
}

#[test]
fn tick_spheres_need_a_tick_chart_and_the_asset_mode_but_anyone_may_drop_them() {
    for tick_chart in [false, true] {
        for asset_mode in [false, true] {
            let pane = TickSpherePane {
                tick_chart,
                asset_mode,
            };
            assert_eq!(pane.may_choose(), tick_chart && asset_mode);
            assert_eq!(
                pane.admit(Some("Binance BTC Spot spheres")).is_ok(),
                tick_chart && asset_mode,
                "{pane:?}"
            );
            assert!(pane.admit(None).is_ok(), "{pane:?}");
            let presets = vec!["Binance BTC Spot spheres".to_owned()];
            let offered = pane.offered(presets.clone(), false);
            assert_eq!(offered, pane.may_choose().then(|| presets.clone()));
            let wearing = pane.offered(presets.clone(), true);
            assert_eq!(
                wearing,
                Some(if pane.may_choose() {
                    presets
                } else {
                    Vec::new()
                }),
                "spheres a pane wears can always be switched off"
            );
        }
    }
}
