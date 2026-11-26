// Currency rules and stored rates: equivalence, both directions, a day
// with no rate, and a bad rule refused by name.

use quantick_civil::parse_iso_date;

use super::*;

fn code(text: &str) -> Currency {
    Currency::new(text).expect("test currency")
}

fn day(text: &str) -> CivilDate {
    parse_iso_date(text).expect("test day")
}

fn rules(same: &[(&str, &str)], pairs: &[(&str, &str)]) -> Result<CurrencyRules, String> {
    let currencies = CurrenciesRecord {
        same: same
            .iter()
            .map(|(alias, currency)| ((*alias).to_owned(), (*currency).to_owned()))
            .collect(),
    };
    let rates: Vec<RateRecord> = pairs
        .iter()
        .map(|(from, to)| RateRecord {
            from: (*from).to_owned(),
            to: (*to).to_owned(),
            venue: "binance".to_owned(),
            symbol: format!("{from}{to}"),
        })
        .collect();
    CurrencyRules::parse(&currencies, &rates)
}

fn usd_rules() -> CurrencyRules {
    rules(&[("USDT", "USD"), ("USDC", "USD")], &[("USDT", "BRL")]).expect("fixture rules")
}

#[test]
fn a_stored_rate_converts_both_ways_and_through_equivalent_codes() {
    let rules = usd_rules();
    let mut store = RateStore::default();
    assert!(store.insert(
        code("USDT"),
        code("BRL"),
        day("2026-11-20"),
        Decimal::new(5, 0)
    ));
    let rates = StoredRates {
        rules: &rules,
        store: &store,
    };
    let on = day("2026-11-20");
    assert_eq!(
        rates.rate(&code("USDT"), &code("BRL"), on),
        Some(Decimal::new(5, 0))
    );
    assert_eq!(
        rates.rate(&code("BRL"), &code("USDT"), on),
        Some(Decimal::new(2, 1)),
        "the reverse pair is the inverse"
    );
    assert_eq!(
        rates.rate(&code("USDC"), &code("BRL"), on),
        Some(Decimal::new(5, 0)),
        "USDC counts as USD, as USDT does"
    );
    assert_eq!(
        rates.rate(&code("BRL"), &code("USD"), on),
        Some(Decimal::new(2, 1))
    );
    assert_eq!(
        rates.rate(&code("USDT"), &code("USD"), on),
        Some(Decimal::ONE)
    );
    assert_eq!(
        rates.rate(&code("BRL"), &code("BRL"), on),
        Some(Decimal::ONE)
    );
    assert_eq!(
        rates.rate(&code("USDT"), &code("BRL"), day("2026-11-21")),
        None,
        "a day with no stored rate has none"
    );
    assert_eq!(
        rates.rate(&code("EUR"), &code("BRL"), on),
        None,
        "no such pair"
    );
}

#[test]
fn a_rule_that_cannot_mean_one_thing_is_refused_by_name() {
    for (same, pairs, named) in [
        (&[("USDT", "USDT")][..], &[][..], "counts as itself"),
        (
            &[("USDT", "USD"), ("USD", "EUR")][..],
            &[][..],
            "alias and a currency",
        ),
        (
            &[("USDT", "USD")][..],
            &[("USDT", "USD")][..],
            "are one currency",
        ),
        (&[(" ", "USD")][..], &[][..], "must not be blank"),
    ] {
        let error = rules(same, pairs).unwrap_err();
        assert!(error.contains(named), "{same:?} {pairs:?}: {error}");
    }
}

#[test]
fn the_store_sits_beside_the_journal_and_a_bad_file_holds_no_rate() {
    let journal = Path::new("home").join("paper-trades");
    assert_eq!(store_path(&journal), Path::new("home").join(RATES_FILE));
    assert_eq!(
        store_path(Path::new("trades")),
        Path::new("trades").join(RATES_FILE)
    );
    let dir = crate::scratch::ScratchDir::new("rates-store");
    let path = dir.join(RATES_FILE);
    assert_eq!(read_store(&path), RateStore::default(), "a missing file");
    std::fs::write(&path, "2026-11-20,USDT,BRL,not a rate\n").expect("write fixture");
    assert_eq!(
        read_store(&path),
        RateStore::default(),
        "an unreadable file"
    );
    std::fs::write(&path, "2026-11-20,USDT,BRL,5.5\n").expect("write fixture");
    assert_eq!(
        read_store(&path).get(&code("USDT"), &code("BRL"), day("2026-11-20")),
        Some(Decimal::new(55, 1))
    );
}
