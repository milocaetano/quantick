//! The symbol multi-select and the merge controls: which symbols load, when
//! the Per asset | Merge all switch shows, what the currency combo offers
//! and which pick is handed to the host to persist.
use std::sync::Arc;

use super::*;
use quantick_paper::instruments::{InstrumentBook, InstrumentCatalog};
use quantick_paper::pnl::{PnlBook, PnlSettings};

const CATALOG: &str = r#"
version = 1

[currencies]
same = { USDT = "USD" }

[instruments.WINZ26]
currency = "BRL"
point_value = "0.20"
size_step = "1"

[instruments.BTCUSDT]
currency = "USDT"
point_value = "1"
size_step = "0.001"
price_unit = "quote"
"#;

fn code(text: &str) -> Currency {
    Currency::new(text).expect("test currency")
}

struct World {
    declared: InstrumentBook,
    catalog: Arc<InstrumentCatalog>,
    settings: PnlSettings,
}

impl World {
    fn new(merge_currency: Option<&str>) -> Self {
        let catalog = Arc::new(InstrumentCatalog::parse(CATALOG).expect("fixture catalog"));
        Self {
            declared: InstrumentBook::new(),
            settings: PnlSettings {
                catalog: Arc::clone(&catalog),
                chosen: Some(PnlPrimary::Money),
                merge_currency: merge_currency.map(code),
            },
            catalog,
        }
    }

    fn pnl(&self) -> PnlReader<'_> {
        PnlReader {
            book: PnlBook {
                declared: &self.declared,
                catalog: &self.catalog,
                feed: "",
            },
            settings: &self.settings,
        }
    }
}

/// A report open on `symbols`' saved trades, cut through `world`.
fn report(world: &World, symbols: &[&str]) -> State {
    let mut state = State::default();
    Model::update(&mut state, Intent::Reveal(String::new()));
    let rows = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| row(symbol, None, trade_at(1_000 * (index as i64 + 1), 10)))
        .collect();
    Model::update(&mut state, loaded_report(rows));
    state.ensure_report_view(TzOffset::new(0), world.pnl());
    state
}

#[test]
fn the_symbol_multi_select_loads_the_ticked_symbols_and_empty_reads_all() {
    let mut state = State::default();
    let open = Model::update(&mut state, Intent::Open("WINZ26".to_owned()));
    assert_eq!(open.load, Some(Load::Report(BTreeSet::from(["WINZ26".to_owned()]))));
    let tick = Model::update(&mut state, Intent::ToggleSymbol("BTCUSDT".to_owned()));
    assert_eq!(
        tick.load,
        Some(Load::Report(BTreeSet::from(["BTCUSDT".to_owned(), "WINZ26".to_owned()])))
    );
    assert_eq!(state.report_symbol(), None, "two symbols name no single one");
    Model::update(&mut state, Intent::ToggleSymbol("BTCUSDT".to_owned()));
    let untick = Model::update(&mut state, Intent::ToggleSymbol("WINZ26".to_owned()));
    assert_eq!(untick.load, Some(Load::Report(BTreeSet::new())), "none ticked reads all");
    assert!(state.filters_narrowed("WINZ26"));

    let world = World::new(None);
    Model::update(&mut state, loaded_report(vec![]));
    assert_eq!(state.symbols_summary(), "All 1 symbols");
    Model::update(&mut state, Intent::ToggleSymbol("X".to_owned()));
    assert_eq!(state.symbols_summary(), "1 of 1 symbols");
    assert_eq!(
        state.symbol_choices(world.pnl()),
        vec![("X".to_owned(), true, "X · pt value unset".to_owned())]
    );
}

#[test]
fn the_switch_shows_only_for_more_than_one_walk_and_merge_all_defaults_to_the_chart() {
    let world = World::new(None);
    let one = report(&world, &["WINZ26"]);
    let controls = one.merge_controls(world.pnl(), "WINZ26");
    assert_eq!(controls.switch, None, "one walk: no switch");
    assert_eq!(
        controls.currency,
        Some((code("BRL"), vec![code("BRL"), code("USD")])),
        "one asset still offers its own currency and USD"
    );

    let mut two = report(&world, &["WINZ26", "BTCUSDT"]);
    let controls = two.merge_controls(world.pnl(), "BTCUSDT");
    assert_eq!(controls.switch, Some(false), "per asset");
    assert_eq!(controls.currency, None, "no combo until merged");
    assert_eq!(controls.default, code("USD"), "the chart symbol's currency, USDT read as USD");
    assert_eq!(two.merge_controls(world.pnl(), "WINZ26").default, code("BRL"));

    let merge = Model::update(&mut two, Intent::Merge(Some(code("BRL"))));
    assert_eq!(merge.load, Some(Load::Report(BTreeSet::new())), "rates read again");
    assert_eq!(merge.pnl_pick, None, "the switch persists nothing");
    two.ensure_report_view(TzOffset::new(0), world.pnl());
    let controls = two.merge_controls(world.pnl(), "WINZ26");
    assert_eq!(controls.switch, Some(true));
    assert_eq!(controls.currency, Some((code("BRL"), vec![code("BRL"), code("USD")])));
    let view = two.report_view().as_ref().expect("cut");
    assert_eq!(view.equity.walks().len(), 1, "one merged curve");
    assert!(view.merge.caption().expect("captioned").starts_with("Merged in BRL"));

    let last = World::new(Some("USD"));
    assert_eq!(
        two.merge_controls(last.pnl(), "WINZ26").default,
        code("USD"),
        "the last pick beats the chart's currency"
    );
}

#[test]
fn a_currency_pick_is_persisted_and_the_one_assets_own_reads_per_asset() {
    let world = World::new(None);
    let mut one = report(&world, &["WINZ26"]);
    let usd = Model::update(&mut one, Intent::MergeCurrency(code("USD")));
    assert_eq!(usd.pnl_pick, Some(PnlPick::MergeCurrency(code("USD"))));
    assert_eq!(one.report_merge(), Some(&code("USD")), "WIN alone, shown in USD");
    one.ensure_report_view(TzOffset::new(0), world.pnl());
    let back = Model::update(&mut one, Intent::MergeCurrency(code("BRL")));
    assert_eq!(back.pnl_pick, Some(PnlPick::MergeCurrency(code("BRL"))));
    assert_eq!(one.report_merge(), None, "its own currency is per asset");
}
