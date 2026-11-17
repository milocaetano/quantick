//! The whole-look and tick sphere calls through the fake window: the rule
//! decides before the pane's view wears anything.
use super::*;
use crate::bubble_pane::TickSpherePane;
use crate::test_support::{FakeBubbles, FakePane, FakeTab, FakeWindow};
use quantick_control_schema::bubble_preset::ListResult;
use serde_json::json;

/// One tab (id 1) whose pane (id 2) has these facts and an order-flow view.
fn window(facts: TickSpherePane) -> FakeWindow {
    let mut pane = FakePane::with_trades(2, 50, []);
    pane.sphere_facts = facts;
    pane.bubbles = Some(FakeBubbles::default());
    let mut tab = FakeTab::new(1, "BTCUSDT");
    tab.panes.push(pane);
    FakeWindow::new(vec![tab])
}

fn look(window: &FakeWindow) -> &FakeBubbles {
    window.tabs[0].panes[0]
        .bubbles
        .as_ref()
        .expect("the pane has a view")
}

fn look_mut(window: &mut FakeWindow) -> &mut FakeBubbles {
    window.tabs[0].panes[0]
        .bubbles
        .as_mut()
        .expect("the pane has a view")
}

/// A whole look brings its tick spheres by the rule tick.set asks.
#[test]
fn a_whole_look_brings_spheres_only_where_the_rule_admits_them() {
    let actor = FakeWindow::assistant();
    let input = json!({"tab_id": "1", "pane_id": "2", "preset": "mini index"});
    for (tick_chart, asset_mode) in [(false, true), (true, false), (true, true)] {
        let mut pane = window(TickSpherePane::new(tick_chart, asset_mode));
        let answer = apply(&mut pane, &mut (), &actor, &input).unwrap();
        assert_eq!(answer["changed"], true);
        let worn = Some(("mini index".to_owned(), tick_chart && asset_mode));
        assert_eq!(look(&pane).preset, worn);
        let address = json!({"tab_id": "1", "pane_id": "2"});
        let reset = reset(&mut pane, &mut (), &actor, &address).unwrap();
        assert_eq!(reset["changed"], true);
        assert_eq!(look(&pane).preset, None);
    }
    let mut no_view = window(TickSpherePane::new(true, true));
    no_view.tabs[0].panes[0].bubbles = None;
    let error = apply(&mut no_view, &mut (), &actor, &input).unwrap_err();
    assert!(format!("{error:?}").contains(NO_VIEW), "{error:?}");
    let mut elsewhere = json!({"tab_id": "1", "pane_id": "9", "preset": "mini index"});
    assert!(apply(&mut no_view, &mut (), &actor, &elsewhere).is_err());
    elsewhere["tab_id"] = json!("7");
    assert!(apply(&mut no_view, &mut (), &actor, &elsewhere).is_err());
}

#[test]
fn tick_set_asks_the_rule_before_the_pane_wears_anything() {
    let input = |preset: Value| json!({"tab_id": "1", "pane_id": "2", "preset": preset});
    let actor = FakeWindow::assistant();
    let mut volume_chart = window(TickSpherePane {
        tick_chart: false,
        asset_mode: true,
    });
    let refused = tick(
        &mut volume_chart,
        &mut (),
        &actor,
        &input(json!("Binance BTC Spot spheres")),
    );
    let violated = |error: ControlError| {
        assert_eq!(error.context.next_steps.len(), 1, "{error:?}");
        error
            .context
            .violated_precondition
            .map(|id| id.as_str().to_owned())
    };
    assert_eq!(
        violated(refused.unwrap_err()).as_deref(),
        Some(quantick_control_schema::bubble_scale::TICK_CHART_PRECONDITION)
    );
    assert_eq!(look(&volume_chart).worn, None, "refused before any edit");
    let mut no_mode = window(TickSpherePane {
        tick_chart: true,
        asset_mode: false,
    });
    let refused = tick(
        &mut no_mode,
        &mut (),
        &actor,
        &input(json!("Binance BTC Spot spheres")),
    );
    assert_eq!(
        violated(refused.unwrap_err()).as_deref(),
        Some(quantick_control_schema::tick_bubbles::ASSET_MODE_PRECONDITION)
    );
    assert_eq!(look(&no_mode).worn, None, "refused before any edit");
    let mut plain_asset = window(TickSpherePane {
        tick_chart: true,
        asset_mode: false,
    });
    let dropped = tick(&mut plain_asset, &mut (), &actor, &input(Value::Null)).unwrap();
    assert_eq!(
        look(&plain_asset).worn,
        Some(None),
        "spheres can always be dropped"
    );
    assert_eq!(dropped["changed"], true);
    assert!(
        dropped["tick_bubbles"].is_null(),
        "the answer reads what the view wears"
    );
    let mut btc = window(TickSpherePane {
        tick_chart: true,
        asset_mode: true,
    });
    tick(
        &mut btc,
        &mut (),
        &actor,
        &input(json!("Binance BTC Spot spheres")),
    )
    .unwrap();
    assert_eq!(
        look(&btc).worn,
        Some(Some("Binance BTC Spot spheres".to_owned()))
    );
    assert_eq!(look(&btc).tuning, TickTuning::default());
    let tuned = |extra: Value| {
        let mut request = input(json!("mini index regions"));
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        request
    };
    tick(
        &mut btc,
        &mut (),
        &actor,
        &tuned(json!({"peak_spheres": false, "opacity": "0.6"})),
    )
    .unwrap();
    assert_eq!(
        look(&btc).tuning,
        TickTuning {
            peak_spheres: Some(false),
            opacity: Some(0.6),
        }
    );
    look_mut(&mut btc).worn = None;
    for refused in [
        json!({"opacity": "1.5"}),
        json!({"opacity": "0"}),
        json!({"opacity": 0.5}),
        json!({"peak_spheres": "yes"}),
    ] {
        let error = tick(&mut btc, &mut (), &actor, &tuned(refused.clone()));
        assert!(error.is_err(), "{refused}");
    }
    let mut strip = input(Value::Null);
    strip["peak_spheres"] = json!(true);
    assert!(tick(&mut btc, &mut (), &actor, &strip).is_err());
    assert_eq!(look(&btc).worn, None, "every refusal comes before any edit");
}

