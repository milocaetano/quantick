//! The switch prompt's question and answers, and the menu standing the
//! window's keys down while it is up.
use super::*;
use crate::workspace_menu::WorkspaceMenuState;

fn question(tab: u64, target: &str) -> SwitchQuestion {
    SwitchQuestion {
        tab,
        symbol: "WINZ26".to_owned(),
        workspace: "Scalp".to_owned(),
        target: target.to_owned(),
        changes: vec!["RSI".to_owned()],
        followers: Vec::new(),
    }
}

#[test]
fn save_and_switch_without_saving_carry_the_asked_switch() {
    for (answer, save) in [
        (SwitchAnswer::Save, true),
        (SwitchAnswer::SwitchWithoutSaving, false),
    ] {
        let mut prompt = SwitchPrompt::default();
        prompt.ask(question(3, "Swing"));
        let step = prompt.answer(answer);
        let (workspace, target) = ("Scalp".to_owned(), "Swing".to_owned());
        let asked = SwitchStep {
            tab: 3,
            workspace,
            target,
            save,
        };
        assert_eq!(step, Some(asked));
        assert_eq!(prompt.question(), None, "answered once");
    }
}

#[test]
fn cancel_changes_nothing_and_takes_the_question_down() {
    let mut prompt = SwitchPrompt::default();
    prompt.ask(question(3, "Swing"));
    assert_eq!(prompt.answer(SwitchAnswer::Cancel), None);
    assert_eq!(prompt.question(), None);
    assert_eq!(prompt.answer(SwitchAnswer::Save), None, "nothing up");
}

#[test]
fn a_later_switch_replaces_the_question_up() {
    let mut prompt = SwitchPrompt::default();
    prompt.ask(question(3, "Swing"));
    prompt.ask(question(3, "Tape"));
    let step = prompt.answer(SwitchAnswer::SwitchWithoutSaving);
    assert_eq!(step.map(|step| step.target), Some("Tape".to_owned()));
}

/// A switch question up stands the window's keys down as a close's does.
#[test]
fn a_switch_question_up_is_asking() {
    let mut state = WorkspaceMenuState::default();
    assert!(!state.asking());
    state.switch.ask(SwitchQuestion {
        tab: 7,
        symbol: "WINZ26".to_owned(),
        workspace: "Scalp".to_owned(),
        target: "Swing".to_owned(),
        changes: Vec::new(),
        followers: Vec::new(),
    });
    assert!(state.asking());
    state.switch.answer(SwitchAnswer::Cancel);
    assert!(!state.asking());
}
