use super::*;
use crate::Model as _;
fn open(query: &str) -> State {
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Open {
            tab: 17,
            side: PaneSide::Time(2),
            query: query.into(),
            follows_focus: false,
        },
    );
    state
}
fn edit(state: &mut State, query: &str, down: bool, up: bool) {
    Model::update(
        state,
        Intent::Edit {
            query: query.into(),
            down,
            up,
        },
    );
}
fn finish(state: &mut State, enter: bool, escape: bool) -> Option<BarSwitchRequest> {
    Model::update(
        state,
        Intent::Finish {
            clicked: None,
            enter,
            escape,
            abandoned: false,
        },
    )
}
#[test]
fn migration_preserves_state_and_effects() {
    for (query, expected) in [
        ("2000t", "tick:2000"),
        ("1d", "time:1d"),
        ("50R", "renko:50"),
    ] {
        let mut state = open(query);
        assert_eq!(
            state.open().unwrap().candidates,
            BUILTIN_BARS.quick_matches(query)
        );
        let request = finish(&mut state, true, false).unwrap();
        assert_eq!((request.tab, request.side), (17, PaneSide::Time(2)));
        assert_eq!(request.config.to_config_string(), expected);
        assert!(state.open().is_none());
        assert_eq!(finish(&mut state, true, false), None);
    }
}
#[test]
fn escape_closes_without_a_change() {
    let mut state = open("5");
    assert_eq!(finish(&mut state, false, true), None);
    assert!(state.open().is_none());
}
#[test]
fn a_new_number_moves_the_highlight_back_to_the_top() {
    let mut state = open("15");
    for _ in 0..3 {
        edit(&mut state, "15", true, false);
    }
    assert_eq!(state.open().unwrap().selected, 3);
    edit(&mut state, "150", false, false);
    assert_eq!(
        finish(&mut state, true, false).unwrap().config,
        BUILTIN_BARS.quick_candidates(150)[0]
    );
}
#[test]
fn enter_takes_the_first_reading_and_the_arrow_the_next() {
    for (query, first, next) in [
        ("2000t", "tick:2000", "trades:2000"),
        ("1d", "time:1d", "dollar:1"),
    ] {
        assert_eq!(
            finish(&mut open(query), true, false)
                .unwrap()
                .config
                .to_config_string(),
            first
        );
        let mut state = open(query);
        edit(&mut state, query, true, false);
        assert_eq!(
            finish(&mut state, true, false)
                .unwrap()
                .config
                .to_config_string(),
            next
        );
    }
}
#[test]
fn query_limits_aliases_and_empty_candidates_use_engine_rules() {
    let mut state = open("123456789T");
    for query in [
        "123456789TR",
        "123456789R5",
        "123456789Rr",
        "1mo",
        "2d",
        "1w",
        "0",
        "",
    ] {
        edit(&mut state, query, false, false);
        let current = state.open().unwrap();
        assert_eq!(current.query, quick_query_text(query));
        assert_eq!(
            current.candidates,
            BUILTIN_BARS.quick_matches(&current.query)
        );
    }
    assert_eq!(finish(&mut state, true, false), None);
    assert!(state.open().is_none());
}
#[test]
fn navigation_is_bounded_and_idle_edits_reuse_the_projection() {
    let mut state = open("15");
    let allocation = state.open().unwrap().candidates.as_ptr();
    for _ in 0..100 {
        edit(&mut state, "15", true, false);
    }
    let current = state.open().unwrap();
    assert_eq!(current.selected, current.candidates.len() - 1);
    assert_eq!(allocation, current.candidates.as_ptr());
    for _ in 0..100 {
        edit(&mut state, "15", false, true);
    }
    assert_eq!(state.open().unwrap().selected, 0);
}
#[test]
fn focus_change_closes_normal_switch_and_retargets_hook_switch() {
    for follows_focus in [false, true] {
        let mut state = open("15");
        Model::update(
            &mut state,
            Intent::Open {
                tab: 17,
                side: PaneSide::Time(2),
                query: "15".into(),
                follows_focus,
            },
        );
        Model::update(
            &mut state,
            Intent::Observe {
                tab: 23,
                side: PaneSide::Flow,
                text: String::new(),
                widget_focused: true,
                modified: false,
            },
        );
        let request = finish(&mut state, true, false);
        assert_eq!(
            request.map(|request| (request.tab, request.side)),
            follows_focus.then_some((23, PaneSide::Flow))
        );
    }
}
#[test]
fn typed_open_requires_bare_digits_and_no_widget_focus() {
    for (text, widget_focused, modified, opens) in [
        ("15", false, false, true),
        ("h", false, false, false),
        ("1", true, false, false),
        ("1", false, true, false),
        ("", false, false, false),
    ] {
        let mut state = State::default();
        Model::update(
            &mut state,
            Intent::Observe {
                tab: 0,
                side: PaneSide::Flow,
                text: text.into(),
                widget_focused,
                modified,
            },
        );
        assert_eq!(state.open().is_some(), opens);
    }
}
#[test]
fn invalid_click_and_empty_enter_are_noops_and_abandonment_closes() {
    let mut state = open("0");
    assert_eq!(finish(&mut state, true, false), None);
    assert!(state.open().is_some());
    assert_eq!(
        Model::update(
            &mut state,
            Intent::Finish {
                clicked: Some(usize::MAX),
                enter: false,
                escape: false,
                abandoned: false
            }
        ),
        None
    );
    assert!(state.open().is_some());
    Model::update(
        &mut state,
        Intent::Finish {
            clicked: None,
            enter: false,
            escape: false,
            abandoned: true,
        },
    );
    assert!(state.open().is_none());
}
