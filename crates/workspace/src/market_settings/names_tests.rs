use super::super::{DrawingKey, MarketKey, parse_settings, to_text};

/// One mark of `tool`, named `name` when given.
fn mark(id: u64, tool: &str, name: Option<&str>) -> String {
    let name = name.map_or_else(String::new, |name| format!("name = \"{name}\"\n"));
    format!(
        "[[drawings.items]]\nid = {id}\ntool = \"{tool}\"\n{name}color = [1, 2, 3, 255]\nwidth_px = 1.0\nfill_alpha = 0\nlocked = false\nhidden = false\nshared = false\n\n[[drawings.items.points]]\ntime_ms = 1000\nprice = 100.0\nbar = 1.0\n\n[drawings.items.band]\naxis = \"price\"\n\n"
    )
}

fn set(asset: &str, role: &str, marks: &[String]) -> String {
    format!(
        "[[drawings]]\nvenue = \"binance\"\nasset = \"{asset}\"\nrole = \"{role}\"\n\n{}",
        marks.concat()
    )
}

/// BTCUSDT: a flow set with a named zone, an unnamed zone, the trader's own
/// line name and an unnamed line; a context set with an unnamed zone. ETHUSDT:
/// one unnamed zone, numbered on its own.
fn file() -> String {
    format!(
        "version = 1\n\n{}{}{}",
        set(
            "BTCUSDT",
            "flow",
            &[
                mark(1, "rectangle", Some("zone-4")),
                mark(2, "rectangle", None),
                mark(3, "trend-line", Some("congestion 108k")),
                mark(4, "horizontal-line", None),
            ]
        ),
        set("BTCUSDT", "context:time:5m", &[mark(5, "rectangle", None)]),
        set("ETHUSDT", "flow", &[mark(1, "rectangle", None)]),
    )
}

fn names(settings: &super::MarketSettings, asset: &str, context: Option<&str>) -> Vec<String> {
    let key = DrawingKey::of_pane("binance", asset.to_owned(), context.map(str::to_owned));
    let marks = settings.drawings(&key).iter();
    marks.map(|mark| mark.name.clone().unwrap()).collect()
}

#[test]
fn a_load_names_every_unnamed_mark_per_market_above_the_names_it_uses() {
    let settings = parse_settings(&file()).unwrap();
    assert_eq!(
        names(&settings, "BTCUSDT", None),
        ["zone-4", "zone-5", "congestion 108k", "line-1"]
    );
    assert_eq!(names(&settings, "BTCUSDT", Some("time:5m")), ["zone-6"]);
    assert_eq!(names(&settings, "ETHUSDT", None), ["zone-1"]);
}

#[test]
fn names_are_the_same_on_every_load_and_after_a_save_and_load() {
    let first = parse_settings(&file()).unwrap();
    assert_eq!(first, parse_settings(&file()).unwrap(), "every load agrees");
    let saved = to_text(&first).unwrap();
    assert_eq!(
        parse_settings(&saved).unwrap(),
        first,
        "the file keeps them"
    );
}

#[test]
fn the_floors_are_the_highest_number_each_prefix_uses_in_the_market() {
    let settings = parse_settings(&file()).unwrap();
    let market = MarketKey {
        venue: "binance".to_owned(),
        asset: "BTCUSDT".to_owned(),
    };
    let floors = settings.name_floors(&market);
    assert_eq!(floors.get("zone"), Some(&6));
    assert_eq!(floors.get("line"), Some(&1));
}

#[test]
fn the_name_high_water_is_kept_in_the_file_and_never_lowered() {
    let mut settings = parse_settings(&file()).unwrap();
    let market = MarketKey {
        venue: "binance".to_owned(),
        asset: "BTCUSDT".to_owned(),
    };
    let floors = |zone: u64| crate::drawing_names::NameFloors::from([("zone".to_owned(), zone)]);
    settings.raise_name_floors(&market, &floors(9));
    settings.raise_name_floors(&market, &floors(2));
    let reread = parse_settings(&to_text(&settings).unwrap()).unwrap();
    assert_eq!(reread, settings);
    assert_eq!(reread.name_floors(&market).get("zone"), Some(&9));
    let other = MarketKey {
        venue: "binance".to_owned(),
        asset: "ETHUSDT".to_owned(),
    };
    assert_eq!(
        reread.name_floors(&other).get("zone"),
        Some(&1),
        "per market"
    );
}
