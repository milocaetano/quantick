use super::*;
use crate::Model as _;
use quantick_workspace::market_list::{CatalogMarket, Venue, VenueKind};

fn opened() -> State {
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Open {
            query: String::new(),
            switching: None,
        },
    );
    Model::update(
        &mut state,
        Intent::Refresh(Box::new(MarketSnapshot {
            venues: vec![Venue {
                feed_id: "binance".into(),
                name: "Binance".into(),
                chip: 'B',
                kind: VenueKind::Live,
                bar: Some("tick:50".into()),
                bridged: false,
            }],
            markets: ["BTCUSDT", "ETHUSDT"]
                .map(|symbol| CatalogMarket {
                    feed_id: "binance".into(),
                    symbol: symbol.into(),
                    asset: symbol.into(),
                    aliases: vec![],
                    user_added: true,
                })
                .to_vec(),
            starter_feed: "binance".into(),
            workspaces: vec!["Default".into(), "Scalp".into()],
            ..Default::default()
        })),
    );
    state
}
fn finish(state: &mut State) -> Option<PaletteIntent> {
    Model::update(
        state,
        Intent::Finish {
            escape: false,
            scrim_clicked: false,
            dropdown_open: false,
        },
    )
}

#[test]
fn migration_preserves_state_and_effects() {
    let mut state = opened();
    Model::update(&mut state, Intent::Refuse("host refusal".into()));
    assert_eq!(state.open.as_ref().unwrap().refusal(), Some("host refusal"));
    Model::update(&mut state, Intent::Query("ETH".into()));
    assert_eq!(state.open.as_ref().unwrap().refusal(), None);
    assert_eq!(state.listing.rows().next().unwrap().symbol, "ETHUSDT");
    Model::update(
        &mut state,
        Intent::Keys(vec![PaletteKey::Enter { background: true }]),
    );
    let Some(PaletteIntent::Open(choice)) = finish(&mut state) else {
        panic!("open effect");
    };
    assert_eq!(choice.symbol, "ETHUSDT");
    assert_eq!(choice.bar, "tick:50");
    assert!(choice.background);
    assert!(state.is_open(), "the host settles the submitted effect");
    assert_eq!(finish(&mut state), None, "delivered once");
    Model::update(&mut state, Intent::Refuse("cannot open".into()));
    assert_eq!(state.open.as_ref().unwrap().refusal(), Some("cannot open"));
    Model::update(&mut state, Intent::Close);
    Model::update(&mut state, Intent::Refuse("late refusal".into()));
    assert!(!state.is_open());
}

#[test]
fn ordered_keys_and_rows_keep_first_effect_while_all_inputs_update_state() {
    let mut state = opened();
    Model::update(
        &mut state,
        Intent::Keys(vec![
            PaletteKey::Enter { background: true },
            PaletteKey::Down,
            PaletteKey::Enter { background: false },
        ]),
    );
    Model::update(
        &mut state,
        Intent::Row {
            index: 1,
            submit: true,
        },
    );
    let Some(PaletteIntent::Open(choice)) = finish(&mut state) else {
        panic!("open effect");
    };
    assert_eq!(choice.symbol, "BTCUSDT");
    assert!(choice.background);
    assert_eq!(
        state
            .open
            .as_ref()
            .unwrap()
            .selected_row(&state.listing)
            .unwrap()
            .symbol,
        "ETHUSDT"
    );
}

#[test]
fn dismissal_cancels_submission_but_dropdown_dismissal_does_not_close() {
    for (escape, scrim_clicked, dropdown_open, closed) in [
        (true, false, true, true),
        (false, true, false, true),
        (false, true, true, false),
    ] {
        let mut state = opened();
        Model::update(
            &mut state,
            Intent::Keys(vec![PaletteKey::Enter { background: false }]),
        );
        let effect = Model::update(
            &mut state,
            Intent::Finish {
                escape,
                scrim_clicked,
                dropdown_open,
            },
        );
        assert_eq!(state.is_open(), !closed);
        assert_eq!(effect.is_none(), closed);
        assert_eq!(finish(&mut state), None);
    }
}

#[test]
fn footer_and_focus_intents_delegate_to_workspace_and_handler_defaults() {
    let mut state = opened();
    Model::update(
        &mut state,
        Intent::Focus {
            stop: PaletteFocus::Field,
            acquired: false,
            idle: true,
        },
    );
    assert!(state.request_focus);
    Model::update(
        &mut state,
        Intent::Keys(vec![PaletteKey::Tab, PaletteKey::Down]),
    );
    assert_eq!(state.open.as_ref().unwrap().focus(), PaletteFocus::Bars);
    Model::update(&mut state, Intent::BarKind("volume".into()));
    assert_eq!(
        state.open.as_ref().unwrap().bar(&state.listing),
        format!("volume:{}", default_bar_size("volume"))
    );
    Model::update(&mut state, Intent::BarSize("13".into()));
    Model::update(&mut state, Intent::Workspace("Scalp".into()));
    assert_eq!(
        state.open.as_ref().unwrap().bar(&state.listing),
        "volume:13"
    );
    assert_eq!(state.open.as_ref().unwrap().workspace(), "Scalp");
}
