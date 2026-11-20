use super::*;
use rust_decimal::Decimal;

fn row(id: u64, state: DeskState, link: DeskLink<DrawingId>) -> DeskRow<DrawingId> {
    DeskRow {
        tab_id: 7,
        pane_id: 1,
        id: InstanceRef::Script(id),
        name: "renko_poc_flip.pine".to_owned(),
        state,
        link,
        action: DeskAction::Alarm,
        status: "watching".to_owned(),
    }
}

fn asset() -> Asset {
    Asset {
        symbol: "WINZ26".to_owned(),
        bar_type: "Renko 10".to_owned(),
    }
}

fn window(state: DeskState, code: Option<&str>, history: Vec<usize>) -> StrategyWindow {
    let row = row(3, state, DeskLink::Chart);
    let mut window = StrategyWindow::open(&row, "WINZ26 · Renko 10".to_owned(), Page::Summary);
    window.detail = Some(Ok(StrategyDetail {
        row,
        description: "flip".to_owned(),
        side: Side::Sell,
        placement: ScriptPlacement::Range {
            a: Decimal::from(100),
            b: Decimal::from(110),
        },
        inputs: vec![("Min POC gap".to_owned(), "2".to_owned())],
        fires_today: 9,
        code: code.map(str::to_owned),
        history,
    }));
    window
}

#[test]
fn a_header_pauses_a_watching_asset_and_resumes_a_paused_one() {
    let watching = DeskGroup {
        asset: asset(),
        paused: false,
        rows: vec![row(1, DeskState::Watching, DeskLink::Chart)],
    };
    let header = group(&watching).header;
    assert_eq!(header.label, "WINZ26 · Renko 10");
    assert_eq!(header.count, "1");
    assert_eq!(header.toggle, Toggle::Pause);
    assert_eq!(header.send, (7, Command::PauseAsset(asset())));
    assert_eq!(header.hover, "pause every strategy on WINZ26 · Renko 10");

    let paused = DeskGroup {
        paused: true,
        ..watching
    };
    assert_eq!(
        group(&paused).header.send,
        (7, Command::ResumeAsset(asset()))
    );
}

#[test]
fn a_line_says_its_state_link_and_action_and_offers_its_toggle() {
    let drawn = DeskLink::Drawing {
        drawing: DrawingId(4),
        name: Some("zone-1".to_owned()),
    };
    let watching = line(&row(1, DeskState::Watching, drawn), "a");
    assert_eq!(watching.state, ("WATCHING", Ink::Buy));
    assert_eq!(watching.link, "zone-1");
    assert_eq!(watching.action, ("alarm", Ink::Violet));
    let pause = (7, Command::Pause(InstanceRef::Script(1)));
    assert_eq!(watching.toggle, Some((Toggle::Pause, pause)));
    assert_eq!(
        watching.remove,
        (7, Command::Remove(InstanceRef::Script(1)))
    );

    let paused = line(&row(1, DeskState::Paused, DeskLink::Chart), "a");
    assert_eq!(paused.state, ("PAUSED", Ink::Muted));
    assert_eq!(paused.link, "chart");
    let resume = (7, Command::Resume(InstanceRef::Script(1)));
    assert_eq!(paused.toggle, Some((Toggle::Resume, resume)));

    let reason = "the chart closed".to_owned();
    let ended = line(&row(1, DeskState::Ended { reason }, DeskLink::Chart), "a");
    assert_eq!(ended.state, ("ENDED", Ink::Sell));
    assert_eq!(ended.toggle, None, "an ended instance arms from its window");

    let unnamed = DeskLink::Drawing {
        drawing: DrawingId(4),
        name: None,
    };
    assert_eq!(link(&unnamed), "drawing 4");
}

#[test]
fn a_page_word_opens_that_page_and_any_other_the_summary() {
    assert_eq!(Page::parse(" code "), Page::Code);
    assert_eq!(Page::parse("History"), Page::History);
    assert_eq!(Page::parse("nonsense"), Page::Summary);
}

#[test]
fn the_summary_lists_where_action_inputs_asset_and_fires_today() {
    let window = window(DeskState::Watching, Some("//@version=6"), vec![]);
    assert_eq!(window.title(), "renko_poc_flip.pine");
    assert!(window.readable());
    assert_eq!(window.description(), "flip");
    assert_eq!(window.code(), Some("//@version=6"));
    assert_eq!(window.save_name, "renko_poc_flip copy");
    let words = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    assert_eq!(
        window.summary(),
        [
            words("Where", "range 100–110"),
            words("Action", "alarm · sell"),
            words("Min POC gap", "2"),
            words("Asset and bar", "WINZ26 · Renko 10"),
            words("Fires today", "9"),
        ]
    );
}

#[test]
fn history_places_each_fire_along_the_loaded_bars() {
    let fired = window(DeskState::Watching, None, vec![2, 5, 10]);
    let (caption, marks) = fired.history();
    assert_eq!(caption, "3 fires on the loaded bars");
    assert_eq!(marks, [0.2, 0.5, 1.0]);
    let none = window(DeskState::Watching, None, vec![]);
    assert_eq!(none.history().1, Vec::<f32>::new());
}

#[test]
fn only_an_ended_instance_arms_and_only_a_script_saves() {
    let watching = window(DeskState::Watching, None, vec![]);
    assert!(!watching.arms());
    assert!(!watching.saves(), "a drawn preset has no script");
    let reason = "stopped".to_owned();
    let ended = window(DeskState::Ended { reason }, Some("src"), vec![]);
    assert!(ended.arms());
    assert!(ended.saves());
    let id = InstanceRef::Script(3);
    assert_eq!(ended.arm(), (7, Command::Arm(id)));
    assert_eq!(ended.duplicate(), (7, Command::Duplicate(id)));
    let name = "renko_poc_flip copy".to_owned();
    let save = Command::SaveAs { instance: id, name };
    assert_eq!(ended.save_as(), (7, save));

    let mut forgotten = ended;
    forgotten.forget();
    assert!(forgotten.detail().is_none() && !forgotten.arms());
    forgotten.detail = Some(Err("gone".to_owned()));
    assert!(!forgotten.readable());
    assert_eq!(forgotten.description(), "gone");
    assert_eq!(forgotten.title(), "Strategy");
}

#[test]
fn a_save_toasts_its_file_and_a_refusal_its_reason() {
    let result = |saved_as: Option<&str>| CommandResult {
        capability: "strategy.desk.save_as".to_owned(),
        tab_id: None,
        changed: 1,
        created: None,
        saved_as: saved_as.map(str::to_owned),
    };
    assert_eq!(
        toast(Ok(result(Some("flip copy.pine")))),
        Some("saved to the library as flip copy.pine".to_owned())
    );
    assert_eq!(toast(Ok(result(None))), None);
    let refused = ControlError::invalid_request("the strategy has ended");
    assert_eq!(
        toast(Err(refused)),
        Some("the strategy has ended".to_owned())
    );
}