/// A view whose catalog answers one page, or refuses.
struct Shelf {
    page: Result<ListResult, String>,
    config: quantick_orderflow::HeatmapConfig,
}

impl PaneBubblesPort for Shelf {
    type Theme = ();
    fn bubble_theme(&self) {}
    fn pane_bubbles(
        &mut self,
        _: WireU64,
        _: WireU64,
    ) -> Result<&mut dyn crate::bubble_pane::BubblePane<()>, ControlError> {
        Ok(self)
    }
}

impl crate::bubble_pane::BubblePane<()> for Shelf {
    fn tick_sphere_pane(&self) -> TickSpherePane {
        TickSpherePane::new(false, false)
    }
    fn bubble_look(&mut self) -> Option<&mut dyn BubbleLook<()>> {
        Some(self)
    }
    fn asset_save(&mut self) -> Option<&mut dyn crate::bubble_pane::AssetSave> {
        None
    }
}

impl BubbleLook<()> for Shelf {
    fn wear_preset(&mut self, _: &str, _: bool, _: &()) -> Result<bool, String> {
        unreachable!("a listing wears nothing")
    }
    fn restore_factory_look(&mut self) -> bool {
        unreachable!("a listing wears nothing")
    }
    fn presets_page(&self, input: &ListInput) -> Result<ListResult, String> {
        self.page.clone().map(|mut page| {
            page.tab_id = input.tab_id;
            page.pane_id = input.pane_id;
            page
        })
    }
    fn wear_tick(&mut self, _: Option<&str>, _: TickTuning) -> Result<bool, String> {
        unreachable!("a listing wears nothing")
    }
    fn look_snapshot(&self) -> quantick_control_schema::orderflow::BubblesStateSnapshot {
        unreachable!("a listing reads the page alone")
    }
    fn look_config(&self) -> &quantick_orderflow::HeatmapConfig {
        &self.config
    }
}

/// The listing answers the view's page for the addressed pane, and the
/// view's refusal (a stale revision, a limit out of range) is the caller's
/// to fix.
#[test]
fn the_preset_list_answers_the_views_page_or_its_refusal() {
    let actor = FakeWindow::assistant();
    let page = ListResult {
        tab_id: WireU64::new(0),
        pane_id: WireU64::new(0),
        preset: "Crypto default".to_owned(),
        names: vec!["Crypto default".to_owned()],
        total: WireU64::new(2),
        revision: "r1".to_owned(),
        next_offset: Some(WireU64::new(1)),
    };
    let mut shelf = Shelf {
        page: Ok(page),
        config: quantick_orderflow::HeatmapConfig::default(),
    };
    let query = json!({"tab_id": "1", "pane_id": "2", "offset": "0", "limit": 1, "revision": null});
    let listed = list(&mut shelf, &mut (), &actor, &query).unwrap();
    assert_eq!(
        (&listed["tab_id"], &listed["pane_id"]),
        (&json!("1"), &json!("2"))
    );
    assert_eq!(listed["names"], json!(["Crypto default"]));
    assert_eq!(listed["next_offset"], "1");
    shelf.page = Err("Preset catalog changed; restart at offset zero".to_owned());
    let error = list(&mut shelf, &mut (), &actor, &query).unwrap_err();
    assert_eq!(
        error.code.as_str(),
        quantick_control::error::codes::INVALID_REQUEST
    );
    assert!(error.message.contains("restart"), "{error:?}");
    let mut unknown = query.clone();
    unknown["page"] = json!(1);
    assert!(list(&mut shelf, &mut (), &actor, &unknown).is_err());
}
