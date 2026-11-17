//! The switch that asks first: a tab whose workspace switch would drop
//! unsaved edits asks before it moves, as a close does.
//!
//! The prompt holds the one question up and nothing else. Whether a switch
//! drops edits is the book's word (`WorkspaceBook::switch_drops`, the rule
//! the switch itself drops by), and carrying out the answer is the caller's.
//! A switch that drops nothing never asks.

/// The trader's answer to the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchAnswer {
    /// Write the tab's edits into its workspace, then switch.
    Save,
    /// Switch and let the edits go.
    SwitchWithoutSaving,
    /// Stay: nothing changes.
    Cancel,
}

impl SwitchAnswer {
    /// The prompt's buttons, left to right; Esc is the first, Enter the last.
    pub const BUTTONS: [Self; 3] = [Self::Cancel, Self::SwitchWithoutSaving, Self::Save];
}

/// What switching one tab would lose, as its question names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchQuestion {
    pub tab: u64,
    pub symbol: String,
    /// The workspace the tab is on, which a save writes.
    pub workspace: String,
    /// The workspace the switch puts the tab on.
    pub target: String,
    /// What changed since the workspace was saved.
    pub changes: Vec<String>,
    /// The clean tabs on the same workspace a save would also change.
    pub followers: Vec<String>,
}

/// What an answer leaves the caller to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchStep {
    pub tab: u64,
    /// The workspace the question named: a tab no longer on it makes the
    /// answer stale, and the caller carries out nothing.
    pub workspace: String,
    pub target: String,
    /// Write the edits into the tab's workspace before it moves.
    pub save: bool,
}

/// The switch waiting for an answer, across frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SwitchPrompt {
    asking: Option<SwitchQuestion>,
}

impl SwitchPrompt {
    /// Put `question` up in place of any still up: the last switch asked
    /// for is the one the trader answers.
    pub fn ask(&mut self, question: SwitchQuestion) {
        self.asking = Some(question);
    }

    /// The question waiting for an answer.
    #[must_use]
    pub fn question(&self) -> Option<&SwitchQuestion> {
        self.asking.as_ref()
    }

    /// Answer the question up: the switch to carry out, or `None` for
    /// Cancel, which changes nothing, or when no question is up.
    pub fn answer(&mut self, answer: SwitchAnswer) -> Option<SwitchStep> {
        let asked = self.asking.take()?;
        let save = match answer {
            SwitchAnswer::Cancel => return None,
            SwitchAnswer::Save => true,
            SwitchAnswer::SwitchWithoutSaving => false,
        };
        Some(SwitchStep {
            tab: asked.tab,
            workspace: asked.workspace,
            target: asked.target,
            save,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
