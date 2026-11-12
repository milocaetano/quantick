//! Window, draft and debounce regressions moved from the app surface.
use super::*;
fn ready() -> State {
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Loaded {
            candles: CandleSettings::open(std::path::PathBuf::new()),
            footprint: PresetStore::default(),
        },
    );
    state
}
fn open(state: &mut State, tab: ChartSettingsTab, style: ChartStyle) {
    Model::update(
        state,
        Intent::Open {
            tab,
            target: None,
            style,
        },
    );
}
fn pending(open: bool, now: u64) -> State {
    let mut state = ready();
    state.session.open = open;
    state.log_pending = true;
    state.last_change = Some(now);
    state
}
#[test]
fn every_door_reaches_the_one_window() {
    let style = ChartStyle::default();
    let mut state = ready();
    open(&mut state, ChartSettingsTab::Footprint, style);
    assert!(state.is_open_on(ChartSettingsTab::Footprint));
    open(&mut state, ChartSettingsTab::Candles, style);
    assert!(state.is_open_on(ChartSettingsTab::Candles));
    Model::update(&mut state, Intent::Close);
    assert!(!state.is_open());
    assert_eq!(state.tab, ChartSettingsTab::Candles);
}
#[test]
fn reset_returns_to_the_look_the_window_opened_on() {
    let opened_on = ChartStyle::default();
    let mut state = ready();
    open(&mut state, ChartSettingsTab::Candles, opened_on);
    let mut moved = opened_on;
    moved.candles = CandlePreset::Glass.style();
    moved.canvas.grid_enabled = !opened_on.canvas.grid_enabled;
    Model::update(&mut state, Intent::EditStyle(moved, None, 0));
    assert_eq!(state.reset, Some(opened_on));
    Model::update(
        &mut state,
        Intent::Refresh {
            style: moved,
            footprint: FootprintConfig::default(),
            target: "Flow chart".into(),
            customized: false,
        },
    );
    assert_eq!(state.reset, Some(opened_on));
    assert_eq!(
        Model::update(&mut state, Intent::ResetCandles(1)).style,
        Some(opened_on)
    );
    Model::update(&mut state, Intent::Close);
    assert_eq!(state.reset, None);
    open(&mut state, ChartSettingsTab::Footprint, moved);
    Model::update(&mut state, Intent::EditStyle(opened_on, None, 2));
    assert_eq!(state.reset, Some(moved));
}
#[test]
fn the_log_waits_for_the_gesture_to_settle() {
    let mut state = pending(true, 100);
    assert_eq!(
        Model::update(&mut state, Intent::Tick(100 + STYLE_LOG_DEBOUNCE_MS / 2)).log,
        None
    );
    assert_eq!(
        Model::update(&mut state, Intent::Tick(100 + STYLE_LOG_DEBOUNCE_MS)).log,
        Some(None)
    );
    assert_eq!(
        Model::update(&mut state, Intent::Tick(100 + STYLE_LOG_DEBOUNCE_MS * 4)).log,
        None
    );
}
#[test]
fn closing_the_window_logs_without_waiting() {
    assert_eq!(
        Model::update(&mut pending(true, 100), Intent::Tick(100)).log,
        None
    );
    assert_eq!(
        Model::update(&mut pending(false, 100), Intent::Tick(100)).log,
        Some(None)
    );
    assert_eq!(
        Model::update(&mut State::default(), Intent::Tick(100)).log,
        None
    );
}
#[test]
fn closing_between_frames_still_records_the_edit() {
    let mut state = pending(true, 100);
    state.pending_preset = Some(CandlePreset::Classic);
    Model::update(&mut state, Intent::Close);
    assert!(!state.is_open());
    assert_eq!(
        Model::update(&mut state, Intent::Tick(100)).log,
        Some(Some(CandlePreset::Classic))
    );
    assert_eq!(Model::update(&mut state, Intent::Tick(100)).log, None);
}
#[test]
fn a_warning_lasts_until_the_trader_moves_on() {
    let mut state = ready();
    state.candle_refusal = Some("Reserved preset name".into());
    Model::update(&mut state, Intent::Tick(0));
    assert!(state.candle_refusal.is_some());
    Model::update(&mut state, Intent::CandleName("new name".into()));
    assert_eq!(state.candle_refusal, None);
}
#[test]
fn migration_preserves_state_and_effects() {
    let mut state = ready();
    open(&mut state, ChartSettingsTab::Candles, ChartStyle::default());
    let effects = Model::update(
        &mut state,
        Intent::SelectCandle(CandlePreset::Glass.label().into(), 20),
    );
    assert_eq!(effects.style.unwrap().candles, CandlePreset::Glass.style());
    Model::update(&mut state, Intent::CandleName("night".into()));
    let effects = Model::update(&mut state, Intent::SaveCandle);
    assert_eq!(
        effects.persistence,
        vec![Persistence::SaveCandle {
            name: "night".into(),
            style: state.style
        }]
    );
    Model::update(
        &mut state,
        Intent::CandleSaved {
            name: "night".into(),
            result: Err("read only".into()),
        },
    );
    assert_eq!(state.candle_name, "night");
    assert_eq!(state.candle_refusal.as_deref(), Some("read only"));
    Model::update(
        &mut state,
        Intent::CandleSaved {
            name: "night".into(),
            result: Ok(()),
        },
    );
    assert!(state.candle_name.is_empty());
    assert!(state.candle_refusal.is_none());
    Model::update(&mut state, Intent::FootprintName("scalp".into()));
    assert_eq!(
        Model::update(&mut state, Intent::SaveFootprint).persistence,
        vec![Persistence::SaveFootprint]
    );
    assert_eq!(state.footprint_rows, ["scalp"]);
    assert!(state.footprint_name.is_empty());
    assert_eq!(
        Model::update(&mut state, Intent::DeleteFootprint("scalp".into())).persistence,
        vec![Persistence::SaveFootprint]
    );
    assert!(state.footprint_rows.is_empty());
}
#[test]
fn unloaded_state_never_requests_persistence() {
    let mut state = State::default();
    Model::update(&mut state, Intent::CandleName("pending".into()));
    assert!(
        Model::update(&mut state, Intent::SaveCandle)
            .persistence
            .is_empty()
    );
    Model::update(&mut state, Intent::FootprintName("pending".into()));
    assert!(
        Model::update(&mut state, Intent::SaveFootprint)
            .persistence
            .is_empty()
    );
}

#[test]
fn close_clears_refusal_for_ui_and_control_lifecycles() {
    for control in [false, true] {
        let mut state = ready();
        open(&mut state, ChartSettingsTab::Candles, ChartStyle::default());
        state.candle_refusal = Some("Read only".into());
        if control {
            state.session.close(state.candles.as_mut().unwrap());
            Model::update(&mut state, Intent::SessionChanged(None));
        } else {
            Model::update(&mut state, Intent::Close);
        }
        assert_eq!(state.candle_refusal, None);
        assert!(!state.open);
    }
}
#[test]
fn unchanged_refresh_retains_preset_projection() {
    let mut state = ready();
    Model::update(&mut state, Intent::FootprintName("scalp".into()));
    Model::update(&mut state, Intent::SaveFootprint);
    let before = state.footprint_rows.as_ptr();
    let intent = Intent::Refresh {
        style: state.style,
        footprint: state.footprint.clone(),
        target: state.footprint_target.clone(),
        customized: state.footprint_customized,
    };
    assert_eq!(Model::update(&mut state, intent), Effects::default());
    assert_eq!(before, state.footprint_rows.as_ptr());
}
